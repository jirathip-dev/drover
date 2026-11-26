# ADR: any shed agent as a role's lane, guarded by shed's judge (#1694)

Status: proposed (architect lane, 2026-11-27). Read `usage.md` first, then this, `shapes.md`, `boundaries.md`.

## Summary

Decision: a role names any agent in shed's list. shed asks drover's hook before each tool call and drover answers allow or deny from the rules it has today. Agent facts come from shed's list, not from code. A test run and a live check stop an agent that acts without asking.
Slices: read shed's agent list; the hook's new mode; start lanes under the judge; the test run and the live check; delete the Codex and Gemini code; later, after 10 clean Claude rounds, delete Claude's hook format.
Open questions: (1) the live Antigravity round needs the merged build, so record it on the rollout, not in this PR? (2) accept that an agent's own "safe" calls skip the judge and are checked after they ran? (3) accept two short paid turns per agent version per host for the test run?

## Question

How does a role run any agent shed can start, with drover's guard on every tool call, no agent named in
drover's code, and a safe way off Claude's own hook format (q3480: no built-in default, every lane judged)?

## Where it stands (staging a572bc4ba)

- `registry! { claude, codex, gemini }` in `providers/mod.rs`. `Config::parse` refuses an `agent` no module
  claims; spawn refuses `ACP_UNGUARDED` unless that module's hook is in the profile (`spawn::unguarded`).
- `gemini.rs` names `gemini-acp`; shed's id is `gemini`. `codex.rs` starts its agent in a full-access mode.
- `hook::decide` reads a hook-format JSON through `Dialect` tables; the rules are the 1,100 lines under it.
- Lanes start with `shed run --acp <id> --policy allow`; the Claude profile sets `bypassPermissions`.
- shed#359 is merged on shed's staging: `--policy judge --judge "<cmd>"`. It runs the command per
  permission request (`name`, `labels`, `cwd`, `tool_call`, `options` on stdin), takes
  `{"decision", "reason"}`, picks a once-only option, and denies on any failure. The hosts' shed predates it.
- shed's list already holds, per agent: install and login lines, a status command, the model variable,
  efforts, the folder variable, limit and sign-out texts. shed applies model and effort itself
  (`shed run --model --effort`) and reports `rate-limited` and `error auth:` itself.
- `core::session` already turns shed's events into lane state, stall, usage limit and sign-out.

## Options

1. **One guard, one rule set, two parsers until the cut-over (chosen).** The rules judge a typed `Call`.
   shed's judge request and, for now, Claude's hook JSON are each parsed into it. Agent facts are read
   from shed's list.
2. **A drover module per agent** (an Antigravity provider with its own hook). Rejected by the owner
   (2026-11-24): new code per vendor, and most agents have no hook format.
3. **Translate a judge request into Claude's hook JSON and reuse `decide` as is.** Smallest diff, but the
   rules would keep reading a vendor's shape for good, and the cut-over could not delete it.
4. **Judge everything on merge, drop the hook format in the same PR.** No window, no second parser; but
   every fleet's Claude lanes would change guard at once, untested live, on a shed the hosts do not have.

## Decision

- **Role config.** `agent = "<shed agent id>"` on every role; any id is accepted. Until the cut-over
  `harness = "claude"` still loads as that module's agent id, so today's files keep loading; no default
  is built in. `Config::parse` stays pure; checks that need shed's list run at spawn, `drover set`,
  `drover config propose` and doctor.
- **Agent facts are data.** `core::agents` is the one reader of `shed agents --json` (`shapes.md`).
  drover passes `--model` and `--effort` and lets shed apply them. `[limits].effort` is dropped for an
  agent that takes none; a role's own model or effort the agent does not take is `BAD_VALUE`.
  rev-is-not-impl compares the record's program name.
- **The judge.** A judged lane starts with shed's policy `judge`, its command `drover hook` in a new ACP
  mode (`--acp`). The lane's plain variables go in `shed run --env`, so the judge reads its lane as the
  hook does today. The ACP mode maps the `toolCall` to a `Call` (`shapes.md`), builds the same `Policy`,
  runs the same `decide` and prints one verdict. Every failure is a deny; with no `DROVER_LANE` it denies too.
- **What the judge cannot do.** It cannot rewrite a call: a heavy command is denied once with the
  wrapped line to run. It gets no call after the fact: turn end, the drive kick, the push kick and the
  "end your turn" line move to shed's `reply` and `tool_call` events in `core::session`.
- **Which guard.** `Judge` for every agent whose module has no hook format, whatever `acp_policy` says;
  refused `ACP_UNGUARDED` when the host's shed has no judge. Claude stays `Hooked` until a fleet sets
  `acp_policy = "judge"`, then runs `Both`.
- **Spotting an agent the judge never sees.** drover writes every start itself and passes no
  auto-approve switch; a mode switch is denied. Two checks cover the rest:
  - *The test run* (`judge::probe`), before an agent version's first lane on a host and in doctor: a
    throwaway judged session is asked to run one shell command and write one file, both markers in a
    scratch folder; the judge logs and denies. Guarded only if the shell request arrived, drover could
    read it, and neither marker exists. Anything else refuses `ACP_UNGUARDED` and names the agent.
  - *The live check* (`judge::audit`): for each `execute`, `edit`, `delete` or `move` call in shed's
    stream with no permission request, drover replays its rules on shed's record of it. A call the rules
    refuse stops the lane, logs `lane-unguarded` and marks the agent unguarded on that host.
- **Providers.** `codex.rs` and `gemini.rs` are deleted, with the trait methods and registry functions
  that data replaces (`boundaries.md`). One module stays for what shed's record cannot carry. A remote
  lane of an agent with no token proxy runs on that host's own sign-in.
- **Docs.** One new section file in `docs/lanes/` (agents); `docs/lanes/hook.md` gains the judge entry.

## The cut-over

1. #1694 merges. Claude lanes stay `Hooked`; nothing changes for a fleet until a card.
2. Hosts get a shed with the judge (shed updates itself; doctor's row says when).
3. Card A (owner): one fleet's `rev` on `antigravity-acp`. One review round end to end; result recorded.
4. Card B (owner): `acp_policy = "judge"` under drover's own `[fleet.<n>.runtime]`. Claude lanes run
   `Both`: judged, in the default permission mode with "always ask" on the shell and write tools, the
   hook format still on and denying first.
5. **The gate: 10 judged rounds** of the Claude agent, at least 5 impl rounds that push and 5 rev rounds
   that record a verdict, with 0 `judge-unasked`, no `lane-unguarded`, no judge `hook-failed-closed`, and
   no lane left `blocked` on a permission. Doctor's agent row shows the count. A miss resets it.
6. The cut-over issue (filed when the gate is met) deletes `Dialect`, `hook_dialect`, the hook-format
   parser, the profile's hook block and `bypassPermissions`, `spawn::unguarded`, `stray_hooks`,
   `hooks_allowed`, `Guard`, `harness`, `[runtime] acp` and `acp_policy`; the harness chips become agent
   chips. Its card removes the dead keys from every fleet.toml.

## Consequences

- One PR for slices 1 to 5; the judged path is dark until a card. `Hooked` lanes keep today's start words.
- Two ways to guard exist until the cut-over. The gate and its issue bound that; the rules never fork.
- A heavy command costs a judged lane one extra round trip. The `filed #N` line of an orchestrator needs
  a tool's output, which shed's events do not carry: orchestrators need it solved before step 6.
- `REVIEW_SAME_PROVIDER` and effort checks move from config load to spawn, `set`, `propose` and doctor.
- drover's own path must have no space: `shed run --judge` splits its words at spaces. Doctor checks it.
- `shed-proto`'s pin moves to a shed main that carries shed#359.
- For shed, not blocking: `--judge` as repeatable words; a tool's output on the `tool_call` event.
- Rough size: about +650 src (agents, judge, probe, audit, follower) against about −250 (two modules,
  trait methods, registry functions, `acp_env`). The cut-over removes about 300 more.

## Slices

1. `core::agents`: shed's list typed; install, login, status, vendor, efforts and model read from it;
   config accepts any agent id; the moved checks.
2. `hook::Call`, `hook::policy`, `core::judge` and `judge::call`; the hook's ACP mode; the per-rule tests.
3. `Guard`; the judged start in `term` and `spawn`; the follower's turn end, kicks and turn line.
4. `judge::probe` and `judge::audit`; `agent-guard`, `judge-unasked`, `lane-unguarded`; doctor's rows.
5. Delete `codex.rs`, `gemini.rs` and what data replaced; docs; the PR body's `## Rollout` (cards A, B).
6. Its own issue, after the gate: the cut-over's step 6.

## Open questions for the owner

1. **The live Antigravity round.** A lane cannot run its build against a real fleet before merge.
   Recommended: amend the AC line; the round is card A, recorded on the rollout, and gates slice 6.
2. **Unasked calls.** Accept that a generic agent's own "safe" calls skip the judge and are checked
   after they ran (stop the lane on a refused one)? Recommended: yes; the test run blocks an agent that
   skips a write or a shell command that changes anything.
3. **The test run's cost**: two short turns per agent version per host. Recommended: accept.

## `.drover/arch.toml` lines

No central list or generated file is converted (`registry!` shrinks to one module and a test fake), so
there is no `frozen`, `generated` or `[[collect]]` line. The impl lane adds these bans in the PR:

```toml
[[ban]]
pattern = "antigravity"
paths = ["crates/*/src/**", "web/src/**"]
reason = "#1694: an agent is a row of shed's list; no code names one"
comments = true
ignore_case = true

[[ban]]
pattern = "tool_call"
paths = ["crates/*/src/**"]
except = ["crates/core/src/judge/**"]
reason = "#1694: an ACP toolCall is read in core::judge only; the rules take a hook::Call"

[[ban]]
pattern = "--judge"
paths = ["crates/*/src/**"]
except = ["crates/core/src/term.rs", "crates/core/src/judge/**"]
reason = "#1694: only term builds a judged start, from judge::command"

[[ban]]
pattern = "\"agents\", \"--json\""
paths = ["crates/*/src/**"]
except = ["crates/core/src/agents.rs"]
reason = "#1694: shed's agent list is read in core::agents only"
```

The cut-over issue adds `pattern = "hook_event_name"` and `pattern = "Dialect"` with no exception.
