mod common;
use common::*;
use drover::config::Config;
use drover::events::read;
use drover::hosts::place;
use serde_json::json;

const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";

fn setup() -> drover::Ctx {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let cfg = format!("{base}\n[hosts.mini]\ntags = [\"fast\"]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir("repro-host"), &cfg);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let now = 1_000_000;
    let cache = json!({
        "mini": {"tags": ["os:macos", "arch:arm64"], "home": "/Users/me", "probed": now, "seen": now, "up": true},
        "bazzite": {"tags": ["os:linux", "arch:x86_64"], "home": "/home/me", "probed": now, "seen": now, "up": true,
                    "load": 0.1, "mem": 12.0, "disk": 200.0},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    c
}

fn fake(diff: &str) -> Fake {
    Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply(&format!("{SSH}'cat' '/proc/loadavg'"), "8 1 1 1/9 42")
        .reply(&format!("{SSH}'nproc'"), "4")
        .reply("gh pr diff issue-12", diff)
}

fn placed(c: &drover::Ctx, r: &Fake, role: &str) -> String {
    let cfg = Config::load(&c.config).unwrap();
    let events = read(&c.state).unwrap();
    let (mini, bazzite) = (FakeShed::new(), FakeShed::new());
    let over = OverSsh {
        f: r,
        hosts: vec![("bazzite", &bazzite)],
    };
    let p = place(
        c,
        &cfg,
        &mini.on(&over),
        &events,
        ("sendmeter", 12, role),
        (None, &|_| None),
    );
    let p = p.unwrap().unwrap();
    assert_eq!(p.refusal, None);
    p.name
}

#[test]
fn a_repro_issues_rev_runs_on_the_os_its_target_gated_test_needs() {
    let c = setup();
    let gated = fake("+++ b/src/a.rs\n+#[cfg(target_os = \"linux\")]\n+#[test]\n+fn reaps() {}\n");
    assert_eq!(placed(&c, &gated, "rev"), "mini", "not --repro");
    run(&c, &gated, &["queue", "sendmeter", "add", "12", "--repro"]);
    assert_eq!(placed(&c, &gated, "impl"), "mini", "impl: least busy");
    assert_eq!(placed(&c, &gated, "rev"), "bazzite", "rev: os:linux");
    assert_eq!(placed(&c, &fake("+fn plain() {}\n"), "rev"), "mini", "no gated test");
}
