use crate::config::{expand, home, Fleet};
use crate::providers;
use crate::spawn::Plan;
use crate::Ctx;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const READ: [&str; 9] = [
    ".cargo",
    ".rustup",
    ".local/share/mise",
    ".local/bin",
    ".gitconfig",
    ".config/git",
    ".config/gh",
    "Library/Developer",
    "Library/Keychains",
];
const WRITE: [&str; 5] = [
    ".cargo/registry",
    ".cargo/git",
    ".npm",
    "Library/Developer",
    "Library/Logs/CoreSimulator",
];
const DENY: [&str; 3] = [".ssh", ".aws", ".gnupg"];
const HOSTS: [&str; 7] = [
    "github.com",
    "*.github.com",
    "*.githubusercontent.com",
    "crates.io",
    "*.crates.io",
    "static.rust-lang.org",
    "registry.npmjs.org",
];
const SOCKETS: &str = "/private/tmp";
const MACH: [&str; 2] = ["com.apple.CoreSimulator.*", "com.apple.trustd.agent"];

/// The lane's srt rules, beside its worktree on its host (outside what the lane may write).
pub fn file(p: &Plan) -> PathBuf {
    PathBuf::from(format!("{}.srt.json", p.worktree.display()))
}

/// The lane's own temp folder, under its scratch.
pub fn tmp(p: &Plan) -> PathBuf {
    let scratch = p.env.get("DROVER_SCRATCH_TARGET").map(|s| expand(s));
    scratch
        .unwrap_or_else(|| PathBuf::from(format!("{}.scratch", p.worktree.display())))
        .join("tmp")
}

/// `start` (a `pane run` line) as `srt` running it with the lane's rules, when the lane is sandboxed.
pub fn wrap(p: &Plan, mut start: Vec<String>) -> Vec<String> {
    if let (Some(_), Some(line)) = (p.sandbox, start.get_mut(3)) {
        let tmp = format!("{}={}", providers::SANDBOX_TMP_ENV, tmp(p).display());
        let args = [tmp, "srt".into(), "--settings".into(), file(p).display().to_string()];
        *line = format!("{} {line}", crate::hosts::quote("env", &args));
    }
    start
}

/// The lane's srt settings: writes to its worktree, build and temp; reads of system, toolchains and its repo; no other home folder; network to `HOSTS` only.
pub fn settings(ctx: &Ctx, f: &Fleet, p: &Plan, local: bool) -> Value {
    let h = home();
    let at = |r: &[&str]| r.iter().map(|d| h.join(d)).collect::<Vec<_>>();
    let needs = providers::get(&p.harness).sandbox(ctx, &p.profile);
    let build = ["CARGO_TARGET_DIR", "DROVER_SCRATCH_TARGET"].into_iter();
    let build = build.filter_map(|k| Some(expand(p.env.get(k)?)));
    let repo = expand(&f.local);
    let mut write: Vec<PathBuf> = [p.worktree.clone(), repo.join(".git"), tmp(p), SOCKETS.into()].into();
    write.extend(build.chain(ctx.state.parent().map(Path::to_path_buf)));
    write.extend(at(&WRITE).into_iter().chain(needs.write));
    let mut read: Vec<PathBuf> = [repo, ctx.config.clone()].into();
    read.extend(Path::new(&ctx.exe).parent().map(Path::to_path_buf));
    read.extend(at(&READ).into_iter().chain(needs.read).chain(write.clone()));
    if local {
        write = resolved(write, false);
        read = resolved(read, true);
    }
    let deny: Vec<PathBuf> = std::iter::once(h.clone())
        .chain(at(&DENY))
        .chain([crate::secret::dir(ctx)])
        .collect();
    let hosts: Vec<&str> = HOSTS.into_iter().chain(needs.hosts).collect();
    json!({
        "filesystem": {"denyRead": deny, "allowRead": read, "allowWrite": write, "denyWrite": []},
        "network": {"allowedDomains": hosts, "deniedDomains": [], "allowMachLookup": MACH, "allowUnixSockets": [SOCKETS]},
    })
}

fn resolved(paths: Vec<PathBuf>, links: bool) -> Vec<PathBuf> {
    let inner = |d: &PathBuf| {
        let entries = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path());
        entries.filter(|e| links && e.is_symlink()).collect::<Vec<_>>()
    };
    let hops = |p: &PathBuf| {
        let up = p.ancestors().skip(1).filter(|a| links && a.is_symlink());
        up.map(Path::to_path_buf).collect::<Vec<_>>()
    };
    let targets: Vec<PathBuf> = paths
        .iter()
        .flat_map(|p| std::iter::once(p.clone()).chain(inner(p)))
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect();
    let up: Vec<PathBuf> = paths.iter().flat_map(hops).collect();
    let mut all: Vec<PathBuf> = paths.into_iter().chain(targets).chain(up).collect();
    all.sort();
    all.dedup();
    all
}
