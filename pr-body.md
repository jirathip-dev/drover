Closes #1309

Step 1 ("The view") of the approved drive-push ADR (`docs/architecture/drive-push/`, q2026), which lands with this PR.

## For humans

Each drive step now asks every boat once for everything about its lanes, instead of one call per fact, and keeps the answer on the main host.
Status, waiting, check, the board and drive read lane facts from that answer, so a pass no longer waits on boats after its first call.
A boat lane's own commands (turn ends, asks, verdicts) now arrive within the same step; nothing changes for the lanes themselves.

## Parity

none changed or dropped; `docs/parity.md` is untouched. `crates/server/src/serve.rs` only changes where the lane rows get their data:
- R7 (per-request live I/O) stays `kept`: boat sessions now come from the `state.db` view, so a page load makes no ssh call for them; this host's sessions are read as before.
- 49, 50 (lane rows) stay `kept`: same rows and fields, built from the view.
- 90 (demo export) stays `open`: the export polls once before it renders so its lanes come from the view.

## What changed

- `core::link` (new): `frame` (the `Frame`/`Report`/`Seen`/`Worktree`/`Watch` types), `watch` (`drover hosts report`, hidden: the host's end), `intake::apply` (the only writer of the view; runs each outbox line once by its `(lane, sent)` id), `view::View` (the only reader: `sessions`, `worktree`, `lines`, `stale`, `here`), `look`/`look_at`/`fresh` (the poll: one report per host, the main host's in process, boats in parallel).
- `state.db`: `host_view` and `lane_view` tables (`db::view_put`, `db::view`).
- Readers moved to the view: drive's pass and its readers (sessions, orphans, slo, repair), `waiting` (head, verdict.md, uncommitted state), `drive::pickup`, `gate::verdict` and NO_PROOF's verdict body, `check` (sessions, dead/parked lanes, stale panes, `outbox-stale` from reported lines), `hosts::survey`, `lanes::join` (parked), `land::auto_update`, `status`/`waiting`, and serve's boat sessions.
- Deleted: `session::all`, `outbox::{drain, look, stale, take, boxes, apply}`, the snapshot's ssh batching (`Miss::Collect` queueing, `Snap::fetch`, `prev`/`want`), `check::with_remote`.
- `outbox::queue`: `ts` is strictly rising under a file lock (two lines in one ms no longer share an id).
- `.drover/arch.toml`: the ADR's three step-1 bans (`outbox.jsonl`, `lane_view`, `hosts::On` in waiting/session list).
- Docs: `docs/hosts.md` (how boat lane facts and outbox lines arrive), `docs/lanes.md`.

## Decided (routine, inside the spec)

- Outbox lines travel inside their worktree's facts and stay in the file until step 2's ack; the log's id runs each once. An ack now would be a second call per host each step.
- The watch names the live lanes' worktrees and the host's configured roots; a root that does not answer (`runner::answers`) is skipped, so a hung disk costs no git call.
- `check`, `status`, `waiting`, `land` and `spawn` poll only a host with no fresh word (2 × `fast_secs`); every drive pass polls all hosts first; a spawn re-reads its own host so the next check sees the new lane; `clean` polls all hosts before it removes anything.
- serve reads this host's own sessions live (as before) and every boat's from the view: from the view alone the board would lag its own shed by up to `fast_secs` until step 2's local watcher.
- The slow pass keeps #1308's reachability probe before its report (2 calls a host); the fast step makes exactly 1.
- `Report` carries shed's error as `shed: Option<String>` (no shed seq yet) and no `Load` (admission moves in step 4); `Seen` carries shed's own `TerminalInfo`.

## Targets (amendment q2026)

- A pass makes 1 call per host before it plans and 0 after, on a runner that fails any other: `perf::a_step_over_30_lanes_on_4_boats_makes_one_report_call_per_host_and_reads_none_after` (strict runner answers 255 to any call that is no report; asserts `{b1: 1, b2: 1, b3: 1, b4: 1}`, then `status` makes 0 calls).
- 30 lanes on 4 hosts under 5 s at +700 ms a call: the same test, `drive --fast` 0.7 s.
- Event-to-action at most `fast_secs` + 5 s: `perf::a_boat_lanes_turn_end_is_acted_on_in_the_next_step_inside_5s` (a boat lane's queued `turn blocked` runs in the next step, which takes under 5 s, logged with `lag_ms`; a second step does not run it again).

## Lines

- src/ net +496 (budget: the ADR plans about +550 over steps 1–4, about +800 at its peak): the five `link` files and the two tables, less the deleted drain, batching and per-fact reads. The owner's q2075 raise (ci/cap.d/2026-11-19-55500.toml) is on staging: merged, 54235 / 55500.
- web/ net +0

no doc: `crates/core/src/cli.rs`'s `cmd: Some(HostsCmd::Report)` is a match arm on the existing `Hosts { cmd }` field, not a new key; the new hidden `drover hosts report` is in docs/hosts.md.

## Evidence (head c9fb3cb673847158af64212fad51937a3bb26354: merge of origin/staging 49f95b77)

Update round: staging conflicted on `crates/server/src/serve.rs` (`sessions`): kept the view read and it now also fills staging's new `listed` cache that `recent_sessions` reads. `screens.rs` lane sheet calls `snapshot::lanes(ctx, …)` (this branch's signature); staging's new `remote_sheet` test takes the step's `look` before it reads, like every remote test here.

- `cargo build --workspace --all-targets`: exit 0
- `cargo fmt --all --check`: exit 0
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` then `cargo clippy --workspace --tests --locked -- -D warnings -A clippy::unwrap_used …` (CI's flags): exit 0
- `scripts/test-shards.sh check`: exit 0
- `scripts/timing-check.sh`: exit 0
- `scripts/parity-check.sh HEAD^1`: exit 0
- `drover lesson-check`: exit 0
- `drover cap-check --config ci/cap.toml --base origin/staging`: exit 0, 55408 / 55500
- `drover arch-check --base HEAD^1 --pr-body <this body>`: exit 1, its only violation `just gen` not found on this host (CI runs it)
- `cargo test -p drover --test remote_sheet`: exit 0
- `cargo test --workspace --locked --no-fail-fast -j 4 -- --test-threads=4`: exit 101: 236 test targets, 234 ok; `lanes_sheet::a_lane_prompt_replies_before_shed_takes_it…` (a 50 ms timing assert, 78 ms under the parallel load; alone x3: exit 0, 0, 0) and `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it` (this host only, see below)

## Not verified here

- `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it` fails on this lane's host because it has a `shed` at `/usr/local/bin` on the timer's PATH (doctor reports ✓ where the test expects ✗); the code it covers is unchanged.
- The release-mode perf budgets and the WebKit/ui jobs run in CI only.





