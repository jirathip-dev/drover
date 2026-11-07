Closes #570

## For humans
A conductor working inside its state folder can again run commands that hide or merge their output (`2>&1`, `>/dev/null`, `&>/dev/null`).
The hook stops treating those as writes to a file named `&1` or to the folder itself.
Real writes into the state folder from there (for example `echo x > ./a`) are still refused.

## What changed
- `crates/core/src/hook.rs`:
  - New `dups()`: removes file-descriptor duplications (`N>&M`, `>&M`, `N<&M`, `>&-`) and reads `&>` as `>`, so the `&` no longer splits a segment.
  - Redirect targets `/dev/null`, `/dev/stdout` and `/dev/stderr` are never writes.
  - A relative word is resolved against the cwd only if it is a redirect target or an argument of a writer (`cp`, `rm`, `sed -i`, …). Before, every word in a segment with `>` was resolved, including the command name and `$n`.
- `crates/cli/tests/hook.rs`: new test `fd_duplications_write_no_file`, run from a cwd in the conductor's own thread folder and from one in the state folder:
  - Allowed: `echo x 2>&1 | tail -1`, `… >/dev/null 2>&1`, `… &>/dev/null`, `>&2`/`<&0`.
  - Refused: `echo x > ./a`, `2>&1 >./a`, `&>a`, `>&a` from the state folder; `> ../a` and `>/dev/null > ../t5/a` from the own folder.

src/ net +35 (no budget given): one small scanner plus the per-segment redirect-target list.

## Evidence (head ef8463d2c378990ac9e83b0743c07017bff026ff)
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (28673 / 31500)
- `cargo test -p herdsman --test hook` → exit 101: 33 passed, 3 failed. The new test passes.
  - The 3 failures also fail on the base without this change (checked locally).
  - They are `a_closed_deny_logs_one_event_and_alerts_once_per_cause_an_hour`, `a_conductor_command_is_logged_for_its_thread` and `deny_commands_override_per_fleet_and_log_denials`.
  - Probably caused by the lane's environment (`HERDSMAN_OUTBOX` etc.).
- Full `cargo test` (with the lane variables unset) → exit 101.
  - The first run stopped at a failure in `chat_tab.rs` (WebKit scroll).
  - A `--no-fail-fast` rerun was killed partway through, at `tests/herd.rs`. By then 3 more tests outside the hook had failed: the bootstrap install test, a WebKit card test and a serial-busy queue test.
  - None of these touch the hook. CI is the authoritative run.
- CI at ef8463d2c378990ac9e83b0743c07017bff026ff: rust (full build, clippy, tests), web, ui, ui-webkit, evidence, board-freeze → all SUCCESS
