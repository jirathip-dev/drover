Closes #984

## For humans
Each host can now cap how many lanes are actively working at once (`max_working`) and how many iOS simulators run (`max_simulators`), across all fleets.
Starts over a cap wait their turn in the queue and show "waiting for a slot" instead of piling load onto the machine.
Idle lanes, and issues that are ready, need a human or are parked, take no slot. Reviewer panes close once their verdict is in, and builder panes close once review has started.
`drover status` shows each capped host's usage. Nothing changes until a host sets one of the keys.

## What changed
- `[hosts.<name>] max_working` / `max_simulators` (unset = no cap).
- drive checks host slots before every start: the inline path in `act`, and the kicked path in `launch`, which also counts spawns still starting. A full host is the soft refusal `HOST_FULL`, so it is retried in queue order.
- A lane holds a working slot only while its agent is busy and its issue is not ready/needs-human/parked/blocked/done; a `sim` lane holds its simulator slot while its pane lives.
- `check` closes an idle rev/verify pane once its verdict is logged, and an idle impl/fix pane once a review spawn followed it (worktrees stay).
- `drover status` prints `<host>: working N/cap · simulators N/cap`.
- Decided: a hand-typed `drover spawn` is not held, because the cap is drive's queue discipline. That avoided a second check in placement (see why note).

## Line budget
src/ net +98 (budget none given): this is the spec's surface, trimmed from +163. The cap fits after #807 merged (q1667: wait for #807/#721, no cap change): 44028 / 45400.

## Tests
Evidence at head `4b6eceb78fc43c0a99e3c54134fd9bf5e91fe8d2` (the code is identical to c717aa4b; 4b6eceb only removes three test comments):
- `cargo fmt --check`: exit 0
- `cargo clippy --all-targets --locked -- -D warnings`: exit 0 (at c717aa4b)
- `drover arch-check --config .drover/arch.toml --base origin/staging`: exit 0, 0 violations
- `drover cap-check --config ci/cap.toml`: exit 0, 44028 / 45400
- `cargo test --locked --test host_slots`: exit 0, 5 passed (all four acceptance tests plus kicked starts counted against the cap)
- `cargo test --locked --no-fail-fast` (at c717aa4b, run with a scratch `HOME` and no lane env vars): exit 101, with one failure. `handover::a_handover_fails_no_request_and_loses_no_live_event` hit a 60 s socket read timeout under host load; rerun alone (`cargo test --locked --test handover`) it gives exit 0. The rest of the suite passed.
- Why the scratch `HOME`: tests write sick-disk markers to the real `~/.cache/drover/sick-disks` and read `DROVER_LANE`, so a run inside a lane picks up host state.

Sim goldens moved on purpose: `the_known_fleet_gives_exact_metrics` idle lane time drops from 5561 s to 1004 s because finished panes now close. The planted hunt bug now trips on a lane's 3rd start (lanes restart less often), with hunt seed 11. It is found at round 5, and random seeds 2–7 don't hit it.

## Why (round 2)

Decided: `[hosts.<name>] max_working` / `max_simulators` caps, unset = no cap, checked by drive before every start (inline spawn in `act`, kicked spawn in `launch` counting spawns still starting)
Because: "A per-host cap on **working** lanes across all fleets, set in config"
Because: q1621
Decided: a lane holds a working slot only while herdr reports its agent busy and its issue is not ready, needs-human, parked, blocked or done
Because: "A lane idle at its prompt, waiting on CI, waiting on an owner ask, at `ready`, `needs-human` or `parked` does not count."
Decided: a full host is the soft refusal `HOST_FULL` (pending SOFT, waiting HOST_WAIT), so drive retries it in queue order and status shows `waiting for a slot: N/cap working on <host>`
Because: "it is never refused or dropped, and starts in queue order when a slot frees"
Decided: an idle rev/verify pane closes once its verdict is logged; an idle impl/fix pane closes once a review spawn followed it; worktrees stay
Because: "a rev/verify lane's pane closes once its verdict is recorded"
Rejected: a second slot check in spawn placement (`hosts::at`) — it duplicated drive's check and put the branch 68 lines over ci/cap.toml; a hand-typed `drover spawn` is the owner's explicit start (docs/hosts.md says so)
Check: crates/cli/tests/host_slots.rs drives the real `drover drive` / `check` / `status` entry points for all four acceptance tests
Check: crates/cli/tests/sim goldens (idle 1004, planted bug on the 3rd start, hunt seed 11) move because finished panes now close
Owner: Hosts can now cap how many lanes work at once and how many simulators run; extra starts wait their turn instead of overloading the machine.

<sub>note on 4b6eceb · git log --notes=why</sub>

