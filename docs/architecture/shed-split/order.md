# The order, the gates and the issues to file (#1567)

## When a shed API counts as released

A drover step may start when all four hold:

1. the shed issue is merged and **on shed main** (hosts update from main, never from staging);
2. a **`client-v<P>.<n>` tag** exists at or after that commit (shed's release job pushes it when a client crate changed);
3. **every enabled host's shed answers it**: shed updates itself within minutes of a release, `machine.links`
   shows each host's `hello`, and a host still behind answers `SHED_VERSION` and takes no lane;
4. drover's **pin PR** for that tag has merged.

Today none holds for any step: shed main is at protocol 2, 46 commits behind staging, with no client crate and
no tag. Step 0 is the owner's.

## The steps

| step | repo | what | needs |
|---|---|---|---|
| 0 | shed | #265 (the token proxy) merges, and X1 if it is ready; staging is promoted to main; the first `client-v*` tag is pushed; every host's shed updates itself | the owner's promote |
| 1 | shed | X1: the memory pressure level in a sample | nothing: file now, so step 0's promote can carry it |
| 1 | farrier | F1: control commands through the caller's runner | nothing: file now |
| 2 | drover | **D0** the client (it sets the first pin) | 0 |
| 3 | drover | **D9** provider facts from the catalog | D0 |
| 3 | drover | **D1** (#1575) hosts are shed machines | D0; X1 released |
| 4 | drover | **D12** host charts from shed's history | D1 |
| 4 | drover | **D11** the link reads shed | D1 |
| 4 | drover | **D2** (#1576) accounts are shed's | D1; shed #264 and #265 released (step 0 carries both) |
| 5 | shed, then drover | H1 then **D3** secrets; H2 then **D4** self-update; H3 then **D5** heavy slots | each its H; D1 |
| 6 | shed, then drover | H4 then **D6** cloud and runner VMs; H5 then **D7** the sandbox | H4 needs H1 and H2, H5 needs H3; D1 |
| 7 | shed, then drover | H6 then **D8** balances | D2 |
| any | drover | **D10** timer jobs through farrier-service | F1 |

Steps on one row, and D9 beside D1, do not depend on each other: they can be open at once. Steps 5 to 7 are
shed #259's steps 9 to 23, unchanged. Between a shed release and its drover step the old code runs; there is no
flag day and no lane restarts at any step.

## What each drover step deletes and keeps

Sizes are estimates in src lines; each PR states its own `src/ net`.

| step | deletes | keeps | net |
|---|---|---|---|
| D0 | `shed.rs`'s handshake, framing, `cli` and `adapter`; `term.rs`'s CLI word builders; the `shed help run` probe and `SHED_OLD`; the fake server in `crates/cli/tests/common/fake_shed.rs` | `version_alert`, doctor's shed row (from `hello`), the `shed.seq` mark, `term`'s typed helpers as thin calls; `ssh … shed connect` as the client's one-exchange transport, until D1 | −250 |
| D9 | in `providers/`: login, install, status, token and folder variables, efforts, models, the dsh wizard and kick; `Provider`'s fact methods | profile sync, hook dialects, guard meta, `hooks_allowed`, doctor rows, the "core names no provider" bans | −350 |
| D1 | `On`'s ssh wrap and shared connection, the session cap, the refused-session retry, `hosts/probe.rs`, `shed.rs`'s `connect` and `start_job`, `Pass`'s connection repair, `hosts.down`, the probe script's first lines; `host.rs`'s sampling; `top.rs`'s `ps` walk; doctor's `vm_stat` row | `Pass`'s budget, marks, late and unsent counts; `admit`; the sweep; tags, health rows, placement | −950 |
| D12 | `hosts::record`, `sample`, the history file, `host::groups` | `Row`, `spark`, `series`, `chart`, fed from `machine.stats`; the lanes line from the log; the group bar from the 20 largest processes | −150 |
| D11 | `link::run`, `dial`; `watch::watch`, its stamp and stdin acks; `Runner::pipe`; the service's thread per host | `report`, `intake`, `view`; one follower of shed's `events` in the service; with the service stopped, the poll as today | −200 |
| D2 | `proxy.rs`, the tunnel in `survey`, `[proxy]`, `providers::Proxy`, the reset parser in `limit.rs`, `login_hold` | the `LOGIN_NEEDED` card, the limit hold, `hosts keys` as a view of `proxy.keys` | −600 |
| D3 | `secret::file`, `path`, `Delivered`, `secrets-left`, `write_private` | names, the request card, `ls`, `rm`, events, the wip secret scan | −120 |
| D4 | `release.rs` (find, stage, the swap line, builds, the signature check, the install lines); `update.rs`'s `remote_swap` and its shell text | `step`, the marks, alerts, the swap order, profiles sync, the serve handover | −500 |
| D5 | `heavy.rs`'s lock line and `hold`'s own slot; `top`'s wait rows from files | which commands are heavy, the hook's rewrite, the limit numbers, `top`'s layout | −250 |
| D6 | `cloud.rs`'s recipes, start, stop, refresh, snapshot, pin and ssh config; bootstrap's installs; the VM side of `runners.rs` | tick, spend, budgets, idle stop, the wip save, the card; runner registration and its caps; the setup step list | −550 |
| D7 | `sandbox.rs`'s `wrap`, `file`, `resolved`, its fixed lists | whether a fleet is sandboxed; the folders and hosts a fleet needs | −100 |
| D8 | `usage::deepseek`, `parse_balance`, the key file | `warn_below`, the alert, the board's row | −60 |
| D10 | `timer.rs`'s `files`, `ctl`, `refused`, `rebootstrap`, `restart`, `job_pid`, the file compare in `drift`, the write and load in `install`; the three job templates | the job list, intervals, PATH, the tick, `due`, the Services rows | −250 |

About 4,300 lines in all.

## Issues to file

### shed

| # | title | scope |
|---|---|---|
| X1 | machine.stats: the kernel's memory pressure level | `Sample::mem_level`: normal, warn or critical from `kern.memorystatus_vm_pressure_level` on macOS, none elsewhere; in `now` and in `history`. An addition: no protocol change. About 20 lines |
| H1 to H6, cap | as shed #259's ADR lists them | not filed yet. This ADR adds nothing to their scope |

Open already: #265 (the proxy, step 0 waits for it). #295 and #296 gate no drover step.

### farrier

| # | title | scope |
|---|---|---|
| F1 | farrier-service: control commands through the caller's runner | `Env` takes an optional function that runs one control command (tool, arguments) and returns its exit and text, with a time limit; the default stays farrier's own. shed keeps working unchanged |

### drover

Already filed, with what this ADR changes in them:

| # | change |
|---|---|
| #1575 (D1) | blocked on D0 and X1 as well. "The client pinned to a tag" leaves its scope (D0). The last ssh words (`On::wrap`) move into `link/` until D11 deletes them. The `[limits]` keys of [shapes.md](shapes.md) are in its rollout |
| #1576 (D2) | none |
| #1561 | to close: #1567 replaces it |

To file. Each PR moves every caller, deletes what its row above lists, and states a negative `src/ net`:

| # | title | acceptance, beyond its row | needs |
|---|---|---|---|
| D0 | the shed client: drover on shed-client at a tag | no `Hello::ours` and no `shed_client` outside `crate::shed` (two bans); an ACP lane starts with one `run` call: no `shed run` command line, no temp file for MCP servers; an old server is `SHED_VERSION` naming the call; `spawn --dry-run` prints each call from its typed params | step 0 |
| D9 | provider facts come from shed's catalog | `providers/` has no login, install, status or variable name of any agent; config is checked against the built-in records, a lane starts by the served ones; dsh lanes start through the record's `[terminal]`; `toml` is 1.2 and `cargo deny` passes | D0 |
| D12 | host charts read shed's history | no `host-history.jsonl` is written; the Host tab's series equal `machine.stats` history for each host; one read per host a minute at most; the PR names the two visible changes (the group bar, the lanes line) | D1 |
| D11 | the link reads shed's events | no `drover hosts watch` process on any host; a `turn done` on another host kicks its issue with the report its turn's end causes, no later sample needed; a mid-turn outbox line arrives within `link_sample_secs`; `link-down` follows shed's link | D1 |
| D3 | secrets are shed's crate | as shed #259's D3 | H1; D1 |
| D4 | self-update through shed | as shed #259's D4 | H2; D1 |
| D5 | heavy slots are shed's | as shed #259's D5 | H3; D1 |
| D6 | cloud hosts and runner VMs are shed VMs | as shed #259's D6; the `launchctl bootstrap` ban loses its exception | H4; D1 |
| D7 | the sandbox is a run option | as shed #259's D7 | H5; D1 |
| D8 | balances are shed's | as shed #259's D8 | H6; D2 |
| D10 | timer jobs through farrier-service | `drover timer install` twice changes nothing the second time; a job launchd refuses is healed as today; tests run no real `launchctl`; the rollout section names the one reload | F1 |
| pin | pin: shed `client-v<P>.<n>` | one line in `Cargo.toml` and the lock; filed by an orchestrator when a tag a step needs exists | the tag |
