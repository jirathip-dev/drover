//! #275: a lane on another host (`HERDSMAN_MAIN` set) runs its ask, verdict and own events on the main host over ssh:
//! the main log gets them, the main host alone pushes, and the lane host's log is never written. One test: the lane's
//! env vars are process-wide.
mod common;
use common::*;
use herdsman::events::read;
use herdsman::runner::{Out, Runner};

const MAIN_SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 mini -- 'env' 'HERDSMAN_LANE=sendmeter:12:impl' \
'/opt/herdsman/bin/herdsman' ";

/// The lane host's runner: its ssh to `mini` runs herdsman in process on the main host's `main` ctx.
struct Remote<'a> {
    fake: Fake,
    main: &'a herdsman::Ctx,
}

impl Runner for Remote<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let line = format!("{prog} {}", args.join(" "));
        let scripted = self.fake.replies.iter().any(|(p, _)| line.starts_with(p.as_str()));
        let Some(rest) = line.strip_prefix(MAIN_SSH).filter(|_| !scripted) else {
            return self.fake.run(prog, args);
        };
        self.fake.calls.borrow_mut().push(line.clone());
        let words: Vec<&str> = rest.split("' '").map(|w| w.trim_matches('\'')).collect();
        // The main host's ssh session has no `HERDSMAN_MAIN`.
        std::env::remove_var("HERDSMAN_MAIN");
        let (code, stdout) = run(self.main, &self.fake, &words);
        std::env::set_var("HERDSMAN_MAIN", "mini:/opt/herdsman/bin/herdsman");
        Ok(Out {
            code,
            stdout,
            stderr: String::new(),
        })
    }
}

fn with_push(dir: &std::path::Path) -> String {
    let file = dir.join("pushed");
    format!(
        "{CONFIG}\n[[notify]]\ncmd = \"cat >> '{}'\"\non = [\"needs-decision\"]\n",
        file.display()
    )
}

#[test]
fn a_remote_lane_writes_and_pushes_on_the_main_host_only() {
    let (md, ld) = (tmpdir("remote-main"), tmpdir("remote-lane"));
    let (main, lane) = (ctx(&md, &with_push(&md)), ctx(&ld, &with_push(&ld)));
    std::env::set_var("HERDSMAN_LANE", "sendmeter:12:impl");
    std::env::set_var("HERDSMAN_MAIN", "mini:/opt/herdsman/bin/herdsman");
    let r = Remote {
        fake: Fake::default().reply(&format!("{MAIN_SSH}'verdict'"), "ok\n"),
        main: &main,
    };
    let (code, out) = run(
        &lane,
        &r,
        &["ask", "sendmeter", "12", "Which way?", "--option", "a", "--option", "b"],
    );
    assert_eq!((code, out.as_str()), (0, "q1\n"), "{:?}", r.fake.calls.borrow());
    let ev = read(&main.state).unwrap();
    assert_eq!(ev.iter().filter(|e| e.kind == "needs-decision").count(), 1);
    assert!(!lane.state.exists(), "the lane host's log is never written");
    let pushed = std::fs::read_to_string(md.join("pushed")).unwrap();
    assert!(
        pushed.contains("needs you") && pushed.contains("Which way?"),
        "{pushed}"
    );
    assert!(!ld.join("pushed").exists(), "no push from the lane host");
    // A verdict's file goes as an absolute path (main reads it over ssh on this host).
    let sha = "a".repeat(40);
    let (code, out) = run(
        &lane,
        &r,
        &[
            "verdict",
            "sendmeter",
            "12",
            "PASS",
            "--sha",
            &sha,
            "--file",
            "verdict.md",
        ],
    );
    assert_eq!((code, out.as_str()), (0, "ok\n"));
    let cwd = std::env::current_dir().unwrap();
    let want = format!("'--file' '{}'", cwd.join("verdict.md").display());
    assert!(
        r.fake.calls.borrow().iter().any(|c| c.ends_with(&want)),
        "{:?}",
        r.fake.calls.borrow()
    );
    // Reads stay local: no ssh for anything but the lane's writes.
    run(&lane, &r, &["asks"]);
    assert_eq!(r.fake.count("ssh"), 2);
    std::env::remove_var("HERDSMAN_MAIN");
    std::env::remove_var("HERDSMAN_LANE");
}
