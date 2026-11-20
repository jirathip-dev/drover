use drover::hosts::{each, On, Pass, MUX_REFUSED};
use drover::runner::{argv, Out, Runner};
use std::sync::{Barrier, Condvar, Mutex};

const MAX_SESSIONS: usize = 10;
const CALLS: usize = 20;
const GRACE: std::time::Duration = std::time::Duration::from_millis(200);

#[derive(Default)]
struct Master {
    free: bool,
    open: usize,
    most: usize,
    done: usize,
    refused: usize,
    bare: usize,
}

#[derive(Default)]
struct Mux {
    m: Mutex<Master>,
    cv: Condvar,
    hold: bool,
    refuse_first: Mutex<usize>,
    sleeps: Mutex<Vec<u64>>,
}

impl Runner for Mux {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], _input: &str) -> anyhow::Result<Out> {
        assert_eq!(prog, "ssh");
        let refused = Out {
            code: 255,
            stderr: format!("mux_client_request_session: session request failed: {MUX_REFUSED}\n"),
            ..Out::default()
        };
        let mut m = self.m.lock().unwrap();
        if args.iter().any(|a| a == "-oControlPath=none") {
            m.bare += 1;
        }
        let mut first = self.refuse_first.lock().unwrap();
        if m.open >= MAX_SESSIONS || *first > 0 {
            *first = first.saturating_sub(1);
            m.refused += 1;
            return Ok(refused);
        }
        drop(first);
        m.open += 1;
        m.most = m.most.max(m.open);
        self.cv.notify_all();
        let cap = drover::config::limits().ssh_sessions;
        while self.hold && !m.free && m.open < MAX_SESSIONS {
            if m.open < cap.min(CALLS - m.done) {
                m = self.cv.wait(m).unwrap();
                continue;
            }
            m = self
                .cv
                .wait_timeout_while(m, GRACE, |m| m.open < MAX_SESSIONS)
                .unwrap()
                .0;
            m.free = true;
        }
        self.cv.notify_all();
        m.open -= 1;
        m.done += 1;
        self.cv.notify_all();
        Ok(Out {
            stdout: "ok\n".into(),
            ..Out::default()
        })
    }
    fn sleep(&self, secs: u64) {
        self.sleeps.lock().unwrap().push(secs);
    }
}

#[test]
fn twenty_parallel_calls_to_one_host_stay_under_its_max_sessions_and_all_succeed() {
    let mux = Mux {
        hold: true,
        ..Mux::default()
    };
    let pass = Pass::fast(&mux);
    let start = Barrier::new(CALLS);
    let outs = each(0..CALLS, |i| {
        start.wait();
        On::new(&pass, Some("boat"))
            .run("echo", &argv(&[&i.to_string()]))
            .unwrap()
    });
    let m = mux.m.lock().unwrap();
    for o in &outs {
        assert_eq!((o.code, o.stdout.as_str()), (0, "ok\n"), "{}", o.stderr);
        assert!(!o.stderr.contains("host offline"), "{}", o.stderr);
    }
    let cap = drover::config::limits().ssh_sessions;
    assert!(cap < MAX_SESSIONS);
    assert_eq!(m.most, cap, "at most ssh_sessions sessions open on the master at once");
    assert_eq!((m.refused, m.bare), (0, 0));
}

#[test]
fn a_session_the_master_refuses_runs_again_on_the_master_and_the_host_stays_up() {
    let mux = Mux {
        refuse_first: Mutex::new(2),
        ..Mux::default()
    };
    let pass = Pass::fast(&mux);
    let on = On::new(&pass, Some("boat"));
    let o = on.run("echo", &argv(&["1"])).unwrap();
    assert_eq!((o.code, o.stdout.as_str()), (0, "ok\n"), "{}", o.stderr);
    let again = on.run("echo", &argv(&["2"])).unwrap();
    assert_eq!(again.code, 0, "{}", again.stderr);
    let m = mux.m.lock().unwrap();
    assert_eq!(
        (m.refused, m.bare, m.done),
        (2, 0, 2),
        "retried on the master, never bare"
    );
    let wait = drover::config::limits().ssh_retry_secs;
    assert_eq!(*mux.sleeps.lock().unwrap(), [wait, wait]);
}

struct Sshd(&'static str);

impl Runner for Sshd {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let line = args.last().unwrap();
        assert!(prog == "ssh" && args.iter().any(|a| a == "boat"), "{args:?}");
        assert!(
            line.starts_with("'sh' '-c' '{ sudo -n sshd -T") && line.contains("@notty"),
            "{line}"
        );
        Ok(Out {
            stdout: self.0.into(),
            ..Out::default()
        })
    }
}

#[test]
fn the_sshd_row_flags_no_keepalive_few_sessions_and_stale_ones_and_passes_a_bootstrapped_host() {
    let row = |out| drover::hosts::sshd_row(&On::new(&Sshd(out), Some("boat")), "boat");
    assert_eq!(
        row("maxsessions 10\nstale 158\n"),
        "! host boat sshd — ClientAliveInterval 0, ClientAliveCountMax 3, MaxSessions 10 (want 16+), \
         158 stale notty sessions (at most 20) → hosts bootstrap boat, then restart sshd there"
    );
    assert_eq!(
        row("clientaliveinterval 60\nclientalivecountmax 3\nmaxsessions 32\nstale 2\n"),
        "✓ host boat sshd — ClientAliveInterval 60, ClientAliveCountMax 3, MaxSessions 32 (want 16+), \
         2 stale notty sessions (at most 20)"
    );
    assert!(row("").starts_with("! host boat sshd — unreadable"));
}
