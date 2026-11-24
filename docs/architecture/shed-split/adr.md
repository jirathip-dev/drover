# ADR: drover adopts shed's client, machines, accounts, catalog and host APIs (#1567)

## Summary

Decision: drover keeps the policy (which lane goes where and when, spend, approvals, the guard) and stops doing host work itself. It talks to shed through shed's own client, reaches every other host over the link shed already holds, reads health, processes, accounts and usage from shed, takes provider facts from shed's catalog, and leaves the rollout of its own binary, secrets, heavy slots and the sandbox to shed. It asks shed to install nothing else, and sets no host up itself: toolchains, the shared build caches and a host's disk floor belong to the host setup recipe of shed's VM helper (shed #337), which the owner runs on the Mini and the Air through a run card. A lane's own build folders, and the cache settings its start carries (#1641), stay drover's. Renting VMs has left this plan for its own decision (#1644), and CI runners for their own repo (jirathip-dev/runners): drover's runner code is deleted once that repo covers it. Four things stay: the copy of a closed lane's transcript, the handover of drover's own web server, the sweep of stray processes in a finished lane's folder, and the file through which a lane on another host sends its commands to the main host (#1646): shed has no connection from a host back to the main host, so the main host keeps fetching that file.
Slices: #1623's small fix (a rented host that shed shows as up counts as up) has landed, `e24a53998`. Then fourteen drover steps, each after the shed release it needs and each deleting the code it replaces: the account for lanes and orchestrator threads on the main host (#1576, no key set means "auto") and the client; provider facts; the token proxy's deletion (#1604); hosts as shed machines; host charts; the host link; secrets; self-update, for drover's own binary only; heavy slots; the sandbox; balances; the timer jobs through farrier; shed's new state fields. shed's two releases of 2026-11-22 and 2026-11-23 already carry what the first four need. Two installs wait for a check: #1576 until drover's doctor shows a signed-in named account for each agent, #1604 until no lane started on drover's proxy is still alive. Until #1604 a lane on another host signs in through drover's proxy, as today.
Open questions: (1) an agent that shed's proxy does not serve (Codex today) cannot have an account on another host. With no account key set, let it run there on that host's own login, as today (recommended, and what shed's accounts decision says), or keep such lanes on the main host? (2) shed's health reading of a host has carried its free disk since shed's first release, and drover would read it there for its low-disk refusal and the Host tab's chart. Your decision names a machine as link, processes, CPU and memory: keep that one read (recommended: it is a number, and shed cleans nothing), or have drover measure free disk itself? Your four answers of 2026-11-23 are decisions here; the newest one's points are listed at the top of adr.md with where each landed.

**Status:** proposed by the architect lane, 2026-11-22; revised four times on 2026-11-23, on the owner's four
answers. Read on drover staging `7c0a28c1c`, shed main `7540e74` (tag `client-v5.1`), shed staging `94c1ede` and
farrier `d34a7bb`. It follows shed's accepted ADRs #191, #256 and #259, and what changed #259 on 2026-11-23: #307
(VMs are an optional helper's), #309 (one state per flow) and the owner's decision on what a machine is (shed
#336, #337). It reopens none. Beside it stand #1644 as amended (VMs, CI runners) and #1649's approved ADR
(the flow-table folder on branch `issue-1649`, the host fact).
The candidates one by one, how the callers read, the statecharts and the boundaries: [README.md](README.md). The
steps, gates and issues: [order.md](order.md). Keys, types and pins: [shapes.md](shapes.md).

What the first revision changed: accounts are D2a and D2b (decision 7); step 0 is done; cloud and runner VMs left
for #1644 (decision 13); a last step reads shed's state fields (decision 14); #1616's machine list is taken in;
the two folded symptoms have their step ([order.md](order.md)).

What the second changed, by the owner's six points: the outbox stays for a lane's verbs on another host
(#1646, decision 15); a rate is two reads (decision 3); `Reach::Draining` and the `reconnecting` row (decision
3); D2a's and D2b's installs each wait for a gate (decision 7); #1623's fix landed before D1 (decision 16).

What the third changed, by the owner's answer of 2026-11-23 on what a machine is:

| the owner's point | where |
|---|---|
| a shed machine is a place to run agents: link, processes, CPU, memory | decision 17; open question 2 for the one disk reading |
| toolchains, caches and host disk are shed-cloud's host setup recipe (shed #337); the Mini and the Air run it through a run card. Runner VMs were in this point; the fourth revision takes them out | decision 17; [shapes.md](shapes.md), "Who owns what on a host" |
| D4 and H2 move drover's and shed's own binaries only | decision 10; [order.md](order.md), H2's row |
| no `installs` in `crate::shed` | decision 10: its file there is `release.rs`, drover's own binary |
| a lane's build folders stay drover's (`reclaim`, `DISK_BUDGET`); the shared caches are the recipe's | decision 17 |
| #1641's cache env stays in `spawn` | decision 17; it rides the start's `env` |

Also new since the second: shed released `client-v5.1` on 2026-11-23 with its token proxy (#265), so step 3 of
[order.md](order.md) is done and one pin serves D2a, D0, D9 and D2b.

What the fourth changed, by the owner's answer of 2026-11-23 on CI runners and the host facts:

| the owner's point | where |
|---|---|
| CI runners (the Mini's native runners, the Docker slots, the runner VMs) are `jirathip-dev/runners`', not shed-cloud's | decision 13; [README.md](README.md), row 8b |
| README row 8, README's policy paragraph, decision 17's "on runner VMs" | row 8 is 8a (lane-host VMs, #1644) and 8b (CI runners); the paragraph says runner registration is deleted, not moved; decision 17 names no runner |
| `runners.rs` is deleted once `runners` covers it (#1644 as amended) | decision 13; [order.md](order.md), #1644's row; [shapes.md](shapes.md), `[ci_runners]` |
| #1649's host facts are up, loaded, draining and offline | decision 18; [shapes.md](shapes.md), "#1649's host fact" |
| loaded comes from the host's own sample (admission over `machine.stats`'s `now`), not from `Reach` | decision 18; `Reach` keeps its four values and gains none |
| #1649 reads both only in drive's pass | decision 18; [README.md](README.md), the rules |

## The question

About 5,000 lines of drover do host work that shed does or will do: an ssh wrapper with its own connection sharing, host
sampling from `ps`, `lsof` and `vm_stat`, a shed client written by hand, launchd and systemd files, a token proxy,
an installer in shell text, VM recipes, a secrets store, a slot line on file locks, a sandbox wrap. shed has
designed or built a home for each. The owner decided what moves (#1567 and its amendment). What is left to settle
is drover's side: for each piece move, delete or keep; which shed API each move waits for; and an order in which
every step lands on a released shed and leaves drover working.

## What the code says today

Found while reading, and each one shapes the decision:

- **Two releases are out.** `client-v5.0` (`fbd81d9`, 2026-11-22), at protocol 5: the client and the catalog
  (#199 to #203), accounts 1 to 4 (#261 to #264), the machine, file and process APIs (#266), the tag job (#267),
  the dsh record (#258). `client-v5.1` (`7540e74`, 2026-11-23), still protocol 5: the token proxy (#265), #295
  and #296. Half an hour after the second, the main host's `shed` was still the first build.
- **drover sets hosts up today.** `drover hosts bootstrap` (`cloud::bootstrap`) installs git, gh, just, Rust,
  node, each agent's CLI, shed and drover's own binary on another host. `release.rs` and `update.rs` move
  drover's own binary and nothing else. Every lane has a build folder of its own, which the tick trims.
- **drover still speaks to shed the old way.** It pins `shed-proto` at `9b88eb5` (protocol 2), starts ACP lanes
  and orchestrator threads through the `shed` CLI (`term::Acp`'s words), reads `shed help run` to learn whether
  a host takes `--meta`, and reaches another host's shed with `ssh … shed connect` per call.
- **drover already lists its hosts in shed.** Since #1616 every drive pass adds each `[hosts]` machine with
  `shed machine add`, under its `machine` value as the name, and removes none.
- **shed's plan moved twice on 2026-11-23.** #307: VMs are rented by an optional helper, `shed-cloud`, never by
  shed's server or by drover; #259's `vm.*` is gone. #309: each terminal, machine, link, account and key gets
  one `flow` state; `machine.links` and the old state fields are dropped once drover reads the new ones.
- **The catalog forces a version move.** `shed-catalog` pins `toml` at 1.2.1; drover is on 0.8, and `deny.toml`
  forbids two versions of a crate. shed also pins `serde`, `serde_json` and `thiserror` exactly, at the versions
  drover's lock happens to have today.
- **farrier-service starts `launchctl` and `systemctl` itself**, with no time limit. drover starts every process
  in `runner.rs`, cut at a limit, and its tests replace that runner.
- **The link's watcher runs on this host too.** Only its carriage to another host is ssh; the frames, their
  order check and the beat serve both. So "move the transport" is not a cut along a file.
- **shed samples the percent of memory not available; drover's `HOST_BUSY` gate reads the kernel's pressure
  level** (normal, warn, critical). They are different signals.
- **hermes needs no record:** #1570 removes that harness. dsh already has one.
- **shed's links run one way.** The main host's shed holds a link to each machine; a machine's shed holds none
  back, and D1 adds none. Nothing a lane runs on another host can call the main host: what it has is its
  worktree's outbox, which the main host pulls.
- **shed's numbers are counters and averages, not rates.** `Proc::cpu_pct` is a process's CPU time over its
  whole life; `history` is one sample a minute; `swap_out_pages` counts from boot.

## Options

**How drover reaches a machine after the move**

| | option | verdict |
|---|---|---|
| A | Keep `On` as an ssh wrapper beside shed's link, for calls that are "just a command" | two routes to one host, two answers to "is it up" |
| B | Every call site calls shed's client | 544 places take a `&dyn Runner` and 79 build an `On`; each would grow a branch for this host or another |
| C | **One module, `crate::shed`. A `Host` is this host or a machine, and is still a `Runner`** | chosen: `ok(&host, "git", …)` reads as before; a remote one is `exec` |

**Where the client lands**

| | option | verdict |
|---|---|---|
| D | Inside D1 (#1575), as filed: "the client pinned to a tag" | D1 already moves transport, probes, sampling and files |
| E | **Its own step first, D0, with the route to each host unchanged** | chosen: it changes how drover speaks, not whom it reaches, so a fault in it is a fault in the client alone |

**The link's carriage**

| | option | verdict |
|---|---|---|
| F | Leave the watcher on ssh | drover keeps opening ssh connections after everything else stopped |
| G | Ask shed for a piped `exec` and run the watcher through it | a new shed stream that carries what shed's `events` already says |
| H | **Follow shed's `events`; fetch the same one-call report when a lane's state changes** | chosen: no watcher process, no frames in order, no dial loop, on this host or another |

**launchd and systemd**

| | option | verdict |
|---|---|---|
| I | Keep `timer.rs`'s own files and `launchctl` words | a third copy of what shed and farrier have |
| J | farrier-service as it is | `launchctl` would run outside drover's runner: no limit, and every timer test would need a fake tool on PATH |
| K | **farrier-service, after it takes the caller's runner (F1)** | chosen |

**The memory signal in admission**

| | option | verdict |
|---|---|---|
| L | Gate on shed's percent with a new limit | changes when `HOST_BUSY` refuses, with no data to pick the number |
| M | Read the level with one `exec` a pass | sampling stays in drover |
| N | **shed adds the level to its sample (X1)** | chosen: about 20 lines in shed, the gate does not change |

X1 carries a second addition, a process's CPU time, for option T below.

**A rate from shed's numbers**

| | option | verdict |
|---|---|---|
| S | Read `cpu_pct` and the newest `history` sample as they come | a lane that was busy for an hour and now idles still reads busy; a start now is gated by a sample up to a minute old |
| T | **The difference of two reads: CPU time between two `terminal.procs`, pages between successive `now`** | the owner's decision. X1 adds the CPU time itself (`cpu_ms`): one percent of a two-hour life is 72 CPU-seconds, more than the whole gap between the reads |

**A lane's verb on another host, after D1**

| | option | verdict |
|---|---|---|
| U | `exec` on the main host through shed, as #1646 was filed | there is no link from a host back to the main host |
| V | **The outbox, pulled by the link's report** | the owner's decision: one route, before and after D1 |

**Accounts: one step or two**

| | option | verdict |
|---|---|---|
| Q | One step after D1 and shed's proxy, as shed #256's D2 row has it | the account key, which needs neither, waits for both |
| R | **Two: D2a (#1576) the account on the main host, D2b (#1604) the proxy's deletion** | the owner's decision: D2a needs only shed's release of 2026-11-22; D2b needs shed's proxy, released a day later |

**Who moves the pin**

| | option | verdict |
|---|---|---|
| O | Each step's PR moves it to the tag it needs | two open steps edit the same `Cargo.toml` line and `Cargo.lock` |
| P | **A pin PR of its own for each shed tag a step needs** | chosen: one line, filed when the tag exists; a step's PR never edits the pin |

## The decision

1. **One door.** `crate::shed` is the only module that names shed's client. It gives a `Host`: this host, or a
   machine by its `[hosts]` name. A `Host` runs commands (`exec`), reads and writes files, and answers for its
   link, health and processes. It implements `Runner`, so placement, clean, doctor and git calls keep their shape.
2. **The client first (D0).** `shed.rs`'s handshake and framing, and `term.rs`'s CLI words, are deleted for
   `shed-client`. Until D1 another host is still reached by `ssh … shed connect`, now as the client's
   one-exchange transport. ACP lanes and orchestrator threads start with `run` over the socket: shed builds the
   command from `model` and
   `effort`, and `meta` carries the guard. `SHED_OLD` and the `shed help run` probe go: an old server
   answers `SHED_VERSION`, naming the call. The 424-line fake server in drover's tests goes for the client's own.
3. **Hosts are shed machines (D1).** drover registers each `[hosts]` entry that has a `machine`, and never
   removes one (#1616 does this through the CLI today; D1 moves it to `machine.add`). A host is offline from
   the link's `down_since`; a link that is `reconnecting` is not up, so the pass sends that host nothing. A
   host that drains (`Reach::Draining`, for #1644 and #1649) takes no new lane; every call still goes there.
   Admission reads `machine.stats`'s `now`, and its swap rate is the pages between this pass's `now` and the
   last pass's, never the per-minute `history`. `top` and lane-hog read `terminal.procs` twice,
   `cpu_sample_secs` apart, and take the CPU time between the two reads. `Pass` keeps its budget, its marks and
   "sent nothing to a host that is down"; the probe, the repair of shared connections and the session cap go.
   A batch exists check is one `exec`.
4. **Sampling moves, the sweep stays.** `host.rs` loses `sample`, `pace`, `window`, `lane_cpu`, `top`, `groups`,
   `held` and `df`. It keeps `admit` (the rules), and `reap`, `reap_built` and `keychain_jam`, which now run
   through a `Host`: they look for processes by folder after a lane is gone, outside any terminal's tree, which
   shed's per-terminal view cannot see. A live lane's children are ended by pid from `terminal.procs`; a lane
   itself by shed's `kill`.
5. **History is shed's (D12).** The host-history file and its writer are deleted. The Host tab's series come from
   `machine.stats` with `since`, cached a minute a host; its lanes line is counted from drover's own log.
6. **The link reads shed (D11).** A lane's state change in shed's `events` makes its host due; drover then asks
   that host for the report it already knows how to give, once (`drover hosts report`, by `exec` there, in
   process here), and every `link_sample_secs` anyway. The intake and the view do not change. The kept-open
   watcher, the frame order check, the beat, the dial loop and `Runner::pipe` are deleted. A link is down when
   shed's link is.
7. **Accounts are shed's, in two steps**, as shed #256 says and the owner split it.
   **D2a (#1576)**: an `account` key on a fleet, a role and `[fleet.<n>.role.orchestrator]`: a name or
   `"auto"`, and no key is `"auto"`. Two callers pass it, `spawn` and `orchestrator.rs`'s `start()`, both through
   `term::Acp`. On the main host `LOGIN_NEEDED` and the limit hold read `account.ls`, and the card's sign-in is
   `account.login`. It needs the `shed-proto` pin at protocol 5 (`client-v5.1`, the newest tag) and nothing else of this ADR: not D0, not D1,
   not shed #265. A lane on another host carries no account yet: it signs in through drover's proxy and its
   limit or sign-out holds that host, as today. **Its install is gated**: doctor shows a signed-in named
   account for every agent a role or an orchestrator starts, so no thread's start meets `NO_ACCOUNT`. The rows
   ship with the pin PR, one build earlier.
   **D2b (#1604)**: after D2a, on shed #265, which `client-v5.1` carries. An ACP lane on another host starts with one `run`
   at the main host's shed, with `machine` and `account`, so shed's proxy gives it a key and it never holds a
   token. `proxy.rs`, the tunnel, `[proxy]` and the per-lane keys are deleted. It does not wait for D1: the
   machine is in shed's list already (#1616), and until D1 that lane's reads, prompts and kills keep today's
   route to the same terminal. **Its install waits** until no lane started on drover's proxy is alive: such a
   lane holds a key only drover's proxy takes.
   **An agent the proxy does not serve.** Only a record with a `[proxy]` table gets a key; today that is Claude
   Code. Another ACP agent on another host, with no `account` key set, is sent no account: it runs on that
   host's login, and its limit or sign-out holds that host, as today and as shed #256 says (open question 1).
   With a key set, the start is refused by shed's `ACCOUNT_LOCAL_ONLY`, naming the account.
8. **Provider facts are the catalog's (D9).** Login, status, install, the token and folder variables, models,
   efforts and limit lines are read from the record. The install line is text to drover: a doctor row's fix,
   and bootstrap's agent step for as long as #1644 leaves bootstrap one (decision 17). `providers/` keeps what drover does to an
   agent: profile sync, the hook dialect, guard meta, doctor rows. The dsh wizard watcher is deleted: the record
   has it. drover checks its config against the built-in records and starts lanes by what the host's shed serves.
   A fact drover needs and the record lacks is added to the record, never kept in drover.
9. **Jobs are farrier's (D10).** `timer.rs` keeps which jobs exist, their arguments, intervals and PATH, the tick
   and the Services rows. The plist and unit text, load, unload, restart, the pid and the refused check become
   `farrier_service::Service`. The old `dev.herdsman.*` labels are its `supersedes`.
10. **Its own binary, secrets, slots, the sandbox and balances (D4, D3, D5, D7, D8)** move as shed #259 lays
    out, D4 and H2 narrowed by the owner (2026-11-23): shed's `install.*` moves a release of shed's own binary
    and of drover's, and nothing else. D4 is drover's binary at drover's path on each host: find, stage, swap
    and back, in `crate::shed`'s `release.rs`, whose functions take a build and no tool name or path. There is
    no `installs` area in `crate::shed`, and drover calls no `machine.setup`. drover keeps the decision half of
    each step, listed in [order.md](order.md).
11. **Four things stay in drover.** The transcript copy (a lane drover removed is drover's record; shed keeps
    nothing after `rm`). The serve handover (drover's listeners, drover's next build; shed's handover serves only
    shed). The stray sweep (item 4). The outbox (item 15).
12. **Order and gates.** A step starts when its shed API is on shed main under a `client-v*` tag and every
    enabled host's shed answers it. Each step moves every caller and deletes the old code in one PR. Two
    installs have a gate of their own, D2a's and D2b's ([order.md](order.md)).
13. **Lane-host VMs and CI runners are not steps here.** shed #307 replaced #259's VM plan, and drover's side is
    #1644's own ADR: drover publishes demand and reads which machines are pool VMs; it starts and stops none.
    There is no D6. What this ADR owes #1644 is D1: a VM is a shed machine like any other host.
    CI runners are `jirathip-dev/runners`' (the owner, 2026-11-23): the Mini's native runners, the Docker slots,
    the runner VMs and their image with its toolchains and sccache. They are not shed-cloud's, and a runner is
    no shed machine. `runners.rs`, the `drover runners` verbs, `[ci_runners]` and the recipe's `runner_*` lines
    are deleted once `runners` covers them (#1644 as amended); nothing of them moves to shed. No step here
    touches them: `runners.rs` reaches no host through `On`, only `gh` and `cloud.rs`'s recipe loader
    (`cloud::load`, `cloud::run`), so D1 leaves it as it is and it goes before `cloud.rs` or with it.
14. **shed's state fields (D13).** When shed #309's take-overs are released, `crate::shed` reads `machine.ls`
    and each `flow` in place of `machine.links` and the old state fields. Only then may shed drop the old
    ones (shed #323). What drover does in each state is its own table's (#1649), not this step's.
15. **A lane's verbs on another host go through the outbox (#1646), before and after D1.** The verb writes one
    line in its worktree; the main host pulls it with the link's report (the watcher until D11, one `exec` of
    `drover hosts report` from D11), runs it as that lane and acks it in the next report. #1646's "then through
    shed exec" does not hold; its rule (a lane reads no drover state where it runs), its test and its wider
    list of verbs do, and it waits for no step here. D1 and D11 keep the outbox, its acked mark and the intake.
16. **#1623's fix did not wait for D1.** It landed as `e24a53998`, in `hosts::survey`: a cloud host that
    shed's machine list shows `up` is up, whatever its last `cloud-*` event says, and the pass logs
    `cloud-up`. D1 then deletes that rule and the probe.
17. **Neither shed nor drover sets a host up or keeps its disk.** A shed machine is a place to run agents: its
    link, its processes, their CPU and memory. Toolchains, the shared build caches (sccache's folder, pnpm's
    store) and the disk floor are shed-cloud's host setup recipe (shed #337): in the pool's image, and on the
    Mini and the Air by `shed-cloud setup --on <machine>` from a run card. A CI runner is not among them: its
    image and its caches are `jirathip-dev/runners`' (decision 13). So no step here
    moves bootstrap's tool steps to shed: they run as today until #1644, which drops them for the recipe.
    What stays drover's is lane state: a lane's worktree and its build folders (the tick's trim, `reclaim`,
    `clean`, `DISK_BUDGET`), the `DISK_LOW` refusal, and the cache variables `spawn` sets on a lane's start
    (#1641), which ride `RunParams.env` from D0. drover never sizes, trims or deletes a shared cache; it
    reports its size. [shapes.md](shapes.md) has the table.
18. **#1649's host fact has two inputs, and `Reach` is one of them.** Its values are up, loaded, draining and
    offline. `Reach` is the link: it gives up, draining and offline. `loaded` is the host's own sample: the
    `machine.stats` `now` that the pass already reads for admission, taken on that machine by its own shed,
    over its limits. No link state, no slow or late call and no load on the main host makes another host
    `loaded`, and `Reach` gains no fifth value. Both are read from shed in drive's pass only, once per host,
    and stored on the host's row; `facts::host` (#1654), a kick, `waiting` and serve read that row and call no
    shed. A sample that does not come leaves the row's last one: late is not absent, and only `Reach` makes a
    host offline. D1 rewrites that reader's two inputs and no row of #1649's table
    ([shapes.md](shapes.md), "#1649's host fact").

What this changes of earlier drover decisions, said plainly:

- **#1309 (drive-push): "each boat sends every lane change over the main host's own ssh connection, kept open".**
  The connection kept open is shed's link. A change in a worktree that no state change follows (a lane's own
  event written mid-turn) arrives at the next sample, at most `link_sample_secs` later, not within half a second.
  A `turn done`, an ask and a verdict are each followed by the turn's end, so they arrive as fast as now.
- **`docs/hosts.md`: drover reaches a host "as `ssh <name> …` for everything".** After D1 it opens no ssh
  connection; after #1644 it writes no ssh config.
- **#923 and #707** are reversed by shed #256 and #259; D2a and D3 carry that out.
- **#1487 (one connection per host per pass)** is moot: there is one socket, this host's.

## Consequences

- drover loses about 3,700 src lines over the fourteen steps ([order.md](order.md)), and about 850 more outside
  them: `cloud.rs` under #1644, `runners.rs` once `jirathip-dev/runners` covers it. Every step's PR but D2a's and D13's states a negative `src/ net`, and no step needs the cap raised.
- **With D2a every ACP start on the main host needs a named shed account.** `auto` draws on named accounts only, so
  the owner names the main host's logins in shed first, and D2a's build is installed only when doctor's account
  rows are all `✓`. `NO_ACCOUNT` stays one card with the `shed account add` line, for an account removed later.
- **D2b's install empties the other hosts of ACP lanes first.** `hosts off` for each, their lanes finish, then
  the build goes in. A lane that will not end is killed and starts again on shed's proxy.
- **A lane's verb on another host is as fast as the report.** One that ends the turn arrives with the turn's
  end; one mid-turn (a triage close, #1646) within `link_sample_secs`. It never gets an answer in the same call.
- **lane-hog and `top` wait `cpu_sample_secs` between their two reads.** lane-hog only on a host already over its
  load limit; `cpu_pct` is shown nowhere as a rate.
- **Admission keeps a few numbers per host between passes:** the last `now`'s swap count and time, with the last
  five rates, in `hosts.json`. The `swapcount*` and `swapouts` files go.
- **Between D2a and D2b accounts stop at the main host.** A lane on another host runs on `[proxy]`'s one
  token, whatever `account` says, and two proxies exist side by side: drover's and shed's.
- **Between D2b and D1 a lane on another host has two routes:** its start over shed's link, the rest over ssh.
  A link that is down refuses the start (`NO_PROXY`, shown as `NO_PROXY_TOKEN`) while drover's own probe may
  still say up. D1 ends that: one route, one answer.
- **The host fact is as fresh as the last pass.** Between passes nothing reads shed for it: a kick and the
  page see the row the pass stored, with the sample's time on it. One `machine.stats` a host a pass serves
  admission and the fact; X1's memory level is in both.
- **One answer to "is the host up":** shed's link. `hosts.down`, the probe and the `host-key-changed` text match
  go; a changed host key is the link's `error` line on the offline alert.
- **A host needs a current shed and nothing else of drover's transport.** The `drover` binary stays on each host
  for the lane hook, `drover hosts report` and profiles sync, so D4 still rolls it out.
- **A new host is set up by three hands, none of them an installer in drover.** The recipe puts the tools and
  the caches there (a run card, on a host that is no VM); shed puts shed there; drover then adds its own binary
  (D4), the clones, the sign-in panes and profiles sync. After #1644 a missing tool is a doctor row naming the
  recipe's command, never an install drover runs.
- **`DISK_LOW` can only free lane folders.** A host under its floor because a cache grew is the recipe's to
  bring back: drover's refusal names `drover clean` and the recipe's command.
- **This host's shed is in the path to every host.** While it is down drover reaches none of them. shed's own
  job keeps it alive; from D1 drover no longer kicks that job (`start_job` and `connect --autostart` go) and
  doctor's row names shed's own start line.
- **Remote session names gain the machine:** `box/lane-256-impl` at this host's shed is the terminal that was
  `lane-256-impl` at box's. Live lanes are found again by their spawn event's `machine`; none restarts.
- **Tests change shape.** A remote host in a test is the client's fake server answering `exec` and `file.*`, not
  a fake `ssh` on PATH. D0 and D1 carry most of that work.
- **drover's versions of `serde`, `serde_json`, `thiserror` and `toml` follow shed's tag.** The monthly deps lane
  cannot move them past shed's exact pins. D9 moves drover to `toml` 1.2 in the same PR.
- **The first `drover timer install` after D10 rewrites every job file**, since farrier renders them: one reload
  of each job, named in that PR's rollout section.
- **Seven `[limits]` keys are deleted, one is renamed and one is new:** four of ssh's five, three of the link's
  six, and `cpu_sample_secs` ([shapes.md](shapes.md)).
- **Between a shed release and its drover step the old code runs.** Nothing in shed takes over by itself.

## The new crates

Each is a new direct dependency, so each was the owner's decision: all four are approved (2026-11-23). All are
the owner's own repos, pinned by git tag or revision; `deny.toml`'s `allow-git` gains farrier.

| crate | step | replaces | brings into the lock |
|---|---|---|---|
| `shed-client` | D0 | `shed.rs`'s client, the test fake; `shed-proto`, a direct dependency since #298 and moved to `client-v5.1` for D2a, stops being one (the client re-exports it) | nothing new (`thiserror` is there) |
| `shed-catalog` | D9 | the facts in `providers/` | `toml` 1.2 in place of 0.8 |
| `shed-secrets` | D3 | the store half of `secret.rs` | `security-framework` and its `core-foundation` crates, macOS only |
| `farrier-service` | D10 | the job files and control words in `timer.rs` | nothing: it has no dependencies |

Not taken: `farrier-update` (shed #259 made `install.*` the one path for shed's and drover's own binaries) and
`shed-accounts`, `shed-hosts` (server side only). `docs/serve.md`'s allowed list gains each crate in the PR that adds it.

## Hot files and `.drover/arch.toml`

This work converts no central list. It is laid out so that fourteen PRs share as little as they can:

- **`crate::shed` is a folder, one file per area** (`crates/core/src/shed/*.rs`: lanes, machines, files, stats,
  accounts, slots, and `release.rs` for drover's own binary). A step adds its own file; `hosts.rs`, at its
  2,000-line limit today, only shrinks.
- **The pin moves in its own PR**, so no two steps edit the same `Cargo.toml` line or the lock.
- **Docs stay one file per area:** `docs/hosts.md` (D1), `docs/accounts.md` (D2a, D2b), `docs/providers.md` (D9),
  `docs/service.md` (D10, D11), `docs/config.md` for the keys.

The lines, each added by the PR named. D0:

```toml
[[ban]]
pattern = "Hello::ours"
paths = ["crates/*/src/**"]
reason = "#1567: shake hands through shed_client; no hand-written client"

[[ban]]
pattern = "shed_client"
paths = ["crates/*/src/**"]
except = ["crates/core/src/shed/**"]
reason = "#1567: crate::shed is the one door to shed; callers hold a Host"
```

D1:

```toml
[[ban]]
pattern = "ControlMaster"
paths = ["crates/*/src/**"]
reason = "#1567: shed holds the link to every machine; drover opens no ssh connection"

[[ban]]
pattern = "vm_stat"
paths = ["crates/*/src/**"]
reason = "#1567: a host's health is machine.stats, read in crates/core/src/shed/"
```

D4 edits the `releases/download` ban: its `except` goes with `crates/core/src/release.rs`, and its reason becomes
"#1567: shed's install.find reads drover's release". D10:

```toml
[[ban]]
pattern = "launchctl bootstrap"
paths = ["crates/*/src/**"]
except = ["crates/core/src/cloud.rs"]
reason = "#1567: a job is loaded by farrier-service, and its fix line is Service::commands; cloud.rs until #1644 deletes it"
```

#1644 drops that `except`. D2b:

```toml
[[ban]]
pattern = "base_url_env"
paths = ["crates/*/src/**"]
reason = "#1567: shed's proxy gives a run on a machine its key and URL; drover sets no proxy variable"
```

D2a, D11 and D13 add no line: the `outbox.jsonl` and `lane_view` bans keep their paths.
