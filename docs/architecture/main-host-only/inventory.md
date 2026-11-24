# main-host-only: the inventory (#1691)

Read on staging `356c5da7d`. `src/` is 64,266 lines and the tests are 94,752. Line counts of whole files are
exact; "≈ out" for a file that stays is an estimate from its functions' spans. The number in brackets is the
slice (`adr.md`, "Slices").

About 4,800 lines of `src/` and 13,900 lines of tests exist only for lanes on other hosts. The issue's guess for
tests was 5,000 to 6,000: the shared fixtures and the remote halves of files named for something else make up
the difference.

## Code: files that go whole

| File | Lines | Slice | Note |
|---|---|---|---|
| `core/src/cloud.rs` | 674 | 2, 10 | slice 2 leaves `key`, `load`, `user_recipe`, `builtin`, `run` (≈ 70), which `runners.rs` calls; slice 10 takes the rest |
| `core/src/proxy.rs` | 492 | 3 | |
| `core/src/outbox.rs` | 36 | 4 | |
| `core/src/soak.rs` | 158 | 5 | the nightly multi-host soak; the serve memory soak (`tests/soak.rs`, CI's `soak` step) is another thing and stays |
| `core/src/hosts/probe.rs` | 111 | 7 | |
| `core/src/runners.rs` | 359 | 10 | with the built-in recipe `recipes/boat.toml` (54) |

## Code: files that shrink

| File | Lines | What goes | ≈ out |
|---|---|---|---|
| `core/src/hosts.rs` | 1,989 | [1] `updating`, the version read in `survey`. [2] the cloud lines of `survey`, `at` and `place`, `held_for`, `wake`. [3] `acp_health`, `acp_profiles`, `Unguarded`, `acp_guard`, `sick`, `proxy_ok`, the proxy halves of `login_hold` and `hold_alert`, the tunnel in `survey`. [4] `import`. [5] `health`, `shed_row`, `SHED_RESTART`, `sshd_step`, `sshd_row`, `real_exe`, `with_path`, `link_line`. [6] the choice between hosts in `place`, `unsaved`, `left`, `each`, `evacuate`, the off half of `toggle` and `shown`. [7] the remote half of `Pass` (`sparing`, `seen`, `late`, `unsent`, `fast`, `host`, most of `call`), `disabled`, `down`, `went`, `down_for`, `idle_off`, `offline`, `held`, `configured`. [8] `present`, `EXISTS`, the sweep in `disk_low`. [9] `On`, `Slot`, `Runner for On`, `CM`, `rehome`, `under` | 1,100 |
| `core/src/check.rs` | 2,530 | [1] `host-sync-failed`. [2] `cloud::tick` and the stopped checks. [4] `outbox_stale`. [7] the `host-offline` and `host-online` alerts, the remote half of `stale`. [8] `trim_hosts`, `trim_remote`, `TRIM`, `remote_orphans`, `DIRS`, the remote half of `trim_on`, `build_dirs` and `clean_all`. [10] the runners tick | 285 |
| `core/src/check/sweep.rs` | 267 | [8] `disabled`, `sweep_off`, `hosts`, `reported`, the remote half of `sweep` | 113 |
| `core/src/update.rs` | 627 | [1] the loop over hosts in `members`, `Step::Hold`, the remote loop in `swap`, `remote_swap`, the host rows of `plan` | 130 |
| `core/src/spawn.rs` | 1,959 | [3] `remote_acp`, the lane key, `remote` in `unguarded` and `acp_cmd`. [4] `DROVER_OUTBOX`, the rules file. [6] `--host`, the move between hosts. [9] the `home` closures and each `on.machine` branch | 160 |
| `core/src/link/` | 835 | [4] `Line`, `Worktree.lines`, `acked`, `ack`, `lines`, the stdin acks, `lane_of`, `ran`, `run`, `View::lines`. [8] `exe`, the ssh half of `report` and `dial`, `look_at`'s machine, the `cat` in `read`. The rest is the main host's own watcher and the view: it stays | 190 |
| `core/src/cli.rs` | 2,505 | [2] `hosts destroy`, `bootstrap`, `snapshot`. [3] `proxy`, `hosts keys`. [4] `forward`, `foreign`. [5] `soak`. [6] `--host`, `hosts off`. [7] `Pass::fast` and `sparing` at their call sites. [10] `runners` | 162 |
| `core/src/config.rs`, `config/limit_docs.rs` | 2,027 | the fields of `shapes.md`; `ProxyCfg`, `GithubToken`, `CLOUD_ALLOW`, `host_acp`, `shed_on`, `CiRunners`; seventeen `[limits]` keys with their doc rows | 190 |
| `core/src/doctor.rs` | 531 | [1] the `shed machine` row. [2] the cloud rows. [3] the proxy row. [5] the rows of each remote host | 64 |
| `core/src/shed.rs` | 404 | [1] `unlisted`, `add_machines`. [7] `Wire::Connect`, `connect`, `start_job` | 64 |
| `core/src/roots.rs` | 396 | [8] `clone_warm_on`, `WARM`, the remote halves of `state`, `eject`, `watch`, `restart` | 65 |
| `core/src/session/` | 992 | [3] the proxy causes. [9] `Session.host`, `Session.machine`, `list::tagged`, one answer per host in `Sessions` | 59 |
| `core/src/drive.rs`, `drive/remote.rs`, `drive/pickup.rs` | 2,525 | [1] the `add_machines` call. [6] the `evacuate` call. [7] `Tally` and the pass event's `remote` field, the `OFFLINE` mapping. [8] the remote read in `fetch`. `Laps` and `Memo` time and memoize a pass: they stay | 91 |
| `core/src/hook.rs` | 1,139 | [4] the `DROVER_RULES` branch of `lane_rules`, `rules_stale`, the frozen fields | 36 |
| `core/src/timer.rs` | 796 | [5] the nightly soak. [7] `skipped`, `hosts-skipped`. [8] the `trim_hosts` step | 35 |
| `core/src/slo.rs` | 564 | [6] `Heal::Move`: on one host it only retried the start, which every pass does | 25 |
| `core/src/providers/mod.rs` | 330 | [3] `Proxy`, `proxied` | 25 |
| `core/src/snapshot.rs`, `runner.rs`, `waiting.rs`, `queue.rs`, `stall.rs`, `secret.rs`, `lanes.rs`, `starts.rs`, `setup.rs`, `breaker.rs` and nine files with one `On` each | | [7] `snapshot::remote`, the ssh time limit, the `host offline` lines. [8] the remote half of `secrets_left`. [9] one `On` or `machine` each | 110 |
| `server/src/screens_hosts.rs` | 158 | [2] the cloud word. [3] the token proxy section and its sheet. [6] the off toggle | 51 |
| `server/src/serve.rs`, `serve/fresh.rs`, `serve/tails.rs`, `serve/v1.rs` | | [2] the cloud card row. [6] the toggle's route. [7] `Source::Remote` | 45 |
| `service/src/links.rs` | 13 | [8] the thread per host becomes one | 3 |

`web/src` reads the screens as data and needs no change of its own; `web/dist` is rebuilt only if a slice
touches it.

## What stays, and where it lives

| Main-host behaviour | Code that stays |
|---|---|
| admission: `LOGIN_NEEDED`, `DISK_LOW`, `MEM_LOW`, `HOST_BUSY`, `--force`, the load without drover's own processes | `hosts::gate`, `login_hold`, `disk_low`, `host::sample`, `pace`, `window`, `admit` |
| start, working and simulator slots | `starts.rs`, `hosts::held_slots`, `slot_before`, `working_full`, `slots_line`; `max_parallel_spawns`, `start_max`, `start_release`, `max_working`, `max_simulators` |
| heavy slots and the `[heavy]` patterns | `heavy.rs`, the hook's rewrite, `drover heavy`, `drover hold`, `[fleet.<n>.heavy]`, `heavy_max`, `heavy_kind_max`, `heavy_wait_minutes` |
| lane-hog and `drover top` | `check::hogs`, `unhog`, `host::lane_cpu`, `top.rs` |
| disk: the budget, build trim, `reclaim`, `clean`, strays, orphans, the scratch sweep of this host | `check::budget`, `trim_builds`, `reclaim`, `clean_all`, `strays`, `orphans`, `sweep` |
| workspace roots and `hosts eject` | `roots.rs` |
| tags, needs and `NO_HOST` when the main host lacks one | `hosts::probe`, `tags`, `needs`, `repro_needs` |
| the Host tab's card, its history and charts | `hosts::sample`, `record`, `Row`, `spark`, `series`, `chart`, `cards`, `shown` |
| the runtime breaker and its canary | `breaker.rs` |
| self-update of the main host, the serve handover, the staging serve | `update.rs`, `release.rs` |
| the view of lane facts, fed by this host's own watcher | `link/`, `drover hosts report` and `watch`, the service's one link, every `link_*` limit |
| shed on the main host: sessions, ACP lanes, `ACP_UNGUARDED`, setup terminals | `shed.rs`, `session/`, `term.rs`, `spawn::unguarded`, `setup.rs` |
| a lane's secrets file | `secret.rs`, with the local half of `On::write_private` as a plain function |

## Tests: files that go whole (6,014 lines)

| Slice | Files (lines) |
|---|---|
| 1 | `drive/shed_machines.rs` (39) |
| 2 | `cloud.rs` (1,400), `boat.rs` (442) |
| 3 | `remote_acp.rs` (1,297) |
| 4 | `outbox.rs` (182), `paused_delivery.rs` (116) |
| 5 | `ssh_sessions.rs` (158) |
| 6 | `stall_move.rs` (220) |
| 7 | `host_deadline.rs` (323), `host_spared.rs` (158), `stalled_host.rs` (155), `tick_dead_host.rs` (150), `ssh_deadline.rs` (83), `pass_run_for.rs` (52) |
| 8 | `lane_dead_remote.rs` (261), `remote_exists.rs` (195), `hosts/report.rs` (73), `hosts/upstream.rs` (77), `hosts/warm.rs` (100), `hosts/wip.rs` (139), `drive/remote_unlisted.rs` (55), `drive/rev_unlisted.rs` (28) |
| 10 | `runners.rs` (311) |

All paths are under `crates/cli/tests/`. A top-level file's line in the shard list of `scripts/test-shards.d/`
goes with it. `ssh_deadline.rs` pins the text of the ssh call sites in `hosts.rs`, `hosts/probe.rs` and `proxy.rs`:
the first slice to change one of them (3 at the latest) removes that site from it.

## Tests: files that shrink (≈ 7,900 lines out)

| File | Lines | ≈ out | Slice | What stays |
|---|---|---|---|---|
| `hosts.rs` | 1,740 | 1,650 | 4 to 8 | `host_busy_names_the_lane_holding_the_cpu`; the main-host half of the tag, needs and admission tests |
| `host_offline.rs` | 846 | 825 | 7 | `a_call_is_killed_at_its_limit` |
| `scratch.rs` | 815 | 513 | 8 | every test of this host's sweep |
| `link.rs` | 480 | 430 | 4 | the service test, on a main-host fixture |
| `remote_rules.rs` | 497 | 430 | 4 | the local half of the leftover secrets files test |
| `faults.rs` | 523 | 394 | 4, 5, 8 | the five tests on this host's shed link |
| `host_sync.rs` | 496 | 380 | 1 | the unsigned-binary and missing-asset tests of the main host |
| `perf.rs` | 1,451 | 389 | 4, 8 | the fixture without its remote host |
| `remote_verdict.rs` | 384 | 300 | 8 | merge during a pending admission; a fix round after a full-disk step |
| `shed.rs` | 2,494 | 221 | 2, 7, 8 | all else |
| `drive_kicks.rs` | 1,495 | 210 | 6 | all else |
| `tick_stuck.rs` | 421 | 182 | 7 | the stuck-tick tests, on a main-host fixture |
| `shed_batch.rs` | 206 | 157 | 7 | the local socket test |
| `clone_heal.rs` | 439 | 151 | 8 | all else |
| `host_down.rs`, `offline_tick.rs` | 397 | 260 | 7 | the two kick lease tests; the tick budget test |
| `orphans.rs` | 408 | 105 | 8 | all else |
| `drive_sim.rs`, `sim/whatif.rs` | 731 | 139 | 6, 7 | all else |
| `disk.rs` | 857 | 94 | 6 | all else |
| `common/mod.rs`, `common/fake_shed.rs` | 1,290 | 171 | 9 | `boat`, `OverSsh` and the ssh batch branches go |
| 24 more files, under 90 lines out each | | 900 | by area | `host_slots.rs`, `repro_host.rs`, `lane_host_move.rs`, `hosts_screen.rs`, `shed_others.rs`, `shed_version.rs`, `api_v1_ops.rs`, `setup_terminal.rs`, `doctor.rs`, `hook.rs` and others |

Three files named for hosts are main-host tests and stay as they are: `host_tab.rs`, `host_words.rs` (one
`NO_HOST` row changes) and `host_history.rs` (its second host's rows go).

## Two rules for the tests

- **T1. A test with a main-host half keeps it.** The remote lines leave the test; the test stays. Where its
  fixture is remote only, the fixture is cut down to the main host in the same slice. Examples:
  `link.rs`'s service test, `hosts.rs`'s lane-hog test, `perf.rs`'s `fixture`, `common`'s `v1_board_on`.
- **T2. A main-host behaviour that is tested only through a remote lane keeps its test.** The lane in the test
  becomes a local one: its `machine` line and its fake ssh go. Known cases: a wip push retried under a new name,
  `kill` saving work before it closes the pane, a supersede saving dirty work first, the close of a killed
  lane's leftover shell pane, the leak check of a supersede's snapshot (`hosts.rs`, `hosts/wip.rs`); the setup
  terminal tests (`setup_terminal.rs`, `api_v1_ops.rs`); the build-trim order (`scratch.rs`).

So a slice may delete a test, cut lines from one, or cut its fixture. It adds none, but slice 6's one test of
the dropped `machine` entry.

## Docs, generated files, lists

The sections named here are files of `docs/hosts/`, and the refusal files are under
`roles/usage/30-refusals/`.

| Slice | Docs that go | Regenerated |
|---|---|---|
| 1 | the first part of `update.md`; `HOST_UPDATING.md` | `docs/config/limits.md`, `fleet.example.toml` |
| 2 | `cloud.md`, `bootstrap.md` | the same, `assets/types/api.d.ts`, the hosts screen golden |
| 3 | `acp.md`, the proxy part of `docs/accounts.md`, the example's `50-proxy.toml` part, `NO_PROXY_TOKEN.md` | the same |
| 4 | `remote-asks.md`, `hook-rules.md` | `docs/config/limits.md`, `fleet.example.toml` |
| 5 | `health.md`, the soak half of `soak.md` | `assets/types/api.d.ts` |
| 6 | `off.md`, most of `placement.md`; `DESIGN_MAIN_ONLY.md` | |
| 7 | `add.md`, `limits.md`, the rest of `update.md`, the offline parts of `monitoring.md` and of the troubleshooting page; `HOST_OFFLINE.md` | `docs/config/limits.md`, `fleet.example.toml`, `docs/flow.md` |
| 8 | the remote lines of `roots.md` and of `docs/lanes/` | `docs/config/limits.md`, `fleet.example.toml` |
| 9 | the three stale lines of the index `docs/hosts.md` | `assets/types/api.d.ts` |
| 10 | `runners.md` | `fleet.example.toml` |

`docs/hosts.md`, `events/passive.rs` and `events/push.rs` are frozen lists: a slice may take lines out of them
or replace a line with one, never add one. The usage doc is rebuilt from `cli.rs` and `roles/usage/`.

arch-check reads this folder like any doc: a `drover` command in backticks must exist, and so must a path
from the repo root. So these pages name the commands and files a slice deletes without the `drover` prefix and
without their root folder. No slice has to edit them to stay green.
