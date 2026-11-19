use super::*;

#[test]
fn an_extend_during_a_pass_only_requeues_it() {
    let c = setup("drive-extend-lock");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        12,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("3"), "body": "FAIL"}),
    );
    ev(&c, 14, "needs-human", json!({"code": "ROUND_LIMIT"}));
    lease_as(&c, "drive-sendmeter.lock", 4242, c.now, 3600);
    let r = fake(NONE)
        .reply("gh pr view issue-12", &pr(&sha("3"), "green"))
        .reply("kill -0 4242", "")
        .reply("git -C", "");
    let (code, out) = run(&c, &r, &["extend", "sendmeter", "12"]);
    assert!(code == 0 && out.ends_with("#12: extend\n"), "{out}");
    assert!(started("sendmeter-impl-12").is_empty(), "{:?}", r.calls.borrow());
    assert_eq!(state(&c), "queued");
    assert!(
        lease_on(&c, "drive-sendmeter.lock").is_some(),
        "the pass's lock is left alone"
    );
    let all = read(&c.state).unwrap();
    assert_eq!(all.iter().filter(|e| e.kind == "round-extended").count(), 1);
    assert_eq!(drover::pending::of(&all, "sendmeter", 12), None);
}

fn at_update_limit(name: &str) -> (drover::Ctx, Fake) {
    let c = setup(name);
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        11,
        "verdict",
        json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
    );
    for k in 1..=3 {
        let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "workspace": "",
            "runtime": "shed", "worktree": wt, "branch": "issue-12", "update": true});
        ev(&c, 20 + k, "spawn", d);
        let p = format!("c{k}");
        ev(
            &c,
            30 + k,
            "verdict",
            json!({"verdict": "PASS", "sha": sha(&p), "body": "PASS"}),
        );
    }
    let r = fake(NONE)
        .reply("gh pr view issue-12", &stale_pr(&sha("c3"), "CONFLICTING", "DIRTY"))
        .reply("git -C", "");
    (c, r)
}

fn nh(c: &drover::Ctx) -> usize {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "needs-human")
        .count()
}

#[test]
fn an_extend_on_update_limit_grants_one_update_round() {
    let (c, r) = at_update_limit("drive-extend-update");
    let (out, _) = drive(&c, &r);
    assert!(out.contains("REFUSED UPDATE_LIMIT"), "{out}");
    assert_eq!(state(&c), "needs-human");
    let (code, out) = run(&c, &r, &["extend", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    let runs = started("sendmeter-impl-12").len();
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["conflict"], "{out}");
    assert_eq!(state(&c), "fix(1)+u4");
    assert_eq!(started("sendmeter-impl-12").len(), runs + 1, "{:?}", r.calls.borrow());
    ev(
        &c,
        90,
        "verdict",
        json!({"verdict": "PASS", "sha": sha("d4"), "body": "PASS"}),
    );
    drover::gh::forget_pr(&c, "sendmeter", 12);
    let r = fake(NONE)
        .reply("gh pr view issue-12", &stale_pr(&sha("d4"), "CONFLICTING", "DIRTY"))
        .reply("git -C", "");
    drive(&c, &r);
    assert_eq!(state(&c), "needs-human");
    let all = read(&c.state).unwrap();
    let last = all.iter().rfind(|e| e.kind == "needs-human").unwrap();
    assert_eq!(last.str("code"), Some("UPDATE_LIMIT"));
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--supersede", "--update"]);
    assert!(code == 3 && out.contains("REFUSED UPDATE_LIMIT"), "{out}");
    assert_eq!(started("sendmeter-impl-12").len(), runs + 1, "no second update lane");
}

#[test]
fn an_update_limit_refusal_while_extended_spends_no_extension() {
    let (c, r) = at_update_limit("drive-extend-round");
    ev(&c, 50, "needs-human", json!({"code": "ROUND_LIMIT"}));
    let (code, out) = run(&c, &r, &["extend", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    let (extended, before) = (state(&c), nh(&c));
    let s = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!((s.extra, s.updates), (1, 3), "a review round, as before");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--supersede", "--update"]);
    assert!(code == 3 && out.contains("REFUSED UPDATE_LIMIT"), "{out}");
    assert_eq!(state(&c), extended);
    assert_eq!(nh(&c), before, "no needs-human logged");
    assert_eq!(started("sendmeter-impl-12"), Vec::<String>::new());
}
