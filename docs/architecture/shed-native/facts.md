# shed-native: every lane fact, its shed field, the code that goes (#1053)

Surfaces: **B** the Lanes rows and lane sheet (old board, cooee's screens), **A** `/api/v1` (`state`, `lanes`,
`herd`, `needs`, `lane/<agent>`, `shed`), **C** `drover check` events, **D** drive's acts. shed fields are
`TerminalInfo`'s (shed `ls` / `get`) unless an event is named. "Goes" is deleted; "moves" keeps its behaviour
behind `core::session`.

## From shed

| # | Fact | Shown | shed field | drover code that goes |
|---|---|---|---|---|
| 1 | The session is there | B A (`gone`), D (`LANE_LIVE`, respawn) | listed by `ls`; `state` = `running` or `exited` | `term::agent_of`, `term::agents`, `list`, `agent_list`, `harness_in` and the `pane get` word; `host::agents`, `host::live_in`, `hosts::remote_agents`, `all_agents` (rows of JSON). Today an exited session is filtered out and looks the same as a removed one |
| 2 | Status: working, blocked, idle, done | B A C D | `reported.state` (table in `shapes.md`) | the `agent_status` string and its 30 uses in 15 files; `snapshot::waits` turning lanes back into `{"name", "agent_status"}`; `lanes::join`'s rank by section name |
| 3 | What it is doing now | B A (`activity`, `now`) | `reported.note` | `lanes::chat`, `activity`, `spin`, `now`, `now_of`, `doing`, `short`, `chrome`; `Provider::status_word`, `now_line`, `tool_line`; `Lane::session` and `lanes::said`, `read_tail`, `Tail` (the harness's own JSONL); the per-row screen read in `lane_row` and `snapshot::take` |
| 4 | Why it is blocked | B A (`needs` card) | `reported.state` = `blocked` or `waiting-approval`, `reported.kind`, `reported.note` | `lanes::pending`, `prompt`; `Provider::pending_menu`, `answer_keys`; serve's `pick` (`POST /answer` pressing a menu digit) |
| 5 | Usage limit | C (`usage-limit`), D (host hold) | `reported.state` = `rate-limited`, note = the reset time | `Provider::usage_limited`, `cap_banner` and its pattern; the 500-character reply tail in `shed-out.json`; check's `term::recent` read |
| 6 | Signed out | C (`signed-out`), D, the thread's sign-in note | `reported.state` = `error`, note `auth: …` | `Provider::signed_out` and its pattern; `orchestrator::signed_out`'s screen read |
| 7 | Never started | D (`START_FAILED`) | no `reported` yet, `START_SECS` after the spawn | `stall::stalled`'s empty-screen test |
| 8 | Stalled | C (`stall`), D (`LANE_STALLED`) | `reported.state` quiet (`shapes.md`) since the stamp, no turn-end | check's inline rule on the issue's last event time; `stall::stalled` matching `waiting`'s words (`idle without pushing`); the `last screen:` tail in the reason (shed's note instead) |
| 9 | Silent mid-turn, failed start, stale check lane, hung conductor (#792, #785, #819, #946) | C D | events `turn`, `text`, `tool_call`, `reply`, `resources` | `stall::changed` and `pane-seen.json` (a hash of the screen). The rest already reads shed's events: it moves, typed, as `Pulse` |
| 10 | Dead, and why | B A (`dead`), C (`lane-dead` + `cause`), D (resume or respawn) | `state` = `exited`, `exit`, the last `reported`; events `exited {code}`, `killed` | `shed::log`'s guesses (a limit banner in the reply tail, "a turn was open"); `shed::exit_cause` (moves, typed `Cause`). A poll now sees an exit even when its event was missed |
| 11 | Orphan | C (`orphan-lane`), D (adopt or close), A (`shed` rows) | a session whose `name` no `spawn` or `orchestrator-start` names; whose it is: `labels.fleet`, `issue`, `role`; where: `cwd` | `spawn::shaped` (guessing fleet, issue and role from the name or the cwd); the unnamed-pane branches (`pane_id`, `foreground_cwd`) in `spawn::orphan`, `spawn::orphans`, check and `lanes::join`; `lanes::join`'s cwd-inside-a-fleet guess |
| 12 | Transcript | B A (`lane/<agent>`), D (handover to the next lane) | `read {events}`: the session's own log (ACP); `read {history}`: its text (PTY) | `lanes::body`, `roll`, `log`, `GAP`, `strip`, `session`, `session_file`; `Provider::transcript`; check's roll into `transcripts/<agent>.log`; `term::recent`; `spawn::handover`'s JSONL and glyph parsing |
| 13 | ACP or PTY | B A (`runtime`) | `acp` | the constant `"runtime": "shed"` in `agent_of` |
| 14 | Process and CPU | D (idle trim, hung) | `pid`, `resources.cpu_pct` | the `pid` and `cpu` JSON keys |
| 15 | Asleep (hibernated) | D (`HOST_BUSY` before a wake, #908) | `reported.state` = `hibernated` | `term::awake`'s string test |

## drover's own, on top (unchanged)

| Fact | Source |
|---|---|
| Turn-end: `done`, `blocked`, `need-input`, `working` | the lane's `drover turn` (`turn-end` event), read by `waiting::turn` |
| Role, harness, model, effort, port, worktree, root, branch, spawn time | the `spawn` event |
| Host and place | fleet.toml `[hosts]` and which host's shed listed the session |
| Since when shed has said it | drover's stamp of shed's change (`lane-state.since`, `shapes.md`); shed has no such field |
| Parked, held by #N | git (worktree clean and pushed) and the log, as today, typed as `LaneState` |
| Closed by drover | `lane-closed`, `lane-killed` |

## What still looks at a screen, and why it is not a lane fact

- `drover setup`'s sign-in pane (`setup::screen`): a person's login terminal, shown as it is.
- A PTY lane's transcript and a blocked card's `tail`: shed's `read` text, masked and shown, never parsed.
- The kick of the two TUI harnesses in `crates/core/src/providers/` (`read_argv` in their kick commands):
  start-up plumbing that waits for the prompt; it feeds nothing drover shows.

## Files the migration touches

`crates/core/src/`: `term.rs`, `shed.rs`, `lanes.rs`, `stall.rs`, `check.rs`, `drive.rs`, `waiting.rs`,
`spawn.rs`, `gate.rs`, `design.rs`, `orchestrator.rs`, `hosts.rs`, `host.rs`, `slo.rs`, `snapshot.rs`, `roots.rs`,
`rename.rs`, `cloud.rs`, `lib.rs`, `cli.rs`, `providers/mod.rs` and each provider's screen readers;
`crates/service/src/lanes.rs`; `crates/server/src/serve.rs`, `serve/v1.rs`, `serve/chat.rs`, `screens.rs`.
Docs: `docs/runtimes.md` ("State is reported, not scraped" becomes true), `docs/lanes.md`, `docs/serve.md`
(the rolling transcript section goes).
