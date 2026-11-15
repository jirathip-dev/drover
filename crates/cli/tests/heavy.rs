mod common;
use common::{tmpdir, CONFIG};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const JOB: &str = "echo \"start $1\" >> log; while [ ! -f \"go-$1\" ]; do sleep 0.05; done; echo \"end $1\" >> log\n";

fn lane(host: &str) -> PathBuf {
    let d = tmpdir("heavy");
    let heavy = "\n[fleet.sendmeter.heavy]\nlint = [\"sh lint.sh*\"]\ntest = [\"*test.sh*\"]\n";
    let cfg = format!("{CONFIG}{heavy}\n[hosts.mini]\nheavy_max = 2\n{host}\n");
    std::fs::write(d.join("fleet.toml"), cfg).unwrap();
    std::fs::write(d.join("lint.sh"), JOB).unwrap();
    std::fs::write(d.join("test.sh"), JOB).unwrap();
    d
}

fn in_lane(prog: &str, d: &Path) -> Command {
    let mut c = Command::new(prog);
    for k in [
        "DROVER_SCRATCH_TARGET",
        "DROVER_RULES",
        "DROVER_SERVICE",
        "DROVER_OUTBOX",
        "DROVER_THREAD",
    ] {
        c.env_remove(k);
    }
    c.current_dir(d)
        .env("DROVER_LANE", "sendmeter:24:impl")
        .env("DROVER_HOST", "mini")
        .env("FLEET_CONFIG", d.join("fleet.toml"))
        .env("FLEET_STATE", d.join("state/events.jsonl"));
    c
}

fn hook(d: &Path, cmd: &str) -> String {
    let mut child = in_lane(env!("CARGO_BIN_EXE_drover"), d)
        .args(["hook", "--role", "impl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": d, "tool_input": {"command": cmd}});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

fn tickets(d: &Path) -> Vec<drover::heavy::Ticket> {
    drover::heavy::line(&d.join("state/heavy"))
        .into_iter()
        .map(|(_, t)| t)
        .collect()
}

fn until(what: &str, ok: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !ok() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn log(d: &Path) -> String {
    std::fs::read_to_string(d.join("log")).unwrap_or_default()
}

fn start(d: &Path, cmd: &str) -> Child {
    let out: Value = serde_json::from_str(&hook(d, cmd)).unwrap();
    let wrapped = out["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .unwrap()
        .to_string();
    let n = tickets(d).len();
    let child = in_lane("bash", d).args(["-c", &wrapped]).spawn().unwrap();
    until("its ticket", || tickets(d).len() > n);
    child
}

fn go(d: &Path, job: &str) {
    std::fs::write(d.join(format!("go-{job}")), "").unwrap();
}

fn finish(children: Vec<Child>) {
    for mut c in children {
        assert!(c.wait().unwrap().success());
    }
}

#[test]
fn unmatched_commands_are_unchanged() {
    let d = lane("");
    assert_eq!(hook(&d, "cargo fmt && ls"), "");
    assert!(hook(&d, "FOO=1 sh lint.sh a").contains("updatedInput"));
    assert!(!d.join("state/heavy").exists());
}

#[test]
fn extra_commands_wait_first_come_first_served_and_all_finish() {
    let d = lane("");
    let kids: Vec<Child> = ["a", "b", "c", "e"]
        .map(|j| start(&d, &format!("sh test.sh {j}")))
        .into();
    until("two running", || log(&d).lines().count() == 2);
    let t = tickets(&d);
    assert_eq!(t.iter().filter(|t| t.running).count(), 2);
    let waiting: Vec<_> = t.iter().filter(|t| !t.running).collect();
    assert_eq!(
        waiting.iter().map(|t| t.cmd.as_str()).collect::<Vec<_>>(),
        ["sh test.sh c", "sh test.sh e"]
    );
    assert!(waiting
        .iter()
        .all(|t| t.lane == "sendmeter:24:impl" && t.kind == "test" && t.since > 0));
    go(&d, "a");
    until("c's start", || log(&d).contains("start c"));
    std::thread::sleep(Duration::from_millis(600));
    assert!(!log(&d).contains("start e"), "{}", log(&d));
    for j in ["b", "c", "e"] {
        go(&d, j);
    }
    finish(kids);
    let l = log(&d);
    assert!(
        l.find("end a") < l.find("start c") && l.find("start c") < l.find("start e"),
        "{l}"
    );
    assert_eq!(l.matches("end ").count(), 4);
    assert_eq!(tickets(&d), [] as [drover::heavy::Ticket; 0]);
}

#[test]
fn a_kind_limit_holds_its_kind_while_others_run() {
    let d = lane("heavy_kind_max = { lint = 1 }");
    let a = start(&d, "sh lint.sh a");
    let b = start(&d, "sh lint.sh b");
    let c = start(&d, "sh test.sh c");
    until("c's start", || log(&d).contains("start c"));
    assert!(!log(&d).contains("start b"), "{}", log(&d));
    for j in ["a", "b", "c"] {
        go(&d, j);
    }
    finish(vec![a, b, c]);
    assert!(log(&d).contains("end b"));
}

#[test]
fn a_killed_holder_frees_its_slot() {
    let d = lane("");
    let mut holders = vec![start(&d, "sh test.sh a"), start(&d, "sh test.sh b")];
    let c = start(&d, "sh test.sh c");
    until("two running", || log(&d).lines().count() == 2);
    let mut a = holders.remove(0);
    a.kill().unwrap();
    a.wait().unwrap();
    until("c's start", || log(&d).contains("start c"));
    for j in ["a", "b", "c"] {
        go(&d, j);
    }
    finish(holders);
    finish(vec![c]);
}

#[test]
fn a_wait_past_the_limit_runs_and_logs_heavy_timeout_once() {
    let d = lane("heavy_wait_minutes = 0");
    let kids = ["a", "b", "c"].map(|j| start(&d, &format!("sh test.sh {j}")));
    until("three running", || log(&d).lines().count() == 3);
    for j in ["a", "b", "c"] {
        go(&d, j);
    }
    finish(kids.into());
    let state = d.join("state/events.jsonl");
    let ev = drover::events::read(&state).unwrap();
    let late: Vec<_> = ev.iter().filter(|e| e.kind == "heavy-timeout").collect();
    assert_eq!(late.len(), 1, "{late:?}");
    assert_eq!(late[0].str("cmd"), Some("sh test.sh c"));
    assert_eq!(late[0].str("kind"), Some("test"));
    let phone: drover::config::Notify = toml::from_str("cmd = \"\"\ntype = \"webpush\"").unwrap();
    assert!(phone.wants("heavy-timeout"), "a default phone notifier alerts on it");
}
