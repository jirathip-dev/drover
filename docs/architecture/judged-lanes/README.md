# judged-lanes: any shed agent as a lane, guarded by shed's judge (#1694)

## Summary

Decision: a role names any agent in shed's list. shed asks drover before each tool call and drover answers allow or deny from the rules it has today. Agent facts come from shed's list, not from code. Every lane runs on the main host. A test run and a live check stop an agent that acts without asking.
Slices: read shed's agent list; the hook's new mode; start lanes under shed's judge; the test run and the live check; delete the Codex and Gemini code; later, after card A's Antigravity round and 10 clean Claude rounds, delete Claude's hook format.
Open questions: (1) a chat a client attaches (cooee's Orchestrator, a terminal) is guarded by Claude's hook format too. At the cut-over, hand it shed's judge command instead, and give an issue it files its home chat when the chat queues it? Recommended: yes.

Status: revised 2026-11-27 after the owner's revise (q3963). Read on staging 3342daeae.

## What this revision changes

1. **Main host only.** One agent list (the main host's shed), one test stamp per agent version. No start on
   another host, no token proxy, no frozen rules file: #1697 and the rest of #1691 delete those, and this
   design reads none of them.
2. **Sign-in and usage limits are shed's accounts** (#1576, merged). drover runs no status command itself.
3. **No step for orchestrators before the cut-over.** drover's threads are cut (#1773), so no lane needs a
   tool's output. What is left is the chat a client attaches: the one open question.
4. **The three old questions are decisions now:** the live Antigravity round is card A after merge and is
   recorded there; a call the agent did not ask about is checked after it ran and a refused one stops the
   lane; the test run's two short turns per agent version are accepted.
5. **Found in the redo:**
   - shed gives the agent no reason with a denied call. drover sends the reason as the lane's next prompt.
   - In the trial mode the old hook only denies. Turn end, kicks and heavy commands already run the new
     way, so the 10 rounds test what the cut-over leaves.
   - The new module is `core::acp`, not `judge`: #1571 uses that word for the lane that answers asks.

## Read in this order

| File | What |
|---|---|
| `usage.md` | how fleet.toml, spawn, the hook, the follower and doctor read after the change; the acceptance tests |
| `adr.md` | the question, options, decision, the cut-over, consequences, slices, the open question, the `.drover/arch.toml` lines |
| `shapes.md` | the types, what is read from shed's list, the ACP-to-hook mapping, the new events |
| `boundaries.md` | which module may call which, and the three statecharts |

## The design on one screen

| Fact or step | Today | After |
|---|---|---|
| Which agents can be a lane | the `registry!` list in code | any id `shed agents` lists on the main host |
| Guard before a tool call | the vendor's own hook format calls `drover hook` | shed's `judge` policy calls `drover hook --acp` for each permission request |
| The rules | `hook::decide` on a vendor-shaped JSON | the same rules on one typed `Call`; two parsers feed it until the cut-over, then one |
| Why a call was refused | the hook's answer, read by the agent at once | shed's event; drover sends it as the lane's next prompt |
| Model, effort | per-provider code (`acp_env`) | `shed run --model --effort`; shed applies its own record |
| Install line, login line, vendor, session paths | trait methods | fields of shed's record (`core::agents`) |
| Signed out, usage limit | shed's accounts (`account.ls`) | unchanged |
| Turn end, drive kick, push kick, the "end your turn" line | the vendor hook's stop and post-tool calls | shed's `reply` and `tool_call` events, in `core::session` |
| An agent shed's judge never sees | not checked | a test run before its first lane (`ACP_UNGUARDED`, names the agent); a live check on shed's event stream |
| Heavy commands | the hook rewrites the call | the hook denies once; the reason is the wrapped line to run |

Nothing in core names an agent. `providers/` keeps one module for what shed's record cannot carry (Claude's
profile files, session options and sandbox needs) and, until the cut-over, its hook format.

## The open question

**A client's chat at the cut-over.** `drover attach` hands a client a settings file whose hook is Claude's
format. drover does not start that agent, so #1694 does not change it. But the cut-over deletes the format,
and such a chat would then run with no guard. Recommended: the cut-over makes `attach` hand out shed's judge
command in place of the settings path, the client starts its agent under shed's judge, and the cut-over
waits for cooee to do so. The `filed #N` line needs a tool's output, which shed's judge never sees: it goes,
and an issue's home chat is set when the chat queues it (`drover queue add`, already logged with the chat).
The other choice is to keep the hook format for attached chats only, so the format never leaves the code.
