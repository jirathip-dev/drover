Closes #1138

## For humans
An issue whose last lane ran on a host that lacks the tags it now needs (for example a fleet moved to the dev VMs) is placed on a host that has them, instead of going back to the old one.
It stays on the old host only when its worktree there still holds unpushed work, and the spawn then says so.

## What changed
- `place()` computes `needs()` before the last-host block and reuses the last spawn's host only when its tags cover the needs and it is not busy/offline (as before), or when `check::unsaved` finds unpushed work in its worktree there. That check now runs when the host is busy **or** lacks the tags. When it stays, the note `stays on <host>: its worktree there has unpushed work` is appended to the refusal (existing DISK_LOW case) or, with no refusal, set as the placement's `why`, which the spawn event records.
- The `fits` filter reuses the same tag check closure.
- `--host`, role pins and design/architect lanes return before this block and are unchanged.

## Repro
Commit 1 (99f47f644) adds only `a_last_host_without_the_needs_tag_yields_unless_its_worktree_has_unpushed_work` in crates/cli/tests/hosts.rs. On the old code it fails: `cargo test -p drover --test hosts a_last_host` → exit 101, `left: Ok("bazzite") right: Ok("mini")` ("no xcode on bazzite").

## Evidence (head 8a66c35265416e3929298129bd415af2baa0fd3d)
- `cargo test -p drover --test hosts` → exit 0 (45 passed, including the new test and the existing sticky, hosts-off and DISK_LOW cases)
- `cargo fmt --check` → exit 0
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `cargo run -q -p drover -- cap-check --config ci/cap.toml` → exit 0 (45693 / 47200)
- `cargo test --workspace` → exit 101 twice, each time from a different test that is unrelated to this change and timing-sensitive on the busy shared host: `limits::ssh_connect_secs_sets_the_connect_timeout_a_spawn_plan_prints` on run 1, `handover::a_serve_started_by_a_handover_hands_over_again_past_its_memory_limit` on run 2. Each passes alone: `cargo test -p drover --test limits` → exit 0, `cargo test -p drover --test handover` → exit 0.

src/ net +3 (budget none stated): moving `needs()` up and adding the tag check plus the note branch.
