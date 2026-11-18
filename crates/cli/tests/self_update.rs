mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;
use std::path::{Path, PathBuf};

const SHA: &str = "abcdef1234567890abcdef1234567890abcdef12";
const OLD: &str = "0000000000000000000000000000000000000000";
const FLEET: &str = "[fleet.drover]\nrepo = \"jirathip-dev/herdsman\"\nlocal = \"/x/drover\"\nintegration = \"staging\"\n\n[fleet.drover.role.impl]\nharness = \"claude\"\n";

fn ver(sha: &str) -> String {
    format!("drover 0.1.0 ({sha})")
}

fn sums() -> String {
    format!(
        "{}  drover-aarch64-apple-darwin\n{}  drover-x86_64-unknown-linux-gnu\n",
        "a".repeat(64),
        "b".repeat(64)
    )
}

fn setup(name: &str) -> (drover::Ctx, PathBuf) {
    setup_with(name, FLEET)
}

fn setup_with(name: &str, config: &str) -> (drover::Ctx, PathBuf) {
    let d = tmpdir(name);
    let mut c = ctx(&d, config);
    version_bin(&d.join("bin/drover"), &ver(OLD));
    c.exe = d.join("bin/drover").to_string_lossy().into();
    std::fs::create_dir_all(&c.home).unwrap();
    let live = std::fs::canonicalize(&c.exe).unwrap();
    (c, live)
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

fn at(c: &drover::Ctx, now: u64) -> drover::Ctx {
    drover::Ctx { now, ..c.clone() }
}

fn has(p: &Path, sha: &str) -> bool {
    std::fs::read_to_string(p).is_ok_and(|s| s.contains(&ver(sha)))
}

fn release(f: Fake, branch: &str, sha: &str, exe: &Path) -> Fake {
    let tag = drover::release::tag(branch, sha);
    f.reply(
        &format!("git -C /x/drover rev-parse origin/{branch}"),
        &format!("{sha}\n"),
    )
    .reply(&format!("gh release download {tag} "), &sums())
    .reply(&format!("sh -c ( n='{}.new'", exe.display()), &ver(sha))
}

fn base(c: &drover::Ctx, live: &Path) -> Fake {
    let staging = c.state.with_file_name("update-staging/bin/drover");
    Fake::default()
        .reply(&format!("{}.new doctor --config-only", live.display()), "")
        .reply(&format!("{}.new doctor --config-only", staging.display()), "")
        .reply(&format!("{} profiles sync", live.display()), "")
        .reply("git -C /x/drover fetch", "")
        .reply("launchctl", "")
        .reply("systemctl", "")
}

fn heads(c: &drover::Ctx, live: &Path, heads: &[(&str, &str)]) -> Fake {
    let staging = c.state.with_file_name("update-staging/bin/drover");
    let exe = |b: &str| {
        if b == "staging" {
            staging.clone()
        } else {
            live.to_path_buf()
        }
    };
    heads
        .iter()
        .fold(base(c, live), |f, (b, sha)| release(f, b, sha, &exe(b)))
}

const MAIN: &str = "1111111111111111111111111111111111111111";
const MAIN2: &str = "2222222222222222222222222222222222222222";
const STAGING: &str = "3333333333333333333333333333333333333333";

#[test]
fn branch_main_ignores_a_newer_staging_head_and_installs_main_heads_release() {
    let (c, live) = setup_with("upd-main", &format!("{FLEET}\n[self_update]\nbranch = \"main\"\n"));
    let r =
        heads(&c, &live, &[("main", MAIN)]).reply("git -C /x/drover rev-parse origin/staging", &format!("{STAGING}\n"));
    let (code, out) = run(&c, &Disk(&r), &["self-update"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("✓ installed 1111111 at "), "{out}");
    assert!(
        r.called("git -C /x/drover fetch -q origin main"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(!r.called("git -C /x/drover fetch -q origin staging") && !r.called("gh release download build-staging-"));
    assert!(has(&live, MAIN), "the release is live");

    let r = heads(&c, &live, &[("main", MAIN), ("staging", MAIN2)]);
    assert_eq!(run(&c, &Disk(&r), &["self-update"]).1, "= up to date @ 1111111\n");
    assert!(!r.called("sh -c ( n="), "nothing staged");

    let r = heads(&c, &live, &[("main", STAGING)]);
    let (code, out) = run(&c, &Disk(&r), &["self-update"]);
    assert!(code == 0 && out.starts_with("✓ installed 3333333 at "), "{out}");
    assert!(has(&live, STAGING));
    let ev = kinds(&c, "self-updated");
    let got: Vec<_> = ev
        .iter()
        .map(|e| (e.str("branch").unwrap(), e.str("sha").unwrap()))
        .collect();
    assert_eq!(got, [("main", MAIN), ("main", STAGING)]);
}

#[test]
fn no_release_builds_nothing_waits_and_alerts_once_after_the_wait_limit() {
    let (c, live) = setup("upd-norel");
    let r = || base(&c, &live).reply("git -C /x/drover rev-parse origin/main", &format!("{SHA}\n"));
    let pass = |now: u64| {
        let r = r();
        let (code, out) = run(&at(&c, now), &Disk(&r), &["self-update"]);
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("main @ abcdef1: waiting for its release (no release build-abcdef1"),
            "{out}"
        );
        assert!(!r.called("cargo") && !r.called("sh -c ( n=") && !r.called("git -C /x/drover worktree"));
        assert!(r.called("gh release download build-abcdef1 "), "retried each pass");
    };
    pass(c.now);
    assert!(
        kinds(&c, "self-update-waiting").is_empty(),
        "no alert inside the wait limit"
    );
    pass(c.now + 46 * 60);
    pass(c.now + 50 * 60);
    let w = kinds(&c, "self-update-waiting");
    assert_eq!(w.len(), 1, "once per head");
    assert_eq!((w[0].str("key"), w[0].str("target")), (Some(SHA), Some("live")));
    assert!(has(&live, OLD) && !Path::new(&format!("{}.new", live.display())).exists());

    let r = release(base(&c, &live), "main", SHA, &live);
    let (code, out) = run(&at(&c, c.now + 55 * 60), &Disk(&r), &["self-update"]);
    assert!(code == 0 && out.starts_with("✓ installed abcdef1"), "{out}");
}

#[test]
fn a_release_missing_past_the_wait_alerts_once_with_the_failed_job_and_its_first_error() {
    let (c, live) = setup("upd-norel-why");
    let repo = "jirathip-dev/herdsman";
    let runs = format!("gh run list -R {repo} -w release.yml -c {SHA} ");
    let annotations = |id: u64| format!("gh api repos/{repo}/check-runs/{id}/annotations");
    let error = "the macOS signing failed: Developer ID Application: no identity found";
    let r = || {
        base(&c, &live)
            .reply("git -C /x/drover rev-parse origin/main", &format!("{SHA}\n"))
            .reply(&runs, r#"[{"databaseId":44263734602}]"#)
            .reply(
                &format!("gh run view 44263734602 -R {repo} --json jobs"),
                r#"{"jobs":[{"databaseId":7,"name":"linux"},{"databaseId":8,"name":"macos"}]}"#,
            )
            .reply(&annotations(7), "[]")
            .reply(
                &annotations(8),
                &json!([
                    {"annotation_level": "warning", "message": "a warning"},
                    {"annotation_level": "failure", "message": error},
                    {"annotation_level": "failure", "message": "a later error"}
                ])
                .to_string(),
            )
    };
    let mut asked = vec![];
    for mins in [0, 46, 50] {
        let r = r();
        let (code, out) = run(&at(&c, c.now + mins * 60), &Disk(&r), &["self-update"]);
        assert_eq!(code, 0, "{out}");
        asked.push(r.called(&runs));
    }
    assert_eq!(asked, [false, true, false], "the run is read only for the alert");
    let w = kinds(&c, "self-update-waiting");
    assert_eq!(w.len(), 1, "once per head");
    assert_eq!((w[0].str("job"), w[0].str("error")), (Some("macos"), Some(error)));
    let reason = w[0].str("reason").unwrap();
    assert!(reason.contains("the macos job") && reason.contains(error), "{reason}");
}

#[test]
fn a_bad_checksum_or_a_wrong_version_keeps_the_live_binary_and_alerts_once() {
    let other = "9999999999999999999999999999999999999999";
    for (name, code, said, why) in [
        (
            "upd-sum",
            4,
            String::new(),
            "checksum: the download does not match SHA256SUMS",
        ),
        ("upd-ver", 0, ver(other), "version: it says"),
    ] {
        let (c, live) = setup(name);
        let new = PathBuf::from(format!("{}.new", live.display()));
        let r = || {
            release(base(&c, &live), "main", SHA, &live).once(&format!("sh -c ( n='{}'", new.display()), code, &said)
        };
        for _ in 0..2 {
            let r = r();
            let (exit, out) = run(&c, &Disk(&r), &["self-update"]);
            assert_eq!(exit, 1, "{out}");
            assert!(out.contains(&format!("✗ local: abcdef1 not staged: {why}")), "{out}");
            assert!(has(&live, OLD), "{name}: the live binary is untouched");
            assert!(!new.exists(), "{name}: drover.new is gone");
            assert!(!r.called(&format!("{} profiles sync", live.display())));
        }
        let failed = kinds(&c, "self-update-failed");
        assert_eq!(failed.len(), 1, "{name}: once per host and head");
        assert_eq!(failed[0].str("key"), Some("local abcdef1"));
        assert!(failed[0].str("reason").unwrap().contains(why), "{failed:?}");
        assert!(kinds(&c, "self-updated").is_empty());
    }
}

fn job_args(c: &drover::Ctx, label: &str) -> Vec<String> {
    if cfg!(target_os = "macos") {
        let p = std::fs::read_to_string(c.home.join(format!("Library/LaunchAgents/{label}.plist"))).unwrap();
        let a = p.split("<array>").nth(1).unwrap().split("</array>").next().unwrap();
        a.split("<string>")
            .skip(1)
            .map(|s| s.split("</string>").next().unwrap().to_string())
            .collect()
    } else {
        let p = std::fs::read_to_string(c.home.join(format!(".config/systemd/user/{label}.service"))).unwrap();
        let l = p.lines().find_map(|l| l.strip_prefix("ExecStart=")).unwrap();
        l.split(' ').map(String::from).collect()
    }
}

#[test]
fn the_staging_serve_runs_the_staging_head_and_cannot_write_the_live_state() {
    let text = format!("{FLEET}\n[self_update]\nstaging_serve = true\nstaging_addr = \"127.0.0.1:0\"\n");
    let (c, live) = setup_with("upd-stg", &text);
    let r = heads(&c, &live, &[("staging", STAGING), ("main", MAIN)]);
    let (code, out) = run(&c, &Disk(&r), &["self-update"]);
    assert_eq!(code, 0, "{out}");
    let bin = c.state.with_file_name("update-staging/bin/drover");
    assert!(
        out.contains(&format!(
            "✓ staging serve: installed 3333333 at {}; staging serve on 127.0.0.1:0",
            bin.display()
        )),
        "{out}"
    );
    assert!(
        r.called("gh release download build-staging-3333333 "),
        "the staging prerelease"
    );
    assert!(has(&bin, STAGING) && has(&live, MAIN), "main installed apart");
    std::fs::copy(env!("CARGO_BIN_EXE_drover"), &bin).unwrap();
    let args = job_args(&c, "dev.drover.serve-staging");
    assert_eq!(
        args,
        [
            bin.to_string_lossy().as_ref(),
            "serve",
            "--read-only",
            "--addr",
            "127.0.0.1:0"
        ]
    );

    append(&c.state, &Event::new(c.now, "drover", None, "note", json!({}))).unwrap();
    let before = read(&c.state).unwrap().len();
    let mut p = std::process::Command::new(&args[0])
        .args(&args[1..])
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .env("DROVER_HOME", &c.home)
        .env("PATH", "/usr/bin:/bin")
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut err = std::io::BufReader::new(p.stderr.take().unwrap());
    let mut banner = String::new();
    std::io::BufRead::read_line(&mut err, &mut banner).unwrap();
    let addr = banner
        .trim()
        .strip_prefix("drover serve: read-only view on http://")
        .unwrap()
        .trim_end_matches('/')
        .to_string();
    let post = |path: &str| {
        let mut s = std::net::TcpStream::connect(&addr).unwrap();
        let body = "fleet=drover";
        let req = format!("POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        std::io::Write::write_all(&mut s, req.as_bytes()).unwrap();
        let mut res = String::new();
        let _ = std::io::Read::read_to_string(&mut s, &mut res);
        res
    };
    let paused = post("/pause");
    let queued = post("/queue");
    let _ = p.kill();
    let _ = p.wait();
    assert!(paused.starts_with("HTTP/1.1 405"), "{paused}");
    assert!(queued.starts_with("HTTP/1.1 405"), "{queued}");
    assert_eq!(
        read(&c.state).unwrap().len(),
        before,
        "nothing written to the live state"
    );
}

#[test]
fn a_release_is_staged_gated_swapped_synced_and_restarted() {
    use std::os::unix::fs::MetadataExt;
    let (c, live) = setup("upd-green");
    let new = format!("{}.new", live.display());
    let r = release(base(&c, &live), "main", SHA, &live);
    let (code, out) = run(&c, &Disk(&r), &["self-update", "--dry-run"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("main @ abcdef1: build-abcdef1 found\n"), "{out}");
    assert!(
        out.contains("would stage, then swap, last") && !r.called("sh -c ( n="),
        "{out}"
    );

    let old = live.with_file_name("drover.running");
    std::fs::hard_link(&live, &old).unwrap();
    let (code, out) = run(&c, &Disk(&r), &["self-update"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("✓ installed abcdef1 at "), "{out}");
    assert!(has(&live, SHA), "swapped in");
    let ino = |p: &Path| std::fs::metadata(p).unwrap().ino();
    assert_ne!(ino(&live), ino(&old), "renamed over, never rewritten in place");
    assert!(has(&old, OLD), "a running serve keeps its file");
    assert!(!PathBuf::from(&new).exists());
    let at = |p: &str| {
        r.index(p)
            .unwrap_or_else(|| panic!("{p} not called: {:?}", r.calls.borrow()))
    };
    let order = [
        at("gh release download build-abcdef1 "),
        at(&format!("sh -c ( n='{new}'")),
        at(&format!("{new} doctor --config-only")),
        at(&format!("{} profiles sync", live.display())),
        at(if cfg!(target_os = "macos") {
            "launchctl kickstart -k gui/"
        } else {
            "systemctl --user restart"
        }),
    ];
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    let calls = r.calls.borrow().join("\n");
    assert!(
        !calls.contains("cargo") && !calls.contains("codesign") && !calls.contains("--tokens"),
        "{calls}"
    );
    assert_eq!(kinds(&c, "self-updated")[0].str("sha"), Some(SHA));
    assert!(kinds(&c, "self-update-swapping").is_empty(), "no remote host to hold");

    let r2 = release(base(&c, &live), "main", SHA, &live);
    assert_eq!(run(&c, &Disk(&r2), &["self-update"]).1, "= up to date @ abcdef1\n");
    assert!(!r2.called("sh -c ( n="));
}

#[test]
fn the_serve_restart_is_the_jobs_own_restart_never_a_handover() {
    let config = format!("{FLEET}\n[limits]\nserve_handover_secs = 1\n");
    let (c, live) = setup_with("upd-kick", &config);
    let sock = drover::update::serve_sock(&c, 7777);
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let ctl = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    ctl.set_nonblocking(true).unwrap();
    let r = release(base(&c, &live), "main", SHA, &live);
    let (code, out) = run(&c, &Disk(&r), &["self-update"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim_end().ends_with("serve restarted"), "{out}");
    let restart = if cfg!(target_os = "macos") {
        "launchctl kickstart -k gui/"
    } else {
        "systemctl --user restart dev.drover.serve.service"
    };
    assert!(r.called(restart), "{:?}", r.calls.borrow());
    assert!(ctl.accept().is_err(), "no serve was asked to hand over");
    let calls = r.calls.borrow().join("\n");
    assert!(!calls.contains(" serve"), "no other serve start: {calls}");
}

#[test]
fn a_failed_doctor_keeps_the_old_binary_and_alerts_once_per_head() {
    let (c, live) = setup("upd-doctor");
    let new = format!("{}.new", live.display());
    let bad = "✗ config — limits.effort: \"bogus\" must be one of …\n";
    let r = || release(base(&c, &live), "main", SHA, &live).once(&format!("{new} doctor --config-only"), 1, bad);
    let (code, out) = run(&c, &Disk(&r()), &["self-update"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("doctor --config-only: ✗ config — limits.effort"), "{out}");
    assert!(has(&live, OLD));
    assert!(!PathBuf::from(&new).exists(), "the staged build is dropped");
    let failed = kinds(&c, "self-update-failed");
    assert_eq!(failed.len(), 1);
    assert!(failed[0].str("reason").unwrap().contains("limits.effort"));
    let r2 = r();
    assert_eq!(run(&c, &Disk(&r2), &["self-update"]).0, 1);
    assert!(!r2.called("profiles sync"));
    assert_eq!(kinds(&c, "self-update-failed").len(), 1);
}

#[test]
fn pause_and_resume_are_logged_and_pause_stops_the_job() {
    let (c, live) = setup("upd-pause");
    assert_eq!(
        run(&c, &Fake::default(), &["self-update", "--pause"]).1,
        "✓ self-update paused\n"
    );
    let r = release(base(&c, &live), "main", SHA, &live);
    let (code, out) = run(&c, &Disk(&r), &["self-update"]);
    assert_eq!(
        (code, out.as_str()),
        (0, "= self-update paused → drover self-update --resume\n")
    );
    assert!(r.calls.borrow().is_empty());
    assert_eq!(kinds(&c, "self-update-paused")[0].str("by"), Some("owner"));
    run(&c, &Fake::default(), &["self-update", "--resume"]);
    assert_eq!(kinds(&c, "self-update-resumed").len(), 1);
    let r = base(&c, &live).reply("git -C /x/drover rev-parse origin/main", &format!("{SHA}\n"));
    assert!(run(&c, &Disk(&r), &["self-update"])
        .1
        .contains("waiting for its release"));
    assert!(has(&live, OLD));
}

#[test]
fn scheduled_clean_keeps_its_age_cap_and_live_dirty_rules_and_alerts_a_failure_once() {
    let d = tmpdir("clean-sched");
    let c = ctx(&d, CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let day = 86_400;
    let lane = |issue: u64, merged_at: u64| {
        let w = c.worktrees.join("sendmeter").join(format!("issue-{issue}"));
        std::fs::create_dir_all(&w).unwrap();
        let w = w.to_string_lossy().into_owned();
        let d = json!({"role": "impl", "agent": format!("impl-{issue}"), "worktree": w, "pane": format!("impl-{issue}"), "runtime": "shed"});
        append(&c.state, &Event::new(10, "sendmeter", Some(issue), "spawn", d)).unwrap();
        let m = json!({"pr": "1", "merge_sha": "m", "into": "staging"});
        append(&c.state, &Event::new(merged_at, "sendmeter", Some(issue), "merged", m)).unwrap();
        w
    };
    let old = c.now - 4 * day;
    let removable: Vec<String> = (1..=12).map(|n| lane(n, old)).collect();
    let fresh = lane(20, c.now - day);
    let live = lane(21, old);
    let dirty = lane(22, old);
    let all: Vec<&String> = removable.iter().chain([&fresh, &live, &dirty]).collect();
    let git = |f: Fake| {
        let f = all
            .iter()
            .fold(f, |f, w| f.reply(&format!("git -C {w} rev-list"), "0\n"));
        f.reply("git -C", "")
    };
    sh.lane("impl-21", "idle", &live);
    let r = git(Fake::default()
        .once(&format!("git -C {dirty} status"), 0, "?? notes.md\n")
        .once_err(&format!("git -C {dirty} add -A"), 1, "add failed"));
    let (code, out) = run(&c, &r, &["clean", "--scheduled"]);
    assert_eq!(code, 0, "{out}");
    let cleaned: Vec<String> = kinds(&c, "clean")
        .iter()
        .map(|e| e.str("worktree").unwrap().into())
        .collect();
    assert_eq!(cleaned.len(), 10, "10 a run: {out}");
    assert!(cleaned.iter().all(|w| removable.contains(w)), "{cleaned:?}");
    assert!(out.contains("stop: 10 removed this run"), "{out}");
    assert!(
        !out.contains(&fresh) && !cleaned.contains(&fresh),
        "merged a day ago stays: {out}"
    );
    assert!(out.contains(&format!("keep {live}: live agent")), "{out}");
    assert!(out.contains(&format!("keep {dirty}: wip save failed")), "{out}");
    for w in &cleaned {
        assert!(
            r.called(&format!("git -C /tmp/does-not-matter/sendmeter worktree remove {w}")),
            "{out}"
        );
    }
    assert!(
        !r.called(&format!("git -C /tmp/does-not-matter/sendmeter worktree remove {live}")),
        "{out}"
    );
    assert!(kinds(&c, "clean-failed").is_empty(), "a passing run alerts nothing");
    for w in &cleaned {
        std::fs::remove_dir_all(w).unwrap();
    }

    sh.remove("impl-21");
    let r = || git(Fake::default());
    let (code, out) = run(&c, &r(), &["clean", "--scheduled"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "clean").len(), 14, "{out}");
    assert!(!kinds(&c, "clean").iter().any(|e| e.str("worktree") == Some(&fresh)));
    drop(sh);
    let (code, out) = run(&c, &r(), &["clean", "--scheduled"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(run(&c, &r(), &["clean", "--scheduled"]).0, 1);
    let failed = kinds(&c, "clean-failed");
    assert_eq!(failed.len(), 1);
    assert!(
        failed[0].str("reason").unwrap().contains("refusing to clean blind"),
        "{failed:?}"
    );
}

#[test]
fn setup_and_timer_write_the_update_and_daily_clean_jobs() {
    let c = ctx(&tmpdir("upd-jobs"), FLEET);
    let r = Fake::default().reply("which", "/opt/bin/x\n");
    let (code, out) = run(&c, &r, &["timer", "install", "--no-load"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("✓ wrote dev.drover.update") && out.contains("✓ wrote dev.drover.clean"),
        "{out}"
    );
    let (update, clean) = if cfg!(target_os = "macos") {
        let p = |l: &str| std::fs::read_to_string(c.home.join(format!("Library/LaunchAgents/{l}.plist"))).unwrap();
        (p("dev.drover.update"), p("dev.drover.clean"))
    } else {
        let p = |l: &str| std::fs::read_to_string(c.home.join(format!(".config/systemd/user/{l}.timer"))).unwrap();
        (p("dev.drover.update"), p("dev.drover.clean"))
    };
    assert!(update.contains("300") && !update.contains("03:30"), "{update}");
    let daily = [
        "<key>Hour</key><integer>3</integer><key>Minute</key><integer>30</integer>",
        "OnCalendar=*-*-* 03:30:00",
    ];
    assert!(daily.iter().any(|d| clean.contains(d)), "{clean}");
    let (_, out) = run(&c, &r, &["timer", "remove"]);
    assert!(
        out.contains("✓ removed dev.drover.update") && out.contains("✓ removed dev.drover.clean"),
        "{out}"
    );
}

#[test]
fn a_config_with_telegram_keys_loads_with_a_warning() {
    let text = format!("{FLEET}\n[telegram]\nchat = -100123\nowner = 4242\n");
    let c = ctx(&tmpdir("upd-telegram"), &text);
    let (code, out) = run(&c, &Fake::default(), &["doctor", "--config-only"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("✓ config — "), "{out}");
    let warn = "! config — [telegram] is removed (#557): drover has no Telegram bridge now; drop the table";
    assert!(out.lines().any(|l| l == warn), "{out}");
}

#[test]
fn a_job_launchd_refuses_is_rebootstrapped_after_the_swap() {
    use std::os::unix::fs::MetadataExt;
    let (c, live) = setup("upd-refused");
    let uid = std::fs::metadata(&c.home).unwrap().uid();
    let mac = cfg!(target_os = "macos");
    let ctl = |label: &str, unit: &str| -> [String; 3] {
        if mac {
            [
                format!("launchctl print gui/{uid}/{label}"),
                format!("launchctl bootout gui/{uid}/{label}"),
                format!(
                    "launchctl bootstrap gui/{uid} {}",
                    c.home.join(format!("Library/LaunchAgents/{label}.plist")).display()
                ),
            ]
        } else {
            ["is-active", "disable --now", "enable --now"].map(|a| format!("systemctl --user {a} {label}.{unit}"))
        }
    };
    let [clean_probe, clean_down, clean_up] = ctl("dev.drover.clean", "timer");
    let [serve_probe, serve_down, serve_up] = ctl("dev.drover.serve", "service");
    let r = Fake::default()
        .reply(&clean_probe, "job state = spawn failed\n")
        .reply(&serve_probe, "last exit reason = OS_REASON_CODESIGNING\n")
        .reply(&clean_down, "")
        .reply(&clean_up, "")
        .reply(&serve_down, "")
        .reply(&serve_up, "");
    let r = Fake {
        replies: [r.replies, release(base(&c, &live), "main", SHA, &live).replies].concat(),
        ..Fake::default()
    };
    let (code, out) = run(&c, &Disk(&r), &["self-update"]);
    assert_eq!(code, 0, "{out}");
    let at = |p: &str| {
        r.index(p)
            .unwrap_or_else(|| panic!("{p} not called: {:?}", r.calls.borrow()))
    };
    assert!(at(&clean_down) < at(&clean_up), "{:?}", r.calls.borrow());
    assert!(at(&serve_down) < at(&serve_up), "{:?}", r.calls.borrow());
    assert!(
        !r.called("launchctl kickstart") && !r.called("systemctl --user restart"),
        "re-bootstrapped, not kickstarted: {:?}",
        r.calls.borrow()
    );
    let tick = ctl("dev.drover.tick", "timer");
    assert!(!r.called(&tick[1]), "a healthy job is left alone");
    assert!(out.contains("dev.drover.clean"), "{out}");
}
