Closes #1098

## For humans
Old code comments that narrated project history (issue numbers, "as the old board did") are gone from the Rust and the phone page's TypeScript.
The comment rule now covers every source file, including the server's page code and the phone page, so they cannot creep back.
Nothing behaves differently.

## Parity
none: no parity row changes or is dropped. The `web/`, `web/dist/`, `assets/types/` and `serve.rs` edits are comment deletions, two regex literals with the same meaning (`/^\//` → `/^[/]/`), the bundle rebuilt from them and doc text ts-rs copies into the types.

## What changed
- `.drover/arch.toml`: the strict comment policy's `except` keeps only the generated `assets/types/**` and `web/dist/**`; `serve.rs`, `serve/{chat,fresh,slo}.rs` and `web/src/**` are now checked. Strict already scanned whole files (test `strict_fails_an_old_plain_comment_…` in `crates/cli/tests/arch_check.rs`), so arch.rs needed no change.
- 323 banned comment lines deleted (serve.rs, serve/chat.rs, web/src), trailing ones included.
- 786 `///` doc lines rewritten by rule to drop issue numbers, ~50 more by hand; history wording ("as before", "as herd.js did", "the old board's") rewritten. Format examples that looked like issue numbers became `#N`.
- `web/src/schemes.ts` stays TypeScript, now typed: its banned `// @ts-nocheck` is gone and so are its comment markers. Its test (`tests/schemes.rs`) slices it up to `export {` and runs it as `t.ts` under node 24, which strips the types. No `allowJs`, so the strict comment policy and the TS lint both cover it.
- Two regex literals `/^\//` → `/^[/]/` (same meaning; the comment scanner read `//` as a comment). `web/dist/board.js` rebuilt: only those characters differ.
- `assets/types/api.d.ts` regenerated (ts-rs copies the doc comments) and its header string in `crates/server/tests/types.rs` lost `(#375)`.

## Lines
src/ net −279 (staging 52131 → 51852 at 08afca0b, `drover cap-check`), budget n/a: a deletion pass. Diff over crates/ and web/src vs origin/staging: 101 files, +975 −1284. Web TS 855 → 768.

## Evidence at 08afca0b5f284b8aae2c95f83c029cca14ad027b
Head 08afca0b is the latest merge of origin/staging (5451162a). Its two conflicts, `crates/core/src/drive.rs` and `crates/core/src/hosts.rs`, were doc lines only; staging's wording was kept without the issue numbers. Three new doc lines from staging lost their issue numbers too (`waiting.rs`, `check/sweep.rs`, `tests/common/mod.rs`). Every result below was produced at 08afca0b:
- `rg '//.*#[0-9]{2,4}' crates web/src` → no matches
- `drover arch-check` (this branch's own binary, whole files) → exit 1: 884 files, 1 violation, and that one is `just gen` exited 127 because `just` is not on this host. No comment violation.
- The `gen` recipe's five commands, run by hand (`cargo test -p drover-server --features ts export_bindings`, `npm --prefix web ci`, `npm --prefix web run build`, `drover diagram > docs/workflow.md`, `BLESS=1 cargo test -p drover --test brief_golden`) → 0, and no generated file drifted (`git status` clean for them)
- `cargo fmt --all --check` → 0
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` (CI's command) → 0
- `scripts/test-shards.sh check` → 0
- `drover cap-check --config ci/cap.toml` → 0 (51852 / 53500)
- web: `npm run check` 0, `npm run lint` 0, `../scripts/lint-fixtures.sh ts` 0
- `cargo test --workspace --locked --no-fail-fast` → 101, but no test failed: 167 targets passed and 63 never ran (`could not execute process … (never executed)`), because the host removed the lane's build folder in the middle of the run. The 63 were rerun in the lane's scratch target: the library and unit tests → 0; the CLI targets → 101 with 1 failure, `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`. That failure comes from the host: shed is installed at `/usr/local/bin/shed` and `~/.cargo/bin/shed`, so `doctor` finds it, and this branch's change to `shed.rs` is comment-only. CI is the clean-host proof for that test.

## Not done here
Marking the issue serial over crates/ and web/ is a queue setting, not something a lane can set. Not checked here: the `ui` job's screenshots. The only page change is the two regex literals and the bundle rebuilt from them.
