mod common;
use common::{tmpdir, CONFIG};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn sh(d: &Path, cwd: &Path, cmd: &str) -> String {
    let call = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": cwd,
        "tool_input": {"command": cmd}});
    let mut child = Command::new(env!("CARGO_BIN_EXE_drover"))
        .args(["hook", "--role", "orchestrator"])
        .env_remove("FLEET_LANE")
        .env("DROVER_LANE", "sendmeter:-:orchestrator")
        .env("DROVER_THREAD", "t6")
        .env("FLEET_CONFIG", d.join("fleet.toml"))
        .env("FLEET_STATE", d.join("state/events.jsonl"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(call.to_string().as_bytes())
        .unwrap();
    String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
}

#[test]
fn a_orchestrator_copies_from_the_state_dir_into_its_own_folder() {
    let d = tmpdir("hook-1477");
    std::fs::write(d.join("fleet.toml"), CONFIG).unwrap();
    let (st, c) = (d.join("state"), d.join("state/orchestrator"));
    std::fs::create_dir_all(c.join("t6")).unwrap();
    let (own, other, s) = (
        c.join("t6"),
        c.join("t5").display().to_string(),
        st.display().to_string(),
    );
    let o = own.display().to_string();
    for (cwd, cmd) in [
        (&own, format!("cp {other}/mock.png {o}/")),
        (&own, format!("cp -r {other}/ui {o}/ui")),
        (&own, format!("cat {s}/events.jsonl > {o}/events.txt")),
        (&own, format!("grep handoff {other}/.thread.md >> {o}/notes.md")),
        (&st, format!("cp /w/a.png {o}/")),
        (&st, format!("cat {other}/a.md > {o}/a.md")),
    ] {
        assert_eq!(sh(&d, cwd, &cmd), "", "{}: {cmd}", cwd.display());
    }
    for (cwd, cmd) in [
        (&own, format!("cp {o}/a.png {other}/")),
        (&own, format!("cp {other}/a.png {o}/.thread.md")),
        (&own, format!("mv {other}/a.png {o}/")),
        (&own, format!("cp {other}/.brief.md {o}/")),
        (&own, format!("cp {other}/.thread.md {o}")),
        (&own, format!("cp -r {other}/ {o}/")),
        (&own, format!("cp -r {other}/. {o}")),
        (&own, format!("cp -l {s}/events.jsonl {o}/x")),
        (&own, format!("cp -s {s}/events.jsonl {o}/x")),
        (&own, format!("cp --link {s}/events.jsonl {o}/x")),
        (&own, format!("cp -t {s} {o}/a.png")),
        (&own, format!("cat {s}/events.jsonl > {other}/x")),
        (&own, format!("cat {s}/events.jsonl > {o}/x > {s}/y")),
        (&own, format!("cat a > {o}/.brief.md")),
        (&own, format!("cp a {s}/fleet.toml > {o}/x")),
        (&own, format!("touch {s}/fleet.toml > {o}/x")),
        (&own, format!("rsync a {s}/fleet.toml > {o}/x")),
        (&own, format!("curl -o {s}/fleet.toml u > {o}/x")),
        (&own, format!("wget -O {s}/fleet.toml u > {o}/x")),
        (&own, format!("sort -o {s}/fleet.toml a > {o}/x")),
        (&st, "cp /w/a.png ./a.png".to_string()),
        (&st, "echo x > a".to_string()),
    ] {
        assert!(sh(&d, cwd, &cmd).contains("is blocked"), "{}: {cmd}", cwd.display());
    }
}
