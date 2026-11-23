# fleet.toml: serial groups (#294)

A serial group is a set of paths only `max` issues at a time may change. Each fleet may have any number; each group has
its own limit and its own Queue mark (🔒name).

```toml
[[fleet.app.serial]]
name = "db"                    # ≤ 8 chars, also the Queue mark
paths = ["supabase/migrations/**"]
max = 10                       # default 1

[[fleet.app.serial]]
name = "ci"
paths = [".github/workflows/ci.yml", "scripts/lib/ci*", "scripts/check-*"]
max = 1
```

The old form, `serial_paths` / `serial_max` / `serial_label`, still works: it is one group named `serial_label` (unset:
`db`). Giving both forms in one fleet is a config error; `drover doctor` suggests the new form.

## Joining

An issue is in a group when

- it is **marked**: `drover queue add N --serial ci`, or `queue set N serial ci,db` (replaces its marks). A plain
  `--serial` means the fleet's first group, as before; or
- its **pushed diff** touches one of the group's paths: drive checks each open PR and logs
  `serial-joined {group, by: "diff"}`.

`drover merge` still refuses `SERIAL_UNMARKED ci: PR touches scripts/check-x.mjs` for a PR touching a group it never
joined (belt and braces).

## Slots

An issue holds a slot in a group from joining until it is merged, closed or parked (in flight: an impl/fix lane, at
review or ready).

- A slot gates only new impl work: a marked issue with no open PR starts only with a free slot:
  `SERIAL_BUSY ci: #2968 holds 1/1`. A blocked issue holds no slot.
- A pushed head is never held (#1136): an open PR whose diff joins a full group is reviewed, verified, fixed and merged
  like any other, and counts toward the group's use. A conflict at merge gets an update round as usual.
- Issues already in flight over a limit when a group is added keep going (grandfathered).

`status` and `board` show each group's use: `serial: db 3/10 · ci 1/1`.

Hot files (#891): each file the last stored hotspots list names is its own one-slot group, `hot <file>`, joined
only by a PR's diff (never a `--serial` mark). Drive drops a file from it on its next pass once
`.drover/arch.toml` on the integration branch lists it `frozen`, `generated` or `shared` (#1147; `hot-files {files}`
logs each change).

## Fast-first landing (#350)

```toml
[fleet.app.land]
mode = "fast-first"                 # default "full-first": today's rule
fast = ["lint", "unit"]             # existing CI check names that count as fast
full_first_paths = ["db/migrations/**", ".github/workflows/**"]
```

- **Landing.** A PASS at the exact head lands once every `fast` check is green there, without the `merge_label` full
  run. A PR touching `full_first_paths` lands on the full suite (`required_checks`, else all), as before.
- **Swap-if-unchanged.** It lands only while integration is still in its head (`--match-head-commit` plus an ancestor
  check). If integration moved, merge refuses `BEHIND` (no block); drive catches the PR up and its fast checks run again.
- **After landing.** Every landing logs `landed {mode, sha}`. Each drive step reads the full suite on integration's tip:
  - **Green:** logs `land-green` and resumes landings.
  - **Red:** a runner/infra red (cancelled, timed out, never started) is re-run once. Otherwise fast-first landings
    pause (`land-paused`), and the culprit is found:
    - one landing since the last green is the culprit;
    - several are bisected: each landing commit's own run, else one dispatched on a `drover-bisect-<sha>` ref.
  - **Revert.** A revert PR opens and lands once its fast checks are green (`reverted`). The issue reopens in a fix
    round, with the failed job's log as its finding.
  - **Full-first culprit.** It is never reverted: it gets the fix round only (`land-fix`).
- **Alerts.** `land-paused` and `reverted` go to every notifier of the fleet, whatever its `on` says.
- **Numbers.** `drover stats land <fleet>` prints both modes' landings, per day, median PASS → landed, reverts and check
  time, plus the minutes integration sat red.
- **Promotion.** `main` stays human-only; promote a staging commit only once its full suite is green.

## Renaming a fleet (#935)

`drover fleet rename <old> <new>` is a dry run that prints every change it would make. With `--yes` it applies them.
It is owner only: the lane hook refuses it, so it runs as a run card's `--cmd`. It refuses `NOT_PAUSED` unless `<old>`
is paused, `LANE_LIVE` while a lane of an open issue in `<old>` is running, and `BAD_NAME` when `<new>` is taken or
not a valid name. First it backs up state.db, fleet.toml and the per-fleet state files. Then, in one transaction, it
moves the queue, events, asks, verdicts and secrets (their `secret-set` events) to `<new>`, renames the state files
and thread repo bindings, and renames the `[fleet.<old>*]` tables with every value unchanged. Any failure restores
the backup. It logs one `fleet-renamed` event, and old names keep resolving in `status`, `asks`, `board` and
`queue ls`. The fleet stays paused until the owner resumes it.

## Merge label (#947)

`merge_label` (unset by default) is the PR label the full CI needs. drover adds it on a PASS at the head and removes it
when the head moves past that PASS. `drover setup` creates it in the repo, and `drover doctor` checks that it exists.
New fleets set `merge_label = "merge-ready"` with the affected-only CI recipe in [ci.md](ci.md).

## Hotspots (#889)

`drover hotspots [FLEET]` (every fleet when none is named) lists the files 1 in 20 of the merged PRs (at least 5)
touched in the last 14 days on the integration branch; a merge touching more than 40 files is a sweep and not counted
(#1147), and the report's header says how many were skipped, any language, each with the distinct PR pairs it was predicted to conflict in
(#973: pairs, not one per push) and the update rounds of the issues that touched it (drover's log, every fleet on the
same repo). A file no longer at the integration tip, or one `.drover/arch.toml` lists frozen, generated or shared, is
left out. It keeps the list in the state folder as
`hotspots-<fleet>.json`. With any hot file it opens `hotspots: <fleet> week of <monday>`, or updates that issue while it
is open, and queues it `--architect`. `drover tick` stores each fleet's list daily and files its issue weekly; `hotspots = "off"` in `[fleet.<n>]` opts
out.

## History baseline (#1099)

`drover baseline <fleet> [<sha>]` (the owner's; default the integration tip) writes `baseline = "<sha>"` in
`[fleet.<n>]` and logs `baseline-set`; a sha not on the integration branch is `REFUSED BASELINE_OFF_INTEGRATION`.
Lanes whose branch contains it see `git log` and `git blame` only since it, by local grafts; nothing is rewritten.
Remove the key to undo it. How it works and the other undos: `docs/workflow.md`, "History baseline"; the design:
`docs/architecture/baseline/`.

## In-flight cap (#1129)

`max_in_flight = N` in `[fleet.<n>]` caps the fleet's PRs in flight (an issue in impl, fix, review, verify or ready,
or one held with an open PR); unset is 8, `0` is off. At the cap drive starts no new issue but keeps reviewing, fixing
and merging; `drover spawn` of a new issue is refused `IN_FLIGHT_CAP` unless `--force`. A start drive admitted holds a
slot until its lane comes or `spawn_pending_max_secs` passes; the refusal names the issues with a lane or a PR, and
counts those starting. Drive gives the free slots to the queue in order; the kicks it launched count no cap again.

## Done checks (#1362)

`turn done` is a hard gate: before any review it checks, in the lane's worktree on its host, that `HEAD` is the pushed PR
head, that the PR body names it (`PR_BODY`), and runs each `done_checks` command (`[fleet.<n>] done_checks = ["…"]`),
each killed after `gate_check_minutes`. Unset = `./.gatekeeper/run check` where the repo has `.gatekeeper/run` (the
wrapper fetches gatekeeper, so no host installs it), else `drover arch-check` where the repo has `.drover/arch.toml`
and `cargo fmt --check` where it has a `Cargo.toml`. A check only checks; one that fails, cannot start or times out
refuses the `done` (`DONE_CHECK`, with its output tail). CI still runs format and arch-check.

## Hygiene (#1101)

One history and issue convention is on for every fleet with no config. `[hygiene]` sets it for all fleets,
`[fleet.<n>.hygiene]` per fleet (its keys win); `"off"` opts a key out.

| key | default | what |
|---|---|---|
| `merge` | `"squash"` | the merge method `[fleet.<n>] merge` and the repo should use; also `"merge"` |
| `comments` | `"strict"` | `.drover/arch.toml`'s `[comments] policy`; the rule ships in `rules/shared.md` |
| `baseline` | `"setup"` | the arch-check baseline, set at fleet setup, redone on the owner's ask |
| `triage` | `"weekly"` | a weekly triage lane |
| `docs_trust` | `["docs/archive/"]` | paths a new lane worktree leaves out (sparse checkout); `[]` = off |
| `issue_spec` | `"triage"` | triage folds amendments into `## Spec (current)` |
| `docs` | `"on"` | `.drover/arch.toml` has the #735 `[docs]` stale-doc check |

`drover setup` applies the set to each fleet's local clone, writing only what is missing: `[comments] policy = "strict"`
and a `[docs]` section (paths `docs/**/*.md` and `README.md`, the repo's `cli`, `config` and `api` files as `watch`) in
`.drover/arch.toml`, then the first arch-check baseline when `baseline = "setup"` and none exists. Commit what it wrote.

`drover doctor` prints each fleet's `hygiene` line and a red `! hygiene` row for each drift: the fleet's own
`merge` mode (#1097) differing from it, the repo on GitHub not allowing that method, or `comments = "strict"` with integration's `.drover/arch.toml` saying otherwise, or `docs = "on"` with no `[docs]` there.

## Public repos (#1376)

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

## Limits (#844)

Every `[limits]` key, its default and what it sets: [config/limits.md](config/limits.md), `drover config show --md`'s output.
