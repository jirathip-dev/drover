mod common;
use common::*;
use drover::config::Config;
use drover::events::{read, Event};
use drover::hosts::{cached, place};
use serde_json::json;
use std::path::Path;

const NOW: u64 = 1_000_000;

const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";

const DROVER: &str = "[fleet.drover]\nrepo = \"jirathip-dev/herdsman\"\nlocal = \"/x/drover\"\nintegration = \"staging\"\n\n[fleet.drover.role.impl]\nharness = \"hermes\"\n";

fn config(extra: &str) -> String {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    format!("{base}\n{DROVER}\n[hosts.mini]\ntags = [\"fast\"]\n\n[hosts.boat2]\nmachine = \"boat2\"\n\n[hosts.boat3]\nmachine = \"boat3\"\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n{extra}")
}

fn setup(name: &str, extra: &str, bazzite_up: bool, seen: u64) -> drover::Ctx {
    let c = ctx(&tmpdir(name), &config(extra));
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos", "arch:arm64", "xcode"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux", "arch:x86_64", "docker"], "home": "/home/me", "probed": NOW,
                    "seen": NOW - seen, "up": bazzite_up, "load": 0.1, "mem": 12.0, "disk": 200.0},
        "boat2": {"tags": ["os:linux", "arch:x86_64"], "home": "/home/me", "probed": NOW, "seen": NOW, "up": true},
        "boat3": {"tags": ["os:linux", "arch:x86_64"], "home": "/home/me", "probed": NOW, "seen": NOW, "up": true},
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

const STATUS: &str = "'sh' '-c' 'for x in";
const NEW: &str = "1111111111111111111111111111111111111111";
const OLD: &str = "0000000000000000000000000000000000000000";

fn ver(sha: &str) -> String {
    format!("drover 0.1.0 ({sha})")
}

fn ssh(m: &str) -> String {
    SSH.replace("bazzite", m)
}

fn status(f: Fake, m: &str, live: &str, staged: Option<&str>) -> Fake {
    let staged = staged.map_or("-".into(), ver);
    f.reply(&format!("{}{STATUS}", ssh(m)), &format!("{}\n{staged}\n", ver(live)))
}

fn synced(status: &str) -> Fake {
    healthy(fake(0.1, 0.1))
        .reply(&format!("{SSH}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\ndocker\n")
        .reply("sh -c uname -s", "Darwin\narm64\n/Users/me\n")
        .reply(&format!("{SSH}{STATUS}"), status)
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == kind)
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

/// The self-update's fake: main at NEW with its release, the main host staging and swapping, and each boat's builds.
fn rollout(c: &drover::Ctx, boats: &[(&str, &str, Option<&str>)]) -> Fake {
    let tag = drover::release::tag("main", NEW);
    let sums = format!(
        "{}  drover-x86_64-unknown-linux-gnu\n{}  drover-aarch64-apple-darwin\n",
        "b".repeat(64),
        "a".repeat(64)
    );
    let f = Fake::default()
        .reply("git -C /x/drover rev-parse origin/main", &format!("{NEW}\n"))
        .reply("git -C /x/drover fetch", "")
        .reply(&format!("gh release download {tag} "), &sums)
        .reply(&format!("sh -c ( n='{}.new'", c.exe), &ver(NEW))
        .reply(&format!("{}.new doctor --config-only", c.exe), "")
        .reply(&format!("{} profiles sync", c.exe), "")
        .reply("launchctl", "")
        .reply("systemctl", "");
    boats.iter().fold(f, |f, (m, live, staged)| {
        status(f, m, live, *staged)
            .reply(&format!("{}'sh' '-c' '( n=", ssh(m)), &ver(NEW))
            .reply(&format!("{}'sh' '-c' 'export PATH", ssh(m)), "")
    })
}

fn swapped(f: &Fake, m: &str) -> Option<usize> {
    f.calls
        .borrow()
        .iter()
        .position(|l| l.starts_with(&format!("{}'sh' '-c' 'export PATH", ssh(m))) && l.contains("mv -f"))
}

#[test]
fn a_lagging_host_holds_the_swap_30_minutes_then_the_rest_swap_and_placement_passes_it_over() {
    let mut c = setup("rollout-lag", "", true, 0);
    let dir = c.state.parent().unwrap().parent().unwrap().canonicalize().unwrap();
    c.exe = dir.join("bin/drover").display().to_string();
    version_bin(Path::new(&c.exe), &ver(OLD));
    run(
        &c,
        &fake(2.0, 0.1),
        &["queue", "sendmeter", "add", "12", "--needs", "docker"],
    );
    let at = |mins: u64| drover::Ctx {
        now: NOW + mins * 60,
        ..c.clone()
    };
    let pass = |mins: u64, boats: &[(&str, &str, Option<&str>)]| {
        let f = rollout(&c, boats);
        let (code, out) = run(&at(mins), &Disk(&f), &["self-update"]);
        (f, code, out)
    };
    let up = [("boat2", OLD, None), ("boat3", OLD, None)];
    let (f, code, out) = pass(0, &up);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("waiting up to 30 min for bazzite"), "{out}");
    assert!(
        f.called(&format!("{}'sh' '-c' '( n=", ssh("boat2"))) && f.called(&format!("{}'sh' '-c' '( n=", ssh("boat3")))
    );
    let staged = [("boat2", OLD, Some(NEW)), ("boat3", OLD, Some(NEW))];
    let (f, code, out) = pass(29, &staged);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("waiting up to 1 min for bazzite"), "{out}");
    for (f, m) in [(&f, "boat2"), (&f, "boat3")] {
        assert_eq!(swapped(f, m), None, "no swap before the lag limit");
    }
    assert!(Path::new(&format!("{}.new", c.exe)).exists() && std::fs::read_to_string(&c.exe).unwrap().contains(OLD));

    let (f, code, out) = pass(31, &staged);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("✓ installed 1111111") && out.contains("left behind: bazzite"),
        "{out}"
    );
    let (b2, b3) = (swapped(&f, "boat2").unwrap(), swapped(&f, "boat3").unwrap());
    let mini = f.index(&format!("{} profiles sync", c.exe)).unwrap();
    assert!(b2 < mini && b3 < mini, "the main host swaps last");
    assert!(std::fs::read_to_string(&c.exe).unwrap().contains(NEW));
    let order: Vec<String> = read(&c.state)
        .unwrap()
        .iter()
        .map(|e| e.kind.clone())
        .filter(|k| k.starts_with("self-update") || k.starts_with("host-"))
        .collect();
    let i = |k: &str| order.iter().position(|o| o == k).unwrap();
    assert!(
        i("self-update-staging") < i("self-update-swapping") && i("self-update-swapping") < i("self-updated"),
        "{order:?}"
    );
    let ok: Vec<_> = kinds(&c, "host-sync")
        .iter()
        .filter(|e| e.val("ok") == true)
        .map(|e| e.str("host").unwrap().to_string())
        .collect();
    assert_eq!(ok, ["boat2", "boat3"]);
    let behind = kinds(&c, "host-behind");
    assert_eq!(behind.len(), 1, "{behind:?}");
    assert_eq!(
        (behind[0].str("host"), behind[0].str("wants")),
        (Some("bazzite"), Some(NEW))
    );

    let c40 = at(40);
    let e = placed(&c40, &fake(2.0, 0.1), 12, "impl", None).unwrap_err();
    assert!(e.starts_with("HOST_UPDATING: bazzite: drover there is ?"), "{e}");
    let (_, code, out) = pass(40, &[("boat2", NEW, None), ("boat3", NEW, None)]);
    assert_eq!((code, out.as_str()), (0, "= up to date @ 1111111\n"));
    assert_eq!(kinds(&c, "host-behind").len(), 1, "logged once");
    let log = serde_json::to_string(&read(&c.state).unwrap().to_vec()).unwrap();
    assert!(!log.contains("ACP_UNGUARDED") && !log.contains("NO_HOST"), "{log}");

    let back = [("boat2", NEW, None), ("boat3", NEW, None), ("bazzite", OLD, None)];
    let (f, code, out) = pass(45, &back);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("✓ caught up bazzite to 1111111"), "{out}");
    assert!(swapped(&f, "bazzite").is_some() && swapped(&f, "boat2").is_none());
    assert!(
        f.count(&format!("{} profiles sync", c.exe)) == 0,
        "the main host is left alone"
    );
    assert_eq!(
        placed(&at(46), &fake(2.0, 0.1), 12, "impl", None).as_deref(),
        Ok("bazzite")
    );
}

#[test]
fn hosts_refresh_reads_each_hosts_build_and_never_installs() {
    let c = setup("hosts-build-read", "", true, 0);
    let _sh = FakeShed::attached(&c);
    let bazzite = FakeShed::new();
    let r = synced(&format!("{}\n-\n", ver(OLD)));
    let (code, out) = run(&c, &two(&r, &bazzite), &["hosts", "refresh"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!("· drover {OLD} ≠ {} ·", env!("DROVER_SHA"))),
        "{out}"
    );
    let calls = r.calls.borrow().join("\n");
    assert!(
        !calls.contains("cargo install") && !calls.contains("drover.new") && !calls.contains("mv -f"),
        "{calls}"
    );
    assert!(kinds(&c, "host-sync").is_empty());
}

#[test]
fn an_unreachable_or_switched_off_host_costs_no_build_read() {
    let c = setup("hosts-sync-off", "", true, 0);
    let r = fake(0.1, 0.1).reply("sh -c uname -s", "Darwin\narm64\n/Users/me\n");
    run(&c, &r, &["hosts", "refresh"]);
    assert!(!cached(&c)["bazzite"].up);
    assert!(!r.calls.borrow().iter().any(|l| l.contains(STATUS)));
    let c = setup("hosts-sync-disabled", "enabled = false\n", true, 0);
    let _sh = FakeShed::attached(&c);
    let (bazzite, r) = (FakeShed::new(), synced(&format!("{}\n-\n", ver(OLD))));
    run(&c, &two(&r, &bazzite), &["hosts", "refresh"]);
    assert!(!r.called(&format!("{SSH}{STATUS}")));
}

#[test]
fn a_changed_build_rereads_the_host_health_in_the_same_pass_so_spawn_places_there() {
    let c = setup("hosts-sync-landed", "", true, 0);
    let _sh = FakeShed::attached(&c);
    let stale = format!(
        "✗ acp guard claude — drover there is drover 0.1.0 (0ld), not drover {}",
        drover::VERSION
    );
    reprobe(&c, &[&stale]);
    let mut all = cached(&c);
    let b = all.get_mut("bazzite").unwrap();
    (b.probed, b.drover) = (NOW, OLD.into());
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
    let bazzite = FakeShed::new();
    let landed = synced(&format!("{}\n-\n", ver(env!("DROVER_SHA"))));
    let (code, out) = run(&c, &two(&landed, &bazzite), &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        landed.called(&format!("{SSH}'git' '--no-replace-objects' 'ls-remote'")),
        "health ran again"
    );
    assert_eq!(cached(&c)["bazzite"].health, Vec::<String>::new());
    assert_eq!(placed(&c, &fake(2.0, 0.1), 12, "impl", None).as_deref(), Ok("bazzite"));
}
