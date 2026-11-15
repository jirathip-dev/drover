mod common;
use common::*;
use drover::lanes::{body, now, roll, said, short, spin, strip, GAP};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn lines(s: &[&str]) -> Vec<String> {
    s.iter().map(|l| l.to_string()).collect()
}

fn file(p: &Path) -> Vec<String> {
    std::fs::read_to_string(p).unwrap().lines().map(String::from).collect()
}

#[test]
fn overlapping_reads_append_once_and_a_miss_writes_a_gap() {
    let f = tmpdir("roll").join("transcripts/a.log");
    let (all, live) = roll(&f, &lines(&["> p", "⏺ a", "  x", "⏺ b"]));
    assert_eq!((all.len(), live), (4, 1));
    assert_eq!(file(&f), ["> p", "⏺ a", "  x"]);
    assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    roll(&f, &lines(&["> p", "⏺ a", "  x", "⏺ b"]));
    roll(&f, &lines(&["\x1b[2m  x\x1b[0m", "⏺ b", "  y", "⏺ c"]));
    assert_eq!(file(&f), ["> p", "⏺ a", "  x", "⏺ b", "  y"]);
    let (all, _) = roll(&f, &lines(&["⏺ z", "  q", "⏺ w"]));
    assert_eq!(file(&f)[5..], [GAP, "⏺ z", "  q"]);
    assert_eq!(all.last().unwrap(), "⏺ w");
}

#[test]
fn the_transcript_keeps_the_last_5000_lines() {
    let f = tmpdir("roll-cap").join("transcripts/a.log");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, (0..4995).map(|i| format!("  {i}\n")).collect::<String>()).unwrap();
    let read: Vec<String> = (0..10).map(|i| format!("  new {i}")).chain(["⏺ live".into()]).collect();
    roll(&f, &[lines(&["  4994"]), read].concat());
    let got = file(&f);
    assert_eq!(
        (got.len(), got[0].as_str(), got[4999].as_str()),
        (5000, "  5", "  new 9")
    );
}

#[test]
fn ansi_lines_mask_run_by_run_and_strip_to_text() {
    let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let screen = format!("\x1b[0m\x1b[38;2;255;255;255m⏺ \x1b[0mtoken {secret} \x1b[1mbold\x1b[0m\n");
    let b = body(&screen);
    assert!(!b[0].contains(secret) && b[0].contains("\x1b[1mbold"), "{b:?}");
    assert_eq!(strip(&b[0]), "⏺ token ghp_••• bold");
}

#[test]
fn a_session_transcript_keeps_prompts_and_final_replies() {
    let jsonl =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/session/claude.jsonl"));
    let jsonl = jsonl.unwrap();
    let (said, now) = said(&jsonl);
    let got: Vec<(&str, &str)> = said
        .iter()
        .map(|s| (s["k"].as_str().unwrap(), s["text"].as_str().unwrap()))
        .collect();
    assert_eq!(
        got,
        [
            ("me", "Read /w/issue-9/.brief.md and execute it fully."),
            ("ai", "Done: **src/drive.rs** fixed, tests pass.\n- one\n- two"),
            ("me", "also run the tests with token ghp_•••"),
        ]
    );
    assert_eq!(said[1]["at"], "2026-11-03T10:05:00.000Z");
    assert_eq!(now.as_deref(), Some("now: running cargo test 2>&1 | tee t.log"));
    let done = r#"{"type": "assistant", "message": {"content": [{"type": "text", "text": "Tests pass."}]}}"#;
    assert_eq!(drover::lanes::said(&format!("{jsonl}{done}\n")).1, None);
}

#[test]
fn now_is_the_panes_last_tool_line() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/screens");
    let screen = |n: &str| std::fs::read_to_string(dir.join(format!("{n}.txt"))).unwrap();
    let claude = now(&screen("claude-ansi")).unwrap();
    assert!(
        claude.starts_with("now: running git commit -q -m \"serve: choices wrap"),
        "{claude}"
    );
    assert_eq!(
        now("⏺ Update(src/drive.rs)\n  ⎿  Updated\n⏺ All green.\n").as_deref(),
        Some("now: editing src/drive.rs")
    );
    assert_eq!(now("  ┊ 💻 $ cargo test\n").as_deref(), Some("now: running cargo test"));
    assert!(now(&screen("hermes-idle")).unwrap().starts_with("now: running sleep 8"));
    assert_eq!(now("⏺ Done (all tests pass).\n"), None);
}

#[test]
fn a_now_command_is_summarised_to_its_first_command() {
    for (cmd, want) in [
        (
            "git fetch origin && git merge origin/staging 2>&1 | tail -5; just --list; cat ci/cap.toml",
            "git fetch origin +3",
        ),
        ("cd ~/w/issue-9 && git merge origin/staging 2>&1 | tail -5; just --list", "git merge origin/staging +1"),
        (
            "cd /Users/jk/w/issue-2684; S=/tmp/s; FILES=$(git diff --name-only | rg -v '^docs/'); cargo fmt --check $FILES",
            "cargo fmt",
        ),
        ("sed -n '1,80p' src/usage.rs", "sed src/usage.rs"),
        ("pnpm vitest > t.log 2>&1", "pnpm vitest"),
        ("RUST_LOG=debug cargo test --test chat_blocks 2>&1 | tee t.log", "cargo test chat_blocks"),
        ("python3 - <<'EOF'", "python3 (script)"),
        ("cargo build >/dev/null || cargo build 2>&1 | head -40", "cargo build +1"),
    ] {
        assert_eq!(short(cmd), want, "{cmd}");
    }
    let home = std::env::var("HOME").unwrap();
    assert_eq!(
        now(&format!("⏺ Bash(cat {home}/w/a.rs)\n")).as_deref(),
        Some("now: running cat ~/w/a.rs")
    );
}

#[test]
fn a_claude_spinner_is_its_word_and_elapsed_time() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/screens");
    let screen = |n: &str| std::fs::read_to_string(dir.join(format!("{n}.txt"))).unwrap();
    let s = |w: &str, t: Option<&str>| Some((w.to_string(), t.map(String::from)));
    assert_eq!(
        spin("claude", &screen("claude-working")),
        s("✶ Pontificating…", Some("12m"))
    );
    assert_eq!(spin("claude", &screen("claude-tip")), s("✻ Deliberating…", Some("1m")));
    let short_run = "⏺ Bash(cargo test)\n\n✻ Herding… (42s · ↓ 1.2k tokens · esc to interrupt)\n";
    assert_eq!(spin("claude", short_run), s("✻ Herding…", None));
    assert_eq!(spin("claude", &screen("claude-idle")), None);
    assert_eq!(spin("dsh", &screen("dsh-working")), None);
    assert_eq!(spin("hermes", &screen("hermes-working")), None);
    assert_eq!(spin("hermes", short_run), None);
}
