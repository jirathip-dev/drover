mod common;
use common::*;
use serde_json::Value;
use std::path::PathBuf;

fn spawned(sandbox: &str) -> (drover::Ctx, FakeShed, PathBuf, String) {
    let d = tmpdir("lane-sandbox");
    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let cfg = CONFIG
        .replace("harness  =  \"hermes\"    # toggled in tests", "harness = \"claude\"")
        .replace("/tmp/does-not-matter/sendmeter", repo.to_str().unwrap())
        .replace(
            "integration = \"staging\"   # never main",
            &format!("integration = \"staging\"\n{sandbox}"),
        );
    let c = ctx(&d, &cfg);
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out}");
    let start = sh
        .sent("sendmeter-impl-12")
        .into_iter()
        .find(|t| t.starts_with("exec "))
        .expect("no start line");
    (c, sh, wt, start)
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_sandboxed_fleet_starts_its_agent_under_srt_with_rules_walling_off_home_secrets_and_the_network() {
    let (c, _sh, wt, start) = spawned("sandbox = \"srt\"");
    let file = format!("{}.srt.json", wt.display());
    assert!(
        start.contains(&format!("'srt' '--settings' '{file}' 'claude'")),
        "{start}"
    );
    let rules: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let fs = &rules["filesystem"];
    let home = drover::config::home().display().to_string();
    let secrets = c.config.with_file_name("secrets").display().to_string();
    let deny = strings(&fs["denyRead"]);
    assert!(
        deny.contains(&home) && deny.contains(&secrets) && deny.contains(&format!("{home}/.ssh")),
        "{deny:?}"
    );
    let write = strings(&fs["allowWrite"]);
    assert!(write.iter().any(|w| PathBuf::from(w) == wt), "{write:?}");
    assert!(
        !write.contains(&home) && !strings(&fs["allowRead"]).contains(&home),
        "{fs}"
    );
    let tmp = start
        .split("CLAUDE_CODE_TMPDIR=")
        .nth(1)
        .unwrap()
        .split('\'')
        .next()
        .unwrap();
    assert!(
        PathBuf::from(tmp).is_dir() && write.iter().any(|w| w == tmp),
        "{tmp} {write:?}"
    );
    let hosts = strings(&rules["network"]["allowedDomains"]);
    assert!(
        hosts.contains(&"github.com".into()) && hosts.contains(&"*.anthropic.com".into()),
        "{hosts:?}"
    );
    assert!(!hosts.iter().any(|h| h == "*"), "{hosts:?}");
}

#[test]
fn an_unsandboxed_fleet_starts_its_agent_as_before() {
    let (_c, _sh, wt, start) = spawned("");
    assert!(!start.contains("srt"), "{start}");
    assert!(!PathBuf::from(format!("{}.srt.json", wt.display())).exists());
}

#[test]
fn an_unknown_sandbox_is_a_config_error() {
    let d = tmpdir("lane-sandbox-bad");
    let cfg = CONFIG.replace(
        "integration = \"staging\"   # never main",
        "integration = \"staging\"\nsandbox = \"seatbelt\"",
    );
    let c = ctx(&d, &cfg);
    let (code, out) = run(&c, &Fake::default(), &["spawn", "sendmeter", "12", "impl", "--dry-run"]);
    assert_ne!(code, 0, "{out}");
}
