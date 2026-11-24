# ADR: main-host-only — the cut of the remote-agent code (#1691)

## Summary

Decision: delete everything drover carries for lanes on other hosts, in ten delete-only slices, leaf first, down to the ssh wrapper and the machine field themselves. First the owner empties the other hosts and takes them out of fleet.toml (step 0), so every slice removes code that can no longer run and no slice needs a gate of its own. The main host behaves as today: admission, slots, heavy slots, lane-hog, top, disk trim, its own self-update. Its own entry in fleet.toml stays as written; old keys are ignored, and an entry that names another machine is dropped with one warning.
Slices: 1 self-update of other hosts and the shed machine list (unblocks #1685); 2 cloud VMs; 3 the token proxy; 4 lane verbs from another host (the outbox); 5 checks of other hosts and the nightly soak; 6 placement between hosts; 7 offline hosts and the ssh pass; 8 files and sweeps on other hosts; 9 the ssh wrapper and the machine field; 10 CI runners, when the runners repo covers them. About 4,800 lines of code and 13,900 of tests go.
Open questions: (1) are the cloud VMs destroyed in step 0, or who stops them after slice 2? (2) the cut does not wait for the runners repo; slice 10 does: accept? (3) about 10 added lines keep an old fleet.toml safe: accept? (4) nine of the sixteen listed issues close, six shrink, one stays: accept the table? (5) ten issues, filed on approve, serial: accept?

## Status

Proposed (architect lane, 2026-11-24, staging `356c5da7d`). Read `usage.md` first, then this, `inventory.md`,
`shapes.md`, `boundaries.md`.

## Question

Agents run on the main host only (owner, 2026-11-24). drover still holds about 4,800 lines of code and 13,900
lines of tests for lanes on other hosts, and that code still fails: #1685 is a check of a second host that
blocks the main host's own update. In what order is it deleted so that every step keeps CI green, changes
nothing the main host does, and leaves no half of the old design behind?

## Where it stands

- **The remote code is not a module, it is a parameter.** `hosts::On { machine: Option<&str> }` wraps the runner
  in 31 files; `machine` is named on 261 lines of 40 files, 35 of them reading it off an event; `Sessions` is
  one answer per host. Six files are remote only (`cloud.rs`, `proxy.rs`, `outbox.rs`, `soak.rs`,
  `hosts/probe.rs`, `runners.rs`: 1,830 lines). The other 3,000 lines are branches inside files that stay
  (`inventory.md`).
- **`link/` is not remote only.** The main host is a link host too: the service keeps `drover hosts watch` open
  on this host, and drive, check and the board read lane facts from the view it feeds. Of its 835 lines about
  190 are the outbox and the ssh carriage.
- **`drive/remote.rs` is misnamed.** `Laps` and `Memo` time and memoize any pass. Only `Tally` (45 lines) counts
  calls to other hosts.
- **Config loads loosely.** `Config`, `HostCfg` and `Limits` take unknown keys, so a removed key is ignored
  without code. One key is not safe to ignore: without `machine`, a second host's entry would read as a second
  main host.
- **The tests are three times the issue's guess.** Besides the files named for it, the remote host sits in
  shared fixtures (`common`'s board, `perf.rs`, `tick_stuck.rs`, `drive_jobs.rs`), and some main-host behaviour
  is tested only through a remote lane.
- **`runners.rs` calls `cloud::load` and `cloud::run`**, which use three small helpers, and nothing else of
  `cloud.rs`.

## Options

| Option | Why not |
|---|---|
| 1. Leave the code; keep the hosts off in config (today) | it still breaks the main host (#1685), and every PR in `hosts.rs`, `check.rs` and `spawn.rs` reads and tests around it |
| 2. Delete the six whole files and their callers; keep `On`, `machine` and the choice between hosts "in case" | leaves the transport and half of each caller dead: about 40 branches nothing runs or tests, and two ways to read a lane's file |
| 3. One PR | about 4,800 and 13,900 lines over well more than a hundred files: no review can hold it, and it conflicts with every open PR |
| 4. A build feature that compiles the remote code out | the code stays, untested by default: worse than today |
| 5. Wait for the shed split's D1 (#1575) to turn `On` into shed machines, then delete | D1 is blocked on a shed release, and it would port code this cut deletes |
| **6. Step 0 by the owner, then leaf-first delete-only slices, the last one removing `On` and `machine` (chosen)** | |

Smaller choices:

- **The main host's entry stays `[hosts.<name>]`.** A new `[host]` table would be a feature and a config change
  for the owner. The name keeps its meaning in the Host tab, the history file and the `spawn` event.
- **Step 0 before any code.** With the other hosts out of fleet.toml, no remote path can run. The slices are
  then plain dead-code removal: no slice has an install gate, and their order follows only what compiles.
- **Leaf first.** A slice removes callers before the thing called. So `On` and `Pass` shrink last, and no slice
  has to fake a host.
- **Old events are kept and read once.** The log keeps `machine` on old events. One predicate, `Event::here`,
  tells them apart (`shapes.md`); the frozen lists of event kinds are not edited.
- **The view stays.** Reading lane facts straight from shed would be a redesign of #1309, not a deletion. It is
  #1670's, shrunk (below).
- **Mixed tests keep their main-host half** (`inventory.md`, rules T1 and T2).

## Decision

1. **Step 0 is the owner's, with no code.** It holds when all five lines hold; the orchestrator quotes them in
   slice 1's issue.

   | | Holds when | How |
   |---|---|---|
   | a | no open issue, parked ones included, has its last lane on another host | `spawn <fleet> <n> <role> --supersede --host main` moves a live one (unpushed work goes to a `wip/` branch first); `kill --yes --host <name> <agent>` saves and ends one that will not run soon. drover does not contact a host that is off: `hosts on <name>` first, `off` after |
   | b | `hosts keys` lists no live key | follows from a |
   | c | each cloud VM is destroyed, or the owner names who stops it from now on | `hosts destroy <name>`; after slice 2 drover can stop none |
   | d | the other hosts' finished worktrees and secrets files are swept | `drover clean --yes`, once, while drover still reaches them |
   | e | fleet.toml has no `[hosts.<n>]` with `machine`, no `[proxy]`, no `[fleet.<n>.soak]`, no role `host` pin naming a removed host; the removed hosts' names are in `private_names` | one `drover config propose` card |

   After e, #1685's failing row no longer prints: the main host updates again before any slice merges. The
   names go to `private_names` because the public-leak check takes host names from `[hosts]`: once an entry is
   gone, its name would pass unchecked into a public PR or commit.

2. **Ten slices, each delete-only.** A slice removes code, the tests of that code, its docs section, its
   `[limits]` keys and its refusal files, then runs `just gen`. It adds nothing but the one thing named in
   decision 5. Slices 1 to 9 are serial, each blocked on the one before: they all edit the same shared files.

3. **The main host's behaviour does not change.** `inventory.md`, "What stays", names the code behind each
   behaviour of the issue's list. No slice edits a function in that table beyond removing its `machine`
   argument or its loop over hosts.

4. **`[hosts.<name>]` is the main host's entry**, with the keys of `shapes.md`. `Config.hosts` stays a map that
   holds at most one entry after `parse`; `Config::main()` reads it.

5. **Old config never fails a load, and never changes meaning.** Removed keys and tables are ignored, with no
   code. One key needs code: an entry with `machine` is dropped at load, so it never reads as a second main
   host, and gives one row on the retired-key path (doctor, the board's config line, `check`'s
   `config-warning`). This is the cut's only added code: about 10 lines in `config.rs` and one test, in slice 6.

6. **`drover hosts` keeps four verbs:** the list, `refresh`, `on <name>` (it releases a `LOGIN_NEEDED` hold)
   and `eject`; `report` and `watch` stay for the service. `--host` leaves `spawn`, `kill` and `close`.

7. **Runners: the cut does not wait.** `runners.rs` and the `runners` verbs follow the runners-repo
   decision of #1567: deleted, not moved, once `jirathip-dev/runners` covers them. Slice 2 deletes `cloud.rs`
   but the five functions `runners.rs` calls (about 70 lines). Slice 10 deletes those, `runners.rs`,
   `[ci_runners]`, the verbs and the recipe. It waits for one thing: the runners repo covers `status`, `revive`,
   `add`, `remove`, `tokens`, `retire-local` and the 30-minute token refresh. The `runners` name stays accepted
   in `[orchestrator] allow`, where an unknown name fails the load.

8. **The remote-build policy is not here.** It will hang off `[fleet.<n>.heavy]`, which this cut does not touch.
   `usage.md` gives the one call shape and the exit codes drover needs from `runners run`.

## Slices

`src/ net` is the expected change of `crates/*/src`; "tests" is lines removed under `crates/cli/tests`. Each
row's functions, files and docs are listed in `inventory.md` under the same number.

| # | Area | Deletes | `src/` net | Tests |
|---|---|---|---|---|
| 1 | self-update of other hosts, the shed machine list | in `update.rs` the loop over hosts, `Step::Hold`, the remote swap, `remote_swap`; `hosts::updating` and `HOST_UPDATING`; `shed::unlisted`, `add_machines`, drive's call and doctor's `shed machine` row; `host-sync-failed` | −220 | −440: `host_sync.rs` but its two main-host tests, `drive/shed_machines.rs`, one doctor test |
| 2 | cloud VMs | `cloud.rs` but its recipe loader; the cloud keys of `[hosts]`; `hosts bootstrap`, `snapshot`, `destroy`; overflow to a cloud host, `wake`, `held_for`; the cloud card and doctor rows | −800 | −1,950: `cloud.rs`, `boat.rs`, the cloud tests of `hook.rs`, `shed.rs`, `hosts_screen.rs` |
| 3 | the token proxy, ACP lanes on other hosts | `proxy.rs`; `[proxy]`; the `proxy` command, `hosts keys`; `remote_acp` and the lane key in `spawn`; `acp_guard`, `acp_health`, `acp_profiles`; `[hosts] runtime`; the proxy half of the login hold; the proxy section of the hosts sheet | −860 | −1,350: `remote_acp.rs`, the proxy rows of `hosts_screen.rs` and its golden |
| 4 | lane verbs from another host | `outbox.rs`, `cli::forward`, `foreign`; outbox lines in the link's frames, intake and acks; `outbox-stale`; the frozen rules file and `rules_stale`; `hosts::import` | −360 | −1,680: `outbox.rs`, `paused_delivery.rs`, `remote_rules.rs`, `link.rs` and `perf.rs`'s remote tests, the outbox tests of `faults.rs` |
| 5 | checks of other hosts | `hosts::health`, `shed_row`, the sshd row and step; doctor's rows per remote host; `soak.rs`, `[fleet.<n>.soak]`, the `soak` command, the tick's nightly start | −360 | −550: `ssh_sessions.rs`, the health tests of `hosts.rs`, the soak tests of `faults.rs` |
| 6 | placement between hosts | the choice in `hosts::place`, `unsaved`, `left`, `each`, `evacuate`; `--host`; the role `host` pin and `DESIGN_MAIN_ONLY`; `hosts off` and `enabled`; `Heal::Move`. Adds decision 5's dropped entry | −310 | −1,130: `stall_move.rs`, the placement tests of `hosts.rs`, `drive_kicks.rs`, `host_slots.rs`, `repro_host.rs`, `disk.rs` |
| 7 | offline hosts, the ssh pass | the remote half of `hosts::Pass`, `hosts/probe.rs`, `hosts.down`, `offline`, `held`, `configured`, `disabled`, `idle_off`; `HOST_OFFLINE`, `host-offline`, `hosts-skipped`; `drive::Tally`; `Wire::Connect`; the flow table's `offline` and `draining`; eight `[limits]` keys | −590 | −2,620: `host_offline.rs`, `host_deadline.rs`, `host_spared.rs`, `stalled_host.rs`, `tick_dead_host.rs`, `ssh_deadline.rs`, `pass_run_for.rs`, most of `host_down.rs`, `offline_tick.rs`, `tick_stuck.rs`, `shed_batch.rs` |
| 8 | files and sweeps on other hosts | `check::trim_hosts`, `trim_remote`, `remote_orphans`; the remote half of `sweep.rs`, `roots.rs` and `clean`; `hosts::present`; the ssh half of `link::report`, `dial` and `read` | −430 | −3,470: `lane_dead_remote.rs`, `remote_exists.rs`, `hosts/report.rs`, `upstream.rs`, `warm.rs`, `wip.rs`, the rest of `hosts.rs`, the remote blocks of `scratch.rs`, `remote_verdict.rs`, `clone_heal.rs`, `orphans.rs`, `faults.rs` |
| 9 | the ssh wrapper and the machine field | `hosts::On`, `Slot`, `rehome`, `under`; `machine` and `home` on `Placed`, `Session`, `Seen` and the `spawn` event; one answer per host in `Sessions`. Every caller takes the runner; `Event::here` is the one reader left | −390 | −370: `common`'s `boat`, `OverSsh` and ssh batch branches; the remote host in shared fixtures |
| 10 | CI runners (gated, decision 7) | `runners.rs`, the rest of `cloud.rs`, `[ci_runners]`, the `runners` verbs, the recipe, the runners tick | −510 | −310: `runners.rs` |

About −4,830 and −13,870. `src/` goes from 64,266 to about 59,400; the line cap is 65,000.

What a slice's issue must carry besides its row: its `src/ net` in the PR body; the visible changes it makes
(slice 2: the cloud row of a host card; 3: the token proxy section; 6: the on and off toggle; 7 and 9: one host
card where there were several), since a visual change the issue does not name fails review; and the generated
files of `inventory.md`'s last table. Slice 9 is mechanical and wide (31 files): if its diff is over one
review, it splits by caller, since `On::new(r, None)` stays valid until the type goes.

## Effect on filed issues

| Issue | Verdict | Why, and the new scope |
|---|---|---|
| #1575 D1: hosts are shed machines | shrink | the main host's numbers only: admission reads `machine.stats`, `top` and lane-hog read `terminal.procs`, `host.rs`'s sampling goes. Out: `machine.add`, links and `Reach`, `exec`, `file.write`. Its "no ssh" line is met by slices 7 to 9 |
| #1576 D2a: accounts are shed's | keep | accounts and the login hold are the main host's; only its sentence on lanes of another host falls away |
| #1604 D2b: the token proxy is shed's | close | slice 3 deletes `proxy.rs`, the tunnel, `[proxy]` and `hosts keys` outright; no lane is left for a proxy to serve |
| #1646: remote lanes never read drover state | close | no lane runs where fleet.toml and the log are not; slice 4 deletes the outbox |
| #1642: private names for remote lanes | close | the same; folded under #1646 |
| #1549: lesson, triage and deps lanes placed on remote hosts | close | every role runs on the main host; slice 6 deletes the choice and the pin. Its stopgap is not needed with the hosts off |
| #1647: placement policy, persistent or pool | close | there is no pool and no second host for lanes. Which commands go to a build machine is the remote-build policy on `[heavy]`, a new issue after runners#11 |
| #1463: repro reviews on hosts that can run the tests | close | every review runs on the main host, never on a bare VM. A capability the main host lacks is a `needs` tag: `NO_HOST` with its one card, which is what the issue asked for |
| #1644: architect, cloud VMs leave drover | close | slice 2 deletes `cloud.rs` and slice 10 the runner code: its whole deletion list. No lane host is rented, so no VM tool is designed here; build machines are the runners repo's |
| #1672 D4: self-update through shed | shrink | drover's own binary on the main host: `release.rs` on shed's `install.*`. `remote_swap` and bootstrap's step are gone (slices 1, 2); it needs H2 only |
| #1669 D12: host charts read shed's history | shrink | one host: the main host's card from `machine.stats` history; after the shrunk #1575 |
| #1670 D11: the link reads shed's events | shrink | this host only: the service follows shed's events in place of the `hosts watch` child. Its lines on another host and on the outbox are moot |
| #1677 D13: drover reads shed's state fields | shrink | a lane's state is its `flow`. `reach` and `machine.ls` are moot; it no longer needs D1 or D2b |
| #1496: one shed script per remote pass | close | no pass calls another host |
| #1641: one dependency build per host | shrink | the main host's lanes only: the cache settings on a lane's start, `reclaim` and `clean` reporting the shared cache. Its "remote hosts get the same" line goes; builds that move to a build machine use the runners repo's warm worktrees |
| #1673 D5: heavy slots are shed's | close | owner, 2026-11-24: heavy slots stay out of shed; `heavy.rs` stays drover's |

Not in the issue's list: **#1685** loses its cause at step 0 e and again at slice 1, which deletes the row it
trips on. Its second ask (one alert when a release stays unstaged) is not this cut's and stays its own.

Of the six issues taken off the queue for this ADR: #1646, #1549, #1604, #1647 and #1644 close; #1669 goes back
on, shrunk, after #1575.

## Consequences

- **Good.** `src/` loses about 7.5 percent and the tests about 15 percent. The class behind some 30 closed
  issues (a host offline, a tunnel down, a verb on the wrong host, a boat that moved) has no code left to fail.
  `hosts.rs` falls to about 900 lines. The shed split loses D2b and D5 and the remote halves of D1, D4, D11,
  D12 and D13; D0, D2a, D3 and D7 to D10 keep their scope.
- **No way back by config.** After slice 6 a `machine` entry is a warning, not a host. Lanes on another host
  would be a new design on shed's machines, not a revert.
- **A need the main host lacks is refused.** A lane that needs `docker` or `os:linux` gets `NO_HOST` until the
  remote-build policy sends such commands to a build machine. The second Mac takes no lane: it takes commands,
  through `runners run`.
- **The main host is the one point of failure**, as it already was for drive, the log and the board.
- **The approved flow table changes.** Its host fact goes from four values to two and loses the `host-offline`
  row (slice 7). Its slice 2 (#1654) reads one host's sample. Whichever of the two lands second adapts.
- **Between step 0 and slice 9 dead code still compiles** and its tests still run. Nothing depends on that
  window being short, but each open PR in the shared files meets ten deletions: the slices merge fastest when
  queued on top.
- **Old logs stay readable.** Events of other hosts fold as before; `Event::here` keeps them from holding or
  cleaning anything on this host.
- **Tests are cut, not only deleted.** About a third of the 13,900 lines sit in files that stay. Rules T1 and
  T2 keep each main-host assertion; a reviewer checks them per slice.

## Open questions

1. **The cloud VMs (step 0 c).** After slice 2 drover can neither stop nor destroy one. Recommended: destroy
   each with `hosts destroy` in step 0. If one is kept as a build machine, say so: it is then the runners
   repo's from that day.
2. **Runners.** Recommended: do not wait (decision 7). The other way holds slice 2 until the runners repo is
   ready, so that `cloud.rs` goes in one piece; it gains nothing but one smaller file for a while.
3. **The 10 added lines (decision 5).** Without them slice 6 is pure deletion, and a fleet.toml that still has a
   second host would treat it as a second main host. Step 0 e clears yours; the lines protect any other copy.
   Recommended: accept.
4. **The issue table.** Nine close, six shrink, one stays. Recommended: accept; the orchestrator closes and
   amends on approve.
5. **Ten issues.** This issue's PR is the ADR alone. Recommended: the orchestrator files slices 1 to 10 on
   approve, 1 to 9 each blocked on the one before and queued on top, 10 blocked on slice 2 and on its gate.

## The `.drover/arch.toml` lines

No hot file is converted here: the docs are already one file per area, the usage doc one file per refusal, the
test shards a collected folder. Three guards change, each in the slice that makes it true.

Slice 4 replaces the `outbox.jsonl` ban (it loses its `except`):

```toml
[[ban]]
pattern = "outbox.jsonl"
paths = ["crates/*/src/**"]
reason = "#1691: every lane runs on the main host and writes its log; no verb is queued in a file"
```

Slice 9 replaces the `hosts::On` ban (it widens from two files to all) and adds one:

```toml
[[ban]]
pattern = "hosts::On"
paths = ["crates/*/src/**"]
reason = "#1691: lanes run on the main host only: a lane's files are this host's, its commands run on the runner"

[[ban]]
pattern = "ssh"
paths = ["crates/*/src/**"]
except = ["crates/core/src/hook.rs", "crates/core/src/sandbox.rs", "crates/core/src/hygiene.rs"]
reason = "#1691: drover reaches no other host; a build machine is reached by `runners run`"
```

The three files in `except` are the ones that name ssh as data today (a wrapper word, a denied folder, a leak
pattern); slice 9 confirms the list with one arch-check run.

Slice 10 takes one line out of `shared`:

```toml
"crates/core/src/cloud.rs",
```

`.drover/arch-baseline.txt` loses the size entries of each deleted file in the slice that deletes it.
