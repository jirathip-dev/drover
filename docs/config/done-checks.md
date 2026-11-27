# Done checks (#1362)

`turn done` is a hard gate: before any review it checks, in the lane's worktree on its host, that `HEAD` is the pushed PR
head, that the PR body names it (`PR_BODY`), and runs each `done_checks` command (`[fleet.<n>] done_checks = ["…"]`),
each killed after `gate_check_minutes`. Unset = light checks only, in seconds (#1863): `drover arch-check` where the repo
has `.drover/arch.toml` and `cargo fmt --check` where it has a `Cargo.toml`, in every repo, one with `.gatekeeper/run`
included. CI is the one full gate run a merge needs; a red CI after `done` is a fix round. A fleet that sets
`done_checks = ["./.gatekeeper/run check"]` opts back into the full set at done: the wrapper fetches gatekeeper, so no
host installs it; drover writes the PR body to a file under the worktree's build folder and hands the fleet's
`private_names` on stdin as `DROVER_PRIVATE_NAMES`, so a remote host needs no fleet config. A check only checks; one
that fails, cannot start or times out refuses the `done` (`DONE_CHECK`, with its output tail).
