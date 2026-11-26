# judged-lanes: the callers' usage (#1694)

Sketches, not final code. Names are the ones `shapes.md` defines.

## fleet.toml

```toml
[fleet.myapp.role.impl]
agent = "claude-acp"          # any id `shed agents` lists; no `harness`
model = "opus"

[fleet.myapp.role.rev]
agent = "antigravity-acp"     # no drover code knows this id

[fleet.myapp.runtime]
acp_policy = "judge"          # until the cut-over: also judge the agent that still has a hook format
```

A role that names no agent is a config error that names the role. Until the cut-over `harness = "claude"`
still loads and means `agent = "claude-acp"`; nothing is defaulted.

## spawn

```rust
let agents = agents::Catalog::on(&on)?;                    // `shed agents --json` on the lane's host
let agent = agents.get(&p.acp).with_context(|| missing(&p.acp, &on))?;   // names the id and `shed agents add`
agents::fits(agent, role)?;                                // BAD_VALUE: a model or effort it does not take
match judge::guard(cfg, fleet, agent) {
    Guard::Hooked => today's start, unchanged,             // gone at the cut-over
    Guard::Judge | Guard::Both => {
        judge::ready(&on)?;                                // ACP_UNGUARDED: this host's shed has no judge
        judge::probe::guarded(ctx, cfg, &on, agent)?;      // ACP_UNGUARDED: names the agent and why
        let start = term::Judged {
            judge: judge::command(ctx, &on),               // [<drover>, "hook", "--acp"]
            secs: cfg.limits.judge_secs,
            env: p.lane_env(),                             // DROVER_LANE, DROVER_HOST, DROVER_RULES, … no secret
            model: p.model.as_deref(),
            effort: agents::effort(agent, p),              // only one the agent lists
        };
        shed.start_judged(ctx, r, &p.agent, labels, cwd, &acp, &start)?;
    }
}
```

The words drover runs:

```
shed run --acp antigravity-acp --policy judge --judge "<drover> hook --acp" --judge-secs 30 \
  --env DROVER_LANE=myapp:12:rev --env DROVER_HOST=main --model <m> --name myapp-rev-12 --cwd <worktree> \
  --label … -- sh -c '<secrets, QoS> exec "$@"' sh <the installed adapter>
```

## the judge

```rust
Cmd::Hook { acp: true, probe, .. } => {
    let verdict = judge::answer(ctx, r, stdin, probe.as_deref());   // never an Err: every failure is a deny
    writeln!(out, "{}", json!(verdict))?;
}

pub fn answer(ctx, r, stdin, probe) -> Verdict {
    let req: Request = match parse(stdin) { Ok(q) => q, Err(why) => return closed(ctx, why) };
    let call = match judge::call::of(&req) { Ok(c) => c, Err(why) => return closed(ctx, why) };
    if let Some(dir) = probe { return probe::seen(dir, &call) }     // logs the call, denies
    let policy = match hook::policy(ctx, r, &lane()?) { … Err(why) => return closed(ctx, why) };
    match hook::decide(&call, &policy, &git).or_else(|| heavy_line(&call, &policy)) {
        Some(reason) => Verdict::deny(reason),
        None => Verdict::allow(),
    }
}
```

`hook::policy` is today's body of `run_hook` that builds `hook::Policy`, moved so both entries share it.
`closed` logs `hook-failed-closed` and answers deny with `REFUSED HOOK_FAILED_CLOSED: …`.

## the follower (core::session, on the lane's host)

```rust
match &e.kind {
    EventKind::Turn { .. } => turn::opened(ctx, lane),                       // the turn mark: `prompt`
    EventKind::ApprovalRequest { call, .. } => audit::asked(ctx, &e.name, call),
    EventKind::ToolCall { .. } => {
        audit::ran(ctx, cfg, &e.name, &e.kind)?;                             // unasked? replay the rules
        if let Some(id) = turn::pushed(&e.kind) { drive::kick_once(ctx, r, at, "push", Some(id)); }
    }
    EventKind::Reply { .. } => {
        audit::close(ctx, cfg, &e.name)?;                                    // `judge-unasked`, if any
        turn::ended(ctx, cfg, r, lane)?;                                     // the turn line once, else kick `stop`
    }
    …
}
```

## doctor and setup

```
✓ agent claude-acp 0.101.0 on main — signed in, guarded, 10 judged rounds, 0 unasked
✗ agent antigravity-acp 1.5.2 on main — unguarded: ran a shell command without asking → no lane starts on it
! agent gemini on build-1 — not installed → shed agents add gemini
```

```rust
for (host, id) in agents::named(cfg) {                     // every agent a role names, per host it may run on
    let a = Catalog::on(&on)?.get(id);
    rows.push(agents::row(a, agents::signed_in(&on, a), judge::probe::stamp(&events, host, a)));
}
```

`setup`'s login list and a host's health check read `agent.login` and `agent.status` the same way.

## The acceptance tests

- `drover hook` in ACP mode with an ACP `toolCall` on stdin, one test per rule, each asserting the JSON verdict
  and its reason: a push to staging and to main, `gh pr merge`, `gh issue comment`, a container command,
  an edit of fleet config, a reviewer's write and commit, the cap file, `OPAQUE_SHELL`, a mode switch.
- The same rule table driven through both parsers gives the same answers (until the cut-over).
- Fails closed: not JSON, `tool_call` null, an `execute` with no readable command, an edit with no path,
  no `DROVER_LANE`, an unreadable rules file, fleet.toml not loading: each is a deny.
- A role with `agent = "fake-acp"` (a test catalog row, no provider module) spawns with `--policy judge`
  and the judge words; with a shed that has no judge it is refused `ACP_UNGUARDED`.
- The test run against a fake agent: asks first (guarded), runs without asking (unguarded), sends a
  command drover cannot read (unreadable), says nothing (inconclusive). The last three refuse and name it.
- The follower on a recorded event stream: a turn end kicks drive once; an unreported turn end sends the
  turn line once; an unasked call the rules refuse logs `lane-unguarded` and stops the lane.
