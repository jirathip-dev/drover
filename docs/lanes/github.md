# GitHub stays clean

Issues are the spec, PRs the code. Lanes never comment: the Claude hook denies
`gh issue|pr comment` and `gh api` comment writes. Routing, queue, lane and verdict state
live in the event log. `drover merge` writes the one closing comment (merge sha and
verdict sha), then closes the issue's lane panes and reclaims their worktrees; a lane with
uncommitted or unpushed work is kept and reported. #1479: a PR body's `## Rollout` (fenced
fleet.toml lines) becomes an open rollout in the filing thread until a config card applies it
(docs/orchestrator.md, Rollouts). Merging into drover's own repo ends by
printing the `cargo install` line to reinstall it (printed, not run).
