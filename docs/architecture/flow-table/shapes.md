# flow-table: data shapes (#1649)

## The files

```
crates/core/flow/
  issue/   queued.toml design.toml design-review.toml build.toml review.toml ready.toml
           blocked.toml needs-human.toml parked.toml ended.toml
  ask/     routine.toml routed.toml with-owner.toml answered.toml orphaned.toml
  card/    posted.toml approved.toml
  handoff/ pick.toml picked.toml
  rollout/ open.toml proposed.toml
  cap/     near.toml asked.toml raising.toml
  pass/    due.toml running.toml paused.toml
  slot/    held.toml
  host/    holding.toml
  outcomes.toml
```

One file per state of one flow. `crates/core/build.rs` joins the folder into one embedded string, as it does
`roles/usage/` today; no Rust file lists the files. The folder is outside `src/`: rows are data and do not count
toward the line cap.

## A state file

```toml
flow      = "issue"
states    = ["impl", "fix"]          # the fold's names this file covers
reads     = ["lane", "host", "pr", "verdict", "ask", "turn"]
ignores   = []                       # reads + ignores = all six facts, or the file does not load
in_flight = true                     # holds a serial slot, an in-flight slot, a host's working slot

[[row]]
id      = "pushed"                   # full id: issue/build/pushed
when    = { turn = ["done"], verdict = ["none"], pr = ["open", "pending", "red", "green", "conflicting"],
            lane = "*", host = "*", ask = ["none"] }
do      = "spawn-check"
owner   = "drive"
timeout = "act_minutes"
to      = "review"
say     = "pushed {head}: review starts"
```

Rules the loader enforces:

- Rows are tried top to bottom; the first whose `when` matches is the row. Order matters only inside one file.
- `when` names every fact in `reads`: a list of values, or `"*"`.
- `owner`, `do`, `timeout` and `say` are required. `then`, `since`, `escalate` and `to` are optional.
- Unknown keys and unknown values are load errors. So is a `timeout` that is not a `[limits]` key, `subject`
  or `owner-held`.

## Types (`crates/core/src/flow/*`)

```rust
pub struct Cond { pub lane: Lane, pub host: Host, pub pr: Pr, pub verdict: Verdict, pub ask: Ask, pub turn: Turn }

pub enum Lane    { None, Running, Limited, Asleep, Done, Exited, Gone }
pub enum Host    { Up, Loaded, Draining, Offline }
pub enum Pr      { None, Merged, Closed, Conflicting, Red, Pending, Green, Open }
pub enum Verdict { None, Written, Pass, Fail, Refused }
pub enum Ask     { None, Open, Answered }
pub enum Turn    { None, Working, Done, Blocked, NeedInput }

pub struct Row {
    pub id: RowId,                    // "<flow>/<file>/<id>", stable: events and cards name it
    pub when: When,                   // per fact: a set of values, or any
    pub act: Action,                  // `do`
    pub then: Vec<Action>,            // the retries' actions, at most two; empty: `do` again
    pub owner: Owner,
    pub timeout: Timeout,
    pub since: Since,
    pub escalate: Escalate,
    pub to: Option<StateName>,
    pub say: String,
}

pub enum Owner    { Drive, Lane, Orchestrator, Owner }
pub enum Timeout  { Limit(LimitKey), Subject, OwnerHeld } // a [limits] key · the subject's own (a run card's --timeout) · the owner's stop
pub enum Since    { Entered, Heard, HostHeard, Reset } // row entered · the lane last heard · its host's last frame · shed's limit reset
pub enum Escalate { Ladder, Orchestrator, Owner } // default · skip the retries · an owner topic: straight to the owner

pub enum Action {
    Wait,
    Start, SpawnBuild, SpawnCheck, Respawn, RespawnCheck, Move, Update,
    Nudge, ContinueAtReset, TellCheck, Deliver,
    HandOff, Record, NoChange, CiFail, UnstickCi, Merge, RecordMerge,
    ApplyDecision, RaiseCard, Remind, TakeDefault, ToOwner, Expire, CloseSuperseded,
    RunCard, KillRun, DeliverHandoff, TellOrchestrator, Release,
    EndPass, DropMarker, Clean, Reconcile,
}
```

The host fact's source, in the link's own types (`facts.md`):

```rust
pub struct Sample { pub load_per_cpu: f64, pub pressure: Pressure, pub at: u64 }   // taken on the host itself

pub struct Report   { pub build: String, pub shed: Option<String>, pub lanes: Vec<Seen>,
                      pub sample: Option<Sample> }                                 // + sample
pub struct HostSeen { /* link, since, why, boot, n, heard, build, as today */
                      pub sample: Option<Sample>, pub drain: Option<String> }      // + sample, + the order in force
```

`Owner::Orchestrator` is one more than the issue's three: while a card sits in its thread, the orchestrator
holds the row. `flow::act` matches `Action` with no wildcard arm, so a new action cannot ship without its effect.

Who may act, as a type (`boundaries.md`, "Which process runs what"):

```rust
/// A held pass or kick lease. Its fields are private to `drive`, so only a drive pass or kick has one.
pub struct Pass<'a> { /* ctx, the lease, the fleet, a kick's issue */ }      // crate::drive

pub fn act(pass: &Pass, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, step: &Step) -> Result<Outcome>;
pub(super) fn escalate(pass: &Pass, …);          // flow::escalate: only `act` calls it
pub fn run(pass: &Pass, …);                      // flow::shadow: its events, under the same lease
```

`decide`, the readers and `Episode::of` take no `Pass`: any process may read.

## What an action returns

```rust
pub enum Outcome {
    Done,                             // it happened; the next sample shows another row
    Queued { behind: Vec<IssueRef> }, // behind named issues that have rows of their own: no try, no clock
    Held   { by: Holder },            // a host, a slot or a cap: no try; the holder's own clock runs
    Failed { code: &'static str, why: String },   // one try used
    Final  { code: &'static str, why: String },   // retrying cannot help: escalate now
}
```

`outcomes.toml` classes every refusal code once. Shadow needs it to count tries, so it lands in slice 3. It replaces `pending::SOFT`, `SKIP`, `DISK`, `starts::stops`,
`drive::owner_refusal`, `waiting::HOST_WAIT` and the `waits` list in `pending::stuck`:

```toml
queued = ["SERIAL_BUSY", "IN_FLIGHT_CAP", "CAP", "IN_FLIGHT", "ALREADY_DONE", "BLOCKED_ON", "PAUSED"]
held   = ["HOST_BUSY", "HOST_FULL", "HOST_OFFLINE", "HOST_UPDATING", "MEM_LOW", "DISK_LOW", "DISK_BUDGET",
          "LOGIN_NEEDED", "SPAWN_PENDING", "RUNTIME_DOWN"]
final  = ["ROUND_LIMIT", "UPDATE_LIMIT", "LANE_LIVE", "SERIAL_UNMARKED", "DESIGN_MAIN_ONLY", "MCP_FORBIDDEN",
          "ACP_UNGUARDED", "SECRET_IN_WIP", "BLOCKER_STRANDED", "MERGED"]
# every other code, EXIT and ERROR: failed
```

A code in no class and not known to `roles/usage/30-refusals/` fails the table test.

## Episode, tries and the clock

```rust
pub struct Episode {                  // read from the log, never stored apart
    pub state: StateName,
    pub head: Option<Sha>,
    pub tries: BTreeMap<RowId, u8>,   // `row-tried` with outcome failed, since the episode began
    pub entered: Option<(RowId, u64)>,// the row the issue is in, and since when
    pub level: Level,                 // Auto | Orchestrator { ask } | Owner { card }
    pub answered: BTreeSet<RowId>,    // rows the orchestrator has answered in this episode
    pub spawn: Option<Seq>,           // this round's spawn: the session and host the readers look at
}
```

- An **episode** begins when the PR head changes, when the issue moves to another working state, or when the
  owner decides (an answer, `[retry]`, `[one more round]`, `unpark`). A stop in `needs-human` or `parked` and
  the way back are not a move. Tries are counted per row over the episode, so an issue that flips between two
  rows (a lane that dies, is respawned, dies again) does not start from zero each time.
- **An orchestrator's answer begins no episode.** It gives the row three more tries, once, and the row goes
  into `answered`. When a row in `answered` runs out of tries again, the owner card is raised at once.
- A row gets **three tries**: `do`, then `then[0]`, then `then[1]` (or `do` again). A try is spent by a `Failed`
  outcome or by the timeout running out.
- The **clock** starts at `since`: when the issue entered the row, when shed last heard the lane, when the
  lane's host sent its last frame, or shed's limit reset time. `deadline = since + [limits] <timeout>`.
- **A `heard` clock stops at the host's last frame.** The deadline has passed only when a frame that arrived
  at or after it still shows the lane quiet. With no newer frame nothing expires on a guess, whichever end is
  loaded. The `host` fact bounds that wait: with no frame for `host_offline_minutes` the host is `offline`.
- **A `host-heard` clock** is the `*/host-offline` rows': `host_offline_minutes` from the host's last frame,
  as `hosts::evacuate` counts today (`seen + host_offline_minutes`). A machine with no frame ever counts from
  when the row was entered.
- After the third try, or on `Final`, the ladder runs (`boundaries.md`).

`Step` is what `decide` returns: the row, the try number, the action due now (or none before the deadline), the
deadline, the level and the line to show.

## New events (all passive: the fold ignores them)

| Event | Data | Logged by |
|---|---|---|
| `row-entered` | `row`, `cond`, `deadline` | `act`, when the issue's row changes |
| `row-tried` | `row`, `try`, `action`, `outcome`, `code`, `why` | `act`, once per try |
| `row-escalated` | `row`, `level` (`orchestrator`, `owner`), `ask`, `thread` | `escalate` |
| `row-retry` | `row`, `by` (`owner`, or `orchestrator-<thread>` once per row and episode) | `act` (`apply-decision`), for an answer to the card: the row's tries go back to zero |
| `verdict-seen` | `lane`, `head`, `first` (`PASS`, `FAIL`), `mtime` | intake, once per new `verdict.md` |
| `verdict-refused` | `code`, `head`, `why` | `drover verdict`, when it refuses a file |
| `flow-disagree` | `kind`, `row`, `cond`, and the two sides below | shadow mode only |

`row-*` and `flow-disagree` are written only by a drive pass or kick, under its lease. A tap or `drover answer`
logs the `decision`; the `row-retry` is `apply-decision`'s, in the kick that follows.

In shadow mode `flow::shadow` writes `row-entered`, `row-tried` and `row-escalated` itself, each marked
`"shadow": true`, so the shadow episode has tries, deadlines and a level although nothing acts. A shadow try
is spent when the row's deadline passes, or when today's drive ran the same action for the issue and it failed
(its refusal code, classed by `outcomes.toml`). A shadow escalation raises no ask and no card.

`flow-disagree` has two kinds, each logged once per issue, row and episode:

| `kind` | Logged when | Data |
|---|---|---|
| `action` | in one pass the table's step and today's differ in owner, action or role | `table` (action, try, deadline), `drive` (step, who, what) |
| `escalation` | one side reached a person and the other did not by the episode's end; or both did, more than `act_minutes` apart or at different levels | `table_at`, `table_level`, `drive_at`, `drive_code` (the `needs-human` code, ask or alert) |

`driven`, `promise`, `promise-missed`, `promise-void`, `no-action`, `pending-action`, `turn-prompted`,
`self-heal`, `respawn` and the `stall` key `undriven:<since>` stop being written at slice 6. Readers of old logs
keep reading them.

`needs-human` keeps its name and gains `row`; its `code` stays for old readers. A card's ask is a
`needs-decision` with `row`, `routed` (the thread; absent when the card goes straight to the owner) and the
options `retry`, `park` (plus `one more round` on a round-limit row).

## `[limits]` keys

Every timeout is a `[limits]` key. Existing keys the rows use: `stall_minutes`, `silent_minutes`,
`ci_start_minutes`, `ci_stuck_minutes`, `host_offline_minutes`, `ask_default_minutes`, `ask_waiting_minutes`,
`ask_expire_hours`, `done_close_minutes`, `spawn_pending_max_secs`, `pass_max_secs`, `gate_check_minutes`.

| New key | Default | Replaces |
|---|---|---|
| `act_minutes` | 10 | `promise_passes` × `slow_secs`: drive's own action must have happened |
| `escalate_minutes` | 30 | new: how long a card stays with the orchestrator before the owner gets it |
| `start_minutes` | 10 | the constant `stall::START_SECS` |
| `check_quiet_minutes` | 120 | the constant `stall::STALE_CHECK_SECS` |
| `host_wait_minutes` | 30 | the 900 s and 1800 s in `check.rs` (`spawn-held`, `host-busy`); also how long a loaded host's lane may stay unheard |
| `card_ttl_minutes` | 15 | the constant `card::TTL` |

`promise_passes` goes at slice 6. `[fleet.<n>] flow = "shadow" | "on" | "legacy"` selects the engine per fleet
between slices 3 and 6, then goes too.
