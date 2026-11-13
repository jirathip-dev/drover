use crate::config::Config;
use crate::events::{emit, read, Event};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `drover self-update [--dry-run | --pause | --resume]`.
pub fn self_update(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    mode: (bool, bool, bool),
) -> Result<i32> {
    let (dry, pause, resume) = mode;
    let events = read(&ctx.state)?;
    if pause || resume {
        let (kind, word) = [("self-update-resumed", "resumed"), ("self-update-paused", "paused")][usize::from(pause)];
        let by = std::env::var("DROVER_LANE").ok().filter(|l| !l.is_empty());
        let d = json!({"by": by.as_deref().unwrap_or("owner")});
        emit(&ctx.state, cfg, Event::new(ctx.now, "drover", None, kind, d))?;
        writeln!(out, "✓ self-update {word}")?;
        return Ok(0);
    }
    let toggle = events
        .iter()
        .rfind(|e| e.kind == "self-update-paused" || e.kind == "self-update-resumed");
    if toggle.is_some_and(|e| e.kind == "self-update-paused") {
        writeln!(out, "= self-update paused → drover self-update --resume")?;
        return Ok(0);
    }
    let Some(f) = cfg.fleet.values().find(|f| ours(f)) else {
        writeln!(out, "= self-update: no fleet for the drover repo")?;
        return Ok(0);
    };
    let su = &cfg.self_update;
    let live = std::fs::canonicalize(&ctx.exe).unwrap_or_else(|_| ctx.exe.clone().into());
    let home = ctx.state.with_file_name("update");
    let code = install(ctx, cfg, r, out, f, &events, (&su.branch, home, live, dry, false))?;
    if !su.staging_serve {
        return Ok(code);
    }
    let home = ctx.state.with_file_name("update-staging");
    let t = ("staging", home.clone(), home.join("bin/drover"), dry, true);
    Ok(code.max(install(ctx, cfg, r, out, f, &events, t)?))
}

/// #894: the control socket of the serve listening on `port` (`drover_server::handover`).
pub fn serve_sock(ctx: &Ctx, port: u16) -> PathBuf {
    ctx.state.with_file_name(format!("serve-{port}.sock"))
}

/// This repo's own fleet, by either name until the GitHub repo is renamed too (#456).
pub fn ours(f: &crate::config::Fleet) -> bool {
    ["drover", "herdsman"].contains(&f.repo_name())
}

type Target<'a> = (&'a str, PathBuf, PathBuf, bool, bool);

fn install(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    f: &crate::config::Fleet,
    events: &[Event],
    (branch, home, live, dry, staging): Target,
) -> Result<i32> {
    let local = f.local_path();
    let l = local.to_string_lossy();
    let git = |a: &[&str]| ok(r, "git", &[&["-C", &*l], a].concat());
    let what = if staging { "staging serve: " } else { "" };
    git(&["fetch", "-q", "origin", branch])?;
    let sha = git(&["rev-parse", &format!("origin/{branch}")])?;
    let sha = sha.trim();
    if std::fs::read_to_string(home.join("sha")).is_ok_and(|s| s.trim() == sha) {
        writeln!(out, "= {what}up to date @ {sha:.7}")?;
        return Ok(0);
    }
    let (ci, checks) = crate::land::suite(r, f, sha);
    if ci != "green" {
        let first = crate::gh::not_green(&checks).map_or(String::new(), |c| format!(": {c}"));
        writeln!(out, "= {what}{branch} @ {sha:.7}: CI {ci}{first}; not installed")?;
        return Ok(0);
    }
    let new = PathBuf::from(format!("{}.new", live.display()));
    if dry {
        let then = if staging {
            format!(
                "would (re)start the read-only staging serve on {}",
                cfg.self_update.staging_addr
            )
        } else {
            "would run profiles sync and hand serve over (a restart if no serve answers)".into()
        };
        let plan = [
            format!(
                "would build {sha:.7} ({branch}) in {} (CI green)",
                home.join("src").display()
            ),
            format!(
                "would sign it \"drover-local\", run its doctor --config-only and --tokens, then swap it in at {}",
                live.display()
            ),
            then,
        ];
        writeln!(out, "{}", plan.join("\n"))?;
        return Ok(0);
    }
    std::fs::create_dir_all(live.parent().unwrap_or(&live))?;
    let staged = stage(r, &local, &home, sha, &new).and_then(|()| gate(r, &home.join("src"), &new));
    let res = staged.and_then(|()| {
        std::fs::rename(&new, &live).with_context(|| format!("swap into {}", live.display()))?;
        std::fs::write(home.join("sha"), sha)?;
        if staging {
            return staging_serve(ctx, cfg, r, &live).context("installed, then the staging serve");
        }
        let synced = ok(r, &live.to_string_lossy(), &["profiles", "sync"]).context("installed, then profiles sync");
        synced.and_then(|_| hand_over(ctx, cfg, r).context("installed, then the serve restart"))
    });
    let Err(e) = res else {
        let d = json!({"sha": sha, "branch": branch});
        emit(&ctx.state, cfg, Event::new(ctx.now, "drover", None, "self-updated", d))?;
        let synced = if staging { "" } else { "profiles synced, " };
        let done = res.unwrap_or_default();
        writeln!(out, "✓ {what}installed {sha:.7} at {}; {synced}{done}", live.display())?;
        return Ok(0);
    };
    let _ = std::fs::remove_file(&new);
    let why = format!("{sha:.7}: {e:#}");
    let kept = if format!("{e:#}").starts_with("installed, then") {
        "now live at"
    } else {
        "kept"
    };
    writeln!(out, "✗ self-update {why}; {kept} {}", live.display())?;
    if !events
        .iter()
        .any(|e| e.kind == "self-update-failed" && e.str("key") == Some(sha))
    {
        let d = json!({"key": sha, "reason": format!("self-update {why}")});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, "drover", None, "self-update-failed", d),
        )?;
    }
    Ok(1)
}

fn hand_over(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Result<String> {
    let asked = || -> Result<String> {
        let port = cfg
            .serve
            .addr()
            .rsplit_once(':')
            .and_then(|(_, p)| p.parse().ok())
            .unwrap_or(7777);
        let mut c = std::os::unix::net::UnixStream::connect(serve_sock(ctx, port))?;
        c.set_read_timeout(Some(std::time::Duration::from_secs(cfg.limits.serve_handover_secs)))?;
        writeln!(c, "handover")?;
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(c), &mut line)?;
        let pid = line
            .trim()
            .strip_prefix("ok ")
            .with_context(|| format!("serve: {}", line.trim()))?;
        Ok(format!("serve handed over (bridge pid {pid})"))
    };
    asked().or_else(|_| {
        let label = cfg.serve.launchd_label.as_deref().unwrap_or("dev.drover.serve");
        crate::timer::restart(ctx, r, label).map(|_| "serve restarted".into())
    })
}

fn staging_serve(ctx: &Ctx, cfg: &Config, r: &dyn Runner, live: &Path) -> Result<String> {
    let at = Ctx {
        exe: live.to_string_lossy().into(),
        ..ctx.clone()
    };
    let addr = &cfg.self_update.staging_addr;
    let args = ["serve", "--read-only", "--addr", addr];
    let job = crate::timer::install(&at, cfg, r, crate::timer::STAGING, &args, true);
    if job.starts_with('✗') {
        bail!("{job}");
    }
    if job.starts_with("= already") {
        crate::timer::restart(ctx, r, crate::timer::STAGING)?;
    }
    Ok(format!("staging serve on {addr}"))
}

/// #708: `! self-update — paused since <time> → …` while the last toggle is a pause (doctor and the board).
pub fn paused_line(r: &dyn Runner, events: &[Event]) -> Option<String> {
    let e = events
        .iter()
        .rfind(|e| e.kind == "self-update-paused" || e.kind == "self-update-resumed")
        .filter(|e| e.kind == "self-update-paused")?;
    let at = crate::usage::date(r, e.ts, "+%b %-d %H:%M");
    Some(format!(
        "! self-update — paused since {at} → drover self-update --resume"
    ))
}

/// Check `sha` out in drover's own worktree (`<home>/src`), build it there and copy the binary to `new`.
fn stage(r: &dyn Runner, local: &Path, home: &Path, sha: &str, new: &Path) -> Result<()> {
    let (src, target) = (home.join("src"), home.join("target"));
    let (s, t) = (src.to_string_lossy(), target.to_string_lossy());
    std::fs::create_dir_all(home)?;
    if src.join(".git").exists() {
        ok(r, "git", &["-C", &s, "checkout", "-q", "--detach", sha]).context("checkout")?;
    } else {
        let l = local.to_string_lossy();
        ok(r, "git", &["-C", &l, "worktree", "add", "-q", "--detach", &s, sha]).context("checkout")?;
    }
    let manifest = src.join("Cargo.toml");
    let build = ["build", "--release", "--locked", "-p", "drover", "--manifest-path"];
    ok(
        r,
        "cargo",
        &[&build[..], &[&manifest.to_string_lossy(), "--target-dir", &t]].concat(),
    )
    .context("build")?;
    std::fs::copy(target.join("release/drover"), new).context("copy the build")?;
    Ok(())
}

fn gate(r: &dyn Runner, src: &Path, new: &Path) -> Result<()> {
    let n = new.to_string_lossy();
    if ok(r, "which", &["codesign"]).is_ok() {
        let script = src.join("scripts/signing-identity.sh");
        let o = r.run("sh", &crate::runner::argv(&[&script.to_string_lossy(), "sign", &n]))?;
        match o.code {
            0 => {}
            2 => bail!("sign: no \"drover-local\" identity → sh scripts/signing-identity.sh, once"),
            _ => bail!("sign: {}", format!("{}{}", o.stdout, o.stderr).trim()),
        }
    }
    let o = r.run(&n, &crate::runner::argv(&["doctor", "--config-only"]))?;
    if o.code != 0 {
        bail!("doctor --config-only on the live config: {}", o.stdout.trim());
    }
    Ok(())
}
