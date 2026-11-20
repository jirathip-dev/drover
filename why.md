Decided: the fleet pass skips merged and closed issues, as the old fast pass did, so 150 done issues cost no lock and no log fold.
Because: crates/core/src/drive.rs:1263
Decided: a SHED_MISSING spawn refusal holds back the pass's other spawns, like the other host-wide codes.
Because: crates/core/src/starts.rs:168
Decided: handoff checks that an issue is being built before scanning the log for it.
Because: "5 fleets, 40 lanes: all passes done within 10 s"
Rejected: raising the #954 budget — staging's pass passes CI under 75 ms; the merged pass should too.
Rejected: keeping done issues' locks for the sim golden — 150 locks were most of the pass's CPU.
Check: perf a_pass_over_30k_events_and_150_done_issues_stays_fast is green in CI's release perf job.
Check: the sim golden moved only by interleaving; every invariant and gate scenario passes.
Owner: each project's background check is cheap again on a long history with many finished issues.
