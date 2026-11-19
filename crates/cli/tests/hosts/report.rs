use super::*;

#[test]
fn a_remote_host_with_no_fleet_toml_reports_its_running_lane_and_is_up() {
    let c = setup("hosts-bare-report", "", true, 0);
    remote_spawn(&c, NOW - 900);
    let (sh, bazzite) = (FakeShed::attached(&c), live_on());
    let f = fake(1.0, 1.0);
    let r = two(&f, &bazzite);
    let on = drover::hosts::On::new(&r, Some("bazzite"));
    let watch = r#"{"roots":[],"worktrees":[]}"#;
    let o = drover::runner::Runner::run_in(
        &on,
        "/opt/drover/bin/drover",
        &drover::runner::argv(&["hosts", "report"]),
        watch,
    )
    .unwrap();
    assert_eq!(o.code, 0, "{o:?}");
    let frame: serde_json::Value = serde_json::from_str(o.stdout.trim()).unwrap();
    assert_eq!(frame["body"]["shed"], serde_json::Value::Null, "{frame}");
    assert_eq!(frame["body"]["lanes"][0]["session"]["name"], IMPL, "{frame}");
    look(&c, &sh.on(&r));
    let cfg = Config::load(&c.config).unwrap();
    let v = drover::link::view::View::load(&c, &cfg).unwrap();
    assert_eq!(v.down(Some("bazzite")), None);
    let there = v.sessions().0.into_iter().find(|h| h.0.as_deref() == Some("bazzite"));
    let names: Vec<String> = there.unwrap().1.unwrap().into_iter().map(|s| s.name).collect();
    assert_eq!(names, [IMPL]);
}

#[test]
fn a_failing_report_names_its_reason_and_keeps_the_hosts_folders() {
    let c = trim_setup("hosts-report-fails", "");
    let cfg = Config::load(&c.config).unwrap();
    let r = healthy(fake(0.1, 0.1))
        .reply(&format!("{SSH}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\ndocker\n")
        .reply(&format!("{SSH}'/opt/drover/bin/drover' 'hosts' 'report'"), "")
        .reply(SWEEP, "sendmeter-impl-12\n")
        .reply(QUICK, &format!("{TRIMMED}\ntotal 9 freed 9\n"));
    let (sh, bazzite) = (FakeShed::attached(&c), live_on());
    let o = two(&r, &bazzite);
    look(&c, &sh.on(&o));
    let (_, hosts) = run(&c, &o, &["hosts", "refresh"]);
    let line = hosts.lines().find(|l| l.starts_with("bazzite · ")).unwrap();
    assert!(
        line.starts_with("bazzite · offline") && line.contains(" · report: no frame"),
        "{hosts}"
    );
    let (_, doctor) = run(&c, &o, &["doctor"]);
    assert!(doctor.contains("\n✗ host bazzite report — no frame"), "{doctor}");
    let mut out = Vec::new();
    drover::check::trim_hosts(&c, &cfg, &sh.on(&o), &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("its report fails (no frame): no folder removed"), "{out}");
    assert!(!r.called(SWEEP) && !r.called(QUICK), "{:?}", r.calls.borrow());
}

#[test]
fn a_hosts_line_shows_the_build_it_reports() {
    let c = setup("hosts-report-build", "", true, 0);
    let cache = c.state.with_file_name("hosts.json");
    let mut seen: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&cache).unwrap()).unwrap();
    seen["bazzite"]["drover"] = json!("d91e726".repeat(6)[..40]);
    std::fs::write(&cache, seen.to_string()).unwrap();
    let (sh, bazzite) = (FakeShed::attached(&c), FakeShed::new());
    let f = fake(0.1, 0.1);
    let r = two(&f, &bazzite);
    look(&c, &sh.on(&r));
    let (_, hosts) = run(&c, &r, &["hosts"]);
    let line = hosts.lines().find(|l| l.starts_with("bazzite · ")).unwrap();
    assert!(line.contains(&format!(" · drover {} ✓", drover::SHA)), "{hosts}");
}
