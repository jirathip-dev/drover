//! #82: shipped code and docs stay setup-neutral: no install's own names (people, repos, paths,
//! hosts). The person in charge is "the owner"; examples use `acme/*` and `<owner>/<repo>`.
use std::path::{Path, PathBuf};

/// Matched case-insensitively anywhere in a shipped file.
/// #355 made `conductor` a drover role, so it is no longer an install's own name.
const WORDS: [&str; 6] = [
    "jirathip",
    "synergy",
    "sendmeter",
    "claude-accounts",
    "tail8c3301",
    "hermes-brain",
];

/// (file, exact text with whitespace collapsed, why it may stay). Each one is removed before the scan.
const EXCEPTIONS: [(&str, &str, &str); 13] = [
    (
        "assets/herd.js",
        "jirathip-dev/corral",
        "MIT notice: the source Herd mode is ported from",
    ),
    (
        "assets/herd.js",
        "MIT © jirathip-dev",
        "MIT notice of the ported Corral code",
    ),
    (
        "assets/themes/farm/sprites.svg",
        "(c) 2026 jirathip-dev, MIT",
        "MIT notice of the ported Corral horses",
    ),
    (
        "docs/design/farm-v2/make_sprites.py",
        "(c) 2026 jirathip-dev, MIT",
        "#242: MIT notice of the ported Corral horses in the sheet it writes",
    ),
    (
        "docs/design/farm-v2/sprites.svg",
        "(c) 2026 jirathip-dev, MIT",
        "#242: MIT notice of the ported Corral horses",
    ),
    (
        "docs/design/theme-370/proposed/farm/sprites.svg",
        "(c) 2026 jirathip-dev, MIT",
        "#370: MIT notice of the ported Corral horses",
    ),
    (
        "docs/design/farm-v2/README.md",
        "MIT © jirathip-dev",
        "#242: MIT notice of the ported Corral horses",
    ),
    (
        "docs/serve.md",
        "jirathip-dev/corral",
        "MIT notice: the source the horses, icon and Herd mode are ported from",
    ),
    (
        "docs/serve.md",
        "MIT © jirathip-dev",
        "MIT notice of the ported Corral code and icon",
    ),
    (
        "docs/serve.md",
        "https://jirathip-dev.github.io/herdsman/",
        "the project's own public demo",
    ),
    (
        "docs/dsh.md",
        "https://github.com/jirathip-dev/herdsman/issues/",
        "the project's own issue links",
    ),
    (
        "docs/setup.md",
        "https://github.com/jirathip-dev/herdsman/issues/",
        "the project's own issue links",
    ),
    (
        "README.md",
        "https://github.com/jirathip-dev/herdsman/issues/",
        "the project's own issue links",
    ),
];

fn files(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        for e in std::fs::read_dir(p).unwrap() {
            files(&e.unwrap().path(), out);
        }
    } else {
        out.push(p.to_path_buf());
    }
}

#[test]
fn shipped_files_name_no_install() {
    let root = &Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut all = vec![];
    for p in [
        "crates/core/src",
        "crates/server/src",
        "crates/cli/src",
        "assets",
        "roles",
        "docs",
        "README.md",
        "fleet.example.toml",
    ] {
        files(&root.join(p), &mut all);
    }
    let (mut bad, mut used) = (vec![], [false; EXCEPTIONS.len()]);
    for f in all {
        let rel = f.strip_prefix(root).unwrap().to_string_lossy().into_owned();
        // Images and other binaries hold no text to check.
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let mut text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        for (i, (file, allowed, _why)) in EXCEPTIONS.iter().enumerate() {
            if *file == rel && text.contains(allowed) {
                used[i] = true;
                text = text.replace(allowed, "");
            }
        }
        let lower = text.to_lowercase();
        bad.extend(
            WORDS
                .iter()
                .filter(|w| lower.contains(*w))
                .map(|w| format!("{rel}: {w}")),
        );
    }
    // A stale exception goes too, so the list never outgrows what ships.
    bad.extend(
        EXCEPTIONS
            .iter()
            .zip(used)
            .filter(|(_, u)| !u)
            .map(|((f, a, _), _)| format!("{f}: unused exception {a:?}")),
    );
    assert!(
        bad.is_empty(),
        "setup-specific names in shipped files (#82):\n{}",
        bad.join("\n")
    );
}

/// #732 (q911): role texts speak of "the lane" and "the agent": no harness or provider's name in a line's behaviour
/// (a `code` span, a file or command's name, may carry one).
#[test]
fn role_texts_name_no_harness() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../roles");
    let mut bad = vec![];
    for e in std::fs::read_dir(&root).unwrap() {
        let f = e.unwrap().path();
        let text = std::fs::read_to_string(&f).unwrap();
        for (n, line) in text.lines().enumerate() {
            let prose: String = line.split('`').step_by(2).collect::<Vec<_>>().join(" ").to_lowercase();
            let words = prose.split(|c: char| !c.is_ascii_alphanumeric());
            if let Some(w) = words
                .into_iter()
                .find(|w| ["claude", "hermes", "codex", "anthropic", "openai"].contains(w))
            {
                bad.push(format!(
                    "{}:{}: {w}: {line}",
                    f.file_name().unwrap().to_string_lossy(),
                    n + 1
                ));
            }
        }
    }
    assert!(bad.is_empty(), "say \"the lane\" or \"the agent\":\n{}", bad.join("\n"));
}
