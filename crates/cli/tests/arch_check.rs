//! #734: `herdsman arch-check` on fixture repos: a cross-layer `use` or import fails with file:line, a banned call
//! outside its exception fails, and a file or function over its size limit fails unless the baseline lists it at
//! that size, so an untouched offender passes and a grown one fails.
mod common;
use common::tmpdir;
use std::path::Path;

const ARCH: &str = r##"
[[layer]]
name = "core"
paths = ["crates/core/src/**"]
mods = ["herdsman_core"]

[[layer]]
name = "server"
paths = ["crates/server/src/**"]
mods = ["herdsman_server"]
may_use = ["core"]

[[layer]]
name = "web-api"
paths = ["web/src/api/**"]

[[layer]]
name = "web-ui"
paths = ["web/src/ui/**"]
may_use = ["web-api"]

[[ban]]
pattern = "Command::new"
paths = ["crates/*/src/**"]
except = ["crates/core/src/runner.rs"]
reason = "#713: spawn processes through the Runner"

[[size]]
paths = ["crates/*/src/**/*.rs"]
max_file_lines = 8
max_fn_lines = 4
"##;

fn repo(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let d = tmpdir(name);
    for (p, text) in [(".herdsman/arch.toml", ARCH)].iter().chain(files) {
        let p = d.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn arch(dir: &Path, args: &[&str]) -> (i32, String) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_herdsman"))
        .arg("arch-check")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    (o.status.code().unwrap(), s)
}

#[test]
fn a_cross_layer_use_fails_with_file_and_line_and_an_allowed_one_passes() {
    let d = repo(
        "arch-layers",
        &[
            (
                "crates/core/src/lib.rs",
                "//! core\nuse std::fmt;\nuse herdsman_server::api;\n",
            ),
            (
                "crates/server/src/lib.rs",
                "use herdsman_core::{gate, queue};\nfn f() { herdsman_core::x(); }\n",
            ),
            ("web/src/ui/board.ts", "import { get } from \"../api/client\";\n"),
            (
                "web/src/api/client.ts",
                "// the client\nimport {\n  draw,\n} from '../ui/board';\n",
            ),
        ],
    );
    let (code, out) = arch(&d, &[]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("crates/core/src/lib.rs:3: layer `core` uses `server` (herdsman_server::api)"),
        "{out}"
    );
    assert!(
        out.contains("web/src/api/client.ts:4: layer `web-api` uses `web-ui` (web/src/ui/board)"),
        "{out}"
    );
    assert!(out.contains("2 violations"), "{out}");
    std::fs::write(d.join("crates/core/src/lib.rs"), "use std::fmt;\n").unwrap();
    std::fs::write(d.join("web/src/api/client.ts"), "export const get = 1;\n").unwrap();
    let (code, out) = arch(&d, &[]);
    assert_eq!((code, out.contains("0 violations")), (0, true), "{out}");
}

#[test]
fn a_banned_call_outside_its_exception_fails() {
    let runner = "pub fn run() {\n    std::process::Command::new(\"git\");\n}\n";
    let d = repo(
        "arch-ban",
        &[
            ("crates/core/src/runner.rs", runner),
            ("crates/core/src/gate.rs", "// never Command::new here\nfn f() {}\n"),
            (
                "crates/cli/src/main.rs",
                "fn main() {\n    let _ = Command::new(\"sh\");\n}\n",
            ),
        ],
    );
    let (code, out) = arch(&d, &[]);
    assert_eq!(code, 1, "{out}");
    let want = "crates/cli/src/main.rs:2: `Command::new` is banned here: #713: spawn processes through the Runner";
    assert!(out.contains(want), "{out}");
    assert!(out.contains("1 violations"), "{out}");
}

#[test]
fn a_file_or_fn_grown_past_its_baseline_fails_and_an_untouched_offender_passes() {
    let long_fn = "fn big() {\n    a();\n    b();\n    c();\n}\n";
    let d = repo(
        "arch-size",
        &[
            ("crates/core/src/big.rs", &format!("{long_fn}\n\n\n\n// 10 lines\n")),
            ("crates/core/src/ok.rs", "fn small() {\n    a();\n}\n"),
        ],
    );
    let (code, out) = arch(&d, &[]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("crates/core/src/big.rs:1: `crates/core/src/big.rs` is 10 lines, over 8 (baseline None)"),
        "{out}"
    );
    assert!(out.contains("`crates/core/src/big.rs:big` is 5 lines, over 4"), "{out}");
    let (code, out) = arch(&d, &["--write-baseline"]);
    assert_eq!(code, 0, "{out}");
    let base = std::fs::read_to_string(d.join(".herdsman/arch-baseline.txt")).unwrap();
    assert_eq!(base, "crates/core/src/big.rs 10\ncrates/core/src/big.rs:big 5\n");
    let (code, out) = arch(&d, &[]);
    assert_eq!(
        (code, out.contains("0 violations")),
        (0, true),
        "untouched offenders pass: {out}"
    );
    let grown = "fn big() {\n    a();\n    b();\n    c();\n    d();\n}\n\n\n\n\n// 11 lines\n";
    std::fs::write(d.join("crates/core/src/big.rs"), grown).unwrap();
    std::fs::write(d.join("crates/core/src/new.rs"), long_fn).unwrap();
    let (code, out) = arch(&d, &[]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("`crates/core/src/big.rs` is 11 lines, over 8 (baseline Some(10))"),
        "{out}"
    );
    assert!(
        out.contains("`crates/core/src/big.rs:big` is 6 lines, over 4 (baseline Some(5))"),
        "{out}"
    );
    assert!(
        out.contains("crates/core/src/new.rs:1: `crates/core/src/new.rs:big` is 5 lines"),
        "{out}"
    );
    assert!(out.contains("3 violations"), "{out}");
}

#[test]
fn no_config_has_nothing_to_check() {
    let d = tmpdir("arch-none");
    let (code, out) = arch(&d, &[]);
    assert_eq!((code, out.trim()), (0, "no .herdsman/arch.toml: nothing to check"));
}

fn git(dir: &Path, args: &[&str]) {
    let o = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

fn committed(name: &str, arch: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let d = tmpdir(name);
    for (p, text) in [(".herdsman/arch.toml", arch)].iter().chain(files) {
        let p = d.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    git(&d, &["init", "-q"]);
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "base"]);
    d
}

const OLD: &str = "// an old comment\nfn old() {}\n";

#[test]
fn only_an_added_comment_fails_but_safety_over_unsafe_and_one_doc_line_on_a_pub_item() {
    let d = committed(
        "arch-comments",
        "[comments]\npolicy = \"off\"\n",
        &[("src/lib.rs", OLD), ("web/a.ts", "export const a = 1;\n")],
    );
    let (code, out) = arch(&d, &["--base", "HEAD"]);
    assert_eq!((code, out.contains("0 violations")), (0, true), "{out}");
    let added = [
        "// SAFETY: the pointer is valid",
        "unsafe { f() }",
        "/// One line.",
        "pub fn a() {}",
        "/// First.",
        "/// Second.",
        "pub fn b() {}",
        "fn c() {",
        "    // new",
        "}",
        "/* block",
        "  still */",
    ];
    let lib = format!("{OLD}{}\n", added.join("\n"));
    std::fs::write(d.join("src/lib.rs"), lib).unwrap();
    std::fs::write(d.join("web/a.ts"), "/** Shown. */\nexport const a = 1;\n// not ok\n").unwrap();
    let (code, out) = arch(&d, &["--base", "HEAD"]);
    assert_eq!(code, 1, "{out}");
    let flagged: Vec<&str> = out
        .lines()
        .filter_map(|l| l.split(": a new comment").nth(1).and(l.split(": a").next()))
        .collect();
    assert_eq!(
        flagged,
        [
            "src/lib.rs:7",
            "src/lib.rs:8",
            "src/lib.rs:11",
            "src/lib.rs:13",
            "src/lib.rs:14",
            "web/a.ts:3"
        ],
        "{out}"
    );
    let (code, out) = arch(&d, &[]);
    assert_eq!(
        (code, out.contains("0 violations")),
        (0, true),
        "no --base, nothing judged: {out}"
    );
}

const DOCS: &str = "[docs]\npaths = [\"docs/**/*.md\", \"README.md\"]\nwatch = [\"src/cli.rs\"]\ncli = true\n";

#[test]
fn a_doc_naming_a_missing_path_line_link_or_flag_fails() {
    let readme = "See `src/lib.rs:2`, `src/lib.rs`, [doc](docs/a.md) and `herdsman spawn --dry-run <f>`.\n";
    let doc = "`../src/lib.rs` and `herdsman queue <fleet> add --top 3`\n```\n`src/gone.rs`\n```\n";
    let d = committed(
        "arch-docs",
        DOCS,
        &[
            ("src/lib.rs", "fn a() {}\nfn b() {}\n"),
            ("README.md", readme),
            ("docs/a.md", doc),
        ],
    );
    let (code, out) = arch(&d, &[]);
    assert_eq!((code, out.contains("0 violations")), (0, true), "{out}");
    let stale = "`src/gone.rs`, `src/lib.rs:3`, [x](docs/none.md), `herdsman spawn --gone`, `herdsman nope`\n";
    std::fs::write(d.join("docs/b.md"), stale).unwrap();
    let (code, out) = arch(&d, &[]);
    assert_eq!(code, 1, "{out}");
    for want in [
        "docs/b.md:1: `src/gone.rs`: no such path",
        "docs/b.md:1: `src/lib.rs:3`: src/lib.rs has fewer than 3 lines",
        "docs/b.md:1: link `docs/none.md` does not exist",
        "docs/b.md:1: `herdsman spawn --gone`: `spawn` has no `--gone`",
        "docs/b.md:1: `herdsman nope`: no herdsman command `nope`",
        "5 violations",
    ] {
        assert!(out.contains(want), "{want}: {out}");
    }
}

#[test]
fn a_changed_flag_needs_a_doc_naming_it_or_no_doc_in_the_pr_body() {
    let cli = "struct Args {\n    old_flag: bool,\n}\n";
    let d = committed(
        "arch-doc-touch",
        DOCS,
        &[("src/cli.rs", cli), ("docs/a.md", "`--old-flag`\n")],
    );
    std::fs::write(d.join("src/cli.rs"), "struct Args {\n    new_flag: bool,\n}\n").unwrap();
    std::fs::write(d.join("body.md"), "Closes #1\n").unwrap();
    let (code, out) = arch(&d, &["--base", "HEAD", "--pr-body", "body.md"]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("src/cli.rs:2: `new_flag` added, but no changed doc line names it"),
        "{out}"
    );
    assert!(out.contains("src/cli.rs:2: `old_flag` removed"), "{out}");
    std::fs::write(d.join("body.md"), "no doc: internal only\n").unwrap();
    let (code, out) = arch(&d, &["--base", "HEAD", "--pr-body", "body.md"]);
    assert_eq!((code, out.contains("0 violations")), (0, true), "{out}");
    std::fs::write(d.join("body.md"), "Closes #1\n").unwrap();
    std::fs::write(d.join("docs/a.md"), "`--new-flag`\n").unwrap();
    let (code, out) = arch(&d, &["--base", "HEAD", "--pr-body", "body.md"]);
    assert_eq!((code, out.contains("0 violations")), (0, true), "{out}");
}
