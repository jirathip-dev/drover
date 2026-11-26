mod common;
use common::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const NOW: u64 = 1_000_000;
const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";

fn setup(name: &str) -> (drover::Ctx, FakeShed) {
    let base = CONFIG.replace("harness  =  \"claude\"", "harness = \"claude\"");
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
    let sh = FakeShed::new();
    sh.attach(&c);
    remote::proxied(&c);
    (c, sh)
}

fn remote_fake() -> Fake {
    Fake::default()
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply(&format!("{SSH}'cat' '/proc/loadavg'"), "0.4 1 1 1/9 42")
        .reply(&format!("{SSH}'nproc'"), "4")
        .reply(
            &format!("{SSH}'/opt/drover/bin/drover' 'profiles' 'sync' '--profile'"),
            "",
        )
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply(&format!("{SSH}'git' '--no-replace-objects' "), "")
        .reply(&format!("{SSH}'sh' '-c' 'git clone"), "")
        .reply(&format!("{SSH}'sh' '-c' 'mkdir"), "")
        .reply(&format!("{SSH}'sh' '-c' 'umask 077"), "")
        .reply(&format!("{SSH}'rm' '-f'"), "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

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
    let export = remote::start_line(r, "bazzite").unwrap();
    let mut env = BTreeMap::new();
    for (i, (k, v)) in export.split_whitespace().filter_map(|w| w.split_once('=')).enumerate() {
        let v = v.trim_end_matches('\r').trim_matches('\'');
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

fn hook_there(env: &BTreeMap<String, String>, remote: &Path, toml: Option<&str>, cmd: &str) -> String {
    let conf = remote.join("fleet.toml");
    std::fs::remove_file(&conf).ok();
    if let Some(t) = toml {
        std::fs::write(&conf, t).unwrap();
    }
    let mut c = Command::new(env!("CARGO_BIN_EXE_drover"));
    c.env_remove("DROVER_SCRATCH_TARGET").env_remove("FLEET_LANE");
    for k in ["DROVER_LANE", "DROVER_HOST", "DROVER_RULES"] {
        c.env_remove(k);
    }
    for (k, v) in env.iter().filter(|(k, _)| *k != "DROVER_OUTBOX") {
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
    let (c, _sh) = setup("remote-rules");
    let (r, bazzite) = (remote_fake(), FakeShed::new());
    let o = remote::Acp::over(OverSsh {
        f: &r,
        hosts: vec![("bazzite", &bazzite)],
    });
    let (code, out) = run(&c, &o, &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let remote = tmpdir("remote-rules-bazzite");
    let env = lane_env(&r, &remote);
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

fn hook_with_rules(remote: &Path, rules: &Path) -> String {
    let env = BTreeMap::from([
        ("DROVER_LANE".to_string(), "sendmeter:12:impl".to_string()),
        ("DROVER_HOST".to_string(), "bazzite".to_string()),
        ("DROVER_RULES".to_string(), rules.display().to_string()),
    ]);
    hook_there(&env, remote, Some(CONFIG), "git status")
}

#[test]
fn a_missing_or_unreadable_rules_file_fails_closed_naming_it() {
    let remote = tmpdir("remote-rules-closed");
    let rules = remote.join("issue-12.rules.json");
    for text in [None, Some("not json"), Some("{\"branches\": 3}")] {
        std::fs::remove_file(&rules).ok();
        if let Some(t) = text {
            std::fs::write(&rules, t).unwrap();
        }
        let out = hook_with_rules(&remote, &rules);
        let why = format!(
            "REFUSED HOOK_FAILED_CLOSED: rules file {} is unreadable (",
            rules.display()
        );
        assert!(out.contains("\"deny\"") && out.contains(&why), "{text:?}: {out}");
    }
}

#[test]
fn the_rules_file_holds_no_secrets_and_the_lane_may_not_edit_it() {
    let (c, _sh) = setup("remote-rules-secrets");
    let extra = "\n[fleet.sendmeter.env]\nPLAIN = \"plain-env-value\"\nAPI = \"secret:API_KEY\"\n\
                 \n[[fleet.sendmeter.notify]]\ncmd = \"curl https://api.telegram.org/botSEKRIT/send\"\n";
    let text = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, text + extra).unwrap();
    secret_file(&c, "API_KEY", "tok-VALUE-123", 0o600);
    let (r, bazzite) = (remote_fake(), FakeShed::new());
    let o = remote::Acp::over(OverSsh {
        f: &r,
        hosts: vec![("bazzite", &bazzite)],
    });
    let (code, out) = run(&c, &o, &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let wt = "/home/me/.local/share/drover/work/sendmeter/issue-12";
    let inputs = r.inputs.borrow();
    let rules = inputs
        .iter()
        .find(|i| i.0.ends_with(&format!("'{wt}.rules.json'")))
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&rules.1).unwrap();
    let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["branches", "builds", "deny", "groups", "hygiene", "reviewer"]);
    for secret in [
        "SEKRIT",
        "telegram",
        "plain-env-value",
        "API_KEY",
        "tok-VALUE-123",
        "secret:",
    ] {
        assert!(!rules.1.contains(secret), "{secret}: {}", rules.1);
    }
    let remote = tmpdir("remote-rules-edit");
    let file = tmpdir("remote-rules-wt").join("issue-12.rules.json");
    std::fs::write(&file, &rules.1).unwrap();
    let env = BTreeMap::from([
        ("DROVER_LANE".to_string(), "sendmeter:12:impl".to_string()),
        ("DROVER_RULES".to_string(), file.display().to_string()),
    ]);
    let edit = hook_there(&env, &remote, None, &format!("echo '{{}}' > {}", file.display()));
    assert!(
        edit.contains(&format!("writing under {} is blocked", file.display())),
        "{edit}"
    );
}

#[test]
fn a_config_change_marks_a_live_remote_lane_rules_stale_until_respawned() {
    let (c, _sh) = setup("remote-rules-stale");
    let (f, bazzite) = (remote_fake(), FakeShed::new());
    let r = remote::Acp::over(OverSsh {
        f: &f,
        hosts: vec![("bazzite", &bazzite)],
    });
    assert_eq!(
        run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]).0,
        0
    );
    let (_, board) = run(&c, &r, &["board", "sendmeter"]);
    assert!(!board.contains("rules stale"), "{board}");
    let text = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, text.replace("stall_minutes = 30", "stall_minutes = 40")).unwrap();
    let (_, board) = run(&c, &r, &["board", "sendmeter"]);
    assert!(!board.contains("rules stale"), "{board}");
    let text = std::fs::read_to_string(&c.config).unwrap();
    let deny = "deny_commands_add = [\"terraform apply\"]\n\n[runtime]";
    std::fs::write(&c.config, text.replacen("\n[runtime]", &format!("\n{deny}"), 1)).unwrap();
    let (_, board) = run(&c, &r, &["board", "sendmeter"]);
    assert!(board.contains("rules stale"), "{board}");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--supersede"]);
    assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
    let start = format!("{SSH}'shed' 'run' '--acp'");
    assert_eq!(f.count(&start), 2, "the lane restarted on bazzite's shed");
    let (_, board) = run(&c, &r, &["board", "sendmeter"]);
    assert!(!board.contains("rules stale"), "{board}");
}

#[test]
fn spawn_syncs_the_lanes_profile_on_a_host_with_no_fleet_toml() {
    let (c, _sh) = setup("remote-rules-profile");
    let (r, bazzite) = (remote_fake(), FakeShed::new());
    let o = remote::Acp::over(OverSsh {
        f: &r,
        hosts: vec![("bazzite", &bazzite)],
    });
    assert_eq!(
        run(&c, &o, &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]).0,
        0
    );
    let sync = format!("{SSH}'/opt/drover/bin/drover' 'profiles' 'sync' '--profile' 'fleet-impl' '--role' 'impl'");
    let (at, start) = (r.index(&sync), r.index(&format!("{SSH}'shed' 'run' '--acp'")));
    assert!(at.is_some() && at < start, "{:?}", r.calls.borrow());
    let there = ctx(&tmpdir("remote-rules-profile-there"), "");
    std::fs::remove_file(&there.config).unwrap();
    let (code, out) = run(
        &there,
        &Fake::default(),
        &["profiles", "sync", "--profile", "fleet-rev", "--role", "rev"],
    );
    assert_eq!(code, 0, "{out}");
    let settings = std::fs::read_to_string(there.config.with_file_name("claude/fleet-rev/settings.json")).unwrap();
    assert!(settings.contains("hook --role rev"), "{settings}");
    assert_ne!(
        run(&there, &Fake::default(), &["profiles", "sync"]).0,
        0,
        "a full sync still needs fleet.toml"
    );
}

#[test]
fn a_remote_cap_lane_may_write_the_cap_file_and_no_other_lane_may() {
    for (name, title, allowed) in [
        ("remote-rules-cap", Some("cap: raise the src/ line cap to 53,500"), true),
        ("remote-rules-nocap", Some("Fix the meter"), false),
        ("remote-rules-untitled", None, false),
    ] {
        let (c, _sh) = setup(name);
        let f = match title {
            Some(t) => remote_fake().once(
                "gh issue view 12",
                0,
                &json!({"title": t, "body": "AC: it works"}).to_string(),
            ),
            None => remote_fake(),
        };
        let bazzite = FakeShed::new();
        let r = remote::Acp::over(OverSsh {
            f: &f,
            hosts: vec![("bazzite", &bazzite)],
        });
        let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]);
        assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
        let remote = tmpdir(&format!("{name}-bazzite"));
        let env = lane_env(&f, &remote);
        let out = hook_there(&env, &remote, None, "printf 'line_cap = 53500' > ci/cap.d/x.toml");
        assert!(!remote.join("state/cache").exists());
        assert_eq!(out.contains("REFUSED CAP_FILE"), !allowed, "{title:?}: {out}");
        if allowed {
            assert_eq!(out, "", "{title:?}");
        }
        let (_, board) = run(&c, &r, &["board", "sendmeter"]);
        assert!(!board.contains("rules stale"), "{board}");
    }
}

struct Disk<'a> {
    o: remote::Acp<'a, OverSsh<'a>>,
    root: std::path::PathBuf,
    rm_fails: std::sync::atomic::AtomicUsize,
}

impl Disk<'_> {
    fn at(&self, p: &str) -> std::path::PathBuf {
        self.root.join(p.trim_start_matches('/'))
    }
    fn file(&self, args: &[String], input: &str) -> Option<anyhow::Result<drover::runner::Out>> {
        use std::sync::atomic::Ordering::SeqCst;
        let last = args.last()?;
        if let Some(p) = last.strip_prefix("'rm' '-f' '").and_then(|p| p.strip_suffix('\'')) {
            self.o.r.f.calls.borrow_mut().push(format!("ssh {}", args.join(" ")));
            if self.rm_fails.load(SeqCst) > 0 {
                self.rm_fails.fetch_sub(1, SeqCst);
                return Some(Ok(drover::runner::Out {
                    code: 255,
                    stderr: "Connection closed by remote host".into(),
                    ..Default::default()
                }));
            }
            std::fs::remove_file(self.at(p)).ok();
            return Some(Ok(drover::runner::Out::default()));
        }
        if last.starts_with("'sh' '-c' 'umask 077") {
            let p = last.rsplit_once("' '")?.1.strip_suffix('\'')?;
            let f = self.at(p);
            std::fs::create_dir_all(f.parent()?).unwrap();
            std::fs::write(&f, input).unwrap();
        }
        None
    }
}

impl drover::runner::Runner for Disk<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        let mine = (prog == "ssh").then(|| self.file(args, "")).flatten();
        mine.unwrap_or_else(|| self.o.run(prog, args))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<drover::runner::Out> {
        let mine = (prog == "ssh").then(|| self.file(args, input)).flatten();
        mine.unwrap_or_else(|| self.o.run_in(prog, args, input))
    }
    fn sleep(&self, secs: u64) {
        self.o.sleep(secs);
    }
}

fn secret_spawn(name: &str, rm_fails: usize) -> (drover::Ctx, Fake, FakeShed, std::path::PathBuf) {
    let (c, _sh) = setup(name);
    let extra = "\n[fleet.sendmeter.env]\nAPI = \"secret:API_KEY\"\n";
    let text = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, text + extra).unwrap();
    secret_file(&c, "API_KEY", "tok-VALUE-123", 0o600);
    let (r, bazzite, root) = (remote_fake(), FakeShed::new(), tmpdir(&format!("{name}-disk")));
    {
        let d = Disk {
            o: remote::Acp::over(OverSsh {
                f: &r,
                hosts: vec![("bazzite", &bazzite)],
            }),
            root: root.clone(),
            rm_fails: rm_fails.into(),
        };
        let (code, out) = run(&c, &d, &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]);
        assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    }
    (c, r, bazzite, root)
}

const ENV_FILE: &str = "/home/me/.local/share/drover/work/sendmeter/issue-12.env";

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<drover::events::Event> {
    let log = drover::events::read(&c.state).unwrap();
    log.iter().filter(|e| e.kind == kind).cloned().collect()
}

#[test]
fn after_a_remote_spawn_its_host_has_no_secrets_file() {
    let (_c, r, _b, root) = secret_spawn("remote-secrets-gone", 0);
    assert!(
        r.called(&format!("{SSH}'sh' '-c' 'umask 077")),
        "{:?}",
        r.calls.borrow()
    );
    let f = root.join(ENV_FILE.trim_start_matches('/'));
    assert!(f.parent().unwrap().exists() && !f.exists(), "{}", f.display());
}

#[test]
fn a_remote_spawn_whose_remove_fails_leaves_no_file_once_the_retry_runs() {
    let (c, r, bazzite, root) = secret_spawn("remote-secrets-retry", 1);
    let f = root.join(ENV_FILE.trim_start_matches('/'));
    assert!(f.exists(), "the failed remove left it");
    let left = kinds(&c, "secrets-left");
    assert_eq!(left.len(), 1, "logged once");
    assert_eq!(left[0].str("path"), Some(ENV_FILE));
    assert_eq!(left[0].str("machine"), Some("bazzite"));
    let log = drover::events::read(&c.state).unwrap();
    assert!(log
        .iter()
        .all(|e| !serde_json::to_string(e).unwrap().contains("tok-VALUE-123")));
    let d = Disk {
        o: remote::Acp::over(OverSsh {
            f: &r,
            hosts: vec![("bazzite", &bazzite)],
        }),
        root: root.clone(),
        rm_fails: 0.into(),
    };
    run(&c, &d, &["drive"]);
    assert!(!f.exists(), "the next pass removed it");
    assert_eq!(kinds(&c, "secrets-removed").len(), 1);
    let rm = format!("{SSH}'rm' '-f' '{ENV_FILE}'");
    let tries = r.count(&rm);
    run(&c, &d, &["drive"]);
    assert_eq!(r.count(&rm), tries, "a removed file is not retried");
}

#[test]
fn the_lanes_shell_removes_its_secrets_file_once_it_has_sourced_it() {
    let (_c, r, _bazzite, root) = secret_spawn("remote-secrets-shell", usize::MAX);
    let f = root.join(ENV_FILE.trim_start_matches('/'));
    assert!(f.exists(), "spawn could not remove it");
    let export = remote::start_line(&r, "bazzite").unwrap();
    let line = export.trim_end().replace(ENV_FILE, &f.display().to_string());
    let o = Command::new("sh")
        .args(["-c", &format!("{line} && printf %s \"$API\"")])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout), "tok-VALUE-123", "{line}");
    assert!(!f.exists(), "{line}");
}

#[test]
fn clean_lists_leftover_secrets_files_and_removes_them_with_yes() {
    let (c, _sh) = setup("remote-secrets-clean");
    let ours = c.worktrees.join("sendmeter/issue-9");
    std::fs::create_dir_all(&ours).unwrap();
    let beside = c.worktrees.join("sendmeter/issue-9.env");
    let alone = c.worktrees.join("oldproj/issue-3.env");
    std::fs::create_dir_all(alone.parent().unwrap()).unwrap();
    for p in [&beside, &alone] {
        std::fs::write(p, "export X='v'\n").unwrap();
    }
    let gone = "/home/me/.local/share/drover/work/sendmeter/issue-4.env";
    let r = remote_fake().reply(&format!("{SSH}'sh' '-c' 'cd && cd"), &format!("env\t{gone}\n"));
    let bazzite = FakeShed::new();
    let o = remote::Acp::over(OverSsh {
        f: &r,
        hosts: vec![("bazzite", &bazzite)],
    });
    let (code, dry) = run(&c, &o, &["clean"]);
    assert_eq!(code, 0, "{dry}");
    for p in [
        beside.display().to_string(),
        alone.display().to_string(),
        format!("bazzite:{gone}"),
    ] {
        assert!(dry.contains(&format!("secrets file {p}\n")), "{p}: {dry}");
    }
    let rm = format!("{SSH}'rm' '-f' '{gone}'");
    assert!(
        beside.exists() && alone.exists() && !r.called(&rm),
        "dry run removes nothing"
    );
    let (code, out) = run(&c, &o, &["clean", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(!beside.exists() && !alone.exists() && ours.exists(), "{out}");
    assert!(r.called(&rm), "{:?}", r.calls.borrow());
    assert!(out.contains(&format!("removed secrets file bazzite:{gone}")), "{out}");
}
