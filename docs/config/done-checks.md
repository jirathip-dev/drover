# Done checks (#1362)

`turn done` is a hard gate: before any review it checks, in the lane's worktree on its host, that `HEAD` is the pushed PR
head, that the PR body names it (`PR_BODY`), and runs each `done_checks` command (`[fleet.<n>] done_checks = ["…"]`),
each killed after `gate_check_minutes`. Unset = `./.gatekeeper/run check` where the repo has `.gatekeeper/run` (the
wrapper fetches gatekeeper, so no host installs it), else `drover arch-check` where the repo has `.drover/arch.toml`
and `cargo fmt --check` where it has a `Cargo.toml`. A check only checks; one that fails, cannot start or times out
refuses the `done` (`DONE_CHECK`, with its output tail). CI still runs format and arch-check.
