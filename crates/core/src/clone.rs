use crate::config::Config;
use crate::events::{emit, Event};
use crate::overlap::git;
use crate::runner::Runner;
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

const BROKEN: [&str; 4] = [
    "bad object",
    "did not send all necessary objects",
    "unresolved deltas",
    "invalid index-pack output",
];

/// `git -C <dir> fetch <args>` in a fleet clone, one drover process at a time. A fetch failing on a missing or bad object, or any fetch while an earlier heal is due, heals the clone first, logs it, and fetches again.
pub fn fetch(ctx: &Ctx, r: &dyn Runner, dir: &Path, args: &[&str]) -> Result<String> {
    let _held = lock(r, dir)?;
    let key = format!("clone-heal:{}", dir.display());
    let due = crate::db::get::<String>(&ctx.state, &key).filter(|(_, e)| !e.is_empty());
    let fetch = || git(r, dir, &[&["fetch"][..], args].concat());
    let why = match (fetch(), &due) {
        (Err(e), _) if BROKEN.iter().any(|b| format!("{e:#}").contains(b)) => format!("{e:#}"),
        (_, Some((_, e))) => e.clone(),
        (res, None) => return res,
    };
    let t = crate::clock::instant();
    let healed = heal(r, dir);
    let secs = t.elapsed().as_secs();
    match &healed {
        Ok(dropped) => {
            crate::db::put(&ctx.state, &key, ctx.now, &"").ok();
            log(ctx, dir, "clone-healed", json!({"dropped": dropped, "secs": secs}));
        }
        Err(h) => {
            crate::db::put(&ctx.state, &key, ctx.now, &why).ok();
            if due.is_none() {
                log(
                    ctx,
                    dir,
                    "clone-broken",
                    json!({"error": format!("{h:#}"), "fetch": why}),
                );
            }
        }
    }
    healed.and_then(|_| fetch())
}

fn lock(r: &dyn Runner, dir: &Path) -> Result<Option<std::fs::File>> {
    let gitdir = dir.join(".git");
    if !gitdir.is_dir() {
        return Ok(None);
    }
    let f = std::fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(gitdir.join("drover-fetch.lock"))?;
    for _ in 0..crate::runner::limit("git", &["fetch".into()]).as_secs() {
        match f.try_lock() {
            Ok(()) => return Ok(Some(f)),
            Err(std::fs::TryLockError::WouldBlock) => r.sleep(1),
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
    }
    anyhow::bail!("another drover process is still fetching {}", dir.display())
}

fn broken_refs(r: &dyn Runner, dir: &Path) -> Result<Vec<String>> {
    let refs = git(
        r,
        dir,
        &[
            "for-each-ref",
            "--format=%(objectname) %(symref) %(refname)",
            "refs/remotes",
        ],
    )?;
    let missing = |l: &str| {
        let (sha, name) = l.split_once(' ')?;
        let name = name.strip_prefix(' ')?;
        git(r, dir, &["cat-file", "-e", sha]).is_err().then(|| name.to_string())
    };
    Ok(refs.lines().filter_map(missing).collect())
}

fn heal(r: &dyn Runner, dir: &Path) -> Result<Vec<String>> {
    git(r, dir, &["ls-remote", "-q", "origin", "HEAD"])?;
    let dropped = broken_refs(r, dir)?;
    for b in &dropped {
        git(r, dir, &["update-ref", "-d", b])?;
    }
    git(r, dir, &["fetch", "--refetch", "-q", "origin"])?;
    git(r, dir, &["fsck", "--connectivity-only"])?;
    Ok(dropped)
}

fn log(ctx: &Ctx, dir: &Path, kind: &str, mut d: Value) {
    let Ok(cfg) = Config::load(&ctx.config) else {
        return;
    };
    let clone = dir.display().to_string();
    let fleet = cfg.fleet.iter().find(|(_, f)| f.local_path() == dir);
    let fleet = fleet.map_or("drover", |(n, _)| n.as_str());
    if let Some(m) = d.as_object_mut() {
        m.insert("clone".into(), json!(clone));
        m.insert(
            "reason".into(),
            json!(format!(
                "{clone}: {}",
                if kind == "clone-broken" {
                    "the fetch fails and the heal failed"
                } else {
                    "healed"
                }
            )),
        );
    }
    let e = Event::new(ctx.now, fleet, None, kind, d);
    emit(&ctx.state, &cfg, if kind == "clone-broken" { e.push() } else { e }).ok();
}

/// `doctor`'s row for `name`'s clone: connectivity ok, or the first missing object and the commands that heal it.
pub fn doctor(r: &dyn Runner, name: &str, dir: &Path) -> String {
    let o = r.run(
        "git",
        &crate::runner::argv(&["-C", &dir.to_string_lossy(), "fsck", "--connectivity-only"]),
    );
    let o = o.unwrap_or_default();
    let shown = dir.display();
    if o.code == 0 {
        return format!("✓ clone {name} — {shown} connectivity ok");
    }
    let text = format!("{}{}", o.stdout, o.stderr);
    let what = text
        .lines()
        .find(|l| l.starts_with("missing "))
        .or_else(|| text.lines().next());
    let drops = broken_refs(r, dir).unwrap_or_default();
    let drops = drops.iter().map(|b| format!("git -C {shown} update-ref -d {b} && "));
    let fix = format!("{}git -C {shown} fetch --refetch origin", drops.collect::<String>());
    format!(
        "✗ clone {name} — {shown}: {} → {fix}",
        what.unwrap_or("fsck failed").trim()
    )
}

/// The clean job's sweep: each fleet clone's `tmp_pack_*` files older than a day (a killed fetch's leftovers); dry: listed only.
pub fn sweep(cfg: &Config, out: &mut dyn Write, dry: bool) -> Result<()> {
    for f in cfg.fleet.values() {
        let pack = f.local_path().join(".git/objects/pack");
        let old = |p: &std::fs::DirEntry| {
            let tmp = p.file_name().to_string_lossy().starts_with("tmp_pack_");
            let age = p
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok());
            tmp && age.is_some_and(|a| a.as_secs() > 86_400)
        };
        let Ok(all) = std::fs::read_dir(&pack) else {
            continue;
        };
        let stale: Vec<std::fs::DirEntry> = all.flatten().filter(old).collect();
        if stale.is_empty() {
            continue;
        }
        let gone = stale
            .iter()
            .filter(|p| !dry && std::fs::remove_file(p.path()).is_ok())
            .count();
        let did = if dry {
            format!("would remove {}", stale.len())
        } else {
            format!("removed {gone}")
        };
        writeln!(out, "{did} temp pack(s) a fetch left in {}", pack.display())?;
    }
    Ok(())
}
