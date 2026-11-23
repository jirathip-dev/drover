use super::*;

const LISTED: &str = r#"[{"name":"gone","target":"gone"}]"#;

#[test]
fn a_hosts_machine_missing_from_shed_is_added_back_by_a_pass_and_its_lane_then_killed() {
    let c = setup("drive-shed-machines");
    let hosts = "[hosts.mini]\n\n[hosts.box]\nmachine = \"box\"\n\n[hosts.off]\nmachine = \"off\"\nenabled = false\n";
    reconfig(&c, &format!("{CONFIG}\n{hosts}"));
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "workspace": "",
        "runtime": "shed", "worktree": wt, "branch": "issue-12", "machine": "box", "host": "box",
        "confirmed": true});
    ev(&c, c.now - 1200, "spawn", d);
    let boat = FakeShed::new();
    boat.lane("sendmeter-impl-12", "idle", &wt.display().to_string());
    let both = r#"[{"name":"gone","target":"gone"},{"name":"box","target":"box"}]"#;
    let f = fake(NONE)
        .once("shed machine ls --json", 0, LISTED)
        .reply("shed machine ls --json", both)
        .reply("shed machine add", "")
        .reply("git -C", "");
    let r = OverSsh {
        f: &f,
        hosts: vec![("box", &boat)],
    };
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(f.count("shed machine add"), 1, "{:?}", f.calls.borrow());
    assert!(f.called("shed machine add box box"), "{:?}", f.calls.borrow());
    assert!(!f.called("shed machine rm"), "drover never removes a machine");
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(f.count("shed machine add"), 1, "adding twice is once");
    let (code, out) = run(&c, &r, &["kill", "sendmeter-impl-12", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(!boat.has("sendmeter-impl-12"), "{out}");
}
