use crate::access::{civil_month_day, civil_year};
use crate::config::{Config, Fleet};
use crate::events::Json;
use crate::runner::{argv, json, ok, Runner};
use crate::Ctx;
use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// One advisory against the integration head: the affected package and the version that fixes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advisory {
    pub package: String,
    pub fixed: String,
}

/// `drover deps <fleet>`.
#[derive(clap::Args)]
pub struct Args {
    pub fleet: String,
}

/// Run `drover deps`: exit 4 when the lane's prompt was not confirmed.
pub fn cli(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<i32> {
    Ok(if run(ctx, cfg, r, out, fleet)? { 0 } else { 4 })
}

fn on(f: &Fleet) -> bool {
    f.deps.as_deref() == Some("monthly") && !f.paused
}

fn titled(cfg: &Config, r: &dyn Runner, fleet: &str, (state, title, exact): (&str, &str, bool)) -> Result<Option<u64>> {
    let repo = &cfg.fleet(fleet)?.repo;
    let search = format!("\"{title}\" in:title");
    let list = json(
        r,
        "gh",
        &[
            "issue",
            "list",
            "-R",
            repo,
            "--state",
            state,
            "--search",
            &search,
            "--json",
            "number,title",
        ],
    )?;
    let hit = (list.as_array().into_iter().flatten())
        .find(|i| {
            i.at("title")
                .as_str()
                .is_some_and(|t| t == title || !exact && t.starts_with(title))
        })
        .and_then(|i| i.at("number").as_u64());
    Ok(hit)
}

/// `drover deps <fleet>`: file this month's `deps:` issue, queue it and spawn its `deps` lane, unless a `deps:` issue is open; false when the lane's prompt was not confirmed (exit 4).
pub fn run(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<bool> {
    if let Some(n) = titled(cfg, r, fleet, ("open", "deps: ", false))? {
        writeln!(out, "{fleet}: deps issue #{n} is open; no second one")?;
        return Ok(true);
    }
    let day = ctx.now / 86400;
    let title = format!("deps: {fleet} {}-{:02}", civil_year(day), civil_month_day(day).0);
    let body = include_str!("../roles/deps-issue.md").replace("{fleet}", fleet);
    crate::lesson::file(ctx, cfg, r, out, (fleet, "deps", None), (&title, &body))
}

/// `tick`'s deps runs where `deps = "monthly"` (a paused fleet waits): the update lane every 30 days, the advisory check daily.
pub fn monthly(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    for name in cfg.fleet.iter().filter(|(_, f)| on(f)).map(|x| x.0) {
        if crate::timer::due(ctx, &format!("deps-{name}"), 30 * 86400)? {
            if let Err(e) = run(ctx, cfg, r, out, name) {
                writeln!(out, "{name} deps: {e:#}")?;
            }
        }
        if crate::timer::due(ctx, &format!("advisory-{name}"), 86400)? {
            if let Err(e) = advisories(ctx, cfg, r, out, name) {
                writeln!(out, "{name} advisories: {e:#}")?;
            }
        }
    }
    Ok(())
}

/// The advisory check on `fleet`'s integration head: each advisory with no `deps: advisory <id>` issue yet gets one, queued on top with its lane.
pub fn advisories(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let (local, head) = (
        f.local_path().to_string_lossy().into_owned(),
        format!("origin/{}", f.integration),
    );
    let dir = ctx.state.with_file_name(format!("deps-{fleet}"));
    let d = dir.to_string_lossy().into_owned();
    ok(r, "git", &["-C", &local, "fetch", "-q", "origin", &f.integration])?;
    if dir.exists() {
        ok(r, "git", &["-C", &d, "checkout", "-q", "--force", "--detach", &head])?;
    } else {
        ok(
            r,
            "git",
            &["-C", &local, "worktree", "add", "-q", "--detach", &d, &head],
        )?;
    }
    for (id, a) in scan(r, &dir) {
        let title = format!("deps: advisory {id}");
        if titled(cfg, r, fleet, ("all", &title, true))?.is_some() {
            continue;
        }
        let body = include_str!("../roles/deps-advisory.md")
            .replace("{fleet}", fleet)
            .replace("{id}", &id)
            .replace("{package}", &a.package)
            .replace("{fixed}", &a.fixed)
            .replace("{integration}", &f.integration);
        if let Err(e) = crate::lesson::file(ctx, cfg, r, out, (fleet, "deps", None), (&title, &body)) {
            writeln!(out, "{fleet} {title}: {e:#}")?;
        }
    }
    Ok(())
}

/// The advisories the repo's checks report in checkout `dir`, by id: cargo-deny's for a `Cargo.lock`, `pnpm audit` or `npm audit` for their lockfile.
pub fn scan(r: &dyn Runner, dir: &Path) -> BTreeMap<String, Advisory> {
    let d = dir.to_string_lossy();
    let mut found = BTreeMap::new();
    if dir.join("Cargo.lock").exists() {
        let m = dir.join("Cargo.toml").to_string_lossy().into_owned();
        let a = argv(&["deny", "--manifest-path", &m, "--format", "json", "check", "advisories"]);
        if let Ok(o) = r.run("cargo", &a) {
            found.extend(
                o.stderr
                    .lines()
                    .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                    .filter_map(|v| rustsec(&v)),
            );
        }
    }
    let audit = match (
        dir.join("pnpm-lock.yaml").exists(),
        dir.join("package-lock.json").exists(),
    ) {
        (true, _) => Some(("pnpm", "--dir")),
        (_, true) => Some(("npm", "--prefix")),
        _ => None,
    };
    if let Some((pm, at)) = audit {
        let v = r.run(pm, &argv(&["audit", "--json", at, &d])).ok();
        let v: Value = v.and_then(|o| serde_json::from_str(&o.stdout).ok()).unwrap_or_default();
        found.extend(npm(&v));
    }
    found
}

fn rustsec(v: &Value) -> Option<(String, Advisory)> {
    let f = v.at("fields");
    let a = f.at("advisory");
    let id = a.at("id").as_str()?.to_string();
    let mut notes = f.at("notes").as_array().into_iter().flatten().filter_map(Value::as_str);
    let fixed = notes
        .find_map(|n| n.strip_prefix("Solution: "))
        .unwrap_or("none listed");
    let package = a.at("package").as_str().unwrap_or_default().into();
    Some((
        id,
        Advisory {
            package,
            fixed: fixed.into(),
        },
    ))
}

fn npm(v: &Value) -> Vec<(String, Advisory)> {
    let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let pnpm = (v.at("advisories").as_object().into_iter().flatten()).map(|(_, a)| {
        let url = s(a.at("url"));
        let id = a
            .at("github_advisory_id")
            .as_str()
            .map_or_else(|| url.rsplit('/').next().unwrap_or_default().into(), String::from);
        let fixed = s(a.at("patched_versions"));
        (
            id,
            Advisory {
                package: s(a.at("module_name")),
                fixed,
            },
        )
    });
    let npm = (v.at("vulnerabilities").as_object().into_iter().flatten()).flat_map(|(_, p)| {
        let fix = p.at("fixAvailable").at("version").as_str().map(String::from);
        let via = p.at("via").as_array().into_iter().flatten().filter(|x| x.is_object());
        via.map(move |x| {
            let id = s(x.at("url")).rsplit('/').next().unwrap_or_default().to_string();
            let fixed = fix.clone().unwrap_or_else(|| format!("outside {}", s(x.at("range"))));
            (
                id,
                Advisory {
                    package: s(x.at("name")),
                    fixed,
                },
            )
        })
    });
    pnpm.chain(npm).filter(|(id, _)| !id.is_empty()).collect()
}
