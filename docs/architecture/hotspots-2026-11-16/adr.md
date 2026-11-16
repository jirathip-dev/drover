# ADR: hotspots week of 2026-11-16: fix the list, then convert by class (#1147)

## Summary

Decision: most of the 283 listed files are not hot. 25 repo-wide sweeps (the rename, the comment strip, the move into crates) put 154 of them on the list, and every listed file runs its PRs one at a time, so 15 issues wait today on files nobody fights over. Fix the list first (skip sweeps, measure against the merge rate, refresh it daily); regenerate built files instead of merging them; let an alert say for itself that it reaches the phone; turn the agent help tail, the CI file and the three big docs into one file per area; and class the engine's big files as shared: kept from growing by a size limit, no longer one PR at a time.
Slices: 1 the list (sweeps, rate, daily refresh), 2 shared files with the size limit, 3 one command regenerates every built file, 4 phone alerts by flag, 5 the help tail as a folder; then four follow-up issues: CI per area, docs per area, and last week's two approved but never filed (the CLI folder, one entry per limits key).
Open questions: (1) hot means 1 in 20 merged PRs (33 files today, recommended) or stays at 5 PRs (133 files)? (2) the engine's 19 big files: shared with a size limit (recommended), or still one PR at a time, which queues 7 in 10 PRs behind each other?

Status: proposed (architect lane, 2026-11-16). Read `README.md` first.

## Question

The report names 283 files, and since #891 each one is a one-slot serial group. Which rows are real, which class is
each real one (a list, a built file, a doc, or code many PRs share by nature), and what takes each off the list?

## Where it stands

- `crates/core/src/hotspots.rs:12` counts every first-parent merge. 25 merges touch more than 40 files (the rename
  413, the comment strip 240, the theme packs 164, the move into crates 153); they lift 154 files over the bar of 5.
- The bar is a fixed 5 PRs in 14 days. At 439 merges that is 1 PR in 88: 133 files pass it without the sweeps.
- `tick` computes the list once a week. A file stays a serial group for up to 7 days after its churn ends.
- A PR takes all its hot groups at once or none (#1032). The median merge touches 6 listed files.
- `frozen` and `generated` are the only ways off the list. Code that is not a list has neither, so `drive.rs`,
  `spawn.rs` and `check.rs` stay one-at-a-time for good: 291 of 414 merges touch one of 19 such files.
- `web/dist/board.js` is 2 lines and `web/dist/board.css` 1: a built, minified file that every UI PR rewrites whole.
- `crates/core/src/config.rs:1063` lists the 44 kinds a phone gets by default; `crates/core/src/check.rs:719` reads
  it per alert. It is the list #973 named and left for this week.
- A `[limits]` key is written in five places: the struct, its default, `LIMIT_DOCS`, the table in `docs/config.md`
  and `fleet.example.toml`, whose comments `crates/core/src/propose.rs:248` reads as the settings screen's help.
- #973's slices 5 (the CLI folder), 6 (the limits macro) and 7 (the size ratchet) were approved; no issue was filed.

## Options

1. **Convert all 283 by class, as listed.** Sixty test files and forty docs become folders for churn that three
   sweeps made. Weeks of moves, each conflicting with every open PR.
2. **Fix the list only.** Sweeps and the rate bar leave 33 rows; the built files, the two lists and the help tail
   keep colliding, and the engine stays one PR at a time.
3. **Fix the list, convert what is a list or a built file, and give code that is neither its own class (chosen).**
   Every real row gets one of four endings: `frozen`, `generated`, split per area, or `shared` under a size limit.
4. **Turn hot-file serial off.** Undoes the owner's decision of Nov 12 for lists too, where one-at-a-time is right
   until the list is converted.

## Decision

- **Sweeps (slice 1).** A merge that touches more than 40 files is not counted. The header says how many were skipped.
- **The bar (slice 1).** Hot is `max(5, counted merges / 20)`: 21 of 414 today, 33 files. Open question 1.
- **Daily (slice 1).** `tick` stores the list every day and files the issue every 7. `drover hotspots` does both, as now.
- **Shared (slice 2).** `.drover/arch.toml` gains `shared`: files many PRs touch by nature. A shared file is off the
  list and out of the serial rule like a converted one. Each must fall under a `[[size]]` rule, or arch-check fails.
  Two rules go on with it: 150 lines a function and 2,000 a file in `crates/*/src`, 1,500 a file in tests; the baseline
  is written once, and a listed offender may not grow. New code lands in a new function, a new test in a new file.
  Only an ADR adds a `shared` line. Open question 2.
- **Built files (slice 3).** `just gen` writes the TS types, `web/dist`, `docs/workflow.md` and the brief goldens.
  All are listed `generated`; arch-check (`crates/core/src/arch.rs:329`) runs the command and fails on a difference.
  The `web` job keeps its typecheck and loses its dist diff; the `rust` job loses its diagram diff. One way to check.
- **Phone alerts (slice 4).** `Event` gains `push`, like `passive`: serde default, skipped when false.
  `Event::push(self)` sets it at the emit site. `Notify::wants` takes the event and asks `is_push()`: the flag, or a
  kind in the frozen list of the 44 old ones. `WEBPUSH_ON` is deleted; its two tests assert the emitted event.
- **The help tail (slice 5).** `roles/usage-tail.md` becomes a folder: one file a section, one a refusal code.
  `crates/core/build.rs` joins them in path order; `usage_doc` includes the result. The old file is deleted.
- **CI (follow-up A).** One workflow file per job area, job names unchanged so `required_checks` still match. The
  `rust` job keeps its setup, build and test steps; its 20 gate steps become one script each in a collected folder,
  run by one step. The `rust` workflow file is frozen. After #1162, which rewrites the test step.
- **Docs (follow-up B).** `docs/serve.md`, `docs/lanes.md` and `docs/hosts.md` become a folder each, one file per
  `##` section. Each old file stays as a short index and is frozen. After #1096, which deletes part of them.
- **The CLI and the limits (follow-ups C, D).** As approved in `docs/architecture/hotspots-2026-11-09/adr.md`. D adds:
  the table in `docs/config.md` becomes a generated file, the `[limits]` block leaves `fleet.example.toml`, and the
  settings screen reads its help from `LIMIT_DOCS`.
- **Left alone.** `assets/ui-audit.js`, `scripts/ui-audit.sh` and `fleet.example.toml` served the old board and decay.
  `Cargo.lock` is cargo's (13 PRs, 2 pairs). `.drover/arch.toml` has 2 real PRs.

## Consequences

- The list drops from 283 to 7 once the PR lands: the three docs, the CI file, the three decaying files. Each
  has a follow-up or an end date. Next week's table is what nobody has decided yet.
- 14 of the 15 held issues release within a day of the merge, once each host has updated itself and `tick` has
  run. The fifteenth waits on the CI file behind a PR that really edits it.
- An old binary cannot parse `shared` (the parser refuses unknown keys). Until a host updates, its drive pass logs
  that and keeps the old list (`crates/core/src/drive.rs:1242`); nothing else stops.
- Shared files take concurrent PRs again. Two that collide cost one update round: 15 to 20 rounds per
  100 PRs on these files over the 14 days. The size limit is the pressure that replaces the queue.
- Every lane that changes `check`, `spawn`, `pass`, `waits` or `dispatch` must carve its change into a new function
  from the first day. Four test files cannot grow: a new test is a new file, so one more test binary each.
- Two UI PRs still conflict in `web/dist`: the update round reruns `just gen` and commits. No hand merge.
- arch-check now needs `npm ci` in `web` and one golden test run; a few seconds in the `rust` job.
- A new pushable alert or refusal code is one line at its source or one new file. The usage text is one code a
  line, alphabetical: longer by about 30 lines, easier to look up.
- Behaviour is unchanged except: which files are hot, and that a shared file's PRs no longer wait.
  Rough size: `src/` net about +60 (the list +25, `shared` +15, `push` +20, the build script +15, `WEBPUSH_ON` −45).
- Docs: `docs/config.md` (hotspots and hot files paragraphs), `docs/arch-check.md` (`shared`, `just gen`),
  `docs/serve.md` (the alerts default).

## Open questions for the owner

1. **The bar.** 1 in 20 merged PRs, floor 5 (recommended: 33 files today, and it follows the fleet's pace), or keep
   5 PRs (133 files after the sweeps are skipped).
2. **The engine's big files.** `shared` with the size limit (recommended), or keep them one PR at a time: then 291
   of 414 PRs queue behind each other, 3.5 groups each. With "keep", slice 2 is the size limit alone.

## The `.drover/arch.toml` lines

This PR (slices 2, 3, 4). The top-level keys replace today's three; the `[[size]]` tables go below the bans:

```toml
frozen = ["ci/cap.toml", "crates/core/src/events/passive.rs", "crates/core/src/events/push.rs"]
generated = [
    "assets/types/api.d.ts",
    "web/dist/board.js",
    "web/dist/board.css",
    "docs/workflow.md",
    "crates/cli/tests/golden/briefs/design.md",
    "crates/cli/tests/golden/briefs/impl.md",
    "crates/cli/tests/golden/briefs/impl-fix.md",
    "crates/cli/tests/golden/briefs/impl-repro.md",
    "crates/cli/tests/golden/briefs/impl-update.md",
    "crates/cli/tests/golden/briefs/rev.md",
    "crates/cli/tests/golden/briefs/rev-repro.md",
]
generate = "just gen"
shared = [
    "crates/core/src/check.rs",
    "crates/core/src/cli.rs",
    "crates/core/src/cloud.rs",
    "crates/core/src/config.rs",
    "crates/core/src/doctor.rs",
    "crates/core/src/drive.rs",
    "crates/core/src/events.rs",
    "crates/core/src/gate.rs",
    "crates/core/src/hook.rs",
    "crates/core/src/hosts.rs",
    "crates/core/src/lib.rs",
    "crates/core/src/queue.rs",
    "crates/core/src/spawn.rs",
    "crates/core/src/timer.rs",
    "crates/core/src/waiting.rs",
    "crates/server/src/api.rs",
    "crates/server/src/serve.rs",
    "crates/server/src/serve/chat.rs",
    "crates/server/src/serve/v1.rs",
    "crates/*/tests/**",
]

[[size]]
paths = ["crates/*/src/**/*.rs"]
max_file_lines = 2000
max_fn_lines = 150

[[size]]
paths = ["crates/*/tests/**/*.rs"]
max_file_lines = 1500
```

Slice 5 deletes `roles/usage-tail.md`: no line. Follow-up A adds:

```toml
frozen = [".github/workflows/rust.yml"]   # appended to the list above

[[collect]]
dir = "ci/gates"
glob = "*.sh"
```

Follow-up B adds:

```toml
frozen = ["docs/serve.md", "docs/lanes.md", "docs/hosts.md"]   # appended

[[collect]]
dir = "docs/serve"
glob = "*.md"

[[collect]]
dir = "docs/lanes"
glob = "*.md"

[[collect]]
dir = "docs/hosts"
glob = "*.md"
```

Follow-up C replaces the `shared` entry for `cli.rs` with the folder's `mod.rs`. Follow-up D adds `"docs/limits.md"` to
`generated` and one line to `just gen`.
