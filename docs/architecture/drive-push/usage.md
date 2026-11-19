# drive-push: how the callers read (#1309)

Read this first. These are the call sites after the change; `adr.md` says why, `shapes.md` has the types and
`boundaries.md` the module rules and statecharts. New names are in code blocks because they do not exist yet.

## A lane on a boat: nothing changes

```
$ drover turn done
queued for the main host: delivered within seconds
```

The lane still appends one line to its own worktree's outbox. It holds no key, token or route to the Mini.

## The boat's end: one command, three verbs

```
drover hosts report     # stdin: Watch (roots, intervals). stdout: one Frame, a Report: the whole truth. Exits.
drover hosts watch      # the same Report first, then one Frame a change, until its pipe closes.
drover hosts ack        # stdin: `<worktree>\t<sent>` a line: these outbox lines arrived; leave them out from now on.
```

`report` is #1308's one round trip per host and stays as the catch-up poll. `watch` is `report` kept open. Both run
on any host, the Mini included (there with no ssh). Nothing is installed on a boat: the watcher is the far end of
the Mini's ssh call and ends with it.

## The Mini's end: a link per host

```rust
// crates/service: one thread per [hosts] entry, the Mini's own included.
// core::link
pub fn run(ctx: &Ctx, cfg: &Config, r: &dyn Runner, host: &str, wake: &dyn Fn(Wake)) {
    loop {
        let _mine = lease(ctx, &format!("link-{host}.lock"));        // one reader of a host at a time
        match open(r, cfg, host) {                                    // ssh <host> drover hosts watch
            Ok(frames) => {
                for f in frames {                                     // a line each; None after link_stale_secs
                    match intake::apply(ctx, cfg, r, host, f) {       // view, events, cursor
                        Ok(woke) => woke.into_iter().for_each(wake),
                        Err(why) => break set(ctx, host, Link::Down { why }),
                    }
                }
            }
            Err(why) => set(ctx, host, Link::Down { why }),
        }
        wait(backoff.next());                                         // 1 s, 2 s … link_retry_max_secs
    }
}
```

With the service stopped, the timer's fast step does the same thing by poll:

```rust
// drive's fast step, before any fleet's pass: every host with no fresh word, in parallel, one call each.
for host in View::load(ctx, cfg)?.stale() {
    let woke = intake::apply(ctx, cfg, r, host, link::report(r, cfg, host)?)?;   // bounded by ssh_call_secs
    woke.into_iter().for_each(|w| wake(w));
}
```

`intake::apply` is the only writer of the view and the only place an outbox line runs.

## Reading a boat: the view, never ssh

```rust
// drive, waiting, lanes, check, stall, slo, the server: was session::all(cfg, r), one `shed ls` per host over ssh.
let view = View::load(ctx, cfg)?;               // state.db only
let sessions = view.sessions();                 // a host with no fresh word answers Err, as an unreachable one does today

// waiting.rs: was three ssh calls per issue.
//   ok(&on, "git", &["-C", wt, "log", "-1", "--format=%H %ct"])     the worktree's head
//   there.read(Path::new(&format!("{wt}/verdict.md")))               the reviewer's verdict
//   on.exists(wt)                                                    is the worktree there
let wt = view.worktree(&lane);                  // Option<&Worktree>: head, upstream, dirty, unpushed, verdict
let head = wt.and_then(|w| w.head.as_ref());
let verdict = wt.and_then(|w| w.verdict.as_ref()).filter(|v| v.mtime >= spawn.ts);

// hosts.rs admission: was a survey's ssh look, or `df` over ssh inside the pass.
let load = view.host(name).map(|h| &h.load);    // load per cpu, free memory, free disk per root

// status, board, waiting: was hosts::offline(ctx, cfg, sp).
match view.host(name).map(|h| &h.link) {
    Some(Link::Live) => {}
    Some(other) => line(format!("host offline: {name} ({})", other.why())),
    None => {}
}
```

No caller asks whether a lane is on the Mini or on a boat: the Mini's own lanes come through the same view.

## A fleet's pass: it plans

```rust
// `drover drive <fleet>`: one process, under drive-<fleet>.lock, ended at pass_max_secs. core::drive::plan
pub fn fleet(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str) -> Result<()> {
    let _mine = lease(ctx, &format!("drive-{fleet}.lock"))?;
    let _end = deadline(cfg.limits.pass_max_secs);          // logs pass-slow with its step, then exits
    gh::sync(ctx, r, cfg.fleet(fleet)?, fleet)?;            // the fleet's PRs: one query, as today
    let (events, view) = (working(&ctx.state)?, View::load(ctx, cfg)?);
    let plan = plan::of(ctx, cfg, &view, &events, fleet);   // pure: no program is started to make it
    for s in &plan.starts {                                 // queue order, fleet weights
        match starts::admit(ctx, cfg, &view, &events, s) {  // admit.lock: slots, host gate, caps, the marker
            Ok(slot) => job::start(ctx, r, s, slot),        // detached; returns at once
            Err(refusal) => note(s, refusal),               // SPAWN_PENDING, HOST_FULL, HOST_BUSY …
        }
    }
    for due in &plan.due {
        job::kick(ctx, r, fleet, due.issue, &due.id);       // the issue's job; the same id is driven once
    }
    log_pass(ctx, fleet, &plan)
}
```

## An issue's job: it acts

```rust
// `drover kick <fleet> <n> --now`, as today: under drive-<fleet>-<n>.lock, a job slot of its fleet.
// core::drive::job: today's per-issue body of the pass, unchanged in what it does.
verdict → merge (premerge build, under merge-<fleet>.lock) → auto-update of the other ready PRs
spawn rev | fix | respawn | conflict update        // admitted through starts::admit first, like a start
prompt, nudge, close a pane, save a lane's commits  // the calls that do reach a boat, each bounded by ssh_call_secs
```

A job reads boat facts from the view too. Its result is its events (`driven`, `spawn`, `merged`, `spawn-error`);
those wake the fleet's loop. Nobody waits for a job.

## The loops

```rust
// crates/service: one task per fleet. Timers alone: the fast job starts `drover drive <fleet>` for each, detached.
loop {
    select! { _ = woke(fleet) => {}, _ = sleep(fast_secs) => {} }
    run("drover", &["drive", fleet], pass_max_secs).await;    // a held lock: skipped; wakes meanwhile: one more pass
}
// A Wake that names an issue goes straight to its job; it also wakes the fleet when a slot may have freed.
```

## The tests every step shares

```rust
// One fixture: a remote that costs 0.7 s a call on the test clock, counts calls per host and can feed a stream.
let boats = SlowRemote::new(4).lanes(30).cost_ms(700);
drive_fleet(&ctx, &boats, "app");
assert_eq!(boats.calls_in_pass(), 0);                       // step 1 on: a pass never calls a boat to read
boats.lane("app-impl-12").runs(&["turn", "done"]);          // an outbox line appears on the fake boat
assert!(boats.acted_within_ms("app", 12, 5_000));           // step 2 on: its job started inside 5 s
boats.cut_stream("boat-2"); boats.heal("boat-2");           // sleep, reboot, split: no line lost, none run twice
```
