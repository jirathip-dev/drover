mod common;
use common::{tmpdir, CONFIG};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn drover(d: &Path, args: &[&str], lane: Option<&str>, raw: &str) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_drover"));
    cmd.env_remove("DROVER_LANE")
        .env_remove("FLEET_LANE")
        .env_remove("DROVER_SCRATCH_TARGET")
        .env_remove("DROVER_GUARD");
    if let Some(l) = lane {
        cmd.env("DROVER_LANE", l);
    }
    let mut child = cmd
        .args(args)
        .current_dir(d)
        .env("FLEET_CONFIG", d.join("fleet.toml"))
        .env("FLEET_STATE", d.join("state/events.jsonl"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(raw.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{args:?} exited {:?}", out.status);
    String::from_utf8(out.stdout).unwrap()
}

fn dir(name: &str, cfg: &str) -> std::path::PathBuf {
    let d = tmpdir(name);
    std::fs::create_dir_all(d.join("state")).unwrap();
    std::fs::write(d.join("fleet.toml"), cfg).unwrap();
    d
}

fn acp(d: &Path, lane: Option<&str>, raw: &str) -> Value {
    let out = drover(d, &["hook", "--acp"], lane, raw);
    assert_eq!(out.lines().count(), 1, "one verdict line: {out}");
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"))
}

fn request(d: &Path, tool_call: &Value) -> String {
    json!({"name": "sendmeter-impl-24", "labels": {"fleet": "sendmeter"}, "cwd": d, "tool_call": tool_call,
        "options": [{"id": "a", "name": "Allow", "kind": "allow_once"}, {"id": "r", "name": "Reject", "kind": "reject_once"}]})
    .to_string()
}

fn execute(cmd: &str) -> Value {
    json!({"toolCallId": "t1", "title": cmd, "kind": "execute", "rawInput": {"command": cmd}})
}

fn edit(path: &str) -> Value {
    json!({"toolCallId": "t2", "title": "Edit", "kind": "edit", "locations": [{"path": path}], "rawInput": {"file_path": path}})
}

fn judged(d: &Path, role: &str, tool_call: &Value) -> Option<String> {
    let v = acp(d, Some(&format!("sendmeter:24:{role}")), &request(d, tool_call));
    match v["decision"].as_str() {
        Some("allow") => None,
        Some("deny") => Some(v["reason"].as_str().unwrap().to_string()),
        _ => panic!("no decision: {v}"),
    }
}

fn hooked(d: &Path, role: &str, tool: &str, ti: &Value) -> Option<String> {
    let input = json!({"hook_event_name": "PreToolUse", "tool_name": tool, "cwd": d, "tool_input": ti});
    let out = drover(
        d,
        &["hook", "--role", role],
        Some(&format!("sendmeter:24:{role}")),
        &input.to_string(),
    );
    let v: Value = serde_json::from_str(&out).ok()?;
    Some(
        v["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()?
            .to_string(),
    )
}

#[test]
fn the_acp_mode_refuses_what_the_hook_format_refuses_rule_by_rule() {
    let d = dir("hook-acp-rules", CONFIG);
    let fleet = d.join("fleet.toml").display().to_string();
    let shell = [
        ("impl", "git push origin staging", "staging"),
        ("impl", "git push origin HEAD:main", "main"),
        ("impl", "gh pr merge 5", "merge"),
        ("impl", "gh issue comment 5 --body hi", "comment"),
        ("impl", "docker run alpine", "NO_CONTAINERS"),
        ("impl", "pkill cargo", "NO_BROAD_KILL"),
        ("impl", "bash -c \"$X\"", "OPAQUE_SHELL"),
        ("rev", "git commit -m x", "read-only"),
    ];
    for (role, cmd, why) in shell {
        let got = judged(&d, role, &execute(cmd)).unwrap_or_else(|| panic!("{cmd} allowed"));
        assert!(got.starts_with("drover: ") && got.contains(why), "{cmd}: {got}");
        let theirs = hooked(&d, role, "Bash", &json!({"command": cmd})).unwrap();
        assert_eq!(got, theirs, "{cmd}: both parsers, one rule");
    }
    let writes = [
        ("impl", fleet.as_str(), "protected"),
        ("rev", "src/main.rs", "reviewers are read-only"),
    ];
    for (role, path, why) in writes {
        let got = judged(&d, role, &edit(path)).unwrap_or_else(|| panic!("{path} allowed"));
        assert!(got.contains(why), "{path}: {got}");
        let theirs = hooked(&d, role, "Edit", &json!({"file_path": path})).unwrap();
        assert_eq!(got, theirs, "{path}: both parsers, one rule");
    }
    let mode =
        json!({"toolCallId": "t3", "title": "mode", "kind": "switch_mode", "rawInput": {"mode": "bypassPermissions"}});
    assert!(judged(&d, "impl", &mode).unwrap().contains("MODE_SWITCH"));
    let argv = json!({"toolCallId": "t4", "kind": "execute", "rawInput": {"command": ["bash", "-c", "gh pr merge 5"]}});
    assert!(judged(&d, "impl", &argv).unwrap().contains("merge"));
    let patch = "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-a\n+b\n*** End Patch";
    let patched = json!({"toolCallId": "t5", "kind": "edit", "rawInput": {"patch": patch}});
    assert!(judged(&d, "rev", &patched).unwrap().contains("not src/main.rs"));
    let other = json!({"toolCallId": "t6", "kind": "other", "rawInput": {"cmd": "gh pr merge 5"}});
    assert!(judged(&d, "impl", &other).unwrap().contains("merge"));
}

#[test]
fn the_acp_mode_allows_an_ordinary_call() {
    let d = dir("hook-acp-allow", CONFIG);
    assert_eq!(judged(&d, "impl", &execute("cargo test")), None);
    assert_eq!(judged(&d, "impl", &edit("src/main.rs")), None);
    assert_eq!(judged(&d, "rev", &edit("verdict.md")), None);
    for kind in ["read", "search", "think", "fetch"] {
        let call = json!({"toolCallId": "t", "kind": kind, "rawInput": {"path": "/etc"}});
        assert_eq!(judged(&d, "rev", &call), None, "{kind}");
    }
    let v = acp(&d, Some("sendmeter:24:impl"), &request(&d, &execute("ls")));
    assert_eq!(v, json!({"decision": "allow", "reason": ""}));
}

#[test]
fn the_acp_mode_fails_closed() {
    let d = dir("hook-acp-closed", CONFIG);
    let lane = Some("sendmeter:24:impl");
    let closed = |v: Value, what: &str| {
        assert_eq!(v["decision"], "deny", "{what}: {v}");
        assert!(
            v["reason"].as_str().unwrap().contains("HOOK_FAILED_CLOSED"),
            "{what}: {v}"
        );
    };
    closed(acp(&d, lane, "not json"), "not JSON");
    closed(acp(&d, lane, &request(&d, &Value::Null)), "no toolCall");
    let bare = json!({"toolCallId": "t", "kind": "execute", "rawInput": {}});
    closed(acp(&d, lane, &request(&d, &bare)), "an execute with no command");
    let nowhere = json!({"toolCallId": "t", "kind": "edit", "rawInput": {}});
    closed(acp(&d, lane, &request(&d, &nowhere)), "an edit with no path");
    closed(acp(&d, None, &request(&d, &execute("ls"))), "no DROVER_LANE");
    let broken = dir("hook-acp-broken", "[fleet.x\n");
    closed(
        acp(&broken, lane, &request(&broken, &execute("ls"))),
        "fleet.toml not loading",
    );
    let log = drover::events::read(&d.join("state/events.jsonl")).unwrap();
    assert!(
        log.iter().any(|e| e.kind == "hook-failed-closed"),
        "the failure is logged"
    );
}

#[test]
fn the_probe_mode_records_each_request_and_denies_it() {
    let d = dir("hook-acp-probe", CONFIG);
    let p = d.join("probe");
    std::fs::create_dir_all(&p).unwrap();
    let probe = p.to_string_lossy().into_owned();
    for call in [
        execute("touch x/ran-shell"),
        json!({"toolCallId": "t", "kind": "execute", "rawInput": {}}),
    ] {
        let out = drover(&d, &["hook", "--acp", "--probe", &probe], None, &request(&d, &call));
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["decision"], "deny", "{out}");
    }
    let asked = std::fs::read_to_string(p.join("asked")).unwrap();
    assert!(asked.starts_with("shell touch x/ran-shell\nunreadable "), "{asked}");
}
