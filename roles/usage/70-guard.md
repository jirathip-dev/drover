## Tool guard, sign-in, MCP

Lanes skip permission prompts; the lane's hook blocks `gh pr merge`, `gh issue|pr comment`, `gh pr review`,
`gh api` comment writes, `git push` to main/master/staging/integration, edits to fleet, fleet-operations and agent
profile config. Reviewers are read-only: no edits but `verdict.md`, no `git commit|push|merge|rebase|reset`. Design and architect lanes are owner-only: started by the owner or an orchestrator thread (`drover spawn … design|architect`), never by a lane; never in verdict or merge.
The hook also refuses `deny_commands` (default: containers: docker, colima, orb, podman, `supabase start`… → `REFUSED NO_CONTAINERS`) and, for every lane, `pkill`, `killall` and `pgrep … | xargs kill` (`REFUSED NO_BROAD_KILL`) and host CPU stress: uncapped `yes` to a file or in a loop, `stress`, a loop backgrounding its body (`REFUSED HOST_STRESS`); a call it can't judge (input not JSON, fleet.toml not loading, an unknown fleet) `REFUSED HOOK_FAILED_CLOSED`.
Sign-in: lanes run on the host's own CLI login; a remote ACP lane signs in through the main host's token proxy. Secrets (`secret:<NAME>` env) live only in the main host's OS store as `drover/<fleet>/<NAME>`; spawn writes a lane's to a 0600 file beside its worktree (over ssh stdin to a remote host), the pane's shell sources it, spawn deletes it. MCP: only the role's `mcp` (default none).
