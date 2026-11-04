//! AC4: pipe PreToolUse (Claude) and pre_tool_call (Hermes) JSON into `herdsman hook`.
mod common;
use common::{tmpdir, CONFIG};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

/// `herdsman hook` as `role`, inside a fleet lane (`HERDSMAN_LANE` set) or outside one.
fn hook_in(lane: Option<&str>, role: &str, input: Value) -> String {
    hook_env(&[("HERDSMAN_LANE", lane)], role, input)
}

/// `herdsman hook` as `role` with each lane var set (`Some`) or unset (`None`).
fn hook_env(vars: &[(&str, Option<&str>)], role: &str, input: Value) -> String {
    hook_cfg(CONFIG, vars, role, input).0
}

/// `herdsman hook` under fleet.toml `cfg`: its stdout and the temp dir (`state/events.jsonl`).
fn hook_cfg(cfg: &str, vars: &[(&str, Option<&str>)], role: &str, input: Value) -> (String, std::path::PathBuf) {
    let d = tmpdir("hook");
    std::fs::write(d.join("fleet.toml"), cfg).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_herdsman"));
    cmd.env_remove("HERDSMAN_LANE").env_remove("FLEET_LANE");
    for (k, v) in vars {
        if let Some(v) = v {
            cmd.env(k, v);
        }
    }
    let mut child = cmd
        .args(["hook", "--role", role])
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
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    (String::from_utf8(out.stdout).unwrap(), d)
}

fn hook_as(role: &str, input: Value) -> String {
    hook_in(Some(&format!("sendmeter:24:{role}")), role, input)
}

/// The same tool call in Claude's shape and in both Hermes shapes (stdin wire
/// `tool_input`, raw `args`). Claude tool names map to Hermes ones.
fn shapes(tool: &str, ti: Value) -> [Value; 3] {
    let htool = match tool {
        "Bash" => "terminal",
        "Edit" => "patch",
        _ => "write_file",
    };
    let mut hti = ti.clone();
    if let Some(f) = ti.get("file_path") {
        hti = json!({"path": f});
    }
    let cwd = std::env::temp_dir();
    [
        json!({"hook_event_name": "PreToolUse", "tool_name": tool, "cwd": cwd, "tool_input": ti}),
        json!({"hook_event_name": "pre_tool_call", "tool_name": htool, "cwd": cwd, "tool_input": hti,
            "profile": "fleet-rev", "session_id": "s", "extra": {}}),
        json!({"tool_name": htool, "args": hti}),
    ]
}

/// Runs the call in every shape as `role`. Some(message) when all deny (each in its
/// own dialect), None when all allow (empty stdout); panics when the shapes disagree.
fn call(role: &str, tool: &str, ti: Value) -> Option<String> {
    let outs: Vec<String> = shapes(tool, ti).into_iter().map(|i| hook_as(role, i)).collect();
    if outs.iter().all(String::is_empty) {
        return None;
    }
    let v: Vec<Value> = outs
        .iter()
        .map(|o| serde_json::from_str(o).unwrap_or_default())
        .collect();
    let claude = &v[0]["hookSpecificOutput"];
    assert_eq!(claude["permissionDecision"], "deny", "claude: {outs:?}");
    for h in &v[1..] {
        assert_eq!(h["action"], "block", "hermes: {outs:?}");
        assert_eq!(h["message"], claude["permissionDecisionReason"], "{outs:?}");
    }
    Some(claude["permissionDecisionReason"].as_str().unwrap().to_string())
}

fn bash_as(role: &str, cmd: &str) -> Option<String> {
    call(role, "Bash", json!({"command": cmd}))
}

fn bash(cmd: &str) -> Option<String> {
    bash_as("impl", cmd)
}

/// #24 AC4: the hook is inert outside a fleet lane (shared Hermes profiles serve
/// non-fleet lanes too) and enforces its rules inside one.
#[test]
fn hook_is_inert_unless_fleet_lane_is_set() {
    for cmd in ["gh pr merge 5", "git push origin HEAD:staging"] {
        for input in shapes("Bash", json!({"command": cmd})) {
            assert_eq!(
                hook_in(None, "impl", input.clone()),
                "",
                "{cmd} allowed without HERDSMAN_LANE"
            );
            assert_eq!(
                hook_in(Some(""), "impl", input.clone()),
                "",
                "{cmd} allowed with empty HERDSMAN_LANE"
            );
            assert_ne!(
                hook_in(Some("fleet:24:impl"), "impl", input),
                "",
                "{cmd} blocked in a fleet lane"
            );
        }
    }
    for input in shapes("Bash", json!({"command": "git status"})) {
        assert_eq!(
            hook_in(Some("fleet:24:impl"), "impl", input),
            "",
            "git status allowed in a fleet lane"
        );
    }
}

/// #27: `FLEET_LANE` still arms the hook for one release; `HERDSMAN_LANE` is
/// authoritative when both are set, so an empty `HERDSMAN_LANE` keeps it inert.
#[test]
fn legacy_fleet_lane_arms_the_hook_and_herdsman_lane_wins() {
    for input in shapes("Bash", json!({"command": "gh pr merge 5"})) {
        let arm = |vars: &[(&str, Option<&str>)]| hook_env(vars, "impl", input.clone());
        assert_ne!(
            arm(&[("FLEET_LANE", Some("fleet:24:impl"))]),
            "",
            "FLEET_LANE alone arms"
        );
        assert_ne!(
            arm(&[("HERDSMAN_LANE", Some("a:1:impl")), ("FLEET_LANE", Some("b:2:rev"))]),
            "",
            "both set arms"
        );
        assert_eq!(
            arm(&[("HERDSMAN_LANE", Some("")), ("FLEET_LANE", Some("fleet:24:impl"))]),
            "",
            "empty HERDSMAN_LANE wins"
        );
        assert_eq!(arm(&[("FLEET_LANE", Some(""))]), "", "empty FLEET_LANE is inert");
    }
}

#[test]
fn gh_pr_merge_is_denied() {
    let out = bash("gh pr merge 12").expect("denied");
    assert!(
        out.contains("herdsman merge"),
        "deny message points to herdsman merge: {out}"
    );
    assert!(bash("cd /x && gh pr merge 3 --squash --admin").is_some());
    assert!(bash("gh api -X PUT repos/o/r/pulls/3/merge").is_some());
}

#[test]
fn github_comments_are_denied() {
    for cmd in [
        "gh issue comment 5 -b x",
        "gh pr comment 5 -b x",
        "gh api -X POST repos/o/r/issues/5/comments",
        "gh api repos/o/r/issues/5/comments -f body=x",
        "gh api --method=PATCH repos/o/r/issues/comments/9 -f body=y",
        "cd /x && gh pr review 5 --comment -b x",
    ] {
        let out = bash(cmd).unwrap_or_else(|| panic!("{cmd} allowed"));
        assert!(out.contains("never comment on GitHub"), "{cmd}: {out}");
    }
    for cmd in [
        "gh issue view 5",
        "gh pr view 5 --json state",
        "gh issue view 5 --comments",
        "gh api repos/o/r/issues/5/comments",
    ] {
        assert_eq!(bash(cmd), None, "read-only {cmd} is allowed");
    }
}

#[test]
fn push_to_staging_is_denied() {
    for role in ["impl", "rev"] {
        assert!(bash_as(role, "git push origin staging")
            .unwrap()
            .contains("herdsman merge"));
        assert!(bash_as(role, "git push origin HEAD:staging").is_some());
    }
    assert!(bash("git push origin HEAD:main").is_some());
    assert!(bash("git push -f origin +feature:refs/heads/staging").is_some());
    assert!(bash("git -C /some/repo push origin main").is_some());
    assert!(bash("git push --all origin").is_some());
}

#[test]
fn hermes_terminal_workdir_resolves_bare_push() {
    // A bare `git push` pushes HEAD of the terminal's workdir.
    let d = tmpdir("hook-wd");
    let git = |a: &[&str]| {
        assert!(Command::new("git")
            .arg("-C")
            .arg(&d)
            .args(a)
            .output()
            .unwrap()
            .status
            .success())
    };
    git(&["init", "-q", "-b", "staging"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "x",
    ]);
    let input = json!({"hook_event_name": "pre_tool_call", "tool_name": "terminal", "cwd": "/",
        "tool_input": {"command": "git push", "workdir": d}});
    let v: Value = serde_json::from_str(&hook_as("impl", input)).unwrap();
    assert_eq!(v["action"], "block");
}

#[test]
fn delete_refspec_on_protected_branch_is_denied() {
    assert!(bash("git push origin :staging").is_some());
    assert!(bash("git push origin --delete staging").is_some());
    assert!(bash("git push origin :refs/heads/main").is_some());
    assert!(bash("git push -d origin master").is_some());
    assert_eq!(bash("git push origin :issue-12-thing"), None);
    assert_eq!(bash("git push origin --delete rev-12-r1"), None);
}

#[test]
fn push_to_feature_branch_is_allowed() {
    assert_eq!(bash("git push origin issue-12-thing"), None);
    assert_eq!(bash("git push -u origin feature-x && git status"), None);
    assert_eq!(bash("gh pr view 12 --json state"), None);
}

#[test]
fn reviewers_are_read_only() {
    for cmd in [
        "git commit -am x",
        "git push origin rev-12-r1",
        "git -C /x merge main",
        "git rebase origin/staging",
        "git reset --hard",
    ] {
        let out = bash_as("rev", cmd).unwrap_or_else(|| panic!("rev {cmd} allowed"));
        assert!(out.contains("read-only"), "{cmd}: {out}");
    }
    for cmd in [
        "git commit -am x",
        "git push origin issue-12",
        "git rebase origin/staging",
    ] {
        assert_eq!(bash_as("impl", cmd), None, "impl may run {cmd}");
    }
    for cmd in [
        "git status",
        "git diff origin/staging...HEAD",
        "cargo test",
        "gh pr view 5",
    ] {
        assert_eq!(bash_as("rev", cmd), None, "rev may run {cmd}");
    }
    let src = json!({"file_path": "/wt/src/hook.rs"});
    assert!(call("rev", "Edit", src.clone()).unwrap().contains("read-only"));
    assert!(call("rev", "Write", src.clone()).is_some());
    assert_eq!(call("impl", "Edit", src), None);
    assert_eq!(
        call("rev", "Write", json!({"file_path": "/wt/rev-12-r1/verdict.md"})),
        None
    );
    assert_eq!(call("rev", "Write", json!({"file_path": "verdict.md"})), None);
    // A V4A patch carries no `path`.
    let v4a = json!({"tool_name": "patch", "args": {"mode": "patch", "patch": "*** Update File: src/a.rs\n"}});
    assert_eq!(
        serde_json::from_str::<Value>(&hook_as("rev", v4a)).unwrap()["action"],
        "block"
    );
}

#[test]
fn protected_config_edits_are_denied() {
    let d = tmpdir("hook-cfg");
    let cfg = d.join("fleet.toml");
    std::fs::write(&cfg, CONFIG).unwrap();
    let run = |input: Value| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_herdsman"))
            .arg("hook")
            .env("FLEET_CONFIG", &cfg)
            .env("FLEET_STATE", d.join("state/events.jsonl"))
            .env("HERDSMAN_LANE", "sendmeter:24:impl")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
    };
    let deny = |tool: &str, ti: Value| {
        let outs: Vec<String> = shapes(tool, ti).into_iter().map(&run).collect();
        assert!(outs[0].contains("\"deny\""), "{outs:?}");
        assert!(outs[1..].iter().all(|o| o.contains("\"block\"")), "{outs:?}");
    };
    deny("Edit", json!({"file_path": cfg}));
    deny(
        "Bash",
        json!({"command": format!("sed -i '' s/hermes/claude/ {}", cfg.display())}),
    );
    deny(
        "Write",
        json!({"file_path": "~/.hermes/profiles/fleet-rev/config.yaml"}),
    );
    for i in shapes("Bash", json!({"command": format!("cat {}", cfg.display())})) {
        assert_eq!(run(i), "");
    }
}

#[test]
fn design_spawn_is_owner_only() {
    for cmd in [
        "fleet spawn x 1 design",
        "herdsman spawn x 1 design",
        "~/.local/bin/herdsman spawn x 1 design --dry-run",
    ] {
        let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
        for (var, role) in [
            ("FLEET_LANE", "impl"),
            ("HERDSMAN_LANE", "rev"),
            ("HERDSMAN_LANE", "impl"),
        ] {
            let out = hook_env(&[(var, Some("x:1:impl"))], role, input.clone());
            assert!(out.contains("owner-only"), "{var} {role} {cmd}: {out}");
        }
        assert_eq!(
            hook_env(&[], "impl", input.clone()),
            "",
            "the owner (no lane) may: {cmd}"
        );
    }
    assert_eq!(bash_as("impl", "herdsman spawn x 1 impl"), None);
    assert_eq!(bash_as("impl", "herdsman board x # design"), None);
}

/// #410: a conductor thread spawns design lanes; impl, rev, orch and other lanes still may not.
#[test]
fn a_conductor_may_spawn_design_lanes() {
    let run = |lane: &str, cmd: &str| {
        let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
        hook_in(Some(lane), "impl", input)
    };
    let spawn = "herdsman spawn synergy 2475 design";
    assert_eq!(run("synergy:-:conductor", spawn), "", "a conductor may");
    for lane in [
        "synergy:12:impl",
        "synergy:12:rev",
        "synergy:-:orch",
        "synergy:-:design",
        "conductor",
    ] {
        assert!(run(lane, spawn).contains("owner-only"), "{lane}");
    }
    for cmd in ["herdsman setup", "herdsman secret ls"] {
        assert!(run("synergy:-:conductor", cmd).contains("REFUSED OWNER_ONLY"), "{cmd}");
    }
}

/// #410: text a `thread reply|say` or `ask` quotes, or a heredoc's body, is not a command; a real one after it is.
#[test]
fn quoted_text_is_not_a_command() {
    let run = |lane: &str, cmd: &str| {
        let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
        hook_in(Some(lane), "impl", input)
    };
    let reply = r#"herdsman thread reply t5 "run \`herdsman spawn synergy 2475 design\`""#;
    let heredoc = "cat > f.md <<'EOF'\nrun `herdsman spawn synergy 2475 design`\nEOF\nwc -l f.md";
    for lane in ["synergy:-:conductor", "synergy:-:orch"] {
        assert_eq!(run(lane, reply), "", "{lane}");
    }
    for cmd in [
        heredoc,
        "cat <<-EOF > f.md\n\therdsman setup\n\tEOF",
        "herdsman thread say t1 'herdsman secret ls'",
        "herdsman ask x 1 \"spawn?\" --option \"herdsman spawn x 1 design\" --option='docker ps'",
        "herdsman thread reply t1 'run docker ps'",
    ] {
        assert_eq!(run("synergy:-:orch", cmd), "", "{cmd}");
    }
    for cmd in [
        "herdsman thread reply t1 x; herdsman spawn x 1 design",
        "herdsman thread reply t1 \"x\"; herdsman spawn x 1 design",
        "cat > f.md <<'EOF'\nx\nEOF\nherdsman spawn x 1 design",
        "sh -c 'herdsman spawn x 1 design'",
        "echo \"herdsman spawn x 1 design\" | herdsman spawn x 1 design",
        // Substitutions still run inside double quotes and unquoted-delimiter heredocs.
        "herdsman ask q \"$(herdsman spawn x 1 design)\"",
        "herdsman thread reply t1 \"see `herdsman spawn x 1 design`\"",
        "herdsman ask q --option \"a $(herdsman spawn x 1 \"design\")\"",
        "cat > f.md <<EOF\n$(herdsman spawn x 1 design)\nEOF",
        // A heredoc fed to a shell runs its body; a comment's quote does not swallow the next line.
        "bash <<'EOF'\nherdsman spawn x 1 design\nEOF",
        "cat <<'EOF' | sh\nherdsman spawn x 1 design\nEOF",
        "eval \"x\" $(cat <<'EOF'\nherdsman spawn x 1 design\nEOF\n)",
        "bash <<<\"herdsman spawn x 1 design\"",
        "herdsman ask q \"a\" # '\nherdsman spawn x 1 design",
        // `$'x\''` is the word `x'`: the command after it still runs; so does a split-quoted word.
        "herdsman ask q $'x\\'' ; herdsman spawn x 1 design",
        "herdsman thread reply t1 $'x\\'' ; herdsman spawn x 1 design",
        "herdsman thread reply t1 'x ; herdsman spawn x 1 design",
        "herdsman spawn x 1 'des'ign",
        // `eval` and `ssh` re-parse the quoted text, so it is a command there.
        "eval herdsman ask q '; herdsman spawn s 1 design'",
        "ssh h herdsman ask q '; herdsman spawn s 1 design'",
        "herdsman spawn x 1 \"des\"ign",
    ] {
        assert!(run("synergy:-:orch", cmd).contains("owner-only"), "{cmd}");
    }
    for cmd in [
        "bash <<'EOF'\ngh pr merge 5\nEOF",
        "sh -s <<EOF\ngit push origin staging\nEOF",
        "herdsman ask q $'x\\'' ; gh pr merge 5",
        "eval herdsman ask q '; gh pr merge 5'",
    ] {
        assert!(!run("synergy:12:impl", cmd).is_empty(), "{cmd}");
    }
    assert_eq!(run("synergy:-:orch", "ls # herdsman spawn x 1 design"), "");
    assert_eq!(
        run(
            "synergy:-:orch",
            "time FOO=1 herdsman ask q '; herdsman spawn s 1 design'"
        ),
        ""
    );
    assert!(run("synergy:-:orch", "herdsman thread reply t1 x && docker ps").contains("NO_CONTAINERS"));
}

/// #410: a conductor writes in its own thread folder, but not herdsman's files there, another thread's or the log.
#[test]
fn a_conductor_writes_its_own_thread_folder() {
    let write = |file: &std::path::Path, d: &std::path::Path| {
        let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Write",
            "tool_input": {"file_path": file, "content": "x"}});
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_herdsman"));
        let mut child = cmd
            .args(["hook", "--role", "conductor"])
            .env_remove("FLEET_LANE")
            .env("HERDSMAN_LANE", "synergy:-:conductor")
            .env("HERDSMAN_THREAD", "t6")
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
            .write_all(input.to_string().as_bytes())
            .unwrap();
        String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
    };
    let d = tmpdir("hook-own");
    std::fs::write(d.join("fleet.toml"), CONFIG).unwrap();
    let c = d.join("state/conductor");
    assert_eq!(write(&c.join("t6/mock.png"), &d), "", "its own folder");
    assert_eq!(write(&c.join("t6/ui/a.html"), &d), "", "a subfolder");
    for f in [
        c.join("t6/.thread.md"),
        c.join("t6/.brief.md"),
        c.join("t6/../t5/x"),
        c.join("t5/x"),
        d.join("state/events.jsonl"),
    ] {
        assert!(write(&f, &d).contains("is protected"), "{}", f.display());
    }
}

/// #198: the push guard spots an AI `Co-Authored-By:` (warn only) and finds the pushed repo.
#[test]
fn ai_coauthor_trailer_is_spotted() {
    use herdsman::hook::{ai_coauthor, push_dir};
    let log = "fix: x\n\nCo-Authored-By: Jane <j@x.io>\n    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>\n";
    assert_eq!(
        ai_coauthor(log),
        Some("Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>")
    );
    assert_eq!(ai_coauthor("fix: x\n\nCo-Authored-By: Jane <j@x.io>\n"), None);
    let push = json!({"tool_name": "Bash", "cwd": "/w", "tool_input": {"command": "cargo fmt && git -C /r push -u origin issue-1"}});
    assert_eq!(push_dir(&push).as_deref(), Some("/r"));
    let log_only = json!({"tool_name": "Bash", "cwd": "/w", "tool_input": {"command": "git log"}});
    assert_eq!(push_dir(&log_only), None);
}

const NO_CONTAINERS: &str = "herdsman: REFUSED NO_CONTAINERS: lanes never start containers (owner rule). Use the \
                             repo's non-container test path, or ask the conductor.";

/// #216: every default entry is denied as a command word: plain, `sh -c`, `env`, `sudo`, a full path, a pipeline,
/// `&&`, `$( )`; the same words as arguments, and other `supabase` subcommands, are allowed.
#[test]
fn container_commands_are_denied_as_command_words() {
    for c in [
        "docker ps",
        "docker-compose up -d",
        "podman run x",
        "nerdctl ps",
        "colima start",
        "orb start",
        "orbctl status",
        "limactl list",
        "supabase start",
        "supabase db start",
    ] {
        for cmd in [
            c.to_string(),
            format!("bash -c '{c}'"),
            format!("sh -c \"cd /x && {c}\""),
            format!("env FOO=1 {c}"),
            format!("sudo {c}"),
            format!("/usr/local/bin/{c}"),
            format!("cat x | {c} | tee y"),
            format!("cargo test; {c}"),
            format!("echo $({c})"),
            format!("sudo -u root {c}"),
            format!("sudo -n {c}"),
            format!("env -u X {c}"),
            format!("nice -n 5 {c}"),
            format!("timeout 5 {c}"),
            format!("if true; then {c}; fi"),
            format!("{{ {c}; }}"),
            format!("eval \"{c}\""),
            format!("\\{c}"),
        ] {
            assert_eq!(bash(&cmd).as_deref(), Some(NO_CONTAINERS), "{cmd}");
        }
    }
    for cmd in [
        "DOCKER_HOST=unix:///x.sock cargo test",
        "env DOCKER_HOST=tcp://h:2375 make",
        "open -a Docker",
        "open -a OrbStack",
        "open -a Docker.app",
        "export DOCKER_HOST=tcp://h:2375",
        "supabase --debug start",
    ] {
        assert_eq!(bash(cmd).as_deref(), Some(NO_CONTAINERS), "{cmd}");
    }
    for cmd in [
        "supabase migration new x",
        "grep docker file",
        "rg -n 'docker ps' src",
        "echo docker",
        "git commit -m 'drop docker'",
        "open -a Safari",
        "orbit run",
    ] {
        assert_eq!(bash(cmd), None, "{cmd} allowed");
    }
}

#[test]
fn container_commands_are_allowed_outside_a_lane() {
    for input in shapes("Bash", json!({"command": "docker ps"})) {
        assert_eq!(hook_in(None, "impl", input.clone()), "");
        assert_eq!(hook_in(Some(""), "impl", input), "");
    }
}

/// #216: `[fleet.<n>] deny_commands` replaces the list (empty = off), `deny_commands_add` extends it, `why` is the
/// message; each denial is logged `hook-denied`.
#[test]
fn deny_commands_override_per_fleet_and_log_denials() {
    let run = |fleet_toml: &str, cmd: &str| {
        let cfg = format!(
            "{CONFIG}\n[fleet.other]\nrepo = \"o/r\"\nlocal = \"/tmp/x\"\nintegration = \"staging\"\n{fleet_toml}"
        );
        let input = json!({"tool_name": "Bash", "cwd": "/", "tool_input": {"command": cmd}});
        let (out, d) = hook_cfg(&cfg, &[("HERDSMAN_LANE", Some("other:7:impl"))], "impl", input);
        let reason = serde_json::from_str::<Value>(&out).ok().map(|v| {
            v["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .to_string()
        });
        (
            reason,
            std::fs::read_to_string(d.join("state/events.jsonl")).unwrap_or_default(),
        )
    };
    let (r, log) = run("", "docker ps");
    assert_eq!(r.as_deref(), Some(NO_CONTAINERS), "the default applies");
    let ev: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!(
        (ev["type"].as_str(), ev["fleet"].as_str(), ev["issue"].as_u64()),
        (Some("hook-denied"), Some("other"), Some(7))
    );
    assert_eq!(ev["rule"].as_str(), Some(&NO_CONTAINERS["herdsman: ".len()..]));

    assert_eq!(
        run("deny_commands = []", "docker ps").0,
        None,
        "empty list turns it off"
    );
    let replace =
        r#"deny_commands = ["git push --force", { cmd = "npm publish", why = "REFUSED NO_PUBLISH: ask the owner" }]"#;
    assert_eq!(run(replace, "docker ps").0, None, "replaced");
    let r = run(replace, "cd x && git push --force origin issue-7").0.unwrap();
    assert!(
        r.contains("REFUSED NO_COMMAND: lanes never run `git push --force`"),
        "{r}"
    );
    assert_eq!(run(replace, "git push origin issue-7").0, None);
    assert_eq!(
        run(replace, "npm publish").0.as_deref(),
        Some("herdsman: REFUSED NO_PUBLISH: ask the owner")
    );
    let add = r#"deny_commands_add = ["terraform apply"]"#;
    assert!(run(add, "terraform apply -auto-approve").0.is_some());
    assert_eq!(run(add, "docker ps").0.as_deref(), Some(NO_CONTAINERS), "defaults kept");
    assert_eq!(run(add, "terraform plan").0, None);
}

/// #256: per-host rules from `HERDSMAN_HOST`: a cloud host allows docker and `supabase start` by default, a Mac keeps
/// the ban; a host's `allow_commands` and `deny_commands_add` override that.
#[test]
fn docker_is_allowed_on_a_cloud_host_and_denied_on_a_mac() {
    let run = |hosts: &str, host: &str, cmd: &str| {
        let cfg = format!("{CONFIG}\n[hosts.mini]\n\n[hosts.cloud1]\nmachine = \"cloud1\"\ncloud = \"boat\"\n{hosts}");
        let input = json!({"tool_name": "Bash", "cwd": "/", "tool_input": {"command": cmd}});
        let vars = [
            ("HERDSMAN_LANE", Some("sendmeter:7:impl")),
            ("HERDSMAN_HOST", Some(host)),
        ];
        hook_cfg(&cfg, &vars, "impl", input).0
    };
    let denied = |o: String| o.contains("NO_CONTAINERS") || o.contains("NO_COMMAND");
    for cmd in ["docker ps", "docker compose up -d db", "supabase start"] {
        assert!(run("", "cloud1", cmd).is_empty(), "{cmd} allowed on the cloud host");
        assert!(denied(run("", "mini", cmd)), "{cmd} denied on the Mac");
        assert!(denied(run("", "", cmd)), "{cmd} denied with no host");
    }
    assert!(denied(run("", "cloud1", "podman run x")), "the rest of the ban stays");
    assert!(
        denied(run("allow_commands = []", "cloud1", "docker ps")),
        "a cloud host can keep the ban"
    );
    let add = "deny_commands_add = [\"terraform apply\"]";
    assert!(denied(run(add, "cloud1", "terraform apply")));
    assert!(run(add, "cloud1", "docker ps").is_empty());
    let mac = "\n[hosts.mini2]\nallow_commands = [\"docker\"]";
    assert!(run(mac, "mini2", "docker ps").is_empty(), "a Mac may opt in");
}

/// #288: lanes run on the host too, so pairing is the owner's: `serve approve|unpair|devices --revoke` deny in a lane.
#[test]
fn serve_approve_is_owner_only() {
    for cmd in [
        "herdsman serve approve 123456",
        "cd /tmp && ~/.local/bin/herdsman serve unpair x",
        "herdsman serve devices --revoke x",
    ] {
        let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
        let out = hook_in(Some("x:1:impl"), "impl", input.clone());
        assert!(out.contains("REFUSED OWNER_ONLY"), "{cmd}: {out}");
        assert_eq!(hook_in(None, "impl", input), "", "the owner (no lane) may: {cmd}");
    }
    assert_eq!(bash("herdsman serve pending"), None);
    assert_eq!(bash("herdsman serve devices"), None);
}

/// #303: setup terminals (and the jobs `setup`) are the owner's: `herdsman setup …` denies in a lane.
#[test]
fn setup_is_owner_only() {
    for cmd in ["herdsman setup mini claude", "~/.local/bin/herdsman setup --no-load"] {
        let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
        let out = hook_in(Some("x:1:impl"), "impl", input.clone());
        assert!(out.contains("REFUSED OWNER_ONLY"), "{cmd}: {out}");
        assert_eq!(hook_in(None, "impl", input), "", "the owner (no lane) may: {cmd}");
    }
}

/// #387: a lane (the conductor too) proposes config and requests secrets, but never applies, lists or removes them.
#[test]
fn config_apply_and_secret_ls_rm_are_owner_only() {
    for cmd in [
        "herdsman config apply c1",
        "herdsman config discard c1",
        "herdsman secret ls",
        "herdsman secret rm all TOKEN",
    ] {
        let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
        let out = hook_in(Some("x:-:conductor"), "conductor", input.clone());
        assert!(out.contains("REFUSED OWNER_ONLY"), "{cmd}: {out}");
        assert_eq!(hook_in(None, "impl", input), "", "the owner (no lane) may: {cmd}");
    }
    assert_eq!(bash("herdsman config propose t1 /tmp/fleet.toml"), None);
    assert_eq!(bash("herdsman secret request t1 API_KEY --why x"), None);
}

/// #386: a conductor's allowed `herdsman`/`gh` command is logged for its thread's tool rows (`thread-cmd`); its
/// `thread reply`, other commands, denied ones and calls outside a thread are not.
#[test]
fn a_conductor_command_is_logged_for_its_thread() {
    let cmds = |vars: &[(&str, Option<&str>)], cmd: &str| {
        let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
        let (out, d) = hook_cfg(CONFIG, vars, "conductor", input);
        let log = std::fs::read_to_string(d.join("state/events.jsonl")).unwrap_or_default();
        let rows: Vec<Value> = log.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        (
            out,
            rows.into_iter()
                .filter(|e| e["type"] == "thread-cmd")
                .collect::<Vec<_>>(),
        )
    };
    let lane = [
        ("HERDSMAN_LANE", Some("sendmeter:-:conductor")),
        ("HERDSMAN_THREAD", Some("t1")),
    ];
    let cmd = "gh issue create -R acme/x -t \"Rate limits\" -F -";
    let (out, got) = cmds(&lane, cmd);
    assert!(out.trim().is_empty(), "allowed: {out}");
    assert_eq!(got.len(), 1);
    assert_eq!(
        (&got[0]["thread"], &got[0]["cmd"], &got[0]["fleet"]),
        (&json!("t1"), &json!(cmd), &json!("sendmeter"))
    );
    assert_eq!(cmds(&lane, "herdsman queue sendmeter add 52").1.len(), 1);
    assert!(
        cmds(&lane, "herdsman thread reply t1 \"done\"").1.is_empty(),
        "the reply is the message"
    );
    assert!(cmds(&lane, "ls -la").1.is_empty());
    assert!(
        cmds(&lane, "gh pr merge 5").1.is_empty(),
        "a denied call is not a tool row"
    );
    assert!(cmds(&lane[..1], "gh issue list").1.is_empty(), "no thread");
}

/// #425: a lane may `queue block` its own issue (to hold itself), never another issue.
#[test]
fn a_lane_may_queue_block_only_its_own_issue() {
    let input =
        |cmd: &str| json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
    for cmd in [
        "herdsman queue sendmeter block --on 23 24",
        "herdsman queue sendmeter block 24 --on 23",
    ] {
        assert_eq!(
            hook_in(Some("sendmeter:24:impl"), "impl", input(cmd)),
            "",
            "its own issue: {cmd}"
        );
    }
    for cmd in [
        "herdsman queue sendmeter block --on 23 25",
        "cd /tmp && herdsman queue sendmeter block 25 --on 24",
    ] {
        let out = hook_in(Some("sendmeter:24:impl"), "impl", input(cmd));
        assert!(out.contains("REFUSED OWN_ISSUE"), "{cmd}: {out}");
        assert_eq!(hook_in(None, "impl", input(cmd)), "", "the owner (no lane) may: {cmd}");
    }
}
