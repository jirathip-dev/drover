use std::path::{Path, PathBuf};

const WORDS: [&str; 6] = [
    "jirathip",
    "synergy",
    "sendmeter",
    "claude-accounts",
    "tail8c3301",
    "hermes-brain",
];

const EXCEPTIONS: [(&str, &str, &str); 11] = [
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
        "assets/swag/farm/sprites.svg",
        "(c) 2026 jirathip-dev, MIT",
        "MIT notice of the ported Corral horses",
    ),
    (
        "assets/swag/SOURCE",
        "jirathip-dev/swag",
        "#928: the shared theme repo the packs are pinned from",
    ),
    (
        "docs/themes.md",
        "[jirathip-dev/swag](https://github.com/jirathip-dev/swag)",
        "#928: the shared theme repo the packs come from",
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
