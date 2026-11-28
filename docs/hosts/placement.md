# Placement

`drover spawn` picks the host like this:

1. `--host <name>` forces that host.
2. Otherwise, a role pinned with `[fleet.<n>.role.<r>] host = "main"` (or a host name) goes there. `main` is the host
   without `machine`. A `design` lane always runs on the main host: it defaults to `main`,
   and `--host` or a pin naming another host is refused `DESIGN_MAIN_ONLY`.
3. Otherwise, an issue that already had a lane stays on that lane's host, because its worktree is there. This holds
   even when the host has been turned off.
4. A `--repro` review whose diff adds a test under `tests/ui/` or the fleet's `ui_paths` needs `webkit` in place of the
   fleet's `needs` (#1355), and the spawn event names that test; with no enabled `webkit` host it is refused `NO_HOST`
   and the issue gets one needs-human card. A `cfg(target_os)` test adds its `os:` tag.
5. Otherwise, drover takes the enabled hosts whose tags cover the needs, and picks the least busy: lowest load per
   CPU first, then the most free memory.

Admission runs per host: `DISK_LOW`, `MEM_LOW`, then `HOST_BUSY`. A host that fails admission (or is unreachable) is
skipped. A `DISK_LOW` names the done lanes' worktrees `drover clean --yes` would remove on that host (or says there
are none and points to `drover reclaim --apply`); a remote host's finished lanes' folders are swept first (#1215), and a spawn still starting toward a full host holds no `max_parallel_spawns`
slot, so it never holds back spawns bound for another host (#416). `drover clean` covers every host's merged or
closed lanes (shown as `<host>:<path>`) and lists the orphan folders under each enabled host's worktree root;
`--orphans --yes` removes them. It lists each leftover secrets file (`*.env` beside a worktree or none) on every host;
`--yes` removes them (#1532). It prints one summary line per host (`--verbose` lists each lane), and a host that is off or offline is one line (`boat3: 42 lanes, host off — not checked`) and no connection; a disabled host the last look saw down is not probed. A lane kept off-root is logged `clean-settled` once: no later run walks it and `status` shows no row for it; its old root is one `orphan root` line, and `--orphans` takes its lanes back through the usual wip save and removal (#1633). When every fitting host fails, the spawn is refused with the least-busy host's reason. The load in `HOST_BUSY` leaves out
drover's own processes and the `cp` clones they run (#825), and a refusal on load names the 3 busiest other processes
(`top: rustc 900%, …`). A tapped start held by load alone waits and starts on the first pass the host admits it; its
card never comes back for the load. When no host fits at
all, it is refused with `REFUSED NO_HOST: no host with docker`. drive
retries on its next pass.

### Start slots (#1186)

Each host starts its own lanes: `[hosts.<name>] start_max = 2` caps the lanes starting at once there (unset: `[limits]
start_max`, 2), so a clone and install burst on one host never waits on another host's starts. The main host is also
bounded by `[limits] max_parallel_spawns` (6), and keeps one of those slots for a fix round or review. Drive picks the
host at admission, passing over hosts whose start or working slots are full, and the spawn runs there; when every
fitting host is full it is skipped with `SPAWN_PENDING: boat-dev-2 2/2 starting`. A host's start slots are split
between the fleets with something to start, by fleet weight. `start_release = "started"` (the default, also in
`[limits]`) frees a start's slot once its harness is up on the host; `"confirmed"` holds it until its first prompt is
confirmed. `spawn_pending_max_secs` stays the stale backstop: a start that has not finished by then stops counting (logged
`stale-marker`), its slot is free again and its issue keeps its queue place, so the next pass starts it first (#1371). Every pass, the fast step too, runs the start step
before its PR reads, while some host has a free start slot; a full pass tries once more at its end for the issues its
own steps just released. With `max_parallel_spawns = 0` (spawns in the pass) only that end-of-pass step runs.
#1321: a pass only plans. Each issue with a step due (a verdict, a merge, a prompt) is handed to its own job, a detached
`kick --why plan`, at most `[limits] job_max` (4) of a fleet at once; the pass waits for none, and a start returns as
soon as its marker is taken. A spawn a job makes itself (a review, fix or respawn) takes a start-slot marker through the
same admission first. A start's own kick holds a start slot, not a job slot (#1583), so a verdict or merge job never
waits behind starts in flight. A fleet merges one PR at a time under `merge-<fleet>.lock`; a second merge is refused `IN_FLIGHT`
and tried again by a later pass.
`max_working` and the heavy-slot gate still bound the running load.

### Working-lane and simulator slots (#984)

`[hosts.<name>] max_working = 12` caps the lanes running a turn on that host, across all fleets; `max_simulators = 1`
caps the live lanes that need the `sim` tag (by fleet, role or `queue add --needs sim`). Unset means no cap. A lane
idle at its prompt, or whose issue is `ready`, `needs-human`, `parked`, `blocked` or done, holds no working slot; a
`sim` lane holds its simulator slot while its pane lives (unless its issue is in one of those states). Over a cap a
start is not refused for good: drive skips it with `HOST_FULL` (`waiting for a slot: 12/12 working on mini`), counting
spawns still starting, and starts it in queue order once a slot frees; a hand-typed `drover spawn` is not held. `drover status` prints `mini: working N/12 ·
simulators N/1` for each capped host. Once its verdict is in, a reviewer's idle pane closes on the next `check`.
Likewise, a builder's idle pane closes once a review started after it. Their worktrees stay until merge.

The `spawn` event records the lane's `host` and `machine`. From then on, every shed call for that lane goes to
that host's shed (pane reads, prompts, stop, kill, close), and its worktree work runs over ssh on that host:
`git worktree`, the brief and MCP files, the reviewer's `verdict.md`, the dirty and unpushed checks, and `clean`.

`spawn --dry-run` places the lane first and prints the commands exactly as they are sent: `--machine`, that host's
paths and home, and no token.
