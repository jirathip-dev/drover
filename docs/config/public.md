# Public repos (#1376)

Text bound for a public repo is checked before it is sent: a lane's `gh issue|pr create|edit|comment|review|close`
and writing `gh api` text (the hook), the commits a lane pushes (the pre-push hook), every wip save, and drover's own
texts (`merge`'s squash message and closing comment, `amend`, the issues `lesson`, `triage` and `hotspots` file, a
revert PR). Built-in patterns: home paths (a name under the root's `Users` or `home` folder), the names and machines of `[hosts]`,
paths inside drover's state and orchestrator folders, and session ids (UUIDs). A hit refuses `PUBLIC_LEAK`, naming
the pattern (`home-path`, `host`, `session-path`, `session-id`, `private-name #N`) and where, never the value.

| key | default | what |
|---|---|---|
| `[fleet.<n>] public` | GitHub's visibility, read at most once a day (spawn, merge, doctor) and cached; the hook reads only the cache | `true` checks, `false` skips; the flag wins |
| `[fleet.<n>] private_names` | `[]` | names (private repos, clients) never sent to the repo; the list stays in fleet.toml and, for CI, the repo's `PRIVATE_NAMES` secret |

`drover doctor` prints one `! hygiene` line naming the public fleets with no `private_names`. `drover hygiene scan
<fleet>` lists the hits already in the repo's issues, PR bodies, comments and integration history, by pattern with
their links; it changes nothing.

`drover hygiene check [<fleet>] [--base <rev>] [--pr-body <file>]` is the gate (#1483): the tracked files, the commit
messages since `--base` and the PR body against the names list, exit 1 on a hit, each printed `private-name #N at
file:line`, never the name. The list is `$DROVER_PRIVATE_NAMES` (one name a line; CI passes its `private-names`
secret), else the fleet's `private_names` (the argument, else the lane's fleet); a private fleet passes, and no list
fails `list-missing`. It needs no fleet.toml. `drover hygiene push <fleet>` (owner only; an orchestrator posts a run
card) writes the fleet's list as its public repo's `PRIVATE_NAMES` Actions secret and never prints it.
