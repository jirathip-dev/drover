# The shed split: what leaves drover, and in which order (#1567)

## Summary

Decision: drover keeps the policy (which lane goes where and when, spend, approvals, the guard) and stops doing host work itself. It talks to shed through shed's own client, reaches every other host over the link shed already holds, reads health, processes, accounts and usage from shed, takes provider facts from shed's catalog, and leaves the rollout of its own binary, secrets, heavy slots and the sandbox to shed. It asks shed to install nothing else, and sets no host up itself: toolchains, the shared build caches and a host's disk floor belong to the host setup recipe of shed's VM helper (shed #337), which the owner runs on the Mini and the Air through a run card. A lane's own build folders, and the cache settings its start carries (#1641), stay drover's. Renting VMs has left this plan for its own decision (#1644), and CI runners for their own repo (<owner>/runners): drover's runner code is deleted once that repo covers it. Four things stay: the copy of a closed lane's transcript, the handover of drover's own web server, the sweep of stray processes in a finished lane's folder, and the file through which a lane on another host sends its commands to the main host (#1646): shed has no connection from a host back to the main host, so the main host keeps fetching that file.
Slices: #1623's small fix (a rented host that shed shows as up counts as up) has landed, `e24a53998`. Then fourteen drover steps, each after the shed release it needs and each deleting the code it replaces: the account for lanes and orchestrator threads on the main host (#1576, no key set means "auto") and the client; provider facts; the token proxy's deletion (#1604); hosts as shed machines; host charts; the host link; secrets; self-update, for drover's own binary only; heavy slots; the sandbox; balances; the timer jobs through farrier; shed's new state fields. shed's two releases of 2026-11-22 and 2026-11-23 already carry what the first four need. Two installs wait for a check: #1576 until drover's doctor shows a signed-in named account for each agent, #1604 until no lane started on drover's proxy is still alive. Until #1604 a lane on another host signs in through drover's proxy, as today.
Open questions: (1) an agent that shed's proxy does not serve (Codex today) cannot have an account on another host. With no account key set, let it run there on that host's own login, as today (recommended, and what shed's accounts decision says), or keep such lanes on the main host? (2) shed's health reading of a host has carried its free disk since shed's first release, and drover would read it there for its low-disk refusal and the Host tab's chart. Your decision names a machine as link, processes, CPU and memory: keep that one read (recommended: it is a number, and shed cleans nothing), or have drover measure free disk itself? Your four answers of 2026-11-23 are decisions here; the newest one's points are listed at the top of adr.md with where each landed.

Read this file, then [adr.md](adr.md). [order.md](order.md) has the steps, their gates, the two folded symptoms
and the issues to file; [shapes.md](shapes.md) has the config keys, the types and the pins. It follows shed's
accepted ADRs and reopens none of them: the client and the catalog (shed #191, its shed-client ADR),
accounts (shed #256, its accounts ADR),
hosts (shed #259, its hosts ADR), and the two that change #259:
the VM helper (shed #307, its shed-cloud ADR) and
the state table (shed #309, its state-table ADR). It also follows the owner's decision of
2026-11-23 on what a machine is, filed as shed #336 and #337, and the owner's two of the same day on CI runners
(#1644's last amendment) and on the host facts (#1649). It replaces #1561.

## Each candidate

Sizes are the issue's, in src lines. "Step" is the drover issue in [order.md](order.md).

| # | candidate | decision | why | shed API it needs | step |
|---|---|---|---|---|---|
| 1 | remote transport: `On`, the per-pass probe, `ssh … shed connect` (~650) | move | shed holds one link per machine; drover's second ssh path has its own failures (shared connections, session caps, a changed host key) | `exec` with `env`, `stdin` and `timeout`; `file.read`; `file.write` with `mode`; `machine.add`; `machine.links` with `down_since`, `hello`, `os`, `arch`, `home` | D1 |
| 2 | host and process sampling: `host.rs`, the `ps` half of `top.rs`, the history writer (~550) | move, but the folder sweep | shed samples every machine each minute and knows each terminal's process tree | `machine.stats` with `top`, `since` and the memory level (X1); `terminal.procs` with a process's CPU time (X1); `kill` | D1, D12 |
| 3 | the hand-rolled client and the CLI word builders (~370) | delete | shed ships the client; drover's copy probes `shed help run` and starts a process per prompt | the `shed-client` crate at a tag; `run` with `model`, `effort`, `env`, `meta` | D0 |
| 4 | launchd and systemd in `timer.rs` (~250) | move | farrier-service renders, loads and checks the same jobs; shed uses it | farrier F1: control commands through the caller's runner | D10 |
| 5a | the dsh setup-wizard watcher (~80) | delete | shed's dsh record has the wizard, the echo check and the ready wait | the record's `[terminal]` table (on shed staging) | D9 |
| 5b | the transcript copy in `session/transcript.rs` (~60) | keep | it is drover's record of a lane drover removed; shed keeps nothing after `rm` by design, and nobody else has this code | none | |
| 5c | the serve handover and its socket passing (~100) | keep | it hands drover's own listeners to drover's next build; shed's handover is for shed's own server | none | |
| 5d | the link's carriage over ssh (~150) | move the carriage, keep the report | shed already streams every machine's state changes over its link | `events` across machines; `exec` | D11 |
| 6a | accounts and usage on the main host: the account key, the login hold, the limit wait | move | the owner's decisions; shed #256 | `run`'s `account`, `account.ls`, `account.login`: all in `client-v5.0` and after | D2a |
| 6a | the token proxy (`proxy.rs` 492), and accounts on other hosts | move | the same; a run on another host never holds a token | shed #265, in `client-v5.1`: `run` with `machine` and `account`, `Link::proxy`, the proxy's key list and its revoke | D2b |
| 6b | provider facts in `providers/` (~400) | move | one catalog for shed, cooee and drover | the `shed-catalog` crate | D9 |
| 7 | binary rollout: `release.rs` and the shell half of `update.rs` (~500 of 880) | move | shed #259: one checked install path. Narrowed by the owner (2026-11-23) to shed's and drover's own binaries, and this code moves only drover's | `install.find`, `ls`, `stage`, `swap`, `back`, for the caller's own binary | D4 |
| 8a | lane-host VMs (`cloud.rs`, ~490 with its budget) | move, under #1644 | shed #307 replaced #259's plan: an optional helper rents VMs for agents and keeps their budget; drover starts and stops none | `shed-cloud ls` and `demand`; a VM is a shed machine (D1) | #1644 |
| 8b | CI runners (`runners.rs`, 359; the `drover runners` verbs; `[ci_runners]`; the recipe's `runner_*` lines) | delete, once `<owner>/runners` covers them | the owner (2026-11-23): the Mini's native runners, the Docker slots and the runner VMs with their image are that repo's. They are not shed-cloud's, and a runner is no shed machine, so nothing of this moves to shed | none | #1644, as amended |
| 9 | the secrets store in `secret.rs` and the file beside a worktree | move | shed #259: one store, and a lane's secret only in its run's memory | the `shed-secrets` crate; `run`'s `env` | D3 |
| 10 | heavy slots (`heavy.rs`) and the waits in `top.rs` | move | shed #259: a slot is a lease a connection holds | `slot.take`, `free`, `ls`, `limits`; `shed slot run` | D5 |
| 11 | the sandbox wrap (`sandbox.rs`) | move | shed #259: shed builds an ACP lane's command, so only shed can wrap it | `run`'s and `exec`'s `sandbox` | D7 |
| 12 | the DeepSeek balance probe | move | shed #259: a balance is a reading on an account | the record's `[balance]`, `Account::balance` | D8 |
| 13 | host setup: the tools `drover hosts bootstrap` installs (git, gh, just, Rust, node; ~15 lines of `cloud.rs`) | leave drover, and not to shed | the owner (2026-11-23): toolchains, shared caches and the disk floor are shed-cloud's host setup recipe | none of shed's: `shed-cloud setup --on <machine>` (shed #337), from a run card on the Mini and the Air | #1644 |
| 14 | a lane's build folders (the tick's trim, `reclaim`, `clean`, `DISK_BUDGET`), `DISK_LOW`, the cache variables on a lane's start (#1641) | keep | the owner (2026-11-23): they are lane state; drover deletes no shared cache | none | |

What the issue asked to be settled, one line each:

- **A private mode for `file.write`:** `mode`, in shed #266. D3 then removes the need: a lane's secrets stop being a file.
- **A batch exists call:** `exec` is enough. `On::existing` is already one script with the paths on stdin; it becomes one `exec` with the same script and `stdin`.
- **Link down since:** `Link::down_since`, in shed #266. drover's `hosts.down` file goes.
- **Free memory, swap-out, totals, top-N:** in shed #266. One thing is missing: the kernel's memory pressure level, which drover's `HOST_BUSY` gate reads. That is X1. The swap-out is a count from boot: drover's rate is the difference between two passes' `now`, not the per-minute history.
- **A `terminal.procs` tree:** in shed #266, with `own` marking the agent's own processes. Its `cpu_pct` is an average over a process's life, so a lane's CPU share is the CPU time between two reads; X1 adds that time (`cpu_ms`).
- **A tagged client release:** `client-v5.0`, pushed by shed's tag job (#267) on 2026-11-22, and `client-v5.1` a day later.
- **How `[hosts]` become shed machines:** the `[hosts]` name is the machine's name and its `machine` value is the target ([shapes.md](shapes.md)).

Policy that stays, as the issue lists it: placement, needs, slots and holds, admission and pacing, lane-hog, who
gets a secret, profiles sync, the hook guard, the plan and the brief, with the lane's cache variables in it
(#1641). VM spend and budgets stay until #1644, which moves them to shed's helper as the owner decided on
2026-11-23. Runner registration, tokens and health stay in `runners.rs` until `<owner>/runners` covers
them; they are then deleted, not moved.

## How it reads

One module, `crate::shed`, is the only code that names shed's client. Everything else holds a `Host`.

```rust
let shed = shed::local(ctx)?;                          // this host's socket; SHED_VERSION under the client's floor
let host = shed.host(cfg, "box", r)?;                  // the machine "box"; `shed.here(r)` is this host

// D1: what `On` did over ssh. A Host is still a Runner, so these callers do not change.
ok(&host, "git", &["-C", wt, "rev-parse", "HEAD"])?;   // exec {machine: "box", cmd, timeout}
host.existing(&worktrees);                             // one exec: the script, the paths on stdin
host.read(&verdict); host.write(&brief, text)?;        // file.read, file.write

// D1: offline and admission, with no probe
let links = shed.links()?;                             // once a pass; no network: the main shed holds the links
let reach = reach(&links, "box", ctx.now, &cfg.limits);
match &reach {
    Reach::Up => {}
    Reach::Draining { .. } => no_new_lane(),           // #1644: calls go on, its lanes finish
    Reach::Down { since, why } => skip(since, why),    // reconnecting or down: the pass sends it nothing
    Reach::Offline { since, why } => alert_once("host-offline", since, why),
}
let s = host.sample(3, seen.swap("box"))?;             // machine.stats {top: 3}: `now`, and the swap rate against the last pass's `now`
host::admit(&s, &cfg.limits)?;                         // the same rules: level, swap median, load without drover's own

// D1: #1649's host fact. The pass stores the link and that same sample; nothing else reads shed for it
view.host_seen("box", &reach, Some(&s));               // the row `facts::host` reads (#1654): up, loaded, draining, offline

// D1: a lane's CPU share, for top and lane-hog: two reads, never `cpu_pct`
let a = host.procs(&lane.name)?;                       // terminal.procs "box/lane-256-impl"
clock::sleep(cfg.limits.cpu_sample());
let share = shed::busy(&a, &host.procs(&lane.name)?, s.cpus);   // percent of the host's CPUs between the two
```

The account, at D2a: before the client, so it rides the start words drover builds today. Both callers share it.

```rust
// spawn.rs, an ACP lane                              // orchestrator.rs start(), a thread
let account = cfg.account(fleet, &role);              let account = cfg.account(fleet, "orchestrator");
let acp = term::Acp { account: here.then_some(account), ..acp };   // `--account auto`; none on another host until D2b
match b.start(ctx, r, &agent, &labels, &dir, &acp) {
    Err(e) if code(&e) == Some("ACCOUNT_SIGNED_OUT") => login_needed(&who, &e),            // the card's [sign in]: account.login
    Err(e) if code(&e) == Some("ACCOUNT_LIMITED") => hold_until(&who, first_reset(&shed::accounts(r, w)?)),
    Err(e) if code(&e) == Some("NO_ACCOUNT") => name_your_logins(&who, &e),                // one card: `shed account add …`
    other => other?,
}
```

A lane's start, after D0, D2b, D3 and D7 (each adds one field; none adds a branch per provider):

```rust
let run = RunParams {
    name: Some(p.name()), cwd: p.worktree(), labels: p.labels(),
    acp: Some(record.id.clone()), model: role.model.clone(), effort: role.effort.clone(),   // D0: shed builds the command
    meta: provider.guard_meta(&p),                     // drover's: the hook and the settings
    account: p.account(),                              // D2a here; D2b on a machine, where shed's proxy gives the run a key
    env: p.env_with(secrets(&store, &p)?),             // the plan's env, #1641's cache variables in it; D3: secrets in memory only
    sandbox: f.sandbox.then(|| sandbox(&p)),           // D7
    machine: host.machine(), ..RunParams::default()
};
shed.run(&run)                                         // the same three refusals, now `e.code()`
```

Provider facts, after D9. `providers/` keeps one file per provider with what drover does to it and no fact about it:

```rust
let record = catalog.by_agent(&role.harness).ok_or_else(|| unknown(&role.harness))?;
doctor_row(&record.status);  card_text(&record.login);  fix_line(&record.install);   // text: a doctor row's fix
provider.sync(ctx, role, prompt, profile)?;            // stays: settings, hooks, the role prompt
provider.guard_meta(&p);  provider.hook_dialect();     // stays
```

Self-update, after D4: drover's own binary, and no call that could name another tool or path.

```rust
let rel = shed.release(&head)?;                        // install.find: drover's release at that build, read once here
for h in &others { h.stage(&rel)?; }                   // install.stage at that host's drover path, checked there
for h in others.iter().chain([&here]) { h.swap(&head)?; }   // install.swap: the main host last, as today
// h.back() is install.back, when the new build fails its check after the swap
```

The link, after D11: one follower of shed's events instead of a watcher per host.

```rust
for ev in shed.events(&lanes_only)? {                  // every machine's, over shed's links
    if let EventKind::State(_) = ev.kind { due.insert(host_of(&ev.name)); }
}
// at most once per `link_poll_millis` per host, and every `link_sample_secs` anyway
let frame = host.report(&Watch { worktrees, roots, acked })?;   // this host: in process; a machine: exec `drover hosts report`
intake::apply(ctx, cfg, r, out, host.id(), frame)?;    // unchanged: the view's rows, then each outbox line, run once
```

A lane's verb on another host, before and after every step here (#1646). There is no link back to the main host,
so the verb calls nothing:

```rust
// on box, in the lane: `drover triage close …`, `drover turn done`, `drover ask …`
if let Some(lane) = outbox::lane() { return outbox::queue(&lane, &args) }   // one line in <worktree>/.drover/outbox.jsonl
// on the main host: the report above carries the line; intake runs it as that lane and logs `outbox`;
// the next report's `acked` tells box, which moves its acked mark
```

The timer jobs, after D10:

```rust
let job = Job::new(TICK, &[exe, "tick"], Schedule::Every(secs))?
    .path(&path).log(Log::File(log)).launchd(abandon).supersedes(["dev.herdsman.tick"]);
let service = Service::new(job, &env);                 // env carries drover's runner (F1)
match service.install()? { Installed::Unchanged => already(TICK), _ => loaded(TICK) }
if let Some(why) = service.status()?.refused { heal(&service, why)? }
```

## Statecharts

### A host, as drover reads it after D1

```
 [hosts.box] machine = "…"
        │ register: machine.add, twice is once
        ▼                the name is taken by another target
     added ─────────────────────────────────────────────────▶ refused: a doctor row, no lane goes there
        │ the link is up
        ▼    the link is reconnecting or down      down_since older than host_offline_minutes
       up ──────────────────────────▶ down ──────────────────────────────────────────▶ offline
      │ ▲ ▲                          ▲ │ up again                                         │ up again
      │ │ └──────────────────────────┼─┴──────────────────────────────────────────────────┘
      │ │ the drain is lifted        │ its link leaves up
      ▼ │                            │
    draining ────────────────────────┘        shed says the machine drains (#1644)
```

| state | `Reach` | what drover does |
|---|---|---|
| up | `Up` | calls, places lanes |
| draining | `Draining` | places no new lane; every call still goes, so its lanes are read, prompted and ended as on an up host and finish there. No alert. What is saved when a lane ends there is #1644's |
| down | `Down` | sends it nothing this pass; its lanes wait |
| offline | `Offline` | one `host-offline` alert with the link's error; its lanes show `host offline` |
| refused | | nothing, until the owner removes shed's other machine of that name |

`hosts off` is beside this: a disabled host is sent nothing, whatever its link says. A host taken out of
fleet.toml is sent nothing either, and shed keeps the machine. drover stores none of this: each state is read
from `machine.links` at the start of a pass. Nothing in drover's own log can hold a host offline while its link
is up: that is #1623's cause. Its fix, `e24a53998`, ends it for a host shed lists `up`; D1 deletes the rule.

`machine.links` has three link states and no drain: `up` is up, `reconnecting` and `down` are down, then offline
by the age of `down_since`. So D1 builds `Reach::Draining` and every caller's arm for it, and never returns it.
The step that moves `reach` to `machine.ls` returns it: D13, or #1644's lane-host slice if that lands first.
From then the chart is read from the machine's `flow` and its link's ([shapes.md](shapes.md) has both tables).
This chart is the link, and it is one of two inputs to #1649's host fact (up, loaded, draining, offline). `Reach`
gives up, draining and offline. `loaded` is no state here and no link state maps to it: it is the host's own
sample, the `machine.stats` `now` that admission reads, over its limits. Both are read from shed in drive's pass
only, and stored on the host's row ([shapes.md](shapes.md), "#1649's host fact").

### An account, as drover reads it (D2a on the main host, D2b on every host)

shed owns the state and every move; drover reads `account.ls` and the refusal of a start.

```
              a limit                          the reset passes
   ready ───────────────▶ limited ─────────────────────────────────▶ ready
     │ on "auto" shed moves the lane to the next account by itself: drover logs one thread row
     │ signed out
     ▼            the card's [sign in]: account.login
 signed-out ───────────────────────────────────────────────────────▶ ready
```

| shed says | drover does |
|---|---|
| the start is accepted | the spawn event records the account the terminal reports |
| `ACCOUNT_SIGNED_OUT` | one `LOGIN_NEEDED` card for the account, not the host; its held starts go on once `account.ls` shows it ready |
| `ACCOUNT_LIMITED` | holds the start until the first reset in `account.ls`; other hosts and other accounts are not held |
| `NO_ACCOUNT` | one card: no named account of that provider, with the `shed account add` line. D2a's install gate keeps a first start from meeting it ([order.md](order.md)) |
| `ACCOUNT_SETUP` | `ACP_UNGUARDED`: the account's folder has a setup drover's hook would not guard |
| an `account` event on a live lane or thread | one row: "X hit its limit; now on Y" |
| no account was sent (another host before D2b; an agent with no `[proxy]` after it) | today's host hold, from the lane's banner |

### A move, from shed's branch to drover's deletion

```
 shed: merged on staging ─▶ on main ─▶ tagged client-v<P>.<n> ─▶ every enabled host's shed answers it
                                                                          │ (shed updates itself from main)
 drover: pin PR (the tag) ─▶ the step's PR: callers moved, old code deleted in the same PR ─▶ merged
                                    │ a host still behind
                                    ▼
                          SHED_VERSION for that host: it takes no lane until its shed has updated
```

A step never ships a switch between the old way and the new one: its PR moves every caller and deletes the old code.

## Module boundaries

```
 check, drive, spawn, hosts, cloud, update     policy: which, when, how much
        │
        ▼
 crate::shed  (Host, one file per area) ───▶ shed-client ───▶ this host's shed ══ links ══▶ each machine's shed
        ▲
 providers (hooks, guard meta, profiles) ──▶ shed-catalog      facts
 secret    (who gets which, the card)   ───▶ shed-secrets      the store, in drover's own process
 timer     (which jobs, how often)      ───▶ farrier-service ─▶ runner   (F1)
```

| module | holds after the split | may call |
|---|---|---|
| `crate::shed` | `Host`; one file per area: lanes, machines, files, stats, accounts, slots, and `release` (drover's own binary). No installs area: it can ask shed to install nothing else | shed-client |
| `hosts` | placement, needs, slots, holds, `Pass` with its budget and marks; no ssh, no probe | `crate::shed`, `host` |
| `host` | `admit`, the stray sweep (`reap`, `reap_built`), `keychain_jam`; no sampling | a `Host` as a `Runner` |
| `providers` | per provider: profile sync, the hook dialect, guard meta, doctor rows | shed-catalog |
| `secret` | names, the request card, `ls`, `rm`, events | shed-secrets |
| `timer` | the job list, PATH, the tick, the Services rows | farrier-service |
| `update`, `heavy`, `sandbox` | the decision half of each ([order.md](order.md)) | `crate::shed` |
| `cloud` | as today until #1644, which deletes it | `crate::shed` for the host, from D1 |
| `runners` | as today until `<owner>/runners` covers it, then deleted (#1644 as amended); never moved to shed | `gh`, and `cloud`'s recipe loader: no host, no shed, so no step here touches it |
| `flow::facts` (#1654) | the host fact's reader: the host's row, as the pass stored it | the view; never `crate::shed` |
| `link` | the report, the intake, the view; no dial, no frames in order | `crate::shed` |
| `outbox` | a lane's verb on another host, as one line in its worktree; the acked mark | nothing: no shed, no network |
| `check::sweep`, `roots`, `spawn` | lane state on a host's disk: worktrees, build folders, their trim and budget; the cache variables of a start. No tool install, no shared cache's size or trim | a `Host` as a `Runner` |

Rules, each with its enforcer ([adr.md](adr.md) has the `arch.toml` lines):

- Only `crate::shed` names `shed_client`; nothing writes a handshake by hand (two bans, from D0).
- No ssh connection is opened by drover (a ban from D1; cloud's first contact keeps one exception until #1644).
- drover sets no proxy variable and mints no key (a ban from D2b).
- A lane on another host calls nothing on the main host: its verbs write the outbox, and the main host pulls
  (#1646's test: every lane verb run with no fleet.toml and no state dir).
- drover asks shed to install its own binary and nothing else: `crate::shed`'s `release.rs` takes a build and no
  tool or path (the type; the `shed_client` ban keeps every other module from `install.*`).
- drover deletes lane folders only, never a shared cache (#1641's tests on `reclaim` and `clean`).
- Only drive's pass reads `machine.links` and `machine.stats` for the host fact; a kick, `waiting` and serve read
  the row it stored (`facts::host` takes the row and no `Shed`: the type; the request-path guard test).
- A process still starts only in `runner.rs`: `exec` on a machine is shed's, and farrier's control commands go through
  drover's runner (the existing ban; F1).
- serve's request path makes no call that crosses a link: it reads the pass's view and shed's cached answers, as the
  lane list does today (the request-path guard test).
