# ADR: any shed agent as a role's lane, guarded by shed's judge (#1694)

## Summary

Decision: a role names any agent in shed's list. shed asks drover before each tool call and drover answers allow or deny from the rules it has today. Agent facts come from shed's list, not from code. Every lane runs on the main host. A test run and a live check stop an agent that acts without asking.
Slices: read shed's agent list; the hook's new mode; start lanes under shed's judge; the test run and the live check; delete the Codex and Gemini code; later, after card A's Antigravity round and 10 clean Claude rounds, delete Claude's hook format.
Open questions: (1) a chat a client attaches (cooee's Orchestrator, a terminal) is guarded by Claude's hook format too. At the cut-over, hand it shed's judge command instead, and give an issue it files its home chat when the chat queues it? Recommended: yes.

Status: proposed, revised 2026-11-27 after the owner's revise (q3963). Read `usage.md` first, then this,
`shapes.md`, `boundaries.md`. `README.md` lists what this revision changes.

## Question

How does a role run any agent shed can start, with drover's guard on every tool call, no agent named in
drover's code, and a safe way off Claude's own hook format (q3480: no built-in default, every lane judged)?

## Where it stands (staging 3342daeae)

- `registry! { claude, codex, gemini }` in `providers/mod.rs`. `Config::parse` refuses an `agent` no module
  claims; spawn refuses `ACP_UNGUARDED` unless that module's hook is in the profile (`spawn::unguarded`).
- `gemini.rs` names `gemini-acp`; shed's id is `gemini`. `codex.rs` starts its agent in a full-access mode.
  No role uses either.
- `hook::decide` reads a hook-format JSON through `Dialect` tables; the rules are the 1,100 lines under it.
- Lanes start with `shed run --acp <id> --policy allow`; the Claude profile sets `bypassPermissions`.
- shed#359 is on shed's main: `--policy judge --judge "<cmd>"`. It runs the command per permission request
  (`name`, `labels`, `cwd`, `tool_call`, `options` on stdin), takes `{"decision", "reason"}`, picks a
  once-only option, and denies on any failure. The reason goes to shed's event stream, not to the agent.
  The main host's shed predates it.
- shed's list holds, per agent: install and login lines, the model variable, efforts, session paths. shed
  applies model and effort itself (`shed run --model --effort`).
- Sign-in and usage limits are shed's accounts (#1576): spawn holds on `account.ls` (`LOGIN_NEEDED`), and a
  running lane's `rate-limited` and `error auth:` reach `core::session` as shed's state.
- Lanes run on the main host only (#1691). #1697 deletes the token proxy and the start on another host;
  #1773 deletes drover's threads. `drover attach` stays: it hands a client's chat a settings file with
  drover's hook in Claude's format.

## Options

1. **One guard, one rule set, two parsers until the cut-over (chosen).** The rules judge a typed `Call`.
   shed's judge request and, for now, Claude's hook JSON are each parsed into it. Agent facts are read
   from shed's list.
2. **A drover module per agent** (an Antigravity provider with its own hook). Rejected by the owner
   (2026-11-24): new code per vendor, and most agents have no hook format.
3. **Translate a judge request into Claude's hook JSON and reuse `decide` as is.** Smallest diff, but the
   rules would keep reading a vendor's shape for good, and the cut-over could not delete it.
4. **Judge everything on merge, drop the hook format in the same PR.** No window, no second parser; but
   every fleet's Claude lanes would change guard at once, untested live, on a shed the host does not have.

## Decision

- **Role config.** `agent = "<shed agent id>"` on every role; any id is accepted. Until the cut-over
  `harness = "claude"` still loads as that module's agent id, so today's files keep loading; no default
  is built in. `Config::parse` stays pure; checks that need shed's list run at spawn, `drover set`,
  `drover config propose` and doctor.
- **Agent facts are data.** `core::agents` is the one reader of `shed agents --json` (`shapes.md`). The
  issue lists four facts; shed now applies three itself, so drover reads fewer: the model and effort go as
  `--model` and `--effort`, sign-in is shed's accounts, the config folder is `--account`'s. drover reads
  the install and login lines, the vendor, efforts, models and session paths. `[limits].effort` is dropped
  for an agent that takes none; a role's own model or effort the agent does not take is `BAD_VALUE`.
  rev-is-not-impl compares the record's program name.
- **Main host only.** One list, one account store, one test stamp per agent version. A judged start never
  goes to another machine; while the code still has that branch (until #1697) it refuses `ACP_UNGUARDED`.
  #1694 reads nothing of the token proxy, the outbox or a frozen rules file.
- **The judge.** A judged lane starts with shed's policy `judge`, its command `drover hook --acp`. The
  lane's plain variables go in `shed run --env`, so the hook reads its lane as it does today. The ACP mode
  maps the `toolCall` to a `Call` (`shapes.md`), builds the same `Policy`, runs the same `decide` and prints
  one verdict. Every failure is a deny; with no `DROVER_LANE` it denies too.
- **What shed's judge cannot do.**
  - It cannot rewrite a call: a heavy command is denied with the wrapped line to run.
  - It cannot tell the agent why: the follower reads the reason from shed's `approved` event and sends a
    turn's refusals as the lane's next prompt, each reason once per lane.
  - It gets no call after the fact: turn end, the drive kick, the push kick and the "end your turn" line
    move to shed's `reply` and `tool_call` events in `core::session`. A push kick fires on a completed
    `git push` call; shed's event has no output, so a push the remote rejected costs one empty pass.
- **Which guard.** `Judge` for every agent whose module has no hook format, whatever `acp_policy` says;
  refused `ACP_UNGUARDED` when the main host's shed has no judge. Claude stays `Hooked` until a fleet sets
  `acp_policy = "judge"`, then runs `Both`: judged in every way, with the hook format left on only to deny
  before a call.
- **Spotting an agent the judge never sees.** drover writes every start itself and passes no
  auto-approve switch; a mode switch is denied. Two checks cover the rest (both accepted by the owner,
  2026-11-26):
  - *The test run* (`acp::probe`), before an agent version's first lane and in doctor: a throwaway judged
    session on the role's account is asked to run one shell command and write one file, both markers in a
    scratch folder; the hook logs and denies. Guarded only if the shell request arrived, drover could read
    it, and neither marker exists. Anything else refuses `ACP_UNGUARDED` and names the agent. It costs two
    short turns per agent version.
  - *The live check* (`acp::audit`): for each `execute`, `edit`, `delete` or `move` call in shed's stream
    with no permission request, drover replays its rules on shed's record of it. A call the rules refuse
    stops the lane, logs `lane-unguarded` and marks the agent unguarded. It is a check after the call ran.
- **Providers.** `codex.rs` and `gemini.rs` are deleted, with the trait methods and registry functions that
  data replaces (`boundaries.md`). One module stays for what shed's record cannot carry: Claude's profile
  files, session options and sandbox needs, and until the cut-over its hook format. An agent with no
  module gets its role text in the brief and the usage doc from `drover help --agent`.
- **Names.** The module is `core::acp` and its keys and events say `acp`: #1571 uses `judge` for the lane
  that answers asks (`by: judge`, `judge-failed`). "The judge" here is always shed's policy.
- **Docs.** One new section file `docs/lanes/agents.md`; `docs/lanes/hook.md` gains the ACP mode;
  `docs/providers.md` shrinks to the one module.

## The cut-over

1. #1694 merges. Claude lanes stay `Hooked`; nothing changes for a fleet until a card.
2. The main host gets a shed with the judge (shed updates itself; doctor's row says when). The owner adds
   an Antigravity account in shed (`shed account add`), a host step.
3. **Card A** (owner): one fleet's `rev` on `antigravity-acp`. One review round end to end. The result is
   recorded on the rollout, not in the PR, whose body records the plan.
4. **Card B** (owner): `acp_policy = "judge"` under drover's own `[fleet.<n>.runtime]`. Claude lanes run
   `Both`, in the default permission mode with "always ask" on the shell and write tools.
5. **The gate: card A's round recorded, and 10 judged rounds** of the Claude agent, at least 5 impl rounds
   that push and 5 rev rounds that record a verdict, with 0 `acp-unasked`, no `lane-unguarded`, no
   `hook-failed-closed` from the ACP mode, and no lane left `blocked` on a permission. Doctor's agent row
   shows the count. A miss resets it.
6. The cut-over issue (filed when the gate is met) deletes `Dialect`, `hook_dialect`, the hook-format
   parser, the profile's hook block and `bypassPermissions`, `spawn::unguarded`, `stray_hooks`,
   `hooks_allowed`, `Guard`, `DROVER_GUARD`, `harness`, `[runtime] acp` and `acp_policy`; the harness chips
   become agent chips. Its card removes the dead keys from every fleet.toml. What it does for a client's
   attached chat is the open question below.

## Consequences

- One PR for slices 1 to 5; the judged path is dark until a card. `Hooked` lanes keep today's start words.
- Two ways to guard exist until the cut-over. The gate and its issue bound that; the rules never fork.
- A refused call's reason reaches a judged lane one turn late, and a bare heavy command costs it one
  extra turn. The usage doc gains a line telling lanes to run heavy commands wrapped; card B's rounds
  show the real cost.
- An agent with no module has only what shed's accounts check against the owner's own setup, and a fleet
  with `sandbox` refuses it (`BAD_VALUE`, names the agent): its folders and hosts are not known.
- `REVIEW_SAME_PROVIDER` and effort checks move from config load to spawn, `set`, `propose` and doctor.
- drover's own path must have no space: `shed run --judge` splits its words at spaces. Doctor checks it.
- `shed-proto`'s pin moves to a shed tag that carries shed#359 (the `approved` event's `by` and `reason`).
- For shed, not blocking: `--judge` as repeatable words; a denied call's reason shown to the agent.
- Order with the other cuts: #1694 edits none of what #1697 or #1773 delete. After #1697 the refusal for
  another machine is dead code and goes with that branch.
- Rough size: about +680 src (agents, acp, probe, audit, follower) against about −250 (two modules,
  trait methods, registry functions, `acp_env`). The cut-over removes about 300 more.

## Slices

1. `core::agents`: shed's list typed; install, login, vendor, efforts, models and session paths read from
   it; config accepts any agent id; `drover set --agent`; the moved checks.
2. `hook::Call`, `hook::policy`, `core::acp` and `acp::call`; `drover hook --acp`; the per-rule tests.
3. `Guard`; the judged start in `term` and `spawn`; the follower's turn end, kicks, turn line and a denied
   call's reason; `Both` mutes the hook format but for its deny.
4. `acp::probe` and `acp::audit`; `agent-guard`, `acp-unasked`, `lane-unguarded`; doctor's rows.
5. Delete `codex.rs`, `gemini.rs` and what data replaced; docs; the PR body's `## Rollout` (the shed
   account step, cards A and B with their fleet.toml lines, the gate).
6. Its own issue, after the gate: the cut-over's step 6.

## Open question for the owner

1. **A client's chat at the cut-over.** After #1773 the hook format has one more user than lanes: the chat
   a client attaches (`drover attach`, cooee's Orchestrator), which drover does not start and #1694 does
   not change. Step 6 would leave it with no guard. Recommended: step 6 makes `attach` answer with shed's
   judge words in place of the settings path, the client starts its agent under shed's judge, and step 6
   waits for cooee to do so (a cooee issue). The `filed #N` line goes with the format, since shed's judge
   sees no output: an issue's home chat is set when the chat queues it, which the log already carries.
   The other choice: keep the hook format for attached chats only, and never delete it.

## `.drover/arch.toml` lines

No central list or generated file is converted (`registry!` shrinks to one module and a test fake, and
the refusal texts are one file each already), so there is no `frozen`, `generated` or `[[collect]]` line.
The impl lane adds these bans in the PR:

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
except = ["crates/core/src/acp/**"]
reason = "#1694: an ACP toolCall is read in core::acp only; the rules take a hook::Call"

[[ban]]
pattern = "--judge"
paths = ["crates/*/src/**"]
except = ["crates/core/src/term.rs", "crates/core/src/acp/**"]
reason = "#1694: only term builds a judged start, from acp::command"

[[ban]]
pattern = "\"agents\", \"--json\""
paths = ["crates/*/src/**"]
except = ["crates/core/src/agents.rs"]
reason = "#1694: shed's agent list is read in core::agents only"
```

The cut-over issue adds `pattern = "hook_event_name"` and `pattern = "Dialect"` with no exception.
