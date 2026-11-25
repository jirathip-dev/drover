# The lane hook

`drover profiles sync` installs `drover hook --role <role>` as a PreToolUse hook for
Claude lanes. Spawn exports `DROVER_LANE`
(`<fleet>:<issue>:<role>`) in the lane shell, which arms the hook. It blocks `gh pr merge`,
issue and PR comments, `gh pr review`, `git push` to main, master, staging or the
integration branch, and edits to fleet config. Reviewer lanes are
read-only: no edits except `verdict.md`, no `git commit|push|merge|rebase|reset`.

The hook fails closed (#508): in a lane, a call whose input isn't JSON or names no tool, under a fleet.toml that
doesn't load, or from a lane whose fleet isn't in it is denied `REFUSED HOOK_FAILED_CLOSED: <what failed>; fix it,
or ask the owner`. Each logs `hook-failed-closed` (lane and cause, never the command), alerted once per cause an
hour. Its rules come from the binary and the loaded fleet.toml only, never from a file in the lane's worktree.

### Kicks (#304)

The same hook runs on Claude `PostToolUse` (Bash) and `Stop`: a
`git push` that went through, or a finished turn, runs `drover kick <fleet> <issue> --why push|stop`.
So do `drover ask`, `verdict` and `answer`, and the board's answer, override, extend, park and queue
moves. A kick returns at once: a detached `kick --now` (output in `<state>/kick.log`) drives that issue's
due steps alone under the drive lock, logs `kick`, and coalesces kicks within 10 s (or while the lock is
busy) into one follow-up. A lane on another host kicks the main host. The tick and fast timers stay the
safety net for CI, outside merges and stalls.

### Denied commands (#216)

The hook also refuses every `deny_commands` entry run as a command word: in a pipeline, after `&&`
or `;`, inside `$( )`, through `sh|bash|zsh -c`, `env`, `sudo`, `exec`, `xargs`, `nice`, `timeout`,
`eval`, `then`/`do`/`{`, past their flags (`sudo -u root`), after `VAR=…`, or by full path
(`/usr/local/bin/docker`). Matching is lexical and best-effort: `D=docker; $D ps`, `ssh h docker ps`
or `open /Applications/Docker.app` pass. An entry is a command word (`"docker"`) or a command plus
subcommand prefix (`"supabase start"`, `"git push --force"`); an entry ending in `=` (`"DOCKER_HOST="`)
refuses that assignment. `{ cmd = "…", why = "…" }` makes `why` the whole message; a bare entry says
``REFUSED NO_COMMAND: lanes never run `<cmd>` ``. The same word as an argument (`grep docker file`) passes.

The default (`[limits] deny_commands`, listed in `assets/deny_commands.toml`) is the owner's
no-containers rule: `docker`, `docker-compose`, `podman`, `nerdctl`, `colima`, `orb`, `orbctl`,
`limactl`, `supabase start`, `supabase db start`, `DOCKER_HOST=…` and `open -a Docker|OrbStack`, each
refused with `REFUSED NO_CONTAINERS: lanes never start containers (owner rule). Use the repo's
non-container test path, or ask the orchestrator.` `[fleet.<n>] deny_commands` replaces the list (`[]` turns
it off, noted on every config load), `deny_commands_add` extends whichever applies. The board's [config]
sheet shows the active list. Each refusal is logged `hook-denied` (`rule` = the message) and shows on Events.

Per host (#256): spawn exports `DROVER_HOST=<[hosts] name>`, and the hook applies that host's rules on top:
`[hosts.<n>] deny_commands` replaces the fleet's list, `deny_commands_add` extends it, and `allow_commands` drops
entries. A cloud host (`cloud = …`) defaults to allowing `docker`, `docker compose` and `supabase start` (a disposable
machine; `allow_commands = []` keeps the ban); a Mac keeps the ban. A remote lane's hook rules (deny, branches,
heavy slots) are resolved on the main host at spawn and written to its `DROVER_RULES` file (#592); the remote host's
own fleet.toml is never read. A config change reaches a live lane only when it respawns; `drover board` and
`drover waiting` show `rules stale` until then.

Heavy checks (#909): `[fleet.<n>.heavy]` maps a kind to command patterns (`*` matches any text), e.g.
`swiftlint = ["swiftlint analyze*"]`, `vitest = ["*vitest*"]`, `cargo = ["cargo test*", "cargo check*"]`. A lane's
Bash call with a command matching one (claude only: its hook can rewrite the call) runs under `drover heavy`, which
takes one of the host's slots first and frees it when the command exits or its holder dies, even by SIGKILL.
`[hosts.<n>] heavy_max` (default 3) is the slots every lane on the host shares, whatever its fleet;
`heavy_kind_max = { swiftlint = 1 }` caps a kind lower. A full host keeps the command waiting in line, first come
first served (a kind at its own cap lets later kinds by); after `heavy_wait_minutes` (default 30) it runs anyway
and logs `heavy-timeout`. Each command in line is a file in `<state>/heavy/` naming its `lane`, `kind`, `cmd`,
`since` and `running`. Other commands are unchanged.

Build machines (#1708): `[fleet.<n>.remote.<kind>]` sends a heavy kind's commands to the runners repo's build machines first:
`tag = "linux"` (required), and optional `wait` (minutes for a free machine slot), `setup`, `env = ["NAME"]` and
`out = ["path"]`, passed as `runners run --tag … --dir <lane worktree> … -- bash -c <cmd>`. Its output and exit code
are the command's; a remote run holds no slot here. On runners' exit 75, 76, 64 or 78, or no `runners` on `PATH`,
`drover heavy` prints `runners: exit 75, running here` and runs the command here in a heavy slot as above. Never
routed, whatever the patterns: `git`, `gh`, `drover`, `nx affected`, `--changed`, `merge-base`, `origin/`, code
signing, `--release` and `cargo install`, containers, and lines reading stdin (`<`) or `.env`
(`heavy::NEVER_ROUTED`); `remote_deny = ["…"]` in `[fleet.<n>]` adds more. A line mixing a routed kind's commands with
never-routed ones (#1748) sends only those commands, each with its own redirects kept here, and runs the rest here
in order, so its output and exit code are the line's. A line with a pipe into or out of such a command, a subshell,
a here-doc, `<`, `&` or `.env` stays here in a slot and prints `heavy: <why> in the line, running here`. Text in
quotes, comments and here-doc bodies never makes a command a heavy kind. A fleet with any heavy kind or route
gets the `cargo` kind (`heavy::CARGO`: build, check, clippy, test, nextest, run, xtask) unless it sets its own. Config load refuses a routed kind whose
pattern overlaps that list, naming both. `drover clean` runs `runners forget --dir <worktree>` for each worktree it
removes; a failure is printed and kept in the `clean` event's `forget`, and the clean goes on. `drover top` marks a
`runners run` process `[remote <tag>]`.

Gate checks (#1747): a fleet with `[fleet.<n>.remote.cargo]` sends its gate checks the same way. A lane's
`./.gatekeeper/run check…` and `just check…` join the `cargo` kind; `turn done` runs each done check of a lane on the
main host under `drover heavy cargo --remote <tag>` (the gatekeeper check gets its head, PR body and private names
through `--env`, never stdin); `drover hold` sends its command to the fleet whose repo is the job's (`GITHUB_REPOSITORY`)
and holds no slot. Each falls back here as above, printing the `runners:` line. The never-routed list still applies;
a fleet whose gate builds platform-only code (Metal, `xcodebuild`) lists its gate command in `remote_deny`.

A CI job on a self-hosted runner that shares the main host runs its step as `drover hold --wait <minutes> -- <cmd>`
(#1492): it waits until the host passes spawn admission (`MEM_LOW`, `HOST_BUSY`, …) and a slot is free by the main
host's `heavy_max`, holds that slot as kind `ci` and holder `ci:<repo>` (from `GITHUB_REPOSITORY`) while the command
runs, and exits with its code. Lanes' heavy commands wait behind it as behind each other; at the front of the line it
samples admission every `[limits] hold_sample_secs` (default 10). Past the wait it never runs the command: it exits
75 and names the reason on stderr. `drover top` lists it under `holds`.

`drover top` (#910; `--json` for the same data) shows what each live lane is really doing: every process under its
worktree as a tree with its CPU %, run time and heavy kind, and `waiting for heavy slot <kind> <age>` for each of its
commands in line; the header gives the host's CPU idle against `min_cpu_idle`, heavy slots used of `heavy_max` and
the respawns the last drive pass held back (`RESPAWN_PACED`). When CPU idle stays under `min_cpu_idle` for
`[limits] overload_minutes` (default 10) in the host history, `drover check` alerts `host-overloaded` once, naming
the 5 busiest processes and their lanes; once idle is back over the floor, the next overload alerts again.

Enforcement: the Claude `PreToolUse` hook (`drover hook`) refuses the command mechanically.
