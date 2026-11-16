mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::shed::proto::{BUILD, VERSION};
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
fn a_shed_on_path_or_a_serve_off_the_pin_refuses_the_spawn_and_runs_nothing() {
    let c = ctx(&tmpdir("shedpin-path"), CONFIG);
    let sh = FakeShed::attached(&c);
    let out = refused(&c, &sh, &fake("shed 9.9.9 (02fcf97)"));
    assert!(out.contains("shed on PATH is 9.9.9 (02fcf97)"), "{out}");
    assert!(out.contains(&drover::shed::REV[..7]), "{out}");

    let c = ctx(&tmpdir("shedpin-hello"), CONFIG);
    let sh = FakeShed::attached(&c);
    let h = sh.lock().hello.clone().unwrap();
    sh.lock().hello = Some(drover::shed::proto::Hello {
        version: "9.9.9".into(),
        build: "02fcf97".into(),
        ..h
    });
    let out = refused(&c, &sh, &fake("shed: no version"));
    assert!(out.contains("shed serve runs 9.9.9 (02fcf97)"), "{out}");
}

#[test]
fn a_serve_build_other_than_the_shed_on_path_refuses_the_spawn_and_says_restart_serve() {
    let c = ctx(&tmpdir("shedpin-serve"), CONFIG);
    let sh = FakeShed::attached(&c);
    sh.lock().hello.as_mut().unwrap().build = "02fcf97".into();
    let out = refused(&c, &sh, &fake(&ours()));
    assert!(out.contains(&format!("shed serve runs {VERSION} (02fcf97)")), "{out}");
    assert!(out.contains(&format!("shed on PATH is {VERSION} ({BUILD})")), "{out}");
    assert!(out.contains("restart shed serve"), "{out}");
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
    let pushed = std::fs::read_to_string(&pushes).unwrap();
    assert_eq!(pushed.lines().count(), 1, "{pushed}");
}

#[test]
fn shed_install_puts_the_binary_and_serve_on_the_pin_and_doctor_reads_it_back() {
    let c = ctx(&tmpdir("shedpin-install"), CONFIG);
    let _sh = FakeShed::attached(&c);
    let rev = drover::shed::REV;
    let f = fake(&ours())
        .reply("cargo install --git", "")
        .reply("shed install --force", "");
    let (code, out) = run(&c, &f, &["shed", "install"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("--rev {rev}")), "{out}");
    assert!(!f.called("cargo") && !f.called("shed install"), "a dry run: {out}");

    let (code, out) = run(&c, &f, &["shed", "install", "--yes"]);
    assert_eq!(code, 0, "{out}");
    let cargo = f.index(&format!(
        "cargo install --git {} --rev {rev} --locked --force shed",
        drover::shed::git()
    ));
    let serve = f.index("shed install --force");
    assert!(cargo.is_some() && cargo < serve, "{:?}", f.calls.borrow());
    let back = format!("shed on PATH {VERSION} ({BUILD}), pin {}", &rev[..7]);
    assert!(out.contains(&back), "{out}");
    assert_eq!(kinds(&c, "shed-installed").len(), 1, "{out}");
    let (_, doctor) = run(&c, &f, &["doctor", "--config-only"]);
    assert!(
        doctor.lines().any(|l| l.starts_with("✓ shed — ") && l.ends_with(&back)),
        "{doctor}"
    );
}
