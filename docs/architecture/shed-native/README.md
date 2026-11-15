# shed-native: read every lane fact from shed (#1053)

## Summary

Decision: read every lane fact from shed's own session record, through one new typed module. drover adds
only a time stamp, the lane's turn-end and its log. Screen parsing and the herdr-style status row go.
Slices: typed sessions everywhere; one rule set for stall, dead and orphan; transcript from shed's log;
delete the scrape, with checks that keep it out; the running, idle, exited test.
Open questions: the old board's file is frozen: add a board-fix line to the issue? Terminal-UI (non-ACP)
lanes lose phone menu answers and limit and sign-out alerts: accept?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how the callers read after the change, and the acceptance test |
| `adr.md` | the question, options, decision, consequences, slices, open questions, the `.drover/arch.toml` lines |
| `facts.md` | every lane fact: where it shows, its shed field, the drover code that goes |
| `shapes.md` | the types, the status table, the `lane-state` stamp, the transcript archive |
| `boundaries.md` | which module may call which, and the two statecharts |

## The design on one screen

| Fact | shed says | drover adds |
|---|---|---|
| There, or exited | `ls`: `state`, `exit` | nothing |
| Working, blocked, idle, done | `reported.state` | one typed mapping, today's words |
| Doing now | `reported.note` | masking |
| Usage limit, signed out | `rate-limited`; `error` with an `auth:` note | the alert and the host hold, as today |
| Stalled | quiet status | since when (a stamp), "no turn-end", the threshold |
| Never started | no report yet | the time since the spawn |
| Dead, and why | `exited`, exit code, last status | "drover did not close it" |
| Orphan | a session and its labels | "no spawn names it" |
| Transcript | `read`: its events (ACP) or text (PTY) | a copy when drover removes the session |

One module, `core::session`, reads shed. `lanes` holds the rules. check, drive and the server ask typed
questions and never compare a status string, name a shed wire type or read a screen for a fact; four
`[[ban]]` lines in `.drover/arch.toml` keep it so.

## Open questions

1. Add a `board-fix` line to #1053? `crates/server/src/serve.rs` is frozen and holds the lane data all three
   surfaces share. Recommended: yes; the work cannot pass CI without it.
2. Accept that PTY lanes (`kind = "shed"`) lose the phone's menu answers and the usage-limit and sign-out
   alerts? Recommended: yes; the live config runs only `shed-acp`.
