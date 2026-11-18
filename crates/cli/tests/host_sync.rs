mod common;
use common::*;
use drover::config::Config;
use drover::events::{read, Event};
use drover::hosts::{cached, place};
use serde_json::json;

const NOW: u64 = 1_000_000;

const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";

fn config(extra: &str) -> String {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    format!("{base}\n[hosts.mini]\ntags = [\"fast\"]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n{extra}")
}

fn setup(name: &str, extra: &str, bazzite_up: bool, seen: u64) -> drover::Ctx {
    let c = ctx(&tmpdir(name), &config(extra));
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos", "arch:arm64", "xcode"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux", "arch:x86_64", "docker"], "home": "/home/me", "probed": NOW,
                    "seen": NOW - seen, "up": bazzite_up, "load": 0.1, "mem": 12.0, "disk": 200.0},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    c
}

fn fake(mini: f64, remote: f64) -> Fake {
    Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n vm.loadavg", &format!("{{ {} 1 1 }}", mini * 4.0))
        .reply("sysctl -n hw.ncpu", "4")
        .reply(
            &format!("{SSH}'cat' '/proc/loadavg'"),
            &format!("{} 1 1 1/9 42", remote * 4.0),
        )
        .reply(&format!("{SSH}'nproc'"), "4")
        .reply(
            &format!("{SSH}'/opt/drover/bin/drover' 'profiles' 'sync' '--profile'"),
            "",
        )
}

fn two<'a>(f: &'a Fake, bazzite: &'a FakeShed) -> OverSsh<'a> {
    OverSsh {
        f,
        hosts: vec![("bazzite", bazzite)],
    }
}

fn healthy(f: Fake) -> Fake {
    f.reply(&format!("{SSH}'git' '--no-replace-objects' 'ls-remote'"), "abc\tHEAD\n")
        .reply(&format!("{SSH}'sh' '-lc'"), "")
        .reply(
            &format!("{SSH}'/opt/drover/bin/drover' '--version'"),
            &format!("drover {}\n", drover::VERSION),
        )
}

fn placed(c: &drover::Ctx, r: &Fake, issue: u64, role: &str, force: Option<&str>) -> Result<String, String> {
    let cfg = Config::load(&c.config).unwrap();
    let events = read(&c.state).unwrap();
    let (mini, bazzite) = (FakeShed::new(), FakeShed::new());
    match place(
        c,
        &cfg,
        &mini.on(&two(r, &bazzite)),
        &events,
        ("sendmeter", issue, role),
        (force, &|_| None),
    ) {
        Ok(p) => {
            let p = p.unwrap();
            p.refusal
                .map_or(Ok(p.name), |(code, why)| Err(format!("{code}: {why}")))
        }
        Err(e) => Err(format!("{e:#}")),
    }
}

const STATUS: &str = "'sh' '-c' '\"$1\" --version";

const START: &str = "'sh' '-c' 'V=";

fn synced(status: &str) -> Fake {
    healthy(fake(0.1, 0.1))
        .reply(&format!("{SSH}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\ndocker\n")
        .reply("sh -c uname -s", "Darwin\narm64\n/Users/me\n")
        .reply(&format!("{SSH}{STATUS}"), status)
        .reply(&format!("{SSH}{START}"), "")
}

fn syncs(c: &drover::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "host-sync")
        .cloned()
        .collect()
}

fn reprobe(c: &drover::Ctx, health: &[&str]) {
    let mut all = cached(c);
    let b = all.get_mut("bazzite").unwrap();
    b.probed = 0;
    b.health = health.iter().map(|h| (*h).to_string()).collect();
    std::fs::write(
        c.state.with_file_name("hosts.json"),
        serde_json::to_string(&all).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_mismatched_host_gets_one_install_at_this_sha_then_profiles_sync_and_a_matching_one_none() {
    let c = setup("hosts-sync", "", true, 0);
    let _sh = FakeShed::attached(&c);
    let bazzite = FakeShed::new();
    let (sha, old) = (env!("DROVER_SHA"), "drover 0.1.0 (0ld)\n");
    let r = synced(old);
    let (code, out) = run(&c, &two(&r, &bazzite), &["hosts", "refresh"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("· drover 0ld ≠ {sha} ·")), "{out}");
    let repo = "https://github.com/jirathip-dev/herdsman";
    let install = format!("cargo install --git {repo} --rev {sha} --locked --root '\\''/opt/drover'\\'' --force");
    let sync = "|| '\\''/opt/drover/bin/drover'\\'' profiles sync; }; }; echo \"$? ";
    let starts: Vec<String> = r.calls.borrow().iter().filter(|l| l.contains(START)).cloned().collect();
    assert_eq!(starts.len(), 1, "{starts:#?}");
    assert!(
        starts[0].contains(&install) && starts[0].contains(sync),
        "{}",
        starts[0]
    );
    assert!(
        starts[0].ends_with("'drover 0.1.0 (0ld)'"),
        "the from version: {}",
        starts[0]
    );
    let busy = synced(&format!("{old}busy\n"));
    run(&c, &two(&busy, &bazzite), &["hosts", "refresh"]);
    assert!(busy.called(&format!("{SSH}{STATUS}")) && !busy.called(&format!("{SSH}{START}")));
    let landed = synced(&format!(
        "drover {}\ndone 0 {sha} drover 0.1.0 (0ld)\n",
        drover::VERSION
    ));
    let (_, out) = run(&c, &two(&landed, &bazzite), &["hosts", "refresh"]);
    assert!(out.contains(&format!("· drover {sha} ✓ ·")), "{out}");
    assert!(!landed.called(&format!("{SSH}{START}")));
    let ev = syncs(&c);
    assert_eq!(ev.len(), 1, "{ev:?}");
    assert_eq!(
        json!(ev[0].data),
        json!({"host": "bazzite", "from": "0ld", "to": sha, "ok": true})
    );
}

#[test]
fn a_failed_install_is_logged_alerted_once_and_retried_each_tick() {
    let c = setup("hosts-sync-fail", "", true, 0);
    let _sh = FakeShed::attached(&c);
    let bazzite = FakeShed::new();
    let sha = env!("DROVER_SHA");
    let failed = format!("drover 0.1.0 (0ld)\ndone 101 {sha} drover 0.1.0 (0ld)\n");
    for _ in 0..2 {
        let r = synced(&failed);
        run(&c, &two(&r, &bazzite), &["check"]);
        assert_eq!(r.count(&format!("{SSH}{START}")), 1, "retried");
    }
    let ev = syncs(&c);
    assert_eq!(ev.len(), 2, "{ev:?}");
    assert!(ev.iter().all(|e| e.val("ok") == false && e.str("from") == Some("0ld")));
    let alerts = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "host-sync-failed")
        .count();
    assert_eq!(alerts, 1, "once per state");
}

#[test]
fn an_unreachable_or_switched_off_host_costs_no_sync_ssh() {
    let c = setup("hosts-sync-off", "", true, 0);
    let r = fake(0.1, 0.1).reply("sh -c uname -s", "Darwin\narm64\n/Users/me\n");
    run(&c, &r, &["hosts", "refresh"]);
    assert!(!cached(&c)["bazzite"].up);
    assert!(!r
        .calls
        .borrow()
        .iter()
        .any(|l| l.contains(".drover-sync") || l.contains(STATUS)));
    let c = setup("hosts-sync-disabled", "enabled = false\n", true, 0);
    let _sh = FakeShed::attached(&c);
    let (bazzite, r) = (FakeShed::new(), synced("drover 0.1.0 (0ld)\n"));
    run(&c, &two(&r, &bazzite), &["hosts", "refresh"]);
    assert!(!r.called(&format!("{SSH}{STATUS}")) && syncs(&c).is_empty());
}

#[test]
fn a_landed_sync_rereads_the_host_health_in_the_same_pass_so_spawn_places_there() {
    let c = setup("hosts-sync-landed", "", true, 0);
    let _sh = FakeShed::attached(&c);
    let stale = format!(
        "✗ acp guard claude — drover there is drover 0.1.0 (0ld), not drover {}",
        drover::VERSION
    );
    reprobe(&c, &[&stale]);
    let mut all = cached(&c);
    all.get_mut("bazzite").unwrap().probed = NOW;
    std::fs::write(
        c.state.with_file_name("hosts.json"),
        serde_json::to_string(&all).unwrap(),
    )
    .unwrap();
    run(
        &c,
        &fake(2.0, 0.1),
        &["queue", "sendmeter", "add", "12", "--needs", "docker"],
    );
    let e = placed(&c, &fake(2.0, 0.1), 12, "impl", None).unwrap_err();
    assert!(e.starts_with("NO_HOST: bazzite: unhealthy: ✗ acp guard"), "{e}");
    let sha = env!("DROVER_SHA");
    let bazzite = FakeShed::new();
    let landed = synced(&format!(
        "drover {}\ndone 0 {sha} drover 0.1.0 (0ld)\n",
        drover::VERSION
    ));
    let (code, out) = run(&c, &two(&landed, &bazzite), &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        landed.called(&format!("{SSH}'git' '--no-replace-objects' 'ls-remote'")),
        "health ran again"
    );
    assert_eq!(cached(&c)["bazzite"].health, Vec::<String>::new());
    assert_eq!(placed(&c, &fake(2.0, 0.1), 12, "impl", None).as_deref(), Ok("bazzite"));
}

#[test]
fn a_skipped_sync_logs_why_once_per_streak() {
    let c = setup("hosts-sync-skipped", "", true, 0);
    let _sh = FakeShed::attached(&c);
    let bazzite = FakeShed::new();
    let skipped = |c: &drover::Ctx| {
        let all = read(&c.state).unwrap();
        all.iter()
            .filter(|e| e.kind == "host-sync-skipped")
            .cloned()
            .collect::<Vec<_>>()
    };
    let probe = format!("{SSH}'sh' '-c' 'uname");
    for want in [1, 1] {
        reprobe(&c, &[]);
        let r = synced("drover 0.1.0 (0ld)\n").once(&probe, 124, "");
        run(&c, &two(&r, &bazzite), &["check"]);
        assert!(!r.called(&format!("{SSH}{START}")));
        assert_eq!(skipped(&c).len(), want);
    }
    assert!(
        skipped(&c)[0]
            .str("reason")
            .is_some_and(|w| w.contains("not sent (host offline this pass)")),
        "{:?}",
        skipped(&c)[0]
    );
    let r = synced("drover 0.1.0 (0ld)\n");
    run(&c, &two(&r, &bazzite), &["check"]);
    assert_eq!(r.count(&format!("{SSH}{START}")), 1);
    reprobe(&c, &[]);
    let r = synced("drover 0.1.0 (0ld)\n").once(&probe, 124, "");
    run(&c, &two(&r, &bazzite), &["check"]);
    assert_eq!(skipped(&c).len(), 2, "a new streak logs again");
}

fn install_on(name: &str, os: &str, asset: Option<(&str, &str)>) -> (String, String, String) {
    let d = tmpdir(name);
    let (bin, rel, root, tmp) = (d.join("bin"), d.join("rel"), d.join("root"), d.join("tmp"));
    for p in [&bin, &rel, &tmp] {
        std::fs::create_dir_all(p).unwrap();
    }
    let a = "drover-x86_64-unknown-linux-gnu";
    if let Some((body, sum)) = asset {
        std::fs::write(rel.join(a), body).unwrap();
        std::fs::write(rel.join(format!("{a}.sha256")), format!("{sum}  {a}\n")).unwrap();
    }
    let (s, m) = os.split_once('/').unwrap();
    let stubs = [
        ("uname", format!("[ \"$1\" = -s ] && echo {s} || echo {m}")),
        (
            "curl",
            format!(
                "echo \"$@\" >> '{0}/curl.log'; f='{1}'/\"${{4##*/}}\"; [ -f \"$f\" ] || exit 22; cp \"$f\" \"$3\"",
                d.display(),
                rel.display()
            ),
        ),
        (
            "cargo",
            format!(
                "echo \"$@\" > '{0}/cargo.log'; mkdir -p '{1}/bin' && echo cargo-built > '{1}/bin/drover'",
                d.display(),
                root.display()
            ),
        ),
    ];
    for (n, body) in stubs {
        std::fs::write(bin.join(n), format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut perm = std::fs::metadata(bin.join(n)).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        std::fs::set_permissions(bin.join(n), perm).unwrap();
    }
    let line = drover::hosts::install_line(&root.display().to_string(), "c0ffee");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let o = std::process::Command::new("sh")
        .args(["-c", &line])
        .env("PATH", path)
        .env("TMPDIR", &tmp)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(root.join("bin/herdsman").symlink_metadata().is_ok());
    assert_eq!(
        std::fs::read_dir(&tmp).unwrap().count(),
        0,
        "the download dir is removed"
    );
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).unwrap_or_default();
    (
        read(root.join("bin/drover")),
        read(d.join("curl.log")) + &read(d.join("cargo.log")),
        String::from_utf8_lossy(&o.stderr).into(),
    )
}

#[test]
fn a_linux_x86_64_host_installs_main_cis_checked_binary_and_falls_back_to_cargo() {
    let body = "prebuilt\n";
    let real = "fa5571ee502fead0fb341637a5b80546c10f4604d1aa3e770d9de0cea669c21b";
    let bad = "0".repeat(64);
    let url = "https://github.com/jirathip-dev/herdsman/releases/download/c0ffee/drover-x86_64-unknown-linux-gnu";
    let (installed, calls, _) = install_on("install-prebuilt", "Linux/x86_64", Some((body, real)));
    assert_eq!(installed, body, "{calls}");
    assert!(
        calls.contains(url) && calls.contains(&format!("{url}.sha256")) && !calls.contains("install --git"),
        "{calls}"
    );
    let (installed, calls, why) = install_on("install-bad-sum", "Linux/x86_64", Some((body, &bad)));
    assert_eq!(installed, "cargo-built\n", "a bad checksum never installs: {calls}");
    assert!(
        why.contains("checksum mismatch")
            && calls.contains("install --git https://github.com/jirathip-dev/herdsman --rev c0ffee"),
        "{why} {calls}"
    );
    let (installed, calls, why) = install_on("install-no-asset", "Linux/x86_64", None);
    assert_eq!(installed, "cargo-built\n");
    assert!(why.contains("no release asset") && calls.contains(url), "{why} {calls}");
    let (installed, calls, why) = install_on("install-mac", "Darwin/arm64", Some((body, real)));
    assert_eq!(installed, "cargo-built\n");
    assert!(
        why.contains("no prebuilt binary for Darwin/arm64") && !calls.contains(url),
        "{why} {calls}"
    );
}
