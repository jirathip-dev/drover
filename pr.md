Closes #1172

## For humans
On the Queue, each fleet's "done today" row can now be tapped. It opens a list of that fleet's merges from the last 24 hours, newest first: issue number and title, merge time, merged commit.
Tapping an entry opens that issue's card. The count on the row is the length of that list, so the two always agree.

## What changed
- `serve.rs`: the fleet card's `done` list is now the fleet's `merged` events of the last 24 h, all of them. Before, it was issue states that looked done (closed ones too), cut to 5. Each row gains `title`, `t` (merge time) and `sha` (`merge_sha`).
- `screens.rs`: the `done-today-<f>` row gets a chevron and `tap: {open: /screens/queue/done?repo=<f>}`. Its value is `done.len()`. New `/screens/queue/done` screen (under the Queue tab's path so it gets its own `#queue/done…` hash and the back button works): one row per merge (`#N title`, `merged HH:MM UTC · <sha7>`, its age), each tapping `/screens/issue?repo=<f>&issue=<n>`. `on_row` falls back to a merged issue's done row, since merged issues are not queue rows, so its card opens.
- `docs/serve.md`: the new screen and the `done` fields.
- `crates/cli/tests/m3.rs`: its exact done-row substring now carries `title` and `t`.
- `assets/types/api.d.ts` regenerated (`cargo test -p drover-server --features ts export_bindings`; `just` is not on the lane host).

Decided: "today" stays the last 24 h the board already used, not a UTC calendar day.

src/ net +36 (budget: none given): a new screen plus the merged-issue fallback for the card; the done list itself is -1.

## Evidence (head a45213b5d29b50892ee705f273f16ade5f183b08)
- `cargo test -p drover --test cooee_screens --test m3 --test parity_webkit`: exit 0 (14, 10, 12 passed; new: `done_today_opens_todays_merged_events_and_a_row_opens_its_issue_card`)
- `cargo test -p drover --test serve_web --test api_v1`: exit 0 (at 2dd7127; unchanged code paths since)
- `cargo clippy --workspace --lib --bins --locked -- -D warnings`: exit 0 (at 2dd7127)
- `cargo clippy --workspace --tests --locked -- -D warnings -A …` (CI's allow list): exit 0 (at 2dd7127)
- `cargo fmt --check`: exit 0
- `drover cap-check --config ci/cap.toml`: exit 0, 47185 / 47200
- `drover arch-check --base HEAD~2 --pr-body …`: the only violation is `just gen` missing on the lane host (no generated file touched)
- WebKit 390 px `done_today_opens_the_list_a_row_opens_its_issue_card_and_back_returns` (tap → list → row → card → close → the screen's back button → `#queue`): skipped locally (no Playwright on the lane host); at 2dd7127 ui-webkit got list, card, close but not back, fixed here by the path move.
