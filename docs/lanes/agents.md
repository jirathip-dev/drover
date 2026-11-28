# Judged lanes: any shed agent (#1694)

A role names any agent in shed's list on the main host (`shed agents`):

```toml
[fleet.myapp.role.rev]
agent = "<a shed agent id>"   # no drover code knows the id
```

drover reads what it needs from that list (`core::agents`): the installed version and adapter, the install line,
the program it fronts (the account key, and what rev-is-not-impl compares), whether it takes a model and which efforts.
A role's own `model` or `effort` the agent does not take is refused `BAD_VALUE` at spawn; `[limits].effort` is
dropped for it. shed applies the model and effort itself (`shed run --model --effort`).

**The guard.** An agent with no drover module is judged: spawn starts it with shed's `judge` policy,
`shed run --policy judge --judge "<drover> hook --acp" --judge-secs <acp_judge_secs> --env DROVER_LANE=… --env
DROVER_GUARD=judge …`. shed runs the judge for each permission request and picks a once-only option by its answer.
Claude, the one agent with a module and a hook format, stays on its hook until a runtime sets
`acp_policy = "judge"`; then it runs both (`DROVER_GUARD=both`: judged, its hook format left on only to deny before
a call). A judged start runs on the main host only. A shed with no judge (`shed help run` lists no `--judge`), or
a drover path with a space (shed splits the judge command at spaces), refuses `ACP_UNGUARDED`, naming the agent.

**The test run.** Before an agent version's first judged lane, spawn starts a throwaway judged session in
`<state>/probe/<agent>/` on the role's account and asks it to run one shell command and write one file; the judge
(`hook --acp --probe <dir>`) records each request and denies it. The agent is guarded only if it asked to run the
shell command, drover could read it, and neither marker file exists. The verdict is logged once per version as an
`agent-guard` event (`guarded`, `unguarded`, `unreadable` or `inconclusive`); any but `guarded` refuses
`ACP_UNGUARDED` at once with its reason. `drover doctor` shows the row and forgets a failed stamp, so the next spawn
tests again. A test run gives up after `acp_probe_seconds`.

**Following a judged lane.** drive's intake reads shed's events for it:

- a judge's deny (`approved`, `by: judge`): its reason is kept for the turn;
- at the turn's end (`reply`): the turn's new refusals go to the lane as one prompt that starts "drover refused N call(s)",
  each reason once per lane; else an impl lane that ran no `drover turn` gets the turn line once; then drive is
  kicked (`stop`);
- a completed `git push` call kicks drive (`push`);
- an `execute`, `edit`, `delete` or `move` call that ran with no permission request is counted (`acp-unasked` at the
  turn's end) and replayed on the rules; one a rule refuses logs `lane-unguarded`, stamps the agent `unguarded` and
  stops the lane (`drover kill --yes`: unpushed work goes to `wip/`).

A heavy command cannot be rewritten through the judge: it is denied `REFUSED HEAVY_SLOT` with the wrapped line to
run instead.
