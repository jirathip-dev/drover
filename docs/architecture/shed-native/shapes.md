# shed-native: data shapes (#1053)

All in one new module, `core::session`, the only place that reads shed's wire types for a lane fact.

```
crates/core/src/session/
  mod.rs         Session, Life, Status, Exit, Cause, Sessions; From<TerminalInfo>: the one mapping
  list.rs        all(cfg, r), get(r, name): shed `ls` / `get` on each enabled host
  stamp.rs       observe(), on_event(): shed's changes into the log as `lane-state`; since(), exit()
  pulse.rs       Pulse: an ACP session's turn, last event, open tools (was shed-out.json's untyped rows)
  transcript.rs  page(), archive(), archived(), last_line(): shed `read`
```

## What shed says

```rust
/// One shed session (shed's `TerminalInfo`), typed. Nothing here is derived by drover.
pub struct Session {
    pub name: String,                       // TerminalInfo.name: the lane's agent name
    pub labels: BTreeMap<String, String>,   // TerminalInfo.labels: fleet, issue, role, harness, round, …
    pub cwd: PathBuf,                       // TerminalInfo.cwd
    pub acp: bool,                          // TerminalInfo.acp.is_some(): an ACP session, else a PTY one
    pub pid: Option<i32>,                   // TerminalInfo.pid
    pub cpu_pct: Option<u64>,               // TerminalInfo.resources.cpu_pct
    pub life: Life,
    pub note: Option<String>,               // TerminalInfo.reported.note (kept after exit)
    pub host: Option<String>,               // the [hosts] entry whose shed listed it (drover's config, not shed's)
    pub machine: Option<String>,
}

pub enum Life {
    Running(Status),                        // TerminalInfo.state == running
    Exited { code: Option<i32>, last: Status },   // state == exited; TerminalInfo.exit; the last reported status
}

/// shed's reported state, every word it has. Built only by `Status::of(Option<&Reported>)`.
pub enum Status {
    Unreported,   // no `reported` yet
    Working,      // working | running
    Blocked,      // blocked | waiting-approval
    Limited,      // rate-limited            (note: the reset time)
    Done,         // done
    Idle,         // idle
    Asleep,       // hibernated
    Failed,       // error                   (note: why; `auth: …` is a sign-out)
    Other,        // stuck, or a word this build does not know
}

impl Status {
    pub fn busy(self) -> bool;    // Working
    pub fn quiet(self) -> bool;   // Done | Idle | Asleep | Failed | Limited | Other: the stall clock runs
    pub fn word(self) -> &'static str;
}

impl Session {
    pub fn status(&self) -> Option<Status>;   // Some while Running
    pub fn signed_out(&self) -> bool;         // Failed, and shed's note starts `auth:`
}
```

| shed `reported.state` | `Status` | word every surface shows |
|---|---|---|
| (none yet) | `Unreported` | `unknown` |
| `working`, `running` | `Working` | `working` |
| `blocked`, `waiting-approval` | `Blocked` | `blocked` |
| `done` | `Done` | `done` |
| `idle`, `hibernated`, `error`, `rate-limited`, `stuck`, any other | `Idle`, `Asleep`, `Failed`, `Limited`, `Other` | `idle` |

The words are today's (`lanes::SECTIONS`), so no API value is new. What changes: `waiting-approval` is
`blocked` (today it falls to idle, the #1033 class of bug), and the raw shed words `error`, `rate-limited`,
`hibernated` and `stuck` no longer leak into the API's `state`; they show as `idle` with shed's note beside.

```rust
/// Every host's sessions. A host whose shed did not answer is not "no sessions".
pub struct Sessions { /* per host: Ok(Vec<Session>) | Err(why) */ }

pub enum Seen<'a> { Here(&'a Session), Absent, Unreachable }

impl Sessions {
    pub fn seen(&self, name: &str, spawn: Option<&Event>) -> Seen<'_>;   // on the spawn's host
    pub fn busy(&self, name: &str) -> bool;                              // Here, Running(Working)
    pub fn in_dir(&self, name: &str, dir: &Path) -> impl Iterator<Item = &Session>;
    pub fn lanes(&self) -> impl Iterator<Item = &Session>;               // carry a `fleet` label
    pub fn failed(&self) -> impl Iterator<Item = (&str, &str)>;          // host, why
}
```

## What drover adds: a time and a cause

shed puts no time on a reported state (`Reported` and `TerminalInfo` have none, checked at the pinned rev
`9b88eb5`). drover stamps it when it learns of the change. The `lane-state` event keeps its shape and gains
`since`:

```json
{"agent": "myapp-impl-12", "state": "done", "note": "end_turn", "since": 1794700000, "shed_seq": 412}
{"agent": "myapp-impl-12", "state": "gone", "why": "exited", "code": 1, "cause": "adapter crash", "since": 1794700900}
```

- `state` is shed's own word, unmapped (`gone` for an exit or kill, as today).
- `since` is this event's time when the `Status` differs from the agent's previous `lane-state`, else the
  previous one's `since`. So a note that changes every minute (`running: cargo test (9m)`) does not reset it.
- A stamp is appended only when it differs from the agent's latest one: the event stream and the poll may both
  report a change, the log gets it once.

```rust
pub struct Exit { pub code: Option<i32>, pub cause: Cause, pub note: Option<String> }

pub enum Cause {
    Limit,    // last status Limited                                   "usage limit"     resumable
    Crash,    // code != 0, lost (code None), or last status Working/Failed   "adapter crash"   resumable
    Killed,   // shed's `killed` event, and no drover close            "killed"
    Ended,    // code 0 with the turn closed                           "ended normally"
}
```

The words in the log (`cause`) are today's, so `drive`'s and `waiting`'s readers of old rows keep working.

```rust
/// An ACP session's pulse, from shed's events only (#785, #792, #819, #946). Was untyped JSON.
pub struct Pulse {
    pub turn_open: bool,          // a `turn` with no `reply` yet
    pub first_turn: Option<u64>,  // when the first prompt went in
    pub heard: u64,               // the last ACP event
    pub said: Option<u64>,        // the last text, tool call or reply
    pub open_tools: BTreeSet<String>,
}
```

Its 500-character reply tail (kept only to grep for a limit banner) is dropped.

## What a lane is

```rust
// lanes.rs
pub struct Lane {
    // from the spawn event, unchanged: fleet, name, issue, role, harness, model, effort, port, since,
    // worktree, root, thread, runtime, host, machine, place, kept
    pub state: LaneState,            // was String
    pub session: Option<Session>,    // was pane: Option<String> and note: Option<String>
}

pub enum LaneState {
    Live(Status),          // shed lists it running
    Dead(Cause),           // shed says exited, or no longer lists it, and drover did not close it
    Parked(String),        // gone, worktree clean and pushed: its short head (#83, from git, as today)
    Held(Vec<u64>),        // gone, its issue held by these (from the log, as today)
    Asleep,                // an orchestrator thread with no session (#422)
}

/// A lane's health for check and drive. Built only by `lanes::health`.
pub enum Health {
    Fine,                          // busy, blocked, or quiet with a turn-end or under the threshold
    NeverStarted { since: u64 },   // Unreported `stall::START_SECS` after its spawn
    Stalled { since: u64 },        // quiet since `since`, no turn-end, `[limits] stall_minutes` passed
    Dead(Exit),
    Closed,                        // drover closed it (`lane-closed`, a kill)
    Unknown,                       // its host is Unreachable: no judgement
}
```

## The transcript

```rust
pub enum Transcript {
    Events(Vec<Logged>),   // ACP: shed `read {events: true, before, n}`; Logged is shed's (seq, kind)
    Screen(String),        // PTY: shed `read {history: true, lines}`, plain text
}
```

The archive is the same data in a file: `<state dir>/transcripts/<agent>.jsonl` (one `Logged` per line) or
`<agent>.txt`, mode 0600, written once when drover removes the session, deleted with the lane's worktree by
the same code that deletes `transcripts/<agent>.log` today.

## API v1: no shape changes

`assets/types/api.d.ts` is not regenerated. Fields that only a screen scrape could fill are sent empty and go
with the old board (#552): `Chat.said`, `older`, `status`, `live`; `LaneNow.w`, `t`; `Blocked.menu`.
`LaneRow.activity`, `LaneNow.n` and `Blocked.activity` carry shed's note; `Blocked.tail` is the last lines of
shed's `read` text, masked and unparsed.
