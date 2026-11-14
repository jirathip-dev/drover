mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

const LANE: &str = "sendmeter-impl-12";

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    let all = read(&c.state).unwrap().to_vec();
    all.into_iter().filter(|e| e.kind == kind).collect()
}

fn hog(name: &str, load: &str) -> (drover::Ctx, Fake) {
    let c = ctx(&tmpdir(name), CONFIG);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": LANE, "pane": "w1:p1", "worktree": wt, "branch": "issue-12"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", d)).unwrap();
    let scratch = c.worktrees.join(".build/.scratch").join(LANE);
    let (mut ps, mut lsof) = (
        String::from("500 1 0.0 /bin/zsh\n"),
        format!("p500\nczsh\nfcwd\nn{}\n", wt.display()),
    );
    for i in 0..24 {
        ps += &format!("{} 500 40.0 /usr/bin/yes\n", 600 + i);
        lsof += &format!("p{}\ncyes\nfcwd\nn{}\n", 600 + i, scratch.display());
    }
    let r = Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", &format!("{{ {load} 20.0 10.0 }}\n"))
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("ps -Ao", &ps)
        .reply("lsof", &lsof)
        .reply("herdr", "")
        .reply("kill", "");
    (c, r)
}

fn pass(c: &drover::Ctx, r: &Fake, secs: u64) {
    let at = drover::Ctx {
        now: c.now + secs,
        ..c.clone()
    };
    run(&at, r, &["drive", "--fast"]);
    run(&at, r, &["drive"]);
}

#[test]
fn a_busy_lane_on_a_host_under_its_load_limit_is_left_alone() {
    let (c, r) = hog("lane-hog-calm", "10.0");
    for p in 0..5 {
        pass(&c, &r, p * 300);
    }
    assert!(kinds(&c, "lane-hog").is_empty());
    assert_eq!(r.count("herdr pane send-keys w1:p1 Escape"), 0);
}

#[test]
fn a_lane_over_half_the_cpu_three_passes_running_is_paused_once_and_killed_only_on_the_owners_tap() {
    let (c, r) = hog("lane-hog", "30.0");
    let pass = |secs: u64| {
        pass(&c, &r, secs);
    };
    for p in 0..2 {
        pass(p * 300);
        assert!(kinds(&c, "lane-hog").is_empty(), "pass {p}: under 3 passes, no alert");
    }
    pass(600);
    assert_eq!(kinds(&c, "lane-hog").len(), 1, "the third pass alerts");
    for p in 3..5 {
        pass(p * 300);
    }
    let hog = kinds(&c, "lane-hog");
    assert_eq!(hog.len(), 1, "one alert");
    let why = hog[0].str("reason").unwrap();
    assert!(
        why.starts_with("sendmeter-impl-12 held 96% of the host's CPU (24 × yes)"),
        "{why}"
    );
    assert_eq!(kinds(&c, "stop").len(), 1, "one stop");
    assert_eq!(r.count("herdr pane send-keys w1:p1 Escape"), 1, "Esc sent once");
    assert!(!r.called("kill"), "nothing killed before the owner's tap");
    let ask = kinds(&c, "needs-decision")
        .into_iter()
        .find(|e| e.data.get("lane_hog").is_some())
        .unwrap();
    assert_eq!(ask.data["options"], json!(["kill its processes", "let it run"]));
    let id = ask.str("id").unwrap();
    run(&c, &r, &["answer", "sendmeter", id, "1", "--by", "owner"]);
    assert!(
        r.called("kill -TERM 600"),
        "the owner's tap kills the lane's yes processes"
    );
    assert!(!r.called("kill -TERM 500"), "never the lane's shell");
}
