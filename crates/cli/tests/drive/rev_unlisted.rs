use super::*;

#[test]
fn a_remote_reviewer_missing_from_one_listing_a_minute_after_start_is_left_running() {
    let c = setup("drive-rev-unlisted");
    reconfig(
        &c,
        &format!("{CONFIG}\n[hosts.mini]\n\n[hosts.boat1]\nmachine = \"boat1\"\n"),
    );
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("b4e191c");
    let rev = c.worktrees.join("sendmeter/rev-12-r1");
    std::fs::create_dir_all(&rev).unwrap();
    let d = json!({"role": "rev", "agent": "sendmeter-rev-12-r1", "pane": "sendmeter-rev-12-r1", "workspace": "",
        "runtime": "shed", "worktree": rev, "branch": "rev-12-r1", "sha": head,
        "machine": "boat1", "host": "boat1", "confirmed": true});
    ev(&c, c.now - 60, "spawn", d);
    let r = fake(NONE)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("ssh ", "")
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert!(steps.is_empty(), "{steps:?} {out}");
    assert!(started("sendmeter-rev-12").is_empty(), "{out}");
    let all = read(&c.state).unwrap();
    assert!(!all.iter().any(|e| e.kind == "lane-lost"), "{out}");
    assert!(!all.iter().any(|e| e.kind == "supersede"), "{out}");
}
