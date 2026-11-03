//! The tool guard of `herdsman help --agent`, in each harness's hook dialect (`Harness::hook_dialect`).
use crate::config::Deny;
use crate::harness::dialects;
use serde_json::Value;

pub struct Policy {
    pub branches: Vec<String>,
    /// Absolute path prefixes that agents may not modify.
    pub paths: Vec<String>,
    /// Reviewer lane: read-only (no edits but `verdict.md`, no commits, no pushes).
    pub reviewer: bool,
    /// #216: the fleet's `deny_commands`.
    pub deny: Vec<Deny>,
}

/// git subcommands that make commits or move refs; denied to reviewers.
const GIT_WRITES: &[&str] = &[
    "commit",
    "push",
    "merge",
    "rebase",
    "cherry-pick",
    "revert",
    "am",
    "reset",
];
const WRITERS: &[&str] = &["rm", "mv", "cp", "tee", "truncate", "chmod", "ln", "install", "dd"];

/// The call's tool input: the first of any dialect's input keys holding an object.
fn tool_input(input: &Value) -> &Value {
    let mut keys = dialects().flat_map(|d| d.input.iter());
    keys.find_map(|k| input.get(k).filter(|v| v.is_object()))
        .unwrap_or(&Value::Null)
}

/// After the fact (a post-tool or turn-end call): nothing left to guard.
pub fn after(input: &Value) -> bool {
    let ev = input["hook_event_name"].as_str().unwrap_or_default();
    dialects().any(|d| d.post == ev || d.stop == ev)
}

/// Some(reason) denies a shell (`Bash`, `terminal`) or file-writing call; `cur_branch` resolves HEAD.
pub fn decide(input: &Value, p: &Policy, cur_branch: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let tool = input["tool_name"].as_str().unwrap_or_default();
    let ti = tool_input(input);
    if dialects().any(|d| d.shell == tool) {
        let cmd = ti["command"].as_str().unwrap_or_default();
        let cwd = ti["workdir"].as_str().or(input["cwd"].as_str()).unwrap_or(".");
        return bash(cmd, cwd, p, cur_branch);
    }
    if !dialects().any(|d| d.edits.contains(&tool)) {
        return None;
    }
    // A V4A `patch` names its files inside the patch text.
    let file = ["file_path", "notebook_path", "path", "patch"]
        .iter()
        .find_map(|k| ti[k].as_str())
        .unwrap_or_default();
    if let Some(pp) = protected_path(file, p) {
        return Some(format!(
            "{file} is protected ({pp}); change it only through `herdsman` (e.g. `herdsman set`)"
        ));
    }
    (p.reviewer && !file.ends_with("/verdict.md") && file != "verdict.md")
        .then(|| format!("reviewers are read-only: only verdict.md may be written, not {file}"))
}

fn protected_path<'a>(s: &str, p: &'a Policy) -> Option<&'a str> {
    let home = crate::config::home().to_string_lossy().into_owned();
    let s = s.replace("$HOME", &home).replace('~', &home);
    p.paths.iter().map(String::as_str).find(|pp| s.contains(pp))
}

fn bash(cmd: &str, cwd: &str, p: &Policy, cur_branch: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if let Some(d) = denied(cmd, &p.deny) {
        return Some(d.reason());
    }
    let seg_breaks: &[char] = &[';', '|', '&', '\n', '(', ')', '`'];
    for seg in cmd.split(seg_breaks) {
        let toks: Vec<String> = seg
            .split_whitespace()
            .map(|t| t.trim_matches(['"', '\'']).to_string())
            .filter(|t| !t.is_empty())
            .collect();
        let t: Vec<&str> = toks.iter().map(String::as_str).collect();
        let found = gh(&t)
            .or_else(|| git_push(&t, cwd, p, cur_branch))
            .or_else(|| reviewer_git(&t, p));
        if let Some(r) = found.or_else(|| design_spawn(&t)) {
            return Some(r);
        }
        if let Some(pp) = protected_path(seg, p) {
            let writes = seg.contains('>')
                || t.iter().any(|w| WRITERS.contains(w))
                || (t.iter().any(|w| *w == "sed" || *w == "perl") && t.iter().any(|w| w.starts_with("-i")));
            if writes {
                return Some(format!(
                    "writing under {pp} is blocked; use `herdsman set` / `herdsman profiles sync`"
                ));
            }
        }
    }
    None
}

/// Words that run the next word as a command (`sh -c`, `sudo`, `env`, `then`, …); `-flags`, `VAR=…` and numbers
/// after them are skipped.
const WRAPPERS: &[&str] = &[
    "env", "sudo", "exec", "command", "nohup", "time", "xargs", "sh", "bash", "zsh", "nice", "timeout", "eval",
    "export", "if", "then", "else", "do", "while", "until", "{", "!",
];
/// Wrapper flags that may take a value (`sudo -u root`, `env -u X`, `xargs -n 1`); `sudo -n docker` still denies.
const VALUE_FLAGS: &[&str] = &["-u", "-g", "-C", "-n", "-U", "-D", "-S", "-P", "-r", "-T"];

/// #216: the first `deny` entry `cmd` runs as a command word, in any segment (pipe, `&&`, `;`, `$( )`), past
/// wrappers and `VAR=…`, by full path too. An entry ending in `=` (`DOCKER_HOST=`) denies that assignment.
/// Best-effort lexical matching: `D=docker; $D ps` or `ssh h docker ps` pass.
pub fn denied<'a>(cmd: &str, deny: &'a [Deny]) -> Option<&'a Deny> {
    cmd.split([';', '|', '&', '\n', '(', ')', '`']).find_map(|seg| {
        let t: Vec<&str> = seg.split_whitespace().map(|w| w.trim_matches(['"', '\''])).collect();
        let mut i = 0;
        while let Some(w) = t.get(i).filter(|w| {
            w.contains('=')
                || w.starts_with(['-', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9'])
                || WRAPPERS.contains(w)
        }) {
            if let Some(d) = deny.iter().find(|d| d.cmd().ends_with('=') && w.starts_with(d.cmd())) {
                return Some(d);
            }
            let next = t.get(i + 1).map(|n| word(n));
            let value = VALUE_FLAGS.contains(w) && !deny.iter().any(|d| d.cmd().split_whitespace().next() == next);
            i += if value { 2 } else { 1 };
        }
        let (first, rest) = t.get(i..)?.split_first()?;
        let first = Some(word(first));
        deny.iter().find(|d| {
            let mut e = d.cmd().split_whitespace();
            // Subcommand words match in order, past flags (`supabase --debug start`); `Docker.app` is `Docker`.
            let mut r = rest.iter().map(|w| w.trim_end_matches(".app"));
            e.next() == first && e.all(|w| r.find(|x| *x == w || !x.starts_with('-')) == Some(w))
        })
    })
}

/// A command word's name: `\docker` and `/usr/local/bin/docker` are `docker`.
fn word(w: &str) -> &str {
    w.trim_start_matches('\\').rsplit('/').next().unwrap_or(w)
}

fn gh(t: &[&str]) -> Option<String> {
    let i = t.iter().position(|w| *w == "gh")?;
    let rest = &t[i + 1..];
    let merge = rest.windows(2).any(|w| w == ["pr", "merge"])
        || (rest.first() == Some(&"api") && rest.iter().any(|w| w.contains("/merge")));
    if merge {
        return Some(
            "merging is gated: run `herdsman merge <fleet> <issue>` (needs exact-head PASS + green CI)".into(),
        );
    }
    let api = rest.first() == Some(&"api");
    let comment = rest
        .windows(2)
        .any(|w| w == ["issue", "comment"] || w == ["pr", "comment"] || w == ["pr", "review"])
        || (api && rest.iter().any(|w| w.contains("/comments") || w.contains("addComment")) && api_writes(rest));
    comment.then(|| {
        "lanes never comment on GitHub: report in your pane; fleet logs routing, verdicts and the merge".into()
    })
}

/// `herdsman spawn … design` (or pre-rename `fleet`), `setup` and `serve approve|unpair|devices --revoke`: owner-only.
fn design_spawn(t: &[&str]) -> Option<String> {
    let i = t
        .iter()
        .position(|w| matches!(w.rsplit('/').next(), Some("herdsman" | "fleet")))?;
    let r = &t[i + 1..];
    if r.first() == Some(&"serve") && ["approve", "unpair", "--revoke"].iter().any(|w| r.contains(w)) {
        return Some("REFUSED OWNER_ONLY: only the owner pairs or unpairs devices, never a lane (#288)".into());
    }
    if r.first() == Some(&"setup") {
        return Some("REFUSED OWNER_ONLY: only the owner runs `herdsman setup`, never a lane (#303)".into());
    }
    (t.get(i + 1) == Some(&"spawn") && t[i + 2..].contains(&"design"))
        .then(|| "design lanes are owner-only: only the owner runs `herdsman spawn … design`, never a lane".into())
}

/// `gh api` sends a write: a non-GET method, or fields/input (which default to POST).
fn api_writes(rest: &[&str]) -> bool {
    rest.iter().enumerate().any(|(i, w)| {
        let method = match *w {
            "-X" | "--method" => rest.get(i + 1).copied(),
            _ => w
                .strip_prefix("--method=")
                .or(w.strip_prefix("-X").filter(|m| !m.is_empty())),
        };
        method.is_some_and(|m| !m.eq_ignore_ascii_case("GET"))
            || w.starts_with("-f")
            || w.starts_with("-F")
            || ["--field", "--raw-field", "--input"]
                .iter()
                .any(|f| w.split('=').next() == Some(f))
    })
}

/// Index of the git subcommand and the repo dir, past global options (`-C <dir>`, `-c <k=v>`, `--git-dir=…`).
fn git_sub(t: &[&str], cwd: &str) -> Option<(usize, String)> {
    let g = t.iter().position(|w| *w == "git")?;
    let mut dir = cwd.to_string();
    let mut i = g + 1;
    while i < t.len() && t[i].starts_with('-') {
        if t[i] == "-C" && i + 1 < t.len() {
            dir = t[i + 1].to_string();
        }
        i += if (t[i] == "-C" || t[i] == "-c") && i + 1 < t.len() {
            2
        } else {
            1
        };
    }
    (i < t.len()).then_some((i, dir))
}

fn reviewer_git(t: &[&str], p: &Policy) -> Option<String> {
    let (i, _) = git_sub(t, ".").filter(|_| p.reviewer)?;
    GIT_WRITES.contains(&t[i]).then(|| {
        format!(
            "reviewers are read-only: no `git {}`; write verdict.md and report in your pane",
            t[i]
        )
    })
}

fn git_push(t: &[&str], cwd: &str, p: &Policy, cur_branch: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let (i, dir) = git_sub(t, cwd)?;
    if t[i] != "push" {
        return None;
    }
    let deny = |b: &str| {
        Some(format!(
            "pushing to `{b}` is blocked; integration branches change only via `herdsman merge`"
        ))
    };
    let mut pos = vec![];
    let mut j = i + 1;
    while j < t.len() {
        let w = t[j];
        if w == "--all" || w == "--mirror" {
            return deny(w);
        }
        if ["-o", "--push-option", "--repo", "--receive-pack", "--exec"].contains(&w) {
            j += 2;
            continue;
        }
        if !w.starts_with('-') {
            pos.push(w);
        }
        j += 1;
    }
    let protected = |b: &str| {
        let b = b.trim_start_matches('+');
        let dst = b.rsplit(':').next().unwrap_or(b);
        let dst = dst.trim_start_matches("refs/heads/");
        p.branches.iter().any(|x| x == dst).then(|| dst.to_string())
    };
    if pos.len() <= 1 {
        let head = cur_branch(&dir)?;
        return protected(&head).and_then(|b| deny(&b));
    }
    for r in &pos[1..] {
        let name = if *r == "HEAD" {
            cur_branch(&dir).unwrap_or_default()
        } else {
            r.to_string()
        };
        if let Some(b) = protected(&name) {
            return deny(&b);
        }
    }
    None
}

/// #198: the repo dir of a `git push` in a Bash/terminal call (for the co-author warning).
pub fn push_dir(input: &Value) -> Option<String> {
    let ti = tool_input(input);
    let cwd = ti["workdir"].as_str().or(input["cwd"].as_str()).unwrap_or(".");
    ti["command"]
        .as_str()?
        .split([';', '|', '&', '\n', '(', ')', '`'])
        .find_map(|seg| {
            let t: Vec<&str> = seg.split_whitespace().map(|w| w.trim_matches(['"', '\''])).collect();
            git_sub(&t, cwd).filter(|(i, _)| t[*i] == "push").map(|(_, d)| d)
        })
}

/// #198: the first `Co-Authored-By:` line in a commit log that names an AI.
pub fn ai_coauthor(log: &str) -> Option<&str> {
    const AI: &[&str] = &["claude", "anthropic", "openai", "codex", "gpt", "copilot", "gemini"];
    log.lines().map(str::trim).find(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("co-authored-by:") && AI.iter().any(|a| l.contains(a))
    })
}

/// #304: a post-tool or stop hook's kick reason: `push` for a `git push` that went through (no `rejected`, `fatal:`
/// or `error:` in its output), `stop` at a turn's end; `None` for anything else (a pre-tool guard call).
pub fn kick_why(input: &Value) -> Option<&'static str> {
    let ev = input["hook_event_name"].as_str()?;
    match dialects().find(|d| d.post == ev || d.stop == ev)? {
        d if d.stop == ev => Some("stop"),
        _ if push_dir(input).is_some() => {
            let res = input["tool_response"].to_string() + &input["result"].to_string();
            let failed = [
                "rejected",
                "fatal:",
                "error:",
                "\"interrupted\":true",
                "\"is_error\":true",
            ];
            (!failed.iter().any(|f| res.contains(f))).then_some("push")
        }
        _ => None,
    }
}

/// The deny answer in the dialect that sent `input`, else the first harness's (`Dialect::deny`).
pub fn deny_json(input: &Value, reason: &str) -> Value {
    let d = dialects().find(|d| (d.sent)(input)).or_else(|| dialects().next());
    d.map_or(Value::Null, |d| (d.deny)(&format!("herdsman: {reason}")))
}
