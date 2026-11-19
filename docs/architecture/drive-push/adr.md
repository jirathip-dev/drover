# ADR: drive-push — boats send, fleets loop alone, slow work leaves the pass (#1309)

## Summary

Decision: each boat sends every lane change to the Mini as it happens, over the Mini's own ssh connection kept
open; lane commands wait in today's outbox until the Mini confirms them. Drive reads only what has arrived, each
fleet drives alone, and slow work (spawns, merges) runs beside a pass, never inside it.
Slices: #1308 first; a local view of every host; the watcher and the link; a loop per fleet; passes that only plan.
Open questions: load the service job on the Mini? Accept that the Mini dials the boat? Raise the line cap by
about 600? Delete the unused lanes-over-HTTP path?

## Status

Proposed (architect lane, 2026-11-18, staging `9ea69d9fb`). Read `usage.md` first, then this, `shapes.md`,
`boundaries.md`.

## Question

A call to a boat costs 0.7 s and one fleet can hold 27 PRs in flight. How does drive learn of a lane change on a
boat as it happens, act on it within about 5 s, and keep every fleet's pass under 60 s, with nothing lost when the
Mini sleeps, a boat reboots or the network splits?

## Where it stands

- Measured on 2026-11-18 (#1307, #1308): 4 boats, about 40 lanes; 0.64–0.84 s a call on a reused connection, 2.4 s
  on a fresh one; passes of 120–620 s; the tick stopped by self-heal 38 times; no finished pass for three fleets
  for over an hour.
- Three pollers read the boats, each for itself: the tick (check, then the slow pass), the fast step every 60 s
  (one look per host, then an outbox drain per lane) and serve's refresh every 10 s. A read the look lacks runs
  live, one ssh call per fact.
- One pass drives every fleet: a shared part under one lock, then the fleets in threads, joined. Inside a fleet
  each issue's steps run one after another in the pass, a merge's premerge build included.
- Spawns already leave the pass (a detached kick holds a start-slot marker), but the pass then waits up to 20 s
  for them, and admission calls the boats.
- What exists and is kept: the outbox (durable, per worktree, deduped by lane and time), `kick` (one issue's steps
  under its own lock, coalesced), leases, retry-safe action ids (#509), a lock per fleet, start-slot markers
  (#1186, #1269), the GitHub webhook with its catch-up poll (#626), shed-native's typed sessions (#1053), and
  `drover service` (#345): built, with "one kept-open SSH connection per host" and "push → review < 5 s" left
  deferred, and not loaded on the Mini (its jobs are tick, fast, serve, update and clean).

## Options

### 1. What runs on the boat

| Option | Carries | Why not |
|---|---|---|
| A. shed's events alone: the Mini follows each boat's shed stream (or shed's machine links) | lane state | no turn-end, outbox line, worktree fact or host load; shed keeps 1,000 events; delivery would hang on another repo's protocol; a second channel is still needed for the rest |
| B. A drover agent installed on each boat | everything, link or no link | a supervised job per boat to install, watch, restart on every self-update and alert on; it buys only what a report on reconnect already gives |
| C. Extend the outbox: lanes write more into it | what a lane's own command says | nothing when a lane dies, idles or pushes without a command; still pulled |
| **D. The outbox plus a watcher that lives with the link (chosen)** | everything | — |

D is C made complete with A's source and without B's daemon. The lane's own commands stay in its outbox: written
by the lane, durable with no link. Everything else is state, not history: what shed lists now, what each worktree
holds now. A watcher reads both on the boat and sends changes; when it was away, its first frame is the whole
truth, so nothing it missed matters.

### 2. Transport

| Option | Why not |
|---|---|
| The token proxy's reverse tunnel: the boat posts to its own 127.0.0.1, tunnelled to the Mini | needs option B's sender, an HTTP endpoint and a credential on the boat that any lane there could read; a dropped tunnel is re-made only by the next spawn or survey |
| The token proxy itself | it exists to hold the one model token; widening it is a security trade for no gain |
| HTTP on the tailnet (`[service] url`, built in #345) | every cloud boat would join the tailnet with a key of its own and gain a route to the Mini; today a lane on a boat has no key, token or route |
| **The Mini's own ssh connection, kept open (chosen)** | — |

The Mini already dials every boat. One of those calls stays open and its output is the stream. No listener, no
tunnel, no new credential, and the rule "a boat never connects to the Mini" holds. A closed pipe is the liveness
signal, seen at once. Every other call to that host rides the same connection (#1308, #345's deferred c).
Honest limit: the boat writes the moment something happens, but it cannot dial. No option reaches a sleeping
Mini; here the Mini redials on waking and the first frame brings it level. The frames do not depend on the pipe:
a boat-dialled transport could carry the same ones later.

### 3. Where the loops run

The timers alone cannot hold a stream. serve could, but the board must never drive. **Chosen: `drover service`**,
built for this, running one thread per link and one task per fleet. With it stopped everything works by poll: the
service makes drover fast, never correct.

### 4. What a pass is

- Today's pass made faster (#1308): the quick win, but a 10-minute premerge build still holds a fleet's pass.
- A pass per fleet that still acts inline: fleets stop delaying each other; issues inside a fleet still do.
- **A pass plans, a job acts (chosen).** The pass reads local state, admits starts and kicks the due issues. Each
  issue's steps run in its own job, which is today's kick. The per-issue code moves; what it does stays.

## Decision

1. **The outbox stays the durable half.** A lane on a boat appends its command to its worktree's outbox, as today.
   A line's id is its lane and send time, made strictly rising. It is re-sent until acked and applied once.
2. **A watcher is the boat's end** (`usage.md`). The Mini starts it over ssh; it ends with that call. It tails
   every outbox, follows the boat's own shed events, and stats each worktree's head and verdict file twice a
   second. It writes one JSON frame a change, a beat when idle, and a whole report first and every 10 minutes.
3. **A link is the Mini's end**: one per host in the service, redialled with backoff, cut when silent for 20 s.
   The Mini is a host too: its own lanes come through the same watcher, with no ssh.
4. **Intake is the only writer.** It applies each frame: the view's rows, the `lane-state` stamp, an outbox line
   run as its lane, the cursor. It returns whom to wake.
5. **The view is the only reader's source.** Sessions, worktree facts, host load and link state are read from
   `state.db`. No pass, check or board request calls a boat to read. A host with no fresh word reads as
   unreachable, which every caller handles today.
6. **A loop per fleet.** A pass is one process for one fleet, under that fleet's lock, on that fleet's clock: a
   wake, or `fast_secs`. It ends itself at `pass_max_secs` (60). The shared lock and the fast/slow split go.
7. **A pass plans; jobs act.** A wake that names an issue starts that issue's job at once. Spawns, merges,
   builds, prompts and cleans run only in jobs, each under its issue's lock with its own limit, at most `job_max`
   a fleet. A fleet merges one PR at a time under a new lease. Nobody waits for a job: its events are its result.
8. **One admission.** Every spawn takes a start-slot marker first, under the one short lock all fleets share,
   reading only the view and the log. Weights, caps and backoff are today's.
9. **Polling is the fallback.** While a host's link is not live, or with no service, the fast step takes one
   report per host. It is the same frame through the same intake.

## When things break

| What happens | What the design does |
|---|---|
| The Mini sleeps | The ssh dies and the watcher ends. Lanes keep working and keep appending to their outboxes. On waking the link redials; the first report lists every line not acked and the current state; lines apply in order, once. |
| A boat reboots | The pipe closes; the link redials with backoff. Outboxes are on disk. The report shows sessions exited or gone with their cause from shed's list, so each dead lane is handled once, as today. |
| A cloud boat is stopped (idle stop, budget) | Its link is `off`: no dialling, no alert. `cloud-start` dials again. |
| The network splits, or goes half-open | No frame for 20 s cuts the link; a watcher whose beat cannot be written ends itself. The host's lanes read `host offline` after two missed polls and are neither respawned nor superseded. `link-down` alerts after 5 minutes. |
| The service is down | The fast step polls every host each minute. Passes still read only the view. A services row says `polling`. |
| The boat runs another drover build | The link shows `updating` (today's `HOST_UPDATING`) until self-update levels it. |
| A frame arrives twice, or late | Dropped by its boot and number. A line already applied is dropped by its id. An old boot's frame after a newer report is dropped. |
| The Mini crashes while applying | The cursor moves last. A line that ran but was not logged runs once more, as today. |

## Self-heal, the SLOs and `drover check`

- **Self-heal (#1307)** stops being "stop the whole tick". Each unit heals itself and only itself: a pass ends
  itself at its limit and names its step; a frozen pass loses its lease and that one process is stopped; a link
  redials; a job over its limit is killed and backs off; the service's watchdog restarts the service. The tick is
  check and chores only, so #1307's progress rule guards a short job.
- **`slo_tick_minutes`** keeps its meaning and its default 10; the stopgap 30 goes back. A tick no longer drives:
  it starts each fleet's chores detached and never waits for a pass.
- **`slo_pass_minutes`** stays per fleet with a new default of 5: no finished pass of a fleet for that long is the
  `Drive is stuck` card. Its heal is new: stop that fleet's stuck pass, never another's.
- **`pass_max_secs`** drops from 120 to 60 and becomes a limit, not a log line.
- **New: event-to-action.** Each wake is timed on the Mini's clock from its arrival to its job's start.
  p95 over `slo_act_secs` (5) for 10 minutes alerts `slo-act` once.
- **`drover check`** reads lanes, worktrees and outboxes from the view: no ssh for a lane fact. `outbox-stale`
  becomes a local read. `host-offline` and `host-online` follow the link at once, not the 5-minute survey. New:
  `link-down` and `link-up`, once per state. The hourly tag and health probe and the cloud idle stop keep their
  own calls.

## Consequences

- One reader of the boats replaces three. The board is as fresh as the link; serve's remote refresh is deleted.
- A boat's cost is paid once: a slow boat delays its own lanes' frames and jobs, not a pass and not another host.
- The Mini's own lanes and a boat's are read the same way; the "is it remote" branches in the readers go.
- Lane commands from a boat arrive in about a second instead of within a minute.
- A premerge build no longer holds a fleet: other issues' verdicts and starts go on beside it.
- Up to `job_max` jobs of a fleet run at once, so merges need their own lease (new) and every spawn a marker (a
  kick's spawn takes none today).
- A pass and a job are separate processes: about a second to start each on today's log (a read-only command
  takes 0.6–1.1 s there), inside the 5 s target.
- Without the service the design still holds, at poll speed: word from a boat is up to a minute old.
- `src/` grows by about 550 lines net, about 800 at its peak before the last step: a rough estimate (open
  question 3).
- The service's API address must move off serve's webhook port: both default to 7778 today (step 2).
- Not in these steps: a command's exit code sent back to the lane with its ack; the git relay; shed's machine
  links; moving the board into the service.

## Migration: one issue a step, each safe alone, each queued blocked on the one before

Every step's test uses one fixture: a fake remote that costs 0.7 s a call on the test clock, counts calls per
host and can feed and cut a stream (`usage.md`).

| Step | Builds | Deletes | Targets (each asserted by a test) | Alone it is safe because |
|---|---|---|---|---|
| 0. #1308 (filed) | timing per sub-step and host; one round trip per host; hosts in parallel; one ssh master | the per-fact reads | its own: 30 lanes on 4 hosts under 20 s; a real tick under 3 min | — |
| 1. The view | frames, intake, the view; the report as #1308's round trip, the Mini included; every reader moved | remote reads in waiting, lanes, pickup, check and serve; the outbox drain; the look's ssh batching | a pass makes 1 call per host before it plans and 0 after, on a runner that fails any other; 30 lanes on 4 hosts under 5 s; event-to-action at most `fast_secs` + 5 s | same data, same poll; a failed report is today's unreachable host |
| 2. The watcher and the link | watch, ack, the link in the service, `link-down`, the `links` services row | the service's own shed follower | a `turn done` on a fake boat starts its job within 5 s (p95 over 100); 0 calls per pass on a live link; 100 lines across 3 cut streams: none lost, none run twice; a 10-minute sleep, a reboot and a half-open link each come level on redial | service stopped or link down is step 1 |
| 3. A loop per fleet | a pass per fleet as its own process with its own deadline; the fast job and the tick start passes and never wait; a task per fleet in the service; per-fleet heal; the SLO changes | the shared drive locks; the fast/slow pass flag; whatever #1307 adds to share one tick between fleets | with one fleet's pass hung, every other fleet's last-pass time still advances each `fast_secs`; a wake starts its fleet's pass within 1 s; the hung pass ends at 60 s and names its step; 5 fleets, 40 lanes: all passes done within 10 s | each fleet's pass is today's pass for one fleet |
| 4. Passes plan, jobs act | the plan; the per-issue body as the job; `job_max`; the merge lease; one admission for every spawn | the 20 s settle wait; the 120 s pending rewrite; spawns without a marker | with a 10-minute fake premerge and a 3-minute fake spawn in flight, every pass ends under 5 s and another issue's verdict is recorded within 5 s; a start returns to the plan in under 1 s | a job is today's kick; the plan only stops doing inline what kicks already do |
| 5. What is left | the docs, the performance table's new rows, a one-hour soak | lanes over HTTP (open question 4); dead `[limits]` keys | soak with 4 fake boats, 40 lanes, 0.7 s a call: p95 event-to-action at most 5 s, longest pass at most 60 s, 0 remote reads in passes; `src/` net at most −250 | deletes only what nothing calls |

What this ADR asks of #1308: its one-round-trip blob is the `Report` in `shapes.md`, printed by the report verb in
`usage.md`. If it lands in another shape, step 1 converts it.

## Open questions for the owner

1. **Load `drover service` on the Mini?** It is built but not loaded; only tick, fast, serve, update and clean
   run. The 5 s target needs it, and it is a launchd change only the owner makes. Recommended: yes, at step 2,
   beside serve (not replacing it), with fast kept as the fallback poll.
2. **The Mini dials, the boat sends.** The issue names three transports; this ADR picks a fourth, the Mini's own
   ssh call kept open. Recommended: accept; it adds no listener, tunnel or credential, and the frames are the same
   if a boat-dialled transport is wanted later.
3. **The line cap.** Headroom is about 550 with the open PRs counted; the steps add about 800 before step 5 gives
   about 250 back (rough). Recommended: one raise of 600 in one orchestrator-filed issue before step 2.
4. **Delete lanes over HTTP** (`[service] url`, the service's command endpoint, lane tokens)? The live config does
   not set it, and after step 2 it is a second way to deliver a lane's command. Recommended: delete in step 5.

## `.drover/arch.toml` lines

No central list or generated file is converted, so there is no `frozen`, `generated` or `[[collect]]` line. New
code lands in new files (`boundaries.md`), so no `shared` line is added either. Each step's PR adds its bans:

```toml
# step 1
[[ban]]
pattern = "outbox.jsonl"
paths = ["crates/*/src/**"]
except = ["crates/core/src/outbox.rs", "crates/core/src/link/**"]
reason = "#1309: a lane's outbox is written by core::outbox and read by core::link only"

[[ban]]
pattern = "lane_view"
paths = ["crates/*/src/**"]
except = ["crates/core/src/link/**", "crates/core/src/db.rs"]
reason = "#1309: the view is written by link::intake and read through link::view::View"

[[ban]]
pattern = "hosts::On"
paths = ["crates/core/src/waiting.rs", "crates/core/src/session/list.rs"]
reason = "#1309: a lane fact is read from link::view::View; only link lists another host's sessions"

# step 4
[[ban]]
pattern = "hosts::On"
paths = ["crates/core/src/drive/plan.rs", "crates/core/src/starts.rs"]
reason = "#1309: a pass and admission never call a host; effects run in drive::job"

[[ban]]
pattern = "cli::run"
paths = ["crates/core/src/drive/plan.rs"]
reason = "#1309: a pass plans; an issue's steps run in its job (drive::job)"
```
