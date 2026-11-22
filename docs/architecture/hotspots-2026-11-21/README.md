# hotspots: herdsman week of 2026-11-16 (#1548)

## Summary

Decision: this issue's PR converts seven of the nine files. The test shard list stops being hand-kept, the limits table and the example config are generated, the four docs become one file per section, and the page's main script is classed as shared code under a size limit. The screens file is #1556's, as you decided (q2870). New since your answer: the CI file is #1587's (the gatekeeper adoption, filed after it), so this PR builds no gate folders of its own.
Slices: 1 test shards (a new test file needs no line), 3 the limits table and the example config generated, 4 the four docs as one file per section, 6 the page script as shared. Not in this PR: 2 the CI gates (#1587), 5 the screens (#1556).
Open questions: (1) Slice 2 goes to #1587 instead of landing here (recommended), or is built here as first written and deleted again by #1587? With yes, #1587's spec gains one line (each WebKit test is its own gate file with its own paths) and should move up the queue. No question, your queue: #1556 should run right behind this issue, the screens file is 15 lines under its size limit.

Read this file, then `adr.md`, `shapes.md`, `boundaries.md`. `screens.md` is #1556's design, not this PR's. The folder
is named for the day the window ends: `docs/architecture/hotspots-2026-11-16/` is #1147's, filed for the same week.

## What changed since the first version (2026-11-21)

- **q2870, your revise.** Slice 5 is #1556. #1342 merged on 2026-11-22, so `screens.md` is redrawn over its typed
  builders.
- **#1586 merged.** The `rust` job is now `lint` and `release-tests`. The shard list is rewritten by a `balance`
  form from measured test times, and a new test file still needs a line in it. Slice 1 as first written (every
  target by a hash of its name, no list) would throw that balance away. It now keeps it.
- **#1587 filed and queued.** `ci.yml` becomes one call to gatekeeper's workflow, each check one gate file, and
  `ci.yml` is frozen. Slice 2's own gate folders and runner would be built this week and deleted the next.
- **#1547's PR (#1550) is conflicting and its issue is closed.** The first version leaned on it for the rule that an
  unlisted test target runs in a hash shard. Slice 1 now carries that rule itself.

## Where it stands (origin/staging d989c5be; counts from the 14 days to 2026-11-21)

| file | PRs | where they land | ending | by |
|---|---|---|---|---|
| `crates/server/src/screens.rs` | 52 | 27 the route or the action match; no screen area's own code over 16 | one file per area | #1556 |
| `scripts/test-shards.txt` | 44 | each adds a `<shard> <target>` line for a new test file | no line for a new test; the path goes | slice 1 |
| `.github/workflows/ci.yml` | 43 | 12 the WebKit path list, 11 a `rust` step, 9 a `ui-webkit` step | one gate file per check; frozen | #1587 |
| `docs/serve.md` | 41 | 15 the screens, 15 API v1, 7 health, 7 the page | one file per section; frozen index | slice 4 |
| `docs/config.md` | 38 | 25 the limits table | the table generated; the rest per section | slices 3, 4 |
| `docs/lanes.md` | 37 | 11 spawn, 11 drive, 8 check | one file per section | slice 4 |
| `docs/hosts.md` | 31 | 8 placement, 5 bootstrap, 4 or fewer each elsewhere | one file per section | slice 4 |
| `fleet.example.toml` | 27 | 14 the `[limits]` block, 9 one more optional `[fleet.web]` key | generated from a folder | slice 3 |
| `web/src/main.ts` | 24 | paint 10, stream 5, act 5, sheet 4: one loop over shared state | shared, under a size rule | slice 6 |

Why six of the nine were back this week: #1147 decided the CI file, the three docs and the limits table as follow-up
issues for the orchestrator to file on approve, and none was filed. This time the two pieces that are not in this
PR are issues that exist and are queued: #1556 and #1587.

## How the callers read, after this PR

A new test file needs no line anywhere. It runs in the shard its name hashes to until the next balance run places it:

```sh
scripts/test-shards.sh 3                         # shard 3's targets, as cargo flags
scripts/test-shards.sh balance shard-*.log       # writes scripts/test-shards.d/<today>.txt, removes the run before
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

And after #1587 and #1556, outside this PR: a new CI check is one new gate file, a new screen or action is an arm in
its own area's file (`screens.md`).

```toml
# .gatekeeper/gates/webkit-herd.toml
about = "the Herd in WebKit (#1092: a figure per lane, night, the next repo, the pinned swag scene)"
fix = "fix the WebKit test the log names"
leg = "ui-webkit"
paths = ["web/**", "assets/**", "crates/server/**", "crates/cli/tests/herd.rs"]
run = "cargo test --locked --test herd"
budget_secs = 120
```

## Slices of this PR, in order (one commit each; the numbers are the first version's)

1. **Shards** (`scripts/test-shards.sh`, `scripts/test-shards.txt`, `.github/workflows/ci.yml`,
   `crates/cli/tests/ci_split.rs`): the list moves, unchanged, into a folder that holds one balance run. A target it
   does not name runs in its hash shard. The `check` form and its `lint` step are deleted: no list can leave a
   target out or run it twice.
3. **Limits and the example** (`docs/config.md`, `fleet.example.toml`, `justfile`, `crates/core/src/cli.rs`,
   `crates/core/src/propose.rs`): `config show --md` writes the table; the example is joined from a folder and
   loses its per-key `[limits]` lines.
4. **Docs** (`docs/serve.md`, `docs/lanes.md`, `docs/hosts.md`, `docs/config.md`, `.drover/docs.md`): text moves
   unchanged into one file per `##` section; each old page stays as a frozen index.
6. **The page script** (`.drover/arch.toml`): `web/src/main.ts` listed `shared`, with a size rule for `web/src`.

Slice 4 is a move, after the small slices so review reads those first. When integration changes one of the four
pages under the PR, the lane takes integration's page and repeats the move: a move is redone, never merged by hand.

## What is not in this PR

- **The CI gates** (slice 2): #1587. `adr.md` says what this ADR asks of it.
- **The screens** (slice 5): #1556, blocked on this issue so the design in `screens.md` is on staging when it starts.
- The CLI folder and the limits macro (#1147's follow-up C and the macro half of D): `crates/core/src/cli.rs` and
  `crates/core/src/config.rs` are `shared` since #1147 and not on this list. #1564 splits the first.
