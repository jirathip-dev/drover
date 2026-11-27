use super::*;
use drover::runner::{Out, Runner};
use std::sync::atomic::{AtomicBool, Ordering};

struct Aging<'a> {
    f: &'a Fake,
    c: &'a drover::Ctx,
    aged: AtomicBool,
}

impl Aging<'_> {
    fn age(&self, prog: &str) {
        if prog != "gh" || self.aged.swap(true, Ordering::SeqCst) {
            return;
        }
        let (rows, _) = drover::db::view(&self.c.state).unwrap();
        assert!(!rows.is_empty(), "the pass looked at the main host first");
        for h in rows {
            let old = drover::link::view::HostSeen {
                heard: Some(self.c.now - 180),
                ..h
            };
            drover::db::view_put(&self.c.state, &old, None).unwrap();
        }
    }
}

impl Runner for Aging<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.age(prog);
        self.f.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.age(prog);
        self.f.run_in(prog, args, input)
    }
    fn sleep(&self, _: u64) {}
}

fn pass_errors(c: &drover::Ctx) -> Vec<serde_json::Value> {
    let all = read(&c.state).unwrap();
    let passes = all.iter().filter(|e| e.kind == "drive-pass" && e.fleet == "sendmeter");
    let acts = passes.flat_map(|e| e.data["actions"].as_array().cloned().unwrap_or_default());
    acts.filter(|a| a["step"] == "pass" || a["outcome"] == "error")
        .collect()
}

#[test]
fn a_main_host_view_gone_stale_mid_pass_fails_no_step_and_the_start_still_happens() {
    let c = setup("drive-stale-view");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let f = fake(NONE).reply("git -C", "");
    look(&c, &f);
    let r = Aging {
        f: &f,
        c: &c,
        aged: AtomicBool::new(false),
    };
    let (code, out) = run(&c, &r, &["drive", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.aged.load(Ordering::SeqCst), "the view went stale mid-pass: {out}");
    assert_eq!(pass_errors(&c), Vec::<serde_json::Value>::new(), "{out}");
    assert!(!out.contains("no word from it"), "{out}");
    assert!(out.contains("sendmeter #12 start: "), "{out}");
    assert_eq!(state(&c), "impl");
}

#[test]
fn a_main_host_that_does_not_answer_skips_the_lane_checks_with_one_line_and_no_pass_error() {
    let c = setup("drive-unheard");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let f = fake(&[("sendmeter-impl-12", "working", &wt.to_string_lossy())]).reply("git -C", "");
    look(&c, &f);
    let (rows, _) = drover::db::view(&c.state).unwrap();
    for h in rows {
        let old = drover::link::view::HostSeen {
            heard: Some(c.now - 180),
            ..h
        };
        drover::db::view_put(&c.state, &old, None).unwrap();
    }
    sh().fault(Fault::Loss(1000));
    let (code, out) = run(&c, &f, &["drive", "sendmeter"]);
    sh().fault(Fault::None);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.matches("its lane checks skipped this pass").count(), 1, "{out}");
    assert_eq!(pass_errors(&c), Vec::<serde_json::Value>::new(), "{out}");
    let all = read(&c.state).unwrap();
    assert!(
        all.iter().any(|e| e.kind == "drive-pass" && e.fleet == "sendmeter"),
        "the pass ran to its end: {out}"
    );
}
