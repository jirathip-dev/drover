# judged-lanes: the callers' usage (#1694)

Sketches, not final code. Names are the ones `shapes.md` defines. Every lane is on the main host (#1691).

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
let agents = agents::Catalog::read(r)?;                    // `shed agents --json`, the main host's
let agent = agents.get(&p.acp).with_context(|| missing(&p.acp))?;   // names the id and `shed agents add`
agents::fits(agent, role)?;                                // BAD_VALUE: a model or effort it does not take
accounts::admit(cfg, r, &agent.vendor, &account)?;         // today's: LOGIN_NEEDED, from shed's `account.ls`
match acp::guard(cfg, fleet, agent) {
    Guard::Hooked => today's start, unchanged,             // gone at the cut-over
    guard @ (Guard::Judge | Guard::Both) => {
        acp::ready(r)?;                                    // ACP_UNGUARDED: this shed has no judge
        acp::probe::guarded(ctx, cfg, r, agent)?;          // ACP_UNGUARDED: names the agent and why
        let start = term::Judged {
            judge: acp::command(ctx),                      // [<drover>, "hook", "--acp"]
            secs: cfg.limits.acp_judge_secs,
            env: p.lane_env(guard),                        // DROVER_LANE, DROVER_PORT, DROVER_GUARD, … no secret
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
  --env DROVER_LANE=myapp:12:rev --env DROVER_GUARD=judge --account auto --model <m> \
  --name myapp-rev-12 --cwd <worktree> --label … -- sh -c '<secrets, QoS> exec "$@"' sh <the installed adapter>
```

An agent with no provider module gets no profile files: its brief carries the role text as today, and its
kick line adds "`drover help --agent` is your usage doc".

## the hook's ACP mode

```rust
Cmd::Hook { acp: true, probe, .. } => {
    let verdict = acp::answer(ctx, r, stdin, probe.as_deref());     // never an Err: every failure is a deny
    writeln!(out, "{}", json!(verdict))?;
}

pub fn answer(ctx, r, stdin, probe) -> Verdict {
    let req: Request = match parse(stdin) { Ok(q) => q, Err(why) => return closed(ctx, why) };
    let call = match acp::call::of(&req) { Ok(c) => c, Err(why) => return closed(ctx, why) };
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

## the follower (`session::stamp::on_event`, per shed event of a judged lane)

```rust
match &e.kind {
    EventKind::Turn { .. } => turn::opened(ctx, lane),                       // the turn mark: `prompt`
    EventKind::ApprovalRequest { call, .. } => audit::asked(ctx, &e.name, call),
    EventKind::Approved { by, decision, reason, .. } => turn::refused(ctx, lane, reason),   // a judge's deny: keep its reason
    EventKind::ToolCall { .. } => {
        audit::ran(ctx, cfg, &e.name, &e.kind)?;                             // unasked? replay the rules
        if let Some(id) = turn::pushed(&e.kind) { drive::kick_once(ctx, r, at, "push", Some(id)); }
    }
    EventKind::Reply { .. } => {
        audit::close(ctx, cfg, &e.name)?;                                    // `acp-unasked`, if any
        turn::ended(ctx, cfg, r, lane)?;   // the turn's refusals as one prompt; else the turn line once; else kick `stop`
    }
    …
}
```

What the lane reads after a turn with a denied call, as its next prompt:

```
drover refused 1 call:
- git push origin staging: pushing to `staging` is blocked…
```

## doctor and setup

```
✓ agent claude-acp 0.101.0 — guarded, 10 judged rounds, 0 unasked
✗ agent antigravity-acp 1.5.2 — unguarded: ran a shell command without asking → no lane starts on it
! agent gemini — not installed → shed agents add gemini
✗ shed — no judge policy (`shed run --judge`) → update shed
```

```rust
for id in agents::named(cfg) {                             // every agent a role names
    let a = catalog.get(id);
    rows.push(agents::row(a, acp::probe::stamp(&events, a), acp::audit::rounds(&events, id)));
}
```

Sign-in rows stay shed's account rows (`shed::account_rows`), keyed by the record's program name.
`setup`'s login list reads the record's `login` line.

## The acceptance tests

- `drover hook --acp` with an ACP `toolCall` on stdin, one test per rule, each asserting the JSON verdict
  and its reason: a push to staging and to main, `gh pr merge`, `gh issue comment`, a container command,
  an edit of fleet config, a reviewer's write and commit, the cap file, `OPAQUE_SHELL`, a mode switch.
- The same rule table driven through both parsers gives the same answers (until the cut-over).
- Fails closed: not JSON, `tool_call` null, an `execute` with no readable command, an edit with no path,
  no `DROVER_LANE`, fleet.toml not loading: each is a deny.
- A role with `agent = "fake-acp"` (a test catalog row, no provider module) spawns with `--policy judge`
  and the judge words; with a shed that has no judge it is refused `ACP_UNGUARDED`.
- The test run against a fake agent: asks first (guarded), runs without asking (unguarded), sends a
  command drover cannot read (unreadable), says nothing (inconclusive). The last three refuse and name it.
- The follower on a recorded event stream: a turn end kicks drive once; an unreported turn end sends the
  turn line once; a denied call's reason is the next prompt, once; an unasked call the rules refuse logs
  `lane-unguarded` and stops the lane.
