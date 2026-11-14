//! The tool guard of `drover help --agent`, in each harness's hook dialect (`Harness::hook_dialect`).
use crate::config::{Config, Deny};
use crate::providers::dialects;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct Policy {
    pub branches: Vec<String>,
    /// Absolute path prefixes that agents may not modify.
    pub paths: Vec<String>,
    /// Reviewer lane: read-only (no edits but `verdict.md`, no commits, no pushes).
    pub reviewer: bool,
    /// #216: the fleet's `deny_commands`.
    pub deny: Vec<Deny>,
    /// #410: an orchestrator lane (`DROVER_LANE`'s role): it may spawn design lanes.
    pub orchestrator: bool,
    /// #410: the orchestrator's thread folder, writable but for drover's `.brief.md` and `.thread.md`.
    pub own: Option<String>,
    /// #538: the capability groups an orchestrator has on (`[orchestrator] allow`).
    pub groups: Vec<String>,
    /// #536: the lane's `DROVER_SCRATCH_TARGET` (`<build_root>/.scratch/<agent>`): Cargo builds only under its `build_root`.
    pub scratch: Option<String>,
    /// #668: an impl lane: `ci/cap.toml` is not its to write (but for the orchestrator-filed raise).
    pub cap: bool,
    /// #868: a design lane: its edits land only under its issue's design paths.
    pub design: Option<Design>,
}

/// #868: a design lane's worktree `root` and the folders under it (`crate::design::Recipe`) it may write.
pub struct Design {
    pub root: String,
    pub allow: Vec<String>,
}

/// #592: a lane's hook rules as the main host resolves them from its fleet.toml. A remote lane's are frozen at spawn in
/// its rules file (`DROVER_RULES`); its host's own fleet.toml is never read. No notifier, token or env value.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Rules {
    /// main, master, staging and every fleet's integration branch.
    pub branches: Vec<String>,
    /// #216/#256: the fleet's `deny_commands` under the lane host's rules, plus #502's broad-kill ban.
    pub deny: Vec<Deny>,
    pub groups: Vec<String>,
    pub reviewer: bool,
    /// #511: an impl-family lane (not lesson): its unreported turn end goes on once.
    pub builds: bool,
}

impl Rules {
    pub fn resolve(cfg: &Config, fleet: &str, role: &str, host: &str) -> Rules {
        let f = cfg.fleet.get(fleet);
        let d = f.map_or_else(|| cfg.limits.deny_commands.clone(), |f| f.deny(&cfg.limits));
        let mut deny = match cfg.hosts.get(host) {
            Some(h) => h.deny(d),
            None => d,
        };
        // #502: no fleet or host drops the broad-kill ban.
        deny.extend(broad_kill());
        let mut branches: Vec<String> = ["main", "master", "staging"].map(String::from).to_vec();
        branches.extend(cfg.fleet.values().map(|f| f.integration.clone()));
        let family = f.map(|f| f.family(role));
        Rules {
            branches,
            deny,
            groups: cfg.orchestrator.groups().into_iter().map(String::from).collect(),
            reviewer: family == Some("rev"),
            builds: role != "lesson" && family == Some("impl"),
        }
    }
}

/// #592: lane spawn `sp` froze rules (a remote lane's) that fleet.toml now resolves otherwise: a respawn takes them.
pub fn rules_stale(cfg: &Config, fleet: &str, sp: &crate::events::Event) -> bool {
    let (role, host) = (sp.str("role").unwrap_or_default(), sp.str("host").unwrap_or_default());
    let now = serde_json::to_value(Rules::resolve(cfg, fleet, role, host)).ok();
    sp.data.get("rules").is_some_and(|r| now.as_ref() != Some(r))
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
const WRITERS: &[&str] = &[
    "rm", "mv", "cp", "tee", "truncate", "chmod", "ln", "install", "dd", "sips",
];

/// #668: the repo's line cap file; one orchestrator-filed issue raises it for the whole queue.
pub const CAP_FILE: &str = "ci/cap.toml";
pub const NO_CAP_EDIT: &str = "REFUSED CAP_FILE: a lane never edits ci/cap.toml; when your PR would cross the cap, \
                               run `drover cap <fleet>` and ask with its view (#668)";

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

/// #508: a call before a tool runs (a pre-tool event, or a bare `args` call without one), not a prompt or notification.
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
    let file = ["file_path", "notebook_path", "path", "patch", "command"]
        .iter()
        .find_map(|k| ti[k].as_str())
        .unwrap_or_default();
    if p.cap && file.contains(CAP_FILE) {
        return Some(NO_CAP_EDIT.into());
    }
    if let Some(pp) = protected_path(file, p).filter(|_| !own_file(file, p)) {
        return Some(format!(
            "{file} is protected ({pp}); change it only through `drover` (e.g. `drover set`)"
        ));
    }
    if let Some(r) = design_write(file, p) {
        return Some(r);
    }
    (p.reviewer && !file.ends_with("/verdict.md") && file != "verdict.md")
        .then(|| format!("reviewers are read-only: only verdict.md may be written, not {file}"))
}

fn design_write(file: &str, p: &Policy) -> Option<String> {
    let d = p.design.as_ref()?;
    let patched: Vec<&str> = (file.lines())
        .filter_map(|l| l.strip_prefix("*** "))
        .filter_map(|l| l.split_once(" File: ").map(|(_, f)| f).or(l.strip_prefix("Move to: ")))
        .map(str::trim)
        .collect();
    let files = if file.starts_with("*** ") { patched } else { vec![file] };
    let ok = |f: &&str| {
        let f = resolve(f, &d.root);
        let rel = Path::new(&f).strip_prefix(&d.root);
        rel.is_ok_and(|rel| d.allow.iter().any(|a| rel.starts_with(a)))
    };
    let bad = files
        .iter()
        .find(|f| !ok(f))
        .copied()
        .or(files.is_empty().then_some(file))?;
    let allow = d.allow.iter().map(|a| format!("{a}/")).collect::<Vec<_>>().join(", ");
    Some(format!(
        "REFUSED DESIGN_PATH: a design lane writes only under {allow}, not {bad} (#868)"
    ))
}

fn expand_home(s: &str) -> String {
    let home = crate::config::home().to_string_lossy().into_owned();
    s.replace("$HOME", &home).replace('~', &home)
}

fn protected_path<'a>(s: &str, p: &'a Policy) -> Option<&'a str> {
    let s = expand_home(s);
    p.paths.iter().map(String::as_str).find(|pp| s.contains(pp))
}

/// #410: `file` lies in the conductor's own thread folder and is not one drover writes there.
fn own_file(file: &str, p: &Policy) -> bool {
    use std::path::{Component, Path};
    let Some(own) = &p.own else { return false };
    let file = expand_home(file);
    let Ok(rel) = Path::new(&file).strip_prefix(own) else {
        return false;
    };
    let normal = rel.components().all(|c| matches!(c, Component::Normal(_)));
    // A symlink there could point anywhere: writing through it is not writing into the folder.
    let linked = Path::new(&file)
        .ancestors()
        .take_while(|a| *a != Path::new(own))
        .any(|a| a.symlink_metadata().is_ok_and(|m| m.is_symlink()));
    normal && !linked && rel.as_os_str() != "" && ![".brief.md", ".thread.md"].iter().any(|f| rel == Path::new(f))
}

/// #499: `w` as an absolute path, resolved against `cwd` (`..` and `.` folded), past a redirection's `2>>`.
fn resolve(w: &str, cwd: &str) -> String {
    use std::path::{Component, PathBuf};
    let w = expand_home(w.trim_start_matches(|c: char| c.is_ascii_digit() || c == '>'));
    let mut out = PathBuf::new();
    for c in std::path::Path::new(cwd).join(w).components() {
        match c {
            Component::ParentDir => drop(out.pop()),
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out.to_string_lossy().into_owned()
}

/// #677: only drover writes why notes: a lane's `git notes` write, or a push naming `refs/notes`, is refused.
fn notes(t: &[&str]) -> Option<String> {
    let rest = &t[t.iter().position(|w| *w == "git")? + 1..];
    let writes = ["add", "append", "edit", "copy", "merge", "remove", "prune"];
    let write = (rest.iter().position(|w| *w == "notes")).is_some_and(|i| rest[i..].iter().any(|w| writes.contains(w)));
    let push = rest.contains(&"push") && rest.iter().any(|w| w.contains("refs/notes"));
    (write || push).then(|| {
        "REFUSED NOTES_DROVER_ONLY: only drover writes or pushes why notes: write why.md, then \
        `drover turn done` (#677)"
            .into()
    })
}

fn bash(cmd: &str, cwd: &str, p: &Policy, cur_branch: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    // #537: a quoted argument's text (`gh issue create --body "…docker…"`) is no command word.
    if let Some(d) = denied(&dups(&unquote(cmd, true)), &p.deny) {
        return Some(d.reason());
    }
    let cmd = &dups(&unquote(cmd, false));
    let mut cwd = cwd.to_string();
    // #538: the hard nos no run card may run, from a conductor's own shell either.
    if let Some(w) = crate::card::forbidden(cmd).filter(|_| p.orchestrator) {
        return Some(format!(
            "REFUSED CARD_FORBIDDEN: this command {w}; an orchestrator never runs it (#538)"
        ));
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
        // #499: a `cd` moves the later segments' cwd, so their relative paths resolve where they run.
        // Its options (`-P`, `--`) are skipped; `pushd` moves it the same way.
        if let ["cd" | "pushd", ref args @ ..] = t[..] {
            if let Some(dir) = args.iter().find(|a| !a.starts_with('-')) {
                cwd = resolve(dir, &cwd);
            }
        }
        let found = gh(&t)
            .or_else(|| git_push(&t, &cwd, p, cur_branch))
            .or_else(|| reviewer_git(&t, p))
            .or_else(|| notes(&t))
            .or_else(|| target_dir(&t, p));
        if let Some(r) = found.or_else(|| design_spawn(&t, p)) {
            return Some(r);
        }
        // #570: a redirection's target; `/dev/null`, `/dev/stdout` and `/dev/stderr` write nothing.
        let targets: Vec<String> = redirects(seg)
            .into_iter()
            .filter(|w| !["/dev/null", "/dev/stdout", "/dev/stderr"].contains(&w.as_str()))
            .collect();
        let bare = unquote(seg, true).replace(['"', '\''], " ");
        let w: Vec<&str> = bare.split_whitespace().collect();
        let drover = (w
            .iter()
            .find(|w| !w.contains('=') && !matches!(word(w), "command" | "time")))
        .is_some_and(|w| matches!(word(w), "drover" | "herdsman" | "fleet"));
        let writer = !drover
            && (w.iter().any(|w| WRITERS.contains(w))
                || (w.iter().any(|w| *w == "sed" || *w == "perl") && w.iter().any(|w| w.starts_with("-i"))));
        // #668: the cap file as a writer's argument or a redirection's target.
        let cap = |w: &String| w.ends_with(CAP_FILE);
        if p.cap && (targets.iter().any(cap) || writer && toks.iter().any(cap)) {
            return Some(NO_CAP_EDIT.into());
        }
        // #499: each path in a word (`--opt=`, `of=`, `a>b` split off; an option's `-t<dir>` as each suffix)
        // resolved against the cwd; a conductor's write lands only in its own folder. The segment's own text,
        // its own-folder words left out, is still checked. #570: a relative word only as a writer's argument
        // or a redirection's target.
        let paths = |w: &str| -> Vec<String> {
            (w.split(['=', '>']))
                .flat_map(|w| match w.strip_prefix('-') {
                    Some(o) => o.char_indices().map(|(i, _)| &o[i..]).collect(),
                    None => vec![w],
                })
                .filter(|w| !w.is_empty() && !w.starts_with('-'))
                .filter(|w| {
                    writer || w.starts_with(['/', '~']) || w.starts_with("$HOME") || targets.iter().any(|t| t == w)
                })
                .map(|w| resolve(w, &cwd))
                .collect()
        };
        // A link's target resolves against the link's folder, not the cwd: `ln` gets no own-folder pass.
        // #504: the shell expands braces, globs, variables and escapes after this check sees the word.
        let copy = matches!(t.first(), Some(&("cp" | "mv" | "install" | "rsync" | "ditto")));
        let into = t.last().filter(|_| copy).map(|w| resolve(w, &cwd));
        let folder =
            |f: &str| into.as_deref() == Some(f) && p.own.as_deref().is_some_and(|o| Path::new(o) == Path::new(f));
        let own = |f: &str| {
            !t.contains(&"ln")
                && !f.contains(['{', '}', '*', '?', '[', ']', '$', '~', '\\'])
                && (own_file(f, p) || folder(f))
        };
        let hit = (t.iter().flat_map(|w| paths(w)))
            .find_map(|f| p.paths.iter().find(|pp| f.starts_with(pp.as_str()) && !own(&f)))
            .map(String::as_str);
        let rest: Vec<&str> = (t.iter().copied())
            .filter(|w| !paths(w).iter().any(|f| own(f)))
            .collect();
        let seg_hit = protected_path(&rest.join(" "), p);
        if let Some(pp) = hit.or(seg_hit) {
            if writer || !targets.is_empty() {
                return Some(format!(
                    "writing under {pp} is blocked; use `drover set` / `drover profiles sync`"
                ));
            }
        }
    }
    None
}

fn redirects(seg: &str) -> Vec<String> {
    let mut quote = None;
    let at = seg.char_indices().filter(|&(_, c)| {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), c) if q == c => quote = None,
            _ => {}
        }
        quote.is_none() && c == '>'
    });
    (at.filter_map(|(i, _)| seg[i + 1..].trim_start_matches('>').split_whitespace().next()))
        .map(|w| w.replace(['"', '\''], ""))
        .collect()
}

/// #570: `cmd` without file-descriptor duplications (`2>&1`, `>&2`, `<&3`, `>&-`) and with `&>` as `>`, so
/// neither splits a segment at its `&` nor reads as a write to a file.
fn dups(cmd: &str) -> String {
    let fd = |d: &char| d.is_ascii_digit() || *d == '-';
    let (mut out, mut it) = (String::new(), cmd.chars().peekable());
    while let Some(c) = it.next() {
        match (c, it.peek()) {
            ('>' | '<', Some('&')) => {
                it.next();
                if it.peek().is_some_and(fd) {
                    while it.next_if(fd).is_some() {}
                    let n = out.trim_end_matches(|d: char| d.is_ascii_digit());
                    if n.is_empty() || n.ends_with(char::is_whitespace) {
                        out.truncate(n.len());
                    }
                } else {
                    out.push(c);
                }
            }
            ('&', Some('>')) => {}
            _ => out.push(c),
        }
    }
    out
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
        "$DROVER_SCRATCH_TARGET",
        "${DROVER_SCRATCH_TARGET}",
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
         CARGO_TARGET_DIR=$DROVER_SCRATCH_TARGET/target ({s}/target), never /tmp or $TMPDIR"
    ))
}

/// #410: `cmd` without the text the shell never runs as a command, so quoting one never matches it: heredoc
/// bodies fed to `cat` or `tee`, comments, and the quoted text of `drover thread reply|say` and `drover ask`
/// (the question, each `--option`). #537: with `args`, also any quoted argument of a command that runs no text of its
/// own: none of its words a wrapper (`sh`, `eval`, …), `ssh`, `watch`, `parallel` or `-c` (`su -c`, `script -c`).
fn unquote(cmd: &str, args: bool) -> String {
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
                // Only when `drover` is the command word: `eval` or `ssh h` before it re-parse the quoted text.
                let i = words
                    .iter()
                    .position(|w| matches!(word(w), "drover" | "herdsman" | "fleet" | "gh"));
                let i = i.filter(|&i| {
                    words[..i]
                        .iter()
                        .all(|w| w.contains('=') || matches!(word(w), "command" | "time"))
                });
                let rest: Vec<&str> = i.map_or(vec![], |i| words[i + 1..].iter().map(String::as_str).collect());
                let text = matches!(rest[..], ["thread", "reply" | "say", ..] | ["ask", ..]);
                let runs =
                    |w: &String| WRAPPERS.contains(&word(w)) || ["ssh", "watch", "parallel", "-c"].contains(&word(w));
                let data = args && words.iter().any(|w| !w.contains('=')) && !words.iter().any(runs);
                let text = text || data;
                // `$'…'` escapes its quote (`$'x\''`); its text is kept, like an unterminated quote's.
                let ansi = c == '\'' && out.ends_with('$');
                let target = out.trim_end().ends_with(['>', '<']);
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
                let msg =
                    i.is_some() && !target && span.contains(|q: char| q.is_whitespace() || ";|&()`<>".contains(q));
                if !(text || msg) || live || ansi || !closed {
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
        return Some("merging is gated: run `drover merge <fleet> <issue>` (needs exact-head PASS + green CI)".into());
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

/// #538: the capability group of a `drover` verb (its words after `drover`), if it is in one.
fn group(r: &[&str]) -> Option<&'static str> {
    let has = |w: &str| r.iter().any(|x| x.split('=').next() == Some(w));
    Some(match (*r.first()?, r.get(1).copied()) {
        ("spawn", _) if r[1..].iter().any(|w| crate::events::STAGES.contains(w)) => "design",
        ("clean", _) if has("--yes") || has("--orphans") || has("--lane") => "clean",
        ("hosts" | "host", Some("on" | "off" | "refresh")) => "hosts",
        ("reclaim", _) => "reclaim",
        ("runners", Some(_)) => "runners",
        ("pause" | "resume", _) => "pause",
        ("set", _) => "set",
        _ => return None,
    })
}

/// #538: the capability group of the `drover` verb `cmd` runs, if any (its thread row names it).
pub fn group_of(cmd: &str) -> Option<&'static str> {
    cmd.split([';', '|', '&', '\n', '(', ')', '`']).find_map(|seg| {
        let t: Vec<&str> = seg.split_whitespace().map(|w| w.trim_matches(['"', '\''])).collect();
        let i = t
            .iter()
            .position(|w| matches!(word(w), "drover" | "herdsman" | "fleet"))?;
        group(&t[i + 1..])
    })
}

/// `setup`, `serve approve|unpair|devices --revoke`, `config apply|discard`, `secret ls|rm` and
/// `card approve|deny`: owner-only (pre-rename `fleet` too). #538: a group verb (`clean --yes`, a design spawn, …)
/// is a conductor's too while `[conductor] allow` has its group on, never another lane's.
fn design_spawn(t: &[&str], p: &Policy) -> Option<String> {
    let orchestrator = p.orchestrator;
    let i = t
        .iter()
        .position(|w| matches!(w.rsplit('/').next(), Some("drover" | "herdsman" | "fleet")))?;
    let r = &t[i + 1..];
    if r.first() == Some(&"serve") && ["approve", "unpair", "--revoke"].iter().any(|w| r.contains(w)) {
        return Some("REFUSED OWNER_ONLY: only the owner pairs or unpairs devices, never a lane (#288)".into());
    }
    if matches!(
        (r.first(), r.get(1)),
        (Some(&"config"), Some(&("apply" | "discard")))
            | (Some(&"secret"), Some(&("ls" | "rm")))
            | (Some(&"card"), Some(&("approve" | "deny" | "run")))
    ) {
        return Some(
            "REFUSED OWNER_ONLY: only the owner applies config, settles run cards or lists and removes secrets, never \
             a lane (#387)"
                .into(),
        );
    }
    // #425: a lane may hold its own issue (`queue <fleet> block --on N <issue>`), never another one.
    let lane = std::env::var("DROVER_LANE").unwrap_or_default();
    let own = lane.split(':').nth(1).filter(|n| n.parse::<u64>().is_ok());
    let block = r.first() == Some(&"queue") && matches!(r.get(2), Some(&("block" | "unblock"))) && own.is_some();
    let other = |(i, w): (usize, &&str)| r[i - 1] != "--on" && w.parse::<u64>().is_ok() && Some(*w) != own;
    if block && r.iter().enumerate().skip(3).any(other) {
        return Some("REFUSED OWN_ISSUE: a lane may `queue block`/`unblock` only its own issue (#425)".into());
    }
    // #419: `lesson` files an issue and spawns a lane: a conductor's verb (the owner's too); its digest (and #510's
    // `apply`, a rules-file rewrite in the lane's own worktree) is open.
    if !orchestrator && r.first() == Some(&"lesson") && !matches!(r.get(1), Some(&"digest" | &"apply")) {
        return Some("REFUSED ORCHESTRATOR_ONLY: only an orchestrator or the owner runs `drover lesson` (#419)".into());
    }
    // #574: `amend` edits the issue's spec: the owner's decision, written by a conductor or the owner.
    if !orchestrator && r.first() == Some(&"amend") {
        return Some("REFUSED ORCHESTRATOR_ONLY: only an orchestrator or the owner runs `drover amend` (#574)".into());
    }
    // #461: `timer install` (only that host verb) is a conductor's too; outside a lane it is the owner's.
    if !orchestrator && !lane.is_empty() && r.first() == Some(&"timer") && r.contains(&"install") {
        return Some(
            "REFUSED ORCHESTRATOR_ONLY: only an orchestrator or the owner runs `drover timer install` (#461)".into(),
        );
    }
    if r.first() == Some(&"wip") && !r.contains(&"--dry-run") {
        return Some("REFUSED OWNER_ONLY: only the owner prunes wip branches (a run card), never a lane (#646)".into());
    }
    if r.first() == Some(&"setup") {
        return Some("REFUSED OWNER_ONLY: only the owner runs `drover setup`, never a lane (#303)".into());
    }
    if !orchestrator && r.first() == Some(&"card") {
        return Some("REFUSED ORCHESTRATOR_ONLY: only an orchestrator posts run cards (#538)".into());
    }
    let g = group(r)?;
    if g == "design" && !orchestrator {
        return Some(
            "design lanes are owner-only: the owner or an orchestrator thread runs `drover spawn … design`, never a lane"
                .into(),
        );
    }
    if !orchestrator {
        return Some(format!(
            "REFUSED OWNER_ONLY: `drover {}` ({g} group) is the owner's or an orchestrator's, never a lane's (#538)",
            r[0]
        ));
    }
    (!p.groups.iter().any(|x| x == g)).then(|| {
        format!(
            "REFUSED GROUP_OFF: the `{g}` group is off in [orchestrator] allow; propose turning it on with \
             `drover config propose` (#538)"
        )
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
            "pushing to `{b}` is blocked; integration branches change only via `drover merge`"
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

/// #386: a shell call's command when it runs `drover` or `gh` (but `drover thread reply`, which is the reply
/// itself): a conductor's tool row in its thread.
pub fn fleet_cmd(input: &Value) -> Option<&str> {
    let tool = input["tool_name"].as_str().unwrap_or_default();
    let cmd = tool_input(input)["command"].as_str()?.trim();
    let first = cmd.split_whitespace().next().map(word)?;
    (dialects().any(|d| d.shell == tool)
        && ["drover", "herdsman", "gh"].contains(&first)
        && !cmd.contains("thread reply"))
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
    log.lines().map(str::trim).find(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("co-authored-by:") && crate::providers::brands().chain(["copilot"]).any(|a| l.contains(a))
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

/// #511: what the stop hook tells an impl lane whose turn ended unreported.
pub const TURN_ASK: &str = "drover: end your turn with `drover turn done|blocked|need-input|working [--why <text>]` \
(done: pushed and ready for review; blocked: --why names the blocker; need-input: after `drover ask`; working: \
stopped mid-task)";

/// #511: the lane's turn marker on its own host: `prompt` once a prompt is submitted, `turn` once `drover turn`
/// ran, `asked` once the stop hook told it.
pub fn turn_mark(state: &Path, lane: &str) -> PathBuf {
    state.with_file_name(format!("turn-{}", lane.replace([':', '/'], "-")))
}

/// #511: an impl lane (`builds`) whose turn ends with no `drover turn` since its last prompt goes on once with
/// `TURN_ASK`; never twice (`asked`, or the harness's own `stop_hook_active`), so it never loops.
pub fn turn_end(input: &Value, mark: &Path, builds: impl FnOnce() -> bool) -> Option<Value> {
    let ev = input["hook_event_name"].as_str().filter(|e| !e.is_empty())?;
    let d = dialects().find(|d| d.prompt == ev || d.stop == ev)?;
    if d.prompt == ev {
        let _ = std::fs::write(mark, "prompt");
        return None;
    }
    let seen = std::fs::read_to_string(mark).unwrap_or_default();
    let again = d
        .again
        .filter(|_| !matches!(seen.as_str(), "turn" | "asked") && input["stop_hook_active"] != true)?;
    if !builds() {
        return None;
    }
    let _ = std::fs::write(mark, "asked");
    Some(again(TURN_ASK))
}

/// The deny answer in the dialect that sent `input`, else the first harness's (`Dialect::deny`).
pub fn deny_json(input: &Value, reason: &str) -> Value {
    let d = dialects().find(|d| (d.sent)(input)).or_else(|| dialects().next());
    d.map_or(Value::Null, |d| (d.deny)(&format!("drover: {reason}")))
}
