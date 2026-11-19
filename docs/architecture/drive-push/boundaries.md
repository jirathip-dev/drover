# drive-push: module boundaries and statecharts (#1309)

## Who may call whom

```
 on each host                          on the Mini
 lane ─▶ outbox file ─┐
 shed's own events ───┼─▶ watch ══ frames over the kept ssh ══▶ link ─▶ intake ─▶ view  ◀── plan, job, check,
 worktree files ──────┘                                                   │       (state.db)   starts, server
                                                                          ├─▶ the log (lane-state, outbox)
                                                                          └─▶ Wake ─▶ loop ─▶ plan ─▶ job ─▶ effects
```

New code goes in new files, so the `shared` files (drive, hosts, check, spawn) shrink or stand still:

```
crates/core/src/link/frame.rs     Frame and its parts: types only
crates/core/src/link/watch.rs     the host's end: report, watch, ack
crates/core/src/link/mod.rs       the Mini's end: open, run, report
crates/core/src/link/intake.rs    apply: the only writer of the view
crates/core/src/link/view.rs      View: the only reader of the view's tables
crates/core/src/drive/plan.rs     a fleet's pass
crates/core/src/drive/job.rs      an issue's job (the pass's per-issue body, moved)
crates/service/src/links.rs       a thread per host
crates/service/src/loops.rs       a task per fleet
```

| Module | Owns | May call | Must not |
|---|---|---|---|
| `link::frame` (new) | `Frame`, `Report`, `Seen`, `Worktree`, `Line`, `Load`, `Watch` | serde | do any I/O |
| `link::watch` (new, runs on a host) | the report, the stat poll, the outbox tail, the ack file | this host's shed socket, local git through the runner, the filesystem | read the log or a fleet.toml, open a network connection |
| `link` (new, the Mini's end) | `open`, `run`, `report`, the link's state and lease | `runner.stream` (ssh, or the local child), `intake` | call drive, spawn or term; decide anything about an issue |
| `link::intake` (new) | `apply`: the cursor, the view's rows, the `lane-state` stamp, running an outbox line | `session::stamp`, the events, the CLI's own run of a lane verb (as `outbox::apply` does today) | call a host; wake anything itself (it returns `Wake`s) |
| `link::view` (new) | `View`, `Link`, `HostSeen` | the database, read only | start a program |
| `outbox` (kept, smaller) | `lane`, `queue`: the lane's write | the filesystem | read another host: `drain`, `look`, `stale` and `take` are deleted |
| `drive::plan` (new) | a fleet's pass: starts, due issues, the `drive-pass` event | `View`, the events, `gh` (the fleet's PR sync), `waiting::scan`, `starts::admit`, `job::kick` | `hosts::On`, `term`, `spawn::spawn`, `gate::merge`: nothing that reaches a boat or runs a build |
| `drive::job` (new) | an issue's due steps, under its lock and a job slot | `gate`, `spawn`, `term`, `land`, `hosts::On` for effects, `View` for facts | read a boat fact over ssh; wait for another job |
| `starts` (kept, grows by drive's `launch` and `admit`) | start and working slots, the marker, fleet weights | `View`, leases, the events | start a program |
| `spawn` (kept) | a lane's start, under a marker | as today, plus `View` for its host's gate | be called from `plan` |
| `session` (kept) | `Session`, `Status`, the stamp | `shed` on this host only (`list::on`) | list another host: `session::all` over ssh is deleted, `View::sessions` replaces it |
| `snapshot` (kept, smaller) | the board's cache of this host's own reads | the runner, locally | hold a remote read: `Miss::Collect` and the ssh batching go |
| `check`, `stall`, `slo`, `waiting`, `lanes` | their rules, unchanged | `View` for facts; a host only to act (close a pane, nudge, clean) | call a host to read a lane fact; `waiting` calls none at all |
| `crates/service` | a thread per link, a task per fleet, the job slots | `link::run`, the runner (`drive`, `kick` as child processes) | call `plan` or `job` in its own process |
| `crates/server` | the board | `View`; a host, only for what a person asks of one lane (a prompt, its transcript), as today | ssh for a lane fact: its remote refresh (`serve_remote_secs`) is deleted |

Three callers reach `intake::apply` and no others: the service's link, the fast step's catch-up poll, and
`drover hosts refresh`. Core gains no layer edge.

## A host's link

```text
 [off]  the host is disabled with no open lane, or a stopped cloud host
   │ hosts on · cloud-start · a lane is placed there
   ▼
 [dialling] ── ssh fails ──▶ [down] ── wait 1 s, 2 s … link_retry_max_secs ──▶ [dialling]
   │ the first frame is a Report: the view is replaced
   ▼
 [live] ── each frame: view, events, cursor ──▶ [live]         a beat at least every link_beat_secs
   │
   ├─ the pipe closes · no frame for link_stale_secs · a gap in n · a line that is no frame ──▶ [down]
   ├─ the frame's version or the Report's build is not ours ──▶ [updating] ── self-update levels the host ──▶ [dialling]
   └─ hosts off with no lane left · cloud-stop ──▶ [off]
```

- While a link is not `live`, the fast step polls that host (one `report`, bounded by `ssh_call_secs`).
- With no service nobody dials: every link reads `down` with the reason `no service`, and the poll is the path.
- A host's lanes read `host offline: <host>` once its last word is older than 2 × `fast_secs`. Drive then neither
  respawns nor supersedes them (#478, unchanged), and they never page.
- `updating` is today's `HOST_UPDATING`: no new lane goes there; its running lanes finish.

## An outbox line

```text
 [written]  the lane appended it to its worktree's outbox, `sent` rising
   │ the watcher sees the file grow (link_poll_millis), or the next Report lists it
   ▼
 [sent] ── intake: an `outbox` event has this (lane, sent) ──▶ dropped
   │ intake: none has
   ▼
 [applied]  run as the lane its worktree's spawn names; `outbox` logged; the issue is kicked with this id
   │ the ack reaches the host
   ▼
 [acked]  left out of every later Report            (an ack lost: sent again, dropped again)
```

## A fleet's loop and its lock

```text
 [idle] ── a Wake for the fleet · fast_secs since its last pass ──▶ [due]
 [due] ── drive-<fleet>.lock is free ──▶ [planning]        held: stays due; wakes meanwhile fold into one more pass
 [planning] ── the plan is logged (`drive-pass`) ──▶ [idle]
 [planning] ── pass_max_secs ──▶ the pass ends itself, `pass-slow` names its step ──▶ [due]
 [planning] ── its process died or froze: the lease runs out at 2 × pass_max_secs ──▶ `pass-stuck`, taken over ──▶ [due]
 [paused]   the fleet is paused: no pass; jobs of its live lanes still run
```

No state of one fleet's loop reads another fleet's lock. The only things fleets share are `admit.lock` (held for
milliseconds), the hosts' slots, and the service's job slots.

## An issue's job

```text
 [none] ── kicked with a new id ──▶ [queued] ── a job slot of its fleet (job_max) and drive-<fleet>-<n>.lock ──▶ [running]
 [running] ── its steps are done: their events are logged and wake the fleet ──▶ [none]
 [running] ── kicked again ──▶ `.again` is set: one more run after this one
 [running] ── over its step's limit ──▶ killed, `failed` for its action id ──▶ backoff, 5 min doubling to 1 h, as a spawn today
 a merge step ── merge-<fleet>.lock is held ──▶ it waits its turn in the job; no other issue's job waits on it
```

## A start slot

```text
 [free] ── plan or job: starts::admit under admit.lock ──▶ [held · admitted]     the marker spawn-<fleet>-<n>.pending
            (reads the view and the log only: the host's start and working slots, its load, memory and disk,
             the fleet's weight and caps; refuses SPAWN_PENDING, HOST_FULL, HOST_BUSY, HOST_OFFLINE, IN_FLIGHT_CAP …)
 [held · admitted] ── the start job runs: worktree, brief, harness ──▶ [held · starting]
 [held · starting] ── `spawn-started`, start_release = started ──▶ [free]         the lane now holds a working slot
 [held · starting] ── `spawn` (prompt confirmed), start_release = confirmed ──▶ [free]
 any held ── `spawn-error` · `failed` ──▶ [free], backoff
 any held ── spawn_pending_max_secs · its job's pid is gone ──▶ [free], `stale-marker`; the issue goes behind the other starts (#1269)
```

Rules the charts fix:

- **A pass waits for nothing.** It never holds its lock while a spawn, a merge, a build or a call to a boat runs.
  Today's 20 s `settle` wait and the 120 s rewrite of a pending action are deleted; a pending action is read from
  the events that followed it.
- **Every spawn takes a marker first**, whoever asks (a start from the plan, a review or fix from a job), through
  the one `starts::admit`. Today a spawn made inside a kick takes none.
- **Admission never calls a host.** Its numbers come from the view, at most `link_beat_secs` old on a live link.
  A host with no fresh word admits nothing (`HOST_OFFLINE`).
- **A slow or dead boat costs only its own lanes.** Its link is one thread; a job's call there is cut at
  `ssh_call_secs`; no pass and no other host's link waits.
- **Polling is the fallback, never the path.** A pass makes no remote read with or without the service; without
  it, word from a boat is at most `fast_secs` old instead of a second.
