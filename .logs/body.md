Closes #603

## For humans
The Lanes tab on the phone now shows lanes that run in shed (including ACP lanes) as working, blocked or done, the same as `shed ls`, instead of dead or gone. This works when only one fleet's runtime names shed. Opening such a lane shows its transcript from shed. Lanes running in herdr look the same as before.

## What changed
- `serve_on` (`crates/server/src/serve.rs`): if the runner it was given has no shed, the board takes shed from the config (`Config::shed()`, so `[runtime]` or else a fleet's own `[fleet.<n>.runtime]`) and wraps the runner in `shed::With`, as the CLI already does in `cli.rs`. `herdsman service --board` passed the bare `Real` runner, so the board's agent list (`herdr::list`) and pane reads (`herdr::recent`, which feeds the sheet's transcript) never asked shed. With the wrapper, both go to shed for its lanes: shed's `ls` for the list, and shed's `read` (the ACP event history for an ACP terminal, #565) for the sheet. A herdr-only config gets no wrapper, so herdr lanes don't change.
- Regression test `the_lanes_tab_lists_a_running_shed_acp_lane_live` (`crates/cli/tests/shed.rs`): a fleet-only `shed-acp` runtime, a fake shed server on a real Unix socket that lists a running `sendmeter-impl-12`, and `serve_on` given a runner with no shed (like the service). It calls `GET /api/v1/lanes` over TCP and checks that the lane is in the `working` section with `state: working` and `gone: false`.

- Amendment A: the Queue tab's rows and sections, Herd (`/herd.json`) and `/api/v1/lanes` are all built from `fn lanes`, which reads the agent list through the server's runner. That runner is now the shed-wrapped one, so herdr and shed lanes come back together as one list on every view, with no change per view.

src/ net +6 (budget: none given). One wrapper at the board's entry point.

## Repro
Commit 1 (47b0f8805) holds only the test. Command: `cargo test -p herdsman --test shed the_lanes_tab_lists_a_running_shed_acp_lane_live`
- At 47b0f8805 (base plus the test): exit 101, `left: (Some("dead"), Some("dead"), Some(true))`, `right: (Some("working"), Some("working"), Some(false))`
- At head 738fe80bdc3fb590369e3b35fa666e3e58808d4e: exit 0

## Evidence (head 3f56ca84706b948963559b61ef70c0ed7b7f82c0, the merge of origin/staging; src diff vs staging unchanged)
- CI at 3f56ca8: green, 7 checks (incl. `rust`). The `thread_handoff` failure from the previous FAIL (`a_failed_handoff_tap_shows_its_error_and_its_retry_is_the_one_delivery`, at d3847a3) came from staging's code at that point; this PR doesn't touch the conductor or handoff. After merging the current origin/staging (#608 and others), `cargo test -p herdsman --test thread_handoff` exits 0 locally (14 passed).
- `cargo build --release`: exit 0
- `cargo clippy --all-targets -- -D warnings`: exit 0
- `cargo fmt --check`: exit 0
- `herdsman cap-check --config ci/cap.toml`: exit 0 (src lines 31980 / cap 35500)
- Local `cargo test`: exit 101 on every run, because the host keeps deleting this lane's build folder partway through (`could not execute process … (never executed)`). Across the runs that did execute, the only real failure was `self_update::pause_and_resume_are_logged_and_pause_stops_the_job`: it sees the lane's own identity (`herdsman:603:impl`) where it expects `owner`. That comes from the lane's environment, and CI (which doesn't have it) passes. CI is the full-suite gate for this head.

Not verified: AC3, the live phone check of the next real ACP lane (Lanes and Queue tabs). It needs the live service restarted on this build.

