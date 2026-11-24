# main-host-only: the cut of the remote-agent code (#1691)

## Summary

Decision: delete everything drover carries for lanes on other hosts, in ten delete-only slices, leaf first, down to the ssh wrapper and the machine field themselves. First the owner empties the other hosts and takes them out of fleet.toml (step 0), so every slice removes code that can no longer run and no slice needs a gate of its own. The main host behaves as today: admission, slots, heavy slots, lane-hog, top, disk trim, its own self-update. Its own entry in fleet.toml stays as written; old keys are ignored, and an entry that names another machine is dropped with one warning.
Slices: 1 self-update of other hosts and the shed machine list (unblocks #1685); 2 cloud VMs; 3 the token proxy; 4 lane verbs from another host (the outbox); 5 checks of other hosts and the nightly soak; 6 placement between hosts; 7 offline hosts and the ssh pass; 8 files and sweeps on other hosts; 9 the ssh wrapper and the machine field; 10 CI runners, when the runners repo covers them. About 4,800 lines of code and 13,900 of tests go.
Open questions: (1) are the cloud VMs destroyed in step 0, or who stops them after slice 2? (2) the cut does not wait for the runners repo; slice 10 does: accept? (3) about 10 added lines keep an old fleet.toml safe: accept? (4) nine of the sixteen listed issues close, six shrink, one stays: accept the table? (5) ten issues, filed on approve, serial: accept?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how the callers read after the cut; the command line; fleet.toml; what drover needs from `runners run` |
| `adr.md` | the question, options, decision, step 0, the ten slices with their `src/ net` and tests, the issue table, consequences, open questions, the `.drover/arch.toml` lines |
| `inventory.md` | every file and function that exists only for other hosts, with line counts; what stays and where; the tests; two rules for mixed tests; docs and generated files per slice |
| `shapes.md` | the types after the cut; old keys; the log; the `[limits]` keys, refusal codes and event kinds that end |
| `boundaries.md` | which module may call which; three statecharts: a start on the main host, the host fact, the cut's own order |

## The cut on one screen

```
 today                                                after
 ─────                                                ─────
 spawn ─▶ place: pick a host ─▶ On(machine) ─▶ ssh    spawn ─▶ place: does the main host admit it? ─▶ runner
          wake a cloud host                                    LOGIN_NEEDED · DISK_LOW · MEM_LOW · HOST_BUSY · NO_HOST
          proxy key + tunnel for an ACP lane
 a lane's verb ─▶ outbox file ─▶ link ─▶ intake       a lane's verb ─▶ the log
 pass ─▶ probe each host, hosts.down, offline,        pass ─▶ marks its step
         evacuate after 15 min
 check ─▶ survey every host, health rows, sshd,       check ─▶ survey this host: tags, load, memory, disk
          trim and sweep over ssh, cloud idle stop             trim and sweep here
 self-update ─▶ stage on every host, wait, swap all   self-update ─▶ stage and swap this binary
 link ─▶ `hosts watch` on each host, over ssh         link ─▶ `hosts watch` on this host (unchanged)
```

| Today | After |
|---|---|
| `hosts::On` wraps the runner in 31 files; each caller asks which host | the runner and this host's files |
| `[hosts]` lists the main host, a second Mac and cloud VMs | `[hosts.<name>]` is the main host's limits, or absent |
| `drover hosts` has eleven verbs | four: the list, `refresh`, `on`, `eject` (and `report`, `watch` for the service) |
| 17 `[limits]` keys time ssh, the proxy, the outbox, offline hosts and the rollout | gone; the `link_*` keys of this host's watcher stay |
| five refusal codes exist only for other hosts | gone: `HOST_OFFLINE`, `HOST_UPDATING`, `NO_PROXY_TOKEN`, `DESIGN_MAIN_ONLY`, `MAIN_HOST` |
| the Host tab shows a card per host, toggles and a token proxy section | one card |
| `src/` 64,266 lines, tests 94,752 | about 59,400 and 80,900 |

## Step 0, before any code

The owner moves or ends every lane on another host, destroys the cloud VMs, sweeps the hosts once, and takes
them out of fleet.toml in one config card (`adr.md`, decision 1). That alone unblocks #1685. After it the
slices can merge at any pace: nothing they delete can run.

## The slices

| # | Area | `src/` net | Tests |
|---|---|---|---|
| 1 | self-update of other hosts, the shed machine list | −220 | −440 |
| 2 | cloud VMs | −800 | −1,950 |
| 3 | the token proxy, ACP lanes on other hosts | −860 | −1,350 |
| 4 | lane verbs from another host: the outbox, frozen hook rules, imported asks | −360 | −1,680 |
| 5 | checks of other hosts: health and sshd rows, the nightly soak | −360 | −550 |
| 6 | placement between hosts | −310 | −1,130 |
| 7 | offline hosts, the ssh pass | −590 | −2,620 |
| 8 | files and sweeps on other hosts, the link's ssh half | −430 | −3,470 |
| 9 | the ssh wrapper `On` and the `machine` field | −390 | −370 |
| 10 | CI runners, behind its own gate | −510 | −310 |

Each is one issue and one PR that only deletes; `adr.md` names its files and `inventory.md` its functions.

## Open questions

1. **The cloud VMs.** Destroyed in step 0 (recommended), or who stops them once slice 2 has merged?
2. **Runners.** The cut does not wait for the runners repo; only slice 10 does (recommended).
3. **Ten added lines.** A `[hosts]` entry that names another machine is dropped at load with one warning, so it
   can never read as a second main host (recommended). It is the only code the cut adds.
4. **The issue table.** #1604, #1646, #1642, #1549, #1647, #1463, #1644, #1496 and #1673 close; #1575, #1672,
   #1669, #1670, #1677 and #1641 shrink; #1576 stays (`adr.md`, "Effect on filed issues").
5. **Ten issues.** Filed by the orchestrator on approve; 1 to 9 serial and on top of the queue, 10 behind its
   gate. This issue's own PR is the ADR alone.
