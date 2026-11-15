# ADR: shed-native — every lane fact read from shed (#1053)

## Summary

Decision: read every lane fact from shed's own session record, through one new typed module. drover adds
only a time stamp, the lane's turn-end and its log. Screen parsing and the herdr-style status row go.
Slices: typed sessions everywhere; one rule set for stall, dead and orphan; transcript from shed's log;
delete the scrape, with checks that keep it out; the running, idle, exited test.
Open questions: the old board's file is frozen: add a board-fix line to the issue? Terminal-UI (non-ACP)
lanes lose phone menu answers and limit and sign-out alerts: accept?

## Status

Proposed (architect lane, 2026-11-15, staging `f146620b0`). Read `usage.md` first, then this, `facts.md`,
`shapes.md`, `boundaries.md`.

## Question

Lanes run on shed and #721 deleted herdr, but drover still learns what a lane is doing the herdr way. How does
drover read every lane fact (status, stall, dead, orphan, transcript) from shed alone, with the lane's turn-end
on top, so that a shed state can never again be shown as something else (#1033)?

## Where it stands

- `term::agent_of` turns shed's `TerminalInfo` into herdr's `agent list` row (`pane_id`, `agent_status`,
  `terminal_title`, `foreground_cwd` readers), an untyped JSON value. 15 files compare `agent_status` as a string.
  #1033 was one missing word in that mapping (`running`); `waiting-approval`, `rate-limited`, `error` and
  `hibernated` are still unmapped and fall to "idle" by accident.
- Exited sessions are filtered out of the list, so "exited" and "removed" look the same, and the exit cause is
  rebuilt from a 500-character tail of the last reply (a limit banner) kept in `shed-out.json`.
- Facts shed already reports are read off the screen instead: the usage limit and sign-out (banner patterns),
  what the lane is doing (spinner word, last tool line), never-started (an empty screen), a stale check lane (a
  hash of the screen), the blocked card (a TUI menu), the transcript (screen reads rolled into a file, or the
  harness's own JSONL).
- An orphan's fleet, issue and role are guessed from its name or its cwd, though drover labels every session.
- check and drive each have their own stall rule; drive's matches the wording of `waiting`'s lines.

## Options

1. **Keep the row, fix the mapping** (what #1033 did). Small, and leaves the cause: an untyped row, strings
   compared in 15 files, screens parsed for facts.
2. **One typed module over shed's `ls` and events; rules in one place (chosen).** `core::session` is the only
   reader of shed's types. Everyone else takes `&Sessions` and asks typed questions.
3. **shed's event stream as the only source** (fold events into state, no `ls`). Rejected: a remote host has no
   stream (one ssh call per request), shed keeps a bounded number of events, and drive may be off for hours.
   `ls` is the truth about now; the stream only makes drover learn of a change sooner.

Three smaller choices:

- **Since when.** shed has no time on a reported state. Chosen: drover stamps shed's change into the log
  (`lane-state.since`), from the event stream where there is one and from the poll everywhere. Not chosen: wait
  for a shed field (it can replace the stamp later, in `stamp.rs` alone); time the stall from the issue's last
  drover event (today: not what the issue asks).
- **A removed session's transcript.** shed drops the log at `rm`. Chosen: copy shed's log to the state dir at
  that moment, same shape, one reader for live and archived. Not chosen: keep exited sessions in shed until
  `clean` (the lane's name must be free for its respawn); no transcript after close (loses today's sheet and
  spawn's handover).
- **A PTY lane** (`kind = "shed"`, a terminal UI). Chosen: it gets exactly what shed reports for it (the
  hook's `working`, `blocked`, `done`; its text shown as it is). Not chosen: keep the parsers for it (two ways
  to read one fact, the #1033 cause); retire the kind here (a scope change: open question 2).

## Decision

**Principle: drover may show shed's text; it never parses it to learn a fact.**

1. **`core::session`** holds `Session`, `Life`, `Status`, `Exit`, `Cause`, `Pulse`, `Transcript` and the single
   mapping from shed's wire types (`shapes.md`). `session::all` reads every enabled host's `ls`, exited sessions
   included; a host that fails is `Unreachable`, never "no sessions".
2. **Status** is shed's `reported.state`, every word mapped once (`shapes.md`). The shown words stay today's.
   `running` is working (#1033, absorbed); `waiting-approval` is blocked; the usage limit is shed's
   `rate-limited`; a sign-out is shed's `error` with an `auth:` note. No banner pattern remains in drover.
3. **Stamp.** `session::observe` (drive's pass, `drover check`) and `on_event` (the stream) write `lane-state`
   with `since`, once per change. Reads never write.
4. **Rules** live in `lanes::health` and `lanes::orphans`, used by check (the alert) and drive (the act):
   - Stall: shed says quiet for `[limits] stall_minutes` since the stamp, and the lane logged no turn-end.
   - Never started: no report from the session `START_SECS` after its spawn.
   - Dead: shed says exited, or lists it no more on a host that answered, and drover did not close it. The cause
     comes from shed's exit code and last reported status.
   - Orphan: a session no `spawn` or `orchestrator-start` names. With drover's labels it is an `orphan-lane`
     (adopted or closed by those labels); without, a row of the `shed` list.
   What drover then does (respawn, resume, escalate, hold the host) does not change.
5. **Transcript** is `session::transcript`: shed's events for an ACP session, shed's text for a PTY one,
   archived when drover removes the session. The rolled pane log, the harness JSONL readers and the sheet's
   scraped status line go. Spawn's handover reads the archive.
6. **Delete**: everything in `facts.md`'s last column. `lanes.rs` keeps `Lane`, `join`, `group`, masking and
   gains the rules; nine screen readers leave the `Provider` trait.
7. **API v1 keeps its shapes** (`shapes.md`); cooee and the old board need no change.

## Consequences

- A shed state drover does not know shows as idle with shed's own note, in one place to fix.
- Behaviour that changes: a blocked lane no longer stalls; the stall clock starts when shed went quiet;
  `waiting-approval` raises the blocked card; `error`, `rate-limited`, `hibernated` and `stuck` no longer appear
  as raw API states; a lane row's subtitle is shed's note (no spinner word or elapsed time); the blocked card has
  no menu buttons (the Say box still sends).
- PTY lanes lose what only a parser gave them: the phone's menu answers, the usage-limit and sign-out alerts
  (shed reports those for ACP sessions only), the `now:` tool line. The same holds for a PTY conductor's
  sign-in note in its thread.
- `src/` shrinks: roughly −750 lines of readers and guesses against +450 in `session/` and the rules (about 150
  of those moved from `shed.rs`). State `src/ net` in the PR.
- State dir: `pane-seen.json` and `transcripts/<agent>.log` are no longer written (old files are left for
  `clean`); `shed-out.json` keeps its name and loses its `text` key; `transcripts/<agent>.jsonl` is new.
- The PR edits the hottest files (`check.rs`, `drive.rs`, `spawn.rs`, `serve.rs`). Slice 1 is mechanical and
  conflicts easily: land it while few PRs are open. New lane-fact work then lands in `session/`, one file per
  fact family, instead of in those files.
- Follow-ups, not in this issue: a `since` on shed's reported state would retire the stamp; answering an ACP
  approval from the phone (shed has the options in its events); `term.rs` still speaks herdr's pane words for
  writes (`pane run`, `agent prompt`): a rename, no fact depends on it.

## Slices (commits of one PR, each green)

1. **Typed sessions.** `core::session` types, mapping and `all`; every reader of the JSON rows takes
   `&Sessions`; `agent_of`, `agents`, `list`, `live_in`, `remote_agents` go. No behaviour change but the mapped
   words. #1033's test passes unchanged.
2. **Stamp and rules.** `lane-state.since`, `observe`, `Pulse` typed; `lanes::health` and `orphans`; check and
   drive use them; usage limit and sign-out from status; `shaped`, `pane-seen.json` and the banner guess go.
3. **Transcript.** `session::transcript` with the archive at close; v1, the screens, the old board's sheet and
   spawn's handover read it.
4. **Delete the scrape.** The readers in `lanes.rs`, the `Provider` screen methods, serve's `pick`, the per-row
   screen reads; the `[[ban]]` lines below; `docs/runtimes.md`, `docs/lanes.md`, `docs/serve.md`.
5. **The test.** One fake shed session through running, idle, exited (`usage.md`), asserting the Lanes rows,
   `/api/v1` and `drover check`'s events at each step, plus an orphan and an unreachable host.

## Open questions for the owner

1. **`board-fix`.** `crates/server/src/serve.rs` is frozen (#552) and holds the lane data that `/api/v1` and
   cooee's screens share with the old board. The issue has no `board-fix` line, so CI's board-freeze fails
   any PR that does this. It fixes broken behaviour (the #1033 class). Recommend: amend #1053 with a
   `board-fix` line. Without it the scrape cannot leave and acceptance 2 cannot be met.
2. **PTY lanes.** After this a `kind = "shed"` lane has no phone menu answers and no usage-limit or sign-out
   alert. Every fleet and host in the live config is `shed-acp`. Recommend: accept. The alternative is a
   follow-up that retires `kind = "shed"` for lanes, which would also delete the TUI kicks and the hook's state
   reports.

## `.drover/arch.toml` lines

No central list or generated file is converted, so there is no `frozen`, `generated` or `[[collect]]` line.
The impl lane adds these bans in slice 4, in the same PR:

```toml
[[ban]]
pattern = "agent_status"
paths = ["crates/*/src/**"]
reason = "#1053: a lane's status is session::Status, mapped from shed once in crates/core/src/session/"

[[ban]]
pattern = "pane_id"
paths = ["crates/*/src/**"]
reason = "#1053: a session is addressed by its shed name; no herdr-shaped row"

[[ban]]
pattern = ".reported"
paths = ["crates/*/src/**"]
except = ["crates/core/src/session/**"]
reason = "#1053: shed's reported state is read in crates/core/src/session/ only (Status::of)"

[[ban]]
pattern = "read_argv"
paths = ["crates/*/src/**"]
except = ["crates/core/src/term.rs", "crates/core/src/setup.rs", "crates/core/src/providers/**"]
reason = "#1053: no lane fact is read off a screen; a transcript is session::transcript"
```
