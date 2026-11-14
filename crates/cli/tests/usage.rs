mod common;
use common::*;
use drover::events::read;
use drover::runner::{Out, Runner};
use drover::serve::Server;
use drover::usage::{epoch, parse_balance};
use std::path::Path;

const DS_KEY: &str = concat!("sk", "-deepseekfixturekey0123456789");

struct Stdin(Fake, Shared<Vec<String>>);

impl Runner for Stdin {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.1.borrow_mut().push(input.into());
        self.0.run(prog, args)
    }
}

fn stdin(f: Fake) -> Stdin {
    Stdin(f, Shared::default())
}

fn http(status: u32, head: &str, body: &str) -> String {
    format!("HTTP/2 {status}\r\ncontent-type: application/json\r\n{head}\r\n{body}")
}

#[test]
fn balance_and_usage_fixtures_parse() {
    let b = parse_balance(r#"{"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"2.28","granted_balance":"0.00","topped_up_balance":"2.28"}]}"#, 7).unwrap();
    assert_eq!((b.currency.as_str(), b.total, b.ts), ("USD", 2.28, 7));
    assert!(parse_balance("{}", 0).is_none());
    assert_eq!(epoch("2026-10-31T12:00:00.123+07:00"), Some(1_793_422_800));
    assert_eq!(epoch("2026-10-31T05:00:00Z"), Some(1_793_422_800));
}

#[test]
fn secret_in_config_is_refused() {
    const JWT: &str = concat!(
        "ey",
        "JhbGciOiJIUzI1NiJ9.eyJzdWIiOiJmaXh0dXJlIn0.c2lnbmF0dXJlZml4dHVyZQ"
    );
    for (i, secret) in [DS_KEY, JWT].into_iter().enumerate() {
        let c = ctx(
            &tmpdir(&format!("usage-secret-{i}")),
            &format!("{CONFIG}\n[providers.deepseek]\nkey_env = \"{secret}\"\n"),
        );
        let (code, out) = run(&c, &Fake::default(), &["config", "show"]);
        assert_eq!(code, 3, "{out}");
        assert!(
            out.contains("SECRET_IN_CONFIG") && out.contains("providers.deepseek.key_env") && !out.contains(secret),
            "{out}"
        );
    }
    let legit = format!(
        "{CONFIG}\nmodels = [\"deepseek-ai/DeepSeek-V3.2-Exp-instruct-20261031-preview\"]\n[providers.deepseek]\nkey_env = \"DEEPSEEK_API_KEY\"\nenv_file = \"/Users/someone/.config/claude-accounts/a1b2c3d4e5f6/tokens-for-personal-account.env\"\n"
    );
    let ok = ctx(&tmpdir("usage-nosecret"), &legit);
    let (code, out) = run(&ok, &Fake::default(), &["config", "show"]);
    assert_eq!(code, 0, "{out}");
}

fn config(d: &Path) -> String {
    let env = d.join("tokens.env");
    std::fs::write(&env, format!("export DEEPSEEK_API_KEY=\"{DS_KEY}\"\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&env, std::fs::Permissions::from_mode(0o600)).unwrap();
    CONFIG.to_string()
        + &format!(
            "\n[providers.deepseek]\nkey_env = \"DEEPSEEK_API_KEY\"\nenv_file = \"{}\"\n",
            env.display()
        )
}

fn no_secret(what: &str, text: &str) {
    assert!(!text.contains(DS_KEY), "{what} leaks a secret: {text}");
}

fn assert_no_leaks(c: &drover::Ctx, r: &Stdin, outs: &[&str], page: &str) {
    let calls = r.0.calls.borrow().join("\n");
    no_secret("argv", &calls);
    assert!(!calls.contains("security "), "{calls}");
    for o in outs {
        no_secret("CLI output", o);
    }
    no_secret("event log", &drover::events::jsonl(&c.state).unwrap_or_default());
    for f in std::fs::read_dir(c.state.parent().unwrap().join("usage"))
        .into_iter()
        .flatten()
    {
        no_secret("cache", &std::fs::read_to_string(f.unwrap().path()).unwrap_or_default());
    }
    no_secret("page", page);
}

#[test]
fn balance_shows_without_leaking_secrets() {
    let d = tmpdir("usage-show");
    let c = ctx(&d, &config(&d));
    let r = stdin(Fake::default().reply(
        "curl -sS -m 15 -D - -K - https://api.deepseek.com/user/balance",
        &http(
            200,
            "",
            r#"{"balance_infos":[{"currency":"USD","total_balance":"2.28"}]}"#,
        ),
    ));
    let page = host_tab(&Server::new(&c, &r, None), "", c.now);
    assert!(page.contains("<span class=\"pvs a\">✓ USD 2.28 · low</span>"), "{page}");
    assert!(!page.contains("lane-hours"), "#923: no account meter: {page}");
    let sent = r.1.borrow().join("\n");
    assert!(sent.contains(&format!("Bearer {DS_KEY}")), "keys go on stdin: {sent}");

    let n = r.0.count("curl");
    let (_, out) = run(&c, &r, &["check"]);
    assert_eq!(r.0.count("curl"), n);
    assert!(
        out.contains("deepseek provider-low: balance USD 2.28 (warn below 5)"),
        "{out}"
    );
    assert_eq!(run(&c, &r, &["check"]).1, "");
    assert_eq!(
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "provider-low")
            .count(),
        1
    );
    assert_no_leaks(&c, &r, &[&out], &page);
}

#[test]
fn deepseek_hints_when_the_env_file_lacks_the_key() {
    let d = tmpdir("usage-nokey");
    let missing = d.join("nope.env");
    let c = ctx(
        &d,
        &format!(
            "{CONFIG}\n[providers.deepseek]\nkey_env = \"DEEPSEEK_API_KEY\"\nenv_file = \"{}\"\n",
            missing.display()
        ),
    );
    let page = host_tab(&Server::new(&c, &Fake::default(), None), "", c.now);
    assert!(
        page.contains(&format!(
            "<article class=pvc data-p=deepseek><div class=pvch><b>DeepSeek</b><span class=m>API keys</span></div><div class=pvr data-t=a><p class=pvh><b class=pvn>balance</b><span class=\"pvs a\">key not found in {}</span>",
            missing.display()
        )),
        "{page}"
    );
}

#[test]
fn a_deepseek_env_file_others_can_read_is_refused() {
    let d = tmpdir("usage-0644");
    let c = ctx(&d, &config(&d));
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(d.join("tokens.env"), std::fs::Permissions::from_mode(0o644)).unwrap();
    let r = stdin(Fake::default());
    let page = host_tab(&Server::new(&c, &r, None), "", c.now);
    assert!(page.contains("REFUSED SECRET_FILE_OPEN: "), "{page}");
    assert!(!r.0.called("curl"), "{:?}", r.0.calls.borrow());
    no_secret("page", &page);
}
