use super::{ctx, run, tmpdir, Fake, FakeShed, CONFIG};
use drover::events::Event;
use serde_json::json;

/// t141's large board (#1312): 150 queued issues, 10 working lanes, thread `t1` with open asks `q1`–`q3`, ≥ 1,500 events.
pub fn large(name: &str) -> (drover::Ctx, FakeShed) {
    let c = ctx(
        &tmpdir(name),
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    let sh = FakeShed::attached(&c);
    let now = drover::now();
    let mut log = vec![];
    let mut ev = |ts: u64, issue: Option<u64>, kind: &str, d: serde_json::Value| {
        log.push(serde_json::to_string(&Event::new(ts, "sendmeter", issue, kind, d)).unwrap());
    };
    for n in 1..=150u64 {
        let t = now - 20_000 + n * 100;
        for i in 0..10 {
            let d = json!({"step": "review", "code": "EXIT", "result": format!("exit 1: try {i}")});
            ev(t + i, Some(n), "driven", d);
        }
        if n <= 10 {
            let (agent, wt) = (format!("sendmeter-impl-{n}"), c.state.with_file_name(format!("wt-{n}")));
            std::fs::create_dir_all(&wt).unwrap();
            let wt = wt.to_string_lossy().into_owned();
            let d = json!({"role": "impl", "agent": agent, "pane": agent, "worktree": wt, "branch": format!("issue-{n}"), "runtime": "shed"});
            ev(t + 20, Some(n), "spawn", d);
            sh.lane(&agent, "working", &wt);
            sh.screen(&agent, "⏺ Working on it\n\n❯ \n");
        }
    }
    ev(
        now - 7_200,
        None,
        "thread-new",
        json!({"thread": "t1", "title": "Rate limits", "harness": "claude"}),
    );
    for i in 0..40u64 {
        let from = if i % 2 == 0 { "owner" } else { "orchestrator" };
        ev(
            now - 7_000 + i * 10,
            None,
            "thread-msg",
            json!({"thread": "t1", "from": from, "text": format!("message {i}")}),
        );
    }
    for q in 1..=3u64 {
        let ask = json!({"id": format!("q{q}"), "thread": "t1", "question": format!("Limit {q}: per key or per IP?"), "options": ["per key", "per ip"], "recommend": 1, "kind": "owner"});
        ev(now - 600 + q, None, "needs-decision", ask);
    }
    let start = json!({"thread": "t1", "agent": "orchestrator-t1", "pane": "orchestrator-t1"});
    ev(now - 60, None, "orchestrator-start", start);
    sh.add(
        "orchestrator-t1",
        json!({"fleet": "sendmeter", "issue": "-", "role": "orchestrator", "thread": "t1", "source": "drover"}),
    );
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    std::fs::write(&c.state, log.join("\n") + "\n").unwrap();
    let issues: Vec<String> = (1..=150).map(|n| n.to_string()).collect();
    let mut args = vec!["queue", "sendmeter", "add"];
    args.extend(issues.iter().map(String::as_str));
    assert_eq!(run(&c, &Fake::default(), &args).0, 0);
    assert!(drover::events::read(&c.state).unwrap().len() >= 1_500);
    (c, sh)
}
