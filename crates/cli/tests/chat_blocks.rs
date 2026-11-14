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

fn node(js: &str) -> Option<String> {
    if std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: no node here");
        return None;
    }
    let src = include_str!("../../../assets/serve.js");
    let part = &src[src.find("// #138 chat blocks").unwrap()..src.find("// chat:end").unwrap()];
    let d = tmpdir("chat-js");
    std::fs::write(d.join("t.js"), format!("{part}\n{js}")).unwrap();
    let o = std::process::Command::new("node").arg(d.join("t.js")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    Some(String::from_utf8(o.stdout).unwrap())
}

#[test]
fn sgr_maps_to_the_nearest_board_token() {
    let cases = [
        ("38;2;248;248;242", ""),
        ("38;2;153;153;153", "t-m"),
        ("38;2;117;113;94", "t-m"),
        ("2", "t-m"),
        ("38;2;177;185;249", "t-l"),
        ("1", "t-b"),
        ("38;2;80;200;80", "t-g"),
        ("38;2;166;226;46", "t-g"),
        ("38;2;78;186;101", "t-g"),
        ("38;2;220;90;90", "t-r"),
        ("38;2;255;107;128", "t-r"),
        ("38;2;230;219;116", "t-a"),
        ("38;2;215;119;87", "t-a"),
        ("38;2;255;193;7", "t-a"),
        ("38;2;102;217;239", "t-c"),
        ("38;2;0;204;204", "t-c"),
        ("38;2;188;140;255", "t-p"),
        ("48;2;61;1;0", "bg-r"),
        ("48;2;2;40;0", "bg-g"),
        ("48;2;92;2;0", "bg-r2"),
        ("48;2;4;71;0", "bg-g2"),
        ("31", "t-r"),
        ("92", "t-g"),
        ("90", "t-m"),
        ("38;5;196", "t-r"),
        ("38;5;244", "t-m"),
        ("1;38;2;220;90;90", "t-r t-b"),
    ];
    let js = format!(
        "console.log(JSON.stringify({}.map(c => runs(`\\x1b[${{c}}mx\\x1b[0m y`))))",
        serde_json::to_string(&cases.map(|c| c.0)).unwrap()
    );
    let Some(out) = node(&js) else { return };
    let got: Vec<Vec<(String, String)>> = serde_json::from_str(&out).unwrap();
    for ((code, want), g) in cases.iter().zip(got) {
        assert_eq!(g[0].1, *want, "SGR {code}");
        assert_eq!(g.last().unwrap().1, "", "SGR {code}: 0 resets");
    }
    assert!(!out.contains('#'), "{out}");
}

#[test]
fn markdown_is_escaped_text_plus_spans() {
    let js = r#"console.log(prose(['⏺ # Plan <script>alert(1)</script>', '  Fix **the bug** in `src/a.rs` & <b>x</b>,', '  wrapped.',
      '', '  - one', '  - two `<i>`', '', '  1. first', '  2. second', '', '  then \x1b[38;2;177;185;249mcargo test\x1b[0m runs']))"#;
    let Some(out) = node(js) else { return };
    assert_eq!(
        out.trim(),
        "<p class=h>Plan &#60;script&#62;alert(1)&#60;/script&#62;</p><p>Fix <b>the bug</b> in <code>src/a.rs</code> &#38; \
         &#60;b&#62;x&#60;/b&#62;, wrapped.</p><ul><li>one</li><li>two <code>&#60;i&#62;</code></li></ul>\
         <ol><li>first</li><li>second</li></ol><p>then <code>cargo test</code> runs</p>"
    );
}

#[test]
fn screens_group_into_role_blocks() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/screens");
    for (name, harness) in [("claude-ansi", "claude"), ("dsh-working", "dsh")] {
        let screen = std::fs::read_to_string(dir.join(format!("{name}.txt"))).unwrap();
        let js = format!(
            "console.log(JSON.stringify(blocks({}, '{harness}', true).map(b => [b.k, b.html]), null, 1))",
            serde_json::to_string(&body(&screen)).unwrap()
        );
        let Some(out) = node(&js) else { return };
        let golden = dir.join(format!("{name}.blocks.json"));
        if std::env::var_os("BLESS").is_some() {
            std::fs::write(&golden, &out).unwrap();
        }
        assert_eq!(out, std::fs::read_to_string(&golden).unwrap(), "{name}: blocks changed");
        let kinds: Vec<(String, String)> = serde_json::from_str(&out).unwrap();
        let kinds: Vec<&str> = kinds.iter().map(|k| k.0.as_str()).collect();
        let want: &[&str] = if harness == "claude" {
            &["ai", "ai", "ai", "run", "qs", "ai", "qs", "ai", "qs", "ai"]
        } else {
            &["ai", "me", "ai", "run", "ai"]
        };
        assert_eq!(kinds, want, "{name}");
    }
}

#[test]
fn an_acp_transcript_keeps_its_line_breaks_and_one_line_per_tool() {
    let screen =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/screens/acp.txt")).unwrap();
    let js = format!(
        "const ls = {};\nconsole.log(JSON.stringify([blocks(ls, 'claude', false), matters(ls, null, [], 'claude')].map(bs => bs.map(b => [b.k, b.html]))))",
        serde_json::to_string(&body(&screen)).unwrap()
    );
    let Some(out) = node(&js) else { return };
    let [full, matters]: [Vec<(String, String)>; 2] = serde_json::from_str(&out).unwrap();
    assert_eq!(full, matters, "both views draw it the same");
    let kinds: Vec<&str> = full.iter().map(|b| b.0.as_str()).collect();
    assert_eq!(kinds, ["txt", "run", "run", "run", "run", "txt"]);
    assert_eq!(
        full[0].1,
        "<div class=txt>&#62; # Brief — drover-impl-12 (acme/sendmeter#12)\nImplement #12: retry a failed send once.\nRun `cargo test` before you push.</div>"
    );
    assert_eq!(
        full[1].1,
        "<details class=\"blk run\"><summary><span class=k>tool</span><span class=cmd>Read src/send.rs (completed)</span><i class=cv></i></summary></details>"
    );
    let title = &full[3].1;
    assert!(
        title.contains("<span class=cmd>`cd /srv/acme/worktrees/sendmeter-impl-12 &#38;&#38; cargo test")
            && title.contains("tee test.log`</span><i class=cv></i></summary><div class=out><pre>")
            && title.contains("(completed)</span></pre></div></details>"),
        "a multi-line title: its first line, the rest behind the tap: {title}"
    );
    assert!(full[4].1.contains("git push -u origin issue-12 (failed)"));
    assert!(full[5].1.contains("500 ms backoff.\nTests pass;"), "{}", full[5].1);
}

#[test]
fn tool_runs_tag_cap_and_keep_the_failure() {
    let js = r#"
const out = n => Array.from({ length: n }, (_, i) => (i ? '     ' : '  ⎿  ') + 'line ' + i);
const ok = blocks(['⏺ Bash(cargo test)', ...out(29)], 'claude', false)[0].html;
const bad = blocks(['\x1b[38;2;255;107;128m⏺\x1b[0m Bash(cargo test)', '  ⎿  Error: Exit code 101', ...out(28).map(l => l.replace('⎿', ' '))], 'claude', false)[0].html;
const run = blocks(['⏺ Read(src/a.rs)'], 'claude', true)[0].html;
const diff = blocks(['⏺ Update(src/a.rs)', '  ⎿  Updated src/a.rs', '      52 \x1b[48;2;61;1;0m-        .filter(|s| s.state)\x1b[0m'], 'claude', false)[0].html;
console.log(JSON.stringify([ok, bad, run, diff]));
"#;
    let Some(out) = node(js) else { return };
    let [ok, bad, run, diff]: [String; 4] = serde_json::from_str(&out).unwrap();
    assert!(
        ok.starts_with(
            "<details class=\"blk run\"><summary><span class=k>$</span><span class=cmd>cargo test</span><i class=cv>"
        ),
        "{ok}"
    );
    assert!(
        ok.contains("line 19</span><span class=rest>") && !ok.contains("cut") && ok.contains("20 of 29 lines"),
        "{ok}"
    );
    assert!(bad.contains("<span class=t-r>exit 101</span>"), "{bad}");
    assert!(
        bad.contains("line 8</span><span class=rest>")
            && bad.contains("⋯ 9 lines</span><span class=\"ol\" style=\"--i:0\">line 18"),
        "{bad}"
    );
    assert!(
        run.contains("<span class=k>Read</span><span class=cmd>src/a.rs</span><span class=t-a>running</span>"),
        "{run}"
    );
    assert!(diff.contains("<span class=\"ol bg-r\" style=\"--i:13\">"), "{diff}");
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

#[test]
fn the_default_view_keeps_prompts_final_replies_and_asks() {
    let js = r#"
const lines = ['> fix it', '⏺ Looking.', '⏺ Bash(cargo test)', '  ⎿  ok', '⏺ Fixed and tested.', '> ship it', '⏺ Read(a.rs)', '⏺ Shipped.'];
const notes = [{ at: 1, text: 'spawned w-impl-9' }, { at: 1793700600, text: 'ask: merge now?' }];
const pane = matters(lines, null, notes, 'claude').map(b => b.k + ' ' + b.key.split(':')[1]);
const said = [{ k: 'me', text: 'fix it', at: '2026-11-03T10:00:00.000Z' }, { k: 'ai', text: '# Fixed\nall green', at: '2026-11-03T10:20:00.000Z' }];
const ts = matters([], said, notes, 'claude');
const k = (l, s, h) => matters(l, s, [], h).map(b => b.k).join(' ');
const other = [k(lines, [], 'claude'), k(lines, null, 'hermes'), k(['⏺ Bash(cargo test)', '  ⎿  ok'], null, 'hermes')];
console.log(JSON.stringify([pane, ts.map(b => b.k), ts[3].html, other]));
"#;
    let Some(out) = node(js) else { return };
    let (pane, ts, html, other): (Vec<String>, Vec<String>, String, Vec<String>) = serde_json::from_str(&out).unwrap();
    assert_eq!(
        pane,
        [
            "note none",
            "note 1",
            "note 1793700600",
            "me > fix it",
            "ai ⏺ Fixed and tested.",
            "me > ship it",
            "ai ⏺ Shipped."
        ]
    );
    assert_eq!(ts, ["note", "me", "note", "ai"]);
    assert!(
        html.contains("<span class=pv>Fixed</span>") && html.contains("<p class=h>Fixed</p><p>all green</p>"),
        "{html}"
    );
    assert_eq!(other, ["note me ai me ai", "me ai me ai", "run"]);
}
