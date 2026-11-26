mod common;
use common::{tmpdir, CONFIG};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const JOB: &str = "echo $$ > \"pid-$1\"; echo \"start $1\" >> log; for i in $(seq 200); do [ -f \"go-$1\" ] || [ ! -f \"$0\" ] && break; sleep 0.05; done; [ -f \"go-$1\" ] || exit 1; echo \"end $1\" >> log\n";

const IDLE: &str = "min_cpu_idle = -1.0\nmin_free_gb = 0.0\nmin_free_mem_gb = 0.0\n";

fn lane(host: &str) -> PathBuf {
    host_with(IDLE, host)
}

fn host_with(limits: &str, host: &str) -> PathBuf {
    let d = tmpdir("heavy");
    let heavy = "\n[fleet.sendmeter.heavy]\nlint = [\"sh lint.sh*\"]\ntest = [\"*test.sh*\"]\n";
    let conf = CONFIG.replace("[limits]\n", &format!("[limits]\n{limits}"));
    let cfg = format!("{conf}{heavy}\n[hosts.mini]\nheavy_max = 2\n{host}\n");
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

struct Job(Option<Child>);

impl Drop for Job {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            drover_sys::kill_group(c.id());
            c.wait().unwrap();
        }
    }
}

fn start(d: &Path, cmd: &str) -> Job {
    let out: Value = serde_json::from_str(&hook(d, cmd)).unwrap();
    let wrapped = out["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .unwrap()
        .to_string();
    let n = tickets(d).len();
    let child = in_lane("bash", d)
        .args(["-c", &wrapped])
        .process_group(0)
        .spawn()
        .unwrap();
    let job = Job(Some(child));
    until("its ticket", || tickets(d).len() > n);
    job
}

fn go(d: &Path, job: &str) {
    std::fs::write(d.join(format!("go-{job}")), "").unwrap();
}

fn alive(pid: &str) -> bool {
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", pid.trim()])
        .output()
        .unwrap();
    out.status.success() && !String::from_utf8_lossy(&out.stdout).trim_start().starts_with('Z')
}

fn finish(jobs: Vec<Job>) {
    for mut j in jobs {
        let mut c = j.0.take().unwrap();
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
    let kids: Vec<Job> = ["a", "b", "c", "e"]
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
    drop(holders.remove(0));
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
    assert!(phone.wants(late[0]), "a default phone notifier alerts on it");
}

#[test]
fn a_dropped_job_ends_with_its_harness() {
    let d = lane("");
    let job = start(&d, "sh test.sh a");
    until("a's start", || log(&d).contains("start a"));
    let pid = std::fs::read_to_string(d.join("pid-a")).unwrap();
    drop(job);
    let end = Instant::now() + Duration::from_secs(5);
    while alive(&pid) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    let gone = !alive(&pid);
    Command::new("kill").args(["-9", pid.trim()]).status().unwrap();
    assert!(gone, "job {} outlived its harness", pid.trim());
}

fn hold(d: &Path, wait: &str, cmd: &[&str]) -> Command {
    let mut c = in_lane(env!("CARGO_BIN_EXE_drover"), d);
    c.env_remove("DROVER_LANE")
        .env_remove("DROVER_HOST")
        .env("GITHUB_REPOSITORY", "acme/det")
        .args(["hold", "--wait", wait, "--"])
        .args(cmd);
    c
}

fn refused(d: &Path) -> String {
    let out = hold(d, "0", &["sh", "test.sh", "h"]).output().unwrap();
    assert_eq!(out.status.code(), Some(75), "{out:?}");
    assert!(!log(d).contains("start h"), "{}", log(d));
    String::from_utf8(out.stderr).unwrap()
}

#[test]
fn hold_runs_its_command_and_returns_its_exit_code() {
    let d = lane("");
    assert_eq!(hold(&d, "1", &["true"]).status().unwrap().code(), Some(0));
    let seven = hold(&d, "1", &["sh", "-c", "exit 7"]).status().unwrap();
    assert_eq!(seven.code(), Some(7));
    assert_eq!(tickets(&d), [] as [drover::heavy::Ticket; 0]);
}

#[test]
fn hold_with_every_slot_taken_gives_up_with_75_and_the_reason() {
    let d = lane("");
    let kids = ["a", "b"].map(|j| start(&d, &format!("sh test.sh {j}")));
    until("two running", || log(&d).lines().count() == 2);
    let why = refused(&d);
    assert!(why.contains("every heavy slot is taken"), "{why}");
    assert_eq!(tickets(&d).len(), 2);
    for j in ["a", "b"] {
        go(&d, j);
    }
    finish(kids.into());
}

#[test]
fn hold_on_a_host_over_its_admission_limits_gives_up_with_75_and_the_reason() {
    let d = host_with("min_cpu_idle = 101.0\nmin_free_gb = 0.0\nmin_free_mem_gb = 0.0\n", "");
    let why = refused(&d);
    assert!(why.contains("HOST_BUSY") && why.contains("cpu idle"), "{why}");
    assert_eq!(tickets(&d), [] as [drover::heavy::Ticket; 0]);
}

#[test]
fn a_lanes_heavy_command_waits_behind_a_running_hold() {
    let d = lane("");
    let child = hold(&d, "1", &["sh", "test.sh", "h"]).process_group(0).spawn().unwrap();
    let h = Job(Some(child));
    until("h's start", || log(&d).contains("start h"));
    let t = tickets(&d);
    assert_eq!(
        (t[0].lane.as_str(), t[0].kind.as_str(), t[0].running),
        ("ci:det", "ci", true)
    );
    let a = start(&d, "sh test.sh a");
    until("a's start", || log(&d).contains("start a"));
    let b = start(&d, "sh test.sh b");
    std::thread::sleep(Duration::from_millis(600));
    assert!(!log(&d).contains("start b"), "{}", log(&d));
    go(&d, "h");
    until("b's start", || log(&d).contains("start b"));
    for j in ["a", "b"] {
        go(&d, j);
    }
    finish(vec![h, a, b]);
}

const ROUTE: &str = "\n[fleet.sendmeter.remote.test]\ntag = \"linux\"\nwait = 5\nsetup = \"make deps\"\nenv = [\"CARGO_BUILD_JOBS\"]\nout = [\"target/shots\"]\n";

fn routed() -> (PathBuf, String) {
    let d = lane("");
    let cfg = std::fs::read_to_string(d.join("fleet.toml")).unwrap();
    std::fs::write(
        d.join("fleet.toml"),
        cfg.replace("\n[hosts.mini]", &format!("{ROUTE}\n[hosts.mini]")),
    )
    .unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(&d)
        .status()
        .unwrap()
        .success());
    let top = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&d)
        .output()
        .unwrap();
    (d, String::from_utf8(top.stdout).unwrap().trim().to_string())
}

fn wrapped(d: &Path, cmd: &str) -> String {
    let out: Value = serde_json::from_str(&hook(d, cmd)).unwrap();
    out["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .unwrap()
        .to_string()
}

fn fake_runners(d: &Path, code: Option<i32>) -> String {
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    if let Some(c) = code {
        let f = bin.join("runners");
        std::fs::write(&f, format!("#!/bin/sh\necho \"$@\" >> runners.log\nexit {c}\n")).unwrap();
        Command::new("chmod").args(["755"]).arg(&f).status().unwrap();
    }
    format!("{}:/usr/bin:/bin", bin.display())
}

#[test]
fn a_routed_kind_becomes_the_remote_wrapper_with_its_config() {
    let (d, top) = routed();
    let w = wrapped(&d, "sh test.sh a");
    let flags = format!(
        "'--remote' 'linux' '--dir' '{top}' '--remote-wait' '5' '--setup' 'make deps' '--env' 'CARGO_BUILD_JOBS' '--out' 'target/shots' '--' 'sh test.sh a'"
    );
    assert!(
        w.contains("'heavy' 'test' '--max' '2' '--wait' '30' ") && w.ends_with(&flags),
        "{w}"
    );
    let lint = wrapped(&d, "sh lint.sh a");
    assert!(
        lint.ends_with("'heavy' 'lint' '--max' '2' '--wait' '30' '--' 'sh lint.sh a'"),
        "an unrouted kind: {lint}"
    );
    let split = wrapped(&d, "git log -1 && sh test.sh a");
    assert!(
        split.starts_with("git log -1 && ") && split.contains("'--remote' 'linux'"),
        "{split}"
    );
    for never in ["npx nx affected -t sh test.sh", "sh test.sh a < input"] {
        let w = wrapped(&d, never);
        assert!(w.contains("'heavy' 'test'") && !w.contains("--remote"), "{never}: {w}");
    }
}

#[test]
fn an_unrouted_fleets_rewrite_is_unchanged() {
    let d = lane("");
    let w = wrapped(&d, "sh test.sh a");
    assert!(
        w.ends_with("'heavy' 'test' '--max' '2' '--wait' '30' '--' 'sh test.sh a'"),
        "{w}"
    );
}

#[test]
fn the_remote_code_passes_through_without_a_local_run() {
    for code in [0, 3] {
        let (d, top) = routed();
        let path = fake_runners(&d, Some(code));
        let st = in_lane("bash", &d)
            .env("PATH", path)
            .args(["-c", &wrapped(&d, "sh test.sh a")])
            .status()
            .unwrap();
        assert_eq!(st.code(), Some(code));
        assert_eq!(log(&d), "", "never run here");
        assert_eq!(tickets(&d), [] as [drover::heavy::Ticket; 0], "no slot held");
        let args = std::fs::read_to_string(d.join("runners.log")).unwrap();
        let want = format!("run --tag linux --dir {top} --wait 5 --setup make deps --env CARGO_BUILD_JOBS --out target/shots -- bash -c sh test.sh a\n");
        assert_eq!(args, want);
    }
}

#[test]
fn a_reserved_code_or_no_runners_runs_here_in_a_slot_and_says_why() {
    for (code, why) in [
        (Some(75), "runners: exit 75, running here"),
        (Some(76), "runners: exit 76, running here"),
        (Some(64), "runners: exit 64, running here"),
        (Some(78), "runners: exit 78, running here"),
        (None, "runners: not found, running here"),
    ] {
        let (d, _) = routed();
        let path = fake_runners(&d, code);
        let mut child = in_lane("bash", &d)
            .env("PATH", path)
            .args(["-c", &wrapped(&d, "sh test.sh a")])
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let mut err = child.stderr.take().unwrap();
        let job = Job(Some(child));
        until("its slot ticket", || !tickets(&d).is_empty());
        go(&d, "a");
        finish(vec![job]);
        let mut said = String::new();
        std::io::Read::read_to_string(&mut err, &mut said).unwrap();
        assert_eq!(said.trim(), why, "{code:?}");
        assert!(log(&d).contains("end a"), "{code:?}: {}", log(&d));
        assert_eq!(code.is_some(), d.join("runners.log").exists());
    }
}

#[test]
fn config_load_refuses_a_routed_pattern_that_hits_the_never_routed_list() {
    let base = "[fleet.acme]\nrepo = \"acme/app\"\nlocal = \"/x/app\"\nintegration = \"staging\"\n";
    let with = |heavy: &str, extra: &str| {
        format!("{base}{extra}[fleet.acme.heavy]\nbuild = [{heavy}]\n[fleet.acme.remote.build]\ntag = \"linux\"\n")
    };
    assert!(drover::config::Config::parse(&with("\"cargo test*\"", "")).is_ok());
    for (heavy, extra, hit) in [
        ("\"cargo test*\", \"npx nx affected*\"", "", "*nx affected*"),
        ("\"git log*\"", "", "git *"),
        ("\"*\"", "", "git *"),
        (
            "\"cargo test*\"",
            "remote_deny = [\"cargo test --doc*\"]\n",
            "cargo test --doc*",
        ),
    ] {
        let e = drover::config::Config::parse(&with(heavy, extra))
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("fleet.acme.remote.build") && e.contains(&format!("hits never-routed \"{hit}\"")),
            "{e}"
        );
    }
}

fn gated() -> PathBuf {
    let d = lane("");
    let cfg = std::fs::read_to_string(d.join("fleet.toml")).unwrap();
    let cfg = cfg
        .replace(
            "test = [\"*test.sh*\"]\n",
            "test = [\"*test.sh*\"]\ncargo = [\"sh test.sh*\"]\n",
        )
        .replace(
            "\n[hosts.mini]",
            "\n[fleet.sendmeter.remote.cargo]\ntag = \"linux\"\n\n[hosts.mini]",
        );
    std::fs::write(d.join("fleet.toml"), cfg).unwrap();
    d
}

fn routed_hold(d: &Path, path: &str, cmd: &[&str]) -> Command {
    let mut c = hold(d, "1", cmd);
    c.env("GITHUB_REPOSITORY", "jirathip-dev/sendmeter").env("PATH", path);
    c
}

#[test]
fn a_routed_fleets_hold_runs_on_the_build_machine_and_holds_no_slot() {
    for code in [0, 3] {
        let d = gated();
        let path = fake_runners(&d, Some(code));
        let st = routed_hold(&d, &path, &["sh", "test.sh", "h"]).status().unwrap();
        assert_eq!(st.code(), Some(code));
        assert_eq!(log(&d), "", "never run here");
        assert_eq!(tickets(&d), [] as [drover::heavy::Ticket; 0], "no slot held");
        let here = d.canonicalize().unwrap();
        let args = std::fs::read_to_string(d.join("runners.log")).unwrap();
        assert_eq!(
            args,
            format!(
                "run --tag linux --dir {} -- bash -c 'sh' 'test.sh' 'h'\n",
                here.display()
            )
        );
    }
    let d = gated();
    let path = fake_runners(&d, Some(5));
    let mut c = routed_hold(&d, &path, &["sh", "test.sh", "h"]);
    c.env_remove("GITHUB_REPOSITORY")
        .env("DROVER_LANE", "sendmeter:24:impl");
    assert_eq!(c.status().unwrap().code(), Some(5), "a lane's hold routes by its fleet");
    assert_eq!(log(&d), "");
}

#[test]
fn a_routed_hold_on_a_reserved_code_runs_here_in_a_slot_and_says_why() {
    let d = gated();
    let path = fake_runners(&d, Some(75));
    let mut child = routed_hold(&d, &path, &["sh", "test.sh", "h"])
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let mut err = child.stderr.take().unwrap();
    let job = Job(Some(child));
    until("h's start", || log(&d).contains("start h"));
    let t = tickets(&d);
    assert_eq!(
        (t[0].lane.as_str(), t[0].kind.as_str(), t[0].running),
        ("ci:sendmeter", "ci", true)
    );
    go(&d, "h");
    finish(vec![job]);
    let mut said = String::new();
    std::io::Read::read_to_string(&mut err, &mut said).unwrap();
    assert_eq!(said.trim(), "runners: exit 75, running here");
    assert!(d.join("runners.log").exists());
}

#[test]
fn a_hold_never_routes_a_never_routed_command_or_an_unrouted_fleets() {
    let d = gated();
    let path = fake_runners(&d, Some(0));
    let st = routed_hold(&d, &path, &["git", "--version"]).status().unwrap();
    assert_eq!(st.code(), Some(0));
    assert!(!d.join("runners.log").exists(), "git never leaves this host");
    let d = lane("");
    let path = fake_runners(&d, Some(0));
    let st = routed_hold(&d, &path, &["sh", "-c", "exit 7"]).status().unwrap();
    assert_eq!(st.code(), Some(7));
    assert!(!d.join("runners.log").exists());
}

#[test]
fn a_routed_fleets_gate_checks_go_to_the_build_machine_under_the_cargo_kind() {
    let d = gated();
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(&d)
        .status()
        .unwrap();
    for gate in ["just check"] {
        let w = wrapped(&d, gate);
        assert!(
            w.contains("'heavy' 'cargo'") && w.contains("'--remote' 'linux'") && w.ends_with(&format!("'--' '{gate}'")),
            "{gate}: {w}"
        );
    }
    let d = lane("");
    assert_eq!(
        hook(&d, "just check"),
        "",
        "an unrouted fleet's gate check is unchanged"
    );
}

#[test]
fn a_gatekeeper_check_never_goes_to_a_build_machine() {
    let d = gated();
    let gate = "./.gatekeeper/run check --gate core/fmt";
    assert_eq!(hook(&d, gate), "", "the hook leaves it on the lane's host");
    std::fs::create_dir_all(d.join(".gatekeeper")).unwrap();
    std::fs::write(d.join(".gatekeeper/run"), "#!/bin/sh\nexit 6\n").unwrap();
    Command::new("chmod")
        .args(["755"])
        .arg(d.join(".gatekeeper/run"))
        .status()
        .unwrap();
    let path = fake_runners(&d, Some(0));
    let st = routed_hold(&d, &path, &["./.gatekeeper/run", "check"])
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(6), "it ran here");
    assert!(!d.join("runners.log").exists(), "never sent to runners run");
}

fn cargo_routed() -> (PathBuf, String, String) {
    let (d, top) = routed();
    let cfg = std::fs::read_to_string(d.join("fleet.toml")).unwrap();
    let route = "\n[fleet.sendmeter.remote.cargo]\ntag = \"linux\"\n\n[hosts.mini]";
    std::fs::write(d.join("fleet.toml"), cfg.replace("\n[hosts.mini]", route)).unwrap();
    let path = fake_runners(&d, Some(3));
    let cargo = d.join("bin/cargo");
    std::fs::write(&cargo, "#!/bin/sh\necho \"cargo-here $*\"\n").unwrap();
    Command::new("chmod").args(["755"]).arg(&cargo).status().unwrap();
    (d, top, path)
}

fn run_line(d: &Path, path: &str, cmd: &str) -> (Option<i32>, String, String) {
    let out = in_lane("bash", d)
        .env("PATH", path)
        .args(["-c", &wrapped(d, cmd)])
        .output()
        .unwrap();
    let s = |b: Vec<u8>| String::from_utf8(b).unwrap();
    (out.status.code(), s(out.stdout), s(out.stderr))
}

#[test]
fn a_mixed_line_routes_its_build_and_runs_the_rest_here() {
    let (d, top, path) = cargo_routed();
    let git = |a: &[&str]| assert!(Command::new("git").args(a).current_dir(&d).status().unwrap().success());
    std::fs::write(d.join("a.txt"), "one\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "a"]);
    std::fs::write(d.join("a.txt"), "two\n").unwrap();
    let o = tmpdir("heavy-out");
    let got = o.join("got");
    let line = format!("cargo test -p x > {}; git diff --stat", got.display());
    let (code, out, _) = run_line(&d, &path, &line);
    assert_eq!(code, Some(0));
    assert!(out.contains("a.txt | 2 +-"), "git diff ran here: {out}");
    let runners = std::fs::read_to_string(d.join("runners.log")).unwrap();
    assert_eq!(
        runners,
        format!("run --tag linux --dir {top} -- bash -c cargo test -p x\n")
    );
    assert!(got.exists(), "the build's redirect is written here");
    assert_eq!(log(&d), "");
    let (code, out, _) = run_line(
        &d,
        &path,
        "cargo test -p x && echo after; gh --version > /dev/null || true",
    );
    assert_eq!((code, out.contains("after")), (Some(0), false), "{out}");
    let (code, _, _) = run_line(&d, &path, "git --version > /dev/null && cargo test -p x");
    assert_eq!(code, Some(3), "the build's exit code is the line's");
}

#[test]
fn a_build_piped_or_reading_stdin_stays_here_and_says_why() {
    let (d, _, path) = cargo_routed();
    std::fs::write(d.join("in"), "").unwrap();
    for (cmd, why) in [
        ("cargo test | cat", "heavy: a pipe in the line, running here"),
        ("cargo test < in", "heavy: stdin (<) in the line, running here"),
    ] {
        let h = hook(&d, cmd);
        assert!(h.contains("updatedInput"), "{cmd}: {h}");
        let w = wrapped(&d, cmd);
        assert!(w.contains("'heavy' 'cargo'") && !w.contains("--remote"), "{cmd}: {w}");
        let (code, out, err) = run_line(&d, &path, cmd);
        assert_eq!(code, Some(0), "{cmd}: {err}");
        assert_eq!(err.trim(), why, "{cmd}");
        assert!(out.contains("cargo-here test"), "{cmd}: {out}");
        assert!(!d.join("runners.log").exists(), "{cmd}");
    }
}

#[test]
fn text_in_a_here_doc_or_quotes_is_no_heavy_kind() {
    let (d, _, _) = cargo_routed();
    for cmd in [
        "cat > /dev/null <<'EOF'\nrun cargo test first\ncargo test -p x\nEOF\ngit status",
        "gh issue create --title t --body 'fix; cargo test -p x'",
        "echo \"a && cargo test\" # cargo test",
    ] {
        assert_eq!(hook(&d, cmd), "", "{cmd}");
    }
}

#[test]
fn the_default_cargo_kind_routes_xtask_and_run() {
    let (d, top, _) = cargo_routed();
    for cmd in ["cargo xtask foo", "cargo run -p x"] {
        let w = wrapped(&d, cmd);
        let tail = format!("'--remote' 'linux' '--dir' '{top}' '--' '{cmd}'");
        assert!(w.contains("'heavy' 'cargo'") && w.ends_with(&tail), "{cmd}: {w}");
    }
}
