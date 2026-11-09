//! #734: `herdsman arch-check`: a repo's architecture rules in `.herdsman/arch.toml`, checked by plain source scanning
//! of Rust and TypeScript: a layer uses only the layers its `may_use` lists, a banned pattern appears only in its
//! exceptions, and a file or function over its size limit fails unless the baseline lists it at that size or more.
use crate::config::glob;
use crate::runner::{argv, Runner};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arch {
    #[serde(default)]
    layer: Vec<Layer>,
    #[serde(default)]
    ban: Vec<Ban>,
    #[serde(default)]
    size: Vec<Size>,
    /// Today's size offenders, `<file>[:<fn>] <lines>` a line (default `.herdsman/arch-baseline.txt`).
    baseline: Option<String>,
    comments: Option<Comments>,
    docs: Option<Docs>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Comments {
    policy: Policy,
    #[serde(default)]
    except: Vec<String>,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Policy {
    On,
    Off,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Docs {
    paths: Vec<String>,
    #[serde(default)]
    watch: Vec<String>,
    #[serde(default)]
    skip: Vec<String>,
    #[serde(default)]
    cli: bool,
}

struct Change {
    file: String,
    line: usize,
    added: bool,
    text: String,
}

/// A layer: its files, the names other files use it by (`herdsman_core`, `crate::gate`, `@app/core`; a relative
/// TypeScript import resolves to a path and matches `paths`), and the layers it may use.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Layer {
    name: String,
    paths: Vec<String>,
    #[serde(default)]
    mods: Vec<String>,
    #[serde(default)]
    may_use: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ban {
    pattern: String,
    paths: Vec<String>,
    #[serde(default)]
    except: Vec<String>,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Size {
    paths: Vec<String>,
    max_file_lines: Option<usize>,
    max_fn_lines: Option<usize>,
}

/// Whether a glob matches `p`, `/**/` also matching no folder (`src/**/*.rs` matches `src/lib.rs`).
fn any(globs: &[String], p: &str) -> bool {
    let m = |g: &str| glob(g.as_bytes(), p.as_bytes());
    globs.iter().any(|g| m(g) || m(&g.replace("/**/", "/")))
}

/// `herdsman arch-check`: each violation as `file:line: why`, exit 1 on any; `write` records today's size offenders
/// as the baseline instead. No config: nothing to check.
pub fn check(
    config: &Path,
    write: bool,
    since: Option<&str>,
    pr_body: Option<&Path>,
    r: &dyn Runner,
    out: &mut dyn Write,
) -> Result<i32> {
    let Ok(text) = std::fs::read_to_string(config) else {
        writeln!(out, "no {}: nothing to check", config.display())?;
        return Ok(0);
    };
    let a: Arch = toml::from_str(&text).with_context(|| format!("parsing {}", config.display()))?;
    let base_path = a.baseline.as_deref().unwrap_or(".herdsman/arch-baseline.txt");
    let base: BTreeMap<String, usize> = std::fs::read_to_string(base_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            l.trim()
                .rsplit_once(' ')
                .and_then(|(k, n)| Some((k.into(), n.parse().ok()?)))
        })
        .collect();
    let mut files = vec![];
    walk(Path::new("."), &mut files);
    files.sort();
    let (mut bad, mut now) = (vec![], BTreeMap::<String, usize>::new());
    for p in &files {
        let ext = Path::new(p).extension().and_then(|x| x.to_str()).unwrap_or_default();
        let rust = ext == "rs";
        let Some(src) = (rust || ext == "ts" || ext == "tsx")
            .then(|| std::fs::read_to_string(p).ok())
            .flatten()
        else {
            continue;
        };
        let lines: Vec<&str> = src.lines().collect();
        let own = a.layer.iter().find(|l| any(&l.paths, p));
        for (i, l) in lines
            .iter()
            .enumerate()
            .filter(|(_, l)| !l.trim_start().starts_with("//"))
        {
            let at = format!("{p}:{}", i + 1);
            for b in a
                .ban
                .iter()
                .filter(|b| l.contains(&b.pattern) && any(&b.paths, p) && !any(&b.except, p))
            {
                bad.push(format!("{at}: `{}` is banned here: {}", b.pattern, b.reason));
            }
            for (own, t) in own
                .into_iter()
                .flat_map(|o| targets(p, l, rust).into_iter().map(move |t| (o, t)))
            {
                let to = a.layer.iter().find(|x| uses(x, &t)).filter(|x| x.name != own.name);
                if let Some(to) = to.filter(|x| !own.may_use.contains(&x.name)) {
                    let (from, may) = (&own.name, &own.may_use);
                    bad.push(format!(
                        "{at}: layer `{from}` uses `{}` ({t}); its may_use is {may:?}",
                        to.name
                    ));
                }
            }
        }
        for s in a.size.iter().filter(|s| any(&s.paths, p)) {
            let mut sizes = vec![(p.clone(), 1, lines.len(), s.max_file_lines)];
            let each = fns(&lines, rust).into_iter();
            sizes.extend(each.map(|(name, at, n)| (format!("{p}:{name}"), at, n, s.max_fn_lines)));
            for (key, at, n, max) in sizes {
                let Some(max) = max.filter(|&m| n > m) else { continue };
                let e = now.entry(key.clone()).or_default();
                *e = n.max(*e);
                match base.get(&key) {
                    Some(&was) if n <= was => {}
                    was => bad.push(format!("{p}:{at}: `{key}` is {n} lines, over {max} (baseline {was:?})")),
                }
            }
        }
    }
    let diff = match since {
        Some(b) => {
            let o = r.run("git", &argv(&["diff", "-U0", "--no-color", "--no-ext-diff", b]))?;
            anyhow::ensure!(o.code == 0, "git diff {b}: {}", o.stderr.trim());
            changes(&o.stdout)
        }
        None => vec![],
    };
    if let Some(c) = a.comments.as_ref().filter(|c| c.policy == Policy::Off) {
        bad.extend(new_comments(&diff, &c.except));
    }
    if let Some(d) = &a.docs {
        bad.extend(stale_docs(d, &files));
        let excused = match pr_body {
            Some(p) => std::fs::read_to_string(p)?.contains("no doc:"),
            None => true,
        };
        if !excused {
            bad.extend(undocumented(d, &diff));
        }
    }
    if write {
        std::fs::write(
            base_path,
            now.iter().map(|(k, n)| format!("{k} {n}\n")).collect::<String>(),
        )?;
        writeln!(out, "wrote {} size offenders to {base_path}", now.len())?;
        return Ok(0);
    }
    for b in &bad {
        writeln!(out, "{b}")?;
    }
    writeln!(out, "arch-check: {} files, {} violations", files.len(), bad.len())?;
    Ok(i32::from(!bad.is_empty()))
}

/// The repo's files under `dir`, skipping dot folders, `target*` and `node_modules`.
fn walk(dir: &Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.starts_with("target") || name == "node_modules" {
            continue;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&e.path(), out),
            Ok(_) => out.push(e.path().to_string_lossy().trim_start_matches("./").into()),
            Err(_) => {}
        }
    }
}

/// Whether `t`, a path a line names, is layer `l`'s: one of its `mods` (or under it), or a file its `paths` match.
fn uses(l: &Layer, t: &str) -> bool {
    let under = |m: &String| t == m || t.starts_with(&format!("{m}::")) || t.starts_with(&format!("{m}/"));
    l.mods.iter().any(under)
        || ["", ".ts", ".tsx", "/index.ts"]
            .iter()
            .any(|x| any(&l.paths, &format!("{t}{x}")))
}

/// What line `l` of file `p` names: in Rust each `a::b` path (one `use a::{b, c}` group expanded); in TypeScript
/// the module an `import`, `export … from` or `require` loads, a relative one resolved against `p`'s folder.
fn targets(p: &str, l: &str, rust: bool) -> Vec<String> {
    let path = |c: char| !(c.is_alphanumeric() || c == '_' || c == ':');
    if rust {
        let mut t: Vec<String> = l.split(path).filter(|w| w.contains("::")).map(String::from).collect();
        if let Some((head, rest)) = l.split_once("::{") {
            let pre = head.rsplit(path).next().unwrap_or_default();
            let items = rest.split(['}', ',']).map(str::trim).filter(|x| !x.is_empty());
            t.extend(items.map(|x| format!("{pre}::{x}")));
        }
        return t;
    }
    let quoted = |s: &str| {
        let s = s.trim_start();
        let q = s.chars().next().filter(|c| "\"'`".contains(*c))?;
        s[1..].split(q).next().map(String::from)
    };
    let Some(spec) = ["from ", "import(", "require(", "import "]
        .iter()
        .filter_map(|k| l.split_once(k))
        .find_map(|(_, s)| quoted(s))
    else {
        return vec![];
    };
    if !spec.starts_with('.') {
        return vec![spec];
    }
    let mut parts: Vec<&str> = p.split('/').collect();
    parts.pop();
    for c in spec.split('/') {
        match c {
            "." => {}
            ".." => drop(parts.pop()),
            c => parts.push(c),
        }
    }
    vec![parts.join("/")]
}

/// Each function's `(name, first line, lines)`: a Rust `fn` or TypeScript `function` / `const f = (…) => {` whose
/// line opens a body, to the first later line at its indent that starts with `}` (formatted source).
fn fns(lines: &[&str], rust: bool) -> Vec<(String, usize, usize)> {
    let kw = if rust { "fn " } else { "function " };
    let mut v = vec![];
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        let arrow = || (!rust && t.contains("=> {")).then(|| t.trim_start_matches("export ").strip_prefix("const "));
        let after = t
            .split_once(kw)
            .filter(|(pre, _)| pre.is_empty() || pre.ends_with(' '))
            .map(|x| x.1);
        let Some(name) = after
            .or_else(|| arrow().flatten())
            .and_then(|r| r.split(|c: char| !c.is_alphanumeric() && c != '_').next())
        else {
            continue;
        };
        if name.is_empty() || l.trim_end().ends_with([';', '}']) {
            continue;
        }
        let ind = l.len() - t.len();
        let end = lines[i + 1..]
            .iter()
            .position(|x| x.len() - x.trim_start().len() == ind && x.trim_start().starts_with('}'));
        if let Some(e) = end {
            v.push((name.to_string(), i + 1, e + 2));
        }
    }
    v
}

fn changes(diff: &str) -> Vec<Change> {
    let (mut v, mut from, mut to, mut old, mut new, mut head) = (vec![], "", "", 0, 0, true);
    for l in diff.lines() {
        if l.starts_with("diff --git ") {
            head = true;
        } else if let Some(h) = l.strip_prefix("@@ -") {
            let num = |s: &str| s.split([',', ' ']).next().and_then(|n| n.parse().ok()).unwrap_or(0);
            let (o, n) = h.split_once(" +").unwrap_or_default();
            (old, new, head) = (num(o), num(n), false);
        } else if head {
            from = l.strip_prefix("--- a/").unwrap_or(from);
            to = l.strip_prefix("+++ b/").unwrap_or(to);
        } else if let Some(t) = l.strip_prefix('+') {
            v.push(Change {
                file: to.into(),
                line: new,
                added: true,
                text: t.into(),
            });
            new += 1;
        } else if let Some(t) = l.strip_prefix('-') {
            v.push(Change {
                file: from.into(),
                line: old,
                added: false,
                text: t.into(),
            });
            old += 1;
        }
    }
    v
}

fn comment_lines(lines: &[&str]) -> Vec<bool> {
    let mut open = false;
    let each = lines.iter().map(|l| {
        let t = l.trim_start();
        let c = open || t.starts_with("//") || t.starts_with("/*");
        if open {
            open = !l.contains("*/");
        } else if let Some(r) = t.strip_prefix("/*") {
            open = !r.contains("*/");
        }
        c
    });
    each.collect()
}

fn allowed_comment(lines: &[&str], com: &[bool], i: usize) -> bool {
    let t = lines[i].trim();
    if t.starts_with("// SAFETY:") {
        return lines.get(i + 1).is_some_and(|n| n.contains("unsafe"));
    }
    let one_doc = (t.starts_with("///") && !t.starts_with("////")) || (t.starts_with("/**") && t.ends_with("*/"));
    let mut item = lines[i + 1..].iter().map(|l| l.trim_start());
    let item = item.find(|l| !l.starts_with("#[") && !l.starts_with('@'));
    let public = item.is_some_and(|l| l.starts_with("pub ") || l.starts_with("export "));
    one_doc && public && (i == 0 || !com[i - 1])
}

fn new_comments(diff: &[Change], except: &[String]) -> Vec<String> {
    let mut seen: BTreeMap<&str, (Vec<String>, Vec<bool>)> = BTreeMap::new();
    let code = |f: &str| [".rs", ".ts", ".tsx"].iter().any(|x| f.ends_with(x)) && !any(except, f);
    let mut bad = vec![];
    for c in diff.iter().filter(|c| c.added && code(&c.file)) {
        let (lines, com) = seen.entry(&c.file).or_insert_with(|| {
            let src = std::fs::read_to_string(&c.file).unwrap_or_default();
            let lines: Vec<String> = src.lines().map(String::from).collect();
            let com = comment_lines(&lines.iter().map(String::as_str).collect::<Vec<_>>());
            (lines, com)
        });
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let i = c.line.saturating_sub(1);
        if com.get(i) == Some(&true) && !allowed_comment(&lines, com, i) {
            bad.push(format!(
                "{}:{}: a new comment (only `// SAFETY:` over unsafe and one `///` on a pub item): put the why in `herdsman why`",
                c.file, c.line
            ));
        }
    }
    bad
}

fn stale_docs(d: &Docs, files: &[String]) -> Vec<String> {
    let cli = d.cli.then(|| {
        let mut c = <crate::cli::Cli as clap::CommandFactory>::command();
        c.build();
        c
    });
    let mut bad = vec![];
    for p in files.iter().filter(|p| any(&d.paths, p)) {
        let mut fence = false;
        for (i, l) in std::fs::read_to_string(p).unwrap_or_default().lines().enumerate() {
            fence ^= l.trim_start().starts_with("```");
            if fence || l.trim_start().starts_with("```") {
                continue;
            }
            for s in l.split('`').skip(1).step_by(2) {
                let dir = Path::new(p).parent().filter(|_| s.starts_with("../"));
                let gap = match (&cli, s.strip_prefix("herdsman ")) {
                    (Some(c), Some(rest)) => cli_gap(c, rest),
                    _ if any(&d.skip, s) => None,
                    _ => missing_path(s, dir.unwrap_or(Path::new("."))),
                };
                bad.extend(gap.map(|g| format!("{p}:{}: `{s}`: {g}", i + 1)));
            }
            for t in l.split("](").skip(1).filter_map(|r| r.split([')', ' ', '#']).next()) {
                let dir = Path::new(p).parent().unwrap_or(Path::new("."));
                if !t.is_empty() && !t.contains(':') && !dir.join(t).exists() {
                    bad.push(format!("{p}:{}: link `{t}` does not exist", i + 1));
                }
            }
        }
    }
    bad
}

fn cli_gap(root: &clap::Command, rest: &str) -> Option<String> {
    let mut c = root;
    for (n, w) in rest.split_whitespace().enumerate() {
        if ["|", "||", "&&", ";", ">"].contains(&w) {
            break;
        }
        let w = w.split('|').next().unwrap_or_default();
        if let Some(f) = w.strip_prefix("--") {
            let f = f.split('=').next().unwrap_or_default();
            if !f.is_empty() && !c.get_arguments().any(|a| a.get_long() == Some(f)) {
                return Some(format!("`{}` has no `--{f}`", c.get_name()));
            }
        } else if let Some(s) = c.find_subcommand(w) {
            c = s;
        } else if n == 0 && w.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
            return Some(format!("no herdsman command `{w}`"));
        }
    }
    None
}

fn missing_path(s: &str, root: &Path) -> Option<String> {
    let p = s.trim_start_matches("./");
    let odd = |c: char| c.is_whitespace() || "*<>{}…$~#?|()[]'\",@=".contains(c);
    if p.contains(odd) || p.contains("::") || p.contains("://") {
        return None;
    }
    let (path, line) = match p.rsplit_once(':') {
        Some((a, n)) => (a, Some(n.split('-').next()?.parse::<usize>().ok()?)),
        None => (p, None),
    };
    let first = path.split('/').next().unwrap_or_default();
    if !(path.contains('/') || line.is_some()) || first.is_empty() || !root.join(first).exists() {
        return None;
    }
    let f = root.join(path.trim_end_matches('/'));
    match (f.exists(), line) {
        (false, _) => Some("no such path".into()),
        (true, Some(n)) if std::fs::read_to_string(&f).map_or(0, |t| t.lines().count()) < n => {
            Some(format!("{path} has fewer than {n} lines"))
        }
        _ => None,
    }
}

fn undocumented(d: &Docs, diff: &[Change]) -> Vec<String> {
    let docs: Vec<&str> = diff
        .iter()
        .filter(|c| any(&d.paths, &c.file))
        .map(|c| c.text.as_str())
        .collect();
    let mut seen = BTreeSet::new();
    let mut bad = vec![];
    for c in diff.iter().filter(|c| any(&d.watch, &c.file)) {
        let t = c.text.trim_start().trim_start_matches("pub ");
        let Some((name, rest)) = t.split_once(':') else {
            continue;
        };
        let key = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !key || rest.starts_with(':') || !seen.insert(name) {
            continue;
        }
        let dash = name.replace('_', "-");
        if !docs.iter().any(|x| x.contains(name) || x.contains(&dash)) {
            let (f, n, how) = (&c.file, c.line, if c.added { "added" } else { "removed" });
            bad.push(format!(
                "{f}:{n}: `{name}` {how}, but no changed doc line names it (or `no doc: <why>` in the PR body)"
            ));
        }
    }
    bad
}
