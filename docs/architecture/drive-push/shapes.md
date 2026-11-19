# drive-push: data shapes (#1309)

## A frame: one JSON line from a host

```rust
/// One line of the watcher's stdout. `report` prints exactly one, a `Report`.
pub struct Frame {
    pub v: u32,          // 1. A link that reads another version stops and shows `updating`.
    pub boot: String,    // this watcher run: 16 random hex chars
    pub n: u64,          // 1, 2, 3 … within the boot. Frame 1 is always a Report.
    pub at: u64,         // the host's clock in ms: shown and measured, never ordered by
    pub body: Body,
}

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    Report(Report),      // the whole truth: replaces everything the Mini knew of this host
    Lane(Seen),          // one session or one worktree changed
    Line(Line),          // one new outbox line
    Host(Load),          // load, memory or disk moved
    Beat,                // nothing changed for link_beat_secs
}

pub struct Report {
    pub build: String,              // the host's drover sha
    pub shed: Result<u64, String>,  // shed's last event seq there, or why it did not answer
    pub load: Load,
    pub lanes: Vec<Seen>,           // every labelled shed session, exited ones too, and every worktree under the roots
    pub lines: Vec<Line>,           // every outbox line not yet acked, oldest first per worktree
}

pub struct Seen {
    pub session: Option<Session>,   // core::session::Session as shed lists it; None: a worktree with no session
    pub pulse: Option<Pulse>,       // core::session::Pulse: turn open, last heard (ACP)
    pub worktree: Option<Worktree>, // None: a session whose folder is no worktree under the roots
}

pub struct Worktree {
    pub path: String,
    pub head: Option<(String, u64)>,   // sha and commit time
    pub upstream: Option<String>,      // the branch's sha on origin as this clone last saw it
    pub dirty: bool,
    pub unpushed: u32,
    pub verdict: Option<Verdict>,      // verdict.md, when there is one
}

pub struct Verdict { pub mtime: u64, pub text: String }                 // text capped at 64 KiB

/// Today's outbox line plus the file it came from.
pub struct Line { pub worktree: String, pub sent: u64, pub lane: String, pub args: Vec<String> }

pub struct Load { pub load_cpu: f64, pub mem_free_gb: f64, pub swap_out: f64, pub disk_free_gb: BTreeMap<String, f64> }

/// The Mini's side of the call, as its stdin: the host has no fleet.toml.
pub struct Watch { pub roots: Vec<String>, pub beat_secs: u64, pub poll_millis: u64, pub sample_secs: u64, pub report_secs: u64 }
```

`Session` and `Pulse` are shed-native's types (#1053); `Session` gains `Serialize` and `Deserialize`, nothing else. A frame
over 1 MiB is dropped and logged; the reader takes unknown fields without failing.

## Ids, order, dedup

| Thing | Its id | Order | Applied once because |
|---|---|---|---|
| a frame | `<host>:<boot>:<n>` | `n` within a boot; a new boot starts with a `Report` | the view keeps `(boot, n)`; `n` at or under it is dropped, a gap redials |
| an outbox line | `(lane, sent)` | file order per worktree; `sent` is ms and strictly rising in a file | the `outbox` event with that `(lane, sent)` is looked up before it runs |
| a wake | the frame's id, or `outbox:<lane>:<sent>` | none needed | `kick --id`: the `processed` table drives one id once (#509) |
| a job's step | `<fleet>#<issue>:<step>@<key>` | per issue, under its lock | `intent`, `done`, `failed` (#509), unchanged |

- The writer makes `sent` rise: `max(now_ms, last line's sent + 1)` under a lock on the file. Today two lines in one
  millisecond would share an id.
- Delivery is at least once: a line stays in every `Report` until its ack reaches the host. A lost ack sends it again
  and the Mini drops it. A crash between running a line and logging its `outbox` event runs it once more, as today.
- Clocks never order anything. `at` gives the lag shown in the `outbox` event; the SLO is measured on the Mini's clock.
- A line runs as the lane its worktree's latest spawn names, never as the line says (today's rule, kept). A line for
  another issue or role, or a verb a lane does not run itself, is logged `outbox` with `refused`.

## The view: what the Mini knows of each host (`state.db`)

```sql
CREATE TABLE host_view (
  host TEXT PRIMARY KEY,
  link TEXT NOT NULL,       -- off | dialling | live | down | updating
  since INTEGER NOT NULL,   -- when it took that state, the Mini's clock
  why TEXT,                 -- down: ssh's first error line; updating: both builds
  boot TEXT, n INTEGER,     -- the cursor: the last frame applied
  heard INTEGER,            -- when that frame arrived, the Mini's clock
  build TEXT, load TEXT     -- the host's drover sha; its Load as JSON
);
CREATE TABLE lane_view (
  host TEXT NOT NULL,
  key TEXT NOT NULL,        -- the session's name, else the worktree's path
  heard INTEGER NOT NULL,
  json TEXT NOT NULL,       -- a Seen
  PRIMARY KEY (host, key)
);
```

```rust
pub struct View { /* both tables, read once */ }
impl View {
    pub fn load(ctx: &Ctx, cfg: &Config) -> Result<View>;
    pub fn sessions(&self) -> Sessions;                      // per host: Ok(its sessions) or Err("no word for 3m: …")
    pub fn worktree(&self, agent: &str) -> Option<&Worktree>;
    pub fn host(&self, name: &str) -> Option<&HostSeen>;     // link, since, why, load, build, heard
    pub fn stale(&self) -> Vec<&str>;                        // hosts to poll now: every enabled one whose link is not live
}

pub enum Link { Off, Dialling, Live, Down { why: String }, Updating { theirs: String } }

pub enum Wake { Issue { fleet: String, issue: u64, id: String }, Fleet(String) }
```

A `Report` replaces the host's rows in one transaction: a lane it does not list is gone from the view. A `Lane`
frame replaces one row. A host's sessions read `Err` when its link is not live and its last word is older than
2 × `fast_secs`; every caller already treats that as an unreachable host (never "no sessions", #1053).

## Events

| Event | When | Data |
|---|---|---|
| `outbox` (kept) | a line ran or was refused | as today (`lane`, `sent`, `args`, `code` or `refused`), plus `host` and `lag_ms` |
| `lane-state` (kept, passive) | a frame shows a lane's status or exit changed | as #1053; a boat's lanes now get it within a second, from the frame |
| `link` (new, passive) | a host's link changes state | `host`, `state`, `why`, `boot` |
| `link-down`, `link-up` (new, pushed) | `check`: an enabled host with open lanes not live for `link_down_minutes`; live again | `host`, `reason`, `since` |
| `slo-act`, `slo-ok` (new kind, pushed once) | event-to-action p95 over `slo_act_secs` for `slo_act_minutes` | `p95_ms`, the slowest fleet and step |
| `kick` (kept) | a job ran | plus `lag_ms`: the wake's arrival to the job's start, on the Mini's clock |
| `drive-pass` (kept) | a fleet's pass ended | `secs`, `starts`, `kicked`; `by: fast` and the `pending` rewrite go |
| `pass-slow`, `pass-stuck` (kept) | a pass ended itself at `pass_max_secs`; a dead pass's lease was taken | plus `fleet` |

New alert kinds are emitted with `.push()`; the two frozen event lists get no new line.

## Leases

| Lease | Holder | Lives |
|---|---|---|
| `link-<host>.lock` (new) | the service's link, or one catch-up poll | renewed on each frame; `link_stale_secs` |
| `drive-<fleet>.lock` (kept) | a fleet's pass | 2 × `pass_max_secs` |
| `drive-<fleet>-<n>.lock`, `kick-<fleet>-<n>.lock` and `.again` (kept) | an issue's job | the step's limit |
| `merge-<fleet>.lock` (new) | the one merge of a fleet in flight (premerge worktree, integration's tip) | `gate_check_minutes` |
| `spawn-<fleet>-<n>.pending` (kept) | a start slot | `spawn_pending_max_secs`, or until `started` |
| `admit.lock` (kept) | one admission, all fleets | local reads only now: milliseconds |
| `chores-<fleet>.lock` (new) | a fleet's slow chores: clean, backlog, hotspots sync | `slow_secs` |
| `drive.lock`, `drive-fast.lock`, `drive-fast-<fleet>.lock` | — | deleted in step 3 |

## `[limits]` keys

| Key | Default | What |
|---|---|---|
| `link_beat_secs` (new) | 5 | a watcher with nothing to say writes a beat this often |
| `link_stale_secs` (new) | 20 | no frame for this long: the link is cut and dialled again |
| `link_retry_max_secs` (new) | 30 | the longest wait between dials (1 s, doubling) |
| `link_poll_millis` (new) | 500 | how often a watcher looks at each outbox, verdict.md and worktree head for a change |
| `link_sample_secs` (new) | 30 | a watcher reads every worktree's facts again this often, changed or not |
| `link_report_minutes` (new) | 10 | a live watcher sends a whole `Report` this often: the catch-up, as `webhook_poll_minutes` is for GitHub |
| `link_down_minutes` (new) | 5 | a host not live this long alerts `link-down` |
| `slo_act_secs`, `slo_act_minutes` (new) | 5, 10 | the event-to-action target and how long its p95 may miss before `slo-act` |
| `job_max` (new) | 4 | issue jobs of one fleet running at once |
| `pass_max_secs` (changed) | 120 → 60 | a pass ends itself here |
| `slo_pass_minutes` (changed) | 30 → 5 | no finished pass of a fleet for this long: the `Drive is stuck` card |
| `slo_tick_minutes` (kept) | 10 | the tick is check and chores only; the owner's stopgap 30 goes back |
| `outbox_stale_minutes` (kept) | 5 | a line reported but not applied this long alerts `outbox-stale`, read from the view |
