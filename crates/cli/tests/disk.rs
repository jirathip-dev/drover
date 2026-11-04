//! #164: build folders freed when a lane finishes, the worktree budget (DISK_BUDGET), the hourly clean, leftovers
//! auto-saved to wip/, orphan folders.
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, read, Event};
use serde_json::json;
use std::path::{Path, PathBuf};

const HEAD: &str = "1111111111111111111111111111111111111111";
const NOW: u64 = 1_000_000;

fn ev(c: &herdsman::Ctx, ts: u64, issue: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(issue), kind, d)).unwrap();
}

fn events(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

/// `<wt>/<dir>/deep/blob` of `bytes`; the build folder.
fn build(wt: &Path, dir: &str, bytes: usize) -> PathBuf {
    let p = wt.join(dir).join("deep");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("blob"), vec![0u8; bytes]).unwrap();
    wt.join(dir)
}

/// A lane's spawn event and its worktree dir under the root.
fn lane(c: &herdsman::Ctx, ts: u64, issue: u64, role: &str, name: &str) -> PathBuf {
    let w = c.worktrees.join("sendmeter").join(name);
    std::fs::create_dir_all(&w).unwrap();
    let agent = format!("sendmeter-{role}-{issue}");
    let d = json!({"role": role, "agent": agent, "worktree": w, "pane": format!("{name}:p1"), "workspace": name});
    ev(c, ts, issue, "spawn", d);
    w
}

fn verdict(c: &herdsman::Ctx, r: &Fake, v: &str) -> String {
    let f = c.state.with_file_name("verdict.md");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, format!("{v}\n")).unwrap();
    let args = [
        "verdict",
        "sendmeter",
        "12",
        v,
        "--sha",
        HEAD,
        "--file",
        f.to_str().unwrap(),
    ];
    let (code, out) = run(c, r, &args);
    assert_eq!(code, 0, "{out}");
    out
}

/// The reviewer's build folders go at its verdict, even when its dirty worktree stays.
#[test]
fn a_reviewer_verdict_frees_its_build_folders() {
    let c = ctx(&tmpdir("disk-rev"), CONFIG);
    let imp = lane(&c, 10, 12, "impl", "issue-12");
    let rev = lane(&c, 20, 12, "rev", "rev-12-r1");
    let (rt, it) = (build(&rev, "target", 100), build(&imp, "target", 100));
    let r = Fake::default()
        .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-12", HEAD)
        .reply(&format!("git -C {} status", rev.display()), " M src/x.rs\n")
        .reply("git -C", "")
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr", "{}");
    verdict(&c, &r, "FAIL");
    assert!(
        !rt.exists() && rev.exists(),
        "the reviewer's target goes, its dirty worktree stays"
    );
    assert!(it.exists(), "a FAIL leaves the builder's (the fix round builds there)");
    assert_eq!(events(&c, "lane-kept").len(), 1);
}

/// A PASS at the head frees the builder's build folders; never while it is working; a tracked one stays.
#[test]
fn a_builder_with_a_pass_frees_its_build_folders() {
    for (status, freed) in [("idle", true), ("working", false)] {
        let c = ctx(&tmpdir("disk-impl"), CONFIG);
        let imp = lane(&c, 10, 12, "impl", "issue-12");
        let (nm, dist) = (build(&imp, "node_modules", 100), build(&imp, "dist", 100));
        let list = agents(&[("sendmeter-impl-12", status, "/x", "issue-12:p1")]);
        let r = Fake::default()
            .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-12", HEAD)
            .once(&format!("git -C {} check-ignore -q dist", imp.display()), 1, "")
            .reply("git -C", "")
            .reply("herdr agent list", &list);
        verdict(&c, &r, "PASS");
        assert_eq!(!nm.exists(), freed, "{status}");
        assert!(dist.exists(), "not git-ignored: stays");
    }
}

/// Over `worktree_budget_gb`: the idlest lane's build folders go first, a working one's never; still over → the
/// stamp makes spawn refuse DISK_BUDGET, and drive's pass stops starting.
#[test]
fn over_budget_frees_the_idlest_lane_first_then_disk_budget() {
    // 2.5 blocks of budget (`du` counts 4096-byte blocks).
    let cfg = |gb: f64| format!("{CONFIG}worktree_budget_gb = {gb}\n");
    let d = tmpdir("disk-budget");
    let c = ctx(&d, &cfg(10240.0 / 1_073_741_824.0));
    let (old, new, busy) = (
        lane(&c, 10, 7, "impl", "issue-7"),
        lane(&c, 20, 8, "impl", "issue-8"),
        lane(&c, 5, 9, "impl", "issue-9"),
    );
    ev(&c, 30, 7, "verdict", json!({"verdict": "PASS", "sha": HEAD})); // #7's last activity: 30
    let (o, n, b) = (
        build(&old, "target", 4096),
        build(&new, "target", 4096),
        build(&busy, "target", 4096),
    );
    let r = Fake::default().reply("git -C", "").reply(
        "herdr agent list",
        &agents(&[("sendmeter-impl-9", "working", "/x", "p")]),
    );
    let run_budget = |c: &herdsman::Ctx| {
        let mut out = vec![];
        herdsman::check::budget(c, &Config::load(&c.config).unwrap(), &r, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };
    let out = run_budget(&c);
    assert!(b.exists(), "a working lane is never freed, idlest or not");
    assert!(
        !n.exists() && o.exists(),
        "#8 (activity 20) before #7 (30); then under budget: {out}"
    );
    let freed = events(&c, "budget-freed");
    assert_eq!(freed.len(), 1);
    assert_eq!(
        (freed[0].str("agent"), freed[0].data["bytes"].as_u64()),
        (Some("sendmeter-impl-8"), Some(4096))
    );
    let stamp = || std::fs::read_to_string(c.state.with_file_name("worktrees.du")).unwrap();
    assert_eq!(stamp(), format!("8192 {}", c.now));

    // Half a block: #7 goes too, the working #9 keeps 4096 → still over.
    let c = ctx(&d, &cfg(2048.0 / 1_073_741_824.0));
    run_budget(&c);
    assert!(!o.exists() && b.exists());
    assert_eq!(stamp(), format!("4096 {}", c.now));
    let r = Fake::default().reply("herdr agent list", NO_AGENTS).reply("git -C", "");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "1", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.starts_with("REFUSED DISK_BUDGET: worktrees 0.0 GB > 0.0000019"),
        "{out}"
    );
    assert!(!r.called("herdr worktree"), "nothing created");
}

/// drive walks the worktrees first: over budget with nothing to free, the start is refused and the pass stops.
#[test]
fn drive_refuses_starts_over_budget() {
    let c = ctx(
        &tmpdir("disk-drive"),
        &format!(
            "{}worktree_budget_gb = 0.000001\n",
            CONFIG.replace(
                "integration = \"staging\"",
                "integration = \"staging\"\ndriver = \"herdsman\""
            )
        ),
    );
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "1", "2"]).0, 0);
    build(&c.worktrees.join("old"), "x", 5000);
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("git -C", "")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#);
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("sendmeter #1 start: REFUSED DISK_BUDGET: worktrees"),
        "{out}"
    );
    assert!(!out.contains("#2 start"), "the pass stops: {out}");
}

/// The tick's `clean --yes` sweep runs once an hour (a merged lane's worktree goes), not only after a merge.
#[test]
fn the_hourly_sweep_runs_once_per_hour() {
    // Paused: drive's own after-merge `clean` (#155: every unpaused fleet gets a pass) would sweep too.
    let cfg = CONFIG.replace("integration = \"staging\"", "integration = \"staging\"\npaused = true");
    let mut c = ctx(&tmpdir("disk-hourly"), &cfg);
    let merged = |c: &herdsman::Ctx, n: u64| {
        ev(
            c,
            50,
            n,
            "merged",
            json!({"pr": "1", "merge_sha": "m", "into": "staging"}),
        )
    };
    let first = lane(&c, 10, 12, "impl", "issue-12");
    merged(&c, 12);
    let ahead = |n: &str| format!("git -C {} rev-list", c.worktrees.join("sendmeter").join(n).display());
    let r = || {
        Fake::default()
            .reply("herdr status", "version: 1\nversion: 1\n")
            .reply("herdr agent list", NO_AGENTS)
            .reply(&ahead("issue-12"), "0\n")
            .reply(&ahead("issue-13"), "0\n")
            .reply("git -C", "")
            .reply("herdr", "{}")
    };
    let tick = |c: &herdsman::Ctx| {
        let r = r();
        let (code, out) = run(c, &r, &["tick"]);
        assert_eq!(code, 0, "{out}");
        (r.count("herdr worktree remove"), out)
    };
    let (n, out) = tick(&c);
    assert_eq!(n, 1, "{out}");
    std::fs::remove_dir_all(first).unwrap(); // what herdr's removal does
    lane(&c, 60, 13, "impl", "issue-13");
    merged(&c, 13);
    c.now = NOW + 3599;
    assert_eq!(tick(&c).0, 0, "within the hour: no sweep");
    c.now = NOW + 3600;
    assert_eq!(tick(&c).0, 1, "an hour on: swept");
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap()
}

/// Real git; herdr and `date` are scripted (no agents, workspace already gone).
struct GitHerdr(Fake);

impl herdsman::runner::Runner for GitHerdr {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        if prog == "git" {
            return herdsman::runner::Real.run(prog, args);
        }
        self.0.run(prog, args)
    }
}

/// A merged lane's unsaved work: a clean scan → committed, pushed to `wip/`, the remote sha read back, removed;
/// a secret → kept, `wip-blocked` once, and it goes to the phone.
#[test]
fn leftovers_are_saved_to_wip_or_kept_on_a_secret() {
    let d = tmpdir("disk-wip");
    let (origin, local) = (d.join("origin.git"), d.join("local"));
    git(&d, &["init", "-q", "--bare", origin.to_str().unwrap()]);
    git(&d, &["clone", "-q", origin.to_str().unwrap(), local.to_str().unwrap()]);
    git(&local, &["commit", "-q", "--allow-empty", "-m", "init"]);
    git(&local, &["push", "-q", "origin", "HEAD"]);
    let c = ctx(
        &d,
        &CONFIG.replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap()),
    );
    let wt = |n: u64, file: &str, body: &str| {
        let p = c.worktrees.join("sendmeter").join(format!("issue-{n}"));
        git(
            &local,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                &format!("issue-{n}"),
                p.to_str().unwrap(),
            ],
        );
        std::fs::write(p.join(".brief.md"), "brief").unwrap();
        std::fs::write(p.join(file), body).unwrap();
        let d = json!({"role": "impl", "agent": format!("impl-{n}"), "worktree": p, "pane": format!("w{n}:p1"), "workspace": format!("w{n}")});
        ev(&c, 10, n, "spawn", d);
        ev(
            &c,
            50,
            n,
            "merged",
            json!({"pr": "1", "merge_sha": "m", "into": "staging"}),
        );
        p
    };
    let saved = wt(12, "notes.txt", "work\n");
    git(&saved, &["commit", "-q", "--allow-empty", "-m", "unpushed"]);
    let secret = wt(13, "env.txt", concat!("TOKEN=gh", "p_abcdefghijklmnopqrstu1234\n"));
    let r = || {
        GitHerdr(
            Fake::default()
                .reply("herdr agent list", NO_AGENTS)
                .reply("date -r", "20261102-0720\n"),
        )
    };
    let (code, dry) = run(&c, &r(), &["clean"]);
    assert_eq!(code, 0, "{dry}");
    assert!(
        dry.contains(&format!(
            "would save {} to wip/ (1 uncommitted change(s)), then remove it",
            saved.display()
        )),
        "{dry}"
    );
    assert!(
        events(&c, "wip-saved").is_empty() && saved.exists(),
        "a dry run saves nothing"
    );

    let (code, out) = run(&c, &r(), &["clean", "--yes"]);
    assert_eq!(code, 0, "{out}");
    let w = "wip/impl-12-20261102-0720";
    let remote = git(&local, &["ls-remote", "origin", &format!("refs/heads/{w}")]);
    let sha = remote.split_whitespace().next().expect("pushed");
    assert_eq!(git(&origin, &["show", &format!("{sha}:notes.txt")]), "work\n");
    assert_eq!(
        git(&origin, &["log", "-1", "--format=%s", sha]).trim(),
        "wip: auto-saved by herdsman"
    );
    assert!(
        git(&origin, &["ls-tree", "--name-only", sha])
            .lines()
            .all(|f| f != ".brief.md"),
        "scratch not saved"
    );
    let ev = &events(&c, "wip-saved")[0];
    assert_eq!(
        (ev.str("branch"), ev.str("sha"), ev.issue),
        (Some(w), Some(sha), Some(12))
    );
    assert!(
        out.contains(&format!("saved {} to {w}", saved.display())) && !saved.exists(),
        "{out}"
    );

    assert!(secret.join("env.txt").exists(), "a secret keeps the worktree: {out}");
    let keep = format!(
        "keep {}: unsaved work has a secret (env.txt:1: TOKEN=•••)",
        secret.display()
    );
    assert!(out.contains(&keep), "{out}");
    assert!(!out.contains("ghp_abc"), "the secret is never printed");
    assert!(
        !git(&local, &["ls-remote", "origin"]).contains("impl-13"),
        "nothing pushed"
    );
    assert_eq!(
        git(&secret, &["status", "--porcelain"]),
        "?? .brief.md\n?? env.txt\n",
        "index restored"
    );
    assert_eq!(events(&c, "wip-blocked").len(), 1);
    run(&c, &r(), &["clean", "--yes"]);
    assert_eq!(events(&c, "wip-blocked").len(), 1, "once, not every sweep");
    assert!(
        herdsman::config::WEBPUSH_ON.contains(&"wip-blocked"),
        "pushed to the phone"
    );
}

/// Folders under the worktree root of no fleet are listed by `clean`, removed only with `--orphans --yes`.
#[test]
fn orphans_are_listed_and_removed_only_with_the_flag() {
    let c = ctx(&tmpdir("disk-orphans"), CONFIG);
    let ours = c.worktrees.join("sendmeter/issue-1");
    let (old, empty) = (c.worktrees.join("oldproj"), c.worktrees.join("retired"));
    for p in [&ours, &empty] {
        std::fs::create_dir_all(p).unwrap();
    }
    build(&old, "issue-3", 10);
    let r = || Fake::default().reply("herdr agent list", NO_AGENTS);
    for args in [&["clean"][..], &["clean", "--yes"], &["clean", "--orphans"]] {
        let (code, out) = run(&c, &r(), args);
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains(&format!("orphan {} 0.0 GB\n", old.display())),
            "{args:?}: {out}"
        );
        assert!(
            out.contains(&format!("orphan {} 0.0 GB\n", empty.display())),
            "{args:?}: {out}"
        );
        assert!(
            !out.contains(&format!("orphan {}", c.worktrees.join("sendmeter").display())),
            "{out}"
        );
        assert!(old.exists() && empty.exists(), "{args:?} removes nothing");
    }
    let (code, out) = run(&c, &r(), &["clean", "--orphans", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(!old.exists() && !empty.exists() && ours.exists(), "{out}");
    assert!(out.contains(&format!("removed orphan {}", old.display())), "{out}");
}

/// The Host tab's [remove orphans]: shown with a confirm when there are orphans; only a paired device may POST it.
#[test]
fn the_host_tab_removes_orphans_for_a_paired_device_only() {
    const ADDR: &str = "127.0.0.1:7777";
    let c = ctx(&tmpdir("disk-orphans-page"), CONFIG);
    let old = c.worktrees.join("oldproj");
    std::fs::create_dir_all(&old).unwrap();
    let r = Fake::default().reply("herdr agent list", NO_AGENTS);
    let s = herdsman::serve::Server::new(&c, &r, Some(ADDR.into()));
    let page = host_tab(&s, ADDR, NOW);
    assert!(
        page.contains(
            "<button data-confirm=\"Remove 1 orphan folder under the worktree root?\">remove orphans</button>"
        ),
        "{page}"
    );
    let i = page.find("name=mtime value=").unwrap() + 17;
    let mt: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let body = format!("op=orphans&fleet=&mtime={mt}");
    let post = |extra: &str| {
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&req, NOW)).into_owned()
    };
    assert!(
        post("").starts_with("HTTP/1.1 401") && old.exists(),
        "unpaired cannot remove"
    );
    let cookie = pair(&s, &c, "", NOW);
    let res = post(&cookie);
    assert!(res.contains("removed%201%20orphan"), "{res}");
    assert!(!old.exists() && !host_tab(&s, ADDR, NOW).contains("remove orphans"));
}
