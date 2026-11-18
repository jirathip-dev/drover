Closes #1096

## For humans
Docs about screens that no longer exist (the old Host tab, the old chat tab, Herd mode's data feed) are deleted or rewritten, and the README now shows only what ships today and what is queued next.
Cap raises are no longer written up as separate note files: the raise's commit records it, CI refuses a raise without that record, and a new command prints the full cap history from git.
Lane briefs now name the docs a lane may trust.

## What changed
- **cap-history gone:** `docs/cap-history/` (47 notes) deleted. `drover cap-check --base <sha>` refuses any commit since the base that touches `ci/cap.toml` or `ci/cap.d/` without a `Cap: <old> → <new> (#issue, q<N>)` line; CI passes the PR's base sha and now checks out the full history (`fetch-depth: 0`). `drover cap log` prints every cap change from `git log -p` of `ci/cap.toml`, `ci/cap.d` and the cap line `ci.yml` held before #129. On this repo that's 44 rows (5,000 at the first commit, then 43 changes up to 45,400 → 47,200). The 47 notes map to 43 commits because several notes described the same commit.
- **Rule text:** `roles/orchestrator.md`, `docs/architecture.md` and the hotspots ADR now describe the trailer rule. **Not done:** the `ci/cap.toml` header still points at `docs/cap-history/`, because the lane hook refuses any lane edit to that file (`REFUSED CAP_FILE`). The next cap-raise issue can fix it.
- **Removed UI:** every Host tab, `/herd.json`, `[chat] tab` and `/frag` passage in docs, roles and the README is deleted or rewritten to match staging. The old `[chat]` tab spec in `docs/orchestrator.md` is replaced by a section on the current Threads tab. `docs/runtime-hc.md` (a herdr spike that came out no-go) and `docs/snapshots/` (old-board screenshots nothing links to) are deleted.
- **Design mockups:** conductor-chat, fleet-cards, pickers, sim-680 and why-notes are deleted. The kept folders cover screens that exist or are queued: chat-*, config-card and threads-tab (#1086–#1088), queue-graph (#1089/#1090), hosts-history (#1091), scene-292 (#1092), plus schemes and screens-page. The two demo thread images moved to `demo/files/`.
- **README:** a short intro, then Today (what is on staging, including that the page stays) and Next (the open queue, by area). Nothing else. The quick start and command table move to a "Quick start" section of `docs/setup.md`; `readme.rs` now checks them there (they still run as written and name only real commands).
- **Lane briefs:** a repo's `.drover/docs.md` becomes a "Docs you may trust" section in every brief. This repo's file lists the README, `docs/*.md`, the kept mockups, the roles and `help --agent`.

Doc lines: **net -3,421** (+148 −3,569 across docs/, README.md, roles/, .drover/docs.md).
src/ net +104 (budget: none stated): the trailer check, `cap log` and the brief section. cap-check at head: 45,531 / cap 47,200.

## Evidence (head d30c7664ad753f969bf7c2e485a978cebd7c7b93)
Merged `origin/staging` after #1084 and #1088 merged (merges, not rebases: lanes never rewrite history). After the merge, the README moves #1081, #1084, #1088 and #1094 from Next to Today, the Threads section says what #1088 shipped, and `docs/parity.md` row 89 (new from #1094) names no removed route; its row number, test and status are unchanged.
- `rg -l 'Host tab|herd\.json|\[chat\] tab|/frag' docs roles README.md` → no output, exit 1
- `scripts/parity-check.sh origin/staging` → 0 (117 rows, 51 tests)
- `cargo fmt --check` → 0
- `cargo clippy --all-targets --locked -- -D warnings` → 0
- `cargo test --locked -p drover --no-fail-fast` with `--test` for arch_check, ask_ids, chat_tab, diagram, form_only, design_stage, hook, limits, lesson, schemes, readme, setup_neutral, round_images, roots, serial_groups, update_worktree, ui_tokens, why_briefs, webhook, timer, demo_export, brief_golden, overlap, page_drafts, parity_check → 0; 25 binaries, 260 passed, 0 failed. New tests: `overlap` `cap_check_refuses_a_cap_edit_without_its_trailer` and `cap_log_prints_every_cap_change_from_git`; `lesson` `rules_file_is_in_impl_and_rev_briefs_and_missing_is_fine` now asserts the docs section is there, and absent when the repo has no `.drover/docs.md`.
- **Correction:** the targeted run this body cited at `d6e6943` ran 0 tests (zsh passed the `--test` flags to cargo as one name filter), so it proved nothing; `readme` and `setup_neutral` were in fact failing there. Both are fixed since `c64f225`.
- The full `cargo test --locked` is CI's (it went green at `c64f225`); it is not run locally on this loaded host.
- `drover cap-check --config ci/cap.toml --base origin/staging` → 0; `drover cap log` → 0, 44 rows
- `drover diagram | diff docs/workflow.md -` → 0
- `drover arch-check` locally fails only its `generated` check, because `just` is not installed on this host; CI has `just`.

## Why (round 1)

Decided: merged origin/staging after #1084 and #1088 (merges, no rebase); README moves #1081, #1084, #1088, #1094 from Next to Today
Because: "**README**: Today = what is on staging now; Next = the open queue. Nothing else."
Decided: docs/parity.md row 89 keeps its number, test and status; only its feature text drops the removed route's name
Because: scripts/parity-check.sh:19
Rejected: deleting row 89 — parity-check fails a gone row unless it reads `dropped q<N>`, and the owner has not decided it
Check: rg -l 'Host tab|herd\.json|\[chat\] tab|/frag' docs roles README.md prints nothing; parity-check passes against origin/staging
Owner: The docs sweep is up to date with the latest merges: the front page lists what just shipped, and the queue list no longer shows it.

<sub>note on d30c766 · git log --notes=why</sub>

