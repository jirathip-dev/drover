# Hosts: lanes on more than one machine (#157)

One **main host** runs herdsman, the board (`herdsman serve`) and the event log. It starts lanes on any host through
herdr's `--machine`, picks the least-busy host that fits, and shows every host on the board. With no `[hosts]` in
fleet.toml nothing changes: every lane runs on the main host, as before.

## Add a host

1. On the main host, save the machine in herdr: `herdr machine add` (an SSH target), then check it with
   `herdr machine status`. herdsman reaches the host two ways under that one name: `herdr --machine <name> …` for
   herdr commands, and `ssh <name> …` for everything else (git, probes, file reads and writes). So the machine's label
   must also work as an SSH host name, e.g. an entry in `~/.ssh/config`. Key-based login only, because herdsman runs
   ssh with `BatchMode=yes`.
2. On the new host, install and log in to:
   - herdr (its server running);
   - git;
   - `gh` (`gh auth login`): lanes there open their PRs with it;
   - git access to each fleet repo: the host clones each repo itself the first time a lane goes there, with plain
     `git clone https://github.com/<repo>.git <local>`, so its git config (`insteadOf`, credential helper) applies;
   - the agent harness (Claude Code logged in);
   - herdsman itself, then run `herdsman profiles sync` there. The lane's tool-guard hook calls `herdsman hook`, and
     the Claude profile lives under that host's own home.

   herdsman never copies credentials to another host: a role's `account` token is only exported to lanes on the
   main host; a lane on another host runs on that host's own Claude login.
3. Add the host to fleet.toml:

```toml
[hosts.mini]            # the main host: no `machine`
tags = ["fast"]

[hosts.bazzite]
machine = "bazzite"     # the herdr machine label (also its SSH name)
tags = ["gpu"]          # hand tags, added to the detected ones
root = "~/lanes"        # worktree root on that host (default ~/.herdr/worktrees)
enabled = true          # false: no new lanes go there (see below)
```

A fleet's `local` that starts with `~/` points to that path under each host's own home. An absolute `local` is used
as written on every host.

## Tags

Each `herdsman check` (and `herdsman hosts refresh`) looks at every host. Once an hour it probes the host's tags with
one shell line: `os:macos` or `os:linux`, `arch:arm64` or `arch:x86_64`, `xcode` (when `xcodebuild -version` works) and
`docker` (when `docker info` works). The results go to `<state dir>/hosts.json`, along with the host's `$HOME`,
whether it answered, and its load/cpu, free memory and free disk. A host's tags are the detected ones plus its hand
`tags`.

## Needs

A lane needs the union of three lists:

- the fleet's `needs` (`[fleet.<n>] needs = ["xcode"]`);
- the role's `needs` (`[fleet.<n>.role.design] needs = ["os:macos"]`);
- the queue item's needs (`herdsman queue <fleet> add 12 --needs docker,gpu` adds to them;
  `herdsman queue <fleet> set 12 needs docker` replaces them; `queue ls` shows them).

## Placement

`herdsman spawn` picks the host like this:

1. `--host <name>` forces that host.
2. Otherwise, a role pinned with `[fleet.<n>.role.<r>] host = "main"` (or a host name) goes there. `main` is the host
   without `machine`. A `design` lane always runs on the main host, in the owner's herdr: it defaults to `main`,
   and `--host` or a pin naming another host is refused `DESIGN_MAIN_ONLY`.
3. Otherwise, an issue that already had a lane stays on that lane's host, because its worktree is there. This holds
   even when the host has been turned off.
4. Otherwise, herdsman takes the enabled hosts whose tags cover the needs, and picks the least busy: lowest load per
   CPU first, then the most free memory.

Admission runs per host: `DISK_LOW`, `MEM_LOW`, then `HOST_BUSY`. A host that fails admission (or is unreachable) is
skipped. When every fitting host fails, the spawn is refused with the least-busy host's reason. When no host fits at
all, it is refused with `REFUSED NO_HOST: no host has docker`, or `no enabled host has docker` when only hosts that are
off fit. drive retries on its next pass.

The `spawn` event records the lane's `host` and `machine`. From then on, every herdr call for that lane carries
`--machine` (pane reads, prompts, stop, kill, close), and its worktree work runs over ssh on that host:
`git worktree`, the brief and MCP files, the reviewer's `verdict.md`, the dirty and unpushed checks, and `clean`.

`spawn --dry-run` places the lane first and prints the commands exactly as they are sent: `--machine`, that host's
paths and home, and no account token. If the agent fails to start on another host, its new workspace is closed.

## Asks, answers and verdicts from another host (#275)

A lane on another host gets `HERDSMAN_MAIN` (`<main host's [hosts] name>:<its herdsman path>`). Its `herdsman ask`,
`answer`, `verdict` and its own events (`hook-denied`, `coauthor-trailer`) run on the main host over
`ssh <main host's name>`, so the main host's log is the only one written and only the main host pushes. So the main
host's `[hosts]` name must also work as an SSH name on every other host (an `~/.ssh/config` entry back to it, key-based).
That host's own `events.jsonl` is never written; herdsman warns when it finds one. Each `check` imports the asks a lane
left in a remote host's own log (lanes started before this) under a new id, with `from` naming the old one.

An answer reaches a remote lane with `herdr --machine <m> agent prompt`, then Enter. When `agent start` fails
`agent_name_not_found` (on any host, the main one under load too), herdsman names the harness if one came up, else
starts it once more; it never closes the pane or workspace of a harness that came up.

## Turn a host off

`herdsman hosts off <name>` (or the `[off]` button on the host's card in the Host tab, paired devices only) sets
`enabled = false` in fleet.toml and logs `host-off` with `by`. `hosts on` and `[on]` undo it, logging `host-on`.
The buttons ask first (`turn air off? no new lanes; 2 running lanes finish`).

An off host gets no new issues. It keeps being monitored, and its running lanes drain there: their reviews and fix
rounds still go to it. The Host tab shows such a host grey, as `off · draining 2 lanes` while lanes remain, then
`off · not used for lanes`. The main host (no `machine`) runs herdsman, the board and the log: it has no `[off]`, and
`hosts off` refuses it `MAIN_HOST`; set `enabled = false` in fleet.toml by hand if you mean it.

## Monitoring

- `herdsman hosts` prints, per host: on or off, reachable or offline, its tags, load/cpu, memory, disk, and its lanes.
- `herdsman status` gets a `HOST` column once fleet.toml has `[hosts]`.
- The Host tab's HOSTS section has one card per host, main first, then A→Z, each with the same rows (#234): load/cpu,
  swap-out, memory, disk, top memory, lanes and tags, the state and why its dot is amber or red in its head. Remote
  values (and top memory, with its age) come from the 5-minute `check`. An unreachable host is grey
  (`offline · last seen 33m`) in the same layout, and its lanes never page (`lane-dead` and `stall` skip them).
- History (#234): `<state dir>/host-history.jsonl` keeps 24 h of `{ts, host, load_cpu, pressure, swap_out, mem_free_gb,
  disk_free_gb, lanes}`: the main host a row a minute (`drive --fast`), a remote host a row every 5 minutes (`check`'s
  survey, no extra SSH). Rows past 24 h are dropped when it is rewritten (at most hourly), and it stays under
  about 180 KB a host. Each row on a card has a 24 h sparkline; a tap opens a 1 h / 6 h / 24 h chart with the limit
  dashed and the lane count as a step line. Downsampling keeps the worst row of each bucket (the max; free space the
  min), so a peak is never hidden.
- Each lane card, and each horse in the Herd, shows its host name.

## Health (#235)

`herdsman doctor` checks every enabled remote host, and `check` does the same once an hour (with the tag probe). A
failure turns the host red on the Host tab (`unhealthy · <reason>`) and keeps new lanes off it: placement refuses it
with `NO_HOST: <host>: unhealthy: <reason>`. The checks, each with its fix:

- **herdr**: the server's version equals the installed client's (`herdr status` on the host). An old server keeps
  running after an update, and `herdr machine add` then fails with "remote server is not ready": update herdr on the
  host and restart its server.
- **herdr session**: on macOS the server must run under the user's launchd job (the login session). A server
  restarted from an SSH shell runs outside it, with the keychain locked: `gh` says "token invalid", private clones
  fail and Claude can't read its login. Fix: `launchctl kickstart -k gui/<uid>/<label>` (the label of the job that
  starts `herdr server`).
- **git**: `ssh <host> git ls-remote https://github.com/<repo>.git` works for each fleet whose needs the host fits.
  herdsman's own git over SSH never has the keychain, so HTTPS fails with `could not read Username for
  'https://github.com'`. A failure reads `✗ git <fleet> — can't reach <repo>: <first error line>` and keeps only
  that fleet's lanes off the host; placement moves them to the next host. Fix: add the host's SSH key to GitHub
  and run `git config --global url.git@github.com:.insteadOf https://github.com/` there.
- **gh** and **claude**: `gh auth status` and `claude auth status` succeed inside a herdr pane on the host (a
  throwaway `herdsman-health` workspace, closed again). Lanes there need `gh` for their PRs, so `gh` logged out is
  a red host problem. Fix: `ssh <host> gh auth login` for gh; for claude, log in from a pane there.
- **herdsman**: `herdsman --version` there prints the same version and git sha as on the main host. A mismatch is
  a warning (amber), not a failure.

## Auto-update

When `check` sees an older herdsman on a remote host, and fleet.toml has the herdsman fleet, it rebuilds the main
host's sha there in the background: `git fetch`, `git checkout --detach <sha>` and
`cargo install --path crates/cli --locked --root <root> --force` in that fleet's checkout on the host, then
`herdsman profiles sync`. Output goes to `~/.herdsman-update.log` on the host. Each host and sha is tried once, and
logged as `host-update` (with the error when the ssh call failed). Ten minutes later `check` reads the version back.
A host still on the old version stays amber. A failed update never blocks the main host.

## When the main host is down

Nothing new starts. Lanes on other hosts keep running in their own herdr servers. When the main host comes back,
`check` finds those lanes again from the log. To move the main host, follow docs/move.md.

## Cloud hosts (#254)

A cloud host is a `[hosts.<name>]` with a `cloud` recipe. herdsman starts it when a lane needs it and stops it when idle.

```toml
[hosts.boat1]
machine = "boat1"          # the ssh alias herdsman writes, and the herdr machine label
cloud = "boat"             # the built-in recipe; or <config>/recipes/<name>.toml
size = "small"             # a key of the recipe's [rate_usd_hour]
budget_usd_month = 50      # hard: none set means 0, so it never starts
max_hours_day = 6          # optional
idle_stop_minutes = 20     # the default
tags = ["docker"]

[limits]
overflow_after_minutes = 10  # the default
```

- **The recipe** is a TOML file of `sh -c` templates: `create`, `start`, `stop`, `status` (prints `running` when up),
  `address` (prints `user@host port`, then the host key line), `destroy`. It also sets `key_env`, the provider key's
  variable, and `[rate_usd_hour]` per size. `{name}` is the host's name and `{size}` its size. The built-in `boat`
  recipe is assets/recipes/boat.toml. Check its commands and rates against docs.boat.dev, and copy it to
  `<config>/recipes/boat.toml` to change them.
- **The provider key** is read on the main host from the environment, else from `[accounts].accounts_env`. Each
  command gets it on stdin and exports it. It never appears in argv, fleet.toml or the event log.
- **Start on demand.** Placement uses a stopped cloud host in two cases: every local host has been refused (busy, low
  memory or disk) for `overflow_after_minutes`, or the lane needs a tag only the cloud host has. A respawn on a lane's
  own host, `--host`, or a role `host` pin starts it too. `spawn` runs `start`, then polls `status` for up to 5 min.
  It then rewrites the host's block in `~/.ssh/config.d/herdsman` and its own known_hosts file
  (`~/.ssh/config.d/herdsman-<name>.known`) from `address`, and then spawns. A dry run never starts a host. Add
  `Include config.d/*` at the top of `~/.ssh/config` once.
- **Idle stop.** `check` stops a running cloud host after `idle_stop_minutes` with no live agent on it. First, each
  lane worktree there with unpushed or dirty work is pushed to `wip/<host>-<lane>`, after the secret scan. A secret
  or a failed push aborts the stop (logged `cloud-abort`). The disk is kept. Lanes there show `paused: cloud host
  stopped` on the Host card, and their next spawn starts the host again.
- **Spend** is the running time (`cloud-start` → `cloud-stop` in the log) × the size's rate, per UTC month and day.
  The Host card shows `spent $12 of $50 · 3.1 h today of 6`, the address age and the last start and stop. The row
  turns amber at 80 % and red at the limit. At the limit, placement refuses `CLOUD_BUDGET` and `check` stops a running
  host (after the wip save). Only an owner edit to fleet.toml raises a limit.
- **Destroy** is owner only: `herdsman host destroy <name>` does the wip save, then runs the recipe's `destroy`.

## Limits

Remote lanes run the Claude harness. The dsh harness writes its pre-push hook file on the main host only.
