Closes #750

## For humans

When you open a lane on the old board, a transcript from a shed (ACP) session now shows line by line instead of as one long paragraph. Each tool the agent ran is one short row; tap it to see the whole command. Lanes whose screens the board already reads (Claude, Hermes, dsh) look the same as before.

## What changed

- `assets/serve.js`: a transcript with no screen marker the sheet parses (⏺ ⎿ ❯ › • ┊ 💻) is drawn as plain text, its line breaks kept, in both the default and the full view. Each `[tool] ` entry is a closed one-line row (the existing tool-run block); a multi-line title's later lines, up to its `(status)`, are behind the tap. Not keyed on harness or runtime (q937: harness and provider agnostic).
- `assets/serve.css`: one rule, `.chat .txt` with `white-space: pre-wrap`.
- `assets/ui-audit.js`, `scripts/ui-audit.sh`, `docs/ui.md`: new `sheet-acp` view, a lane with a recorded-format ACP transcript; it fails unless the text keeps its lines and each tool is one closed one-line row. Its 390 shot is `ui-390/sheet-acp.png`.
- `crates/cli/tests/chat_blocks.rs` + `golden/screens/acp.txt`: the sheet's real chat code on a shed-format transcript.
- Side effect, in spec: the demo's codex scratch pane (an Others agent with no screen markers) now also keeps its lines (`sheet-other`).

src/ net +0 (budget n/a): only assets, tests and docs change.

## Evidence (head 8edca3cb7e125bd1efbdf2017c4cce484a04a03a)

Round 3 (evidence fix): no product change. Merged `origin/staging` (#754, #710, #731, #707); none of them touches `assets/` or `web/`, so `web/dist` is unchanged. All runs at 8edca3cb7e125bd1efbdf2017c4cce484a04a03a.

- `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `env -u HERDSMAN_LANE cargo test --locked -p herdsman --test chat_blocks --test board_freeze`: exit 0, 13 + 4 passed, including `an_acp_transcript_keeps_its_line_breaks_and_one_line_per_tool`.
- `env -u HERDSMAN_LANE cargo test --workspace --locked --no-fail-fast -- --skip lanes_and_sheet_fit_390px --skip chat_views_never_blank --skip one_401_never_latches_unpaired --skip report_mode_in_webkit_outlines_confirms_and_sends_a_bundle --skip every_control_passes_the_audit_at_390px`: exit 101, 1237 passed, 2 failed: `outbox` `a_remote_lane_only_queues_and_drive_drains_it_once` and `shed` `a_shed_acp_fleets_thread_runs_its_conductor_as_an_acp_session_that_sleeps_and_restarts`.
  - Both read the lane's own env. With every `HERDSMAN_*` variable unset, `cargo test --locked -p herdsman --test outbox --test shed` exits 0, 1 + 35 passed. This branch changes no Rust source.
  - `live_board_chat_and_ascii_in_webkit` failed once on timing in an earlier full run (its `/live` update was late while another lane held the UI audit lock). Run alone, `--test live_board` exits 0, 2 passed.
- Not verified locally: the 5 skipped WebKit tests hang on this host. CI runs them.
- Earlier rounds, unchanged: the new `chat_blocks` test fails at base (e501da8) and passes at head; `UI_AUDIT_VIEWS="sheet-acp …" scripts/ui-audit.sh` exit 0, 0 outliers.

## Why (round 1)

Decided: a transcript with no parsed screen marker (⏺ ⎿ ❯ › • ┊ 💻) is drawn as plain text with its line breaks kept, in both views.
Because: "a transcript that is not a parsed Claude screen is shown with its line breaks kept"
Because: assets/serve.js:1246
Decided: each `[tool] ` entry is a closed one-line row (the existing tool-run block), a multi-line title's later lines (to its `(status)`) behind the tap.
Because: "Each line starting `[tool] ` is cut to one line, its first, with the rest behind a tap."
Rejected: keying on the lane's harness or runtime — the owner asked for harness and provider agnostic (q937), and it would add a field to the API.
Rejected: treating hermes and dsh screens as plain — they are parsed today (┊, ›) and the issue says no other change.
Check: crates/cli/tests/chat_blocks.rs an_acp_transcript_keeps_its_line_breaks_and_one_line_per_tool fails on base (one <p>) and passes at head.
Check: the ui job's ui-390/sheet-acp.png shows separate lines and one row per tool; the demo's codex scratch pane (no markers) now also keeps its lines.
Owner: Lane transcripts from shed sessions now show line by line, with each tool call on its own tappable row.

<sub>note on e501da8 · git log --notes=why</sub>

## Why (round 2)

Decided: merged origin/staging three times (#725 lifeboat, #760 architect, #762 cap); the ui-audit.sh view list keeps staging's lifeboat/page and this branch's sheet-acp.
Because: scripts/ui-audit.sh:70
Rejected: none
Owner: The branch is brought up to date with staging; the lane sheet change is unchanged.

<sub>note on 4e2b91b · git log --notes=why</sub>

