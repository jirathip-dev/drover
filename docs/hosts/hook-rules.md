# A remote lane's hook rules (#592)

Only the main host has a fleet.toml. Spawn resolves a remote lane's hook rules there (protected branches, the fleet's
`deny_commands` under the host's `[hosts.<name>]` rules, the reviewer flag) and writes them to
`<worktree>.rules.json` on the lane's host, exported as `DROVER_RULES`, and syncs the lane's harness profile there
by name. The hook there reads only that file (the lane
may not edit it); a missing or unreadable one fails closed `HOOK_FAILED_CLOSED`. It holds no notifier, token or env
value. The rules are frozen at spawn: after a config change that alters them, the board marks the lane `rules stale`
until a respawn. A fleet.toml left on a remote host is no longer read: `drover doctor` shows
`! stale fleet.toml on <host>: no longer read`; remove it by hand. #647: bootstrap removes one an old bootstrap wrote
(only `[fleet.*]` and `[hosts.<host>]`); doctor's host row names the failed check (`unreachable (ssh: …), down 12m`,
`✗ shed — does not answer: …`).
