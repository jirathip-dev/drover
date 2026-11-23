# The order, the gates and the issues to file (#1567)

## When a shed API counts as released

A drover step may start when all four hold:

1. the shed issue is merged and **on shed main** (hosts update from main, never from staging);
2. a **`client-v<P>.<n>` tag** exists at or after that commit (shed's release job pushes it when a client crate changed);
3. **every enabled host's shed answers it**: shed updates itself within minutes of a release, `machine.links`
   shows each host's `hello`, and a host still behind answers `SHED_VERSION` and takes no lane;
4. drover's **pin PR** for that tag has merged.

Today, 2026-11-23: shed main is `7540e74`, tagged `client-v5.1`, protocol 5. `client-v5.0` (`fbd81d9`) carried
the client, the catalog, accounts 1 to 4, the machine, file and process APIs and the tag job; `client-v5.1` adds
the token proxy (#265), #295 and #296. Conditions 1 and 2 hold for D2a, D0, D9 and D2b. 3 is checked for each
host when a step starts: half an hour after the second release the main host's `shed` was still `fbd81d9`, which
serves D2a, D0 and D9 and not D2b. 4 is the first pin PR. X1 is in neither release: D1 waits for the next.

## The steps

| step | repo | what | needs |
|---|---|---|---|
| 0 | shed | done 2026-11-22: staging `3f60200` promoted to main, tagged `client-v5.0` | |
| now | drover | **#1623**, the interim fix: a cloud host shed lists `up` is up | nothing; it is unblocked from #1567 |
| 1 | drover | **pin**: `shed-proto` at `client-v5.1`, and doctor's account rows | 0 |
| 1 | shed | X1: the memory pressure level in a sample, a process's CPU time | nothing: file now, for the next release |
| 1 | farrier | F1: control commands through the caller's runner | nothing: file now |
| 2 | drover | **D2a** (#1576) the account on the main host, for lanes and orchestrator threads | the pin; to install: every account row `✓` (the account gate, below) |
| 2 | drover | **D0** the client (`shed-client` replaces `shed-proto` at the same tag) | the pin |
| 3 | shed | done 2026-11-23: staging `2009a8b` promoted to main, tagged `client-v5.1`: #265, #295, #296. Still open: every host's shed on that build | |
| 3 | drover | **D9** provider facts from the catalog | D0 |
| 4 | drover | **D2b** (#1604) the token proxy is shed's | D2a; every host's shed on step 3's build; to install: no live key of drover's proxy (the proxy gate, below) |
| 4 | drover | **D1** (#1575) hosts are shed machines | D0; X1 released, in shed's next release, and its pin |
| 5 | drover | **D12** host charts from shed's history | D1 |
| 5 | drover | **D11** the link reads shed | D1 |
| 6 | shed, then drover | H1 then **D3** secrets; H2 then **D4** self-update, both for shed's and drover's own binaries only; H3 then **D5** heavy slots | each its H; D1 |
| 7 | shed, then drover | H5 then **D7** the sandbox | H5 needs H3; D1 |
| 8 | shed, then drover | H6 then **D8** balances | D2a |
| any | drover | **D10** timer jobs through farrier-service | F1 |
| last | shed, then drover | shed #318 to #322 (each flow's take-over) then **D13** shed's state fields; then shed #323 drops the old ones | D1, D2b |

Steps on one row, and D9 beside the rows under it, do not depend on each other: they can be open at once. D0
and D2a both touch the one start call (`term::Acp`): the second to land carries the other's change, so after
both the account is `RunParams.account`. D2b and D1 both change how a lane on another host starts: D2b moves the
start to shed's link, D1 everything else; either order works. Steps 6 to 8 are shed #259's steps, without its
VM half and with H2 narrowed to shed's and drover's own binaries. Between a shed release and its drover step the
old code runs; there is no flag day and no lane restarts at any step.

Not in this order: cloud and runner VMs. They are #1644's ADR, after shed's helper (shed #324 to #330) and D1.
Nor #1646: a lane's verbs on another host keep the outbox at every step, so it waits for none and none for it.
Nor host setup: the tools `drover hosts bootstrap` installs go with #1644 to shed-cloud's recipe (shed #337), and
the cache variables of #1641 are `spawn`'s own. Neither waits for a step here, and no step waits for them.

## Two installs with a gate of their own

Both gates hold before the step's build is installed on the main host: at the promote where self-update follows
main, at the merge where it follows the integration branch. Whoever installs reads the gate; the step's issue
stays blocked until it holds, and its PR body quotes the lines that showed it.

| gate | step | holds when | how it is reached |
|---|---|---|---|
| account | D2a (#1576) | `drover doctor` on the main host shows `✓ account <agent>: …` for every agent a role or an orchestrator starts over ACP: a signed-in named account each | the owner's `shed account add <agent> <name>` and its sign-in. The rows ship in the pin PR, so the build before D2a prints them |
| proxy | D2b (#1604) | `drover hosts keys` lists no `live` key: no lane started on drover's proxy is alive, asleep ones included | `drover hosts off <name>` for each other host, so no new lane goes there; their lanes finish. One that will not end: `drover kill`, and its next start is after the install. Then the install, `[proxy]` out of fleet.toml, `drover hosts on <name>` |

Why the account gate: an orchestrator thread is the owner's way in. With no named account its start would send
`auto`, meet `NO_ACCOUNT`, and the thread would not answer. Why the proxy gate: a lane started on drover's proxy
holds a key and a URL that only that proxy and its tunnel serve; D2b deletes both.

## What each drover step deletes and keeps

Sizes are estimates in src lines; each PR states its own `src/ net`.

| step | deletes | keeps | net |
|---|---|---|---|
| D2a | for an ACP lane on the main host: the host hold (`login_hold`) and the reading of a limit or a sign-in from the lane's banner | the `LOGIN_NEEDED` card and the limit hold, fed from `account.ls`; doctor's account rows (the pin PR's); all of it as today for a lane on another host, with `proxy.rs`; a terminal lane (dsh) has no account | +50 |
| D0 | `shed.rs`'s handshake, framing, `cli` and `adapter`; `term.rs`'s CLI word builders; the `shed help run` probe and `SHED_OLD`; the fake server in `crates/cli/tests/common/fake_shed.rs` | `version_alert`, doctor's shed row (from `hello`), the `shed.seq` mark, `term`'s typed helpers as thin calls; `ssh … shed connect` as the client's one-exchange transport, until D1 | −250 |
| D9 | in `providers/`: login, install, status, token and folder variables, efforts, models, the dsh wizard and kick; `Provider`'s fact methods | profile sync, hook dialects, guard meta, `hooks_allowed`, doctor rows, the "core names no provider" bans | −350 |
| D2b | `proxy.rs`, the tunnel in `survey`, `[proxy]`, `providers::Proxy`, the per-lane key in `spawn`, the proxy's own command, `[limits] proxy_connect_secs` | `NO_PROXY_TOKEN` as the name of shed's `NO_PROXY`; `hosts keys` as a view of `proxy.keys`, its revoke as shed's; `login_hold`, the reset parser and `proxy_retry_secs` for a lane with no account (open question 1) | −500 |
| D1 | `On`'s ssh wrap and shared connection, the session cap, the refused-session retry, `hosts/probe.rs`, `shed.rs`'s `connect` and `start_job`, `Pass`'s connection repair, `hosts.down`, the probe script's first lines; `host.rs`'s sampling, with the `swapcount*` and `swapouts` files; `top.rs`'s `ps` walk; doctor's `vm_stat` row; what #1623's fix left of `survey`'s cloud rule (a logged stop keeps a host down while shed does not list it up) | `Pass`'s budget, marks, late and unsent counts; `admit`; the sweep; tags, health rows, placement; the outbox | −950 |
| D12 | `hosts::record`, `sample`, the history file, `host::groups` | `Row`, `spark`, `series`, `chart`, fed from `machine.stats`; the lanes line from the log; the group bar from the 20 largest processes | −150 |
| D11 | `link::run`, `dial`; `watch::watch`, its stamp and stdin acks; `Runner::pipe`; the service's thread per host | `report`, `intake`, `view`; the outbox and its acked mark (#1646); one follower of shed's `events` in the service; with the service stopped, the poll as today | −200 |
| D3 | `secret::file`, `path`, `Delivered`, `secrets-left`, `write_private` | names, the request card, `ls`, `rm`, events, the wip secret scan | −120 |
| D4 | for drover's own binary, the only thing they move: `release.rs` (find, stage, the swap line, builds, the signature check, the install lines); `update.rs`'s `remote_swap` and its shell text | `step`, the marks, alerts, the swap order, profiles sync, the serve handover; every bootstrap step but drover's own, which calls `crate::shed`'s `release.rs` | −500 |
| D5 | `heavy.rs`'s lock line and `hold`'s own slot; `top`'s wait rows from files | which commands are heavy, the hook's rewrite, the limit numbers, `top`'s layout | −250 |
| D7 | `sandbox.rs`'s `wrap`, `file`, `resolved`, its fixed lists | whether a fleet is sandboxed; the folders and hosts a fleet needs | −100 |
| D8 | `usage::deepseek`, `parse_balance`, the key file | `warn_below`, the alert, the board's row | −60 |
| D10 | `timer.rs`'s `files`, `ctl`, `refused`, `rebootstrap`, `restart`, `job_pid`, the file compare in `drift`, the write and load in `install`; the three job templates | the job list, intervals, PATH, the tick, `due`, the Services rows | −250 |
| D13 | the reads of `machine.links`, `Link::down_since` and the old state fields, inside `crate::shed` | `Reach` and every caller of it | 0 |

About 3,700 lines in all. `cloud.rs` and `runners.rs` (about 850 more) leave under #1644.

## Symptoms folded here

Two open issues have their cause in what this ADR removes. This PR changes no code, so neither is fixed by a
commit of it. #1590 is moot as an issue of its own: the step that removes its cause carries its acceptance, and
it closes with this PR. #1623 is not moot: the owner kept its one-line fix as a step of its own, before D1
(2026-11-23), so it is fixed by its own commit and closes with its own PR. The PR body says so in these words.

| issue | its cause | ends at | the PR body's line |
|---|---|---|---|
| #1590: a missing shed binary reads as `SHED_OLD`, and nothing alerts | drover learns what a shed can do by running the `shed` CLI, so a CLI that cannot run reads as an old shed, and no start works without it | D0 | `#1590: moot as its own issue. D0 deletes SHED_OLD and its probe and starts lanes on the server's socket, so a missing CLI stops no start; D0's issue carries its acceptance (docs/architecture/shed-split/order.md).` |
| #1623: two boats stuck offline while ssh, shed and boat show them up | drover keeps its own answer to "is the host up": a probe's last result and the last `cloud-*` event, which `survey` trusts over shed's link | its own fix now; D1 for the rest | `#1623: fixed by <sha>, its own PR: survey trusts shed's up over a stale cloud-stop. D1 then deletes that rule and the probe, and its issue carries the rest of #1623's acceptance (docs/architecture/shed-split/order.md).` |

The interim fix, as the owner kept it: in `hosts::survey`, a cloud host that shed's machine list shows `up`
(the list every drive pass already reads, #1616) is up whatever its last `cloud-*` event says, and that pass
logs `cloud-up`. Its test is #1623's first: a host with a stale stop is up after one `hosts refresh` and one
drive pass, and its held lanes go on. Nothing else of #1623 is built before D1: the branch `issue-1623` changes
the probe's budgets and prints the probe, and D1 deletes the probe; what a refresh prints for a host that is
not up is D1's, from the link.

If this PR opens before that fix has merged, its line reads `#1623: not closed here: its fix is the interim
step of docs/architecture/shed-split/order.md, by the owner's decision of 2026-11-23`, and #1623 stays open.

## Issues to file

### shed

| # | title | scope |
|---|---|---|
| X1 | machine.stats and terminal.procs: the kernel's memory pressure level, a process's CPU time | `Sample::mem_level`: normal, warn or critical from `kern.memorystatus_vm_pressure_level` on macOS, none elsewhere; in `now` and in `history`. `Proc::cpu_ms`: the CPU time of its life, which shed already holds to compute `cpu_pct`. Both additions: no protocol change. About 25 lines |
| H2 | hosts 2: installs, for shed's own binary and a caller's own | narrower than shed #259's "any tool on any machine", by the owner's decision of 2026-11-23: `install.find`, `ls`, `stage`, `swap` and `back` move a release of shed itself, or of the tool that calls (drover's binary at drover's path), and refuse any other. No toolchain, no agent CLI, no cache; how shed knows the caller's own tool is H2's to settle |
| H1, H3, H5, H6, cap | as shed #259's ADR lists them | not filed yet. This ADR adds nothing to their scope. H4's VM half is gone (shed #307). Its `machine.setup` would install tools on a machine, which the owner's decision gives to the recipe (shed #337): drover calls it in no step, whatever shed keeps of it. `machine add --install` is shed's own binary, and no step here needs it |

Filed already and gating a drover step: #265 (released in `client-v5.1`: D2b); #318 to #323 (D13). Filed and
gating none: #336 (shed's non-goals line: a machine is link and processes) and #337 (the host setup recipe),
which #1644 and #1641 read.

### farrier

| # | title | scope |
|---|---|---|
| F1 | farrier-service: control commands through the caller's runner | `Env` takes an optional function that runs one control command (tool, arguments) and returns its exit and text, with a time limit; the default stays farrier's own. shed keeps working unchanged |

### drover

Already filed, with what this ADR changes in them:

| # | change |
|---|---|
| #1576 (D2a) | none to its scope. Its blockers are the pin and the account gate. Four lines of acceptance, from the consequences: the account rides ACP starts only; a lane on another host carries none until #1604; `NO_ACCOUNT` is one card with the `shed account add` line; the PR body quotes the main host's account rows, all `✓` |
| #1604 (D2b) | its blockers stay as filed: shed #265 released (done, `client-v5.1`), and #1576. Added to its scope: an ACP lane on another host starts with one `run` at the main host's shed with `machine` (the proxy serves no other start); an agent whose record has no `[proxy]` is sent no account there unless its key is set (open question 1); `[proxy]` in fleet.toml is a warning, not a refusal, so the build may go in before the key comes out. Its `## Rollout` is the proxy gate's lines in order, with one check more: every enabled host's link shows a relay port before `hosts on` |
| #1575 (D1) | blocked on D0 and X1 as well. "The client pinned to a tag" leaves its scope (D0). The last ssh words (`On::wrap`) move into `link/` until D11 deletes them. The `[limits]` keys of [shapes.md](shapes.md) are in its rollout. It moves #1616's `shed machine add` to `machine.add`. `Reach` has its four values and both tables' tests; a `reconnecting` link is sent nothing. lane-hog and `top` take a CPU share from two reads, the swap rate from two passes' `now`, each with a test on made-up counts ([shapes.md](shapes.md)). A lane's verb on another host still lands (the outbox). From #1623, beyond its own fix: a host whose link is up is up at the next pass, whatever drover's log says, and its held lanes go on; `hosts refresh` prints the link's error and how long it has been down for each host that is not up; `hosts on` then `hosts refresh` never leaves a host with an up link offline |
| #1623 | its scope is the one line and its first test (above); it is unblocked from #1567 and lands now. The rest of its acceptance is #1575's |
| #1646 | its routing line: not "then through shed exec". The outbox is the route before and after #1575, since shed has no link from a host back to the main host; the main host pulls it with the link's report, by `exec` from D11. It is blocked on no step here. A verb mid-turn arrives within `link_sample_secs` (30 s), one that ends the turn at once |
| #1641 | none to its scope, as amended: `spawn` sets the cache variables, and they ride `RunParams.env` once D0 has merged (the second of the two to land carries the other). The folders they name are in the list D7's sandbox may write. `reclaim`, `clean` and doctor report a shared cache's size and delete none. It waits for no step here |
| #1644 | takes D6 over: this ADR has no VM step. Added by the owner's decision of 2026-11-23: bootstrap's tool steps (`TOOLS`, `RUST`, node) do not stay in drover for a persistent host, as shed-cloud's port map had them; they are the recipe's (shed #337), run from a run card (whether an agent's CLI is the recipe's too is #1644's to say with shed #337), and bootstrap keeps drover's own steps: its binary (D4), the clones, the sign-in panes, profiles sync, and a check of each tool with the recipe's command as its fix. Its lane-host half needs D1. Its "no new lane on a draining VM" is `Reach::Draining`, which D1 defines; the read that returns it comes with `machine.ls` (D13, or its own lane-host slice if first) |
| #1649 | its host fact (up, offline, draining) is `Reach`, read through `crate::shed` once D1 has merged; whichever lands second moves those reads |
| #1561 | closed: #1567 replaces it |

To file. Each PR moves every caller, deletes what its row above lists, and states its `src/ net`:

| # | title | acceptance, beyond its row | needs |
|---|---|---|---|
| pin | pin: `shed-proto` at `client-v5.1`; doctor's account rows | one line in `Cargo.toml`, the lock, and whatever no longer compiles; `shed::MIN_PROTOCOL` stays (D2a raises it to 5). Doctor on the main host prints one account row per agent a role or an orchestrator starts over ACP, from `account.ls` ([shapes.md](shapes.md)); a shed under protocol 5 is a `✗` row naming the update | step 0 |
| D0 | the shed client: drover on shed-client at a tag | no `Hello::ours` and no `shed_client` outside `crate::shed` (two bans); an ACP lane and an orchestrator thread start with one `run` call: no `shed run` command line, no temp file for MCP servers; an old server is `SHED_VERSION` naming the call; `spawn --dry-run` prints each call from its typed params. From #1590: with no `shed` on PATH a start succeeds while the server answers; no refusal says `SHED_OLD`; doctor prints, per host, the server's build and whether a `shed` on PATH runs, and names the path it tried | the pin |
| D9 | provider facts come from shed's catalog | `providers/` has no login, install, status or variable name of any agent; config is checked against the built-in records, a lane starts by the served ones; dsh lanes start through the record's `[terminal]`; `toml` is 1.2 and `cargo deny` passes | D0 |
| D12 | host charts read shed's history | no `host-history.jsonl` is written; the Host tab's series equal `machine.stats` history for each host; one read per host a minute at most; the PR names the two visible changes (the group bar, the lanes line) | D1 |
| D11 | the link reads shed's events | no `drover hosts watch` process on any host; a `turn done` on another host kicks its issue with the report its turn's end causes, no later sample needed; a mid-turn outbox line arrives within `link_sample_secs`, whatever verb wrote it (#1646); `link-down` follows shed's link | D1 |
| D3 | secrets are shed's crate | as shed #259's D3 | H1; D1 |
| D4 | self-update through shed | as shed #259's D4, for drover's own binary: `crate::shed`'s `release.rs` is its only caller of `install.*`, and none of its functions takes a tool name or a path ([shapes.md](shapes.md)); bootstrap's drover step calls the same; no other bootstrap step changes | H2; D1 |
| D5 | heavy slots are shed's | as shed #259's D5 | H3; D1 |
| D7 | the sandbox is a run option | as shed #259's D7 | H5; D1 |
| D8 | balances are shed's | as shed #259's D8 | H6; D2a |
| D10 | timer jobs through farrier-service | `drover timer install` twice changes nothing the second time; a job launchd refuses is healed as today; tests run no real `launchctl`; the rollout section names the one reload | F1 |
| D13 | drover reads shed's state fields | `reach` reads `machine.ls`; a lane's state is its `flow`; no `machine.links` call and no old state field is read; shed #323 then removes them from the client, so the compiler holds it | shed #318 to #322 released; D1, D2b |
| pin | pin: shed `client-v<P>.<n>` | one line in `Cargo.toml` and the lock; filed by an orchestrator when a tag a step needs exists | the tag |
