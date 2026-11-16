mod common;
use common::*;
use drover::events::{read, Event};
use drover::shed::proto::{Response, BUILD, PROTOCOL, VERSION};

const FLEET: &str = "[fleet.drover]\nrepo = \"jirathip-dev/herdsman\"\nlocal = \"/x/drover\"\nintegration = \"staging\"\n\n[fleet.drover.role.impl]\nharness = \"claude\"\n";
const SHA: &str = "abcdef1234567890abcdef1234567890abcdef12";
const SHA2: &str = "1234567890abcdef1234567890abcdef12345678";

fn protocol_0() -> FakeShed {
    let b = FakeShed::start("0.0.0", vec![]);
    let mut s = b.lock();
    let h = s.hello.as_mut().unwrap();
    (h.build, h.protocol) = (String::new(), 0);
    drop(s);
    b
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

fn setup(name: &str, b: &FakeShed) -> (drover::Ctx, std::path::PathBuf) {
    let d = tmpdir(name);
    let mut c = ctx(&d, &b.config(FLEET));
    std::fs::create_dir_all(d.join("bin")).unwrap();
    std::fs::write(d.join("bin/drover"), "OLD").unwrap();
    c.exe = d.join("bin/drover").to_string_lossy().into();
    let built = c.state.with_file_name("update/target/release/drover");
    std::fs::create_dir_all(built.parent().unwrap()).unwrap();
    std::fs::write(&built, "NEW").unwrap();
    let live = std::fs::canonicalize(&c.exe).unwrap();
    (c, live)
}

fn host(sha: &str, new: &str, gate: (i32, &str)) -> Fake {
    let checks = r#"[{"__typename": "CheckRun", "name": "rust", "status": "COMPLETED", "conclusion": "SUCCESS"}]"#;
    Fake::default()
        .reply("git -C /x/drover rev-parse origin/main", &format!("{sha}\n"))
        .reply("git -C", "")
        .reply(
            &format!("gh api repos/jirathip-dev/herdsman/commits/{sha}/check-runs"),
            checks,
        )
        .reply("cargo build --release --locked -p drover", "")
        .reply("which codesign", "/usr/bin/codesign\n")
        .reply("sh ", "")
        .once(&format!("{new} doctor --config-only"), gate.0, gate.1)
}

#[test]
fn doctor_shows_the_local_shed_against_drovers_and_fails_on_another_protocol() {
    let ok = FakeShed::new();
    let c = ctx(&tmpdir("shedv-doctor-ok"), &ok.config(FLEET));
    let (code, out) = run(&c, &Fake::default(), &["doctor", "--config-only"]);
    let ours = format!("{VERSION} ({BUILD}) protocol {PROTOCOL}");
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!("✓ shed — server {ours}, drover speaks {ours}")),
        "{out}"
    );

    let old = protocol_0();
    let c = ctx(&tmpdir("shedv-doctor-old"), &old.config(FLEET));
    let (code, out) = run(&c, &Fake::default(), &["doctor", "--config-only"]);
    assert_eq!(code, 1, "{out}");
    let want = format!(
        "✗ shed — SHED_VERSION: drover speaks shed {ours}, the server is 0.0.0 (unknown) protocol 0 → cargo install --git {} --rev {} --locked --force shed, then restart shed serve",
        drover::shed::git(),
        drover::shed::REV
    );
    assert!(out.lines().any(|l| l == want), "{out}");
}

#[test]
fn doctor_gives_each_enabled_remote_host_a_shed_line_or_unreachable() {
    let b = FakeShed::new();
    let hosts = "[hosts.far]\nmachine = \"far\"\n\n[hosts.gone]\nmachine = \"gone\"\n\n\
                 [hosts.off]\nmachine = \"off\"\nenabled = false\n";
    let c = ctx(&tmpdir("shedv-hosts"), &b.config(&format!("{FLEET}\n{hosts}")));
    let ssh = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C \
               -oControlPersist=10m far -- ";
    let mut old = drover::shed::proto::Hello::ours();
    (old.version, old.build, old.protocol) = ("0.0.0".into(), String::new(), 0);
    let hello = serde_json::json!(Response::ok(serde_json::json!(0), serde_json::json!(old)));
    let ls = serde_json::json!(Response::ok(serde_json::json!(1), serde_json::json!([])));
    let f = Fake::default()
        .reply(&format!("{ssh}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\n")
        .reply("herdr --machine far agent list", NO_AGENTS)
        .reply(
            &format!("{ssh}'shed' 'connect' '--autostart'"),
            &format!("{hello}\n{ls}\n"),
        );
    let (_, out) = run(&c, &f, &["doctor"]);
    let lines: Vec<_> = out
        .lines()
        .filter(|l| l.contains(" shed —") && !l.contains("PATH"))
        .collect();
    let ours = format!("{VERSION} ({BUILD}) protocol {PROTOCOL}");
    let far = format!(
        "✗ host far shed — SHED_VERSION: drover speaks shed {ours}, the server is 0.0.0 (unknown) protocol 0 → "
    );
    assert_eq!(lines.len(), 3, "{out}");
    assert!(lines[0].starts_with("✓ shed — server "), "{out}");
    assert!(lines[1].starts_with(&far), "{out}");
    assert_eq!(lines[2], "✗ host gone shed — unreachable", "{out}");
}

#[test]
fn a_protocol_0_server_keeps_the_old_drover_and_alerts_once_naming_the_shed_needed() {
    let old = protocol_0();
    let (c, live) = setup("shedv-install", &old);
    let new = format!("{}.new", live.display());
    let (code, gate) = run(&c, &Fake::default(), &["doctor", "--config-only"]);
    assert_eq!(code, 1, "{gate}");

    let (code, out) = run(&c, &host(SHA, &new, (code, &gate)), &["self-update"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(std::fs::read_to_string(&live).unwrap(), "OLD", "{out}");
    assert!(out.contains("✗ shed — SHED_VERSION"), "{out}");
    let r = host(SHA2, &new, (code, &gate));
    assert_eq!(run(&c, &r, &["self-update"]).0, 1, "the next head, the same shed");
    assert!(!r.called("profiles sync"));
    let failed = kinds(&c, "self-update-failed");
    assert_eq!(failed.len(), 1, "{failed:?}");
    let reason = failed[0].str("reason").unwrap();
    assert!(
        reason.contains(&format!("drover speaks shed {VERSION} ({BUILD}) protocol {PROTOCOL}")),
        "{reason}"
    );
}

#[test]
fn a_shed_version_refusal_alerts_once_per_state_not_a_line_per_pass() {
    let old = protocol_0();
    let c = ctx(&tmpdir("shedv-drive"), &old.config(FLEET));
    let pass = |c: &drover::Ctx| run(c, &Fake::default(), &["drive"]).1;
    for at in [0, 60] {
        let out = pass(&drover::Ctx {
            now: c.now + at,
            ..c.clone()
        });
        assert!(!out.contains("shed events:"), "{out}");
    }
    let alerts = kinds(&c, "shed-version");
    assert_eq!(alerts.len(), 1);
    assert!(alerts[0]
        .str("reason")
        .unwrap()
        .starts_with("SHED_VERSION: drover speaks shed"));

    old.lock().hello.as_mut().unwrap().protocol = PROTOCOL;
    old.lock().hello.as_mut().unwrap().build = BUILD.into();
    pass(&drover::Ctx {
        now: c.now + 120,
        ..c.clone()
    });
    pass(&drover::Ctx {
        now: c.now + 180,
        ..c.clone()
    });
    assert_eq!(kinds(&c, "shed-version-ok").len(), 1);
    assert_eq!(kinds(&c, "shed-version").len(), 1);
}

#[test]
fn a_live_socket_beside_a_failed_shed_unit_shows_the_server_outside_its_service() {
    let b = FakeShed::new();
    let c = ctx(
        &tmpdir("shedv-outside"),
        &b.config(&format!("{FLEET}\n[hosts.far]\nmachine = \"far\"\n")),
    );
    let ssh = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C \
               -oControlPersist=10m far -- ";
    let hello = serde_json::json!(Response::ok(
        serde_json::json!(0),
        serde_json::json!(drover::shed::proto::Hello::ours())
    ));
    let ls = serde_json::json!(Response::ok(serde_json::json!(1), serde_json::json!([])));
    let f = Fake::default()
        .reply(&format!("{ssh}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\n")
        .reply(&format!("{ssh}'shed' 'connect'"), &format!("{hello}\n{ls}\n"))
        .reply(
            &format!("{ssh}'systemctl' '--user' 'show'"),
            "LoadState=loaded\nActiveState=failed\n",
        );
    let (_, out) = run(&c, &f, &["doctor"]);
    assert!(
        out.lines()
            .any(|l| l.starts_with("✗ host far shed — server outside its service") && l.contains(" → ")),
        "{out}"
    );
}
