use super::*;

pub(super) fn acp_fake(adapter: &[&str]) -> Fake {
    acp_fake_listing(&json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": adapter}]))
}

fn acp_fake_listing(agents: &Value) -> Fake {
    spawn_fake()
        .reply("shed help run", "--meta <JSON>")
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
        .reply("shed cancel", "")
}

#[test]
fn shed_129s_listing_resolves_the_adapter_under_installed_and_a_bare_record_is_not_installed() {
    let record = |installed: Value| {
        json!([{"id": "claude-acp", "source": "built-in", "provider": {}, "installed": installed},
               {"id": "codex-acp", "source": "built-in", "provider": {}, "installed": null}])
    };
    let adapter = json!({"id": "claude-acp", "version": "0.107.0", "distribution": "binary",
        "cmd": ["echo", "claude-agent-acp"]});
    for (name, agents) in [
        ("acp-shape-new", record(adapter)),
        (
            "acp-shape-old",
            json!([{"id": "claude-acp", "version": "0.107.0", "distribution": "binary", "cmd": ["echo", "claude-agent-acp"]}]),
        ),
    ] {
        let b = FakeShed::new();
        let c = acp_ctx(name, &acp_config(&b.sock, "claude"));
        let f = acp_fake_listing(&agents);
        let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
        assert_eq!(code, 0, "{name}: {out}");
        let calls = f.calls.borrow();
        let start = calls.iter().find(|c| c.starts_with("shed run")).unwrap();
        assert!(start.ends_with(" sh echo claude-agent-acp"), "{name}: {start}");
    }

    let b = FakeShed::new();
    let c = acp_ctx("acp-shape-bare", &acp_config(&b.sock, "claude"));
    let f = acp_fake_listing(&record(Value::Null));
    let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 1, "{out}");
    assert!(!f.called("shed run"), "{:?}", f.calls.borrow());

    let none = Fake::default().reply("shed agents --json", &record(Value::Null).to_string());
    let (_, out) = run(&c, &none, &["doctor"]);
    assert!(
        out.contains("✗ shed agent claude-acp — not installed → shed agents add claude-acp"),
        "{out}"
    );
    let adapter = json!({"id": "claude-acp", "cmd": ["no-such-adapter-600"]});
    let there = Fake::default().reply("shed agents --json", &record(adapter).to_string());
    let (_, out) = run(&c, &there, &["doctor"]);
    assert!(
        out.contains("✗ shed agent claude-acp — no-such-adapter-600 not on the timer's PATH → "),
        "{out}"
    );
}
