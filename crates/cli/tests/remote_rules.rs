//! #592: a remote host keeps no fleet.toml: spawn resolves the lane's hook rules on the main host and ships them beside
//! the worktree, so the hook there judges a fleet that host never heard of as the main host would.
mod common;
use common::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const NOW: u64 = 1_000_000;
const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/herdsman-ssh-%C -oControlPersist=10m bazzite -- ";

/// CONFIG on Claude, mini (this host) and bazzite (remote), both probed.
fn setup(name: &str) -> herdsman::Ctx {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let c = ctx(
        &tmpdir(name),
        &format!("{base}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n"),
    );
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": NOW, "seen": NOW, "up": true,
                    "load": 0.1, "mem": 12.0, "disk": 200.0},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    c
}

/// A fresh lane on bazzite: its clone, worktree, files, pane and agent start all answer.
fn remote_fake() -> Fake {
    let ws = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr --machine bazzite agent list", NO_AGENTS)
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply(&format!("{SSH}'cat' '/proc/loadavg'"), "0.4 1 1 1/9 42")
        .reply(&format!("{SSH}'nproc'"), "4")
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply(&format!("{SSH}'git' "), "")
        .reply(&format!("{SSH}'sh' '-c' 'mkdir"), "")
        .reply(&format!("{SSH}'sh' '-c' 'umask 077"), "")
        .reply(&format!("{SSH}'rm' '-f'"), "")
        .reply("herdr --machine bazzite worktree create", ws)
        .reply(
            "herdr --machine bazzite pane list --workspace w9",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr --machine bazzite pane run w9:p1 export", "{}")
        .reply("herdr --machine bazzite agent start sendmeter-impl-12", "{}")
        .reply("herdr --machine bazzite agent wait sendmeter-impl-12", "{}")
}

/// The lane's environment on bazzite as the spawn left it: each `export`ed var, a value naming a file the spawn wrote
/// there pointing at that file's copy under `remote`.
fn lane_env(r: &Fake, remote: &Path) -> BTreeMap<String, String> {
    let written: BTreeMap<String, String> = r
        .inputs
        .borrow()
        .iter()
        .filter_map(|(cmd, text)| {
            let path = cmd.strip_suffix('\'')?.rsplit_once("'sh' '")?.1;
            Some((path.to_string(), text.clone()))
        })
        .collect();
    let calls = r.calls.borrow();
    let export = calls.iter().find(|c| c.contains("pane run w9:p1 export")).unwrap();
    let mut env = BTreeMap::new();
    for (i, (k, v)) in export.split_whitespace().filter_map(|w| w.split_once('=')).enumerate() {
        let v = v.trim_matches('\'');
        let v = match written.get(v) {
            Some(text) => {
                let copy = remote.join(format!("file{i}"));
                std::fs::write(&copy, text).unwrap();
                copy.display().to_string()
            }
            None => v.to_string(),
        };
        env.insert(k.to_string(), v);
    }
    env
}

/// `herdsman hook --role impl` on bazzite: the lane's env, that host's own `fleet.toml` (`None`: none at all).
fn hook_there(env: &BTreeMap<String, String>, remote: &Path, toml: Option<&str>, cmd: &str) -> String {
    let conf = remote.join("fleet.toml");
    let _ = std::fs::remove_file(&conf);
    if let Some(t) = toml {
        std::fs::write(&conf, t).unwrap();
    }
    let mut c = Command::new(env!("CARGO_BIN_EXE_herdsman"));
    c.env_remove("HERDSMAN_SCRATCH_TARGET").env_remove("FLEET_LANE");
    for k in ["HERDSMAN_LANE", "HERDSMAN_HOST", "HERDSMAN_RULES"] {
        c.env_remove(k);
    }
    for (k, v) in env
        .iter()
        .filter(|(k, _)| !["HERDSMAN_OUTBOX", "HERDSMAN_SERVICE"].contains(&k.as_str()))
    {
        c.env(k, v);
    }
    let mut child = c
        .args(["hook", "--role", "impl"])
        .current_dir(remote)
        .env("FLEET_CONFIG", &conf)
        .env("FLEET_STATE", remote.join("state/events.jsonl"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let input =
        json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": "/", "tool_input": {"command": cmd}});
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

#[test]
fn a_remote_lanes_hook_judges_a_fleet_its_host_never_heard_of() {
    let c = setup("remote-rules");
    let r = remote_fake();
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let remote = tmpdir("remote-rules-bazzite");
    let env = lane_env(&r, &remote);
    // bazzite was bootstrapped before sendmeter was added: its own fleet.toml knows only another fleet.
    let stale = "[fleet.other]\nrepo = \"o/other\"\nlocal = \"/tmp/other\"\nintegration = \"staging\"\n";
    for toml in [Some(stale), None] {
        let allowed = hook_there(&env, &remote, toml, "git status");
        assert_eq!(allowed, "", "{toml:?}: {allowed}");
        let push = hook_there(&env, &remote, toml, "git push origin staging");
        assert!(
            push.contains("\"deny\"") && push.contains("staging") && !push.contains("HOOK_FAILED_CLOSED"),
            "{toml:?}: {push}"
        );
        let docker = hook_there(&env, &remote, toml, "docker ps");
        assert!(docker.contains("NO_CONTAINERS"), "{toml:?}: {docker}");
    }
}
