# main-host-only: how the callers read after the cut (#1691)

Each block shows today's code and what is left of it. Nothing here is new code but the last block of
"fleet.toml": the cut only removes a parameter, a branch or a wrapper.

## A lane's files and commands

```rust
// today: every caller asks which host, then goes through the wrapper
let on = hosts::On::of(r, sp);                      // ssh when the spawn names a machine
if on.exists(&wt) { ok(&on, "git", &["-C", wt, "rev-parse", "HEAD"])?; }
let verdict = link::read(ctx, cfg, r, sp, &path);   // the view, else `cat` over ssh

// after: the runner and the file system of this host
if wt.exists() { ok(r, "git", &["-C", wt, "rev-parse", "HEAD"])?; }
let verdict = link::read(ctx, cfg, sp, &path);      // the view, else this host's file
```

## Where a lane starts

```rust
// today
let placed = hosts::place(ctx, cfg, r, &events, (fleet, issue, role), (o.host.as_deref(), &skip))?;
let placed = hosts::wake(ctx, cfg, r, (f, o.dry_run), placed)?;        // start a stopped cloud host
let on = hosts::On::new(r, placed.as_ref().and_then(|p| p.machine.as_deref()));

// after: no host is picked. The main host is asked whether it admits one more lane.
let placed = hosts::place(ctx, cfg, r, &events, (fleet, issue, role), &skip)?;
match placed.as_ref().and_then(|p| p.refusal.as_ref()) {
    None => {}                                                         // admitted
    Some(("HOST_BUSY", why)) if o.force => log_override(why),
    Some((code, why)) => return refused(code, why.clone()),            // LOGIN_NEEDED, DISK_LOW, MEM_LOW, HOST_BUSY, NO_HOST
}
```

`place` returns `None` when fleet.toml has no `[hosts]` entry, as today. `NO_HOST` is left with one meaning: the
main host lacks a tag the lane needs.

## A lane's own verbs

```rust
// today, in cli::run
let res = match lane_write.then(|| forward(&sent)).flatten() {         // DROVER_OUTBOX set: queue the line
    Some(queued) => queued,
    None => dispatch(cli.cmd, ctx, r, out, stdin),
};

// after
let res = dispatch(cli.cmd, ctx, r, out, stdin);                       // every lane writes the log here
```

## Lane facts

```rust
// today
let sessions = link::view::View::load(ctx, cfg)?.sessions();          // one answer per host
match sessions.seen(agent, sp.str("machine")) { … }

// after: the same view, fed by this host's own watcher; one list
let sessions = link::view::View::load(ctx, cfg)?.sessions().checked()?;
match sessions.seen(agent) { … }
```

## A pass

```rust
// today: the pass's runner probes each host, holds an offline one off, counts calls it did not send
let pass = hosts::Pass::new(r, Some((mark, ctx.now))).sparing(hosts::disabled(&cfg), Some((ctx, &cfg)));

// after: it only marks the tick's step for the stuck-tick watch
let pass = hosts::Pass::new(r, Some((mark, ctx.now)));
pass.step("check");
```

## Self-update

```rust
// today: every enabled host stages, the main host waits up to self_update_lag_minutes, all swap together
match step(ctx.now, head, &found, &members, &marks, &cfg.limits) {
    Step::Hold { missing, until } => …,
    Step::Swap { ready, behind } => swap(…, &members, (ready, behind)),
    …
}

// after: one binary, this host's
match step(ctx.now, head, &found, &here, &marks, &cfg.limits) {
    Step::Current | Step::Wait { .. } | Step::Stuck => …,
    Step::Swap => swap(ctx, cfg, r, out, (t, events, head)),
}
```

## The command line

```
drover hosts                      the main host: tags, load, memory, disk, lanes
drover hosts refresh              probe its tags again
drover hosts on <name>            let lanes start again after a LOGIN_NEEDED hold
drover hosts eject <name> [root]  empty a workspace root
drover hosts report | watch       this host's own report for the view (the service runs it)
drover spawn <fleet> <n> <role>   no --host
```

Gone: `hosts off`, `hosts keys`, `hosts bootstrap`, `hosts snapshot`, `hosts destroy`, the `proxy` and `soak`
commands, `--host` on `spawn`, `kill` and `close`, and (last, behind its own gate) every `runners` verb.

## fleet.toml

```toml
[hosts.main]                   # optional, at most one, no `machine`: the main host's limits
tags = ["fast"]                # added to the probed ones (os, arch, xcode, docker, webkit)
roots = ["/Volumes/fast/drover", "~/.local/share/drover/work"]
max_working = 12
max_simulators = 1
start_max = 2
heavy_max = 3
heavy_kind_max = { swiftlint = 1 }
heavy_wait_minutes = 30
deny_commands_add = ["docker"]

[fleet.app.heavy]              # unchanged: kind → command patterns that take a heavy slot
xcodebuild = ["xcodebuild *"]
```

An old file keeps loading. A removed key is ignored. An entry with `machine` is dropped at load and named once
by doctor, the board's config line and `check` (`shapes.md`, "Old keys").

## What drover will need from `runners run`

The later remote-build policy marks some `[fleet.<n>.heavy]` kinds as remote. For those the hook rewrites the
lane's command to one call, as it rewrites it to `drover heavy <kind> … -- <cmd>` today. Its design is
jirathip-dev/runners#11's; this is only what drover needs from it.

```
runners run --pool <name> --wait <secs> --timeout <secs> -- <cmd> [args…]
```

- It runs in the lane's worktree and builds that worktree as it is, uncommitted changes included.
- It is one foreground process: the command's output is its output, and when it is killed the remote job ends.
- It needs no drover state and no drover call.

| Exit | Means | What drover does |
|---|---|---|
| the command's own (0 to 123) | the command ran on a build machine | passes it to the lane unchanged |
| 75 | no build machine took the job within `--wait`; nothing ran | the policy's choice: run it here in a heavy slot, or tell the lane |
| 124 | `--timeout` passed; the remote job was killed | passes it on, as its own timeouts do |
| 125 | `runners` itself failed (sync, link, pool); the result is not the command's | as 75, and logs the reason |

75 and 124 are the codes `drover hold` and drover's runner already use for the same two cases.
