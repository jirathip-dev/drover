use super::*;

const STALL: u64 = 30 * 60;

fn done_impl(name: &str, turn: Option<&str>) -> (drover::Ctx, PathBuf) {
    let c = setup(name);
    let wt = lane(&c, c.now - STALL - 600, "impl", "sendmeter-impl-12", "issue-12");
    let d = json!({"agent": "sendmeter-impl-12", "state": "done", "since": c.now - STALL - 400});
    ev(&c, c.now - STALL - 400, "lane-state", d);
    if let Some(t) = turn {
        ev(&c, c.now - STALL - 300, "turn-end", json!({"state": t, "role": "impl"}));
    }
    (c, wt)
}

fn acted(c: &drover::Ctx) -> Vec<String> {
    let all = read(&c.state).unwrap();
    let kinds = ["respawn", "needs-human", "turn-prompted"];
    (all.iter().filter(|e| kinds.contains(&e.kind.as_str())))
        .map(|e| e.kind.clone())
        .collect()
}

fn ends_once(name: &str, turn: Option<&str>) {
    let (c, wt) = done_impl(name, turn);
    let at = wt.display().to_string();
    let r = fake(&[("sendmeter-impl-12", "done", at.as_str())]).reply("git -C", "");
    let (out, _) = drive(&c, &r);
    let later = drover::Ctx {
        now: c.now + 60,
        ..c.clone()
    };
    let (again, _) = drive(&later, &r);
    let acts = acted(&c);
    let ended = acts.iter().filter(|k| *k != "turn-prompted").count();
    assert_eq!(ended, 1, "{acts:?} {out} {again}");
    assert!(!acts.contains(&"turn-prompted".to_string()), "{acts:?} {out}");
}

#[test]
fn a_done_lane_with_no_turn_end_past_the_stall_is_respawned_once() {
    ends_once("done-lane-no-turn", None);
}

#[test]
fn a_done_lane_that_reported_working_past_the_stall_is_respawned_once_not_prompted() {
    ends_once("done-lane-working", Some("working"));
}
