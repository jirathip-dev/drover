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
   - `gh` (`gh auth login`): the host clones each repo itself the first time a lane goes there
     (`gh repo clone <repo> <local>`);
   - the agent harness (Claude Code logged in);
   - herdsman itself, then run `herdsman profiles sync` there. The lane's tool-guard hook calls `herdsman hook`, and
     the Claude profile lives under that host's own home.

   herdsman never copies credentials to another host.
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
2. Otherwise, an issue that already had a lane stays on that lane's host, because its worktree is there. This holds
   even when the host has been turned off.
3. Otherwise, herdsman takes the enabled hosts whose tags cover the needs, and picks the least busy: lowest load per
   CPU first, then the most free memory.

Admission runs per host: `DISK_LOW`, `MEM_LOW`, then `HOST_BUSY`. A host that fails admission (or is unreachable) is
skipped. When every fitting host fails, the spawn is refused with the least-busy host's reason. When no host fits at
all, it is refused with `REFUSED NO_HOST: no host has docker`, or `no enabled host has docker` when only hosts that are
off fit. drive retries on its next pass.

The `spawn` event records the lane's `host` and `machine`. From then on, every herdr call for that lane carries
`--machine` (pane reads, prompts, stop, kill, close), and its worktree work runs over ssh on that host:
`git worktree`, the brief and MCP files, the reviewer's `verdict.md`, the dirty and unpushed checks, and `clean`.

## Turn a host off

`herdsman hosts off <name>` (or the `[off]` button on the host's group in the Host tab, paired devices only) sets
`enabled = false` in fleet.toml and logs `host-off` with `by`. `hosts on` and `[on]` undo it, logging `host-on`.

An off host gets no new issues. It keeps being monitored, and its running lanes drain there: their reviews and fix
rounds still go to it. The Host tab shows such a host grey, as `off · draining 2 lanes` while lanes remain, then
`off · not used for lanes`. The main host can be turned off for lanes too; herdsman and the board keep running there.

## Monitoring

- `herdsman hosts` prints, per host: on or off, reachable or offline, its tags, load/cpu, memory, disk, and its lanes.
- `herdsman status` gets a `HOST` column once fleet.toml has `[hosts]`.
- The Host tab has one group per host: a status dot, the gauges (from the last `check`), its tags and lanes. An
  unreachable host is grey (`offline · last seen 33m`), and its lanes never page (`lane-dead` and `stall` skip them).
- Each lane card, and each horse in the Herd, shows its host name.

## When the main host is down

Nothing new starts. Lanes on other hosts keep running in their own herdr servers. When the main host comes back,
`check` finds those lanes again from the log. To move the main host, follow docs/move.md.

## Limits

Remote lanes run the Claude harness. The dsh harness writes its pre-push hook file on the main host only.
