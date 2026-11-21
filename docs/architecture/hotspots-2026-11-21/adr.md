# ADR: hotspots week of 2026-11-16: nine files, one PR, no follow-ups (#1548)

## Summary

Decision: six of these nine files were already decided in #1147, as follow-up issues that were never filed, so nothing is left for a follow-up this time: this issue's one PR converts all nine. Each list becomes one file per entry (a CI gate, a screen area with its own routes and actions, a doc section, an example key). The test shard list and the limits table stop being written by hand. The page's main script is classed as shared code under a size limit.
Slices: 1 test shards without a list, 2 CI gates as two folders, 3 the limits table and the example config generated, 4 the four docs as one file per section, 5 the screens as one file per area, 6 the page script as shared.
Open questions: (1) #1342 (blocks D4) is second in the queue and rewrites every builder in the screens file this PR moves: hold it until this PR merges (recommended), or let it go first? Two choices you may want to veto: CI stays one workflow file (splitting it would break the release trigger), and a red CI run now names every failing gate instead of stopping at the first.

Status: proposed (architect lane, 2026-11-21). Read `README.md` first.

## Question

The report names nine files. For each: where do its PRs really land, which class is it (a list, a written-twice
table, a page of sections, or code many PRs share by nature), and what takes it off the list in this PR?

## Where it stands

- `crates/server/src/screens.rs` is 1,921 lines under a 2,000-line limit. It holds every screen's builder and two
  `match` lists: the route of each screen and the handler of each action. 27 of its 52 PRs add or change an arm of
  one; the busiest screen area (the queue's) has 16. `action` is 170 lines, over the 150 limit, held by the baseline.
- `scripts/test-shards.txt` assigns each of about 250 test targets to one of four CI shards by hand. A new test file
  must add a line or CI fails. #1547 (its PR is open) lets an unlisted target run in the shard its name hashes to;
  the list then decides nothing a rule could not.
- `.github/workflows/ci.yml` has three lists: the `rust` job's 20 gate steps, the `ui-webkit` job's 12 test steps,
  and one regular expression naming the files whose change runs `ui-webkit`. A new WebKit test edits two of them.
- `.github/workflows/release.yml` starts on `workflow_run` of the workflow named `ci`. Branch protection and the
  fleets' `required_checks` name CI's jobs.
- The limits table in `docs/config.md` is `drover config show`'s output for the defaults, copied by hand and held to
  it by a test in `crates/cli/tests/limits.rs`. `fleet.example.toml` repeats 48 of the keys with a third wording.
- `crates/core/src/propose.rs` reads the example's comments as the settings screen's help text.
- The four docs are pages of `##` sections. No section has more than 15 of its page's PRs but the limits table (25).
- `web/src/main.ts` is 701 lines: the page's view loop. Its busy functions (`paint`, `load`, `stream`, `go`, `show`,
  `act`) call each other over the same module state; 17 of its 24 PRs touch one of them. #1341 (blocks D3, queued)
  replaces the file with one `mount` call.
- #1342 (blocks D4, second in the queue, not blocked) turns the about 170 `json!` blocks of `screens.rs` and
  `crates/server/src/serve/chat.rs` into typed constructors: an edit in every builder this PR moves.
- #1147's follow-ups A (CI), B (docs), C (the CLI folder) and D (limits) were approved and never filed.

## Options

1. **File follow-up issues per class, as #1147 did.** Four were approved, two of them twice (#973, #1147), and none
   was filed; six of this week's nine rows are the result.
2. **List all nine `shared`.** One line each and the list is empty. The lists keep colliding (the report counts 72
   update rounds across the nine rows), and `screens.rs` is 79 lines from its size limit anyway.
3. **Convert each by its class, all in this PR (chosen).** Lists become folders, the two tables become generated,
   docs split per section, and only the one file that is code by nature is listed `shared`.
4. **CI as one workflow file per job** (#1147's shape A). The release would start when any one of five workflows
   ends green. Reusable workflows called from `ci.yml` keep one workflow but rename every check (`rust / rust`),
   which branch protection names. Rejected: the step lists are what collide, and they can leave the file.

## Decision

- **Shards (slice 1).** `scripts/test-shards.txt` is deleted. Every target, `unit` included, runs in the shard
  its name hashes to: #1547's rule for an unlisted target, now the only rule. A target is in exactly one shard by
  construction, so the script's `check` form and the `rust` job's "test shards" step are deleted too.
- **CI gates (slice 2).** Each gate step becomes a script in a folder per job, run by one step through one runner.
  The runner runs every gate, each in its own log group, then fails naming each failed gate; a gate whose name ends
  `.soft.sh` only warns (today's `continue-on-error` step). A WebKit gate's `# paths:` line names the files whose
  change runs the job; the `changes` job asks the runner instead of holding the list. Setup steps (checkout,
  toolchain, cache, node, tools, the release build, Playwright) stay in `ci.yml`. Job ids and names do not change.
  `ci.yml` is frozen: a line added to it fails arch-check; an edited one (a version bump) passes.
- **The limits table (slice 3).** `config show --md` prints the defaults as the Markdown table, with no fleet.toml
  read. `just gen` writes it to `limits.md` in the config docs folder; the file is listed `generated`. The test
  keeps "every key is shown" and loses the table comparison: arch-check does that now.
- **The example (slice 3).** `fleet.example.toml` is listed `generated`: `just gen` joins a folder of fragments in
  path order. Each table is a file; each optional `[fleet.web]` key is its own file. The `[limits]` fragment keeps
  the table header and one comment pointing at the generated table; its per-key lines are deleted, and
  `propose::view` takes a limits key's help from `LIMIT_DOCS`. A key is then written in three places, all in
  `config.rs` (the field, its default, its doc line), down from five.
- **Docs (slice 4).** Each `##` section of the four pages moves, text unchanged, to its own file in a folder named
  for the page. Each old page stays, frozen, as a title and one line saying where its sections are; it lists no
  section, so a new one needs no line there. Code, tests and rule lines that name the old page keep working. The
  four links that carry an anchor and the three tests that read a page's text are pointed at the section's file.
- **Screens (slice 5).** The file and its six siblings move to a `screens` folder under `crates/server/src/serve/`,
  where `serve.rs` finds it without a `#[path]`. Eight areas (queue, lanes, events, cards, fleets, hosts, scene,
  threads) each export one `AREA`: its screens, its actions, and the actions an owner may run that the plugin
  manifest does not list. The router (`mod.rs`) asks each area in turn and names no route and no action. A pure
  move: every screen's JSON and every action's reply stay byte for byte, held by the existing golden tests.
- **The page script (slice 6).** `web/src/main.ts` is listed `shared`. A split was weighed and rejected: the six
  busy functions are one loop and would land in one file again, and #1341 deletes most of the file. A new size rule
  covers `web/src`: 800 lines a file, 80 a function, baseline written once (`composer` in `web/src/blocks.ts`, 119
  lines, is its one entry).
- **Guards.** Two rules keep the router from growing back into a list: a 300-line limit on it, and a ban on a
  `"/screens/` literal in it. The `.build_screen(` ban's exceptions are renamed to the new paths.

## Consequences

- Seven rows leave the list on drive's first pass after the merge (frozen, generated or shared). The two deleted
  paths leave at the next daily store, within a day.
- A new test file, CI gate, WebKit test, screen, action, doc section or optional fleet key no longer edits a file
  another PR edits. A new limits key still edits `config.rs`, which is `shared`.
- CI's `rust` and `ui-webkit` jobs show one gates step. Its log has a group per gate; a failure is an `::error::`
  line and a job-summary row naming the gate. A red run takes longer (the perf gate runs after a failed `fmt`) and
  costs fewer fix rounds: one run shows everything that is red.
- Any gate runs on a host as it does in CI: the lane runs the gate's file.
- Shards are balanced by the hash, not by hand: 62, 61, 68 and 61 targets today, and a target never changes shard.
  The PR body states the four shard times before and after; the `test` check and its name do not change.
- A new CI job or setup step is refused by arch-check. It needs the freeze lifted in the same PR, where review sees it.
- The example loses its block of tuned `[limits]` values; `drover config show` and the generated table carry
  them. The settings screen's help for a limits key is the `LIMIT_DOCS` line, shorter than the example's was.
- Agents find docs one level deeper. `.drover/docs.md`'s first line names the four folders; `.drover/rules.md`'s
  deps line names the gate file instead of `docs/serve.md`, which does not hold the list.
- The screens move adds import lines and eight area tables: `src/` net about +150 against today's headroom of
  298 under the line cap. If the queue has used it by then, the cap rule applies as for any PR.
- The baseline loses `screens.rs:action 170`. A fleet's serial group that names the old screens path matches
  nothing afterwards; the new folder is under `crates/server/src/serve/**`, which that group already lists.
- `web/src/main.ts` takes concurrent PRs: a collision costs one update round (8 in the window). It cannot pass 800
  lines; new page code lands in a new module.
- The PR touches about 150 files, most of them moved text: a sweep, so next week's list does not count it. Every
  open PR on a moved file gets one update round after it merges.
- No new arch.toml key: an older binary on a host reads the new file as it reads today's.

## Open questions for the owner

1. **#1342 and the screens move.** Both touch every builder: whichever merges second redoes its work on the other's
   result. Recommended: hold #1342 until this PR merges (a queue block on #1548; the owner's or the orchestrator's
   to set, not a lane's). A move is cheap to do first and costly to redo over a rewrite, and D4's 170 edits then
   land in eight files that other PRs share less. Its AC and its `json!` ban then read "the screens folder" for
   `screens.rs` (an amendment). The other way round, slice 5 waits for #1342 and moves its result; slices 1 to 4
   and 6 need not wait either way.

## The `.drover/arch.toml` lines

The top-level keys replace today's; the tables are added. Each slice's commit adds its own lines.

```toml
frozen = [
    "ci/cap.toml",
    "crates/core/src/events/passive.rs",
    "crates/core/src/events/push.rs",
    ".github/workflows/ci.yml",                          # slice 2
    "docs/serve.md",                                     # slice 4
    "docs/lanes.md",
    "docs/hosts.md",
    "docs/config.md",
]
generated = [
    # … today's eleven paths
    "docs/config/limits.md",                             # slice 3
    "fleet.example.toml",
]
generate = "just gen"
shared = [
    # … today's twenty globs
    "web/src/main.ts",                                   # slice 6
]

[[collect]]                                              # slice 2
dir = "ci/gates/rust"
glob = "*.sh"

[[collect]]
dir = "ci/gates/ui-webkit"
glob = "*.sh"

[[collect]]                                              # slice 4
dir = "docs/serve"
glob = "*.md"

[[collect]]
dir = "docs/lanes"
glob = "*.md"

[[collect]]
dir = "docs/hosts"
glob = "*.md"

[[collect]]
dir = "docs/config"
glob = "*.md"

[[size]]                                                 # slice 5
paths = ["crates/server/src/serve/screens/mod.rs"]
max_file_lines = 300

[[ban]]
pattern = '"/screens/'
paths = ["crates/server/src/serve/screens/mod.rs"]
reason = "#1548: a route is matched in its area's file (crates/server/src/serve/screens/<area>.rs), never in the router"

[[size]]                                                 # slice 6
paths = ["web/src/**/*.ts"]
max_file_lines = 800
max_fn_lines = 80
```

Edited in place:

```toml
[docs]
skip = [".drover/**", "roles/usage-tail.md", "scripts/test-shards.txt", "crates/server/src/screens.rs"]

[[ban]]
pattern = ".build_screen("
except = ["crates/server/src/serve/hub.rs", "crates/server/src/serve/screens/mod.rs", "crates/server/src/serve/screens/local.rs"]
```

Slices 1 and 5 delete their hot file (`scripts/test-shards.txt`, `crates/server/src/screens.rs`): no `frozen` line.
The `skip` entries keep this folder's and #1147's mentions of the two deleted paths from failing the docs check.
