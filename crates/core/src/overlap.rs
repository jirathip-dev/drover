use crate::config::{Config, Fleet};
use crate::events::{emit, fold, Event, IssueState, Json};
use crate::runner::{argv, ok, Runner};
use crate::{gh, Ctx};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Per issue: (the other issue, 0 for integration, the files both change).
pub type Conflicts = BTreeMap<u64, Vec<(u64, Vec<String>)>>;

pub fn git(r: &dyn Runner, dir: &Path, args: &[&str]) -> Result<String> {
    let d = dir.to_string_lossy();
    ok(r, "git", &[&["-C", &*d], args].concat()).map(|o| o.trim().to_string())
}

/// The last preview's conflicts; never runs git.
pub fn cached(ctx: &Ctx, fleet: &str) -> Conflicts {
    let v = crate::usage::load::<Value>(&gh::cache_path(ctx, "conflicts", fleet, 0)).unwrap_or_default();
    serde_json::from_value(v.at("conflicts").clone()).unwrap_or_default()
}

pub(crate) fn collide(r: &dyn Runner, dir: &Path, a: &str, b: &str) -> Vec<String> {
    let d = dir.to_string_lossy();
    let o = r.run(
        "git",
        &argv(&["-C", &d, "merge-tree", "--write-tree", "--name-only", a, b]),
    );
    let o = o.ok().filter(|o| o.code == 1).unwrap_or_default();
    o.stdout
        .lines()
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

fn keep_going(o: u64) -> String {
    format!("may conflict with #{o}; keep going, drover updates you after it merges")
}

/// Every pair of pushed, unmerged branches and each against integration, cached by head pair; logs one `conflict-predicted` per new pair and prompts an impl/fix lane once per head about a ready PR or integration.
pub fn preview(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, events: &[Event]) -> Result<Conflicts> {
    let (f, p) = (cfg.fleet(fleet)?, gh::cache_path(ctx, "conflicts", fleet, 0));
    let dir = f.local_path();
    let rev = |b: &str| git(r, &dir, &["rev-parse", &format!("origin/{b}")]);
    let ev = |n, kind, d| emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), kind, d));
    crate::clone::fetch_branches(ctx, r, &dir)?;
    crate::baseline::grafts(r, f, &dir).ok();
    let st: Vec<IssueState> = fold(events)
        .into_values()
        .filter(|s| s.fleet == fleet && !s.done())
        .collect();
    let pushed = st
        .iter()
        .filter_map(|s| Some((s.issue, rev(&crate::gate::branch_of(Some(s), s.issue)).ok()?)));
    let heads: Vec<(u64, String)> = std::iter::once((0, rev(&f.integration)?)).chain(pushed).collect();
    let old = crate::usage::load::<Value>(&p).unwrap_or_default();
    let (mut cache, mut out) = (BTreeMap::new(), Conflicts::new());
    for (i, (a, ha)) in heads.iter().enumerate() {
        for (b, hb) in heads.get(i + 1..).unwrap_or_default() {
            let key = format!("{ha} {hb}");
            let files: Vec<String> =
                serde_json::from_value(old.at("cache").at(&key).clone()).unwrap_or_else(|_| collide(r, &dir, ha, hb));
            if !files.is_empty() && old.at("cache").at(&key).is_null() {
                ev(
                    *b,
                    "conflict-predicted",
                    json!({"other": a, "files": files, "heads": key, "reason": (*a > 0).then(|| keep_going(*a))}),
                )?;
            }
            for (x, y) in [(b, a), (a, b)]
                .into_iter()
                .filter(|(x, _)| **x > 0 && !files.is_empty())
            {
                out.entry(*x).or_default().push((*y, files.clone()));
            }
            cache.insert(key, files);
        }
    }
    gh::store(&p, &json!({"cache": cache, "conflicts": out}))?;
    for s in st.iter().filter(|s| s.state == "impl" || s.state.starts_with("fix")) {
        let ready = |o: &u64| *o == 0 || st.iter().any(|x| x.issue == *o && x.state == "ready");
        let (Some(lane), Some((o, files))) = (&s.lane, out.get(&s.issue).and_then(|c| c.iter().find(|c| ready(&c.0))))
        else {
            continue;
        };
        let key = format!("{} {o}", heads.iter().find(|h| h.0 == s.issue).map_or("", |h| &h.1));
        if events
            .iter()
            .any(|e| e.is("heads-up", fleet, s.issue) && e.str("key") == Some(&key))
        {
            continue;
        }
        let (into, files) = (format!("origin/{}", f.integration), files.join(", "));
        let text = if *o == 0 {
            format!("HEADS-UP: your branch conflicts with {into} on {files}. Merge {into} now.")
        } else {
            format!(
                "HEADS-UP: #{o} is ready to merge and touches {files}: your branch {}.",
                keep_going(*o)
            )
        };
        let sp = crate::events::last_spawn(events, lane);
        let pane = sp.and_then(|e| e.str("pane")).unwrap_or_default();
        let on = crate::hosts::On::of(r, sp);
        if crate::term::tell(&on, lane, pane, &text).is_ok() {
            ev(s.issue, "heads-up", json!({"key": key, "reason": text}))?;
        }
    }
    Ok(out)
}

/// `h` merges a passed head (or an earlier pure update of one) with integration and adds nothing: its first parent carries the PASS, its second is on integration, and its tree is the one `git merge-tree` makes of the two.
pub fn pure(ctx: &Ctx, r: &dyn Runner, f: &Fleet, s: &IssueState, h: &str) -> bool {
    let dir = f.local_path();
    crate::clone::fetch_branches(ctx, r, &dir).is_ok() && pure_at(r, &dir, &format!("origin/{}", f.integration), s, h)
}

/// `h` is a pure update of `x`, so a verdict at `x` (whatever it says) carries to `h`.
pub fn carries(ctx: &Ctx, r: &dyn Runner, f: &Fleet, s: &IssueState, x: &str, h: &str) -> bool {
    let mut s = s.clone();
    s.verdicts.push((x.into(), "PASS".into(), String::new()));
    pure(ctx, r, f, &s, h)
}

fn pure_at(r: &dyn Runner, dir: &Path, into: &str, s: &IssueState, h: &str) -> bool {
    let Ok(p) = git(r, dir, &["rev-list", "--parents", "-n1", h]) else {
        return false;
    };
    let [_, a, b] = p.split(' ').collect::<Vec<_>>()[..] else {
        return false;
    };
    let pass = s
        .verdicts
        .iter()
        .rfind(|v| v.0.eq_ignore_ascii_case(a))
        .is_some_and(|v| v.1 == "PASS");
    (pass || pure_at(r, dir, into, s, a))
        && git(r, dir, &["merge-base", "--is-ancestor", b, into]).is_ok()
        && git(r, dir, &["merge-tree", "--write-tree", a, b]).ok()
            == git(r, dir, &["rev-parse", &format!("{h}^{{tree}}")]).ok()
}

/// `drover cap-check`: the lines `config` names to count (`Counted`) against its first `line_cap`, top-level or `[fleet.<n>]`, or a higher one in `cap.d/`; under `warn_within` lines of headroom it warns and still passes. `web/src/**/*.ts` lines are reported, never capped. With `base`, every commit since it that edits the cap must carry a `Cap:` trailer.
pub fn cap_check(r: &dyn Runner, config: &Path, base: Option<&str>, out: &mut dyn std::io::Write) -> Result<i32> {
    let mut bad = 0;
    if let Some(b) = base {
        let range = format!("{b}..HEAD");
        let log = git(
            r,
            Path::new("."),
            &[
                "log",
                "--no-merges",
                "--format=%x1e%h%x1f%B",
                &range,
                "--",
                CAP_PATHS[0],
                CAP_PATHS[1],
            ],
        )?;
        for c in log.split('\x1e').skip(1) {
            let (sha, body) = c.split_once('\x1f').unwrap_or((c, ""));
            if !body.lines().any(cap_trailer) {
                bad += 1;
                writeln!(
                    out,
                    "{sha} edits the line cap without a `Cap: <old> → <new> (#issue, q<N>)` trailer"
                )?;
            }
        }
    }
    let dir = std::fs::read_dir(config.with_file_name("cap.d"));
    let mut files: Vec<PathBuf> = (dir.into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    let raises = files
        .iter()
        .map(|p| Ok((p.display().to_string(), std::fs::read_to_string(p)?)));
    let text = std::fs::read_to_string(config)?;
    let missing = || anyhow::anyhow!("no line_cap in {}", config.display());
    let Cap {
        lines: cap,
        warn_within: warn,
        from,
    } = highest(
        &config.display().to_string(),
        &text,
        raises.collect::<Result<Vec<_>>>()?,
    )
    .ok_or_else(missing)?;
    let rule: Counted = toml::Value::Table(cap_table(&text).ok_or_else(missing)?).try_into()?;
    let n = rule.lines();
    writeln!(out, "src lines (formatted): {n} / cap {cap}")?;
    if from != config.display().to_string() {
        writeln!(out, "cap raised by {from}")?;
    }
    writeln!(
        out,
        "web lines (TypeScript, soft budget, not capped): {}",
        lines(Path::new("web/src"), "ts")
    )?;
    let left = cap - i64::try_from(n)?;
    if (0..warn).contains(&left) {
        writeln!(out, "WARN: {left} lines of headroom (< {warn})")?;
    }
    Ok(i32::from(left < 0 || bad > 0))
}

const CAP_PATHS: [&str; 2] = ["ci/cap.toml", "ci/cap.d"];

pub(crate) fn cap_trailer(l: &str) -> bool {
    let num = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    let Some((old, rest)) = l.trim().strip_prefix("Cap: ").and_then(|t| t.split_once(" → ")) else {
        return false;
    };
    let Some((new, refs)) = rest.split_once(" (#") else {
        return false;
    };
    let Some((issue, q)) = refs.strip_suffix(')').and_then(|t| t.split_once(", q")) else {
        return false;
    };
    [old, new, issue, q].into_iter().all(num)
}

/// `drover cap log`: every change of the line cap in the repo's git history, oldest first, with its `Cap:` trailer, else its subject.
pub fn cap_log(r: &dyn Runner, dir: &Path, out: &mut dyn std::io::Write) -> Result<i32> {
    let args = [
        "log",
        "--reverse",
        "-p",
        "-U0",
        "-G",
        "line_cap = |line cap \\(≤ ",
        "--format=%x1e%cs %h%x1f%B%x1f",
        "--",
        CAP_PATHS[0],
        CAP_PATHS[1],
        ".github/workflows/ci.yml",
    ];
    let cap = |l: &str| {
        let t = l.trim_start();
        let n = t
            .strip_prefix("line_cap = ")
            .or_else(|| t.split("line cap (≤ ").nth(1))?;
        n.split(|c: char| !c.is_ascii_digit()).next()?.parse::<i64>().ok()
    };
    for c in git(r, dir, &args)?.split('\x1e').skip(1) {
        let mut part = c.splitn(3, '\x1f');
        let (head, body, diff) = (
            part.next().unwrap_or_default(),
            part.next().unwrap_or_default(),
            part.next().unwrap_or_default(),
        );
        let at = |sign: char| diff.lines().filter_map(|l| l.strip_prefix(sign)).find_map(cap);
        let (Some(new), old) = (at('+'), at('-')) else {
            continue;
        };
        if old == Some(new) {
            continue;
        }
        let why = (body.lines().find(|l| cap_trailer(l)))
            .or_else(|| body.lines().next())
            .unwrap_or_default();
        let old = old.map_or_else(|| "-".to_string(), |o| o.to_string());
        writeln!(out, "{head} {old} → {new}  {}", why.trim())?;
    }
    Ok(0)
}

/// The effective line cap: the highest `line_cap` among `ci/cap.toml` and the raises in `ci/cap.d/*.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cap {
    pub lines: i64,
    pub warn_within: i64,
    pub from: String,
}

fn highest(name: &str, base: &str, raises: Vec<(String, String)>) -> Option<Cap> {
    let (lines, warn_within) = limits(base)?;
    let num = |t: &str| toml::from_str::<toml::Value>(t).ok()?.get("line_cap")?.as_integer();
    let each = raises.into_iter().filter_map(|(from, t)| {
        Some(Cap {
            lines: num(&t)?,
            warn_within,
            from,
        })
    });
    let first = Cap {
        lines,
        warn_within,
        from: name.into(),
    };
    Some(each.fold(first, |a, b| if b.lines > a.lines { b } else { a }))
}

/// The cap at `sha` of the repo in `dir`, by git alone: `ci/cap.toml` and `ci/cap.d/*.toml` at that commit.
pub fn cap_at_sha(r: &dyn Runner, dir: &Path, sha: &str) -> Option<Cap> {
    let show = |p: &str| git(r, dir, &["show", &format!("{sha}:{p}")]).ok();
    let base = show("ci/cap.toml")?;
    let names = git(r, dir, &["ls-tree", "--name-only", sha, "ci/cap.d/"]).unwrap_or_default();
    let raises = names
        .lines()
        .filter(|n| Path::new(n).extension().is_some_and(|x| x == "toml"))
        .filter_map(|n| Some((n.to_string(), show(n)?)));
    highest("ci/cap.toml", &base, raises.collect())
}

fn cap_table(text: &str) -> Option<toml::Table> {
    let v: toml::Table = toml::from_str(text).ok()?;
    let fleets = v
        .get("fleet")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|f| f.values());
    let fleets = fleets.filter_map(toml::Value::as_table);
    std::iter::once(&v)
        .chain(fleets)
        .find(|t| t.contains_key("line_cap"))
        .cloned()
}

fn limits(text: &str) -> Option<(i64, i64)> {
    let t = cap_table(text)?;
    let warn = t.get("warn_within").and_then(toml::Value::as_integer).unwrap_or(500);
    Some((t.get("line_cap")?.as_integer()?, warn))
}

#[derive(serde::Deserialize)]
struct Counted {
    #[serde(default = "rust_globs")]
    count: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    skip_blank: bool,
    #[serde(default)]
    stop_at_cfg_test: bool,
}

fn rust_globs() -> Vec<String> {
    vec!["src/**/*.rs".into(), "crates/*/src/**/*.rs".into()]
}

impl Counted {
    fn at(r: &dyn Runner, dir: &Path, sha: &str) -> Option<Self> {
        let text = git(r, dir, &["show", &format!("{sha}:ci/cap.toml")]).ok()?;
        toml::Value::Table(cap_table(&text)?).try_into().ok()
    }

    fn counts(&self, p: &str) -> bool {
        let hit = |gs: &[String]| gs.iter().any(|g| crate::config::glob(g.as_bytes(), p.as_bytes()));
        hit(&self.count) && !hit(&self.exclude)
    }

    fn roots(&self) -> Vec<String> {
        let root = |g: &String| {
            g.split('/')
                .take_while(|c| !c.contains(['*', '?']))
                .collect::<Vec<_>>()
                .join("/")
        };
        self.count.iter().map(root).collect()
    }

    fn lines(&self) -> usize {
        let mut files = std::collections::BTreeSet::new();
        for root in self.roots() {
            walk(&root, &mut files);
        }
        let files = files.into_iter().filter(|p| self.counts(p));
        files
            .map(|p| self.body(&p, std::fs::read_to_string(&p).unwrap_or_default().lines()))
            .sum()
    }

    fn lines_at(&self, r: &dyn Runner, dir: &Path, rev: &str) -> Option<i64> {
        let roots: Vec<String> = self
            .roots()
            .into_iter()
            .map(|p| if p.is_empty() { ".".into() } else { p })
            .collect();
        let roots: Vec<&str> = roots.iter().map(String::as_str).collect();
        let o = git(r, dir, &[&["grep", "-z", "-I", "", rev, "--"][..], &roots].concat()).unwrap_or_default();
        let at = format!("{rev}:");
        let mut files: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
        for (p, l) in o.lines().filter_map(|l| l.split_once('\0')) {
            files.entry(p.strip_prefix(&at).unwrap_or(p)).or_default().push(l);
        }
        let files = files.into_iter().filter(|(p, _)| self.counts(p));
        i64::try_from(files.map(|(p, ls)| self.body(p, ls.into_iter())).sum::<usize>()).ok()
    }

    fn body<'a>(&self, p: &str, lines: impl Iterator<Item = &'a str>) -> usize {
        let cut = self.stop_at_cfg_test && Path::new(p).extension().is_some_and(|x| x == "rs");
        let body = lines.take_while(|l| !(cut && l.trim_start().starts_with("#[cfg(test)]")));
        body.filter(|l| !(self.skip_blank && l.trim().is_empty())).count()
    }

    fn rules(&self) -> String {
        let mut each = vec![format!("count {}", self.count.join(" "))];
        each.extend((!self.exclude.is_empty()).then(|| format!("exclude {}", self.exclude.join(" "))));
        each.extend(self.skip_blank.then(|| "skip_blank".to_string()));
        each.extend(self.stop_at_cfg_test.then(|| "stop_at_cfg_test".to_string()));
        if each.len() == 1 && self.count == rust_globs() {
            return "the built-in count of src/**/*.rs and crates/*/src/**/*.rs".into();
        }
        format!("ci/cap.toml: {}", each.join(", "))
    }
}

fn walk(p: &str, out: &mut std::collections::BTreeSet<String>) {
    let dir = if p.is_empty() { "." } else { p };
    if Path::new(dir).is_file() {
        out.insert(p.to_string());
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        walk(&if p.is_empty() { name } else { format!("{p}/{name}") }, out);
    }
}

/// `(sha, lines, cap, warn_within)` at the fleet's fetched integration head with a `ci/cap.toml`, by git alone; cached per sha (`serve`'s banner reads the cache).
pub fn cap_at(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str) -> Option<(String, i64, i64, i64)> {
    let (f, p) = (cfg.fleet(fleet).ok()?, gh::cache_path(ctx, "cap", fleet, 0));
    let dir = f.local_path();
    let sha = git(r, &dir, &["rev-parse", &format!("origin/{}", f.integration)]).ok()?;
    let old = crate::usage::load::<Value>(&p).unwrap_or_default();
    let n = |k: &str| old.at(k).as_i64();
    if old.at("sha") == sha.as_str() {
        return Some((sha, n("lines")?, n("cap")?, n("warn")?));
    }
    let Cap {
        lines: cap,
        warn_within: warn,
        ..
    } = cap_at_sha(r, &dir, &sha)?;
    let lines = Counted::at(r, &dir, &sha)?.lines_at(r, &dir, &sha)?;
    gh::store(&p, &json!({"sha": sha, "lines": lines, "cap": cap, "warn": warn})).ok();
    Some((sha, lines, cap, warn))
}

fn lines(p: &Path, ext: &str) -> usize {
    let each = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path());
    let n = |p: PathBuf| match p.extension() {
        _ if p.is_dir() => lines(&p, ext),
        Some(x) if x == ext => std::fs::read_to_string(&p).map_or(0, |t| t.lines().count()),
        _ => 0,
    };
    each.map(n).sum()
}

fn stated_net(body: &str) -> Option<i64> {
    let rest = body.split("src/ net ").nth(1)?.trim_start();
    let n: String = rest.chars().take_while(|c| "+-,0123456789".contains(*c)).collect();
    n.replace([',', '+'], "").parse().ok()
}

struct Count {
    label: String,
    lines: i64,
    limit: i64,
}

fn cap_script(r: &dyn Runner, f: &Fleet, sha: &str) -> Option<String> {
    let run = |p: &crate::config::Premerge| match p {
        crate::config::Premerge::Cmd(c) => c.clone(),
        crate::config::Premerge::Named(c) => c.run.clone(),
    };
    let own = || {
        git(
            r,
            &f.local_path(),
            &["cat-file", "-e", &format!("{sha}:scripts/line-cap.sh")],
        )
        .ok()
    };
    (f.cap_script.clone())
        .or_else(|| f.premerge.iter().flatten().map(run).find(|c| c.contains("cap")))
        .or_else(|| own().map(|_| "sh scripts/line-cap.sh".to_string()))
}

fn script_caps(
    ctx: &Ctx,
    r: &dyn Runner,
    f: &Fleet,
    script: &str,
    base: &str,
    branch: Option<&str>,
) -> Result<Vec<Count>, String> {
    let (dir, wt) = (
        f.local_path(),
        ctx.state.with_file_name(format!("cap-{}", f.repo.replace('/', "-"))),
    );
    let wts = wt.to_string_lossy();
    let tree = |a: &[&str]| git(r, &dir, &[&["worktree"][..], a].concat());
    tree(&["remove", "--force", &wts]).ok();
    tree(&["add", "-f", "--detach", &wts, base]).map_err(|e| e.to_string())?;
    let merged = branch.is_none_or(|b| git(r, &wt, &["merge", "--no-ff", "--no-edit", b]).is_ok());
    let sh = argv(&["-c", &format!("cd \"$0\" && {script}"), &wts]);
    let o = merged.then(|| r.run("sh", &sh));
    tree(&["remove", "--force", &wts]).ok();
    let o = o.ok_or("conflicts")?.map_err(|e| e.to_string())?;
    let num = |w: &str| w.replace(',', "").parse::<i64>().ok();
    let cap = |l: &str| {
        let (left, rest) = l.split_once(" / ")?;
        let (label, n) = left
            .trim()
            .rsplit_once(char::is_whitespace)
            .unwrap_or(("", left.trim()));
        Some(Count {
            label: label.trim().trim_end_matches(':').trim().to_string(),
            lines: num(n)?,
            limit: num(rest.trim_start().trim_start_matches("cap ").split_whitespace().next()?)?,
        })
    };
    let text = format!("{}{}", o.stdout, o.stderr);
    let caps: Vec<Count> = text.lines().filter_map(cap).collect();
    let last = text.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("nothing");
    (!caps.is_empty())
        .then_some(caps)
        .ok_or_else(|| format!("printed `{}`", last.trim()))
}

/// `drover cap <fleet>`: the queue-wide line-cap view, so one conductor-filed issue raises the cap for the whole queue: integration's total against the cap, every open PR's `src/ net` (its body's, else its measured diff), the queued issues with no PR, and the projected total with the ask a crossing lane files. a fleet with its own cap script counts by it, at integration and on each open PR's merge, every cap it prints.
pub fn cap_view(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, out: &mut dyn std::io::Write) -> Result<i32> {
    let f = cfg.fleet(fleet)?;
    let dir = f.local_path();
    crate::clone::fetch_branches(ctx, r, &dir).ok();
    let head = git(r, &dir, &["rev-parse", &format!("origin/{}", f.integration)]);
    let script = head.as_ref().ok().and_then(|h| cap_script(r, f, h));
    let (sha, caps, warn) = if let Some(s) = &script {
        let sha = head?;
        let warn = cap_at_sha(r, &dir, &sha).map_or(500, |c| c.warn_within);
        let caps = script_caps(ctx, r, f, s, &sha, None).map_err(|got| {
            let at = &f.integration;
            anyhow::anyhow!("`{s}` at origin/{at} printed no `N / cap M` line drover can parse: {got}")
        })?;
        (sha, caps, warn)
    } else {
        let (sha, lines, cap, warn) = cap_at(ctx, cfg, r, fleet).ok_or_else(|| {
            anyhow::anyhow!(
                "no ci/cap.toml line_cap at origin/{} in {}",
                f.integration,
                dir.display()
            )
        })?;
        let label = String::new();
        (
            sha,
            vec![Count {
                label,
                lines,
                limit: cap,
            }],
            warn,
        )
    };
    let lab = |c: &Count| format!("{} ", c.label).trim_start().to_string();
    for c in &caps {
        writeln!(
            out,
            "{fleet}: {}{} / cap {} at {} {sha:.7}, headroom {}",
            lab(c),
            c.lines,
            c.limit,
            f.integration,
            c.limit - c.lines
        )?;
    }
    let rule = script.is_none().then(|| Counted::at(r, &dir, &sha)).flatten();
    let by = match (&script, &rule) {
        (Some(s), _) => format!("`{s}`"),
        (None, Some(c)) => c.rules(),
        (None, None) => "ci/cap.toml".to_string(),
    };
    writeln!(out, "counted by {by}")?;
    let prs = gh::snapshot(ctx, r, f, fleet, ctx.now)?;
    let mut prs: Vec<&Value> = (prs.as_object().into_iter().flat_map(|m| m.values()))
        .filter(|p| p["baseRefName"] == f.integration.as_str())
        .collect();
    prs.sort_by_key(|p| p["number"].as_u64());
    let measured = |b: &str| -> Option<i64> {
        let (c, head) = (rule.as_ref()?, format!("origin/{b}"));
        let base = git(r, &dir, &["merge-base", &sha, &head]).ok()?;
        Some(c.lines_at(r, &dir, &head)? - c.lines_at(r, &dir, &base)?)
    };
    let scripted = |s: &str, b: &str| -> Option<Vec<i64>> {
        let m = script_caps(ctx, r, f, s, &sha, Some(&format!("origin/{b}"))).ok()?;
        let net = |c: &Count| Some(m.iter().find(|x| x.label == c.label)?.lines - c.lines);
        caps.iter().map(net).collect()
    };
    let (mut sums, mut has) = (vec![0; caps.len()], std::collections::BTreeSet::new());
    writeln!(out, "open PRs (src/ net):")?;
    for p in prs {
        let b = p["headRefName"].as_str().unwrap_or_default();
        let issue = b.strip_prefix("issue-").and_then(|n| n.parse::<u64>().ok());
        has.extend(issue);
        let who = issue.map_or_else(|| b.to_string(), |n| format!("#{n}"));
        let net = match &script {
            Some(s) => scripted(s, b).map(|n| (n, "script")),
            None => stated_net(p["body"].as_str().unwrap_or_default())
                .map(|n| (vec![n], "body"))
                .or_else(|| measured(b).map(|n| (vec![n], "measured"))),
        };
        let num = p["number"].as_u64().unwrap_or(0);
        match net {
            Some((n, how)) => {
                let each = n.iter().zip(&caps).map(|(n, c)| match caps.len() {
                    1 => format!("{n:+}"),
                    _ => format!("{n:+} {}", c.label),
                });
                sums.iter_mut().zip(&n).for_each(|(s, n)| *s += n);
                writeln!(
                    out,
                    "  {who} (PR #{num}): {} ({how})",
                    each.collect::<Vec<_>>().join(", ")
                )?;
            }
            None if script.is_some() => writeln!(
                out,
                "  {who} (PR #{num}): unknown (conflicts with {} or the script printed no cap)",
                f.integration
            )?,
            None => writeln!(
                out,
                "  {who} (PR #{num}): unknown (no `src/ net` in its body, branch not fetched)"
            )?,
        }
    }
    let (events, st) = (crate::events::working(&ctx.state)?, crate::events::states(&ctx.state)?);
    let queued: Vec<u64> = (crate::queue::view(&events, &st, fleet).into_iter().map(|i| i.issue))
        .filter(|n| !has.contains(n))
        .collect();
    let none = queued.is_empty().then_some("none");
    writeln!(
        out,
        "queued, no PR: {}",
        none.map_or_else(|| crate::queue::hashes(&queued), String::from)
    )?;
    let mut raise = Vec::new();
    for (c, sum) in caps.iter().zip(&sums) {
        let projected = c.lines + sum;
        let of = if caps.len() > 1 {
            format!("{}: ", c.label)
        } else {
            String::new()
        };
        writeln!(
            out,
            "{of}sum of open PRs: {sum:+}; projected total {projected} / cap {}, headroom {}",
            c.limit,
            c.limit - projected
        )?;
        raise.push((projected > c.limit, format!("{}to {}", lab(c), projected + warn)));
    }
    if raise.iter().any(|r| r.0) {
        raise.retain(|r| r.0);
    }
    let raise: Vec<String> = raise.into_iter().map(|r| r.1).collect();
    writeln!(
        out,
        "ask options: 1. raise {} in one orchestrator-filed issue (recommended) 2. trim",
        raise.join(", ")
    )?;
    Ok(0)
}
