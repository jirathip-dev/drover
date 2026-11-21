use crate::config::Config;
use crate::events::Json;
use crate::runner::{json, ok, Runner};
use crate::Ctx;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

pub const CODE: &str = "PUBLIC_LEAK";
const VISIBILITY_SECS: u64 = 86_400;

/// A public fleet's patterns beyond the built-in home paths and session ids: its hosts, private names and session folders.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Check {
    pub repo: String,
    pub hosts: Vec<String>,
    pub private: Vec<String>,
    pub dirs: Vec<String>,
}

impl Check {
    /// The first pattern `text` hits, by name (`home-path`, `private-name #2`); never the value.
    pub fn find(&self, text: &str) -> Option<String> {
        let low = text.to_lowercase();
        let name = |c: char| c.is_alphanumeric() || c == '_';
        let under = |d: &str| {
            (low.match_indices(&d.to_lowercase()))
                .any(|(i, m)| low.get(i + m.len()..).is_some_and(|r| r.starts_with(name)))
        };
        let uuid = (text.split(|c: char| !c.is_ascii_hexdigit() && c != '-')).any(|w| {
            w.split('-')
                .map(str::len)
                .collect::<Vec<_>>()
                .windows(5)
                .any(|l| l == [8, 4, 4, 4, 12])
        });
        let hit = if ["/users/", "/home/"].iter().any(|d| under(d)) {
            "home-path".to_string()
        } else if self.dirs.iter().any(|d| under(d)) {
            "session-path".into()
        } else if uuid {
            "session-id".into()
        } else if self.hosts.iter().any(|h| hostname(&low, h)) {
            "host".into()
        } else {
            let i = self.private.iter().position(|p| word(&low, p))?;
            format!("private-name #{}", i + 1)
        };
        Some(hit)
    }
}

fn word(low: &str, n: &str) -> bool {
    word_where(low, n, |_, _| true)
}

fn word_where(low: &str, n: &str, used: impl Fn(&str, &str) -> bool) -> bool {
    let n = n.trim().to_lowercase();
    let edge = |c: Option<char>| !c.is_some_and(char::is_alphanumeric);
    !n.is_empty()
        && low.match_indices(&n).any(|(i, _)| {
            let (b, a) = (low.get(..i).unwrap_or(""), low.get(i + n.len()..).unwrap_or(""));
            edge(b.chars().next_back()) && edge(a.chars().next()) && used(b, a)
        })
}

fn hostname(low: &str, n: &str) -> bool {
    let then = |a: &str, c: char, ok: fn(&char) -> bool| {
        a.strip_prefix(c).and_then(|r| r.chars().next()).is_some_and(|x| ok(&x))
    };
    n.contains(['@', '.']) && word(low, n)
        || word_where(low, n, |b, a| {
            ["@", "ssh ", "--host ", "--host=", "hosts."]
                .iter()
                .any(|p| b.ends_with(p))
                || then(a, ':', |c| !c.is_whitespace())
                || then(a, '.', char::is_ascii_alphanumeric)
        })
}

/// The refusal line: where, which pattern, never the value.
pub fn why(repo: &str, at: &str, pattern: &str) -> String {
    format!("REFUSED {CODE}: {at} has a {pattern}, and {repo} is public: reword it, nothing was sent (#1376)")
}

/// Fleet `name`'s repo is public: its `public` flag, else GitHub's visibility cached a day, read by `r` (the hook only reads the cache; public when unknown).
pub fn public(ctx: &Ctx, r: Option<&dyn Runner>, cfg: &Config, name: &str) -> bool {
    let Some(f) = cfg.fleet.get(name) else { return true };
    if let Some(p) = f.public {
        return p;
    }
    let p = crate::gh::cache_path(ctx, "visibility", name, 0);
    let c = crate::usage::load::<Value>(&p).unwrap_or_default();
    let age = ctx.now.checked_sub(c.at("ts").as_u64().unwrap_or(0));
    let fresh = r.is_none() || age < Some(VISIBILITY_SECS);
    let cached = c.at("public").as_bool().filter(|_| c.at("repo") == &f.repo && fresh);
    let Some(r) = r.filter(|_| cached.is_none()) else {
        return cached.unwrap_or(true);
    };
    let v = json(r, "gh", &["repo", "view", &f.repo, "--json", "visibility"]);
    let public = v.map_or(true, |v| {
        !matches!(v.at("visibility").as_str(), Some("PRIVATE" | "INTERNAL"))
    });
    crate::gh::store(&p, &json!({"ts": ctx.now, "repo": f.repo, "public": public})).ok();
    public
}

/// Fleet `name`'s check, or None when its repo is private.
pub fn of(ctx: &Ctx, r: Option<&dyn Runner>, cfg: &Config, name: &str) -> Option<Check> {
    let f = cfg.fleet.get(name).filter(|_| public(ctx, r, cfg, name))?;
    let home = crate::config::home();
    let rel = |p: &Path| {
        format!(
            "{}/",
            p.strip_prefix(&home)
                .unwrap_or(p)
                .to_string_lossy()
                .trim_end_matches('/')
        )
    };
    let state = ctx.state.parent().unwrap_or(&ctx.state);
    let hosts = cfg.hosts.iter().flat_map(|(k, h)| {
        let m = h
            .machine
            .as_deref()
            .map(|m| m.rsplit('@').next().unwrap_or(m).to_string());
        std::iter::once(k.clone()).chain(m)
    });
    Some(Check {
        repo: f.repo.clone(),
        hosts: hosts.collect(),
        private: f.private_names.clone(),
        dirs: vec![rel(state), rel(&crate::orchestrator::dir(ctx, cfg, ""))],
    })
}

/// `PUBLIC_LEAK` when `text`, bound for fleet `name`'s public repo as `at`, hits a pattern.
pub fn text(ctx: &Ctx, r: &dyn Runner, cfg: &Config, name: &str, at: &str, text: &str) -> Result<()> {
    match of(ctx, Some(r), cfg, name).and_then(|c| Some((c.find(text)?, c.repo))) {
        Some((p, repo)) => {
            let msg = why(&repo, at, &p);
            crate::gate::refused(CODE, msg.split_once(": ").map_or(msg.as_str(), |m| m.1))
        }
        None => Ok(()),
    }
}

/// Each `--title`, `--subject` and `--body` of `gh args` for fleet `name` through [`text`].
pub fn args(ctx: &Ctx, r: &dyn Runner, cfg: &Config, name: &str, args: &[&str]) -> Result<()> {
    for w in args.windows(2) {
        if let [f @ ("--title" | "--subject" | "--body"), t] = w {
            let verb = args.get(..2).unwrap_or_default().join(" ");
            text(ctx, r, cfg, name, &format!("gh {verb} {f}"), t)?;
        }
    }
    Ok(())
}

/// `gh args` for fleet `name`, once [`args`] passes.
pub fn gh(ctx: &Ctx, r: &dyn Runner, cfg: &Config, name: &str, a: &[&str]) -> Result<String> {
    args(ctx, r, cfg, name, a)?;
    ok(r, "gh", a)
}

/// Each hit in a `git log -p` or `git diff`'s added lines and commit messages: (commit, `file:line` or `message`, pattern).
pub fn hits(log: &str, find: impl Fn(&str) -> Option<String>) -> Vec<(String, String, String)> {
    let (mut head, mut msg, mut file, mut n, mut sha) = (false, false, "", 0, "");
    let mut out = vec![];
    for l in log.lines() {
        if let Some(s) = l
            .strip_prefix("commit ")
            .filter(|s| s.len() >= 40 && s.bytes().take(40).all(|b| b.is_ascii_hexdigit()))
        {
            (sha, msg, head) = (s.get(..40).unwrap_or(s), true, false);
            continue;
        }
        head = (head || l.starts_with("diff --git ")) && !l.starts_with("@@");
        msg &= !head;
        if msg {
            out.extend(
                l.strip_prefix("    ")
                    .and_then(&find)
                    .map(|p| (sha.into(), "message".into(), p)),
            );
        } else if let Some((_, h)) = l.strip_prefix("@@ ").and_then(|h| h.split_once('+')) {
            n = h.split([',', ' ']).next().and_then(|s| s.parse().ok()).unwrap_or(1);
        } else if head {
            file = l.strip_prefix("+++ b/").unwrap_or(file);
        } else if let Some(p) = l.strip_prefix('+').and_then(&find) {
            out.push((sha.into(), format!("{file}:{n}"), p));
        }
        if !head && !msg && l.starts_with(['+', ' ']) {
            n += 1;
        }
    }
    out
}

/// The pre-push check: the first hit in the commits `sha` pushes (`git log -p`, messages included), as its refusal.
pub fn pushed(r: &dyn Runner, c: &Check, sha: &str) -> Option<String> {
    let log = ok(r, "git", &["log", "-p", sha, "--not", "--remotes"]).ok()?;
    let (s, at, p) = hits(&log, |l| c.find(l)).into_iter().next()?;
    Some(why(&c.repo, &format!("commit {s:.7} {at}"), &p))
}

/// `drover hygiene`'s verbs.
#[derive(clap::Subcommand)]
pub enum Cmd {
    #[command(
        about = "Report only: hits in the repo's issues, PR bodies, comments and history, by pattern with links"
    )]
    Scan { fleet: String },
}

/// The hook's check of a lane's `gh issue|pr create|edit|comment|review|close` or writing `gh api`: its title, body and body file.
pub fn gh_text(c: &Check, cmd: &str, cwd: &str) -> Option<String> {
    for seg in segments(cmd) {
        let Some(i) = seg.iter().position(|w| w == "gh" || w.ends_with("/gh")) else {
            continue;
        };
        let rest = seg.get(i + 1..).unwrap_or_default();
        let (sub, verb) = (
            rest.first().map_or("", String::as_str),
            rest.get(1).map_or("", String::as_str),
        );
        let api = sub == "api";
        let writes =
            matches!(sub, "issue" | "pr") && matches!(verb, "create" | "edit" | "comment" | "review" | "close");
        if !(api || writes) {
            continue;
        }
        for (j, w) in rest.iter().enumerate() {
            let (flag, inline) = w
                .split_once('=')
                .filter(|_| w.starts_with("--"))
                .map_or((w.as_str(), None), |(f, v)| (f, Some(v)));
            let val = inline
                .map(String::from)
                .or_else(|| rest.get(j + 1).cloned())
                .unwrap_or_default();
            let text = match flag {
                "-t" | "--title" | "-b" | "--body" => val,
                "-f" | "-F" | "--field" | "--raw-field" if api => {
                    let v = val.split_once('=').map_or(val.as_str(), |x| x.1);
                    v.strip_prefix('@').map_or_else(|| v.to_string(), |p| read(p, cwd, cmd))
                }
                "-F" | "--body-file" | "--input" => read(&val, cwd, cmd),
                _ => continue,
            };
            if let Some(p) = c.find(&text) {
                return Some(why(&c.repo, &format!("gh {sub} {verb} {flag}"), &p));
            }
        }
    }
    None
}

fn read(p: &str, cwd: &str, cmd: &str) -> String {
    if p == "-" {
        return cmd.split_once("<<").map_or("", |x| x.1).to_string();
    }
    let p = p
        .strip_prefix("~/")
        .map_or_else(|| p.to_string(), |r| format!("$HOME/{r}"));
    let p = match p.strip_prefix('$') {
        Some(r) => {
            let r = r.strip_prefix('{').unwrap_or(r);
            let n = r.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(r.len());
            let (name, rest) = r.split_at(n);
            format!(
                "{}{}",
                crate::config::env(name).unwrap_or_default(),
                rest.trim_start_matches('}')
            )
        }
        None => p,
    };
    std::fs::read_to_string(Path::new(cwd).join(p)).unwrap_or_default()
}

fn segments(cmd: &str) -> Vec<Vec<String>> {
    let (mut segs, mut words, mut cur, mut q, mut quoted) = (vec![], vec![], String::new(), None, false);
    let mut cs = cmd.chars();
    while let Some(c) = cs.next() {
        match (q, c) {
            (Some(x), c) if c == x => q = None,
            (Some('"') | None, '\\') => cur.extend(cs.next()),
            (None, '\'' | '"') => (q, quoted) = (Some(c), true),
            (None, c) if c.is_whitespace() || ";|&".contains(c) => {
                if std::mem::take(&mut quoted) || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
                if !matches!(c, ' ' | '\t') && !words.is_empty() {
                    segs.push(std::mem::take(&mut words));
                }
            }
            (_, c) => cur.push(c),
        }
    }
    words.extend((!cur.is_empty() || quoted).then_some(cur));
    segs.extend((!words.is_empty()).then_some(words));
    segs
}

/// Doctor's one warning: the public fleets with no `private_names` list (the generic patterns still run).
pub fn doctor(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Option<String> {
    let bare: Vec<&str> = (cfg.fleet.iter())
        .filter(|(n, f)| f.private_names.is_empty() && public(ctx, Some(r), cfg, n))
        .map(|(n, _)| n.as_str())
        .collect();
    (!bare.is_empty()).then(|| {
        format!(
            "! hygiene — public fleet(s) with no private_names list: {} → set [fleet.<n>] private_names",
            bare.join(", ")
        )
    })
}

/// `drover hygiene scan`: each hit in the repo's issues, PR bodies, comments and integration history, by pattern with its link; changes nothing.
pub fn scan(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, name: &str) -> Result<()> {
    let f = cfg.fleet(name)?;
    let Some(c) = of(ctx, Some(r), cfg, name) else {
        return Ok(writeln!(out, "{} is private: nothing to check", f.repo)?);
    };
    let mut rows = vec![];
    for kind in ["issue", "pr"] {
        let fields = ["title,body,url,comments"];
        let v = json(
            r,
            "gh",
            &[
                &[
                    kind, "list", "-R", &f.repo, "--state", "all", "--limit", "1000", "--json",
                ][..],
                &fields,
            ]
            .concat(),
        )?;
        for i in v.as_array().into_iter().flatten() {
            let cs = i.at("comments").as_array().into_iter().flatten();
            let texts = [(i, "title"), (i, "body")].into_iter().chain(cs.map(|c| (c, "body")));
            for (t, k) in texts {
                rows.extend(
                    c.find(t.at(k).as_str().unwrap_or_default())
                        .map(|p| (p, t.at("url").as_str().unwrap_or_default().to_string())),
                );
            }
        }
    }
    let dir = f.local_path().to_string_lossy().into_owned();
    let log = ok(
        r,
        "git",
        &["-C", &dir, "log", "-p", &format!("origin/{}", f.integration)],
    )?;
    for (sha, at, p) in hits(&log, |l| c.find(l)) {
        rows.push((p, format!("https://github.com/{}/commit/{sha} {at}", f.repo)));
    }
    rows.sort();
    for (p, link) in &rows {
        writeln!(out, "{p} · {link}")?;
    }
    Ok(writeln!(out, "{} hit(s) in {}; nothing changed", rows.len(), f.repo)?)
}
