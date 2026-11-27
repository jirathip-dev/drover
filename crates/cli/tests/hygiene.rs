mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const HOSTS: &str = "\n[hosts.bazzite]\nmachine = \"me@bazzite.example\"\n";

fn fleets() -> String {
    let fleet = |(n, extra): (&str, &str)| {
        format!("\n[fleet.{n}]\nrepo = \"o/{n}\"\nlocal = \"/tmp/{n}\"\nintegration = \"staging\"\n{extra}")
    };
    let rows = [
        ("open", "public = true\nprivate_names = [\"Globex\", \"Acme Corp\"]\n"),
        ("closed", "public = false\n"),
        ("cached", ""),
    ];
    CONFIG.to_string() + HOSTS + &rows.map(fleet).concat()
}

fn hook(d: &Path, cwd: &Path, lane: &str, args: &[&str], stdin: &str) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_drover"))
        .env_remove("DROVER_RULES")
        .env_remove("DROVER_SCRATCH_TARGET")
        .env("HOME", d)
        .env("DROVER_LANE", lane)
        .env("FLEET_CONFIG", d.join("fleet.toml"))
        .env("FLEET_STATE", d.join("state/events.jsonl"))
        .args([&["hook", "--role", "impl"][..], args].concat())
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = tmpdir(name);
    std::fs::write(d.join("fleet.toml"), fleets()).unwrap();
    d
}

fn bash(d: &Path, fleet: &str, cmd: &str) -> String {
    let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}, "cwd": d});
    hook(d, d, &format!("{fleet}:24:impl"), &[], &input.to_string()).1
}

#[test]
fn a_lanes_gh_text_with_a_home_path_is_refused_by_pattern_never_by_value() {
    let d = dir("hygiene-gh");
    std::fs::write(d.join("body.md"), "logs in /Users/alice/proj/x.log\n").unwrap();
    for cmd in [
        "gh issue create -R o/open --title t --body 'see /Users/alice/proj/x.log'",
        "cd /home/alice/wt && gh pr create --title t --body-file body.md",
        "gh pr edit 3 --body \"$(cat <<'EOF'\nat /Users/alice/proj\nEOF\n)\"",
        "gh issue comment 3 --body=/Users/alice/proj",
        "gh api repos/o/open/issues/3/comments -f body='/Users/alice/proj'",
    ] {
        let out = bash(&d, "open", cmd);
        assert!(
            out.contains("REFUSED PUBLIC_LEAK") && out.contains("home-path"),
            "{cmd}: {out}"
        );
        assert!(!out.contains("alice"), "the value is never printed: {out}");
    }
    let clean = bash(
        &d,
        "open",
        "cd /home/alice/wt && gh pr create --title t --body 'adds the meter'",
    );
    assert!(!clean.contains("PUBLIC_LEAK"), "{clean}");
    let private = bash(&d, "closed", "gh issue create --title t --body 'see /Users/alice/proj'");
    assert!(
        !private.contains("PUBLIC_LEAK"),
        "a private repo is not checked: {private}"
    );
}

#[test]
fn a_private_name_is_refused_by_its_list_index_and_hosts_by_kind() {
    let d = dir("hygiene-names");
    let out = bash(
        &d,
        "open",
        "gh issue create --title t --body 'port the acme corp screens'",
    );
    assert!(
        out.contains("REFUSED PUBLIC_LEAK") && out.contains("private-name #2"),
        "{out}"
    );
    assert!(!out.to_lowercase().contains("acme"), "{out}");
    let out = bash(&d, "open", "gh pr create --title 'ssh bazzite' --body ok");
    assert!(out.contains("gh pr create --title has a host"), "{out}");
    let out = bash(
        &d,
        "open",
        "gh pr create --title t --body 'Acme is a placeholder, as is Globexx'",
    );
    assert!(!out.contains("PUBLIC_LEAK"), "whole names only: {out}");
}

#[test]
fn a_host_name_is_a_leak_only_where_it_is_used_as_a_hostname() {
    let d = dir("hygiene-host-word");
    let out = bash(
        &d,
        "open",
        "gh pr create --title t --body 'the Bazzite now sends a signed alive message; bazzite is idle.'",
    );
    assert!(!out.contains("PUBLIC_LEAK"), "a plain word: {out}");
    for text in [
        "ssh bazzite",
        "me@bazzite",
        "bazzite:/srv/x",
        "bazzite:22",
        "bazzite.local",
        "--host bazzite",
        "[hosts.bazzite]",
        "me@bazzite.example",
    ] {
        let out = bash(&d, "open", &format!("gh pr create --title t --body '{text}'"));
        assert!(out.contains("--body has a host"), "{text}: {out}");
    }
}

#[test]
fn a_public_gatt_service_uuid_is_no_session_id() {
    let d = dir("hygiene-gatt");
    let out = bash(
        &d,
        "open",
        "gh pr create --title t --body 'the service is 7e4e1701-1ea6-40c9-9dcc-13d34ffead57'",
    );
    assert!(!out.contains("PUBLIC_LEAK"), "{out}");
}

fn session_file(d: &Path, id: &str) {
    let dir = d.join(".claude/projects/-wt-x");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{id}.jsonl")), "{}\n").unwrap();
}

#[test]
fn a_uuid_drover_or_the_provider_knows_as_a_session_is_still_refused() {
    let d = dir("hygiene-known-session");
    let filed = "0b1c2d3e-1111-2222-3333-444455556666";
    session_file(&d, filed);
    let logged = "9f8e7d6c-aaaa-bbbb-cccc-ddddeeeeffff";
    let start = Event::new(
        1,
        "open",
        None,
        "orchestrator-start",
        json!({"thread": "t1", "session": logged}),
    );
    append(&d.join("state/events.jsonl"), &start).unwrap();
    for id in [filed, &logged.to_uppercase()] {
        let out = bash(&d, "open", &format!("gh pr create --title t --body 'resume {id}'"));
        assert!(
            out.contains("REFUSED PUBLIC_LEAK") && out.contains("--body has a session-id"),
            "{id}: {out}"
        );
        assert!(!out.to_lowercase().contains(&id.to_lowercase()), "{out}");
    }
}

#[test]
fn a_session_id_already_on_the_integration_branch_passes() {
    let d = tmpdir("hygiene-tree-session");
    let (shown, kept) = (
        "0b1c2d3e-1111-2222-3333-444455556666",
        "1a2b3c4d-5555-6666-7777-888899990000",
    );
    let up = d.join("up");
    std::fs::create_dir_all(&up).unwrap();
    std::fs::write(up.join("ids.md"), format!("example {shown}\n")).unwrap();
    let git = |dir: &Path, a: &[&str]| {
        let o = Command::new("git").args(a).current_dir(dir).output().unwrap();
        assert!(o.status.success(), "{a:?}: {}", String::from_utf8_lossy(&o.stderr));
    };
    git(&up, &["init", "-q", "-b", "staging"]);
    git(&up, &["add", "."]);
    git(
        &up,
        &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "ids"],
    );
    git(&d, &["clone", "-q", "up", "clone"]);
    let fleet = format!(
        "\n[fleet.open]\nrepo = \"o/open\"\nlocal = \"{}\"\nintegration = \"staging\"\npublic = true\n",
        d.join("clone").display()
    );
    std::fs::write(d.join("fleet.toml"), CONFIG.to_string() + &fleet).unwrap();
    session_file(&d, shown);
    session_file(&d, kept);
    let out = bash(&d, "open", &format!("gh pr create --title t --body 'see {shown}'"));
    assert!(!out.contains("PUBLIC_LEAK"), "already public on staging: {out}");
    let out = bash(&d, "open", &format!("gh pr create --title t --body 'see {kept}'"));
    assert!(out.contains("session-id"), "{out}");
}

#[test]
fn visibility_is_read_once_a_day_and_the_flag_wins() {
    let d = tmpdir("hygiene-visibility");
    let mut c = ctx(&d, &fleets());
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let r = Fake::default().reply("gh repo view o/cached --json visibility", r#"{"visibility":"PRIVATE"}"#);
    assert!(!drover::hygiene::public(&c, Some(&r), &cfg, "cached"));
    c.now += 86_399;
    assert!(!drover::hygiene::public(&c, Some(&r), &cfg, "cached"));
    assert_eq!(r.count("gh repo view"), 1, "cached for the day");
    c.now += 1;
    assert!(!drover::hygiene::public(&c, Some(&r), &cfg, "cached"));
    assert_eq!(r.count("gh repo view"), 2, "read again after a day");
    let r = Fake::default().reply("gh repo view", r#"{"visibility":"PUBLIC"}"#);
    assert!(
        drover::hygiene::public(&c, Some(&r), &cfg, "open") && !drover::hygiene::public(&c, Some(&r), &cfg, "closed")
    );
    assert!(!r.called("gh repo view"), "the flag wins: {:?}", r.calls.borrow());
}

const HEAD: &str = "1111111111111111111111111111111111111111";

fn merge_setup(name: &str, config: &str) -> drover::Ctx {
    let c = ctx(&tmpdir(name), config);
    let spawn = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    let v = json!({"verdict": "PASS", "sha": HEAD});
    append(&c.state, &Event::new(20, "sendmeter", Some(12), "verdict", v)).unwrap();
    c
}

fn merge_fake(base: &str, body: &str) -> Fake {
    let green = json!([{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let pr =
        json!({"number": 34, "state": "OPEN", "baseRefName": base, "headRefOid": HEAD, "statusCheckRollup": green});
    let info = json!({"number": 34, "title": "Meter", "body": body, "files": [], "commits": []});
    Fake::default()
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("gh pr merge 34", "")
        .reply(
            "gh pr view 34 -R jirathip-dev/sendmeter --json number",
            &info.to_string(),
        )
        .reply("gh pr view 34", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
        .reply("gh issue", "")
}

#[test]
fn merges_squash_message_and_closing_comment_go_through_the_check() {
    let c = merge_setup("hygiene-merge", CONFIG);
    let r = merge_fake("staging", "## For humans\nLogs land in /home/alice/meter.\n");
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED PUBLIC_LEAK: gh pr merge --body has a home-path"),
        "{out}"
    );
    assert!(!r.called("gh pr merge") && !out.contains("alice"), "{out}");

    let config = CONFIG.replace("integration = \"staging\"", "integration = \"hosts.bazzite\"") + HOSTS;
    let c = merge_setup("hygiene-merge-comment", &config);
    let r = merge_fake("hosts.bazzite", "## For humans\nThe meter counts.\n");
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_ne!(code, 0, "{out}");
    assert!(r.called("gh pr merge 34"), "the squash text passed");
    assert!(
        !r.called("gh issue comment"),
        "the closing comment names the host: never sent"
    );
    let ev = read(&c.state).unwrap();
    let miss = ev
        .iter()
        .find(|e| e.kind == "evidence-missing")
        .expect("evidence-missing");
    assert!(miss.str("reason").unwrap().contains("PUBLIC_LEAK"), "{:?}", miss.data);
}

#[test]
fn amend_text_naming_a_private_name_is_refused_before_the_edit() {
    let config = CONFIG.to_string().replace(
        "integration = \"staging\"   # never main",
        "integration = \"staging\"\nprivate_names = [\"Initech\"]",
    );
    let c = ctx(&tmpdir("hygiene-amend"), &config);
    let ask = json!({"id": "q1", "question": "?"});
    append(&c.state, &Event::new(1, "sendmeter", Some(60), "needs-decision", ask)).unwrap();
    let d = json!({"id": "q1", "choice": "yes", "by": "owner"});
    append(&c.state, &Event::new(2, "sendmeter", Some(60), "decision", d)).unwrap();
    let r = Fake::default()
        .reply("gh issue view 60", &json!({"body": "Tidy the tab.\n"}).to_string())
        .reply("gh issue edit 60", "");
    let (code, out) = run(
        &c,
        &r,
        &["amend", "sendmeter", "60", "--basis", "q1", "match initech's tab"],
    );
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED PUBLIC_LEAK: the amendment has a private-name #1"),
        "{out}"
    );
    assert!(!r.called("gh issue edit"), "{:?}", r.calls.borrow());
    let (code, out) = run(
        &c,
        &r,
        &["amend", "sendmeter", "60", "--basis", "q1", "scope: tab only"],
    );
    assert_eq!(code, 0, "{out}");
    assert!(r.called("gh issue edit 60"));
}

#[test]
fn kill_refuses_a_wip_snapshot_with_a_home_path_and_pushes_nothing() {
    let d = tmpdir("hygiene-wip");
    let c = ctx(&d, CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let wt = d.join("wt").to_string_lossy().into_owned();
    std::fs::create_dir_all(&wt).unwrap();
    let lane = "sendmeter-impl-12";
    let sp =
        json!({"role": "impl", "agent": lane, "pane": lane, "worktree": wt, "branch": "issue-12", "runtime": "shed"});
    append(&c.state, &Event::new(c.now - 60, "sendmeter", Some(12), "spawn", sp)).unwrap();
    sh.lane(lane, "idle", &wt);
    let diff = "diff --git a/run.sh b/run.sh\n+++ b/run.sh\n@@ -0,0 +1,2 @@\n+set -e\n+cd /Users/alice/meter\n";
    let r = Fake::default()
        .reply(&format!("git -C {wt} status --porcelain"), "")
        .reply(&format!("git -C {wt} rev-list --count"), "1\n")
        .reply(&format!("git -C {wt} log -p"), diff)
        .reply(&format!("git -C {wt} "), "")
        .reply("date -r", "20261031-120000\n");
    let (code, out) = run(&c, &r, &["kill", lane, "--yes"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED PUBLIC_LEAK") && out.contains("run.sh:2 has a home-path"),
        "{out}"
    );
    assert!(!out.contains("alice"), "{out}");
    assert!(!r.called(&format!("git -C {wt} push")) && !sh.called("kill", lane));
}

#[test]
fn doctor_warns_once_for_public_fleets_with_no_private_names() {
    let c = ctx(&tmpdir("hygiene-doctor"), &fleets());
    let r = Fake::default().reply("gh repo view", r#"{"visibility":"PUBLIC"}"#);
    let (_, out) = run(&c, &r, &["doctor", "--config-only"]);
    let lines: Vec<&str> = out.lines().filter(|l| l.contains("private_names")).collect();
    assert_eq!(
        lines,
        ["! hygiene — public fleet(s) with no private_names list: cached, sendmeter → set [fleet.<n>] private_names"],
        "{out}"
    );
}

#[test]
fn scan_lists_hits_by_pattern_with_links_and_changes_nothing() {
    let c = ctx(&tmpdir("hygiene-scan"), &fleets());
    let issues = json!([{"title": "t", "body": "runs on bazzite:22", "url": "https://github.com/o/open/issues/1",
        "comments": [{"body": "see /Users/alice/x", "url": "https://github.com/o/open/issues/1#c2"}]}]);
    let prs = json!([{"title": "Globex port", "body": "", "url": "https://github.com/o/open/pull/2", "comments": []}]);
    let sha = "a".repeat(40);
    let log = format!(
        "commit {sha}\nAuthor: t\n\n    clean message\n\ndiff --git a/x b/x\n+++ b/x\n@@ -0,0 +1,2 @@\n+ok\n+/home/bob/y\n"
    );
    let r = Fake::default()
        .reply("gh issue list -R o/open", &issues.to_string())
        .reply("gh pr list -R o/open", &prs.to_string())
        .reply("git -C /tmp/open log -p origin/staging", &log);
    let (code, out) = run(&c, &r, &["hygiene", "scan", "open"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        format!(
            "home-path · https://github.com/o/open/commit/{sha} x:2\n\
             home-path · https://github.com/o/open/issues/1#c2\n\
             host · https://github.com/o/open/issues/1\n\
             private-name #1 · https://github.com/o/open/pull/2\n\
             4 hit(s) in o/open; nothing changed\n"
        )
    );
    let calls = r.calls.borrow();
    assert!(
        calls.iter().all(|l| l.contains(" list ") || l.contains(" log -p ")),
        "{calls:?}"
    );
    assert_eq!(calls.len(), 3);
}

#[test]
fn the_check_targets_gh_repo_then_the_checkouts_remote_then_the_fleet() {
    let d = dir("hygiene-target");
    let f = d.join("f.md");
    std::fs::write(&f, "for the Globex screens\n").unwrap();
    let co = d.join("co");
    std::fs::create_dir(&co).unwrap();
    for args in [
        &["init", "-q"][..],
        &["remote", "add", "origin", "git@github.com:o/closed.git"],
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(&co)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    let f = f.display();
    for cmd in [
        format!("gh issue create --repo o/closed --title t --body-file {f}"),
        format!("gh issue create --repo=O/Closed --title t --body-file {f}"),
        format!("gh issue create -R github.com/o/closed --title t --body-file {f}"),
        format!("cd co && gh issue create --title t --body-file {f}"),
    ] {
        let out = bash(&d, "open", &cmd);
        assert!(
            !out.contains("PUBLIC_LEAK"),
            "a private target is not checked: {cmd}: {out}"
        );
    }
    let out = bash(
        &d,
        "open",
        &format!("gh issue create --repo o/other --title t --body-file {f}"),
    );
    assert!(
        out.contains("REFUSED PUBLIC_LEAK") && out.contains("private-name #1, and o/other is public"),
        "{out}"
    );
    let out = bash(
        &d,
        "open",
        &format!("cd co && gh issue create -R o/other --title t --body-file {f}"),
    );
    assert!(
        out.contains("and o/other is public"),
        "--repo wins over the checkout: {out}"
    );
    let out = bash(&d, "open", &format!("gh issue create --title t --body-file {f}"));
    assert!(
        out.contains("private-name #1, and o/open is public"),
        "no checkout: the fleet's repo: {out}"
    );
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-C"])
        .arg(dir)
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn check(d: &Path, env: &[(&str, &str)], args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_drover"))
        .env_remove("DROVER_PRIVATE_NAMES")
        .env_remove("DROVER_LANE")
        .env("HOME", d)
        .env("FLEET_CONFIG", d.join("none.toml"))
        .env("FLEET_STATE", d.join("state/events.jsonl"))
        .envs(env.iter().copied())
        .args([&["hygiene", "check"][..], args].concat())
        .current_dir(d.join("repo"))
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    (o.status.code().unwrap(), s)
}

fn checkout(name: &str) -> std::path::PathBuf {
    let d = tmpdir(name);
    let repo = d.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.md"), "intro\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    std::fs::write(repo.join("a.md"), "intro\nports the globex screens\n").unwrap();
    std::fs::write(repo.join("untracked.md"), "Acme Corp\n").unwrap();
    git(&repo, &["add", "a.md"]);
    git(&repo, &["commit", "-qm", "work for Acme Corp"]);
    std::fs::write(d.join("body.md"), "## For humans\n\nfor Globex\n").unwrap();
    d
}

#[test]
fn check_fails_on_a_name_in_the_tree_a_commit_message_and_the_pr_body_never_printing_it() {
    let d = checkout("hygiene-check");
    let body = d.join("body.md");
    let env = [("DROVER_PRIVATE_NAMES", "Globex\n\n  Acme Corp \n")];
    let (code, out) = check(&d, &env, &["--base", "main~1", "--pr-body", body.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    let sha = Command::new("git")
        .arg("-C")
        .arg(d.join("repo"))
        .args(["rev-parse", "--short=7", "HEAD"])
        .output();
    let sha = String::from_utf8(sha.unwrap().stdout).unwrap();
    assert_eq!(
        out,
        format!(
            "private-name #1 at a.md:2\nprivate-name #2 at commit {} message\nprivate-name #1 at PR body:3\n\
             3 hit(s) of 2 private name(s); names are never printed\n",
            sha.trim()
        )
    );
    let (code, out) = check(&d, &[("DROVER_PRIVATE_NAMES", "Initech")], &["--base", "main~1"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn check_with_no_list_fails_list_missing_and_reads_the_lanes_fleet_list_on_its_host() {
    let d = checkout("hygiene-check-list");
    for env in [&[][..], &[("DROVER_PRIVATE_NAMES", " \n")]] {
        let (code, out) = check(&d, env, &[]);
        assert_eq!(code, 1, "{out}");
        assert!(out.starts_with("list-missing: "), "{out}");
    }
    std::fs::write(d.join("none.toml"), fleets()).unwrap();
    let (code, out) = check(&d, &[("DROVER_LANE", "open:24:impl")], &[]);
    assert_eq!(
        (code, out.as_str()),
        (
            1,
            "private-name #1 at a.md:2\n1 hit(s) of 2 private name(s); names are never printed\n"
        )
    );
    let (code, out) = check(&d, &[], &["closed"]);
    assert_eq!(
        (code, out.as_str()),
        (0, "o/closed (closed) is private: nothing to check\n")
    );
}

#[test]
fn push_writes_the_list_as_the_repos_secret_on_stdin_and_never_prints_it() {
    let c = ctx(&tmpdir("hygiene-push-secret"), &fleets());
    let r = Fake::default().reply("gh secret set", "");
    let (code, out) = run(&c, &r, &["hygiene", "push", "open"]);
    assert_eq!(
        (code, out.as_str()),
        (0, "wrote 2 name(s) to o/open's PRIVATE_NAMES secret\n")
    );
    assert_eq!(
        *r.inputs.borrow(),
        [(
            "gh secret set PRIVATE_NAMES -R o/open".to_string(),
            "Globex\nAcme Corp".to_string()
        )]
    );
    assert!(r.calls.borrow().iter().all(|l| !l.contains("Globex")));
    let (code, _) = run(&c, &r, &["hygiene", "push", "closed"]);
    assert_eq!((code, r.inputs.borrow().len()), (1, 1));
}

#[test]
fn push_is_the_owners_never_a_lanes_or_an_orchestrators() {
    let d = dir("hygiene-push-hook");
    let cmd = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "drover hygiene push open"}, "cwd": d});
    for lane in ["open:24:impl", "open:-:orchestrator"] {
        let out = hook(&d, &d, lane, &[], &cmd.to_string()).1;
        assert!(out.contains("REFUSED OWNER_ONLY"), "{lane}: {out}");
    }
    assert!(!bash(&d, "open", "drover hygiene check --base origin/staging").contains("REFUSED"));
}

#[test]
fn the_guard_matches_private_names_as_the_shared_set_says() {
    let set = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/private-names");
    let read = |f: &str| std::fs::read_to_string(set.join(f)).unwrap();
    let check = drover::hygiene::Check {
        private: read("names").lines().map(String::from).collect(),
        ..Default::default()
    };
    let hits: Vec<String> = (read("lines.md").lines().zip(1..))
        .filter_map(|(l, n)| Some(format!("{} at lines.md:{n}", check.find(l)?)))
        .collect();
    assert_eq!(hits.join("\n"), read("expect").trim());
}
