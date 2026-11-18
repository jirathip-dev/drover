Closes #1243

## For humans
Scheduled drover jobs now always point at the real installed program, never the shortcut in `~/.local/bin`, whichever path you ran the install from.
After drover updates itself, any job macOS refused to start because of a stale signature is unloaded and loaded again, so it starts running again.
`drover doctor` now shows a red row for a job in that state, with the reinstall command as the fix.

## What changed
- `timer::expect` resolves the binary through symlinks before writing the plist/unit, so every dev.drover.* job names the install root's binary (AC1). `drift` compares against the same path, so the tick's repair rewrites a tick/fast job that names the symlink.
- `timer::refused` probes `launchctl print` (or `systemctl --user is-active`) for `spawn failed` / `OS_REASON_CODESIGNING`. `timer::restart` bootouts and bootstraps such a job instead of running `kickstart -k`. After the serve handover, self-update re-bootstraps every refused tick/fast/update/clean job via `timer::heal` (AC2).
- Doctor's timer rows (`timer::states` = drift + refused) show `✗ timer — <label> spawn failed → drover timer install …` (AC3). `timer install` no longer treats a refused job as live, so it reloads the job instead of reporting `= already`.

Decided: re-bootstrap rather than kickstart only when the probe shows the refusal. A healthy serve still gets `kickstart -k` (or a handover).

src/ net +40 (no budget given): a shared probe, the re-bootstrap, the post-swap heal and the doctor rows.

## Repro
Commit 1 (`0e5f24c`) holds only the three regression tests.
`cargo test --no-fail-fast -p drover --test timer --test self_update -- refuses symlink`
- base + commit 1: exit 101. `install_run_by_the_local_bin_symlink_names_the_install_roots_binary`, `doctor_marks_a_job_launchd_refuses_red` and `a_job_launchd_refuses_is_rebootstrapped_after_the_swap` all fail; the swap test shows `systemctl --user restart dev.drover.serve.service` and no re-bootstrap.
- head `a6f5948d8ac7a72604a94fa78ded080c7f46af8c`: exit 0, 3 passed.

## Evidence (head `a6f5948d8ac7a72604a94fa78ded080c7f46af8c`)
- `cargo fmt --check`: exit 0
- `cargo clippy --workspace --lib --bins --locked -- -D warnings`: exit 0
- `cargo clippy --workspace --tests --locked -- -D warnings -A …` (CI flags): exit 0
- `drover cap-check --config ci/cap.toml`: exit 0 (48528 / 53500 before the merge base moves)
- `drover arch-check --base origin/staging`: exit 1. The only violation is `` `just gen` exited 127 `` because `just` isn't installed on this host. The diff touches none of the generated files.
- `cargo test --locked --no-fail-fast --workspace` (lane `DROVER_*` env unset): exit 101 with one failure, `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`. That failure comes from the host: this machine has a real `/usr/local/bin/shed`, which the test expects to be absent. Every other test binary passes.

Not verified here: AC4 (on the Mini, after release: `launchctl print gui/501/dev.drover.clean` shows the install-root program and no codesigning exit reason). The launchd paths are covered only by the Linux CI fakes.
