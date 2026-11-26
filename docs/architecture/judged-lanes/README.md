# judged-lanes: any shed agent as a lane, guarded by shed's judge (#1694)

## Summary

Decision: a role names any agent in shed's list. shed asks drover's hook before each tool call and drover answers allow or deny from the rules it has today. Agent facts come from shed's list, not from code. A test run and a live check stop an agent that acts without asking.
Slices: read shed's agent list; the hook's new mode; start lanes under the judge; the test run and the live check; delete the Codex and Gemini code; later, after 10 clean Claude rounds, delete Claude's hook format.
Open questions: (1) the live Antigravity round needs the merged build, so record it on the rollout, not in this PR? (2) accept that an agent's own "safe" calls skip the judge and are checked after they ran? (3) accept two short paid turns per agent version per host for the test run?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how fleet.toml, spawn, the hook, the follower and doctor read after the change; the acceptance tests |
| `adr.md` | the question, options, decision, consequences, slices, the cut-over, open questions, the `.drover/arch.toml` lines |
| `shapes.md` | the types, what is read from shed's list, the ACP-to-hook mapping, the new events |
| `boundaries.md` | which module may call which, and the three statecharts |

## The design on one screen

| Fact or step | Today | After |
|---|---|---|
| Which agents can be a lane | the `registry!` list in code | any id `shed agents` lists on the lane's host |
| Guard before a tool call | the vendor's own hook format calls `drover hook` | shed's `judge` policy calls `drover hook` in its new ACP mode (`--acp`) for each permission request |
| The rules | `hook::decide` on a vendor-shaped JSON | the same rules on one typed `Call`; two parsers feed it until the cut-over, then one |
| Model, effort | per-provider code (`acp_env`) | `shed run --model --effort`; shed applies its own record |
| Install line, sign-in, status, folder variable | trait methods | fields of shed's record (`core::agents`) |
| Turn end, drive kick, push kick, the "end your turn" line | the vendor hook's stop and post-tool calls | shed's `reply` and `tool_call` events, in `core::session` |
| Usage limit, signed out, stall | shed's reported state | unchanged |
| An agent the judge never sees | not checked | a test run before its first lane (`ACP_UNGUARDED`, names the agent); a live check on shed's event stream |
| Heavy commands | the hook rewrites the call | the judge denies once with the wrapped line to run |

Nothing in core names an agent. `providers/` keeps one module, for what shed's record cannot carry
(Claude's token proxy, its profile files and session options), and until the cut-over its hook format.

## Open questions

1. **The live Antigravity round.** drover installs only CI's release build, so a lane cannot run the
   new build against a real fleet before its PR merges. Recommended: amend the AC so the live round is
   the first step of the rollout (the owner's config card), its result recorded there, and it gates the
   cut-over issue.
2. **Calls an agent does not ask about.** shed's judge sees only what the agent asks permission for. An
   agent may run calls it thinks are safe (a read-only shell command) without asking. Recommended:
   accept. drover replays its rules on shed's record of every such call and stops the lane when one is
   a call the rules refuse. For Claude the module sets its "always ask" rules, so nothing should skip;
   the cut-over's 10 rounds must show zero.
3. **The test run costs two short model turns** the first time an agent version is seen on a host.
   Recommended: accept; nothing else shows that an agent asks before it acts.
