Closes #1030

## For humans
In the phone app's Queue: a paused fleet folds to one line with a Resume button, and fleets that need you come first with a dot on their repo chip.
Blocked chains show as a tree you can fold, long lists hide behind "+N more", and rows can be moved up and down.
Tapping a row opens an issue card with its stage lit, links to the issue, PR and checks, and the open question with its options; a legend explains every state, and a backlog lets you search, tick and queue issues.
Rows holding serial or hot-file locks show their 🔒 tag.

## What changed
- `crates/server/src/screens.rs`: `queue_screen` builds each parity row (#553 rows 36–48) from blocks cooee already draws; new issue, legend and backlog sheets; a `move` action (▲/▼) running the same `queue mv` as Top; `more=`/`fold=` query keeps "+N more" and chain folds across refresh.
- `cooee-plugin.toml`, `crates/cli/tests/golden/cooee/{queue,permissions}.json`: `move` and `queue` actions, Legend menu item. cooee's plugin pin must take the same.
- No new block type, so no native snapshot is owed.

Out of scope / not done here: per-fleet hue, swipe between repos and a live "[queue N]" count need cooee renderer changes; answering asks inline is the t113 issues. Phone checks (AC 2–3) need a TestFlight build and were not run in this lane.

`src/ net +314 (budget: none stated)` — one screen gaining six rows, three sheets and one action.

## Update round (merge of origin/staging)
Resolved: `cooee-plugin.toml` and `permissions.json` (actions are the union: staging's kill/clean/close/send plus this PR's move/queue), `crates/server/src/screens.rs` (staging's titles from the cache, short-line `cut`, PR number in the subtitle, Lanes sheet, ask sheet and kill/clean/close/send handlers kept; a live row now taps the Lanes sheet, as staging made it; a non-live row still taps the issue sheet). `crates/cli/tests/gh_links.rs` (new on staging) asserted a row tapping straight to the PR; it now opens the row's issue sheet and asserts its issue, PR and checks links (or the latest run, or none).

## Evidence (head cb58b4a5c10639be6a9d68a34afd8c930c6c47d9)
Update round: merged origin/staging (#1029, #895, #808); clean merge, the v1.rs fix and the perf.rs issue read kept.
- `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings` → 0
- `env -u DROVER_LANE cargo test --locked --no-fail-fast` → 0

## Evidence (earlier head 3a2c36330c337826c8df86257ec9b02f88257788)
Fix round 3: CI `rust` was red on `tests/perf.rs`'s 150k-event test ("a board read built an ask index from a copied slice of the log", left 1, right 0). Cause: a perf test running alongside it reads `/api/v1/issue/…`, whose pending lookup ran on a filtered `since` copy of the log and bumped the shared rebuild counter. Fix: `serve/v1.rs` `issue` finds its pending start in the cached working log (as `pending_cards` already does); the 150k test now reads `/api/v1/issue/sendmeter/1000` itself, so the regression fails every run, not only on a race. Also merged origin/staging (#898 serve health).
- `cargo test -p drover --test perf the_state_read_stays` with the v1.rs fix stashed → 101 (left 1, right 0); with it → 0
- `cargo test -p drover --test perf` → 0 (14 passed)
- `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings` → 0
- `env -u DROVER_LANE cargo test --locked` → 0 (with the lane's `DROVER_LANE` set, `self_update`'s pause test records the lane as `by`; env-only, untouched here)

## Evidence (earlier head 7d78274b737e37564adf8775a88d093f64f03a27)
Update round 2: merged origin/staging (#895's serve soak, #1072 Threads, the queued rows' wait line). `screens.rs` conflict: `row` takes staging's `paused` for `pending::waits` but no repo (rows tap the issue sheet, not the PR); an open ask's options line is kept and the wait line is put before it on queued rows; the chain/fold/"+N more" rows pass `paused` through.
- `cargo fmt --check` → 0
- `cargo clippy -q --all-targets -- -D warnings` → 0
- `cargo test -q -p drover --test serve_web` → 0 (13 passed)
- `cargo test -q -p drover --test board_fresh --test cooee_token --test cooee_screens --test demo_export --test gh_links --test lanes_sheet --test owner_extend --test queue_titles --test queue_graph` → 0
- `cargo test -q -p drover-server` → 0
- `tests/page.rs` not run here (no Chrome on this host); CI's `ui` job runs it

## Why (round 1)

Decided: every row is drawn with blocks cooee already has (card fold, row indent/link/swipe, chips, form, toggle, button, sheet); no new block type, so no native snapshot is owed
Because: "reuse existing blocks; add a block only where none fits, with its snapshot"
Decided: "+N more" and a chain's fold are link rows opening the same screen with more=/fold= in the query, the shape of cooee's own fixture row "3 more"; the stream URL carries them so a refresh keeps the view
Because: crates/server/src/screens.rs:197
Decided: a paused fleet's card gets a fold line with Resume as its main button; a fleet that needs you sorts first, is tone warn and its repo chip gets a ● dot
Because: crates/server/src/screens.rs:312
Decided: ▲/▼ are swipe actions (and buttons on the issue sheet) through a new `move` action that runs the same queue mv as Top, logged via cooee
Because: crates/server/src/screens.rs:1006
Decided: a non-live row taps into an issue sheet: lit stage chips, issue/PR/checks links, the asking thread with the ask's options, notes, ▲/▼; a live row keeps its lane-thread tap, a waiting row taps the asking thread
Because: crates/server/src/screens.rs:823
Decided: the backlog is a sheet: filter chips, a search field, a toggle per issue, done "Queue ticked" queues them needs-check like the board's bqueue
Because: "no backlog (search, filters, tick, [queue N])"
Rejected: a per-fleet hue, a live "[queue N]" count and swipe between repos — cooee's tones, static done label and gestures are cooee changes
Check: manifest and permissions fixture gain move and queue; queue.json fixture gains the Legend menu item — cooee's plugin pin must take the same
Owner: The phone Queue folds paused fleets, shows blocked chains as a foldable tree, hides long lists behind "+N more", moves rows up and down, and adds an issue card, a legend and a backlog.

<sub>note on 3a14050 · git log --notes=why</sub>

## Why (round 2)

Decided: merge origin/staging; row takes staging's paused flag for the wait line but no repo, since rows tap the issue sheet rather than the PR
Because: crates/server/src/screens.rs:130
Decided: a row with an open ask keeps its options line; a queued row's wait line goes in front of it
Because: "46: \"waiting: decision\" shows no options and no link to the asking thread."
Rejected: staging's direct PR tap on the row — the issue sheet carries the issue, PR and checks links
Check: a queued row on a paused fleet shows its wait line; a row with an open ask shows its options
Owner: Brought this change up to date with staging again; queued rows now also say what they wait on.

<sub>note on 7d78274 · git log --notes=why</sub>

## Why (round 3)

Decided: an issue read finds its pending start in the cached working log, as pending_cards does, not in the filtered `since` copy
Because: crates/server/src/serve/v1.rs:678
Decided: the 150k-event test reads /api/v1/issue itself, so the rebuild it used to catch only by a racing test is caught every run
Because: crates/cli/tests/perf.rs:697
Rejected: serialising the perf tests — the read really rebuilt from a copied slice; the counter was right
Check: without the v1.rs change the 150k test fails at its rebuild assertion (left 1, right 0)
Owner: Fixed the red speed check: opening an issue's details no longer rescans a copy of the event log.

<sub>note on 3a2c363 · git log --notes=why</sub>

