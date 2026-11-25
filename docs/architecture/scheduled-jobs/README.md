# scheduled-jobs: one mechanism for lesson, triage, deps and hotspots (#1756)

## Summary

Decision: a scheduled job is one config entry (how often, an optional check command, the issue's title and body, the
role, the queue place). One module files its issue, queues it and starts its lane, with at most one open issue per
job. Lesson, triage, deps, advisory and hotspots become five built-in entries; their lane verbs and reports stay.
Slices: S1 the mechanism with lesson, triage, deps and advisory (about -150), S2 hotspots (about -95), S3 the old
config keys go (about -25). The series nets about -270, not -1,000: 790 of the four modules' 1,388 lines are lane verbs.
Open questions: one, not blocking: also cut the shadow lesson checks (#510, about -190 more, unused in this repo)?

Read on staging `456176c71`. No product code here.

| File | What |
|---|---|
| this page | how the callers read, the design on one screen, what changes for the owner |
| [adr.md](adr.md) | question, options, decision, consequences, every verb, key and event with keep, generalise or delete, the migration, the slices, the `arch.toml` lines |
| [shapes.md](shapes.md) | the entry as a type, the five built-in entries, the events |
| [boundaries.md](boundaries.md) | which module may call which, the two statecharts |

## The callers, sketched first

### the owner adds a job: config only

```toml
[job.audit]                         # every fleet; [fleet.api.job.audit] overrides it key by key
every_days = 30
title = "audit: {fleet} {month}"
body = "Review the auth paths against docs/security.md. One PR; list what you checked in its body."
role = "impl"
```

No code, no verb, no timer hook. A fleet turns a built-in job on, off or to another pace with one line:

```toml
[fleet.api.job.triage]
every_days = 7                      # built in as 0: on demand only

[fleet.api.job.lesson]
every_days = 0                      # was: lessons = "off"
```

### `tick`: one step for every job

```rust
pass.step("jobs");
crate::jobs::tick(ctx, cfg, &pass, out)?;
```

```rust
for (fleet, name, job) in cfg.jobs().filter(|(fleet, ..)| !cfg.fleet[fleet].paused) {
    if job.every_days > 0 && timer::due(ctx, &format!("{name}-{fleet}"), job.every_days * 86_400)? {
        jobs::run(ctx, cfg, r, out, (fleet, name), Trigger::Tick)?;
    }
}
```

### a person or an orchestrator: one verb

```
$ drover job herdsman
lesson     every 7 d    last 2026-11-23   next 2026-11-30   open #1741
triage     off
deps       every 30 d   last 2026-11-02   next 2026-12-02
advisory   every 1 d    last 2026-11-25   next 2026-11-26
hotspots   every 1 d    last 2026-11-25   next 2026-11-26   open #1730

$ drover job herdsman lesson
filed herdsman#1771: lessons: herdsman week of 2026-11-25
queued herdsman #1771 on top
spawned herdsman-lesson-1771
```

`job <fleet> <name>` replaces `drover lesson <fleet>`, `drover triage <fleet>`, `drover deps <fleet>` and the
filing half of `drover hotspots`. It is an orchestrator's and the owner's, exit 4 when the lane's prompt is not
confirmed, as `spawn`. The list is any lane's.

### drive and waiting: no job names

```rust
let role = jobs::role(&events, fleet, n).unwrap_or("impl");

if let Some(line) = jobs::empty(cfg, &events, fleet, n) {
    // the lane ended with nothing pushed: prompt it once, then
    jobs::close(ctx, cfg, r, out, (fleet, n), line)?;
}
```

`drover done --no-change <why>` calls the same `jobs::close` with the lane's own line.

### a lane: its verbs do not change

```
drover lesson digest <fleet> --days 7        drover triage close|fold|label|docs …
drover lesson apply <fleet>                  drover hotspots <fleet>
drover done --no-change "<line>" <fleet> <issue>
```

## The design on one screen

A job run has five steps, the same for every job:

1. **Due.** `tick` asks `timer::due("<job>-<fleet>", every_days)`. These are today's stamp files, so nothing runs
   early or twice at the upgrade.
2. **Check**, if the entry has one. The command runs in a checkout of the integration head. It passes: nothing to do,
   one line in `tick.log`. It fails: its output is the issue's `{report}`. This one rule replaces lesson's "under 2
   FAIL verdicts" skip, hotspots' "any hot file" and the advisory parsers.
3. **At most one.** No issue while one this job filed is open (from the log). No issue whose title exists already
   (one GitHub title search).
4. **File and queue.** The title and body are rendered, the issue is created, `job-filed` is logged, the issue is
   queued on top or at the end.
5. **Start.** On top: its role is spawned at once. At the end, or when the spawn is refused: drive starts it in turn,
   with the role `job-filed` names.

A job whose lane may end without a PR sets `empty`. Such a lane is prompted once, then its issue is closed with that
line and the list of what the lane changed, each with its undo.

`jobs.rs` names no job and calls no job's module. What is specific to a job is its entry, its role prompt and the
verbs its lane runs.

## What changes for the owner

- One verb, `job`, files any job by hand. A run by hand ignores the check's verdict (the owner said
  "correct"), and still obeys "at most one".
- Lesson and triage now wait while their last issue is open. Today they file a second one.
- Advisories: one `deps: advisories <date>` issue lists every current advisory with the audit tool's own output, in
  one PR. Today it is one issue, lane and PR per advisory id.
- The monthly deps issue no longer waits for an open advisory issue.
- Hotspots: at most one issue per calendar week and none while one is open. An open issue's body is no longer
  rewritten. `drover hotspots` only reports and exits 1 with any hot file.
- A job that fails to file logs `job-failed`, which alerts. The lessons-only `lessons-missed` alert goes.
- Triage's summary is the closing comment and the `closed` event. The thread reply goes, as cooee-threads D4 plans.
