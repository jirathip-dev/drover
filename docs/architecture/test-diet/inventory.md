# test diet: the inventory by test file (#1760)

Every Rust file under the cli crate's `tests` folder on staging `4cf52e4bf`, 288 files, 94,506 lines. File names
are relative to that folder. The server crate's one test file (431 lines) stays as it is.

How to read a row:

- **today**: lines on staging. **after**: lines once the decided cuts and this diet have both run. Estimates,
  from counted lines; nothing was compiled.
- **keep**: its tests each assert a behaviour at an edge that no other test asserts.
- **table**: tests that differ only in their inputs become rows of one table test; a row's label is the old name.
- **merge → x**: the tests stay and move onto `x`'s fixture and binary, or onto the shared builders.
- **delete**: `dup` (another test, named in the note, asserts the same at the same or an outer layer), `impl`
  (asserts source text, a private function or a call order nobody sees), `dead` (code retired, or a one-off
  migration whose window has passed).
- **leaves #N**: another decided cut deletes or rewrites it. The diet does not touch those lines.
- **soak**: its full size moves to a soak target, off the PR path, and a small check stays (slice 2). This
  changes time, not lines.
- **retired**: the owner dropped the feature in 2026-11-25; its tests and its src go in slice 5.

A test that `.drover/rules.md` names as an enforcer stays whatever else is true: `runner_limits.rs`, the
request-path guard in `serve_health.rs`, `contract_v1.rs`, `heavy.rs`'s dropped-job test, and the tests CI's `ui`
and `ui-webkit` jobs run.

## Totals

| Area | Files | today | after the decided cuts | after the diet |
|---|---|---|---|---|
| Drive loop | 19 | 9,084 | 8,874 | 7,217 |
| Review, verify, design, merge | 22 | 9,863 | 9,010 | 7,603 |
| Shed sessions and lane lifecycle | 26 | 6,865 | 5,622 | 4,262 |
| Hook, spawn, lane verbs, jobs | 14 | 7,468 | 6,345 | 4,644 |
| Check, clean, disk, host | 30 | 11,227 | 6,880 | 5,666 |
| Setup, state, config, the repo itself | 39 | 8,356 | 7,279 | 6,048 |
| Asks, cards, queue | 21 | 5,509 | 5,106 | 4,410 |
| Serve, API, streams | 26 | 6,293 | 6,210 | 4,726 |
| Screens, page, terminal board, themes | 28 | 8,113 | 7,367 | 6,071 |
| Threads, chat, orchestrators | 18 | 7,055 | 1,964 | 1,451 |
| Simulation, perf, the shared harness | 23 | 8,659 | 7,530 | 5,557 |
| Other hosts (whole files of the remote cut) | 22 | 6,014 | 55 | 55 |
| **All** | **288** | **94,506** | **72,242** | **57,710** |

The decided cuts are the rest of #1691, #1571, #1643 with #1587, #1574 and the shed split's test deletes. "After
the diet" counts the owner's retirements of 2026-11-25 as deleted: the pre-rename move (287), the form route (500, in
the serve total, not split by row) and the sim owner tools (1,296). `adr.md`'s figures add the server crate's 431
lines and take out about 600 lines of #1571 and #1756 that the rows do not split out: 72,100 after the decided
cuts, 59,700 after the audit's verdicts, 57,600 with the retirements.

## Drive loop

| file | today | verdict | after | note |
|---|---|---|---|---|
| `drive.rs` | 2,980 | delete 6 `dup`; table (31 tests → 11); builders | 2,240 | Gone: `clean_reclaims_merged_lanes`, `a_pr_head_with_a_passed_done_…` (kept by `turn.rs`), `extend_logs_first_…` (by `owner_extend.rs`), `an_exited_reviewer_without_a_verdict_…` (by `rev_no_verdict.rs`), `the_363_sequence_…`, `the_replayed_cooee_1_log_…`. 30 six-line verdict events become calls. |
| `drive_kicks.rs` | 1,500 | leaves #1691 (210); delete 2; table (14 → 4) | 1,110 | Marker staleness, slot priority, pass alerts, kick outcome are tables. The five "waiting names X" tests are distinct. |
| `drive/done_lanes.rs` | 113 | merge → `drive.rs` table | 95 | The hibernated reviewer is a row of `a_written_verdict_is_recorded_…`. |
| `drive/no_baseline.rs` | 33 | table, in `drive.rs` | 0 | The no-repro pair with `NO_BASELINE` swapped in: one row. |
| `drive/undriven.rs` | 85 | delete 1 `dup` | 58 | `a_pass_at_a_red_ci_head_…` is `drive.rs`'s `red_ci_after_pass_…`. |
| `drive/update_limit.rs` | 114 | delete 1 `dup` | 78 | `an_extend_during_a_pass_…` is `owner_extend.rs`'s board tap. The two UPDATE_LIMIT tests stay. |
| `drive_jobs.rs` | 396 | merge, one fixture | 340 | Job runner and sim clock are written twice; 30 lines are a remote host. |
| `drive_loops.rs` | 231 | keep | 231 | Five distinct mechanisms. |
| `turn.rs` | 605 | delete 2 `dup`; table (4) | 550 | `a_read_head_off_the_pr_head_…` is `done_gate.rs`'s push-first test. |
| `done_close.rs` | 156 | keep | 144 | The only cover of the done-lane closer. |
| `blocked_respawn.rs` | 407 | merge → `turn.rs`; delete 2 | 295 | `a_lessons_issue_done_…` (`dup`, `lesson.rs`); `…past_the_first_thousand_…` (`dead`). 55 helper lines copy `turn.rs`. |
| `action_ids.rs` | 213 | keep | 200 | Holds parity row R14. |
| `pending_actions.rs` | 183 | merge → `owner_extend.rs`; delete the race half | 110 | The race is `action_ids.rs`'s `two_passes_racing_…`. |
| `admission.rs` | 154 | merge → `drive_kicks.rs` | 92 | `fake`, `fleets`, `mark` are byte copies. |
| `kick.rs` | 192 | keep | 186 | |
| `in_flight.rs` | 197 | merge → `drive.rs` | 150 | 45 lines copy `drive.rs`'s fan-out fixture. Tests unique. |
| `waiting.rs` | 762 | table; delete 1 `dup` | 677 | One 14-case fixture is asserted three ways. |
| `orphan_issue.rs` | 291 | table | 211 | Five `fires_once` tests are rows. |
| `slo.rs` | 472 | keep | 450 | |

## Review, verify, design, merge

| file | today | verdict | after | note |
|---|---|---|---|---|
| `merge.rs` | 964 | keep; table (6 rows); delete 1 `dead`; leaves #1643 (32) | 870 | `merge_into_fleet_repo_prints_no_source_build_line` asserts a line no code prints. |
| `gate_unit.rs` | 123 | table, in `merge.rs` | 55 | Three of nine restate `required_checks_gate_only_the_latest_named_run`. |
| `overlap.rs` | 1,330 | leaves #1643 (250, the `cap_check_*` tests); table; delete 1.5 `dup` | 950 | |
| `land.rs` | 519 | keep | 490 | |
| `verify_stage.rs` | 985 | table; delete `dup`, `impl` | 760 | `a_verify_pass_at_an_older_sha_…` is `pipeline.rs`'s `two_checks_…` under another stage name. |
| `design_stage.rs` | 1,268 | keep; table (4 ask texts); builders | 1,040 | 24 queue-add blocks; one process-wide lock makes its 27 tests run in a row (43 s). |
| `design_orphan.rs` | 221 | merge → `design_stage.rs` | 190 | |
| `repro.rs` | 369 | table | 265 | `help_agent_documents_…` is a doc string (`impl`). |
| `repro_host.rs` | 134 | leaves #1691 (84) | 50 | |
| `playbooks.rs` | 176 | table | 130 | `setup` and `pass` copy `repro.rs`. |
| `proof.rs` | 151 | table; delete 1 `dup` | 108 | |
| `form_only.rs` | 259 | keep | 232 | |
| `rev_no_verdict.rs` | 206 | merge → one stalls file | 140 | Not duplicates of `drive.rs`; 96 lines are a copied fixture. |
| `stale_rev.rs` | 130 | merge → the same file | 80 | |
| `stalled_lanes.rs` | 232 | merge → the same file | 150 | |
| `pipeline.rs` | 391 | keep; soak for the replay | 395 | The replay folds every prefix of 1,295 events (76 s). PR path: 150 events that hold every label. |
| `commit_status.rs` | 232 | keep; 1 test → the hook table | 198 | |
| `done_gate.rs` | 767 | leaves #1643 (80) and #1691 (108); builders | 510 | The third copy of a real git repo builder. |
| `typed_choice.rs` | 153 | keep | 150 | Its thread setup is rewritten by #1571. |
| `tangelo.rs` | 368 | merge → `serial_groups.rs`; delete 3 `dup` | 265 | A grab-bag; the file dissolves. |
| `serial_groups.rs` | 501 | keep | 490 | |
| `remote_verdict.rs` | 384 | leaves #1691 (299) | 85 | What stays moves onto the drive fixture. |

## Shed sessions and lane lifecycle

| file | today | verdict | after | note |
|---|---|---|---|---|
| `shed.rs` | 2,455 | leaves #1691 (221); delete 12; table (15) | 1,553 | 8 `dup` (the 111-line orchestrator thread test, the start pair, the auth error), 2 `impl` (adapter request shapes), 2 `dead` (the berth aliases). Six orchestrator tests leave with #1571. |
| `shed_batch.rs` | 206 | leaves #1691; delete `impl` | 0 | What is left compares two private functions. |
| `shed_native.rs` | 208 | keep; delete 1 `dup` | 186 | |
| `shed_others.rs` | 182 | leaves #1691 (52); keep | 125 | |
| `shed_pin.rs` | 129 | merge → `shed_version.rs`; delete 1 `dead` | 105 | |
| `shed_version.rs` | 208 | leaves #1691 (66); keep | 142 | |
| `acp_listing/mod.rs` | 61 | keep | 61 | |
| `adapter_death.rs` | 157 | merge → one respawn file | 120 | 95 helper lines repeat `respawn_pace.rs`. |
| `generic_harness.rs` | 90 | keep; delete 1 `impl` | 83 | |
| `term_adapter.rs` | 63 | delete `impl` | 0 | Both tests grep src. Two bans in `.drover/arch.toml` say the same. |
| `lane_states/mod.rs` | 171 | table (2) | 140 | Its board reads go in process: 20 s less. |
| `lane_sandbox.rs` | 90 | keep; leaves the shed split (78) | 12 | |
| `lane_hog.rs` | 106 | keep | 98 | |
| `lane_fold.rs` | 76 | merge → `working_set.rs` | 40 | The same log and the same budget. |
| `lane_host_move.rs` | 107 | leaves #1691 (57); delete `dup` | 0 | |
| `orphans.rs` | 408 | leaves #1691 (105); table (3) | 270 | |
| `parked.rs` | 255 | merge → `shed_native.rs` | 220 | |
| `respawn_loop.rs` | 162 | merge → one respawn file | 140 | |
| `respawn_pace.rs` | 146 | merge → the same file | 95 | 78 of 146 lines are helpers. |
| `runtime_breaker.rs` | 155 | merge → the same file | 110 | |
| `never_started.rs` | 166 | merge → the same file | 125 | |
| `gone_cwd.rs` | 187 | table (2) | 150 | |
| `access.rs` | 214 | table; delete 1 `dup` | 155 | Its banner tests are rewritten by the shed split. |
| `usage.rs` | 172 | delete 1 `impl`; leaves the shed split (100) | 63 | |
| `keychain_jam.rs` | 154 | keep | 154 | |
| `faults.rs` | 537 | leaves #1691 (407); table (3) | 115 | |

## Hook, spawn, lane verbs, jobs

| file | today | verdict | after | note |
|---|---|---|---|---|
| `hook.rs` | 1,564 | table (34 tests) | 920 | About 395 allow/deny rows; 4 copies of the runner and 24 input closures go. Six orchestrator-folder rows leave with #1571. |
| `hook_wrapped.rs` | 164 | merge → the hook table | 80 | |
| `hook_own_folder.rs` | 79 | merge → the hook table | 27 | |
| `spawn.rs` | 888 | delete 4 `impl`; table | 715 | Role-prompt sentences and `help_agent_doc_is_complete`: the brief goldens hold the text. |
| `spawn_guard.rs` | 554 | table (13 refusal rows); delete 3 `dup` | 385 | |
| `spawn_force.rs` | 37 | merge → `spawn_guard.rs` | 0 | One field more than `force_admits_busy_host_…`. |
| `lane_actions.rs` | 990 | table; delete `dup` | 740 | 11 `kill_*` tests over one snapshot function; `act_needs_a_paired_cookie_…` is `api_v1_ops.rs`'s. |
| `hygiene.rs` | 573 | leaves #1643 (128, check and scan); table | 345 | Six leak-guard tests are about 30 rows. |
| `arch_check.rs` | 871 | leaves #1643 | 0 | |
| `lesson.rs` | 675 | leaves #1643 (72); table | 540 | #1756 takes its tick test. |
| `triage.rs` | 290 | keep; 1 test → the hook table | 250 | |
| `deps.rs` | 195 | keep; 1 test → the hook table | 172 | |
| `hotspots.rs` | 375 | keep; builders | 320 | |
| `protection.rs` | 213 | table | 150 | Eleven distinct outputs of one verb. |

`lesson.rs`, `triage.rs`, `deps.rs` and `hotspots.rs` each assert "the tick runs it every N days, files one
issue, never a second": about 160 lines become one table when #1756 lands. Not counted.

## Check, clean, disk, host

| file | today | verdict | after | note |
|---|---|---|---|---|
| `check_clean.rs` | 968 | table (alerts; the clean table with `disk.rs`) | 872 | "A merged lane's leftover files: removed, saved to wip or kept as a secret" is one 16-row table over ten tests of two files. Gains `self_update.rs`'s two scheduled-clean tests (127). |
| `disk.rs` | 857 | leaves #1691 (94); table; delete 1 `dup` | 530 | |
| `scratch.rs` | 815 | leaves #1691 (515); merge → `disk.rs` | 235 | |
| `reclaim.rs` | 231 | keep | 205 | |
| `reap.rs` | 483 | table | 348 | |
| `roots.rs` | 512 | keep | 425 | |
| `hung_root.rs` | 246 | keep | 229 | |
| `hung_read.rs` | 24 | keep | 24 | Its own binary on purpose (it sets HOME). |
| `overloaded.rs` | 85 | table; soak | 65 | 11 s of real waits. |
| `runner_limits.rs` | 378 | keep (enforcer); delete 2 `dup`; 1 test soak | 316 | Its `Command::new` pair is a strict subset of the arch ban; it stays while the rules line names the file. |
| `abandoned_child.rs` | 35 | merge → `runner_limits.rs` | 18 | |
| `heavy.rs` | 465 | keep (enforcer) | 445 | |
| `top.rs` | 295 | keep | 285 | |
| `hosts.rs` | 1,725 | leaves #1691 (1,635); merge → `host_slots.rs` | 65 | |
| `hosts/doctor.rs` | 46 | leaves #1691; merge → `doctor.rs` | 12 | |
| `host_offline.rs` | 858 | leaves #1691; delete `dup` | 0 | The one test left is `runner_limits.rs`'s 50 ms kill. |
| `host_down.rs`, `offline_tick.rs` | 391 | leaves #1691 (251); merge → `tick_stuck.rs` | 60 | |
| `host_slots.rs` | 295 | leaves #1691 (63); keep | 200 | |
| `host_history.rs` | 265 | keep; delete 1 `dup` | 218 | |
| `host_tab.rs` | 108 | keep | 100 | |
| `host_words.rs` | 226 | keep | 226 | Already one 26-row table; as a data file it is 30 Rust lines (decision 9). |
| `host_sample.rs` | 71 | keep | 62 | |
| `host_sync.rs` | 123 | merge → `self_update.rs` | 70 | |
| `flow_facts.rs` | 62 | leaves #1691 | 0 | All three tests judge another host. Its inventory does not name the file: slice 7 there confirms. |
| `tick_stuck.rs` | 421 | leaves #1691 (182); table | 185 | |
| `clone_heal.rs` | 441 | leaves #1691 (151); keep | 254 | |
| `narrow_refspec.rs` | 157 | merge, the shared repo builder | 73 | 22 test lines under 104 helper lines. |
| `update_worktree.rs` | 156 | merge, the shared repo builder | 114 | 39 test lines under 117 helper lines. |
| `remote_rules.rs` | 488 | leaves #1691 (428); merge → the clean tests | 30 | |

## Setup, state, config, the repo itself

| file | today | verdict | after | note |
|---|---|---|---|---|
| `doctor.rs` | 671 | table | 606 | |
| `timer.rs` | 694 | keep | 678 | Takes `timer_install.rs`'s test. |
| `timer_install.rs` | 49 | merge → `timer.rs` | 0 | |
| `self_update.rs` | 748 | table; 2 tests → the clean tests | 532 | No test here is about another host. |
| `migrate.rs` | 360 | retired (240) | 120 | The one-off `herdsman` move. Four tests are not about the rename and stay. |
| `fleet_rename.rs` | 273 | keep | 264 | |
| `state_db.rs` | 213 | keep; delete 2 `impl` | 174 | One is ignored and asserts nothing; one pins an index name. |
| `state_write.rs` | 121 | keep | 119 | |
| `store.rs` | 63 | keep; 1 retired with the rename | 43 | |
| `events_log.rs` | 150 | table | 142 | The kill-9 loop runs one round on a PR. |
| `compaction.rs` | 317 | keep | 317 | 2 seeds on a PR, 20 in soak. |
| `compact.rs` | 397 | leaves #1574 | 0 | |
| `working_set.rs` | 61 | keep | 61 | |
| `tmpdir.rs` | 47 | keep | 47 | |
| `wedged.rs` | 149 | table | 133 | |
| `limits.rs` | 139 | leaves #1691 (35); delete 2 `dup` | 85 | |
| `retired_keys.rs` | 66 | keep | 69 | The one home for "a retired key loads with a warning"; four other files repeat it. |
| `set_notify.rs` | 127 | table | 112 | |
| `fleet_notify.rs` | 79 | table | 62 | |
| `empty_env.rs` | 17 | delete `dup` | 0 | `migrate.rs` asserts it through the real binary. |
| `sbin_path.rs` | 17 | keep | 17 | |
| `config_invalid.rs` | 86 | keep | 86 | |
| `m3.rs` | 473 | delete 3; the rest → `pr_snapshot.rs`, the queue tests | 350 | The only tests of `wait`, the PR cache, queue add/mv/rm. Wrong file, right tests. |
| `m3_fixes.rs` | 139 | merge → `spawn_guard.rs`, the clean tests | 130 | The only `REFUSED MERGED` test. |
| `readme.rs` | 275 | delete 3 `dup`, 2 `impl`; leaves #1643 (105) | 100 | `doc_links_resolve` is arch-check's doc rule; the leak tests are CI's leak gate. `table_commands_exist` is not covered and stays. |
| `parity_check.rs` | 108 | delete 1 `dup`; leaves #1643 | 0 | |
| `ci_split.rs` | 221 | delete 1 `impl`; leaves #1587 | 0 | |
| `ci_templates.rs` | 53 | delete `impl` | 0 | Pins strings of workflow templates. |
| `ci_stuck.rs` | 95 | keep | 78 | |
| `setup_neutral.rs` | 148 | leaves #1643 | 0 | Not a duplicate today: the arch bans do not cover roles or docs. |
| `brief_golden.rs` | 125 | delete 2 `dup` | 79 | The golden compare is `just gen`'s writer and stays. |
| `diagram.rs` | 88 | delete `dup`, `impl` | 60 | |
| `flow_table.rs` | 145 | keep | 143 | |
| `baseline.rs` | 190 | keep | 170 | |
| `pr_snapshot.rs` | 168 | keep | 160 | |
| `webhook.rs` | 298 | keep | 278 | |
| `webpush.rs` | 653 | keep; builders | 500 | Seven copies of the subscribe fixture. Two thread tests (95) leave with #1571. |
| `report.rs` | 112 | keep | 112 | |
| `autonomy.rs` | 221 | keep | 221 | |

## Asks, cards, queue

| file | today | verdict | after | note |
|---|---|---|---|---|
| `asks.rs` | 638 | table; delete 1 `dup`; builders | 540 | |
| `answer_delivery.rs` | 290 | merge → `asks.rs` | 200 | The three undelivered-answer tests are three cases, not duplicates. |
| `ask_endings.rs` | 300 | keep; delete 1 `dead`; leaves #1571 (49) | 200 | The one-time sweep of #584. |
| `ask_ids.rs` | 289 | keep; delete 1 `impl`, 1 `dup` | 243 | `no_ask_id_is_counted_from_the_log_in_core` greps src. |
| `ask_scale.rs` | 91 | keep at 2,000 events; 60,000 in soak | 86 | The bound is a ratio, so the small log proves the same. |
| `lane_asks.rs` | 402 | leaves #1571 (129, thread routing); table | 236 | |
| `extend_ask.rs` | 220 | merge → `owner_extend.rs`; table | 170 | |
| `owner_extend.rs` | 390 | keep; delete 1 `dead`; table (3) | 325 | The one-off requeue of #1058. Extend is 35 tests across nine files today, 24 after. |
| `needs_human.rs` | 286 | keep; delete 2 `dup`; table | 205 | |
| `queue_blockers.rs`, `queue_board.rs`, `queue_cross_repo.rs`, `queue_graph.rs`, `queue_lanes.rs`, `queue_titles.rs` | 966 | merge → one queue file | 800 | They share little setup; they share a binary and the HTTP helper. `queue_board.rs` loses 28 lines with #1571. |
| `backlog.rs` | 228 | keep | 215 | |
| `amendments.rs` | 99 | keep | 99 | |
| `rollout.rs` | 230 | keep; leaves #1571 (15) | 205 | |
| `run_card.rs` | 514 | keep | 480 | |
| `config_card.rs` | 345 | leaves #1571 (150); keep | 185 | |
| `config_api.rs` | 221 | keep | 221 | |

`kinds` is defined in 10 of these files and a raw HTTP request is built by hand in 11; the needs-human fixture is
rebuilt in three of them and in four files of other areas.

## Serve, API, streams

| file | today | verdict | after | note |
|---|---|---|---|---|
| `serve_web.rs` | 809 | table; delete 1 `dup` | 630 | `parity_81` is also `screens_parity.rs`'s. `parity_41` and `parity_42` are sole row holders and stay. |
| `serve_health.rs` | 369 | keep (enforcer) | 340 | One test really sleeps 3 s five times. |
| `serve_conn.rs`, `serve_load.rs`, `serve_workers.rs` | 427 | merge → one limits file | 350 | 15 bounds asserted, 11 distinct. `serve_workers.rs` waits 50 s on a missing `Connection: close`. |
| `serve_wire.rs` | 224 | keep; delete 1 `impl` | 180 | `wire_report` prints and asserts a 200. |
| `serve_working_set.rs` | 70 | keep | 70 | |
| `live.rs` | 439 | builders; delete 1 `dup` | 345 | Rebuilds the shared board by hand. |
| `handover.rs` | 625 | builders | 520 | Seven distinct process behaviours. |
| `service.rs` | 187 | keep | 187 | |
| `api_v1.rs` | 357 | merge → `api_v1_ops.rs`; delete 1 `dup` | 290 | |
| `api_v1_contract.rs` | 296 | keep; delete its self-test | 205 | The only test that every route answers its type's shape. |
| `api_v1_ops.rs` | 320 | delete 3 `dup` | 195 | `send_…`, `act_…`, `setup_…` restate `lanes_sheet.rs`, `lane_actions.rs`, `setup_terminal.rs` through the same handler. |
| `contract_v1.rs` | 223 | keep (enforcer) | 223 | |
| `history.rs` | 38 | delete `dup` | 0 | The last block of `serve_working_set.rs`. |
| `log_share.rs` | 74 | delete 1 `impl`; merge 1 → `ask_scale.rs` | 28 | |
| `settled_snapshot.rs` | 156 | soak; delete `settled_reads_cost` | 147 | That test asserts an exit code only: 41 s. |
| `stream_quiet.rs` | 240 | delete 1 `dup` | 200 | Most of the rest leaves with #1571. |
| `screen_hub.rs` | 290 | release job only (joins `perf`) | 290 | The only file run in both the debug shards and the release job. |
| `screen_pages.rs` | 335 | keep; delete 1 `impl` | 318 | |
| `lifeboat.rs` | 183 | keep | 183 | |
| `pair_status.rs` | 128 | keep | 128 | No twin. |
| `scene_device.rs` | 166 | builders | 125 | |
| `setup_terminal.rs` | 158 | keep | 145 | |
| `cooee_token.rs` | 94 | table; delete 1 `impl` | 72 | 25 s on a missing `Connection: close`. |
| `remote_sheet.rs` | 85 | merge → `lanes_sheet.rs` | 55 | |

The form route is retired (owner, 2026-11-25), once slice 5 has found no client that calls it: `serve_web.rs`'s form tests
and the tests that write through it elsewhere go, about 500 lines. The handler behind the route stays, because
the screens' actions call it: a form test with an action twin is a `dup`, one without moves onto the action
route. The write half of `/api/v1` stays (#1627 builds on it).

## Screens, page, terminal board, themes

| file | today | verdict | after | note |
|---|---|---|---|---|
| `cooee_screens.rs` | 1,422 | table (a golden loop); delete 2; leaves #1571 (55) | 1,135 | `cooees_queue_stream_fixture_applies_in_place` runs no drover code. `parity_63` (dropped by the owner 2026-11-25, slice 5) is held by `an_action_outside_the_manifest_…`. |
| `lanes_sheet.rs` | 860 | merge (the pairing half → `pair_status.rs`); delete 1 `impl` | 730 | The lane-prompt refusals are asserted three times. |
| `events_tab.rs` | 273 | table | 160 | `parity_83` is 20 rows in 155 lines. |
| `fleet_cards.rs` | 208 | keep | 175 | |
| `gh_links.rs` | 134 | keep | 120 | |
| `hosts_screen.rs` | 141 | leaves #1691 (46); merge → `screens_parity.rs` | 70 | |
| `screens_parity.rs` | 257 | keep | 230 | |
| `board_errors.rs` | 42 | merge → `screens_parity.rs` | 30 | |
| `board_fresh.rs` | 160 | merge → `serve_health.rs` | 140 | |
| `board_style.rs` | 100 | merge → `cooee_screens.rs` | 45 | Its greps of the built board file are `impl`. |
| `board_tui.rs` | 545 | keep (moves with #1759) | 500 | |
| `local_screens.rs` | 96 | merge → `board_tui.rs` | 65 | |
| `page.rs` | 112 | keep (CI `ui`) | 112 | |
| `page_drafts.rs` | 512 | leaves #1571 (300, six thread tests); builders | 150 | |
| `parity_webkit.rs` | 488 | leaves #1571 (270, ten thread tests); delete 2 `dup` (`parity_2`, `parity_20`: dropped by the owner 2026-11-25, slice 5) | 160 | `parity_r25` is the sheet-close enforcer. |
| `issue_sheet_webkit.rs` | 128 | builders | 95 | |
| `keyboard_webkit.rs` | 76 | leaves #1571 | 0 | |
| `url_links.rs` | 42 | merge → `parity_webkit.rs` | 25 | |
| `phone_timings.rs` | 93 | keep | 93 | The 150 ms product gate. |
| `round_images.rs` | 190 | keep | 175 | |
| `schemes.rs` | 280 | delete 1; trim | 220 | `github_dark_is_todays_board` re-tests the pack's tokens. |
| `themes.rs` | 740 | table; delete the pack's own content | 500 | Four tests re-test the pinned pack's files; one loop over the packs replaces them. |
| `herd.rs` | 496 | delete 1 `dup`; table (2) | 450 | |
| `demo_export.rs` | 227 | keep; delete 1 | 230 | |
| `ui_audit_hang.rs` | 75 | soak | 75 | Tooling; 20 s of every run. |
| `ui_lock.rs` | 106 | keep | 104 | |
| `ui_tokens.rs` | 190 | keep | 162 | |
| `verify_browser.rs` | 120 | keep | 120 | |

The page-client tests (`page.rs`, `page_drafts.rs`, `parity_webkit.rs`, `issue_sheet_webkit.rs`, `url_links.rs`,
`phone_timings.rs`, `round_images.rs`, `ui_tokens.rs`, the WebKit half of `herd.rs`: about 1,700 lines after the
thread cut) test how a client draws, not what drover builds. #1665 moves that onto blocks' snapshot tool
(decision 9).

38 `parity_*` tests are in this area. Three are held whole by another test and their rows are dropped (owner, 2026-11-25);
none of the others can go without its row being dropped.

## Threads, chat, orchestrators

| file | today | verdict | after | note |
|---|---|---|---|---|
| `chat_tab.rs` | 1,241 | leaves #1571 | 0 | |
| `threads_tab.rs` | 1,185 | leaves #1571 (981); the card tests → `cooee_profiles.rs` | 100 | |
| `thread_handoff.rs` | 848 | leaves #1571 and #1610 (645); table on rows | 130 | |
| `thread_refs.rs` | 85 | leaves #1571 | 0 | |
| `chat_pick.rs` | 338 | leaves #1571 | 0 | |
| `chat_tui.rs` | 467 | leaves #1571 | 0 | |
| `chat_index.rs`, `chat_scale.rs` | 149 | leaves #1571 | 0 | |
| `orchestrator.rs` | 396 | leaves #1571 (364); 1 test → the spawn tests | 27 | |
| `orchestrator_adopt.rs`, `orchestrator_cold_start.rs` | 301 | leaves #1571 | 0 | |
| `orchestrator_attach.rs` | 172 | merge → `cooee_profiles.rs` | 150 | |
| `orchestrator_cards.rs` | 548 | leaves #1571 (209); merge → `config_card.rs`; delete 1 `dup` | 128 | The run card is held by `run_card.rs`. |
| `orchestrator_default.rs` | 170 | leaves #1571 (78); keep | 90 | |
| `orchestrator_lanes.rs` | 173 | leaves #1571 | 0 | Inferred: its rows have no writer after the cut. #1571 confirms. |
| `orchestrator_names.rs` | 236 | leaves #1571 (60); keep | 176 | |
| `orchestrator_reviews.rs` | 341 | merge → `lane_asks.rs`; table | 250 | 125 helper lines repeat `lane_asks.rs`. |
| `cooee_profiles.rs` | 405 | keep | 400 | |

## Simulation, perf, the shared harness

| file | today | verdict | after | note |
|---|---|---|---|---|
| `sim/main.rs` | 732 | keep 6 tests on the PR path; the hunt self-test to `sim_soak`; the tools' entry points and self-tests retired (575) | 125 | 496 s per run today, about 128 s after. The hunt self-test alone is 215 s; the tools' self-tests 115 s. |
| `sim/model.rs`, `sim/sched.rs`, `sim/scenario.rs`, `sim/invariants.rs`, `sim/gates.rs`, `sim/seeds.rs` | 2,045 | keep | 2,045 | The real drive code under a simulated world; no rule is written twice. |
| `sim/hunt.rs` | 350 | keep | 350 | Half of it is the hunt tool. |
| `sim/replay.rs`, `sim/whatif.rs`, `sim/metrics.rs` | 745 | retired | 0 | Owner tools, not tests: with their entry points and self-tests, 1,320 lines. The nightly metrics table goes with them. |
| `drive_sim.rs` | 500 | leaves #1691 (115); delete `dup` | 0 | A second simulator. Draft PR: `drive.rs`. Quiet reviewer: `stale_rev.rs` and `rev_no_verdict.rs`. Shared files: `disk.rs`, which gains one assertion. |
| `perf.rs` | 1,492 | leaves #1691 (389); merge; 6 tests release only | 850 | A soak target. Three tests are not perf and move to their areas. `parity_r7` joins the request-path guard. |
| `perf/screens.rs` | 201 | keep; release only | 165 | |
| `pass_budget.rs` | 113 | keep | 95 | |
| `pass_profile.rs` | 58 | merge → `pass_budget.rs` | 0 | A manual profiling loop. |
| `soak.rs` | 273 | keep | 235 | Already ignored by default; becomes the `soak` target that takes the full-size runs. |
| `link.rs` | 480 | leaves #1691 (430) | 50 | |
| `common/mod.rs` | 858 | leaves #1691 (98); delete `dead` (90) | 670 | No caller: `chat_fixture`, `chat_scene`, `agents`, `herdr_setup`, `Shell`. |
| `common/fake_shed.rs` | 531 | leaves #1691 (73); keep | 458 | The shed split replaces it later. |
| `common/large.rs` | 69 | merge → the one log builder | 25 | One of seven generators of the same log shape (about 450 lines). |
| `common/recorded.rs` | 59 | keep | 59 | A shared board done right: 13 files read it. |
| `common/remote.rs` | 153 | delete `dead`, after #1691 | 0 | Used only by remote tests; that cut's list does not name it. |
| new: the builders | 0 | add | 430 | The world and the log builder; the client, repo and hook runner bring about 170 more, counted in the areas. |

## Other hosts: whole files of the remote cut (#1691)

All `leaves #1691`, as its own inventory lists them; the diet touches none.

| file | today | after |
|---|---|---|
| `cloud.rs` | 1,395 | 0 |
| `remote_acp.rs` | 1,282 | 0 |
| `boat.rs` | 442 | 0 |
| `host_deadline.rs` | 328 | 0 |
| `runners.rs` | 311 | 0 |
| `lane_dead_remote.rs` | 261 | 0 |
| `stall_move.rs` | 220 | 0 |
| `remote_exists.rs` | 195 | 0 |
| `hosts/wip.rs` | 194 | 55 |
| `outbox.rs` | 182 | 0 |
| `ssh_sessions.rs` | 158 | 0 |
| `host_spared.rs` | 158 | 0 |
| `stalled_host.rs` | 155 | 0 |
| `tick_dead_host.rs` | 147 | 0 |
| `paused_delivery.rs` | 116 | 0 |
| `hosts/warm.rs` | 101 | 0 |
| `ssh_deadline.rs` | 83 | 0 |
| `hosts/upstream.rs` | 78 | 0 |
| `hosts/report.rs` | 73 | 0 |
| `drive/remote_unlisted.rs` | 55 | 0 |
| `pass_run_for.rs` | 52 | 0 |
| `drive/rev_unlisted.rs` | 28 | 0 |

## The duplicate clusters worth knowing

Each was read on both sides. The full lists are the notes above; these are the ones that cross files.

| Behaviour | Asserted in | Kept |
|---|---|---|
| A reviewer quiet two hours is replaced once and its successor's verdict lands | `drive_sim.rs`, `stale_rev.rs`, `rev_no_verdict.rs`, a sim gate scenario | the two small files and the sim |
| A passed draft PR is marked ready once, then merged | `drive_sim.rs`, `drive.rs` | `drive.rs` |
| Extend: logs, requeues, spawns nothing; only in needs-human | `merge.rs`, `owner_extend.rs`, `extend_ask.rs`, `needs_human.rs`, `drive.rs`, `drive/update_limit.rs`, `never_started.rs`, `verify_stage.rs` | 24 of 35 tests: the rule in `merge.rs`, the tap in `owner_extend.rs`, the card in `extend_ask.rs` |
| An alert fires once per state and again when its key changes | `check_clean.rs` and 11 more tests in the drive area, each re-running the pass to prove silence | the once rule in `check_clean.rs`; each kind keeps its threshold and text as a row |
| HOST_BUSY refuses a spawn, starts nothing, forces nothing | `spawn_guard.rs` and 12 tests in the drive area that repeat it before their own point | the gate in `spawn_guard.rs`; one step table per spawn mode |
| What `clean` does with a merged lane's leftover files | three tests of `check_clean.rs`, seven of `disk.rs`, four of `lane_actions.rs` through `kill` | one table on real git; `kill` keeps its push-then-close and its secret refusal |
| A lane is prompted, stopped, or a setup terminal started through the API | `api_v1_ops.rs` against `lanes_sheet.rs`, `lane_actions.rs`, `setup_terminal.rs` | the three specific files |
| A retired config key loads with one warning | `retired_keys.rs`, `doctor.rs`, `self_update.rs`, `access.rs` | `retired_keys.rs` |
| The words of the role prompts and of the agent doc | `spawn.rs` (4), `m3.rs`, `repro.rs`, `design_stage.rs`, `brief_golden.rs` (2) against the brief goldens | the goldens, and `readme.rs`'s check of the README against the binary |
| No process, gh or host sample on a read | `serve_health.rs` (the enforcer), `perf.rs`, `perf/screens.rs`, `serve_web.rs`, `lifeboat.rs`, `pr_snapshot.rs` | the enforcer, widened to any program; the count asserts go, the tests stay |
| The pinned theme pack's own ids, words and colours | four tests of `themes.rs`, one of `schemes.rs`, one of `lanes_sheet.rs` | one loop: every id a theme names is in its sprite file |

## The fixtures that are copied

| What | Where today | Lines | Replaced by |
|---|---|---|---|
| The calm host: three `sysctl` replies, `vm_stat`, `which shed`, `gh issue view` | 93 copies in 67 files | about 600 | the world's default host |
| A lane in the log: a `spawn` event with role, agent, branch, worktree | 1,154 inline literals | not measured | `lane(12, "impl")` |
| A verdict in the log | 152 inline events, six lines each in `drive.rs` | about 600 | `verdict(Pass, head)` |
| A PR's JSON with its check rollup | 68 copies | about 400 | `pr(12, Pr::open(head).ci(Green))` |
| Reading the log by kind: `kinds`, `events`, `logged` | 54 files, 370 lines | 370 | `events(kind)` |
| A clock: `at`, or a ctx cloned with another `now` | 22 files and 14 inline | about 150 | `at(secs)` |
| A bare origin, a clone, a first push; a runner that lets git through | 9 and 6 copies | about 420 | the repo builder |
| A raw HTTP request, its head and body split, JSON parsed | about 40 files | about 900 | the client; it closes its connection |
| A real serve on a socket, or a serve child process | 13 and 5 copies | about 260 | the client's `tcp` and the serve-process helper |
| The hook run with a tool's JSON | 10 copies here, 8 more elsewhere | about 300 | the hook runner |
| A large log of one shape | 7 generators | about 450 | the log builder |
| A browser launch with a paired cookie | 14 copies | about 180 | the board's `webkit` call |
