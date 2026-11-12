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
    baseline: Option<String>,
    comments: Option<Comments>,
    docs: Option<Docs>,
    #[serde(default)]
    frozen: Vec<String>,
    #[serde(default)]
    collect: Vec<Collect>,
    #[serde(default)]
    generated: Vec<String>,
    generate: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Collect {
    dir: String,
    glob: Option<String>,
}

impl Collect {
    fn entries(&self) -> String {
        format!(
            "{}/{}",
            self.dir.trim_end_matches('/'),
            self.glob.as_deref().unwrap_or("*")
        )
    }
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

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct Docs {
    paths: Vec<String>,
    watch: Vec<String>,
    skip: Vec<String>,
    cli: bool,
}

struct Change {
    file: String,
    hunk: usize,
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
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ban {
    pattern: String,
    paths: Vec<String>,
    #[serde(default)]
    except: Vec<String>,
    reason: String,
    #[serde(default)]
    comments: bool,
    #[serde(default)]
    ignore_case: bool,
}

impl Ban {
    fn hits(&self, line: &str) -> bool {
        if self.ignore_case {
            line.to_lowercase().contains(&self.pattern.to_lowercase())
        } else {
            line.contains(&self.pattern)
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Size {
    paths: Vec<String>,
    max_file_lines: Option<usize>,
    max_fn_lines: Option<usize>,
    #[serde(default)]
    exclude: Vec<String>,
}

/// Whether a glob matches `p`, `/**/` also matching no folder (`src/**/*.rs` matches `src/lib.rs`).
pub(crate) fn any(globs: &[String], p: &str) -> bool {
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
    anyhow::ensure!(
        a.generated.is_empty() || a.generate.is_some(),
        "{}: `generated` needs `generate`, the command that writes them",
        config.display()
    );
    let base_path = a.baseline.as_deref().unwrap_or(".herdsman/arch-baseline.txt");
    let base_text = std::fs::read_to_string(base_path).unwrap_or_default();
    let crossings: BTreeSet<&str> = base_text
        .lines()
        .map(str::trim)
        .filter(|l| l.contains(" -> "))
        .collect();
    let base: BTreeMap<String, usize> = base_text
        .lines()
        .filter_map(|l| {
            l.trim()
                .rsplit_once(' ')
                .and_then(|(k, n)| Some((k.into(), n.parse().ok()?)))
        })
        .collect();
    let mut files = vec![];
    let ban_paths: Vec<&String> = a.ban.iter().flat_map(|b| &b.paths).collect();
    walk(Path::new("."), &ban_paths, &mut files);
    files.sort();
    let (mut bad, mut now, mut crossed) = (vec![], BTreeMap::<String, usize>::new(), BTreeSet::new());
    for p in &files {
        let ext = Path::new(p).extension().and_then(|x| x.to_str()).unwrap_or_default();
        let rust = ext == "rs";
        let code = rust || ext == "ts" || ext == "tsx";
        let slashes = code
            || [
                "js", "jsx", "mjs", "cjs", "mts", "cts", "swift", "go", "kt", "java", "c", "h", "cpp",
            ]
            .contains(&ext);
        let bans: Vec<&Ban> = a
            .ban
            .iter()
            .filter(|b| any(&b.paths, p) && !any(&b.except, p))
            .collect();
        let Some(src) = (code || !bans.is_empty())
            .then(|| std::fs::read_to_string(p).ok())
            .flatten()
        else {
            continue;
        };
        let lines: Vec<&str> = src.lines().collect();
        let own = a.layer.iter().find(|l| code && any(&l.paths, p) && !any(&l.exclude, p));
        for (i, l) in lines.iter().enumerate() {
            let at = format!("{p}:{}", i + 1);
            let comment = slashes && l.trim_start().starts_with("//");
            for b in bans.iter().filter(|b| b.hits(l) && (b.comments || !comment)) {
                bad.push(format!("{at}: `{}` is banned here: {}", b.pattern, b.reason));
            }
            for (own, t) in own
                .filter(|_| !comment)
                .into_iter()
                .flat_map(|o| targets(p, l, rust).into_iter().map(move |t| (o, t)))
            {
                let to = a.layer.iter().find(|x| uses(x, &t)).filter(|x| x.name != own.name);
                if let Some(to) = to.filter(|x| !own.may_use.contains(&x.name)) {
                    let key = format!("{p} -> {}", to.name);
                    if !crossings.contains(key.as_str()) {
                        let (from, may) = (&own.name, &own.may_use);
                        bad.push(format!(
                            "{at}: layer `{from}` uses `{}` ({t}); its may_use is {may:?}",
                            to.name
                        ));
                    }
                    crossed.insert(key);
                }
            }
        }
        for s in a
            .size
            .iter()
            .filter(|s| code && any(&s.paths, p) && !any(&s.exclude, p))
        {
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
    let mut diff = vec![];
    if let Some(b) = since {
        let o = r.run("git", &argv(&["diff", "-U0", "--no-color", "--no-ext-diff", b]))?;
        anyhow::ensure!(o.code == 0, "git diff {b}: {}", o.stderr.trim());
        diff = changes(&o.stdout);
    }
    bad.extend(frozen_adds(&a, &diff));
    bad.extend(stale_generated(&a, r)?);
    if let Some(c) = a.comments.as_ref().filter(|c| c.policy == Policy::Off) {
        bad.extend(new_comments(&diff, &c.except));
    }
    if let Some(d) = &a.docs {
        bad.extend(stale_docs(d, &files));
        let body = pr_body.map(std::fs::read_to_string).transpose()?;
        if body.is_some_and(|b| !b.contains("no doc:")) {
            bad.extend(undocumented(d, &diff));
        }
    }
    if write {
        let (keep, grown): (Vec<_>, Vec<_>) = crossed
            .iter()
            .partition(|k| crossings.is_empty() || crossings.contains(k.as_str()));
        let sizes = now.iter().map(|(k, n)| format!("{k} {n}\n"));
        std::fs::write(
            base_path,
            sizes.chain(keep.iter().map(|k| format!("{k}\n"))).collect::<String>(),
        )?;
        let (n, c) = (now.len(), keep.len());
        writeln!(out, "wrote {n} size offenders and {c} layer crossings to {base_path}")?;
        for k in &grown {
            writeln!(
                out,
                "{k}: a new layer crossing is not baselined; the listed ones may only shrink"
            )?;
        }
        return Ok(i32::from(!grown.is_empty()));
    }
    for b in &bad {
        writeln!(out, "{b}")?;
    }
    writeln!(out, "arch-check: {} files, {} violations", files.len(), bad.len())?;
    Ok(i32::from(!bad.is_empty()))
}

/// #890: the globs `text`, an arch.toml, lists as frozen or generated: converted, so never a hotspot.
pub fn converted(text: &str) -> Result<Vec<String>> {
    let a: Arch = toml::from_str(text).context("parsing .herdsman/arch.toml")?;
    Ok(a.frozen.into_iter().chain(a.generated).collect())
}

fn frozen_adds(a: &Arch, diff: &[Change]) -> Vec<String> {
    let folders: Vec<String> = a.collect.iter().map(|c| format!("`{}`", c.entries())).collect();
    let instead = match folders.is_empty() {
        true => "a `[[collect]]` folder".to_string(),
        false => folders.join(" or "),
    };
    let collected = |f: &str| a.collect.iter().any(|c| any(&[c.entries()], f));
    let mut hunks = BTreeMap::<usize, (Vec<&Change>, usize)>::new();
    for c in diff.iter().filter(|c| any(&a.frozen, &c.file) && !collected(&c.file)) {
        let h = hunks.entry(c.hunk).or_default();
        if c.added {
            h.0.push(c);
        } else {
            h.1 += 1;
        }
    }
    let new = hunks.into_values().flat_map(|(adds, gone)| adds.into_iter().skip(gone));
    new.map(|c| {
        let (f, n, t) = (&c.file, c.line, c.text.trim());
        format!("{f}:{n}: `{t}` added to a frozen list: add one file per entry in {instead} instead")
    })
    .collect()
}

fn stale_generated(a: &Arch, r: &dyn Runner) -> Result<Vec<String>> {
    let Some(cmd) = a.generate.as_deref().filter(|_| !a.generated.is_empty()) else {
        return Ok(vec![]);
    };
    let before: Vec<Option<Vec<u8>>> = a.generated.iter().map(|p| std::fs::read(p).ok()).collect();
    let o = r.run("sh", &argv(&["-c", cmd]))?;
    if o.code != 0 {
        return Ok(vec![format!("`{cmd}` exited {}: {}", o.code, o.stderr.trim())]);
    }
    let mut bad = vec![];
    for (p, was) in a.generated.iter().zip(before) {
        if std::fs::read(p).ok() == was {
            continue;
        }
        bad.push(format!(
            "{p}: differs from what `{cmd}` writes: run it and commit the output, never hand-merge a generated file"
        ));
        match was {
            Some(b) => std::fs::write(p, b)?,
            None => std::fs::remove_file(p)?,
        }
    }
    Ok(bad)
}

fn walk(dir: &Path, bans: &[&String], out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let rel: String = e.path().to_string_lossy().trim_start_matches("./").into();
        let named = bans.iter().any(|b| b.starts_with(&format!("{rel}/")));
        if (name.starts_with('.') && !named) || name.starts_with("target") || name == "node_modules" {
            continue;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&e.path(), bans, out),
            Ok(_) => out.push(rel),
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
    let (mut v, mut from, mut to, mut old, mut new, mut head, mut hunk) = (vec![], "", "", 0, 0, true, 0);
    for l in diff.lines() {
        if l.starts_with("diff --git ") {
            head = true;
        } else if let Some(h) = l.strip_prefix("@@ -") {
            let num = |s: &str| s.split([',', ' ']).next().and_then(|n| n.parse().ok()).unwrap_or(0);
            let (o, n) = h.split_once(" +").unwrap_or_default();
            (old, new, head, hunk) = (num(o), num(n), false, hunk + 1);
        } else if head {
            from = l.strip_prefix("--- a/").unwrap_or(from);
            to = l.strip_prefix("+++ b/").unwrap_or(to);
        } else if let Some((sign @ ("+" | "-"), text)) = l.split_at_checked(1) {
            let added = sign == "+";
            let (file, line) = if added { (to, &mut new) } else { (from, &mut old) };
            v.push(Change {
                file: file.into(),
                hunk,
                line: *line,
                added,
                text: text.into(),
            });
            *line += 1;
        }
    }
    v
}

fn comment_lines(lines: &[&str], rust: bool) -> Vec<bool> {
    let (mut block, mut quote, mut raw) = (false, None::<char>, None::<usize>);
    let each = lines.iter().map(|l| {
        let s: Vec<char> = l.chars().collect();
        let (mut c, mut i) = (block, 0);
        while i < s.len() {
            let (ch, next) = (s[i], s.get(i + 1).copied());
            if block {
                block = !(ch == '*' && next == Some('/'));
                i += if block { 1 } else { 2 };
            } else if let Some(n) = raw {
                let end = ch == '"' && s[i + 1..].iter().take(n).filter(|&&x| x == '#').count() == n;
                raw = if end { None } else { raw };
                i += if end { n + 1 } else { 1 };
            } else if let Some(q) = quote {
                quote = if ch == q { None } else { quote };
                i += if ch == '\\' { 2 } else { 1 };
            } else if ch == '/' && next == Some('/') {
                c = true;
                break;
            } else if ch == '/' && next == Some('*') {
                (c, block, i) = (true, true, i + 2);
            } else if rust && ch == 'r' && matches!(next, Some('"' | '#')) && (i == 0 || !s[i - 1].is_alphanumeric()) {
                let n = s[i + 1..].iter().take_while(|&&x| x == '#').count();
                raw = (s.get(i + 1 + n) == Some(&'"')).then_some(n);
                i += if raw.is_some() { n + 2 } else { 1 };
            } else if ch == '"' || (!rust && matches!(ch, '\'' | '`')) {
                (quote, i) = (Some(ch), i + 1);
            } else if rust && ch == '\'' && (next == Some('\\') || s.get(i + 2) == Some(&'\'')) {
                let close = s.get(i + 3..).and_then(|r| r.iter().position(|&x| x == '\''));
                i += close.filter(|_| next == Some('\\')).map_or(3, |p| p + 4);
            } else {
                i += 1;
            }
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
    let code = |f: &str| [".rs", ".ts", ".tsx"].iter().any(|x| f.ends_with(x)) && !any(except, f);
    let (mut added, mut bad) = (BTreeMap::<&str, Vec<usize>>::new(), vec![]);
    for c in diff.iter().filter(|c| c.added && code(&c.file)) {
        added.entry(&c.file).or_default().push(c.line.saturating_sub(1));
    }
    for (f, at) in added {
        let src = std::fs::read_to_string(f).unwrap_or_default();
        let lines: Vec<&str> = src.lines().collect();
        let com = comment_lines(&lines, Path::new(f).extension().is_some_and(|x| x == "rs"));
        for i in at
            .into_iter()
            .filter(|&i| com.get(i) == Some(&true) && !allowed_comment(&lines, &com, i))
        {
            bad.push(format!(
                "{f}:{}: a new comment (only `// SAFETY:` over unsafe and one `///` on a pub item): put the why in `herdsman why`",
                i + 1
            ));
        }
    }
    bad
}

fn stale_docs(d: &Docs, files: &[String]) -> Vec<String> {
    let mut cli = d.cli.then(<crate::cli::Cli as clap::CommandFactory>::command);
    if let Some(c) = &mut cli {
        c.build();
    }
    let cli_gap = |rest: &str| -> Option<String> {
        let mut c = cli.as_ref()?;
        let stop = |w: &&str| ["|", "||", "&&", ";", ">"].contains(w);
        for (n, w) in rest.split_whitespace().take_while(|w| !stop(w)).enumerate() {
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
    };
    let mut bad = vec![];
    for p in files.iter().filter(|p| any(&d.paths, p)) {
        let (mut fence, dir) = (false, Path::new(p).parent().unwrap_or(Path::new(".")));
        for (i, l) in std::fs::read_to_string(p).unwrap_or_default().lines().enumerate() {
            fence ^= l.trim_start().starts_with("```");
            if fence || l.trim_start().starts_with("```") {
                continue;
            }
            for s in l.split('`').skip(1).step_by(2) {
                let gap = match (&cli, s.strip_prefix("herdsman ")) {
                    (Some(_), Some(rest)) => cli_gap(rest),
                    _ if any(&d.skip, s) => None,
                    _ => missing_path(s, if s.starts_with("../") { dir } else { Path::new(".") }),
                };
                bad.extend(gap.map(|g| format!("{p}:{}: `{s}`: {g}", i + 1)));
            }
            for t in l.split("](").skip(1).filter_map(|r| r.split([')', ' ', '#']).next()) {
                if !t.is_empty() && !t.contains(':') && !dir.join(t).exists() {
                    bad.push(format!("{p}:{}: link `{t}` does not exist", i + 1));
                }
            }
        }
    }
    bad
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
    if !f.exists() {
        return Some("no such path".into());
    }
    let n = line?;
    let short = std::fs::read_to_string(&f).map_or(0, |t| t.lines().count()) < n;
    short.then(|| format!("{path} has fewer than {n} lines"))
}

fn undocumented(d: &Docs, diff: &[Change]) -> Vec<String> {
    let docs = |n: &str| diff.iter().any(|c| any(&d.paths, &c.file) && c.text.contains(n));
    let key = |n: &str| !n.is_empty() && n.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'));
    let (mut seen, mut bad) = (BTreeSet::new(), vec![]);
    for c in diff.iter().filter(|c| any(&d.watch, &c.file)) {
        let t = c.text.trim_start().trim_start_matches("pub ");
        let Some((name, _)) = t.split_once(':').filter(|(n, r)| key(n) && !r.starts_with(':')) else {
            continue;
        };
        if seen.insert(name) && !docs(name) && !docs(&name.replace('_', "-")) {
            let (f, n, how) = (&c.file, c.line, if c.added { "added" } else { "removed" });
            bad.push(format!(
                "{f}:{n}: `{name}` {how}, but no changed doc line names it (or `no doc: <why>` in the PR body)"
            ));
        }
    }
    bad
}
