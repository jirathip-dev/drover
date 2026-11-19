mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::shed::proto::{BUILD, PROTOCOL, VERSION};
use serde_json::json;

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

fn fake(version: &str) -> Fake {
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("shed --version", &format!("{version}\n"))
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
}

fn ours() -> String {
    format!("shed {VERSION} ({BUILD})")
}

fn refused(c: &drover::Ctx, sh: &FakeShed, f: &Fake) -> String {
    let (code, out) = run(c, f, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED SHED_VERSION: "), "{out}");
    assert!(sh.all("run").is_empty(), "{out}");
    assert!(kinds(c, "spawn").is_empty(), "{out}");
    assert!(!c.worktrees.join("sendmeter/issue-12").exists(), "{out}");
    out
}

#[test]
fn shed_builds_9b88eb5_and_f026a4d_both_pass_spawn_and_doctor_whichever_runs_serve() {
    for (serve, path) in [("9b88eb5", "f026a4d"), ("f026a4d", "9b88eb5")] {
        let c = ctx(&tmpdir("shedpin-range"), CONFIG);
        let sh = FakeShed::attached(&c);
        sh.lock().hello.as_mut().unwrap().build = serve.into();
        let f = fake(&format!("shed {VERSION} ({path})"));
        let (code, out) = run(&c, &f, &["spawn", "sendmeter", "12", "impl"]);
        assert!(!out.contains("SHED_VERSION"), "{serve}: {out}");
        assert_eq!(code, 0, "{serve}: {out}");
        assert_eq!(sh.all("run").len(), 1, "{serve}: {out}");
        let (_, doctor) = run(&c, &f, &["doctor", "--config-only"]);
        let want = format!(
            "✓ shed — server {VERSION} ({serve}) protocol {PROTOCOL}, drover needs protocol {} or newer; \
             shed on PATH {VERSION} ({path}) (serve hands over itself)",
            drover::shed::MIN_PROTOCOL
        );
        assert!(doctor.lines().any(|l| l == want), "{serve}: {doctor}");
    }
}

#[test]
fn a_serve_under_drovers_minimum_protocol_refuses_the_spawn_naming_both_protocols() {
    let c = ctx(&tmpdir("shedpin-old"), CONFIG);
    let sh = FakeShed::attached(&c);
    sh.lock().hello.as_mut().unwrap().protocol = drover::shed::MIN_PROTOCOL - 1;
    let out = refused(&c, &sh, &fake(&ours()));
    let want = format!(
        "shed serve is {VERSION} ({BUILD}) protocol 0, drover needs shed protocol {} or newer → {}",
        drover::shed::MIN_PROTOCOL,
        drover::shed::UPDATE
    );
    assert!(out.contains(&want), "{out}");
}

#[test]
fn five_lanes_whose_prompt_never_landed_stop_every_respawn_with_one_needs_human_and_one_push() {
    let d = tmpdir("shedpin-unlanded");
    let pushes = d.join("pushes");
    let cfg = format!(
        "{CONFIG}\n[[notify]]\ncmd = \"cat >> '{}'\"\non = [\"needs-human\"]\n",
        pushes.display()
    );
    let c = ctx(&d, &cfg);
    let sh = FakeShed::attached(&c);
    let t0 = 1_000_000;
    for n in 1..=5u64 {
        let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
        std::fs::create_dir_all(&wt).unwrap();
        let agent = format!("sendmeter-impl-{n}");
        let d = json!({"role": "impl", "agent": agent, "pane": agent, "workspace": "", "runtime": "shed",
            "worktree": wt, "branch": format!("issue-{n}"), "confirmed": false});
        append(&c.state, &Event::new(t0 + n * 120, "sendmeter", Some(n), "spawn", d)).unwrap();
        sh.lane(&agent, "idle", &wt.to_string_lossy());
        sh.set(&agent, |t| t["reported"] = serde_json::Value::Null);
    }
    let r = fake(&ours());
    for k in 0..2 {
        let (code, out) = run(
            &drover::Ctx {
                now: t0 + 900 + k * 60,
                ..c.clone()
            },
            &r,
            &["drive"],
        );
        assert_eq!(code, 0, "{out}");
        assert!(kinds(&c, "respawn").is_empty(), "{out}");
        assert!(sh.all("run").is_empty(), "{out}");
    }
    let nh = kinds(&c, "needs-human");
    assert_eq!(nh.len(), 1, "{nh:?}");
    let reason = nh[0].str("reason").unwrap();
    assert!(reason.contains("5 lanes"), "{reason}");
    assert!(reason.contains(&format!("{VERSION} ({BUILD})")), "{reason}");
    let sent = std::fs::read_to_string(&pushes).unwrap();
    assert_eq!(sent.lines().count(), 1, "{sent}");
}

#[test]
fn drover_has_no_shed_install_verb_and_runs_no_cargo() {
    let c = ctx(&tmpdir("shedpin-noinstall"), CONFIG);
    let f = fake(&ours()).reply("cargo install --git", "");
    let (code, out) = run(&c, &f, &["shed", "install", "--yes"]);
    assert_eq!(code, 2, "{out}");
    assert!(!f.called("cargo") && !f.called("shed install"), "{out}");
}
