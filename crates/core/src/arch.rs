//! #734: `herdsman arch-check`: a repo's architecture rules in `.herdsman/arch.toml`, checked by plain source scanning
//! of Rust and TypeScript: a layer uses only the layers its `may_use` lists, a banned pattern appears only in its
//! exceptions, and a file or function over its size limit fails unless the baseline lists it at that size or more.
use crate::config::glob;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
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
pub fn check(config: &Path, write: bool, out: &mut dyn Write) -> Result<i32> {
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
