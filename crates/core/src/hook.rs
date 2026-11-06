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
    /// #410: a conductor lane (`HERDSMAN_LANE`'s role): it may spawn design lanes.
    pub conductor: bool,
    /// #410: the conductor's thread folder, writable but for herdsman's `.brief.md` and `.thread.md`.
    pub own: Option<String>,
    /// #536: the lane's `HERDSMAN_SCRATCH_TARGET` (`<build_root>/.scratch/<agent>`): Cargo builds only under its `build_root`.
    pub scratch: Option<String>,
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

/// #502: lanes share one user, so a pattern kill reaches every lane (BSD `pkill x -n` matched them all).
pub const NO_BROAD_KILL: &str = "REFUSED NO_BROAD_KILL: lanes stop only their own processes: start with \
                                 '& echo $! > f', stop with kill \"$(cat f)\"";

/// #502: `pkill` and `killall`, denied to every lane on top of its fleet's and host's `deny_commands`.
pub fn broad_kill() -> Vec<Deny> {
    ["pkill", "killall"]
        .map(|c| Deny::Why {
            cmd: c.into(),
            why: NO_BROAD_KILL.into(),
        })
        .into()
}

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

/// #508: a call before a tool runs (a pre-tool event, or Hermes' bare `args` without one), not a prompt or notification.
pub fn pre_call(input: &Value) -> bool {
    let ev = input["hook_event_name"].as_str().unwrap_or_default();
    ev.is_empty() || dialects().any(|d| d.pre == ev)
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
    if let Some(pp) = protected_path(file, p).filter(|_| !own_file(file, p)) {
        return Some(format!(
            "{file} is protected ({pp}); change it only through `herdsman` (e.g. `herdsman set`)"
        ));
    }
    (p.reviewer && !file.ends_with("/verdict.md") && file != "verdict.md")
        .then(|| format!("reviewers are read-only: only verdict.md may be written, not {file}"))
}

fn expand_home(s: &str) -> String {
    let home = crate::config::home().to_string_lossy().into_owned();
    s.replace("$HOME", &home).replace('~', &home)
}

fn protected_path<'a>(s: &str, p: &'a Policy) -> Option<&'a str> {
    let s = expand_home(s);
    p.paths.iter().map(String::as_str).find(|pp| s.contains(pp))
}

/// #410: `file` lies in the conductor's own thread folder and is not one herdsman writes there.
fn own_file(file: &str, p: &Policy) -> bool {
    use std::path::{Component, Path};
    let Some(own) = &p.own else { return false };
    let file = expand_home(file);
    let Ok(rel) = Path::new(&file).strip_prefix(own) else {
        return false;
    };
    let normal = rel.components().all(|c| matches!(c, Component::Normal(_)));
    normal && rel.as_os_str() != "" && ![".brief.md", ".thread.md"].iter().any(|f| rel == Path::new(f))
}

fn bash(cmd: &str, cwd: &str, p: &Policy, cur_branch: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let cmd = &unquote(cmd);
    if let Some(d) = denied(cmd, &p.deny) {
        return Some(d.reason());
    }
    // #502: `pgrep x | xargs kill` and `kill $(pgrep x)` kill by pattern too.
    let runs = |c: &str| denied(cmd, &[Deny::Cmd(c.into())]).is_some();
    if runs("pgrep") && runs("kill") {
        return Some(NO_BROAD_KILL.into());
    }
    let seg_breaks: &[char] = &[';', '|', '&', '\n', '(', ')', '`'];
    for seg in cmd.split(seg_breaks) {
        let toks: Vec<String> = seg
            .split_whitespace()
            .map(|t| t.replace(['"', '\''], ""))
            .filter(|t| !t.is_empty())
            .collect();
        let t: Vec<&str> = toks.iter().map(String::as_str).collect();
        let found = gh(&t)
            .or_else(|| git_push(&t, cwd, p, cur_branch))
            .or_else(|| reviewer_git(&t, p))
            .or_else(|| target_dir(&t, p));
        if let Some(r) = found.or_else(|| design_spawn(&t, p.conductor)) {
            return Some(r);
        }
        if let Some(pp) = protected_path(seg, p) {
            let writes = seg.contains('>')
                || t.iter().any(|w| WRITERS.contains(w))
                || (t.iter().any(|w| *w == "sed" || *w == "perl") && t.iter().any(|w| w.starts_with("-i")));
            // #504: a conductor's shell writes its own thread folder too, when every protected word is a file there.
            let own = || {
                t.iter()
                    .map(|w| w.rsplit('>').next().unwrap_or(w))
                    .filter(|w| protected_path(w, p).is_some())
                    // The shell expands braces, globs, variables and escapes after this check sees the word.
                    .all(|w| !expand_home(w).contains(['{', '}', '*', '?', '[', ']', '$', '~', '\\']) && own_file(w, p))
            };
            if writes && !own() {
                return Some(format!(
                    "writing under {pp} is blocked; use `herdsman set` / `herdsman profiles sync`"
                ));
            }
        }
    }
    None
}

/// #536: a `CARGO_TARGET_DIR=` or `--target-dir` outside the lane's `build_root` (a `/tmp` one escapes every budget).
fn target_dir(t: &[&str], p: &Policy) -> Option<String> {
    let s = p.scratch.as_deref()?;
    let root = std::path::Path::new(s).ancestors().nth(2)?.to_string_lossy();
    let dirs = t.iter().enumerate().filter_map(|(i, w)| {
        let v = w
            .strip_prefix("CARGO_TARGET_DIR=")
            .or_else(|| w.strip_prefix("--target-dir="));
        v.or_else(|| (*w == "--target-dir").then(|| t.get(i + 1).copied()).flatten())
    });
    let vars = [
        "$HERDSMAN_SCRATCH_TARGET",
        "${HERDSMAN_SCRATCH_TARGET}",
        "$CARGO_TARGET_DIR",
        "${CARGO_TARGET_DIR}",
    ];
    let ok = |d: &str| {
        let d = vars.iter().fold(expand_home(d), |d, v| d.replace(v, &root));
        !d.contains("..") && (d == root || d.starts_with(&format!("{root}/")))
    };
    let bad = dirs.filter(|d| !d.is_empty()).find(|d| !ok(d))?;
    Some(format!(
        "REFUSED SCRATCH_TARGET: {bad} is outside {root}; build in your scratch dir: \
         CARGO_TARGET_DIR=$HERDSMAN_SCRATCH_TARGET/target ({s}/target), never /tmp or $TMPDIR"
    ))
}

/// #410: `cmd` without the text the shell never runs as a command, so quoting one never matches it: heredoc
/// bodies fed to `cat` or `tee`, comments, and the quoted text of `herdsman thread reply|say` and `herdsman ask`
/// (the question, each `--option`).
fn unquote(cmd: &str) -> String {
    let (mut out, mut cur) = (String::new(), String::new());
    // Pending heredocs: delimiter, quoted, body dropped.
    let mut docs = Vec::<(String, bool, bool)>::new();
    // The current command's unquoted words so far.
    let mut words: Vec<String> = vec![];
    let mut cs = cmd.chars().peekable();
    while let Some(c) = cs.next() {
        if (c.is_whitespace() || "<;|&()`".contains(c)) && !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
        match c {
            '\'' | '"' => {
                // Only when `herdsman` is the command word: `eval` or `ssh h` before it re-parse the quoted text.
                let i = words.iter().position(|w| matches!(word(w), "herdsman" | "fleet"));
                let i = i.filter(|&i| {
                    words[..i]
                        .iter()
                        .all(|w| w.contains('=') || matches!(word(w), "command" | "time"))
                });
                let rest: Vec<&str> = i.map_or(vec![], |i| words[i + 1..].iter().map(String::as_str).collect());
                let text = matches!(rest[..], ["thread", "reply" | "say", ..] | ["ask", ..]);
                // `$'…'` escapes its quote (`$'x\''`); its text is kept, like an unterminated quote's.
                let ansi = c == '\'' && out.ends_with('$');
                out.push(c);
                let (mut span, mut live, mut closed) = (String::new(), false, false);
                while let Some(q) = cs.next() {
                    if q == c {
                        closed = true;
                        break;
                    }
                    let esc = ((c == '"' || ansi) && q == '\\').then(|| cs.next()).flatten();
                    // `$(…)` and backticks still run inside double quotes.
                    live |= c == '"' && esc.is_none() && (q == '`' || (q == '$' && cs.peek() == Some(&'(')));
                    span.extend(Some(q).into_iter().chain(esc));
                }
                if !text || live || ansi || !closed {
                    cur.push_str(&span);
                    out.push_str(&span);
                }
                out.push(c);
            }
            '\\' => {
                out.push(c);
                out.extend(cs.next());
            }
            '<' if cs.next_if_eq(&'<').is_some() => {
                out.push_str("<<");
                if cs.next_if_eq(&'<').is_some() {
                    out.push_str("< ");
                    continue;
                }
                while cs.next_if(|n| matches!(n, '-' | ' ' | '\t')).is_some() {}
                let (mut delim, mut quoted) = (String::new(), false);
                while let Some(n) = cs.next_if(|n| !n.is_whitespace() && !";|&()<>".contains(*n)) {
                    quoted |= matches!(n, '\'' | '"' | '\\');
                    delim.extend((!matches!(n, '\'' | '"' | '\\')).then_some(n));
                }
                // Only a text sink's body is never run: `bash <<EOF` and `eval $(cat <<EOF` run theirs.
                let top = out.matches('(').count() == out.matches(')').count() && out.matches('`').count() % 2 == 0;
                let sink = top && words.first().is_some_and(|w| matches!(word(w), "cat" | "tee"));
                docs.push((delim, quoted, sink));
            }
            '#' if out.is_empty() || out.ends_with(|l: char| l.is_whitespace() || ";|&()`".contains(l)) => {
                while cs.next_if(|n| *n != '\n').is_some() {}
            }
            '\n' | ';' | '|' | '&' | '(' | ')' | '`' => {
                out.push(c);
                words.clear();
                // `cat <<EOF | sh` and `cat <<EOF > >(sh)` run the body too.
                for d in docs.iter_mut().filter(|_| matches!(c, '|' | '(')) {
                    d.2 = false;
                }
                // A heredoc's body starts on the next line and ends at its delimiter's line.
                // An unquoted delimiter's body still runs `$(…)` and backticks, so those lines stay.
                for (d, quoted, drop) in docs.drain(..).filter(|_| c == '\n') {
                    let mut line = String::new();
                    for n in cs.by_ref() {
                        if n != '\n' {
                            line.push(n);
                        } else if line.trim() == d {
                            break;
                        } else {
                            if !drop || (!quoted && (line.contains('`') || line.contains("$("))) {
                                out.push_str(&line);
                                out.push('\n');
                            }
                            line.clear();
                        }
                    }
                }
            }
            c => {
                out.push(c);
                if !c.is_whitespace() {
                    cur.push(c);
                }
            }
        }
    }
    out
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

/// `herdsman spawn … design` (or pre-rename `fleet`; a conductor's too, #410), `setup`, `serve
/// approve|unpair|devices --revoke`, `config apply|discard`, `secret ls|rm` and `accounts import`: owner-only.
fn design_spawn(t: &[&str], conductor: bool) -> Option<String> {
    let i = t
        .iter()
        .position(|w| matches!(w.rsplit('/').next(), Some("herdsman" | "fleet")))?;
    let r = &t[i + 1..];
    if r.first() == Some(&"serve") && ["approve", "unpair", "--revoke"].iter().any(|w| r.contains(w)) {
        return Some("REFUSED OWNER_ONLY: only the owner pairs or unpairs devices, never a lane (#288)".into());
    }
    if matches!(
        (r.first(), r.get(1)),
        (Some(&"config"), Some(&("apply" | "discard")))
            | (Some(&"secret"), Some(&("ls" | "rm")))
            | (Some(&"accounts"), Some(&"import"))
    ) {
        return Some(
            "REFUSED OWNER_ONLY: only the owner applies config or lists and removes secrets, never a lane (#387)"
                .into(),
        );
    }
    // #425: a lane may hold its own issue (`queue <fleet> block --on N <issue>`), never another one.
    let lane = std::env::var("HERDSMAN_LANE").unwrap_or_default();
    let own = lane.split(':').nth(1).filter(|n| n.parse::<u64>().is_ok());
    let block = r.first() == Some(&"queue") && r.get(2) == Some(&"block") && own.is_some();
    let other = |(i, w): (usize, &&str)| r[i - 1] != "--on" && w.parse::<u64>().is_ok() && Some(*w) != own;
    if block && r.iter().enumerate().skip(3).any(other) {
        return Some("REFUSED OWN_ISSUE: a lane may `queue block` only its own issue (#425)".into());
    }
    // #419: `lesson` files an issue and spawns a lane: a conductor's verb (the owner's too); its digest is open.
    if !conductor && r.first() == Some(&"lesson") && r.get(1) != Some(&"digest") {
        return Some("REFUSED CONDUCTOR_ONLY: only a conductor or the owner runs `herdsman lesson` (#419)".into());
    }
    // #461: `timer install` (only that host verb) is a conductor's too; outside a lane it is the owner's.
    if !conductor && !lane.is_empty() && r.first() == Some(&"timer") && r.contains(&"install") {
        return Some(
            "REFUSED CONDUCTOR_ONLY: only a conductor or the owner runs `herdsman timer install` (#461)".into(),
        );
    }
    if r.first() == Some(&"setup") {
        return Some("REFUSED OWNER_ONLY: only the owner runs `herdsman setup`, never a lane (#303)".into());
    }
    (!conductor && t.get(i + 1) == Some(&"spawn") && t[i + 2..].contains(&"design")).then(|| {
        "design lanes are owner-only: the owner or a conductor thread runs `herdsman spawn … design`, never a lane"
            .into()
    })
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

/// #386: a shell call's command when it runs `herdsman` or `gh` (but `herdsman thread reply`, which is the reply
/// itself): a conductor's tool row in its thread.
pub fn fleet_cmd(input: &Value) -> Option<&str> {
    let tool = input["tool_name"].as_str().unwrap_or_default();
    let cmd = tool_input(input)["command"].as_str()?.trim();
    let first = cmd.split_whitespace().next().map(word)?;
    (dialects().any(|d| d.shell == tool) && ["herdsman", "gh"].contains(&first) && !cmd.contains("thread reply"))
        .then_some(cmd)
}

/// #413: a `gh issue create` that ran: its flow-back line (`filed #52: <title>`, number from the printed URL), if known.
pub fn issue_filed(input: &Value) -> Option<String> {
    let cmd = fleet_cmd(input).filter(|c| after(input) && c.contains("gh issue create"))?;
    let title = [" -t ", " --title ", " --title="].iter().find_map(|f| {
        let rest = &cmd[cmd.find(f)? + f.len()..];
        let q = rest.chars().next().filter(|c| matches!(c, '"' | '\''));
        let rest = q.map_or(rest, |q| &rest[q.len_utf8()..]);
        let end = rest.find(|c: char| q.map_or(c.is_whitespace(), |q| c == q));
        Some(rest[..end.unwrap_or(rest.len())].to_string()).filter(|t| !t.is_empty())
    });
    let res = input["tool_response"].to_string() + &input["result"].to_string();
    let n = res.split("/issues/").nth(1).and_then(|r| {
        let d: String = r.chars().take_while(char::is_ascii_digit).collect();
        d.parse().ok()
    });
    let head = n.map_or("filed".into(), |n: u64| format!("filed #{n}"));
    title.map(|t| format!("{head}: {t}")).or(n.map(|_| head))
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

/// #298: the state a hook call reports to shed: `done` at a turn's end, `blocked` while the harness waits on a person,
/// else `working`.
pub fn reported(input: &Value) -> Option<&'static str> {
    let ev = input["hook_event_name"].as_str().filter(|e| !e.is_empty())?;
    Some(match dialects().find(|d| d.stop == ev || d.blocked == ev) {
        Some(d) if d.stop == ev => "done",
        Some(_) => "blocked",
        None => "working",
    })
}

/// The deny answer in the dialect that sent `input`, else the first harness's (`Dialect::deny`).
pub fn deny_json(input: &Value, reason: &str) -> Value {
    let d = dialects().find(|d| (d.sent)(input)).or_else(|| dialects().next());
    d.map_or(Value::Null, |d| (d.deny)(&format!("herdsman: {reason}")))
}
