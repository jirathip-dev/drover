use drover::hosts::{On, Pass};
use drover::runner::{argv, Out, Runner};
use std::sync::Mutex;
use std::time::Duration;

const TAKES: Duration = Duration::from_secs(21);

#[derive(Default)]
struct SlowCheck {
    limits: Mutex<Vec<Duration>>,
}

impl Runner for SlowCheck {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_for(prog, args, "", drover::runner::limit(prog, args))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.run_for(prog, args, input, drover::runner::limit(prog, args))
    }
    fn run_for(&self, prog: &str, args: &[String], _input: &str, limit: Duration) -> anyhow::Result<Out> {
        assert_eq!(prog, "ssh");
        self.limits.lock().unwrap().push(limit);
        let slow = args.iter().any(|a| a.contains("arch-check"));
        let code = if slow && limit < TAKES { 124 } else { 0 };
        Ok(Out {
            code,
            stdout: "0 violations\n".into(),
            ..Out::default()
        })
    }
}

#[test]
fn a_remote_run_for_past_ssh_call_secs_keeps_its_limit_and_its_host() {
    let call = Duration::from_secs(drover::config::limits().ssh_call_secs);
    assert!(call < TAKES);
    let gate = Duration::from_secs(drover::config::limits().gate_check_minutes * 60);
    let ssh = SlowCheck::default();
    let pass = Pass::fast(&ssh);
    let on = On::new(&pass, Some("boat"));
    let sh = argv(&["-c", "cd \"$0\" && drover arch-check", "/wt"]);
    let o = on.run_for("sh", &sh, "", gate).unwrap();
    assert_eq!(o.code, 0, "the check runs to its own limit: {}", o.stderr);
    assert!(pass.late().is_empty(), "no call was killed: {:?}", pass.late());
    let next = on.run("true", &[]).unwrap();
    assert!(!next.stderr.contains("not sent"), "the host stays up: {}", next.stderr);
    assert_eq!(
        *ssh.limits.lock().unwrap(),
        [gate, call],
        "an unnamed limit keeps ssh_call_secs"
    );
}
