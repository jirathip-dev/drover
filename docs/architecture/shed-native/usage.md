# shed-native: how the callers read (#1053)

Read this first. These are the call sites after the change; `adr.md` says why, `shapes.md` has the types,
`facts.md` maps every fact to its shed field and the code that goes.

## One read per pass, typed

```rust
// drive.rs, check.rs: was term::agents(r) + hosts::remote_agents(cfg, r), rows of serde_json::Value.
let sessions = session::all(cfg, r);          // every enabled host's shed `ls`; a host that fails is `Unreachable`
session::observe(ctx, cfg, &sessions)?;       // a changed status, an exit: stamped into the log as `lane-state`
```

`observe` is the only writer of `lane-state`. The service's event follower and drive's intake call the same
stamp for each shed event, so a host drover cannot stream (ssh) gets its stamps from the poll instead.

## Asking about one lane

```rust
// waiting.rs, gate.rs, design.rs, drive.rs, slo.rs: was a["name"] == l && a["agent_status"] == "working"
if sessions.busy(lane) { continue; }

// spawn.rs LANE_LIVE: was host::live_in(&agents, name, dir) over "cwd" / "foreground_cwd"
let live: Vec<&Session> = sessions.in_dir(&plan.agent, &plan.worktree).collect();

// term.rs awake (#908): was t.reported.state == "hibernated"
if session::get(r, agent)?.is_some_and(|s| s.status() == Some(Status::Asleep)) { /* admit first */ }
```

## The four rules: one function, used by check and by drive

```rust
// check.rs: the alert. drive.rs / stall.rs: the act. Both read the same Health.
for s in fold(&events).values().filter(|s| !s.done()) {
    let Some((role, lane)) = s.active_lane() else { continue };
    match lanes::health(ctx, cfg, &events, &sessions, s) {
        Health::Stalled { since } => fire("stall", format!("{role} lane {lane}: quiet for {}, no turn-end", age(now - since))),
        Health::NeverStarted { since } => { /* drive: resend the brief once, respawn once, then START_FAILED */ }
        Health::Dead(exit) => fire("lane-dead", format!("{role} lane {lane} is gone: {}", exit.cause.word())),
        Health::Fine | Health::Closed | Health::Unknown => {}
    }
}
for o in lanes::orphans(cfg, &events, &sessions) {   // a session no spawn names; fleet, issue, role from its labels
    fire("orphan-lane", format!("session {} has no spawn event", o.name));
}

// drive.rs respawn (#662): was shed::exit_cause(&events, lane) filtered by RESUMABLE strings
Health::Dead(exit) if exit.cause.resumable() => respawn(Resume::Session),
```

The usage limit and the sign-out are statuses too, never a banner on a screen:

```rust
// check.rs: was h.usage_limited(&screen) / h.signed_out(&screen) on term::recent(r, pane, "200")
match session.status() {
    Some(Status::Limited) => fire("usage-limit", hold_alert(&events, sp, "hit the usage limit")),
    Some(Status::Failed) if session.signed_out() => fire("signed-out", hold_alert(&events, sp, "is signed out")),
    _ => {}
}
```

## Showing a lane

```rust
// serve.rs, v1.rs, screens.rs, snapshot.rs
let lanes = lanes::join(cfg, r, &events, &st, &sessions);   // Lane { state: LaneState, session: Option<Session>, .. }

row.state = l.state.word();                 // working | blocked | idle | done | unknown | dead | parked | held by #N | asleep
row.activity = l.note().unwrap_or_default(); // shed's note, masked: "running: cargo test (8m)"
row.gone = l.session.is_none();             // was l.pane.is_none()
```

No screen is read to draw a row: `lanes::now_of`, `activity`, `spin` and the 30 s screen cache are gone.

## The transcript

```rust
// v1.rs lane/<agent>, screens.rs lane sheet, serve.rs /lane
match session::transcript::page(r, ctx, &lane, before, n)? {
    Transcript::Events(ev) => acp_blocks(&ev),            // an ACP session: shed's own events, paged by seq
    Transcript::Screen(text) => vec![Block::Screen],      // a PTY session: shed's `read` text, masked, as it is
}
```

`page` reads the live (or exited) session from shed; once drover has removed the session it reads the copy
`archive` saved at that moment. The same call serves spawn's handover to the next lane:

```rust
// spawn.rs handover: was the harness's JSONL (Provider::transcript) or the rolled pane log
let said = session::transcript::archived(ctx, prev_agent);   // prompts, replies, tool titles from shed's events
```

## Closing a session

```rust
// term.rs Shed::exec, Method::Kill (every close goes through it) and Shed::start's stale-name cleanup
session::transcript::archive(r, ctx, name)?;   // shed's log, copied before `rm` drops it
self.call(r, Method::Kill, &target)?;
self.call(r, Method::Rm, &target)?;
```

## The test (acceptance 3)

```rust
// crates/cli/tests/shed_native.rs, on common/fake_shed.rs
shed.lane("myapp-impl-12", "running", wt);              // reported {state: running, note: "running: cargo test (8m)"}
assert_eq!(v1_lane(&s, name)["state"], "working");          // /api/v1/state, /api/v1/lanes, the Lanes screen
assert!(check(&c, &shed).is_empty());

shed.set(name, |t| t["reported"] = json!({"state": "done"}));
clock.add(STALL + 1);
assert_eq!(v1_lane(&s, name)["state"], "done");
assert_eq!(kinds(check(&c, &shed)), ["stall"]);             // and none once a `turn-end` is logged

shed.exit(name);                                            // state exited, exit 1
assert_eq!(v1_lane(&s, name)["state"], "dead");
assert_eq!(check(&c, &shed)[0]["cause"], "adapter crash");
assert!(!v1_transcript(&s, name)["blocks"].as_array().unwrap().is_empty());
```

`a_shed_lane_reported_running_is_working_with_its_note` (#1033, `crates/cli/tests/herd.rs`) runs unchanged.
