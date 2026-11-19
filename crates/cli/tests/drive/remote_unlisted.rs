use super::*;

fn remote_impl(name: &str) -> drover::Ctx {
    let c = setup(name);
    reconfig(
        &c,
        &format!("{CONFIG}\n[hosts.mini]\n\n[hosts.boat1]\nmachine = \"boat1\"\n"),
    );
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "workspace": "",
        "runtime": "shed", "worktree": wt, "branch": "issue-12", "machine": "boat1", "host": "boat1",
        "confirmed": true});
    ev(&c, c.now - 1200, "spawn", d);
    c
}

#[test]
fn a_remote_impl_whose_host_listing_is_unreadable_is_unknown_not_gone() {
    let c = remote_impl("drive-remote-unlisted");
    let r = fake(NONE).reply("ssh ", "").reply("git -C", "");
    let (_, st) = run(&c, &r, &["status", "sendmeter"]);
    assert!(!st.contains("lane gone") && st.contains("unknown"), "{st}");
    let (out, steps) = drive(&c, &r);
    assert!(steps.is_empty(), "{steps:?} {out}");
    assert!(started("sendmeter-impl-12").is_empty(), "{out}");
    let all = read(&c.state).unwrap();
    assert!(
        !all.iter().any(|e| matches!(e.kind.as_str(), "respawn" | "supersede")),
        "{out}"
    );
}

#[test]
fn a_remote_impl_its_hosts_shed_lists_is_never_gone() {
    let c = remote_impl("drive-remote-listed");
    let boat = FakeShed::new();
    let wt = c.worktrees.join("sendmeter/issue-12");
    boat.lane("sendmeter-impl-12", "idle", &wt.display().to_string());
    let f = fake(NONE).reply("git -C", "");
    let r = OverSsh {
        f: &f,
        hosts: vec![("boat1", &boat)],
    };
    let (_, st) = run(&c, &r, &["status", "sendmeter"]);
    assert!(!st.contains("lane gone"), "{st}");
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let all = read(&c.state).unwrap();
    assert!(
        !all.iter().any(|e| matches!(e.kind.as_str(), "respawn" | "supersede")),
        "{out}"
    );
    assert!(!boat.called("close", "sendmeter-impl-12"), "{:?}", boat.methods());
}
