Closes #1172

## For humans
On the Queue, each fleet's "done today" row can now be tapped. It opens a list of that fleet's merges from the last 24 hours, newest first: issue number and title, merge time, merged commit.
Tapping an entry opens that issue's card. The count on the row is the length of that list, so the two always agree.

## What changed
- `serve.rs`: the fleet card's `done` list is now the fleet's `merged` events of the last 24 h, all of them. Before, it was issue states that looked done (closed ones too), cut to 5. Each row gains `title`, `t` (merge time) and `sha` (`merge_sha`).
- `screens.rs`: the `done-today-<f>` row gets a chevron and `tap: {open: /screens/done?repo=<f>}`. Its value is `done.len()`. New `/screens/done` screen: one row per merge (`#N title`, `merged HH:MM UTC · <sha7>`, its age), each tapping `/screens/issue?repo=<f>&issue=<n>`. `on_row` falls back to a merged issue's done row, since merged issues are not queue rows, so its card opens.
- `assets/types/api.d.ts` regenerated (`cargo test -p drover-server --features ts export_bindings`; `just` is not on the lane host).

Decided: "today" stays the last 24 h the board already used, not a UTC calendar day.

src/ net +36 (budget: none given): a new screen plus the merged-issue fallback for the card; the done list itself is -1.

## Evidence (head 2dd7127bd2803fdb04b6ce3351aefe5ffaab8092)
- `cargo test -p drover --test cooee_screens`: exit 0, 14 passed (new: `done_today_opens_todays_merged_events_and_a_row_opens_its_issue_card`)
- `cargo test -p drover --test serve_web --test api_v1 --test parity_webkit`: exit 0
- `cargo clippy --workspace --lib --bins --locked -- -D warnings`: exit 0
- `cargo clippy --workspace --tests --locked -- -D warnings -A …` (CI's allow list): exit 0
- `cargo fmt --check`: exit 0
- `drover cap-check --config ci/cap.toml`: exit 0, 47185 / 47200
- WebKit 390 px `done_today_opens_the_list_a_row_opens_its_issue_card_and_back_returns` (tap → list → row → card → close → back): skipped locally (no Playwright on the lane host); it runs in CI's ui-webkit job.

## Why (round 1)

Decided: the fleet card's done list is the fleet's `merged` events of the last 24 h, all of them, and the row's count is that list's length
Because: "The count and the list come from the same `merged` events, so they always agree."
Because: crates/server/src/serve.rs:2052
Rejected: the old count from issue states that look done — it also counted closed issues, so it could disagree with a list of merges
Decided: today stays the last 24 h the board already used, not a UTC calendar day
Rejected: UTC midnight — for an owner at UTC+7 the list would empty at 7 am
Decided: a merged issue's card is drawn from its done row, since merged issues are not queue rows
Because: crates/server/src/screens.rs:1303
Check: cooee_screens done_today_opens_todays_merged_events_and_a_row_opens_its_issue_card; parity_webkit done_today_opens_the_list_a_row_opens_its_issue_card_and_back_returns runs in ui-webkit only (no Playwright on the lane host)
Owner: tap done today on a fleet card to see what merged today, then tap one to open its card

<sub>note on 2dd7127 · git log --notes=why</sub>

