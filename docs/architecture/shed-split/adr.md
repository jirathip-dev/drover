# ADR: drover adopts shed's client, machines, accounts, catalog and host APIs (#1567)

## Summary

Decision: drover keeps the policy (which lane goes where and when, spend, approvals, the guard) and stops doing host work itself. It talks to shed through shed's own client, reaches every other host over the link shed already holds, reads health, processes, accounts and usage from shed, takes provider facts from shed's catalog, and leaves installs, VMs, secrets, heavy slots and the sandbox to shed. Three things stay: the copy of a closed lane's transcript, the handover of drover's own web server, and the sweep of stray processes in a finished lane's folder.
Slices: thirteen drover steps, each after the shed release it needs and each deleting the code it replaces: the client; provider facts; hosts as shed machines; host charts; the host link; accounts; secrets; self-update; heavy slots; cloud and runner VMs; the sandbox; balances; and the timer jobs through farrier. Nothing can start until shed's staging branch is promoted to main: main is 46 commits behind and has no client crate.
Open questions: (1) four new direct crates, each yours to approve: shed-client, shed-catalog, shed-secrets, farrier-service (recommended: all four). (2) the host link stops keeping its own connection open and reads shed's events instead, which changes one decision of #1309 (recommended). (3) promote shed's staging to main once shed #265 has merged, so one release carries everything the first steps need (recommended)?

**Status:** proposed by the architect lane, 2026-11-22. Read on drover staging `d989c5beb`, shed staging `3f60200`,
shed main `56ae25a` and farrier `d34a7bb`. It follows shed's accepted ADRs #191, #256 and #259 and reopens none.
The candidates one by one, how the callers read, the statecharts and the boundaries: [README.md](README.md). The
steps, gates and issues: [order.md](order.md). Keys, types and pins: [shapes.md](shapes.md).

## The question

About 5,000 lines of drover do host work that shed does or will do: an ssh wrapper with its own connection sharing, host
sampling from `ps`, `lsof` and `vm_stat`, a shed client written by hand, launchd and systemd files, a token proxy,
an installer in shell text, VM recipes, a secrets store, a slot line on file locks, a sandbox wrap. shed has
designed or built a home for each. The owner decided what moves (#1567 and its amendment). What is left to settle
is drover's side: for each piece move, delete or keep; which shed API each move waits for; and an order in which
every step lands on a released shed and leaves drover working.

## What the code says today

Found while reading, and each one shapes the decision:

- **Nothing is released.** shed main is at protocol 2 and has no `shed-client` crate. shed staging is 46 commits
  ahead at protocol 5: the client and the catalog (#199 to #203), accounts 1 to 4 (#261 to #264), the machine,
  file and process APIs (#266), the tag job (#267), the dsh record (#258). The token proxy (#265) is open. No
  `client-v*` tag exists, and every host's shed follows main.
- **drover still speaks to shed the old way.** It pins `shed-proto` at `9b88eb5` (on shed main),
  starts ACP lanes through the `shed` CLI, reads `shed help run` to learn whether a host takes `--meta`, and
  reaches another host's shed with `ssh … shed connect` per call.
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
   one-exchange transport. ACP lanes start with `run` over the socket: shed builds the command from `model` and
   `effort`, and `meta` carries the guard. `SHED_OLD` and the `shed help run` probe go: an old server
   answers `SHED_VERSION`, naming the call. The 424-line fake server in drover's tests goes for the client's own.
3. **Hosts are shed machines (D1).** drover registers each `[hosts]` entry that has a `machine`, and never
   removes one. A host is offline from the link's `down_since`. Admission reads `machine.stats`; `top` and
   lane-hog read `terminal.procs`. `Pass` keeps its budget, its marks and "sent nothing to a host that is down";
   the probe, the repair of shared connections and the session cap go. A batch exists check is one `exec`.
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
7. **Accounts are shed's (D2, #1576)**, as shed #256 says: the `account` key, `LOGIN_NEEDED` and the limit hold
   from `account.ls`, the card's sign-in through `account.login`, `proxy.rs` and its tunnel deleted.
8. **Provider facts are the catalog's (D9).** Login, status, install, the token and folder variables, models,
   efforts and limit lines are read from the record. `providers/` keeps what drover does to an
   agent: profile sync, the hook dialect, guard meta, doctor rows. The dsh wizard watcher is deleted: the record
   has it. drover checks its config against the built-in records and starts lanes by what the host's shed serves.
   A fact drover needs and the record lacks is added to the record, never kept in drover.
9. **Jobs are farrier's (D10).** `timer.rs` keeps which jobs exist, their arguments, intervals and PATH, the tick
   and the Services rows. The plist and unit text, load, unload, restart, the pid and the refused check become
   `farrier_service::Service`. The old `dev.herdsman.*` labels are its `supersedes`.
10. **Installs, VMs, secrets, slots, the sandbox and balances (D3 to D8)** move as shed #259 lays out. drover
    keeps the decision half of each, listed in [order.md](order.md).
11. **Three things stay in drover.** The transcript copy (a lane drover removed is drover's record; shed keeps
    nothing after `rm`). The serve handover (drover's listeners, drover's next build; shed's handover serves only
    shed). The stray sweep (item 4).
12. **Order and gates.** A step starts when its shed API is on shed main under a `client-v*` tag and every
    enabled host's shed answers it. Each step moves every caller and deletes the old code in one PR.

What this changes of earlier drover decisions, said plainly:

- **#1309 (drive-push): "each boat sends every lane change over the main host's own ssh connection, kept open".**
  The connection kept open is shed's link. A change in a worktree that no state change follows (a lane's own
  event written mid-turn) arrives at the next sample, at most `link_sample_secs` later, not within half a second.
  A `turn done`, an ask and a verdict are each followed by the turn's end, so they arrive as fast as now.
- **`docs/hosts.md`: drover reaches a host "as `ssh <name> …` for everything".** After D1 it opens no ssh
  connection; after D6 it writes no ssh config.
- **#923 and #707** are reversed by shed #256 and #259; D2 and D3 carry that out.
- **#1487 (one connection per host per pass)** is moot: there is one socket, this host's.

## Consequences

- drover loses about 4,300 src lines over the thirteen steps ([order.md](order.md)). Every step's PR states a negative `src/ net`, and
  no step needs the cap raised.
- **One answer to "is the host up":** shed's link. `hosts.down`, the probe and the `host-key-changed` text match
  go; a changed host key is the link's `error` line on the offline alert.
- **A host needs a current shed and nothing else of drover's transport.** The `drover` binary stays on each host
  for the lane hook, `drover hosts report` and profiles sync, so D4 still rolls it out.
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
- **Seven `[limits]` keys are deleted and one is renamed:** four of ssh's five, three of the link's six
  ([shapes.md](shapes.md)).
- **Between a shed release and its drover step the old code runs.** Nothing in shed takes over by itself.

## The new crates

Each is a new direct dependency, so each is the owner's decision (open question 1). All are the owner's own
repos, pinned by git tag or revision; `deny.toml`'s `allow-git` gains farrier.

| crate | step | replaces | brings into the lock |
|---|---|---|---|
| `shed-client` | D0 | `shed.rs`'s client, the test fake; `shed-proto` stops being a direct dependency (the client re-exports it) | nothing new (`thiserror` is there) |
| `shed-catalog` | D9 | the facts in `providers/` | `toml` 1.2 in place of 0.8 |
| `shed-secrets` | D3 | the store half of `secret.rs` | `security-framework` and its `core-foundation` crates, macOS only |
| `farrier-service` | D10 | the job files and control words in `timer.rs` | nothing: it has no dependencies |

Not taken: `farrier-update` (shed #259 made `install.*` the one install path) and `shed-accounts`, `shed-hosts`
(server side only). `docs/serve.md`'s allowed list gains each crate in the PR that adds it.

## Hot files and `.drover/arch.toml`

This work converts no central list. It is laid out so that thirteen PRs share as little as they can:

- **`crate::shed` is a folder, one file per area** (`crates/core/src/shed/*.rs`: lanes, machines, files, stats,
  accounts, installs, slots, VMs). A step adds its own file; `hosts.rs`, at its 2,000-line limit today, only shrinks.
- **The pin moves in its own PR**, so no two steps edit the same `Cargo.toml` line or the lock.
- **Docs stay one file per area:** `docs/hosts.md` (D1, D6), `docs/accounts.md` (D2), `docs/providers.md` (D9),
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

D4 edits the `releases/download` ban: its `except` goes with `release.rs`, and its reason becomes
"#1567: shed's install.find reads a release". D10:

```toml
[[ban]]
pattern = "launchctl bootstrap"
paths = ["crates/*/src/**"]
except = ["crates/core/src/cloud.rs"]
reason = "#1567: a job is loaded by farrier-service, and its fix line is Service::commands; cloud.rs until D6 moves bootstrap"
```

D6 drops that `except`. D11 adds no line: the `outbox.jsonl` and `lane_view` bans keep their paths.
