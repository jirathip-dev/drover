mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use drover::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const LANE: &str = "sendmeter-impl-12";

fn lane(name: &str) -> (drover::Ctx, String, FakeShed) {
    let d = tmpdir(name);
    let c = ctx(&d, CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let wt = d.join("wt").to_string_lossy().into_owned();
    std::fs::create_dir_all(&wt).unwrap();
    let sp =
        json!({"role": "impl", "agent": LANE, "pane": LANE, "worktree": wt, "branch": "issue-12", "runtime": "shed"});
    append(&c.state, &Event::new(T - 60, "sendmeter", Some(12), "spawn", sp)).unwrap();
    (c, wt, sh)
}

fn closed(sh: &FakeShed) -> bool {
    sh.called("kill", LANE) && sh.called("rm", LANE)
}

struct AtPush<'a>(Fake, &'a FakeShed, Shared<Vec<bool>>);

impl drover::runner::Runner for AtPush<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        if prog == "git" && args.iter().any(|a| a == "push") {
            self.2.borrow_mut().push(self.1.called("kill", LANE));
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

fn git(wt: &str, status: &str, ahead: &str, log: &str) -> Fake {
    Fake::default()
        .reply(&format!("git -C {wt} status --porcelain"), status)
        .reply(&format!("git -C {wt} rev-list --count"), ahead)
        .reply(&format!("git -C {wt} log -p"), log)
        .reply(&format!("git -C {wt} "), "")
        .reply("date -r", "20261031-120000\n")
}

fn kinds(c: &drover::Ctx, kind: &str) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == kind).count()
}

#[test]
fn kill_dry_run_lists_what_is_lost() {
    let (c, wt, sh) = lane("kill-dry");
    let r = git(&wt, "?? .brief.md\n M src/a.rs\n?? new.rs\n", "3\n", "");
    let (code, out) = run(&c, &r, &["kill", LANE]);
    assert_eq!(code, 0, "{out}");
    let want = format!("{LANE} · impl · 3 unpushed commit(s), 2 dirty file(s) → wip/{LANE}-20261031-120000-");
    assert!(
        out.starts_with(&want) && out.contains(" · pane closes, worktree kept\n"),
        "{out}"
    );
    assert!(out.contains("dry run"), "{out}");
    assert!(
        !sh.called("kill", LANE) && !r.called(&format!("git -C {wt} push")) && !r.called(&format!("git -C {wt} add"))
    );
    assert_eq!(kinds(&c, "lane-killed"), 0);
    let r = git(&wt, "?? .brief.md\n", "0\n", "");
    assert!(run(&c, &r, &["kill", LANE])
        .1
        .starts_with(&format!("{LANE} · impl · nothing unpushed · pane closes")));
}

#[test]
fn kill_pushes_wip_then_closes_and_logs() {
    let (c, wt, sh) = lane("kill-yes");
    sh.lane(LANE, "idle", &wt);
    let f = git(&wt, " M src/a.rs\n", "2\n", "+fn x() {}\n")
        .once(&format!("git -C {wt} diff --cached --name-only"), 0, "")
        .once(&format!("git -C {wt} diff --cached"), 0, "+fn x() {}\n");
    let r = AtPush(f, &sh, Shared::default());
    let log = drover::lanes::log(&c.state, LANE).unwrap();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, "⏺ hi\n").unwrap();
    let (code, out) = run(&c, &r, &["kill", LANE, "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(!log.exists());
    let (r, at_push) = (&r.0, &r.2);
    assert!(
        out.contains(&format!("killed {LANE}: work pushed to wip/{LANE}-20261031-120000-")),
        "{out}"
    );
    let at = |p: &str| {
        r.index(&format!("git -C {wt} {p}"))
            .unwrap_or_else(|| panic!("no git {p}"))
    };
    assert!(
        at("add -A") < at("reset -q -- .brief.md")
            && at("reset -q -- .brief.md") < at("commit -q --no-verify -m wip: kill snapshot")
    );
    assert!(at("commit") < at(&format!("push origin HEAD:refs/heads/wip/{LANE}-20261031-120000-")));
    assert!(closed(&sh) && !sh.has(LANE), "{:?}", sh.methods());
    let pushes: Vec<String> = r
        .calls
        .borrow()
        .iter()
        .filter(|l| l.contains(" push "))
        .cloned()
        .collect();
    let ref_ = format!("git -C {wt} push origin HEAD:refs/heads/wip/{LANE}-20261031-120000-");
    assert!(pushes.len() == 1 && pushes[0].starts_with(&ref_), "{pushes:?}");
    assert_eq!(*at_push.borrow(), [false], "pushed before the pane closed");
    let e = read(&c.state).unwrap().to_vec().pop().unwrap();
    assert_eq!((e.kind.as_str(), e.str("role")), ("lane-killed", Some("impl")));
    assert_eq!(e.str("wip"), pushes[0].rsplit("refs/heads/").next());
    let s = fold(&read(&c.state).unwrap())
        .remove(&("sendmeter".into(), 12))
        .unwrap();
    assert_eq!((s.state.as_str(), s.unfinished("impl", false)), ("queued", false));
    assert!(run(&c, r, &["kill", LANE]).1.starts_with("REFUSED NO_SUCH_AGENT"));
}

#[test]
fn kill_refuses_a_secret_in_the_wip_diff() {
    let (c, wt, sh) = lane("kill-secret");
    sh.lane(LANE, "idle", &wt);
    let r = git(
        &wt,
        "",
        "1\n",
        concat!("+TOKEN = \"gh", "p_abcdefghijklmnopqrstu1234\"\n"),
    );
    let (code, out) = run(&c, &r, &["kill", LANE, "--yes"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED SECRET_IN_WIP"), "{out}");
    assert!(!out.contains("ghp_abc"), "the secret is never printed");
    assert!(
        !r.called(&format!("git -C {wt} push"))
            && !sh.called("kill", LANE)
            && !r.called(&format!("git -C {wt} commit"))
    );
    assert!(r.called(&format!("git -C {wt} reset -q")), "unstaged again");
    assert_eq!(kinds(&c, "lane-killed"), 0);
}

#[test]
fn kill_skips_lockfile_checksums_but_not_secrets() {
    let head = "diff --git a/Cargo.lock b/Cargo.lock\n--- a/Cargo.lock\n+++ b/Cargo.lock\n@@ -1,3 +1,3 @@\n";
    let lock = format!(
        "{head}+checksum = \"{}\"\n+  \"integrity\": \"sha512-{}==\",\n-OLD_TOKEN={}\n",
        "ab".repeat(32),
        "Qx".repeat(43),
        concat!("gh", "p_abcdefghijklmnopqrstu1234")
    );
    let (c, wt, _sh) = lane("kill-lock");
    let r = git(&wt, "", "1\n", &lock);
    assert_eq!(run(&c, &r, &["kill", LANE, "--yes"]).0, 0);
    assert!(r.called(&format!("git -C {wt} push origin HEAD:refs/heads/wip/")));
    let jwt = concat!("ey", "JhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc");
    for (i, bad) in [
        concat!("+checksum = \"gh", "p_abcdefghijklmnopqrstu1234\"").to_string(),
        concat!("+checksum = \"sk", "-abcdefghijklmnopqrstuvwxyz123456\"").into(),
        format!("+  \"integrity\": \"{jwt}\","),
        concat!("+++ b/token.txt gh", "p_abcdefghijklmnopqrstu1234").into(),
        concat!(
            "diff --git a/.env b/.env\n+++ b/.env\n@@ -0,0 +1 @@\n+API_KEY=sk",
            "-abcdefghijklmnopqrstuvwxyz123456"
        )
        .into(),
    ]
    .iter()
    .enumerate()
    {
        let (c, wt, sh) = lane(&format!("kill-lock-secret-{i}"));
        sh.lane(LANE, "idle", &wt);
        let r = git(&wt, "", "1\n", &format!("{lock}{bad}\n"));
        let (code, out) = run(&c, &r, &["kill", LANE, "--yes"]);
        assert!(code == 3 && out.contains("REFUSED SECRET_IN_WIP"), "{bad}: {out}");
        assert!(
            !r.called(&format!("git -C {wt} push")) && !sh.called("kill", LANE),
            "{bad}"
        );
    }
}

struct RealGit(Fake, &'static str);

impl drover::runner::Runner for RealGit {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        if prog != "git" {
            return self.0.run(prog, args);
        }
        self.0.calls.borrow_mut().push(format!("git {}", args.join(" ")));
        if !self.1.is_empty() && args.join(" ").contains(self.1) {
            return Ok(drover::runner::Out {
                code: 1,
                stdout: String::new(),
                stderr: "forced".into(),
            });
        }
        drover::runner::Real.run(prog, args)
    }
}

fn repo(wt: &str, fail: &'static str) -> RealGit {
    let origin = format!("{wt}.git");
    let sh = |a: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .args(a)
                .output()
                .unwrap()
                .status
                .success(),
            "{a:?}"
        );
    };
    sh(&["init", "-q", "--bare", &origin]);
    sh(&["clone", "-q", &origin, wt]);
    for (k, v) in [("user.name", "t"), ("user.email", "t@t"), ("commit.gpgsign", "false")] {
        sh(&["-C", wt, "config", k, v]);
    }
    std::fs::write(format!("{wt}/.gitignore"), ".brief.md\n").unwrap();
    sh(&["-C", wt, "add", "-A"]);
    sh(&["-C", wt, "commit", "-qm", "init"]);
    sh(&["-C", wt, "push", "-q", "origin", "HEAD"]);
    for f in [".brief.md", ".addendum.md", ".mcp.lane.json", "verdict.md"] {
        std::fs::write(format!("{wt}/{f}"), "scratch").unwrap();
    }
    RealGit(Fake::default().reply("date -r", "20261031-120000\n"), fail)
}

fn status(wt: &str) -> String {
    let o = std::process::Command::new("git")
        .args(["-C", wt, "status", "--porcelain", "--ignored"])
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn kill_counts_lane_scratch_as_clean_and_never_pushes_it() {
    let (c, wt, sh) = lane("kill-scratch");
    sh.lane(LANE, "idle", &wt);
    let r = repo(&wt, "");
    let before = status(&wt);
    assert!(run(&c, &r, &["kill", LANE])
        .1
        .starts_with(&format!("{LANE} · impl · nothing unpushed · pane closes")));
    let (code, out) = run(&c, &r, &["kill", LANE, "--yes"]);
    assert_eq!(
        (code, out.trim()),
        (
            0,
            &*format!("{LANE} · impl · nothing unpushed · pane closes, worktree kept\nkilled {LANE}")
        )
    );
    assert!(closed(&sh) && !r.0.called(&format!("git -C {wt} push")) && !r.0.called(&format!("git -C {wt} add")));
    assert_eq!(status(&wt), before);
}

#[test]
fn kill_snapshots_only_the_real_dirty_file() {
    let (c, wt, sh) = lane("kill-real");
    sh.lane(LANE, "idle", &wt);
    let r = repo(&wt, "");
    std::fs::write(format!("{wt}/real.rs"), "fn x() {}\n").unwrap();
    assert!(run(&c, &r, &["kill", LANE])
        .1
        .contains("0 unpushed commit(s), 1 dirty file(s)"));
    let (code, out) = run(&c, &r, &["kill", LANE, "--yes"]);
    assert_eq!(code, 0, "{out}");
    let w = out.rsplit("work pushed to ").next().unwrap().trim();
    let o = std::process::Command::new("git")
        .args(["-C", &format!("{wt}.git"), "show", "--name-only", "--format=", w])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(o.stdout).unwrap().trim(), "real.rs");
    assert!(closed(&sh), "{:?}", sh.methods());
}

#[test]
fn kill_restores_the_index_when_the_snapshot_fails() {
    let (c, wt, sh) = lane("kill-snapfail");
    sh.lane(LANE, "idle", &wt);
    let r = repo(&wt, "diff --cached");
    std::fs::write(format!("{wt}/real.rs"), "fn x() {}\n").unwrap();
    let before = status(&wt);
    let (code, out) = run(&c, &r, &["kill", LANE, "--yes"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED SNAPSHOT_FAILED"), "{out}");
    assert!(
        r.0.called(&format!("git -C {wt} add -A"))
            && !r.0.called(&format!("git -C {wt} push"))
            && !sh.called("kill", LANE)
    );
    assert_eq!(status(&wt), before);
    assert_eq!(kinds(&c, "lane-killed"), 0);
}

#[test]
fn stop_sends_escape() {
    let (c, wt, sh) = lane("stop");
    sh.lane(LANE, "idle", &wt);
    let r = git(&wt, "", "0\n", "");
    let (code, out) = run(&c, &r, &["stop", LANE]);
    assert_eq!((code, out.as_str()), (0, "sent Esc to sendmeter-impl-12\n"));
    assert_eq!(sh.last("keys"), json!({"name": LANE, "keys": ["esc"]}));
    assert_eq!(kinds(&c, "stop"), 1);
    let (code, out) = run(&c, &r, &["stop", "nobody"]);
    assert!(code == 3 && out.starts_with("REFUSED NO_SUCH_AGENT"), "{out}");
}

#[test]
fn close_shuts_an_others_pane_and_refuses_lanes() {
    let (c, _, sh) = lane("close");
    sh.lane(LANE, "idle", "/x");
    sh.lane("dsh-tui-test", "idle", "/tmp");
    let r = Fake::default();
    let (code, out) = run(&c, &r, &["close", "dsh-tui-test"]);
    assert_eq!(
        (code, out.as_str()),
        (0, "closed dsh-tui-test's pane; its worktree and files stay\n")
    );
    assert!(sh.called("kill", "dsh-tui-test") && !sh.has("dsh-tui-test") && !r.called("git"));
    let e = read(&c.state).unwrap().to_vec().pop().unwrap();
    assert_eq!(
        (e.kind.as_str(), e.str("agent")),
        ("closed-other", Some("dsh-tui-test"))
    );
    let (code, out) = run(&c, &r, &["close", LANE]);
    assert!(code == 3 && out.starts_with("REFUSED IS_LANE"), "{out}");
    assert!(!sh.called("kill", LANE) && sh.has(LANE));
    assert_eq!(kinds(&c, "closed-other"), 1);
}

#[test]
fn clean_lane_refuses_unsafe_lanes() {
    let (c, wt, sh) = lane("clean-lane");
    let r = git(&wt, "", "0\n", "");
    let clean = |r: &Fake| run(&c, r, &["clean", "sendmeter", "--lane", LANE]);
    let (code, out) = clean(&r);
    assert!(
        code == 3 && out.contains("REFUSED UNSAFE_CLEAN: sendmeter-impl-12: only after merge"),
        "{out}"
    );
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(
        cache.join("pr-sendmeter-12.json"),
        json!({"ts": 1, "pr": {"number": 42, "state": "OPEN"}}).to_string(),
    )
    .unwrap();
    assert!(clean(&r)
        .1
        .contains("UNSAFE_CLEAN: sendmeter-impl-12: only after merge (PR #42 open)"));
    append(&c.state, &Event::new(T, "sendmeter", Some(12), "merged", json!({}))).unwrap();
    sh.lane(LANE, "idle", "/x");
    assert!(clean(&r).1.contains("UNSAFE_CLEAN: sendmeter-impl-12: live agent"));
    sh.remove(LANE);
    let dirty = git(&wt, " M a.rs\n", "0\n", "");
    assert!(clean(&dirty)
        .1
        .contains("UNSAFE_CLEAN: sendmeter-impl-12: 1 uncommitted change(s)"));
    let (code, out) = clean(&r);
    assert!(code == 0 && out.contains(&format!("would remove {wt}")), "{out}");
    assert!(run(&c, &r, &["clean", "sendmeter", "--lane", "nobody"])
        .1
        .contains("UNSAFE_CLEAN: nobody: no worktree"));
}

fn req(s: &Server, path: &str, body: &str, extra: &str) -> String {
    let r = format!(
        "POST {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, T)).into_owned()
}

#[test]
fn act_needs_a_paired_cookie_and_logs_via_web() {
    let (c, wt, sh) = lane("act");
    sh.lane(LANE, "idle", &wt);
    let f = git(&wt, "", "0\n", "");
    let r = sh.on(&f);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    assert!(req(&s, "/act", &format!("agent={LANE}&op=stop"), "").starts_with("HTTP/1.1 401 "));
    assert!(req(
        &s,
        "/act",
        &format!("agent={LANE}&op=stop"),
        "Origin: https://evil.example\r\n"
    )
    .starts_with("HTTP/1.1 403 "));
    assert!(sh.all("keys").is_empty(), "{:?}", sh.methods());
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let res = req(&s, "/act", &format!("agent={LANE}&op=stop"), &cookie);
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && res.ends_with("sent Esc to sendmeter-impl-12\n"),
        "{res}"
    );
    assert_eq!(sh.last("keys"), json!({"name": LANE, "keys": ["esc"]}));
    let e = read(&c.state).unwrap().to_vec().pop().unwrap();
    let label = drover::devices::load(&c)[0].label.clone();
    assert_eq!(
        (e.kind.as_str(), e.str("via"), e.str("device"), e.str("op")),
        ("act", Some("web"), Some(&*label), Some("stop"))
    );
    let res = req(&s, "/act", &format!("agent={LANE}&op=kill"), &cookie);
    assert!(
        res.contains("pane closes, worktree kept") && !sh.called("kill", LANE),
        "{res}"
    );
    assert_eq!(kinds(&c, "act"), 1);
}

fn screen() -> String {
    "> fix it\n⏺ Bash(cargo test)\n  ⎿ ok\n⏺ Done.\n  All green.\n⏺ Bash(cargo fmt)\n".into()
}

#[test]
fn lane_pages_back_through_the_rolling_transcript() {
    let (c, _, sh) = lane("more");
    let log = drover::lanes::log(&c.state, LANE).unwrap();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, (0..450).map(|i| format!("  out {i}\n")).collect::<String>()).unwrap();
    sh.lane(LANE, "working", "/x");
    sh.screen(LANE, &screen());
    let f = Fake::default();
    let r = sh.on(&f);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", T);
    let get = |q: &str| {
        let res = String::from_utf8_lossy(&s.respond(
            &format!("GET /lane?agent={LANE}{q} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n"),
            T,
        ))
        .into_owned();
        serde_json::from_str::<serde_json::Value>(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap()
    };
    let first = get("&lines=500");
    let lines = first["lines"].as_array().unwrap();
    assert_eq!(
        (lines.len(), &first["live"], &first["before"], &first["more"]),
        (200, &json!(1), &json!(200), &json!(true)),
        "the cap stays at 200"
    );
    assert_eq!(
        (&lines[193], &lines[194], &lines[199]),
        (
            &json!(drover::lanes::GAP),
            &json!("> fix it"),
            &json!("⏺ Bash(cargo fmt)")
        )
    );
    let older = get("&lines=200&before=200");
    assert_eq!(
        (&older["before"], &older["more"], &older["live"]),
        (&json!(400), &json!(true), &json!(0))
    );
    assert_eq!(older["lines"][0], "  out 57");
    let oldest = get("&before=400");
    assert_eq!((&oldest["before"], &oldest["more"]), (&json!(457), &json!(false)));
    assert_eq!(oldest["lines"][0], "  out 0");
    let file = std::fs::read_to_string(&log).unwrap();
    assert!(file.ends_with("  out 449\n┄ not captured: scrolled past between reads ┄\n> fix it\n⏺ Bash(cargo test)\n  ⎿ ok\n⏺ Done.\n  All green.\n"), "{file}");
    let recent = json!({"name": LANE, "lines": 200, "history": true});
    assert!(sh.all("read").contains(&recent), "{:?}", sh.all("read"));
}

#[test]
fn a_gone_lane_reads_its_stored_transcript() {
    let (c, _, sh) = lane("gone-transcript");
    let f = Fake::default();
    let r = sh.on(&f);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", T);
    let get = || {
        let res = String::from_utf8_lossy(&s.respond(
            &format!("GET /lane?agent={LANE} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n"),
            T,
        ))
        .into_owned();
        res
    };
    assert!(get().starts_with("HTTP/1.1 404"), "{}", get());
    let log = drover::lanes::log(&c.state, LANE).unwrap();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, (0..250).map(|i| format!("  out {i}\n")).collect::<String>()).unwrap();
    let res = get();
    let j: serde_json::Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    let lines = j["lines"].as_array().unwrap();
    assert_eq!(
        (lines.len(), &lines[0], &lines[199], &j["live"], &j["more"]),
        (200, &json!("  out 50"), &json!("  out 249"), &json!(0), &json!(true))
    );
}

#[test]
fn inputs_are_16px_and_zoom_is_off() {
    let css = include_str!("../../../assets/serve.css");
    let rule = css.lines().find(|l| l.starts_with(".reply textarea{")).unwrap();
    assert!(
        rule.ends_with("font-size:var(--fs-input)}") && css.contains("--fs-input:16px"),
        "{rule}"
    );
    let (c, _, _sh) = lane("zoom");
    let page = String::from_utf8_lossy(&Server::new(&c, &Fake::default(), None).respond("GET / HTTP/1.1\r\n\r\n", T))
        .into_owned();
    assert!(page.contains("initial-scale=1,maximum-scale=1,user-scalable=no,viewport-fit=cover"));
    assert!(!css.contains(".lock") && !css.contains("body{position:fixed"));
    assert!(include_str!("../../../assets/serve.js").contains("visualViewport"));
}

#[test]
fn secret_flags_real_shapes_only() {
    use drover::lanes::secret;
    for ok in [
        "fn check_prompts_an_idle_orch_after_5_min_then_alerts_after_15() {",
        "    let api_key = config.key();",
        "fn token_for(fleet: &str) -> Option<String> {",
        "let token = std::env::var(\"GITHUB_TOKEN\").unwrap_or_default();",
        "KEY_PATH=/etc/ssl/private/server_2026.pem",
        "let secret_name = \"orch_idle_after_15_min_alert\";",
        "checksum = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"",
        "a3f9c2e1b7d4058a6e3f9c2e1b7d4058a6e3f9c2 src/x.rs",
        "    \"integrity\": \"sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg==\",",
        "    \"apiKey\": \"config_api_key_from_env\",",
    ] {
        assert_eq!(secret(ok), None, "{ok}");
    }
    let pem = concat!("-----BEGIN RSA PRIV", "ATE KEY-----");
    let jwt = concat!(
        "ey",
        "JhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk"
    );
    for (bad, v) in [
        (
            format!("TOKEN = \"ghp_{}\"", "aB3".repeat(12)),
            format!("ghp_{}", "aB3".repeat(12)),
        ),
        (
            concat!("key: sk", "-ant-api03-abcdefghijklmnop1234").into(),
            concat!("sk", "-ant-api03-abcdefghijklmnop1234").into(),
        ),
        (
            concat!("aws_access_key_id = AK", "IAIOSFODNN7EXAMPLE").into(),
            concat!("AK", "IAIOSFODNN7EXAMPLE").into(),
        ),
        (format!("  {pem}"), pem.into()),
        (format!("Authorization: Bearer {jwt}"), jwt.into()),
        ("SECRET=\"h7Gk2pQ9xLm4vB8nR3tZ\"".into(), "h7Gk2pQ9xLm4vB8nR3tZ".into()),
        (
            "API_KEY=\"h7Gk2pQ9xLm4vB8nR3tZ\" // prod".into(),
            "h7Gk2pQ9xLm4vB8nR3tZ".into(),
        ),
        (
            "API_KEY=h7Gk2pQ9xLm4vB8nR3tZ # note".into(),
            "h7Gk2pQ9xLm4vB8nR3tZ".into(),
        ),
        (
            "  \"apiKey\": \"h7Gk2pQ9xLm4vB8nR3tZ\",".into(),
            "h7Gk2pQ9xLm4vB8nR3tZ".into(),
        ),
        ("api_key: h7Gk2pQ9xLm4vB8nR3tZ".into(), "h7Gk2pQ9xLm4vB8nR3tZ".into()),
        (
            format!("x = gho_{}", "aB3".repeat(12)),
            format!("gho_{}", "aB3".repeat(12)),
        ),
        (
            format!("github_pat_11{}", "aB3".repeat(20)),
            format!("github_pat_11{}", "aB3".repeat(20)),
        ),
        (
            concat!("slack: xo", "xb-1234567890-abcdefghijkl").into(),
            concat!("xo", "xb-1234567890-abcdefghijkl").into(),
        ),
        (
            concat!("slack: xo", "xa-2-1234567890-abcdefghij").into(),
            concat!("xo", "xa-2-1234567890-abcdefghij").into(),
        ),
        (
            concat!("slack: xo", "xp-1234567890-abcdefghijkl").into(),
            concat!("xo", "xp-1234567890-abcdefghijkl").into(),
        ),
        (
            concat!("bot 123456789:", "AAHfK3x9Lm4vB8nR3tZqW7eY2uI5oP1aS6d ok").into(),
            concat!("123456789:", "AAHfK3x9Lm4vB8nR3tZqW7eY2uI5oP1aS6d").into(),
        ),
        (
            concat!("TELEGRAM_BOT_TOKEN=123456789:", "AAHfK3x9Lm4vB8nR3tZqW7eY2uI5oP1aS6d").into(),
            concat!("123456789:", "AAHfK3x9Lm4vB8nR3tZqW7eY2uI5oP1aS6d").into(),
        ),
        (
            "const SECRET_KEY: &str = \"h7Gk2pQ9xLm4vB8nR3tZ\";".into(),
            "h7Gk2pQ9xLm4vB8nR3tZ".into(),
        ),
        (
            "    SECRET_KEY: &str = \"h7Gk2pQ9xLm4vB8nR3tZ\",".into(),
            "h7Gk2pQ9xLm4vB8nR3tZ".into(),
        ),
        (
            "const API_KEY: string = \"h7Gk2pQ9xLm4vB8nR3tZ\";".into(),
            "h7Gk2pQ9xLm4vB8nR3tZ".into(),
        ),
        (
            "let apiKey: String = \"h7Gk2pQ9xLm4vB8nR3tZ\"".into(),
            "h7Gk2pQ9xLm4vB8nR3tZ".into(),
        ),
        (
            concat!("let url = \"bot123456789:", "AAHfK3x9Lm4vB8nR3tZqW7eY2uI5oP1aS6d\";").into(),
            concat!("123456789:", "AAHfK3x9Lm4vB8nR3tZqW7eY2uI5oP1aS6d").into(),
        ),
        (
            concat!(
                "https://api.telegram.org/bot123456789:",
                "AAHfK3x9Lm4vB8nR3tZqW7eY2uI5oP1aS6d/sendMessage"
            )
            .into(),
            concat!("123456789:", "AAHfK3x9Lm4vB8nR3tZqW7eY2uI5oP1aS6d").into(),
        ),
        (
            concat!("id = AS", "IAIOSFODNN7EXAMPLE").into(),
            concat!("AS", "IAIOSFODNN7EXAMPLE").into(),
        ),
        (
            "Authorization: Bearer h7Gk2pQ9xLm4vB8nR3tZ".into(),
            "h7Gk2pQ9xLm4vB8nR3tZ".into(),
        ),
        (
            "  \"authorization\": \"Bearer h7Gk2pQ9xLm4vB8nR3tZ\",".into(),
            "h7Gk2pQ9xLm4vB8nR3tZ".into(),
        ),
    ] {
        assert_eq!(secret(&bad), Some(v.as_str()), "{bad}");
    }
}

#[test]
fn kill_saves_code_and_names_the_secret_line() {
    let ok = "diff --git a/src/check.rs b/src/check.rs\n+++ b/src/check.rs\n@@ -10,2 +10,3 @@\n \
              fn a() {}\n+fn check_prompts_an_idle_orch_after_5_min_then_alerts_after_15() {}\n";
    let (c, wt, _sh) = lane("kill-96-ok");
    let r = git(&wt, "", "2\n", ok);
    assert_eq!(run(&c, &r, &["kill", LANE, "--yes"]).0, 0);
    assert!(r.called(&format!("git -C {wt} push origin HEAD:refs/heads/wip/")));
    let bad = format!(
        "{ok}diff --git a/.env b/.env\n+++ b/.env\n@@ -3,1 +3,2 @@\n A=1\n-B=2\n+SECRET=\"h7Gk2pQ9xLm4vB8nR3tZ\" ghp_{}\n",
        "aB3".repeat(12)
    );
    let (c, wt, _sh) = lane("kill-96-bad");
    let r = git(&wt, "", "2\n", &bad);
    let (code, out) = run(&c, &r, &["kill", LANE, "--yes"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("(.env:4: SECRET=\"•••\" •••)") && !out.contains("h7Gk") && !out.contains("ghp_"),
        "{out}"
    );
    assert!(!r.called(&format!("git -C {wt} push")));
}

#[test]
fn a_closed_lane_reads_claudes_saved_session() {
    let (c, wt, sh) = lane("closed-session");
    let ev = json!({"agent": LANE, "reason": "done", "freed_mb": 270});
    append(&c.state, &Event::new(T - 10, "sendmeter", Some(12), "lane-closed", ev)).unwrap();
    let dir = c
        .home
        .join(".claude/projects")
        .join(wt.replace(|ch: char| !ch.is_ascii_alphanumeric(), "-"));
    std::fs::create_dir_all(&dir).unwrap();
    let key = concat!("ghp_", "abcdefghijklmnop0123456789");
    let rows = [
        json!({"type": "user", "message": {"content": "Read the brief and execute it."}}),
        json!({"type": "assistant", "message": {"content": [{"type": "text", "text": format!("Pushed.\ntoken {key}")}, {"type": "tool_use", "name": "Bash", "input": {"command": "cargo test\necho"}}]}}),
        json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "ok"}]}}),
        json!({"type": "summary"}),
    ];
    let jsonl: String = rows.iter().map(|r| format!("{r}\n")).collect();
    std::fs::write(dir.join("s-1.jsonl"), jsonl).unwrap();
    std::fs::write(dir.join("notes.txt"), "not a session").unwrap();
    let log = drover::lanes::log(&c.state, LANE).unwrap();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, "  rolled\n").unwrap();
    let f = Fake::default();
    let r = sh.on(&f);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", T);
    let res = String::from_utf8_lossy(&s.respond(
        &format!("GET /lane?agent={LANE} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n"),
        T,
    ))
    .into_owned();
    let j: serde_json::Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(
        j["lines"],
        json!([
            "❯ Read the brief and execute it.",
            "⏺ Pushed.",
            "  token ghp_•••",
            "⏺ Bash(cargo test)"
        ]),
        "{res}"
    );
    assert_eq!(j["live"], 0);
}

#[test]
fn kill_skips_build_output_and_names_it() {
    let (c, wt, _sh) = lane("kill-build");
    let r = repo(&wt, "");
    std::fs::write(format!("{wt}/real.rs"), "fn x() {}\n").unwrap();
    std::fs::create_dir_all(format!("{wt}/target-lane/release/deps")).unwrap();
    let big = std::fs::File::create(format!("{wt}/target-lane/release/deps/libx.rlib")).unwrap();
    big.set_len(60 << 20).unwrap();
    let (code, out) = run(&c, &r, &["kill", LANE, "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("skipped") && out.contains("target-lane/"), "{out}");
    let b = out
        .split("work pushed to ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    let show = |fmt: &str| {
        let o = std::process::Command::new("git")
            .args(["-C", &format!("{wt}.git"), "show", "--name-only", fmt, b])
            .output()
            .unwrap();
        String::from_utf8(o.stdout).unwrap()
    };
    assert_eq!(show("--format=").trim(), "real.rs");
    let msg = show("--format=%B");
    assert!(msg.contains("skipped") && msg.contains("target-lane/"), "{msg}");
}

#[test]
fn kill_skips_a_file_over_the_size_cap() {
    let (c, wt, _sh) = lane("kill-cap");
    let r = repo(&wt, "");
    std::fs::write(format!("{wt}/real.rs"), "fn x() {}\n").unwrap();
    std::fs::write(format!("{wt}/dump.bin"), vec![7u8; 6 << 20]).unwrap();
    let (code, out) = run(&c, &r, &["kill", LANE, "--yes"]);
    assert!(
        code == 0 && out.contains("skipped") && out.contains("dump.bin (6 MB)"),
        "{out}"
    );
    let o = std::process::Command::new("git")
        .args(["-C", &format!("{wt}.git"), "show", "--name-only", "--format="])
        .arg(
            out.split("work pushed to ")
                .nth(1)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap(),
        )
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(o.stdout).unwrap().trim(), "real.rs");
}

#[test]
fn wip_prune_dry_run_lists_and_deletes_nothing() {
    let d = tmpdir("wip-prune");
    let (origin, local) = (d.join("o.git"), d.join("local"));
    let (o, l) = (
        origin.to_string_lossy().into_owned(),
        local.to_string_lossy().into_owned(),
    );
    let git = |a: &[&str]| {
        let s = std::process::Command::new("git").args(a).output().unwrap();
        assert!(s.status.success(), "{a:?}: {}", String::from_utf8_lossy(&s.stderr));
        String::from_utf8(s.stdout).unwrap()
    };
    git(&["init", "-q", "--bare", &o]);
    git(&["clone", "-q", &o, &l]);
    for (k, v) in [("user.name", "t"), ("user.email", "t@t"), ("commit.gpgsign", "false")] {
        git(&["-C", &l, "config", k, v]);
    }
    git(&["-C", &l, "commit", "-q", "--allow-empty", "-m", "init"]);
    git(&["-C", &l, "push", "-q", "origin", "HEAD:refs/heads/staging"]);
    let wip = |name: &str, file: &str| {
        git(&["-C", &l, "checkout", "-q", "--detach", "origin/staging"]);
        std::fs::create_dir_all(local.join(file).parent().unwrap()).unwrap();
        let mut x = file.len() as u64;
        let bytes: Vec<u8> = (0..2 << 20)
            .map(|_| {
                (
                    x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1),
                    (x >> 56) as u8,
                )
                    .1
            })
            .collect();
        std::fs::write(local.join(file), bytes).unwrap();
        git(&["-C", &l, "add", "-f", file]);
        git(&["-C", &l, "commit", "-qm", "wip"]);
        git(&["-C", &l, "push", "-q", "origin", &format!("HEAD:refs/heads/wip/{name}")]);
    };
    wip("sendmeter-impl-7-20261101-1200", "target-lane/libx.rlib");
    wip("sendmeter-impl-8-20261101-1200", "target-rev/liby.rlib");
    wip("sendmeter-rev-7-r2-20261101-1300", "src/a.rs");
    let c = ctx(&d, &CONFIG.replace("/tmp/does-not-matter/sendmeter", &l));
    append(&c.state, &Event::new(T, "sendmeter", Some(7), "merged", json!({}))).unwrap();
    let r = RealGit(Fake::default(), "");
    let (code, out) = run(&c, &r, &["wip", "prune", "sendmeter", "--dry-run"]);
    assert_eq!(code, 0, "{out}");
    let want = [
        "#7 merged: 2 wip, 1 would delete, 1 kept",
        "#8 ?: 1 wip, 0 would delete, 1 kept",
        "wip/sendmeter-impl-7-20261101-1200 · 2 MB · #7 merged · build output · would delete",
        "wip/sendmeter-impl-8-20261101-1200 · 2 MB · #8 ? · build output · kept",
        "wip/sendmeter-rev-7-r2-20261101-1300 · 2 MB · #7 merged · kept",
    ];
    assert_eq!(out.lines().collect::<Vec<_>>(), want);
    let remote = || git(&["ls-remote", "--heads", &o, "wip/*"]).lines().count();
    assert_eq!(remote(), 3, "a dry run deletes nothing");
    assert!(!r.0.called(&format!("git -C {l} push")));
    let (code, out) = run(&c, &r, &["wip", "prune", "sendmeter"]);
    assert!(
        code == 0 && out.starts_with(&want[0].replace("would delete", "deleted")),
        "{out}"
    );
    assert_eq!(remote(), 2);
}

fn sh_git(a: &[&str]) -> String {
    let s = std::process::Command::new("git").args(a).output().unwrap();
    assert!(s.status.success(), "{a:?}: {}", String::from_utf8_lossy(&s.stderr));
    String::from_utf8(s.stdout).unwrap()
}

#[test]
fn wip_prune_keeps_only_the_newest_of_a_closed_issue() {
    let d = tmpdir("wip-prune-newest");
    let (origin, local) = (d.join("o.git"), d.join("local"));
    let (o, l) = (
        origin.to_string_lossy().into_owned(),
        local.to_string_lossy().into_owned(),
    );
    sh_git(&["init", "-q", "--bare", &o]);
    sh_git(&["clone", "-q", &o, &l]);
    for (k, v) in [("user.name", "t"), ("user.email", "t@t"), ("commit.gpgsign", "false")] {
        sh_git(&["-C", &l, "config", k, v]);
    }
    sh_git(&["-C", &l, "commit", "-q", "--allow-empty", "-m", "init"]);
    sh_git(&["-C", &l, "push", "-q", "origin", "HEAD:refs/heads/staging"]);
    let wip = |name: &str| {
        sh_git(&["-C", &l, "checkout", "-q", "--detach", "origin/staging"]);
        std::fs::write(local.join("a.rs"), name).unwrap();
        sh_git(&["-C", &l, "add", "a.rs"]);
        sh_git(&["-C", &l, "commit", "-qm", "wip"]);
        sh_git(&["-C", &l, "push", "-q", "origin", &format!("HEAD:refs/heads/wip/{name}")]);
    };
    for t in ["0801", "0802", "0803", "0804", "0805"] {
        wip(&format!("sendmeter-impl-48-2026110{}-{t}00-ab12", &t[3..]));
    }
    for t in ["0801", "0802"] {
        wip(&format!("sendmeter-impl-49-2026110{}-{t}00-cd34", &t[3..]));
    }
    let c = ctx(&d, &CONFIG.replace("/tmp/does-not-matter/sendmeter", &l));
    append(&c.state, &Event::new(T, "sendmeter", Some(48), "closed", json!({}))).unwrap();
    append(
        &c.state,
        &Event::new(T, "sendmeter", Some(49), "spawn", json!({"role": "impl"})),
    )
    .unwrap();
    let _sh = FakeShed::attached(&c);
    let r = RealGit(Fake::default(), "");
    let remote = || sh_git(&["ls-remote", "--heads", &o, "wip/*"]);
    let (code, out) = run(&c, &r, &["wip", "prune", "sendmeter", "--dry-run"]);
    assert_eq!(code, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[..2],
        [
            "#48 closed: 5 wip, 4 would delete, 1 kept",
            "#49 impl: 2 wip, 0 would delete, 2 kept"
        ],
        "{out}"
    );
    assert_eq!(remote().lines().count(), 7, "a dry run deletes nothing");
    let (code, out) = run(&c, &r, &["clean", "sendmeter"]);
    assert!(
        code == 0 && out.contains("#48 closed: 5 wip, 4 would delete, 1 kept\n"),
        "{code} {out}"
    );
    assert_eq!(remote().lines().count(), 7, "clean without --yes deletes nothing");
    let (code, out) = run(&c, &r, &["wip", "prune", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("#48 closed: 5 wip, 4 deleted, 1 kept\n"), "{out}");
    let left = remote();
    let left: Vec<&str> = left.lines().filter_map(|l| l.split("refs/heads/").nth(1)).collect();
    assert_eq!(
        left,
        [
            "wip/sendmeter-impl-48-20261105-080500-ab12",
            "wip/sendmeter-impl-49-20261101-080100-cd34",
            "wip/sendmeter-impl-49-20261102-080200-cd34"
        ]
    );
}
