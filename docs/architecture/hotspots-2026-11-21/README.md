# hotspots: herdsman week of 2026-11-16 (#1548)

## Summary

Decision: six of these nine files were already decided in #1147, as follow-up issues that were never filed, so nothing is left for a follow-up this time: this issue's one PR converts all nine. Each list becomes one file per entry (a CI gate, a screen area with its own routes and actions, a doc section, an example key). The test shard list and the limits table stop being written by hand. The page's main script is classed as shared code under a size limit.
Slices: 1 test shards without a list, 2 CI gates as two folders, 3 the limits table and the example config generated, 4 the four docs as one file per section, 5 the screens as one file per area, 6 the page script as shared.
Open questions: (1) #1342 (blocks D4) is second in the queue and rewrites every builder in the screens file this PR moves: hold it until this PR merges (recommended), or let it go first? Two choices you may want to veto: CI stays one workflow file (splitting it would break the release trigger), and a red CI run now names every failing gate instead of stopping at the first.

Read this file, then `adr.md`, `shapes.md`, `boundaries.md`. The folder is named for the day the window ends:
`docs/architecture/hotspots-2026-11-16/` is #1147's, filed for the same week.

## Where it stands (origin/staging 685e81cf, 14 days to 2026-11-21)

404 counted merges, bar 20. Where each file's PRs land, counted per merge from the diff:

| file | PRs | where they land | what it is | ending |
|---|---|---|---|---|
| `crates/server/src/screens.rs` | 52 | 21 the route match, 16 the action match, 27 one or both; no screen area's own code over 16 | two lists inside code | one file per area; the path goes |
| `scripts/test-shards.txt` | 44 | each adds a `<shard> <target>` line for a new test file | a list nothing needs | deleted |
| `.github/workflows/ci.yml` | 43 | 12 the WebKit path list, 11 a `rust` step, 9 a `ui-webkit` step | three lists | two gate folders; frozen |
| `docs/serve.md` | 41 | 15 the screens, 15 API v1, 7 health, 7 the page | a page of sections | one file per section; frozen index |
| `docs/config.md` | 38 | 25 the limits table | a table written twice | generated; the rest per section |
| `docs/lanes.md` | 36 | 11 spawn, 11 drive, 8 check | a page of sections | one file per section |
| `docs/hosts.md` | 31 | 8 placement, 5 bootstrap, 4 or fewer each elsewhere | a page of sections | one file per section |
| `fleet.example.toml` | 27 | 14 the `[limits]` block, 9 one more optional `[fleet.web]` key | two lists | generated from a folder |
| `web/src/main.ts` | 24 | paint 10, stream 5, act 5, sheet 4 | one loop over shared state: code, not a list | shared, under a size rule |

Why six are back: #1147 decided the CI file, the three docs and the limits table as follow-ups A, B and D, for the
orchestrator to file on approve. None was filed. This ADR keeps what those shapes got right and builds them here.

What #1147's CI shape got wrong: it split `ci.yml` into one workflow per job. `.github/workflows/release.yml` starts
when the workflow named `ci` ends green, so five workflows would publish a build after any one of them. The file
stays one workflow; its step lists leave it.

## How the callers read, after

A new screen or action is an arm in its own area's file. The router names no route and no action:

```rust
// crates/server/src/serve/screens/queue.rs
pub(super) const AREA: Area = Area { screen, action, owner: &[], age: true };

fn screen(s: &Server, ctx: &Ctx, r: &Req) -> Option<Value> {
    Some(match r.path {
        "/screens/queue" => s.queue_screen(ctx, r.q),
        "/screens/queue/done" => done_screen(&card(s, ctx, r.q)?, r.q),
        "/screens/issue" => issue::sheet(s, ctx, r.q)?,
        _ => return None,
    })
}

fn action(s: &Server, c: &Call) -> Option<Result<Reply>> {
    Some(match c.name {
        "drive" => s.form_op(c.ctx, "drive", &[("fleet", c.fleet())]).map(Reply::said),
        "limit" => c.issue().and_then(|n| crate::limit::wait(c.ctx, &s.r, (&c.fleet(), n), "owner")).map(Reply::said),
        _ => return None,
    })
}
```

```rust
// crates/server/src/serve/screens/mod.rs: the whole router
let (v, area) = AREAS.iter().find_map(|a| Some(((a.screen)(self, ctx, &req)?, a)))?;
let reply = AREAS.iter().find_map(|a| (a.action)(self, &call)).with_context(|| format!("no handler for `{name}`"))??;
```

A new CI gate is one new file. The job runs the folder; a WebKit gate also says which paths make the job run:

```sh
#!/usr/bin/env bash
# gate: the Herd in WebKit (#1092: a figure per lane, night, the next repo, the pinned swag scene)
# paths: crates/cli/tests/herd.rs
set -euo pipefail
cargo test --locked --test herd
```

A new test file needs no line anywhere: its shard is a hash of its name, the rule #1547 gives an unlisted target.

```sh
scripts/test-shards.sh 3     # the targets whose name hashes to shard 3, as cargo flags
```

A new `[limits]` key is written in `config.rs` only; the docs table follows from it:

```sh
just gen     # … also docs/config/limits.md and fleet.example.toml
```

A new optional fleet key is one new file; the example is the folder, joined:

```text
fleet.example.d/40-fleet-web/deps.toml
# deps = "monthly"   # #1488: `drover tick` runs `drover deps` every 30 days and the advisory check daily
```

A doc change edits the section's own file:

```text
docs/serve/screens.md      docs/lanes/spawn.md      docs/hosts/placement.md      docs/config/hygiene.md
```

## Slices, in order (this issue's PR, one commit each)

1. **Shards** (`scripts/test-shards.sh`, `scripts/test-shards.txt`, `.github/workflows/ci.yml`): the list and its
   check step are deleted; every target takes the hash shard #1547 gives an unlisted one.
2. **CI gates** (`.github/workflows/ci.yml`, the two gate folders, their runner): the `rust` job's 20 gate steps
   become 19 scripts and the `ui-webkit` job's 12 test steps 12; the WebKit path list is read from the gates.
3. **Limits and the example** (`docs/config.md`, `fleet.example.toml`, `justfile`, `crates/core/src/cli.rs`,
   `crates/core/src/propose.rs`): `config show --md` writes the table; the example is joined from a folder and
   loses its per-key `[limits]` lines.
4. **Docs** (`docs/serve.md`, `docs/lanes.md`, `docs/hosts.md`, `docs/config.md`, `.drover/docs.md`): text moves
   unchanged into one file per `##` section; each old page stays as a frozen index.
5. **Screens** (`crates/server/src/screens.rs` and its six sibling files): a pure move into
   `crates/server/src/serve/` as a `screens` folder of eight areas, each with its routes and its actions.
6. **The page script** (`.drover/arch.toml`): `web/src/main.ts` listed `shared`, with a size rule for `web/src`.

Slices 4 and 5 are moves, after the small slices so review reads those first. When integration changes one of the
moved files under the PR, the lane takes integration's file and repeats the move: a move is redone, never merged
by hand.

## What is not in this PR

The CLI folder and the limits macro (#1147's follow-up C and the macro half of D): `crates/core/src/cli.rs` and
`crates/core/src/config.rs` are `shared` since #1147 and not on this list. They stay as they are.
