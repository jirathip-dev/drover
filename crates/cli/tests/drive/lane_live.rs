use super::*;

#[test]
fn a_respawn_refused_lane_live_on_every_pass_gives_one_owner_card() {
    let c = setup("drive-live-loop");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    ev(&c, 11, "pushed", json!({"sha": head}));
    lane(&c, 12, "rev", "sendmeter-rev-12-r1", "rev-12-r1");
    let d = json!({"verdict": "FAIL", "sha": head, "stage": "rev", "loop": "rev", "body": "FAIL"});
    ev(&c, 13, "verdict", d);
    let wt = lane(&c, 14, "impl", "sendmeter-impl-12", "issue-12");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12"});
    ev(&c, 20, "lane-killed", d);
    let r = fake(&[("sendmeter-impl-12", "working", &wt.to_string_lossy())]).reply("git -C", "");
    let at = |mins: u64| drover::Ctx {
        now: c.now + mins * 60,
        ..c.clone()
    };
    let (out, _) = drive(&at(0), &r);
    assert!(out.contains("REFUSED LANE_LIVE"), "{out}");
    assert!(kinds(&c, "needs-human").is_empty());
    for mins in [60, 90] {
        drive(&at(mins), &r);
    }
    let nh = kinds(&c, "needs-human");
    assert_eq!(nh.len(), 1, "{nh:?}");
    assert_eq!(nh[0].str("code"), Some("LANE_LIVE"), "{nh:?}");
    assert!(nh[0].str("reason").unwrap().contains("sendmeter-impl-12"), "{nh:?}");
}
