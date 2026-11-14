mod common;
use common::*;
use drover::events::read;

fn forced(name: &str) -> drover::events::Event {
    let c = ctx(&tmpdir(name), CONFIG);
    let sh = FakeShed::attached(&c);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "4\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("git -C", "")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#);
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--force"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(sh.all("run").len(), 1);
    let all = read(&c.state).unwrap();
    all.iter().find(|e| e.kind == "override").expect("override").clone()
}

#[test]
fn a_forced_spawn_logs_its_override_by_the_owner_or_the_orchestrator() {
    std::env::remove_var("DROVER_LANE");
    std::env::remove_var("DROVER_ORCHESTRATOR");
    let o = forced("force-owner");
    assert_eq!((o.str("code"), o.str("by")), (Some("HOST_BUSY"), Some("owner")));
    std::env::set_var("DROVER_LANE", "sendmeter:-:orchestrator");
    std::env::set_var("DROVER_ORCHESTRATOR", "ops");
    let o = forced("force-orchestrator");
    assert_eq!(o.str("by"), Some("orchestrator-ops"));
}
