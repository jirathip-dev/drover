mod common;
use common::*;
use drover::events::read;
use drover::serve::Server;
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

const T: u64 = 1_000_000;
const TOKEN: &str = "plugin-secret";
const OWN: &str = "Grill the owner before filing anything.";
const KEYS: [&str; 9] = [
    "name",
    "label",
    "agent",
    "instructions",
    "settings",
    "env",
    "policy",
    "hidden",
    "why",
];

fn setup(name: &str) -> (drover::Ctx, std::path::PathBuf) {
    let d = tmpdir(name);
    let own = d.join("conductor.md");
    std::fs::write(&own, OWN).unwrap();
    let cfg = CONFIG.replace("harness  =  \"hermes\"    # toggled in tests", "harness = \"claude\"")
        + &format!(
            "\n[orchestrator]\nname = \"Conductor\"\ninstructions = \"{}\"\n\n[orchestrator.ops]\nname = \"Ops\"\nfleets = [\"sendmeter\"]\n",
            own.display()
        );
    let mut c = ctx(&d, &cfg);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    std::fs::write(c.config.with_file_name("cooee-token"), TOKEN).unwrap();
    (c, own)
}

fn call(s: &Server, auth: bool, (method, path): (&str, &str), body: &str) -> (u16, Value) {
    let bearer = if auth {
        format!("Authorization: Bearer {TOKEN}\r\n")
    } else {
        String::new()
    };
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n{bearer}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (head[9..12].parse().unwrap(), serde_json::from_str(body).unwrap())
}

fn profiles(s: &Server) -> Vec<Value> {
    let (code, v) = call(s, true, ("GET", "/cooee/profiles"), "");
    assert_eq!(code, 200, "{v}");
    let list = v["profiles"].as_array().unwrap().clone();
    for p in &list {
        let keys: Vec<&String> = p.as_object().unwrap().keys().collect();
        assert!(keys.iter().all(|k| KEYS.contains(&k.as_str())), "{p}");
    }
    list
}

fn text(p: &Value) -> String {
    std::fs::read_to_string(p["instructions"].as_str().unwrap()).unwrap()
}

fn hook_of(p: &Value) -> String {
    let dir = std::path::Path::new(p["settings"].as_str().unwrap());
    let s: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
    let pre = s["hooks"]["PreToolUse"].as_array().unwrap();
    let cmds = pre.iter().flat_map(|m| m["hooks"].as_array().unwrap().iter());
    cmds.filter_map(|h| h["command"].as_str())
        .find(|c| c.contains(" hook --role "))
        .unwrap()
        .to_string()
}

fn chat_call(c: &drover::Ctx, p: &Value, thread: &str, armed: bool, cmd: &str) -> String {
    let mut sh = Command::new("sh");
    sh.args(["-c", &hook_of(p)])
        .env_remove("DROVER_LANE")
        .env_remove("FLEET_LANE")
        .env_remove("DROVER_THREAD")
        .env_remove("DROVER_ORCHESTRATOR")
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state);
    if armed {
        for (k, v) in p["env"].as_object().unwrap() {
            sh.env(k, v.as_str().unwrap().replace("{thread}", thread));
        }
    }
    let mut child = sh.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let call = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(call.to_string().as_bytes())
        .unwrap();
    String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
}

#[test]
fn each_orchestrator_is_served_with_the_owners_instructions_and_drovers_hook() {
    let (c, own) = setup("cooee-profiles");
    let r = Fake::default();
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));

    let (code, v) = call(&s, false, ("GET", "/cooee/profiles"), "");
    assert_eq!((code, v["line"].as_str()), (401, Some("the plugin token only")), "{v}");

    let list = profiles(&s);
    let names: Vec<&str> = list.iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["default", "ops"]);
    for p in &list {
        assert_eq!(p["hidden"], true, "{p}");
        assert!(p["why"].as_str().unwrap().contains("no drover PreToolUse hook"), "{p}");
        assert!(p.get("instructions").is_none() && p.get("settings").is_none(), "{p}");
    }

    assert_eq!(run(&c, &r, &["profiles", "sync"]).0, 0);
    let list = profiles(&s);
    let (p, ops) = (&list[0], &list[1]);
    assert_eq!(
        (p["label"].as_str(), ops["label"].as_str()),
        (Some("Conductor"), Some("Ops"))
    );
    assert_eq!(p["agent"], "claude-acp");
    assert!(p.get("hidden").is_none(), "{p}");
    assert_eq!(
        p["env"],
        json!({"DROVER_LANE": "sendmeter:-:orchestrator", "DROVER_ORCHESTRATOR": "default", "DROVER_THREAD": "{thread}"})
    );
    assert_eq!(ops["env"]["DROVER_ORCHESTRATOR"], "ops");
    let brief = text(p);
    assert!(brief.contains(OWN) && brief.contains("# ROLE: ORCHESTRATOR"), "{brief}");
    assert!(brief.contains("Fleet for asks: sendmeter"), "{brief}");
    let own_part = brief.split("# drover — usage for agents").next().unwrap();
    for verb in [
        "thread reply",
        "thread handoff",
        "--typed",
        "card post",
        "config propose",
        "secret request",
    ] {
        assert!(!own_part.contains(verb), "{verb}: {own_part}");
    }

    std::fs::write(&own, "Changed: ship small.").unwrap();
    let brief = text(&profiles(&s)[0]);
    assert!(
        brief.contains("Changed: ship small.") && !brief.contains(OWN),
        "{brief}"
    );

    assert!(hook_of(p).ends_with("hook --role orchestrator"), "{}", hook_of(p));
    let merge = ["gh", "pr", "merge", "5"].join(" ");
    let out: Value = serde_json::from_str(&chat_call(&c, p, "th1", true, &merge)).unwrap();
    let d = &out["hookSpecificOutput"];
    assert_eq!(d["permissionDecision"], "deny", "{out}");
    assert!(
        d["permissionDecisionReason"].as_str().unwrap().contains("drover merge"),
        "{out}"
    );
    assert_eq!(
        chat_call(&c, p, "th1", false, &merge),
        "",
        "the profile's env arms the hook"
    );
    assert_eq!(chat_call(&c, p, "th1", true, "ls"), "");
}

#[test]
fn a_started_chat_is_linked_once_and_takes_no_lane_asks() {
    for k in ["DROVER_OUTBOX", "DROVER_HOST", "DROVER_LANE"] {
        std::env::remove_var(k);
    }
    let (c, _) = setup("cooee-started");
    let r = Fake::default();
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let post = |b: Value| call(&s, true, ("POST", "/cooee/thread-started"), &b.to_string());

    let (code, v) = call(&s, false, ("POST", "/cooee/thread-started"), "{}");
    assert_eq!(code, 401, "{v}");
    let (code, v) = post(json!({"thread": "c-1", "profile": "nobody"}));
    assert_eq!(code, 409, "{v}");
    let (code, v) = post(json!({"thread": "bad id!", "profile": "default"}));
    assert!(code == 409 && v["line"].as_str().unwrap().contains("BAD_VALUE"), "{v}");

    for _ in 0..2 {
        let (code, v) = post(json!({"thread": "c-1", "profile": "ops"}));
        assert_eq!(
            (code, v["line"].as_str()),
            (200, Some("c-1 is ops's chat on sendmeter")),
            "{v}"
        );
    }
    let links: Vec<_> = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "orchestrator-attached")
        .cloned()
        .collect();
    assert_eq!(links.len(), 1);
    assert_eq!(
        json!(links[0].data),
        json!({"thread": "c-1", "orchestrator": "ops", "via": "profile"})
    );

    let ask = [
        "ask",
        "sendmeter",
        "12",
        "Which scope?",
        "--option",
        "small",
        "--option",
        "full",
    ];
    let (code, out) = run(&c, &r, &[&ask[..], &["--thread", "c-1"]].concat());
    assert_eq!(code, 0, "{out}");
    std::env::set_var("DROVER_LANE", "sendmeter:12:impl");
    let (code, _) = run(&c, &r, &ask);
    std::env::remove_var("DROVER_LANE");
    assert_eq!(code, 0);
    let q = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "needs-decision")
        .unwrap();
    assert_ne!(q.str("routed"), Some("c-1"), "{:?}", q.data);

    let ro = Server::new(&c, &r, None);
    let (code, _) = call(&ro, true, ("POST", "/cooee/thread-started"), "{}");
    assert_eq!(code, 405);
}

#[test]
fn an_orchestrator_id_cooee_cannot_name_is_refused_at_load() {
    for id in ["Ops", "default", "a_b"] {
        let e = drover::config::Config::parse(&format!("{CONFIG}\n[orchestrator.{id}]\nfleets = [\"sendmeter\"]\n"));
        let e = e.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("an id is lowercase letters"), "{id}: {e}");
    }
    assert!(
        drover::config::Config::parse(&format!("{CONFIG}\n[orchestrator.ops-2]\nfleets = [\"sendmeter\"]\n")).is_ok()
    );
}
