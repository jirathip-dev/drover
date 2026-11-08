   Compiling herdsman v0.1.0 (/Users/jkmacmini/.herdr/worktrees/herdsman/issue-630/crates/cli)
    Finished `release` profile [optimized] target(s) in 2.29s
     Running tests/check_clean.rs (/Users/jkmacmini/.cache/herdsman/target/.scratch/herdsman-impl-630/target/release/deps/check_clean-c70521d10c89ed16)

running 17 tests
test du_counts_blocks_used_not_apparent_size ... ok
test plain_lines_for_merged_stall_needs_human ... ok
test merged_issues_are_never_flagged ... ok
test clean_reclaims_lanes_whose_issue_closed_outside_merge ... ok
test orphan_lane_fires_once_per_agent ... ok
test lane_dead_fires_once_and_again_after_state_change ... ok
test a_blocked_live_lane_is_held_shown_held_and_never_stalls ... ok
test clean_removes_only_merged_clean_idle_lanes ... ok
test an_unheld_idle_lane_stalls_again_every_two_hours ... ok
test ci_missing_fires_once_per_head_after_the_grace ... ok
test working_lane_is_not_a_stall ... ok
test stall_fires_once_and_again_after_state_change ... ok
test ready_unmerged_fires_once_and_again_after_state_change ... ok
test verdict_closes_the_reviewer_lane_unless_dirty_or_working ... ok
test notifier_gets_plain_line_unless_format_json ... ok
test clean_falls_back_to_git_dropping_only_fleets_own_files_and_keeps_going ... ok
test clean_removes_merged_worktrees_holding_only_lane_artifacts_and_unlogged_rev_trees ... ok

test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.67s

     Running tests/hosts.rs (/Users/jkmacmini/.cache/herdsman/target/.scratch/herdsman-impl-630/target/release/deps/hosts-ccf86b5d92760a80)

running 36 tests
test a_disabled_host_gets_no_trim_call ... ok
test a_host_that_cannot_reach_a_repo_is_skipped_for_that_fleet_only ... ok
test a_failed_remote_start_closes_its_workspace ... ok
test a_remote_dry_run_prints_the_argv_the_spawn_sends ... ok
test a_remote_lane_gets_the_service_url_and_its_own_token ... ok
test a_lane_gets_its_token_from_the_retired_accounts_env_before_the_import ... ok
test a_remote_supersede_stays_on_its_host_and_outlives_a_gone_workspace ... ok
test a_remote_lane_knows_the_main_host_and_answers_reach_it ... ok
test a_supersede_onto_another_host_closes_the_old_lane_on_its_own ... ok
test a_remote_lane_runs_every_command_on_its_host ... ok
test an_unreachable_host_costs_one_short_attempt_per_pass ... ok
test a_symlinked_herdsman_updates_and_reads_back_its_real_install_root ... ok
test a_lost_agent_name_retries_once_without_a_double_start ... ok
test a_lost_agent_name_on_the_main_host_retries_or_keeps_the_harness ... ok
test admission_runs_per_host ... ok
test an_unhealthy_host_takes_no_lanes_and_shows_red ... ok
test an_older_herdsman_there_warns_and_is_rebuilt_at_this_sha_once ... ok
test clean_removes_a_remote_worktree_over_its_host ... ok
test clean_lists_and_removes_a_second_hosts_lane_and_orphan ... ok
test asks_stranded_on_a_remote_host_are_imported_once ... ok
test a_supersede_saves_a_remote_lanes_dirty_work_first ... ok
test disk_low_on_a_remote_host_trims_its_shared_build_folder_first ... ok
test doctor_flags_a_stale_remote_fleet_toml_and_leaves_it ... ok
test each_health_check_says_what_failed_and_how_to_fix_it ... ok
test kill_saves_a_remote_lanes_work_on_its_own_host ... ok
test design_runs_on_the_main_host_only_and_a_role_pin_holds ... ok
test remote_shared_build_folders_are_trimmed_with_a_quick_ssh_not_in_the_budget_pass ... ok
test the_least_busy_host_wins ... ok
test tags_are_detected_cached_and_refreshed_hourly ... ok
test needs_pick_the_host_whose_tags_cover_them_else_no_host ... ok
test the_tick_drives_before_it_trims_remote_hosts ... ok
test a_remote_review_is_read_over_ssh_and_recorded ... ok
test an_off_host_is_never_chosen_but_drains_its_lanes ... ok
test an_offline_host_never_pages_and_shows_grey ... ok
test lane_cards_and_the_herd_name_the_host ... ok
test cli_and_board_toggles_both_log ... ok

test result: ok. 36 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.19s

     Running tests/reclaim.rs (/Users/jkmacmini/.cache/herdsman/target/.scratch/herdsman-impl-630/target/release/deps/reclaim-05ab8965ec88193e)

running 4 tests
test the_shared_build_folder_is_trimmed_past_its_budget_except_a_building_profile ... ok
test a_y_answer_applies ... ok
test preview_changes_nothing_and_apply_removes_only_build_folders ... ok
test busy_or_recent_worktrees_are_skipped ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/roots.rs (/Users/jkmacmini/.cache/herdsman/target/.scratch/herdsman-impl-630/target/release/deps/roots-33b0204f2a0fc8b2)

running 5 tests
test a_new_lane_lands_in_the_first_mounted_root_and_skips_a_missing_one ... ok
test migrate_moves_an_idle_worktree_into_the_new_layout ... ok
test a_pulled_root_is_noted_once_and_its_lane_restarts_on_the_next_root_from_its_pushed_head ... ok
test eject_saves_and_restarts_a_live_lane_on_the_next_root_then_reports_safe ... ok
test a_new_lane_clones_its_fleets_warmest_build_on_its_root_and_gets_the_included_files ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.62s

     Running tests/scratch.rs (/Users/jkmacmini/.cache/herdsman/target/.scratch/herdsman-impl-630/target/release/deps/scratch-0f1029781eda161d)

running 5 tests
test the_oldest_scratch_is_trimmed_over_budget ... ok
test clean_removes_stray_cargo_folders_and_claude_temp_dirs ... ok
test drive_deletes_an_ended_lanes_scratch ... ok
test a_host_short_of_disk_drops_non_working_scratch_largest_first ... ok
test a_remote_hosts_ended_and_orphan_scratch_goes_at_the_tick ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s

     Running tests/shed.rs (/Users/jkmacmini/.cache/herdsman/target/.scratch/herdsman-impl-630/target/release/deps/shed-d3fc65e048642fb2)

running 23 tests
test another_host_is_reached_through_shed_connect_over_ssh ... ok
test another_product_at_the_same_version_is_refused ... ok
test a_wait_that_times_out_answers_as_herdr ... ok
test another_shed_version_is_refused ... ok
test an_unguardable_acp_conductor_refuses_with_a_system_line_and_never_falls_back_to_herdr ... ok
test a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it ... ok
test a_lane_spawns_on_shed ... ok
test an_unguardable_acp_lane_refuses_to_start ... ok
test every_adapter_call_is_a_shed_request ... ok
test the_default_runtime_is_herdr ... ok
test a_shed_acp_fleet_starts_its_lane_as_an_acp_session_with_the_brief_as_its_first_prompt ... ok
test a_remote_host_without_shed_is_up_when_its_herdr_answers ... ok
test the_build_trim_keeps_a_running_shed_lanes_scratch ... ok
test the_lanes_tab_lists_a_running_shed_acp_lane_live ... ok
test a_shed_acp_fleets_thread_runs_its_conductor_as_an_acp_session_that_sleeps_and_restarts ... ok
test an_acp_lanes_state_follows_its_turn_events ... ok
test the_old_kind_and_env_still_work_with_a_warning ... ok
test the_lane_hook_reports_its_state_to_shed ... ok
test an_acp_conductor_runs_under_the_conductor_guard ... ok
test a_guarded_command_is_refused_inside_an_acp_lane ... ok
test drive_reads_berths_seq_file_once_when_sheds_is_missing ... ok
test drive_logs_sheds_state_changes_since_its_last_pass ... ok
test the_service_follows_sheds_events_and_says_when_the_stream_drops ... ok

test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.11s

     Running tests/spawn.rs (/Users/jkmacmini/.cache/herdsman/target/.scratch/herdsman-impl-630/target/release/deps/spawn-36a19b714f083a7a)

running 26 tests
test clone_build_clones_on_a_lockfile_match_and_logs_failures ... ok
test fleet_env_expands_herdsman_port ... ok
test fleet_env_is_exported_into_the_lane ... ok
test claude_role_dry_run_uses_synced_profile ... ok
test hermes_role_dry_run_kicks_prompt_after_boot ... ok
test cap_asks_only_when_your_own_pr_crosses ... ok
test help_agent_doc_is_short_and_complete ... ok
test conflict_predicted_is_not_a_hold ... ok
test lane_title_strips_prefixes_and_cuts_at_words ... ok
test lane_port_is_stable_per_fleet_and_branch ... ok
test each_lane_gets_its_own_cargo_target_under_its_root ... ok
test each_lane_gets_a_private_scratch_dir_under_its_roots_build_folder ... ok
test claude_dry_run_names_every_role ... ok
test lockfiles_match_needs_an_identical_lockfile ... ok
test effort_maps_to_each_harness_flag ... ok
test rev_role_flags_new_commit_trailers_only ... ok
test invalid_effort_is_refused_at_config_load ... ok
test shared_cache_is_exported_into_the_lane ... ok
test profiles_sync_removes_a_stale_generated_profile ... ok
test profiles_turn_attribution_off_and_role_md_has_the_rule ... ok
test profiles_sync_gives_a_verify_profile_its_own_prompt ... ok
test lane_port_collision_takes_the_next_free_one ... ok
test reopen_spawn_clones_no_build_folder ... ok
test unconfirmed_hermes_kick_presses_enter_then_exits_4 ... ok
test real_claude_spawn_writes_brief_and_logs_event ... ok
test failed_clone_is_logged_and_the_spawn_succeeds ... ok

test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s

