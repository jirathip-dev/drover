use crate::runner::{argv, Runner};
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::time::Duration;

/// A published build of `sha`: its tag and each asset's sha256 from `SHA256SUMS`.
pub struct Release {
    pub sha: String,
    pub tag: String,
    sums: BTreeMap<String, String>,
}

/// The head's release, or why there is none yet; either way nothing is built.
pub enum Found {
    Release(Release),
    Missing(String),
}

/// One host's download: the asset's public URL and its expected sha256.
pub struct Asset {
    pub url: String,
    pub sha256: String,
}

/// The full shas a host's live binary and its `drover.new` report (None: missing, or no sha).
#[derive(Debug, Default, Clone)]
pub struct Builds {
    pub live: Option<String>,
    pub staged: Option<String>,
}

/// Why a host's stage failed; its `drover.new` is removed and the live binary is untouched.
#[derive(Debug)]
pub enum Unstaged {
    NoAsset(String),
    Download(String),
    Checksum,
    Version(String),
    Doctor(String),
}

impl std::fmt::Display for Unstaged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unstaged::NoAsset(t) => write!(f, "no release asset for {t}"),
            Unstaged::Download(w) => write!(f, "download: {w}"),
            Unstaged::Checksum => write!(f, "checksum: the download does not match SHA256SUMS"),
            Unstaged::Version(w) => write!(f, "version: {w}"),
            Unstaged::Doctor(w) => write!(f, "doctor --config-only: {w}"),
        }
    }
}

/// The release tag of `sha` on `branch`: `build-<sha7>` for main, `build-<branch>-<sha7>` for another.
pub fn tag(branch: &str, sha: &str) -> String {
    let short = sha.get(..7).unwrap_or(sha);
    if branch == "main" {
        format!("build-{short}")
    } else {
        format!("build-{branch}-{short}")
    }
}

/// The release of `sha` on `branch` in `repo`, read from its `SHA256SUMS` by one `gh release download`.
pub fn find(r: &dyn Runner, repo: &str, branch: &str, sha: &str) -> Found {
    let tag = tag(branch, sha);
    let args = ["release", "download", &tag, "-R", repo, "-p", "SHA256SUMS", "-O", "-"];
    let o = match r.run("gh", &argv(&args)) {
        Ok(o) if o.code == 0 => o,
        Ok(o) => return Found::Missing(format!("no release {tag}: {}", o.stderr.trim())),
        Err(e) => return Found::Missing(format!("no release {tag}: {e:#}")),
    };
    let sums: BTreeMap<String, String> = o
        .stdout
        .lines()
        .filter_map(|l| {
            let (h, n) = l.split_once(char::is_whitespace)?;
            Some((n.trim().trim_start_matches('*').to_string(), h.to_string()))
        })
        .collect();
    if sums.is_empty() {
        return Found::Missing(format!("{tag} lists no asset in SHA256SUMS"));
    }
    Found::Release(Release {
        sha: sha.into(),
        tag,
        sums,
    })
}

impl Release {
    /// The asset for a host with probed `tags` (`os:macos` + `arch:arm64`, or `os:linux` + `arch:x86_64`).
    pub fn asset(&self, repo: &str, tags: &[String]) -> Option<Asset> {
        let has = |t: &str| tags.iter().any(|x| x == t);
        let triple = match (
            has("os:macos") && has("arch:arm64"),
            has("os:linux") && has("arch:x86_64"),
        ) {
            (true, _) => "aarch64-apple-darwin",
            (_, true) => "x86_64-unknown-linux-gnu",
            _ => return None,
        };
        let name = format!("drover-{triple}");
        Some(Asset {
            url: format!("https://github.com/{repo}/releases/download/{}/{name}", self.tag),
            sha256: self.sums.get(&name)?.clone(),
        })
    }
}

/// The 40-hex sha in a `drover <version> (<sha>)` line.
pub fn sha_of(v: &str) -> Option<String> {
    let s = v.trim().rsplit_once('(')?.1.strip_suffix(')')?;
    (s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())).then(|| s.to_string())
}

const BUILDS: &str = "for x in \"$1\" \"$1.new\"; do \"$x\" --version 2>/dev/null || echo -; done";

/// The builds at `exe` and `exe.new` on a host, by one call; Err: the host did not answer.
pub fn builds(on: &dyn Runner, exe: &str) -> Result<Builds> {
    let o = on.run("sh", &argv(&["-c", BUILDS, "sh", exe]))?;
    if o.code != 0 {
        bail!("exit {}: {}", o.code, o.stderr.trim());
    }
    let mut l = o.stdout.lines();
    Ok(Builds {
        live: l.next().and_then(sha_of),
        staged: l.next().and_then(sha_of),
    })
}

/// The shell line that downloads `a` to `exe.new`, checks its sha256 and prints its `--version` (exit 3: download, 4: checksum).
pub fn stage_line(a: &Asset, exe: &str) -> String {
    format!(
        "( n='{exe}.new'; p=\"$n.part\"; mkdir -p \"$(dirname \"$n\")\" && rm -f \"$p\" && curl -fsSL --retry 2 -o \"$p\" '{}' || \
{{ rm -f \"$p\"; exit 3; }}; s=$(sha256sum \"$p\" 2>/dev/null || shasum -a 256 \"$p\") || exit 3; \
[ \"${{s%% *}}\" = '{}' ] || {{ rm -f \"$p\"; exit 4; }}; chmod 755 \"$p\" && mv -f \"$p\" \"$n\" && \"$n\" --version )",
        a.url, a.sha256
    )
}

/// The shell line that puts a staged `exe.new` live at `exe`.
pub fn swap_line(exe: &str) -> String {
    format!("mv -f '{exe}.new' '{exe}'")
}

/// Stage `a` of `rel` at `exe.new` on a host within `limit`: download, checksum, `--version` names `rel.sha`, and with `gate` its `doctor --config-only`.
pub fn stage(
    on: &dyn Runner,
    exe: &str,
    rel: &Release,
    a: &Asset,
    gate: bool,
    limit: Duration,
) -> Result<(), Unstaged> {
    let new = format!("{exe}.new");
    let res = (|| {
        let line = stage_line(a, exe);
        let o =
            (on.run_for("sh", &argv(&["-c", &line]), "", limit)).map_err(|e| Unstaged::Download(format!("{e:#}")))?;
        let said = format!("{}{}", o.stdout, o.stderr).trim().to_string();
        match o.code {
            0 => {}
            3 => return Err(Unstaged::Download(said)),
            4 => return Err(Unstaged::Checksum),
            c => return Err(Unstaged::Version(format!("it does not run (exit {c}): {said}"))),
        }
        if o.stdout.lines().last().and_then(sha_of).as_deref() != Some(rel.sha.as_str()) {
            return Err(Unstaged::Version(format!("it says {said:?}, not {}", rel.sha)));
        }
        if gate {
            let o = on
                .run(&new, &argv(&["doctor", "--config-only"]))
                .map_err(|e| Unstaged::Doctor(format!("{e:#}")))?;
            if o.code != 0 {
                return Err(Unstaged::Doctor(o.stdout.trim().to_string()));
            }
        }
        Ok(())
    })();
    if res.is_err() {
        on.run("rm", &argv(&["-f", &new])).ok();
    }
    res
}

/// The GitHub repo drover publishes its builds in.
pub fn repo() -> &'static str {
    env!("CARGO_PKG_REPOSITORY").trim_start_matches("https://github.com/")
}

/// The check and the install line that put this build at `exe` on a host with `tags`: its main release, else its staging one.
pub fn install_lines(r: &dyn Runner, exe: &str, tags: &[String]) -> (String, String) {
    let check = format!("[ \"$('{exe}' --version)\" = 'drover {}' ]", crate::VERSION);
    let found = match find(r, repo(), "main", crate::SHA) {
        Found::Missing(_) => find(r, repo(), "staging", crate::SHA),
        f @ Found::Release(_) => f,
    };
    let install = match &found {
        Found::Release(rel) => rel
            .asset(repo(), tags)
            .map(|a| format!("{} && {}", stage_line(&a, exe), swap_line(exe))),
        Found::Missing(w) => Some(format!("echo '{}' >&2; false", w.replace('\'', ""))),
    };
    let none = || format!("echo 'no release asset for {}' >&2; false", tags.join(","));
    (check, install.unwrap_or_else(none))
}
