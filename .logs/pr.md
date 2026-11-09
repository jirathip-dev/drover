Closes #712

## For humans

A parked issue can now be put back: `herdsman unpark`, or the new [unpark] button on its queue or lane row (paired phones only), returns it to where it was, and the next drive pass carries on.
When you park an issue you can say why; the reason shows in the queue listing and on the issue's row.

## What changed

- `herdsman unpark <fleet> <issue> [--by]` (owner verb): refuses `NOT_PARKED` unless the issue is parked; logs `unparked` with `by` and the state it goes back to, the state before the `needs-human` the park closed (or the state at park time when parked from a failed extend's card), folded from the log like drive's `repaired`. The queue place is kept; an issue no longer in the queue is added at the end.
- `unparked` folds exactly like `repaired` (`events.rs`), so drive picks the issue up on its next pass.
- `herdsman park … --why <text> [--by]`: stored on the `parked` event; `queue ls` (text and `--json` `why`) and the Queue row's notes show `parked: <why>`.
- Board: a parked Queue row and a Lanes card of a parked issue show `[unpark]`; the op is in `PAIRED_OPS`, the same gate as `[park]`. The Lanes payload gains `LaneRow.unpark` (its fleet) and `LanesTab.mtime` for the form; `assets/types/api.d.ts`, `lanes.json` and the new `post-unpark.json` contract fixtures follow; `web/dist` rebuilt.
- Docs: `docs/lanes.md` (park/unpark), `docs/serve.md` (POST ops); CLI help lists both verbs (`HELP_AGENT_MAX` +6).

Decided routinely: the web [park] form takes no reason field (the CLI's `--why` covers spec item 4; no visual change beyond [unpark] and the reason line). Doctor and `status` are unchanged.

src/ net +94 (budget: none stated) — a new verb, a fold arm, the reason on two surfaces and two payload fields.

## Evidence

Final head: `e8544cc6bf9c6b20421eae62e6a467c296bd1d47` (origin/staging merged in).

At `e8544cc6b`:
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `cargo fmt --all --check` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (36824 / 38028)
- `cargo test --workspace --lib --bins` → exit 0
- `cargo test -p herdsman --test <t>` for spawn, drive, merge, api_v1_contract, parked, queue_tab, queue_board, queue_blockers, serve_web, needs_human, lanes_sheet, design_stage, lesson, boat → each exit 0

At `7cc62025e` (before the staging merge), every test target one at a time (150 targets): all exit 0 but `self_update`, which fails only inside a lane shell (it expects `by: owner` and reads `HERDSMAN_LANE`); `env -u HERDSMAN_LANE cargo test -p herdsman --test self_update` → exit 0. `npm run check` and `npm run build` in `web/` → exit 0, `web/dist` committed.

New tests:
- `drive.rs` `an_unparked_issue_goes_back_to_its_state_and_the_next_pass_starts_it`: unpark of a needs-human issue refuses `NOT_PARKED`; `park --why` shows in `queue ls` and the Queue row; drive skips the parked issue; an unpaired POST `unpark` is 401; a paired one returns it to `queued`; the log has `parked` then `unparked`, both `by: owner`; the next pass starts it.
- `merge.rs` `unpark_restores_the_round_and_requeues_it_last`: back to `fix(3)`, `unparked` carries `by: alice` and the state, the issue goes after #5 and #7, a second unpark refuses.

Not covered by a test: the Lanes card's `[unpark]` button markup.
