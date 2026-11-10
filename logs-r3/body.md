Closes #678

## For humans
Running clean or reclaim by hand now also frees space on a host that is switched off for new lanes, as long as it answers.
A host that does not answer is still skipped after one short check and reported offline.
On such a host, a lane's leftover build folder goes unless that lane is still running there. A lane that moved to another host no longer keeps its old copy.
The hourly automatic clean still leaves switched-off hosts alone, so it gets no slower.

## What changed
- New `check::clean_all` is `clean` with a 4th flag `all`; only the explicit `herdsman clean` CLI passes `true`. `clean` keeps its signature and passes `false`, so the tick, scheduled clean, post-merge teardown and the board's lane clean are unchanged (the board file is frozen, see round 2). With `all`, each disabled remote host is asked for its agents once through `Pass` (one bounded probe, #561). If it answers, its worktrees follow the enabled-host rules, using #664's per-host live-agent filter, wip save, remote pane close and wrong-host gitfile check. If it does not, they are kept as `host offline (not cleaned)`.
- New `sweep_off`: on each disabled host that answers, every `.scratch/<agent>` with no live agent of that name **on that host** is listed (dry run) or removed (`--yes`). It runs from `clean` (whole-fleet runs) and `reclaim` (preview / `--apply`). `reclaim` now runs through `Pass` too, so its probe is bounded.
- `trim_hosts` (the tick's build trim) is unchanged and still covers enabled hosts only.

src/ net +78 (no budget in the issue): a probe helper, a scratch sweep with its own keep rule, and the `clean_all` entry. `sweep_in`'s rule keeps an open issue's latest lane, which is exactly the moved-lane case the spec wants gone. Merged onto current staging: 36808 / cap 38028 (cap raised in q967 / PR #761; q969).

## Tests (commit 1 `9eaf2e9` = tests only; they fail there, pass at head)
- `scratch.rs::clean_sweeps_a_disabled_but_reachable_host`: fake disabled-but-reachable Air with real scratch folders (the `Air` runner runs the real sweep script against a temp `$HOME`). The dry run lists the merged lane's worktree and the dead lane's scratch; `--yes` removes both (`herdr --machine air worktree remove` called, folder gone).
- `scratch.rs::a_disabled_hosts_scratch_stays_only_for_its_own_live_agents`: `reclaim --apply` removes the scratch of an agent now live on this host and keeps the scratch of one live on the Air.
- `tick_stuck.rs::clean_reports_a_silent_disabled_host_offline_after_one_probe`: the `Hang` runner (every Air call times out). `clean --yes` reports `host offline (not cleaned)` for the worktree and the scratch; the Air sees only the one 5 s probe.
- `tick_stuck.rs::the_ticks_clean_never_probes_a_disabled_hosts_worktrees` (was `clean_never_probes_…`; the spec changes the explicit clean, so this test now runs the tick). The hourly clean still says `on a disabled host (not cleaned)` and removes or sweeps nothing there.

Repro at `9eaf2e9`: `cargo test -p herdsman --no-fail-fast --test scratch --test tick_stuck` → exit 101 (the 3 new tests fail: `keep air:…: on a disabled host (not cleaned)`). At head: the same command → exit 0.

## Gates at head 5dc0fb8ef8ba543ce589b89db24e7d25d9ee4d15
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `env -u HERDSMAN_LANE cargo test --no-fail-fast` → exit 101. 161 targets pass. Three local WebKit browser tests fail, all in files this PR leaves alone: `chat_tab::the_thread_follows_its_end_until_you_drag_and_drafts_survive_a_reload` (also fails on plain staging, same assertion at `chat_tab.rs:1592`), `config_card::the_card_lands_live_in_view_and_stays_reachable_in_webkit` (hung over 8 min on this host's WebKit, killed by hand), and `live_board::live_board_chat_and_ascii_in_webkit`. At this head CI's `rust`, `ui` and `ui-webkit` jobs are green, and they run the same suite on clean runners. (`env -u HERDSMAN_LANE` keeps the lane's own env out of `self_update`, see round 1.)
- `cargo test -p herdsman --no-fail-fast --test scratch --test tick_stuck --test conductor_cold_start` → exit 0 (7 + 5 + 2 passed)
- `herdsman cap-check --config ci/cap.toml` → exit 0: 37470 / cap 38028

## Round 2 (board-freeze FAIL)
`crates/server/src/serve.rs` is no longer touched: `clean` keeps its 3-flag signature and the CLI calls the new `clean_all`. The diff vs staging is now `check.rs`, `cli.rs` and the test files only (`conductor_cold_start.rs` holds a separate staging build fix, 6186e3b9).

## Live: against the Air (disabled since 11-03), release build at head 5dc0fb8ef8ba543ce589b89db24e7d25d9ee4d15
The Air was already swept once in round 1 (35 → 46 GB free at a84c095, 9 merged worktrees and ~10.7 GB of scratch removed). This run at head removed what had become cleanable since then. `boat1` (disabled, unreachable) shows the offline path live. Mini-local `skip`/`keep … a live lane` lines from `reclaim` are left out.
```
== head 5dc0fb8ef8ba543ce589b89db24e7d25d9ee4d15
== df before
Filesystem        Size    Used   Avail Capacity iused ifree %iused  Mounted on
/dev/disk3s1s1   228Gi    12Gi    29Gi    30%    459k  302M    0%   /
saved /Users/jirathip/.local/share/herdsman/work/cooee/verify-116-r1 to wip/cooee-verify-116-r1-20261110-123800-air-34f7 @ 8afd318
removed air:/Users/jirathip/.local/share/herdsman/work/cooee/verify-116-r1 (cooee#116 merged)
saved /Users/jirathip/.local/share/herdsman/work/cooee/verify-115-r1 to wip/cooee-verify-115-r1-20261110-123800-air-65ca @ 34e49ad
removed air:/Users/jirathip/.local/share/herdsman/work/cooee/verify-115-r1 (cooee#115 merged)
cooee #83 wip-blocked: /Users/jirathip/.local/share/herdsman/work/cooee/verify-83-r1: unsaved work has a secret (shots/drive.mjs:8: …); kept
keep air:/Users/jirathip/.local/share/herdsman/work/cooee/verify-83-r1: unsaved work has a secret (shots/drive.mjs:8: …)
saved /Users/jirathip/.herdr/worktrees/cooee/issue-48 to wip/cooee-impl-48-20261110-123800-air-f7d0 @ 0f993cc
removed air:/Users/jirathip/.herdr/worktrees/cooee/issue-48 (cooee#48 merged)
keep boat1:/home/user/.herdr/worktrees/shed/rev-47-r2: host offline (not cleaned)
keep boat1:/home/user/.herdr/worktrees/shed/rev-49-r1: host offline (not cleaned)
keep boat1:/home/user/.herdr/worktrees/shed/rev-47-r1: host offline (not cleaned)
scratch: air: removed ~/.local/share/herdsman/work/.build/.scratch/cooee-verify-116-r1 0.04 GB (no live lane there)
scratch: boat1: host offline (not cleaned)
clean exit 0
scratch: boat1: host offline (not cleaned)
0.00 GB freed (logical sizes; APFS clones may free less)
reclaim exit 0
== df after
Filesystem        Size    Used   Avail Capacity iused ifree %iused  Mounted on
/dev/disk3s1s1   228Gi    12Gi    29Gi    29%    459k  306M    0%   /
```
`clean --yes` exits 0 at this head. The Air's cooee issue-48 worktree, which round 1 reported as still dirty after its wip save, is now saved and removed. `reclaim --apply` freed 0 GB more because `clean` had already swept the Air's scratch. `df` hardly moves this time because the big leftovers went in round 1 (35 → 46 GB). Between the two runs the Air went from 46 GB to 29 GB free; this run did not look into why.

## Why (round 2)

Decided: an explicit clean or reclaim probes each disabled host once through Pass; a host that answers is cleaned under the enabled-host rules
Because: "include a disabled host when it is reachable. One probe with the existing hard timeout"
Because: crates/core/src/check.rs:696
Decided: on a disabled host a scratch folder stays only while an agent of that name is live on that host
Because: "A lane that moved to another host (same agent name live elsewhere) does not keep the old host's copy."
Because: crates/core/src/check.rs:1195
Decided: clean keeps its signature (all=false); only the CLI calls clean_all, so serve.rs (board-freeze) and the tick are untouched
Because: "Drive's hourly clean and build trim stay limited to enabled hosts (no tick slowdown)."
Because: crates/core/src/timer.rs:271
Rejected: a 4th tuple flag on clean itself — it edits crates/server/src/serve.rs, which board-freeze refuses without a board-fix issue
Rejected: reusing sweep_in for disabled hosts — it keeps an open issue's latest lane, so a moved lane's old copy would stay
Check: the three new tests in scratch.rs and tick_stuck.rs fail at 9eaf2e9 and pass at head; serve.rs has no diff vs staging
Owner: Running clean or reclaim by hand now also frees space on a host that is switched off for new lanes, as long as it answers; the hourly job still leaves it alone.

<sub>note on cc5f929 · git log --notes=why</sub>


