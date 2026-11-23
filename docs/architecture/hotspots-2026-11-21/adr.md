# ADR: hotspots week of 2026-11-16: seven files in this PR, two in issues that exist (#1548)

## Summary

Decision: this issue's PR converts seven of the nine files. The test shard list stops being hand-kept, the limits table and the example config are generated, the four docs become one file per section, and the page's main script is classed as shared code under a size limit. The screens file is #1556's, as you decided (q2870). New since your answer: the CI file is #1587's (the gatekeeper adoption, filed after it), so this PR builds no gate folders of its own.
Slices: 1 test shards (a new test file needs no line), 3 the limits table and the example config generated, 4 the four docs as one file per section, 6 the page script as shared. Not in this PR: 2 the CI gates (#1587), 5 the screens (#1556).
Open questions: (1) Slice 2 goes to #1587 instead of landing here (recommended), or is built here as first written and deleted again by #1587? With yes, #1587's spec gains one line (each WebKit test is its own gate file with its own paths) and should move up the queue. No question, your queue: #1556 should run right behind this issue, the screens file is 15 lines under its size limit.

Status: revised 2026-11-22 after q2870 (architect lane; first version 2026-11-21). Read `README.md` first.

## Question

The report names nine files. For each: where do its PRs really land, which class is it (a list, a written-twice
table, a page of sections, or code many PRs share by nature), and what takes it off the list: this PR, or an issue
that exists?

## Where it stands (origin/staging d989c5be)

- `scripts/test-shards.txt` assigns each of 256 test targets to one of four CI shards. Since #1586 the
  `balance` form of `scripts/test-shards.sh` rewrites it from nextest's times in a shard job's log. A new test
  file must still add a line by hand or the `lint` job's "test shards" step fails: that line is 44 PRs.
- `.github/workflows/ci.yml` has, since #1586, a `lint` job of 17 check steps, a `release-tests` job of 5 and a
  `ui-webkit` job of 12. The list of files whose change runs `ui-webkit` moved out of it into the
  `ci-changes.sh` script, where a new WebKit test still edits one shared line.
- #1587 (queued, not blocked) replaces `ci.yml` with one call to gatekeeper's workflow. Each check becomes a gate
  file in a folder, the test suite a gate with four shards, and `ci.yml` is frozen. Its spec carries the
  `arch.toml` lines.
- The limits table in `docs/config.md` is `drover config show`'s output for the defaults, copied by hand and held to
  it by a test in `crates/cli/tests/limits.rs`. `fleet.example.toml` repeats 48 of the keys with a third wording.
- `crates/core/src/propose.rs` reads the example's comments as the settings screen's help text.
- The four docs are pages of `##` sections. No section has more than 15 of its page's PRs but the limits table (25).
- `web/src/main.ts` is 701 lines: the page's view loop. Its busy functions (`paint`, `load`, `stream`, `go`, `show`,
  `act`) call each other over the same module state; 17 of its 24 PRs touch one of them. #1341 (blocks D3, queued)
  replaces the file with one `mount` call.
- `crates/server/src/screens.rs` is 1,985 lines under a 2,000-line limit, after #1342 rebuilt its screens on typed
  builders. Its two `match` lists (the route of each screen, the handler of each action) are as they were.
- #1147's follow-ups A (CI), B (docs), C (the CLI folder) and D (limits) were approved and never filed.

## Options

1. **File follow-up issues per class, as #1147 did.** None of its four was filed; six of this week's nine rows are
   the result. A follow-up works only when the issue exists before the ADR is approved.
2. **List all nine `shared`.** One line each and the list is empty. The lists keep colliding (the report counts 72
   update rounds across the nine rows), and `screens.rs` is 15 lines from its size limit anyway.
3. **Convert all nine in this PR** (the first version). Rejected by q2870 for the screens. For the CI file it would
   now add a gate runner and two gate folders that #1587 deletes.
4. **Convert seven here; the other two are #1556 and #1587, both filed and queued (chosen).**
5. **Shards by a hash of the name alone, no list** (the first version's slice 1). Shards would be even by count,
   not by time, and #1586's balance (the slowest shard at most 1.3 times the fastest) would be lost. Rejected.
6. **Shards by nextest's own partition** (`--partition count:n/4`). No list and even shards, but every shard
   compiles all 256 test binaries where it compiles 64 today. Rejected.

## Decision

- **Shards (slice 1).** The list is no longer the roll of every target. It is one balance run's result, in a folder
  that holds one file, named for its day. A target it names runs in that shard. A target it does not name runs in
  the shard its name hashes to. A name with no test file, or with a shard outside 1 to 4, is ignored. Every target
  is therefore in exactly one shard whatever the file holds, so the `check` form and the `lint` job's "test shards"
  step are deleted. `balance` writes a new file and removes the one before it. A new or deleted test file edits
  nothing.
- **The CI file (slice 2): #1587.** This PR touches `ci.yml` only to delete the "test shards" step. What this ADR
  asks of #1587, for you to amend into its spec:
  - Each check that is a step today is its own gate file. Each of the 12 WebKit tests is one, with its own `paths`,
    in the `ui-webkit` leg; a gate never holds a list of tests. Otherwise the hot list only moves into one gate file.
  - The `changes` job and the `ci-changes.sh` script go: what a gate reads is in its own file.
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
- **The screens (slice 5): #1556.** The design is `screens.md`: eight areas, each with its routes and its actions,
  behind a router that names neither. #1556's PR adds that file's `arch.toml` lines.
- **The page script (slice 6).** `web/src/main.ts` is listed `shared`. A split was weighed and rejected: the six
  busy functions are one loop and would land in one file again, and #1341 deletes most of the file. A new size rule
  covers `web/src`: 800 lines a file, 80 a function, baseline written once (`composer` in `web/src/blocks.ts`, 119
  lines, is its one entry).

## Consequences

- Six rows leave the list on drive's first pass after the merge: the four docs (frozen), the example (generated),
  the page script (shared). `scripts/test-shards.txt` no longer exists and leaves at the next daily store.
- `.github/workflows/ci.yml` and `crates/server/src/screens.rs` stay on the list until #1587 and #1556 merge. Next
  week's hotspots issue will name them again; its answer is this ADR. Neither can be frozen before then: nothing
  else would take a new step or a new screen.
- A new test file, doc section or optional fleet key no longer edits a file another PR edits. A new limits key
  still edits `config.rs`, which is `shared`.
- Shards stay as even as the last balance run made them. A target added since then sits in its hash shard until
  the next run, where today it sits wherever its author typed. Who runs `balance`, and when, is as #1586 left it.
- The example loses its block of tuned `[limits]` values; `drover config show` and the generated table carry
  them. The settings screen's help for a limits key is the `LIMIT_DOCS` line, shorter than the example's was.
- Agents find docs one level deeper. `.drover/docs.md`'s first line names the four folders. The dependency
  allow-list moves with its section (Alerts), so `.drover/rules.md`'s deps line and the `lint` job's deps step name
  that section's file.
- arch-check's message for a line added to a frozen file names every collected folder, four more after this PR.
- `src/` net is about +20 (the `--md` form and the limits help), against 1,647 lines of headroom under the cap.
- `web/src/main.ts` takes concurrent PRs: a collision costs one update round (8 in the window). It cannot pass 800
  lines; new page code lands in a new module.
- The PR touches about 80 files, most of them moved text: a sweep, so next week's list does not count it. Every
  open PR on one of the four docs gets one update round after it merges.
- No new arch.toml key: an older binary on a host reads the new file as it reads today's.

## Open questions for the owner

1. **Slice 2 and #1587.** Your answer said slices 1 to 4 and 6 land now; #1587 was filed after it. Recommended:
   slice 2 is #1587, with the two lines above amended into its spec, and #1587 moved up the queue, since `ci.yml`
   stays hot until it merges. The other way: this PR builds the first version's two gate folders and their runner
   over the new `lint` and `ui-webkit` jobs, and #1587 turns each script into a gate file a few days later.

Not a question, but yours or the orchestrator's to do: #1556 is queued behind this issue at place 21. It is worth
running next: any PR that adds 15 lines to the screens file fails the size rule.

## The `.drover/arch.toml` lines

This PR. The top-level keys replace today's; the tables are added. Each slice's commit adds its own lines.

```toml
frozen = [
    "ci/cap.toml",
    "crates/core/src/events/passive.rs",
    "crates/core/src/events/push.rs",
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

[[collect]]                                              # slice 1
dir = "scripts/test-shards.d"
glob = "*.txt"

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

[[size]]                                                 # slice 6
paths = ["web/src/**/*.ts"]
max_file_lines = 800
max_fn_lines = 80
```

Edited in place, so this folder's and #1147's mentions of the deleted list do not fail the docs check:

```toml
[docs]
skip = [".drover/**", "roles/usage-tail.md", "scripts/test-shards.txt"]
```

Slice 1 deletes its hot file's path: no `frozen` line.

#1587's PR (from its spec; here so the nine rows are in one place):

```toml
frozen = [
    # …
    ".github/workflows/ci.yml",
]

[[collect]]
dir = ".gatekeeper/gates"
glob = "*.toml"
```

#1556's PR: the lines at the end of `screens.md`.
