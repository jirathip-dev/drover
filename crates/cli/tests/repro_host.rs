mod common;
use common::*;
use drover::config::Config;
use drover::events::read;
use drover::hosts::place;
use serde_json::json;

const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";

fn setup() -> drover::Ctx {
    hosts_with(
        "repro-host",
        "",
        ["os:macos", "arch:arm64"],
        ["os:linux", "arch:x86_64"],
    )
}

fn hosts_with(name: &str, needs: &str, mini: [&str; 2], bazzite: [&str; 2]) -> drover::Ctx {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let base = base.replace(
        "integration = \"staging\"",
        &format!("integration = \"staging\"\n{needs}"),
    );
    let cfg = format!("{base}\n[hosts.mini]\ntags = [\"fast\"]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir(name), &cfg);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let now = 1_000_000;
    let cache = json!({
        "mini": {"tags": mini, "home": "/Users/me", "probed": now, "seen": now, "up": true},
        "bazzite": {"tags": bazzite, "home": "/home/me", "probed": now, "seen": now, "up": true,
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

fn place_on(c: &drover::Ctx, r: &Fake, role: &str) -> anyhow::Result<Option<drover::hosts::Placed>> {
    let cfg = Config::load(&c.config).unwrap();
    let events = read(&c.state).unwrap();
    let (mini, bazzite) = (FakeShed::new(), FakeShed::new());
    let over = OverSsh {
        f: r,
        hosts: vec![("bazzite", &bazzite)],
    };
    place(
        c,
        &cfg,
        &mini.on(&over),
        &events,
        ("sendmeter", 12, role),
        (None, &|_| None),
    )
}

fn placed(c: &drover::Ctx, r: &Fake, role: &str) -> String {
    let p = place_on(c, r, role).unwrap().unwrap();
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

const UI_TEST: &str = "+++ b/crates/cli/tests/ui/compose.mjs\n+test('page_drafts', async () => {});\n";

#[test]
fn a_repro_issues_rev_with_a_ui_test_runs_on_the_host_with_webkit_whatever_the_fleets_needs() {
    let c = hosts_with(
        "repro-webkit",
        "needs = [\"dev\"]",
        ["os:macos", "webkit"],
        ["os:linux", "dev"],
    );
    let ui = fake(UI_TEST);
    assert_eq!(placed(&c, &ui, "rev"), "bazzite", "not --repro: the fleet's dev host");
    run(&c, &ui, &["queue", "sendmeter", "add", "12", "--repro"]);
    assert_eq!(placed(&c, &ui, "impl"), "bazzite", "impl: the fleet's needs");
    let p = place_on(&c, &ui, "rev").unwrap().unwrap();
    assert_eq!((p.name.as_str(), p.refusal), ("mini", None));
    let why = p.why.unwrap_or_default();
    assert!(
        why.contains("webkit") && why.contains("crates/cli/tests/ui/compose.mjs"),
        "{why}"
    );
    assert_eq!(
        placed(&c, &fake("+++ b/src/a.rs\n+fn plain() {}\n"), "rev"),
        "bazzite",
        "no ui test"
    );
}

#[test]
fn no_enabled_host_with_webkit_starts_no_rev_and_files_one_owner_card() {
    let c = hosts_with(
        "repro-no-webkit",
        "needs = [\"dev\"]",
        ["os:macos", "arch:arm64"],
        ["os:linux", "dev"],
    );
    let ui = fake(UI_TEST).reply("gh issue view 12", r#"{"title":"Fix","body":"AC"}"#);
    run(&c, &ui, &["queue", "sendmeter", "add", "12", "--repro"]);
    for _ in 0..2 {
        let (code, out) = run(&c, &ui, &["spawn", "sendmeter", "12", "rev"]);
        assert!(
            code == 3 && out.contains("REFUSED NO_HOST") && out.contains("webkit"),
            "{out}"
        );
    }
    let ev = read(&c.state).unwrap();
    assert!(!ev.iter().any(|e| e.kind == "spawn"), "no rev lane starts");
    let nh: Vec<_> = ev.iter().filter(|e| e.kind == "needs-human").collect();
    assert_eq!(nh.len(), 1, "{nh:?}");
    assert!(nh[0].str("reason").is_some_and(|r| r.contains("webkit")), "{nh:?}");
}
