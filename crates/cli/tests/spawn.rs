//! AC5: spawn launches the right harness with the synced profile.
mod common;
use common::*;
use herdsman::events::{append, fold, read, Event};
use herdsman::spawn::lane_port;
use serde_json::{json, Value};

/// #187: issue 12's impl lane port with nothing taken.
fn port12() -> String {
    format!("HERDSMAN_PORT={}", lane_port(None, "sendmeter/issue-12", &[]))
}

fn plan(c: &herdsman::Ctx, r: &Fake, role: &str) -> Value {
    let (code, out) = run(c, r, &["spawn", "sendmeter", "12", role, "--dry-run", "--json"]);
    assert_eq!(code, 0, "{out}");
    serde_json::from_str(&out).unwrap()
}

fn on_shed(c: &herdsman::Ctx) -> FakeShed {
    let sh = FakeShed::new();
    sh.attach(c);
    sh
}

fn shed_fake() -> Fake {
    Fake::default().reply("which shed", "/opt/shed/bin/shed\n")
}

fn requests(p: &Value) -> Vec<(String, Value)> {
    let cmds = p["commands"].as_array().unwrap();
    let shed = cmds.iter().filter(|c| c[0] == "shed" && c[2].is_string());
    shed.map(|c| {
        let params = serde_json::from_str(c[2].as_str().unwrap()).unwrap_or_default();
        (c[1].as_str().unwrap().to_string(), params)
    })
    .collect()
}

fn typed(p: &Value, head: &str) -> String {
    let sends = requests(p).into_iter().filter(|r| r.0 == "send");
    let texts = sends.map(|r| r.1["text"].as_str().unwrap().trim_end_matches('\r').to_string());
    texts.into_iter().find(|t| t.starts_with(head)).unwrap()
}

fn export(p: &Value) -> Vec<String> {
    raw_export(p).into_iter().filter(|w| !w.contains("/.build/")).collect()
}

fn raw_export(p: &Value) -> Vec<String> {
    typed(p, "export ").split_whitespace().map(String::from).collect()
}

fn launch(p: &Value) -> Vec<String> {
    let (mut out, mut w, mut quoted, mut any) = (vec![], String::new(), false, false);
    for ch in typed(p, "exec ").chars() {
        match ch {
            '\'' => (quoted, any) = (!quoted, true),
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut w));
                }
                any = false;
            }
            c => (w, any) = (w + &c.to_string(), true),
        }
    }
    out.extend(any.then_some(w));
    out.into_iter().filter(|w| !w.contains("/.build/")).collect()
}

#[test]
fn claude_role_dry_run_uses_synced_profile() {
    let d = tmpdir("spawn-claude");
    let c = ctx(&d, &CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\""));
    let sh = on_shed(&c);
    let r = shed_fake();
    let p = plan(&c, &r, "impl");
    let run = &requests(&p)[0];
    assert_eq!(run.0, "run");
    assert_eq!(
        (&run.1["name"], &run.1["labels"]["harness"]),
        (&json!("sendmeter-impl-12"), &json!("claude"))
    );
    assert_eq!(
        export(&p),
        [
            "export",
            "HERDSMAN_LANE='sendmeter:12:impl'",
            &port12(),
            // #248: no updater, telemetry or other background traffic in a Claude lane.
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1",
            "DISABLE_AUTOUPDATER=1",
            // #708: no account, so a token the pane inherited from a login shell never picks one.
            "&&",
            "unset",
            "CLAUDE_CODE_OAUTH_TOKEN"
        ]
    );
    let start = launch(&p);
    let prof = c.config.parent().unwrap().join("claude/fleet-impl");
    let (settings, role) = (prof.join("settings.json"), prof.join("role.md"));
    let expect = [
        "exec",
        "claude",
        "--dangerously-skip-permissions",
        "--settings",
        settings.to_str().unwrap(),
        "--append-system-prompt-file",
        role.to_str().unwrap(),
        "--model",
        "opus",
    ];
    assert_eq!(&start[..expect.len()], &expect);
    let kick = start.last().unwrap();
    assert!(
        kick.starts_with("Read ") && kick.ends_with("issue-12/.brief.md and execute it fully."),
        "{kick}"
    );
    assert_eq!(p["branch"], "issue-12");
    assert_eq!(p["base"], "origin/staging");
    let wait = requests(&p).pop().unwrap();
    assert_eq!(
        (wait.0.as_str(), &wait.1["name"], &wait.1["until"]),
        ("wait", &json!("sendmeter-impl-12"), &json!(["working", "blocked"]))
    );
    assert_eq!(*r.calls.borrow(), ["which shed"], "dry-run calls nothing");
    assert!(sh.methods().is_empty(), "dry-run calls nothing");
    assert!(read(&c.state).unwrap().is_empty(), "dry-run logs nothing");
}

/// #155: `profiles sync` removes a generated Claude profile no role names (the retired `fleet-orch`), nothing else.
#[test]
fn profiles_sync_removes_a_stale_generated_profile() {
    let c = ctx(&tmpdir("spawn-stale"), CONFIG);
    let dir = c.config.parent().unwrap().join("claude");
    for (p, f) in [("fleet-orch", "role.md"), ("mine", "notes.md")] {
        std::fs::create_dir_all(dir.join(p)).unwrap();
        std::fs::write(dir.join(p).join(f), "x").unwrap();
    }
    let (code, out) = run(&c, &Fake::default(), &["profiles", "sync"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!("removed stale {}", dir.join("fleet-orch").display())),
        "{out}"
    );
    assert!(!dir.join("fleet-orch").exists() && dir.join("mine/notes.md").exists());
    assert!(dir.join("fleet-impl/role.md").exists() && dir.join("fleet-rev/role.md").exists());
}

/// #577: a verify role's own profile gets verify's prompt, still with the read-only check hook.
#[test]
fn profiles_sync_gives_a_verify_profile_its_own_prompt() {
    let verify = "\n[fleet.sendmeter.role.verify]\nharness = \"claude\"\nprofile = \"fleet-verify\"\n";
    let c = ctx(&tmpdir("spawn-verify"), &format!("{CONFIG}{verify}"));
    let (code, out) = run(&c, &Fake::default(), &["profiles", "sync"]);
    assert_eq!(code, 0, "{out}");
    let dir = c.config.parent().unwrap().join("claude/fleet-verify");
    assert!(out.contains(&format!("synced {} (verify)", dir.display())), "{out}");
    let role = std::fs::read_to_string(dir.join("role.md")).unwrap();
    assert!(role.starts_with("# ROLE: VERIFIER"), "{role}");
    let s = std::fs::read_to_string(dir.join("settings.json")).unwrap();
    assert!(s.contains("hook --role rev"), "{s}");
}

/// #198: lanes add no AI co-author: Claude's attribution is off, and every lane's role text says so.
#[test]
fn profiles_turn_attribution_off_and_role_md_has_the_rule() {
    let c = ctx(&tmpdir("spawn-attr"), CONFIG);
    let (code, out) = run(&c, &Fake::default(), &["profiles", "sync"]);
    assert_eq!(code, 0, "{out}");
    let dir = c.config.parent().unwrap().join("claude");
    let s: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("fleet-impl/settings.json")).unwrap()).unwrap();
    assert_eq!(s["attribution"], serde_json::json!({"commit": "", "pr": ""}));
    assert_eq!(s["includeCoAuthoredBy"], false);
    // #248: lane transcripts are kept `[limits] transcript_days` (30, Claude's default: lanes share ~/.claude, so no lower).
    assert_eq!(s["cleanupPeriodDays"], 30);
    // #223: lanes don't inherit the user's IDE/LSP plugins (tsserver ~2 GB per lane).
    for p in ["typescript-lsp", "pyright-lsp", "swift-lsp", "vercel"] {
        assert_eq!(
            s["enabledPlugins"][format!("{p}@claude-plugins-official")],
            false,
            "{p}"
        );
    }
    let role = std::fs::read_to_string(dir.join("fleet-impl/role.md")).unwrap();
    assert!(role.contains("No AI co-author or 'generated with' trailers in commits or PR bodies"));
    assert!(role.contains("## For humans"));
    assert!(herdsman::spawn::role_prompt("rev").contains("## For humans"));
}

/// #226: a reviewer never FAILs on an older commit's trailer; the fix would be a forbidden force-push.
#[test]
fn rev_role_flags_new_commit_trailers_only() {
    let rev = herdsman::spawn::role_prompt("rev");
    assert!(rev.contains("new commits only, never force-push"), "{rev}");
    assert!(herdsman::spawn::role_prompt("impl").contains("never rewrite history or force-push"));
}

/// #320: a predicted conflict is information, not a hold; no generated prompt tells a lane to wait for a merge.
/// #429: the cap is a repo total; a lane asks only when its own PR would cross it.
#[test]
fn cap_asks_only_when_your_own_pr_crosses() {
    let d = tmpdir("help-429");
    let (_, help) = run(&ctx(&d, CONFIG), &Fake::default(), &["help", "--agent"]);
    let impl_ = herdsman::spawn::role_prompt("impl").replace("\n  ", " ");
    for t in [impl_.as_str(), help.as_str()] {
        for w in [
            "The cap in `ci/cap.toml` is a total for the repo",
            "ask only when your own PR, merged onto current integration, would put the total over `line_cap`; trim first, then ask `--kind owner`.",
            "Being near the cap, or another PR's growth, is never a reason to ask.",
            "A `STAGING_WOULD_BREAK` from the cap after another merge is a fix round: trim, or ask under the same rule.",
            // #668: one conductor-filed raise for the queue, never a lane's own edit.
            "Never edit `ci/cap.toml` (the hook refuses it",
            "`herdsman cap <fleet>`'s queue-wide view",
            "in one orchestrator-filed issue (recommended), or trim.",
        ] {
            assert!(t.contains(w), "{w}");
        }
    }
}

#[test]
fn conflict_predicted_is_not_a_hold() {
    let d = tmpdir("help-320");
    let (_, help) = run(&ctx(&d, CONFIG), &Fake::default(), &["help", "--agent"]);
    let impl_ = herdsman::spawn::role_prompt("impl");
    for t in [impl_, help.as_str()] {
        assert!(t.contains("`conflict-predicted` note means another open PR touches the same files"));
        assert!(t.contains("herdsman gives you an update round"));
    }
    assert!(impl_.contains("Don't wait for it.") && help.contains("don't wait for it"));
    for t in ["impl", "rev", "design"]
        .map(herdsman::spawn::role_prompt)
        .into_iter()
        .chain([help.as_str()])
    {
        assert!(!t.contains("to land") && !t.contains("lands."), "{t}");
    }
}

/// #103: `[fleet.<n>] env` is exported into the lane's shell after HERDSMAN_LANE, `~/` expanded.
#[test]
fn fleet_env_is_exported_into_the_lane() {
    let d = tmpdir("spawn-env");
    let cfg = CONFIG.replace(
        "[fleet.sendmeter.role.impl]",
        "[fleet.sendmeter.env]\nCARGO_TARGET_DIR = \"~/.cache/herdsman/target/sendmeter\"\nnpm_config_cache = \"/c/npm\"\n\n[fleet.sendmeter.role.impl]",
    );
    let c = ctx(&d, &cfg);
    let _sh = on_shed(&c);
    let p = plan(&c, &shed_fake(), "impl");
    let home = std::env::var("HOME").unwrap();
    assert_eq!(
        export(&p),
        [
            "export".to_string(),
            "HERDSMAN_LANE='sendmeter:12:impl'".into(),
            port12(),
            format!("CARGO_TARGET_DIR='{home}/.cache/herdsman/target/sendmeter'"),
            "npm_config_cache='/c/npm'".into(),
        ]
    );
}

/// #361: `shared_cache` is exported like `env`.
#[test]
fn shared_cache_is_exported_into_the_lane() {
    let cfg = CONFIG.replace(
        "[fleet.sendmeter.role.impl]",
        "shared_cache = { CARGO_TARGET_DIR = \"/c/cache/sendmeter/target\" }\n\n[fleet.sendmeter.role.impl]",
    );
    let c = ctx(&tmpdir("spawn-shared-cache"), &cfg);
    let _sh = on_shed(&c);
    assert_eq!(
        export(&plan(&c, &shed_fake(), "impl"))[3..],
        ["CARGO_TARGET_DIR='/c/cache/sendmeter/target'".to_string()]
    );
}

/// #630: each lane gets its own Cargo build folder, `<root>/.build/<agent>` (cloned at spawn from its fleet's warmest).
#[test]
fn each_lane_gets_its_own_cargo_target_under_its_root() {
    let other = "[fleet.other]\nrepo = \"jirathip-dev/other\"\nlocal = \"/tmp/x/other\"\nintegration = \"staging\"\n\n\
                 [fleet.other.role.impl]\nharness = \"hermes\"\nprofile = \"fleet-impl\"\n\n";
    let cfg = other.to_string() + CONFIG + "\n[hosts.mini]\nroot = \"/c\"\n";
    let c = ctx(&tmpdir("spawn-shared-target"), &cfg);
    let _sh = on_shed(&c);
    let target = |fleet: &str, n: &str| {
        let (code, out) = run(&c, &shed_fake(), &["spawn", fleet, n, "impl", "--dry-run", "--json"]);
        assert_eq!(code, 0, "{out}");
        let p: Value = serde_json::from_str(&out).unwrap();
        let w = raw_export(&p).into_iter().find(|w| w.contains("TARGET_DIR="));
        w.and_then(|w| Some(w.split_once('=')?.1.to_string()))
    };
    assert_eq!(
        target("sendmeter", "12").as_deref(),
        Some("'/c/.build/sendmeter-impl-12'")
    );
    assert_eq!(
        target("sendmeter", "13").as_deref(),
        Some("'/c/.build/sendmeter-impl-13'")
    );
    assert_eq!(target("other", "12").as_deref(), Some("'/c/.build/other-impl-12'"));
}

/// #536: each lane gets its own scratch dir, named for its agent; a reviewer's per round. #630: under its root's
/// `.build`; with no `[hosts]`, the worktree root's.
#[test]
fn each_lane_gets_a_private_scratch_dir_under_its_roots_build_folder() {
    let c = ctx(
        &tmpdir("spawn-scratch"),
        &(CONFIG.to_string() + "\n[hosts.mini]\nroot = \"/c\"\n"),
    );
    let _sh = on_shed(&c);
    let scratch = |c: &herdsman::Ctx, role: &str| {
        raw_export(&plan(c, &shed_fake(), role))
            .into_iter()
            .find(|w| w.starts_with("HERDSMAN_SCRATCH_TARGET="))
    };
    assert_eq!(
        scratch(&c, "impl").as_deref(),
        Some("HERDSMAN_SCRATCH_TARGET='/c/.build/.scratch/sendmeter-impl-12'")
    );
    assert_eq!(
        scratch(&c, "rev").as_deref(),
        Some("HERDSMAN_SCRATCH_TARGET='/c/.build/.scratch/sendmeter-rev-12-r1'")
    );
    let bare = ctx(&tmpdir("spawn-scratch-bare"), CONFIG);
    let _bare_sh = on_shed(&bare);
    let want = bare.worktrees.join(".build/.scratch/sendmeter-impl-12");
    assert_eq!(
        scratch(&bare, "impl"),
        Some(format!("HERDSMAN_SCRATCH_TARGET='{}'", want.display()))
    );
}

/// #187: the same fleet and branch always hash to the same port, inside the range; the dry run shows it.
#[test]
fn lane_port_is_stable_per_fleet_and_branch() {
    let p = lane_port(None, "sendmeter/issue-12", &[]);
    assert_eq!(p, lane_port(None, "sendmeter/issue-12", &[]));
    assert!((41000..=41999).contains(&p), "{p}");
    assert_ne!(
        p,
        lane_port(None, "other/issue-12", &[]),
        "the fleet is part of the key"
    );
    assert!((5000..=5009).contains(&lane_port(Some("5000-5009"), "a/b", &[])));
    let c = ctx(&tmpdir("spawn-port"), CONFIG);
    assert_eq!(plan(&c, &shed_fake(), "impl")["port"], p);
}

/// #187: a port a live lane holds moves to the next free one (wrapping); the lane's own earlier spawn is no collision.
#[test]
fn lane_port_collision_takes_the_next_free_one() {
    let p = lane_port(Some("7000-7001"), "f/b", &[]);
    let other = if p == 7000 { 7001 } else { 7000 };
    assert_eq!(lane_port(Some("7000-7001"), "f/b", &[p]), other);
    assert_eq!(
        lane_port(Some("7000-7001"), "f/b", &[7000, 7001]),
        p,
        "a full range keeps the hash"
    );

    let cfg = CONFIG.replace(
        "[fleet.sendmeter.role.impl]",
        "port_range = \"7000-7001\"\n\n[fleet.sendmeter.role.impl]",
    );
    let c = ctx(&tmpdir("spawn-port-taken"), &cfg);
    let mine = plan(&c, &shed_fake(), "impl")["port"].as_u64().unwrap();
    let spawn = |n: u64, branch: &str| {
        let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "branch": branch, "port": mine});
        append(&c.state, &Event::new(10, "sendmeter", Some(n), "spawn", d)).unwrap();
    };
    spawn(12, "issue-12");
    assert_eq!(plan(&c, &shed_fake(), "impl")["port"], mine, "its own lane");
    spawn(13, "issue-13");
    assert_eq!(plan(&c, &shed_fake(), "impl")["port"], 14001 - mine);
}

/// #187: `$HERDSMAN_PORT` in a fleet's env expands to the lane's port.
#[test]
fn fleet_env_expands_herdsman_port() {
    let cfg = CONFIG.replace(
        "[fleet.sendmeter.role.impl]",
        "[fleet.sendmeter.env]\nPORT = \"$HERDSMAN_PORT\"\nVITE = \"http://127.0.0.1:$HERDSMAN_PORT/\"\n\n[fleet.sendmeter.role.impl]",
    );
    let c = ctx(&tmpdir("spawn-env-port"), &cfg);
    let _sh = on_shed(&c);
    let p = lane_port(None, "sendmeter/issue-12", &[]);
    assert_eq!(
        export(&plan(&c, &shed_fake(), "impl"))[3..],
        [format!("PORT='{p}'"), format!("VITE='http://127.0.0.1:{p}/'")]
    );
}

/// #103: `clone_node_modules` needs some lockfile, and every lockfile identical in both checkouts.
#[test]
fn lockfiles_match_needs_an_identical_lockfile() {
    let d = tmpdir("lockfiles");
    let (local, wt) = (d.join("local"), d.join("wt"));
    std::fs::create_dir_all(&local).unwrap();
    std::fs::create_dir_all(&wt).unwrap();
    let m = || herdsman::spawn::lockfiles_match(&["package-lock.json", "pnpm-lock.yaml"], &local, &wt);
    assert!(!m(), "no lockfile anywhere is no match");
    std::fs::write(local.join("package-lock.json"), "a").unwrap();
    assert!(!m(), "missing in the lane");
    std::fs::write(wt.join("package-lock.json"), "a").unwrap();
    assert!(m());
    std::fs::write(wt.join("pnpm-lock.yaml"), "b").unwrap();
    assert!(!m(), "a lockfile only the lane has");
    std::fs::write(local.join("pnpm-lock.yaml"), "c").unwrap();
    assert!(!m(), "differing lockfiles");
}

/// #185: `clone_build` clones `target` on a matching `Cargo.lock` only; a failed clone is logged, never retried as a copy.
#[test]
fn clone_build_clones_on_a_lockfile_match_and_logs_failures() {
    let d = tmpdir("clone-build");
    let (local, wt) = (d.join("local"), d.join("wt"));
    std::fs::create_dir_all(local.join("target")).unwrap();
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(local.join("Cargo.lock"), "a").unwrap();
    std::fs::write(wt.join("Cargo.lock"), "b").unwrap();
    let dirs = ["target".to_string(), "node_modules".into()];
    let clone = |r: &Fake| herdsman::spawn::clone_build(r, &dirs, &local, &wt);
    let r = Fake::default().reply("cp ", "");
    assert_eq!(clone(&r), None, "mismatch: no clone");
    assert!(!r.called("cp "));
    std::fs::write(wt.join("Cargo.lock"), "a").unwrap();
    std::fs::create_dir(wt.join(".target.clone")).unwrap(); // what the faked `cp` would have made
    let d = clone(&r).unwrap();
    assert_eq!((d["dirs"].clone(), d["errors"].clone()), (json!(["target"]), json!([])));
    assert!(wt.join("target").is_dir() && !wt.join(".target.clone").exists());
    let calls = r.calls.borrow().clone();
    assert_eq!(calls.len(), 1, "node_modules has no lockfile: {calls:?}");
    let (src, tmp) = (local.join("target"), wt.join(".target.clone"));
    assert!(calls[0].ends_with(&format!(" {} {}", src.display(), tmp.display())));
    let flag = if cfg!(target_os = "macos") {
        "cp -cR "
    } else {
        "cp -R --reflink=always "
    };
    assert!(calls[0].starts_with(flag), "{}", calls[0]);
    assert_eq!(clone(&r), None, "the lane has its own target: never cloned over");
    std::fs::remove_dir(wt.join("target")).unwrap();
    let d = clone(&Fake::default()).unwrap();
    assert_eq!(d["dirs"], json!([]));
    assert!(d["errors"][0].as_str().unwrap().starts_with("target: "), "{d}");
    let d = herdsman::spawn::clone_build(&r, &["targt".into()], &local, &wt).unwrap();
    assert_eq!(d["errors"], json!(["targt: not target or node_modules"]));
    assert_eq!(r.count("cp "), 1);
}

/// #185: a reopened worktree keeps its own build folders.
#[test]
fn reopen_spawn_clones_no_build_folder() {
    let d = tmpdir("clone-reopen");
    let local = d.join("local");
    let cfg = CONFIG
        .replace("/tmp/does-not-matter/sendmeter", &local.to_string_lossy())
        .replace(
            "integration = \"staging\"",
            "integration = \"staging\"\nclone_build = [\"target\"]",
        );
    let c = ctx(&d, &cfg);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(local.join("target")).unwrap();
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(local.join("Cargo.lock"), "a").unwrap();
    std::fs::write(wt.join("Cargo.lock"), "a").unwrap();
    let sh = on_shed(&c);
    let r = shed_fake()
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
        .reply("cp ", "");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    assert!(sh.called("run", "sendmeter-impl-12"), "the lane started");
    let calls = r.calls.borrow();
    assert!(
        !calls.iter().any(|c| c.contains(" worktree add ")),
        "reopened: {calls:?}"
    );
    drop(calls);
    assert!(!r.called("cp "));
    let ev = read(&c.state).unwrap();
    assert!(ev.iter().any(|e| e.kind == "spawn") && !ev.iter().any(|e| e.kind == "build-cloned"));
}

struct Checkout<'a> {
    f: &'a Fake,
    wt: std::path::PathBuf,
    sh: &'a FakeShed,
    beside: Shared<bool>,
    cloned: Shared<bool>,
    typed: Shared<usize>,
}

const LANE: &str = "sendmeter-impl-12";

fn within(ok: impl Fn() -> bool) -> bool {
    let t = std::time::Instant::now();
    while !ok() && t.elapsed().as_secs() < 5 {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    ok()
}

impl herdsman::runner::Runner for Checkout<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        if prog == "git" && args.get(2..4) == Some(&["worktree".into(), "add".into()]) {
            std::fs::create_dir_all(&self.wt).unwrap();
            std::fs::write(self.wt.join("Cargo.lock"), "a").unwrap();
        }
        if prog == "cp" {
            self.beside.set(within(|| !self.sh.all("wait").is_empty()));
            self.typed.set(self.sh.sent(LANE).len());
            self.sh.lock().waits.remove(LANE);
            self.cloned.set(true);
        }
        self.f.run(prog, args)
    }
    fn sleep(&self, secs: u64) {
        within(|| self.cloned.get());
        self.f.sleep(secs)
    }
}

/// #185: a fresh lane whose clone fails: the lane started first, the failure is logged, the spawn exits 0.
#[test]
fn failed_clone_is_logged_and_the_spawn_succeeds() {
    let d = tmpdir("clone-fail");
    let local = d.join("local");
    std::fs::create_dir_all(local.join("target")).unwrap();
    std::fs::write(local.join("Cargo.lock"), "a").unwrap();
    let cfg = CONFIG
        .replace("/tmp/does-not-matter/sendmeter", &local.to_string_lossy())
        .replace(
            "integration = \"staging\"",
            "integration = \"staging\"\nclone_build = [\"target\"]",
        );
    let c = ctx(&d, &cfg);
    let sh = on_shed(&c);
    sh.wait_ends(LANE, "timeout");
    let wt = c.worktrees.join("sendmeter/issue-12");
    let f = shed_fake()
        .once(&format!("git -C {} rev-parse --verify", local.display()), 1, "")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
        .once_err("cp ", 1, "clonefile failed");
    let ck = Checkout {
        f: &f,
        wt,
        sh: &sh,
        beside: Shared::new(false),
        cloned: Shared::new(false),
        typed: Shared::new(0),
    };
    let (code, out) = run(&c, &ck, &["spawn", "sendmeter", "12", "impl"]);
    assert!(ck.beside.get(), "the clone runs beside the kick, not before it");
    assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
    assert_eq!(f.count("cp "), 1, "one clone attempt, never a fallback copy");
    assert!(
        ck.typed.get() >= 2,
        "the lane starts (its exports and harness typed) before the clone"
    );
    let ev = read(&c.state).unwrap();
    let e = ev
        .iter()
        .find(|e| e.kind == "build-cloned")
        .expect("build-cloned logged");
    assert_eq!(e.data["dirs"], json!([]));
    assert!(
        e.data["errors"][0].as_str().unwrap().contains("clonefile failed"),
        "{e:?}"
    );
    assert_eq!(fold(&ev)[&("sendmeter".to_string(), 12)].state, "impl");
}

#[test]
fn hermes_role_dry_run_kicks_prompt_after_boot() {
    let d = tmpdir("spawn-hermes");
    let c = ctx(&d, CONFIG);
    let _sh = on_shed(&c);
    let p = plan(&c, &shed_fake(), "rev");
    assert_eq!(
        export(&p),
        [
            "export".to_string(),
            "HERDSMAN_LANE='sendmeter:12:rev'".into(),
            format!("HERDSMAN_PORT={}", lane_port(None, "sendmeter/rev-12-r1", &[]))
        ]
    );
    let start = launch(&p);
    assert_eq!(
        start[start.len() - 3..],
        ["-p".to_string(), "fleet-rev".into(), "--accept-hooks".into()]
    );
    assert_eq!(start[..2], ["exec", "hermes"].map(String::from));
    let q = requests(&p);
    let at = q.iter().position(|r| r.0 == "wait").unwrap();
    assert_eq!(
        (&q[at].1["name"], &q[at].1["output"]),
        (&json!("sendmeter-rev-12-r1"), &json!("❯"))
    );
    assert_eq!(q[at + 1].0, "send");
    assert_eq!(q[at + 1].1, json!({"name": "sendmeter-rev-12-r1", "text": p["kick"]}));
    assert_eq!(p["base"], "origin/issue-12");
}

#[test]
fn real_claude_spawn_writes_brief_and_logs_event() {
    let d = tmpdir("spawn-real");
    let c = ctx(&d, &CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\""));
    let sh = on_shed(&c);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    // Without a synced profile a claude spawn is refused.
    let (code, _) = run(&c, &shed_fake(), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 1);
    assert!(!sh.called("run", LANE));
    let (code, _) = run(&c, &Fake::default(), &["profiles", "sync"]);
    assert_eq!(code, 0);
    let settings = std::fs::read_to_string(c.config.parent().unwrap().join("claude/fleet-impl/settings.json")).unwrap();
    let s: Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(
        s["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "'/opt/herdsman/bin/herdsman' hook --role impl"
    );
    let r = shed_fake()
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git -C", "");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let sent = sh.sent(LANE);
    assert!(
        sent.len() == 2
            && sent[0].starts_with("export HERDSMAN_LANE='sendmeter:12:impl'")
            && sent[1].starts_with("exec 'claude'"),
        "{sent:?}"
    );
    assert!(
        sent[1].contains("'--name' 'sendmeter · impl #12 · Fix the meter' '--mcp-config'"),
        "{}",
        sent[1]
    );
    let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
    assert!(brief.contains("# ROLE: IMPLEMENTER") && brief.contains("AC: it works") && brief.contains("Closes #12"));
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!(
        (st.state.as_str(), st.lane.as_deref()),
        ("impl", Some("sendmeter-impl-12"))
    );
    let ev = read(&c.state)
        .unwrap()
        .into_iter()
        .rfind(|e| e.kind == "spawn")
        .unwrap();
    assert_eq!(ev.data["port"], lane_port(None, "sendmeter/issue-12", &[]));
    assert_eq!(ev.data["confirmed"], true);
}

#[test]
fn herdr_reopened_workspace_keeps_the_lane_name() {
    let d = tmpdir("spawn-real-herdr");
    let c = ctx(&d, &CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\""));
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git -C", "")
        .reply(
            "herdr worktree open",
            r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#,
        )
        .reply(
            "herdr pane list --workspace w9",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr pane run w9:p1 export HERDSMAN_LANE='sendmeter:12:impl'", "{}")
        .reply("herdr agent start sendmeter-impl-12 --kind claude --pane w9:p1", "{}")
        .reply("herdr agent wait sendmeter-impl-12", "{}");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let open = r
        .calls
        .borrow()
        .iter()
        .find(|c| c.starts_with("herdr worktree open"))
        .cloned()
        .unwrap();
    assert!(open.ends_with("--label sendmeter-impl-12"), "{open}");
    assert!(r.called("herdr workspace rename w9 sendmeter-impl-12"));
}

#[test]
fn unconfirmed_hermes_kick_presses_enter_then_exits_4() {
    let d = tmpdir("spawn-unconfirmed");
    let c = ctx(&d, CONFIG);
    let sh = on_shed(&c);
    sh.wait_ends(LANE, "timeout");
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let r = shed_fake()
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "");
    let (code, _) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 4);
    let prompts = sh.sent(LANE).into_iter().filter(|t| t.starts_with("Read "));
    assert_eq!(prompts.count(), 3, "3 attempts");
    let enters = sh
        .all("keys")
        .into_iter()
        .filter(|k| k["name"] == LANE && k["keys"] == json!(["Enter"]));
    assert_eq!(enters.count(), 3);
    assert_eq!(r.count("sleep 10"), 2, "attempts spaced ≥10 s apart");
    let ev = read(&c.state).unwrap();
    assert_eq!(ev.last().unwrap().data["confirmed"], false);
}

#[test]
fn help_agent_doc_is_short_and_complete() {
    let d = tmpdir("help");
    let c = ctx(&d, CONFIG);
    let (code, out) = run(&c, &Fake::default(), &["help", "--agent"]);
    assert_eq!(code, 0);
    assert!(
        out.lines().count() <= herdsman::HELP_AGENT_MAX,
        "{} lines",
        out.lines().count()
    );
    for w in [
        "herdsman spawn",
        "herdsman merge",
        "herdsman verdict",
        "herdsman set",
        "herdsman status",
        "herdsman profiles sync",
        "herdsman timer install",
        "herdsman check",
        "herdsman clean",
        "herdsman adopt",
        "--force",
        "--supersede",
        "--yes",
        "MAIN_TARGET",
        "HOST_BUSY",
        "LANE_LIVE",
        "WORKTREE_DIRTY",
        "review(n)",
        "--serial",
        "SERIAL_BUSY",
        "SERIAL_UNMARKED",
        "CAP",
        "BAD_VALUE",
        "design",
        "owner-only",
        "herdsman stop",
        "herdsman kill",
        "--lane",
        "SECRET_IN_WIP",
    ] {
        assert!(out.contains(w), "missing {w}");
    }
    assert!(!out.contains("herdsman hook"));
}

/// #2 AC1: a role's effort (or the `[limits]` default) is mapped per harness in the launch argv.
#[test]
fn effort_maps_to_each_harness_flag() {
    let d = tmpdir("effort-claude");
    let cfg = CONFIG
        .replace(
            "harness  =  \"hermes\"    # toggled in tests",
            "harness = \"claude\"\neffort = \"high\"",
        )
        .replace("review_rounds = 3", "review_rounds = 3\neffort = \"xhigh\"");
    let c = ctx(&d, &cfg);
    let _sh = on_shed(&c);
    let p = plan(&c, &shed_fake(), "impl");
    let start = launch(&p);
    let at = start.iter().position(|w| w == "--effort").expect("--effort");
    assert_eq!(start[at + 1], "high");
    assert_eq!(p["effort"], "high");
    // rev sets none: the [limits] default applies, as Hermes `--reasoning`.
    let start = launch(&plan(&c, &shed_fake(), "rev"));
    assert_eq!(start[start.len() - 2..], ["--reasoning", "xhigh"].map(String::from));

    // Claude `none` omits the flag; Hermes passes it through.
    let d = tmpdir("effort-none");
    let c = ctx(
        &d,
        &cfg.replace("effort = \"high\"", "effort = \"none\"")
            .replace("effort = \"xhigh\"", "effort = \"none\""),
    );
    let _sh = on_shed(&c);
    assert!(!launch(&plan(&c, &shed_fake(), "impl")).contains(&"--effort".to_string()));
    let start = launch(&plan(&c, &shed_fake(), "rev"));
    assert_eq!(start[start.len() - 2..], ["--reasoning", "none"].map(String::from));

    // No effort anywhere: no flag at all.
    let d = tmpdir("effort-unset");
    let c = ctx(&d, CONFIG);
    let _sh = on_shed(&c);
    let start = launch(&plan(&c, &shed_fake(), "rev"));
    assert!(!start.contains(&"--reasoning".to_string()), "{start:?}");
}

/// #2 AC1: an unknown level, or one the role's harness lacks, is refused at config load.
#[test]
fn invalid_effort_is_refused_at_config_load() {
    for (cfg, want) in [
        (
            CONFIG.replace("profile = \"fleet-rev\"", "profile = \"fleet-rev\"\neffort = \"max\""),
            "fleet.sendmeter.role.rev: effort \"max\" must be one of",
        ),
        (
            CONFIG.replace("review_rounds = 3", "review_rounds = 3\neffort = \"ultra\""),
            "limits.effort: \"ultra\" must be one of",
        ),
        (
            CONFIG.replace(
                "harness = \"hermes\"\nprofile = \"fleet-rev\"",
                "harness = \"dsh\"\nprofile = \"fleet-rev\"\neffort = \"high\"",
            ),
            "fleet.sendmeter.role.rev: harness dsh does not support effort \"high\"",
        ),
    ] {
        let d = tmpdir("effort-bad");
        let c = ctx(&d, &cfg);
        let err = herdsman::config::Config::load(&c.config).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains(want), "{msg}");
        let (code, _) = run(&c, &Fake::default(), &["spawn", "sendmeter", "12", "rev", "--dry-run"]);
        assert_eq!(code, 1);
    }
}

/// #19: Claude lanes get `--name "<fleet> · <role> #<issue> · <short title>"`.
#[test]
fn claude_dry_run_names_every_role() {
    let d = tmpdir("spawn-name");
    let cfg = CONFIG
        .replace("harness = \"hermes\"", "harness = \"claude\"")
        .replace("harness  =  \"hermes\"", "harness = \"claude\"")
        + "\n[fleet.sendmeter.role.design]\nharness = \"claude\"\nprofile = \"fleet-design\"\n";
    let c = ctx(&d, &cfg);
    let _sh = on_shed(&c);
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(
        cache.join("issue-sendmeter-12.json"),
        r#"{"title":"feat(meter): fix the meter","body":""}"#,
    )
    .unwrap();
    let name = |args: &[&str]| {
        let (code, out) = run(&c, &shed_fake(), args);
        assert_eq!(code, 0, "{out}");
        let p: Value = serde_json::from_str(&out).unwrap();
        let start = launch(&p);
        let at = start.iter().position(|w| w == "--name").expect("--name");
        assert_eq!(start[at + 1], p["title"].as_str().unwrap());
        start[at + 1].clone()
    };
    let dry = |n: &str, role: &str| name(&["spawn", "sendmeter", n, role, "--dry-run", "--json"]);
    assert_eq!(dry("12", "impl"), "sendmeter · impl #12 · fix the meter");
    assert_eq!(dry("12", "rev"), "sendmeter · rev #12 r1 · fix the meter");
    assert_eq!(dry("12", "design"), "sendmeter · design #12 · fix the meter");
    // No cached spec (a dry run never calls GitHub): the head alone.
    assert_eq!(dry("13", "impl"), "sendmeter · impl #13");
}

#[test]
fn lane_title_strips_prefixes_and_cuts_at_words() {
    use herdsman::spawn::lane_title;
    let h = "synergy · impl #2840";
    assert_eq!(
        lane_title(h, "M- prefix for Mango numbers"),
        "synergy · impl #2840 · M- prefix for Mango numbers"
    );
    assert_eq!(
        lane_title(h, "P0: feat(api)!: drop v1"),
        "synergy · impl #2840 · drop v1"
    );
    assert_eq!(
        lane_title(h, "spawn: informative titles"),
        "synergy · impl #2840 · informative titles"
    );
    assert_eq!(lane_title(h, "Fix 10:30 cron"), "synergy · impl #2840 · Fix 10:30 cron");
    assert_eq!(lane_title(h, "  "), h);
    let long = lane_title(
        h,
        "spawn: informative lane titles instead of Claude auto-titles from the brief prompt",
    );
    assert_eq!(long, "synergy · impl #2840 · informative lane titles instead of…");
    assert!(long.chars().count() <= 60);
    // Thai: kept whole when it fits; cut by chars (never bytes) when it does not.
    assert_eq!(lane_title(h, "แก้ไขปุ่มชำระเงิน"), "synergy · impl #2840 · แก้ไขปุ่มชำระเงิน");
    let thai = lane_title(h, &"ภาษาไทย".repeat(10));
    assert!(thai.ends_with('…') && thai.chars().count() <= 60, "{thai}");
    assert!(thai.starts_with("synergy · impl #2840 · ภาษาไทยภาษาไทย"));
}
