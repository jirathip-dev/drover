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
/// CI's names list for `hygiene check`, one name a line: gatekeeper passes its `private-names` secret here.
pub const NAMES_ENV: &str = "DROVER_PRIVATE_NAMES";
/// The Actions secret `hygiene push` writes in a fleet's public repo.
pub const NAMES_SECRET: &str = "PRIVATE_NAMES";
const VISIBILITY_SECS: u64 = 86_400;

/// A public fleet's patterns beyond the built-in home paths: its hosts, private names, session folders and the agent session ids it knows.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Check {
    pub repo: String,
    pub hosts: Vec<String>,
    pub private: Vec<String>,
    pub dirs: Vec<String>,
    #[serde(default)]
    pub private_repos: Vec<String>,
    /// Folders whose file names carry this host's agent session ids: each provider's session folder.
    #[serde(default)]
    pub session_dirs: Vec<String>,
    /// The event log whose `session` fields are agent session ids.
    #[serde(default)]
    pub events: Option<String>,
    /// The fleet's clone and integration branch: a UUID already there is public.
    #[serde(default)]
    pub tree: Option<(String, String)>,
}

impl Check {
    /// The first pattern `text` hits, by name (`home-path`, `private-name #2`); never the value.
    pub fn find(&self, text: &str) -> Option<String> {
        let low = text.to_lowercase();
        let name = |c: char| c.is_alphanumeric() || c == '_';
        let under_but = |d: &str, but: Option<&str>| {
            (low.match_indices(&d.to_lowercase())).any(|(i, m)| {
                low.get(i + m.len()..).is_some_and(|r| {
                    r.starts_with(name)
                        && !but
                            .and_then(|b| r.strip_prefix(b))
                            .is_some_and(|t| !t.starts_with(name))
                })
            })
        };
        let under = |d: &str| under_but(d, None);
        let uuid = uuids(text).any(|u| self.session(&u));
        let hit = if ["/users/", "/home/"].iter().any(|d| under_but(d, Some("user"))) {
            "home-path".to_string()
        } else if self.dirs.iter().any(|d| under(d)) {
            "session-path".into()
        } else if uuid {
            "session-id".into()
        } else if self.hosts.iter().any(|h| hostname(&low, h)) {
            "host".into()
        } else {
            named(&self.private, &low)?
        };
        Some(hit)
    }

    fn session(&self, id: &str) -> bool {
        let logged = || {
            (self.events.as_deref().map(Path::new))
                .filter(|p| crate::db::file(p).exists())
                .and_then(|p| crate::events::read(p).ok())
                .is_some_and(|l| {
                    l.iter()
                        .any(|e| e.str("session").is_some_and(|s| s.eq_ignore_ascii_case(id)))
                })
        };
        let public = || {
            self.tree.as_ref().is_some_and(|(dir, branch)| {
                let at = format!("origin/{branch}");
                ok(&crate::runner::Real, "git", &["-C", dir, "grep", "-qiF", "-e", id, &at]).is_ok()
            })
        };
        (self.session_dirs.iter().any(|d| filed(Path::new(d), id, 4)) || logged()) && !public()
    }
}

fn uuids(text: &str) -> impl Iterator<Item = String> + '_ {
    (text.split(|c: char| !c.is_ascii_hexdigit() && c != '-')).flat_map(|w| {
        let parts: Vec<&str> = w.split('-').collect();
        (parts.windows(5))
            .filter(|p| p.iter().map(|s| s.len()).eq([8, 4, 4, 4, 12]))
            .map(|p| p.join("-").to_lowercase())
            .collect::<Vec<_>>()
    })
}

fn filed(dir: &Path, id: &str, depth: u8) -> bool {
    (std::fs::read_dir(dir).into_iter().flatten().flatten()).any(|e| {
        e.file_name().to_string_lossy().to_lowercase().contains(id)
            || depth
                .checked_sub(1)
                .is_some_and(|d| e.file_type().is_ok_and(|t| t.is_dir()) && filed(&e.path(), id, d))
    })
}

fn named(names: &[String], low: &str) -> Option<String> {
    let i = names.iter().position(|p| word(low, p))?;
    Some(format!("private-name #{}", i + 1))
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
        private_repos: (cfg.fleet.iter())
            .filter(|(n, _)| !public(ctx, None, cfg, n))
            .map(|(_, f)| slug(&f.repo))
            .collect(),
        session_dirs: (crate::providers::all().filter_map(|p| p.sessions()))
            .map(|d| home.join(d).to_string_lossy().into_owned())
            .collect(),
        events: Some(ctx.state.to_string_lossy().into_owned()),
        tree: Some((f.local_path().to_string_lossy().into_owned(), f.integration.clone())),
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

/// `drover hygiene`'s verbs.
#[derive(clap::Subcommand)]
pub enum Cmd {
    #[command(
        about = "Report only: hits in the repo's issues, PR bodies, comments and history, by pattern with links"
    )]
    Scan { fleet: String },
    #[command(
        about = "#1483: the tree, the commit messages since --base and the --pr-body against the private-names list (the fleet's, else $DROVER_PRIVATE_NAMES); exit 1 on a hit"
    )]
    Check {
        #[arg(help = "The fleet whose private_names to read (default: the lane's)")]
        fleet: Option<String>,
        #[arg(long, help = "The PR's base revision: the commit messages since it are checked")]
        base: Option<String>,
        #[arg(long, help = "A file holding the PR's body")]
        pr_body: Option<std::path::PathBuf>,
    },
    #[command(
        about = "#1483: Owner only (a run card): write the fleet's private_names as its public repo's PRIVATE_NAMES Actions secret; never printed"
    )]
    Push { fleet: String },
}

fn slug(repo: &str) -> String {
    let r = repo.trim().trim_end_matches('/').trim_end_matches(".git");
    let mut parts = r.rsplit(['/', ':']);
    let (name, owner) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
    format!("{owner}/{name}").to_lowercase()
}

/// The hook's check of a lane's `gh issue|pr create|edit|comment|review|close` or writing `gh api`: its title, body and body file, bound for `--repo`/`-R`, else `origin`'s remote of the dir after any `cd`, else the fleet's repo.
pub fn gh_text(c: &Check, cmd: &str, cwd: &str, origin: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut dir = cwd.to_string();
    for seg in segments(cmd) {
        if let [cd, to, ..] = seg.as_slice() {
            if cd == "cd" || cd == "pushd" {
                dir = crate::hook::resolve(to, &dir);
            }
        }
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
        let flagged = rest.iter().enumerate().find_map(|(j, w)| match w.as_str() {
            "-R" | "--repo" => rest.get(j + 1).cloned(),
            w => w.strip_prefix("--repo=").map(String::from),
        });
        let repo = flagged
            .or_else(|| origin(&dir))
            .map_or_else(|| c.repo.clone(), |r| slug(&r));
        if c.private_repos.contains(&slug(&repo)) {
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
                return Some(why(&repo, &format!("gh {sub} {verb} {flag}"), &p));
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

/// `drover hygiene`'s verbs; `check` needs no fleet.toml.
pub fn run(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, cmd: Cmd) -> Result<i32> {
    match cmd {
        Cmd::Check { fleet, base, pr_body } => {
            check(ctx, r, out, fleet.as_deref(), base.as_deref(), pr_body.as_deref())
        }
        Cmd::Scan { fleet } => scan(ctx, &Config::load(&ctx.config)?, r, out, &fleet).map(|()| 0),
        Cmd::Push { fleet } => push(ctx, &Config::load(&ctx.config)?, r, out, &fleet).map(|()| 0),
    }
}

/// `drover hygiene check`: each private name in the tracked files, the commit messages since `base` and the PR body, as `private-name #N at file:line`; 1 on a hit, or `list-missing` for a public repo with no list.
pub fn check(
    ctx: &Ctx,
    r: &dyn Runner,
    out: &mut dyn Write,
    fleet: Option<&str>,
    base: Option<&str>,
    body: Option<&Path>,
) -> Result<i32> {
    let cfg = Config::load(&ctx.config).ok();
    let lane = crate::config::env("DROVER_LANE");
    let fleet = fleet.or_else(|| lane.as_deref()?.split(':').next());
    let own = cfg.as_ref().zip(fleet).and_then(|(c, n)| Some((c, n, c.fleet.get(n)?)));
    if let Some((_, n, f)) = own.filter(|(c, n, _)| !public(ctx, Some(r), c, n)) {
        writeln!(out, "{} ({n}) is private: nothing to check", f.repo)?;
        return Ok(0);
    }
    let list = |l: &str| {
        l.lines()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(String::from)
            .collect::<Vec<_>>()
    };
    let names = crate::config::env(NAMES_ENV)
        .map(|v| list(&v))
        .filter(|v| !v.is_empty())
        .or_else(|| own.map(|(_, _, f)| list(&f.private_names.join("\n"))))
        .unwrap_or_default();
    if names.is_empty() {
        writeln!(
            out,
            "list-missing: no private-names list for a public repo: set ${NAMES_ENV} (CI) or [fleet.<n>] private_names"
        )?;
        return Ok(1);
    }
    let find = |l: &str| named(&names, &l.to_lowercase());
    let mut rows = vec![];
    for p in ok(r, "git", &["ls-files", "-z"])?.split('\0').filter(|p| !p.is_empty()) {
        let text = std::fs::read_to_string(p).unwrap_or_default();
        rows.extend((text.lines().zip(1..)).filter_map(|(l, n)| Some(format!("{} at {p}:{n}", find(l)?))));
    }
    if let Some(b) = base {
        let log = ok(r, "git", &["log", &format!("{b}..HEAD")])?;
        rows.extend(
            hits(&log, find)
                .into_iter()
                .map(|(s, _, h)| format!("{h} at commit {s:.7} message")),
        );
    }
    if let Some(b) = body {
        let text = std::fs::read_to_string(b).map_err(|e| anyhow::anyhow!("{}: {e}", b.display()))?;
        rows.extend((text.lines().zip(1..)).filter_map(|(l, n)| Some(format!("{} at PR body:{n}", find(l)?))));
    }
    for row in &rows {
        writeln!(out, "{row}")?;
    }
    writeln!(
        out,
        "{} hit(s) of {} private name(s); names are never printed",
        rows.len(),
        names.len()
    )?;
    Ok(i32::from(!rows.is_empty()))
}

/// `drover hygiene push`: fleet `name`'s `private_names` as its public repo's [`NAMES_SECRET`], sent on stdin, never printed.
pub fn push(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, name: &str) -> Result<()> {
    let f = cfg.fleet(name)?;
    anyhow::ensure!(
        public(ctx, Some(r), cfg, name),
        "{} is private: it needs no {NAMES_SECRET} secret",
        f.repo
    );
    let names: Vec<&str> = (f.private_names.iter())
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
        .collect();
    anyhow::ensure!(
        !names.is_empty(),
        "[fleet.{name}] has no private_names: nothing to push"
    );
    let args = crate::runner::argv(&["secret", "set", NAMES_SECRET, "-R", &f.repo]);
    let o = r.run_in("gh", &args, &names.join("\n"))?;
    anyhow::ensure!(
        o.code == 0,
        "gh secret set {NAMES_SECRET} -R {} failed ({})",
        f.repo,
        o.code
    );
    Ok(writeln!(
        out,
        "wrote {} name(s) to {}'s {NAMES_SECRET} secret",
        names.len(),
        f.repo
    )?)
}
