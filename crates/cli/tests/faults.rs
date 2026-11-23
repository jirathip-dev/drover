mod common;
use common::*;
use std::time::Duration;

const BOUND: Duration = Duration::from_secs(20);

fn within<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || drop(tx.send(f())));
    rx.recv_timeout(BOUND)
        .unwrap_or_else(|_| panic!("{what} still blocked after {} s", BOUND.as_secs()))
}

fn config(sh: &FakeShed) -> String {
    let cfg = CONFIG
        .replace("harness  =  \"hermes\"", "harness = \"claude\"")
        .replace("[limits]\n", "[limits]\nshed_call_secs = 1\n");
    sh.config(&cfg)
}

fn spawn_fake() -> Fake {
    Fake::default()
        .once("git -C /tmp/does-not-matter/sendmeter rev-parse --verify", 1, "")
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn spawn(c: &drover::Ctx) -> (i32, String) {
    let c = c.clone();
    within("spawn", move || {
        run(&c, &spawn_fake(), &["spawn", "sendmeter", "12", "impl"])
    })
}

#[test]
fn a_spawn_behind_a_hung_shed_link_fails_within_its_bound_instead_of_waiting_forever() {
    let sh = FakeShed::new();
    let d = tmpdir("fault-hang");
    let c = ctx(&d, &config(&sh));
    sh.fault(Fault::Hang);
    let (code, out) = spawn(&c);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("no answer"), "{out}");
}
