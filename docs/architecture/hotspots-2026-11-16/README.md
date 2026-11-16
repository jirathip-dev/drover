# hotspots: herdsman week of 2026-11-16 (#1147)

## Summary

Decision: most of the 283 listed files are not hot. 25 repo-wide sweeps (the rename, the comment strip, the move into crates) put 154 of them on the list, and every listed file runs its PRs one at a time, so 15 issues wait today on files nobody fights over. Fix the list first (skip sweeps, measure against the merge rate, refresh it daily); regenerate built files instead of merging them; let an alert say for itself that it reaches the phone; turn the agent help tail, the CI file and the three big docs into one file per area; and class the engine's big files as shared: kept from growing by a size limit, no longer one PR at a time.
Slices: 1 the list (sweeps, rate, daily refresh), 2 shared files with the size limit, 3 one command regenerates every built file, 4 phone alerts by flag, 5 the help tail as a folder; then four follow-up issues: CI per area, docs per area, and last week's two approved but never filed (the CLI folder, one entry per limits key).
Open questions: (1) hot means 1 in 20 merged PRs (33 files today, recommended) or stays at 5 PRs (133 files)? (2) the engine's 19 big files: shared with a size limit (recommended), or still one PR at a time, which queues 7 in 10 PRs behind each other?

Read this file, then `adr.md`, `shapes.md`, `boundaries.md`. It continues `docs/architecture/hotspots-2026-11-09/`: that
ADR's slices 1 to 4 merged today (#973); its slices 5 to 7 were approved and never filed, and are carried here.

## Where it stands (origin/staging cbda5640, 14 days to 2026-11-16)

439 merges. 25 of them touch more than 40 files each; the median merge touches 8.

| what the list says | measured | what it is |
|---|---|---|
| 283 hot files | 154 pass the bar of 5 only through sweeps | noise: slice 1 |
| each is a one-slot serial group (`crates/core/src/queue.rs:304`) | the median merge touches 6 listed files and needs all 6 slots at once | the fleet runs near one at a time |
| 15 live issues held `serial: hot <file>` | 8 of the 12 files they wait on have 1 to 4 real PRs | slice 1 frees them |
| `web/dist/board.js`, `web/dist/board.css` | 2 lines and 1 line, minified: any two UI PRs conflict (99 and 55 pairs) | generated: slice 3 |
| `crates/core/src/config.rs`, 83 PRs | 28 add a kind to `WEBPUSH_ON` (44 entries); 26 add a `[limits]` key (five places a key) | lists: slice 4, follow-up D |
| `roles/usage-tail.md`, 50 PRs | 25 on the refusal codes: lines up to 2,043 characters, ten codes a line | a list: slice 5 |
| `.github/workflows/ci.yml`, 37 PRs | every one under `jobs:`; the `rust` job is 22 named steps | a list: follow-up A |
| `docs/serve.md`, `docs/lanes.md`, `docs/hosts.md` | 49, 39, 33 PRs, two sections each take most | per area: follow-up B |
| `crates/core/src/cli.rs`, 105 PRs | 90 on `dispatch` (1,015 lines) | follow-up C, approved in #973 |
| 19 engine files (`drive.rs`, `spawn.rs`, `check.rs`, …) | 291 of 414 merges touch one; 3.5 of them each | not lists: slice 2 |
| `assets/ui-audit.js`, `scripts/ui-audit.sh`, `fleet.example.toml` | 1, 1 and 2 PRs in the last 3 days | the old board's; they decay by Nov 30 |

The engine's hunks land where they did last week: `check` (532 lines), `spawn` (490), `pass` (366), `waits` (298).
They are behaviour, not lists; no folder takes them off the list. The four test files over 1,500 lines
(`crates/cli/tests/drive.rs`, `crates/cli/tests/shed.rs`, `crates/cli/tests/hosts.rs`, `crates/cli/tests/hook.rs`) are the
four hot test files, and each follows its source file.

## How the callers read, after

The report's header says what it counted; a file is hot against the merge rate:

```text
Hot files in herdsman: 21 or more of 414 merged PRs (1 in 20) in the last 14 days
25 sweeps (over 40 files each) not counted · 23 shared files not listed
```

The serial rule reads a list that is one day old at most (`tick` stores it daily, files the issue weekly):

```rust
hotspots::daily(ctx, cfg, &pass, out)?;   // was `weekly`: store every day, file every 7
```

An alert that reaches the phone says so where it is born; nothing is added to a list:

```rust
fire_event(ctx, cfg, out, events, Event::new(ctx.now, fleet, None, "runners-low", d).push())?;
```

A built file is never merged by hand. One command writes all of them, and arch-check runs the same one:

```sh
just gen     # types, web/dist, docs/workflow.md, the brief goldens
```

A new refusal code is one new file; core's build script joins the folder into the help text:

```text
roles/usage/30-refusals/SHED_OLD.md
- SHED_OLD: spawn: an ACP lane on a host whose shed has no `run --meta` does not start
```

A shared file takes any number of PRs at once, and cannot grow:

```text
crates/core/src/check.rs:64: `crates/core/src/check.rs:check` is 540 lines, over 150 (baseline Some(532))
```

## Slices, in order (this issue's PR, one commit each)

1. **The list** (`crates/core/src/hotspots.rs`, `crates/core/src/timer.rs`, `docs/config.md`): a merge over 40 files
   is not counted; hot is `max(5, merges / 20)`; `tick` stores the list daily and files the issue weekly.
2. **Shared files** (`crates/core/src/arch.rs`, `.drover/arch.toml`, the baseline, `docs/arch-check.md`): the `shared`
   key, two `[[size]]` rules, the baseline written once. Depends on open question 2.
3. **One generator** (`justfile`, `.drover/arch.toml`, `.github/workflows/ci.yml`): `just gen`; four more paths
   listed `generated`; CI's own dist diff and diagram diff steps deleted.
4. **Phone alerts by flag** (`crates/core/src/events.rs`, `crates/core/src/config.rs`, `crates/core/src/check.rs`):
   the `push` field, the frozen list of old kinds, `Notify::wants` on the event.
5. **The help tail as a folder** (`roles/usage-tail.md`, `crates/core/build.rs`, `crates/core/src/cli.rs`): one file a
   section, one a refusal code, joined at build time.

## Follow-up issues (the orchestrator files them on approve)

| | title | queued | waits for |
|---|---|---|---|
| A | `ci: one workflow file per job area; the rust job's gates as a folder` | plain | #1162 (it rewrites the same job) |
| B | `docs: serve, lanes and hosts as one file per section` | plain | #1096 (the docs sweep) |
| C | `cli: the dispatch as one file per command area` | `--refactor` | this PR |
| D | `limits: one entry per key; the docs table generated; the example's block removed` | plain | this PR |

A and B are specified in `shapes.md`. C is slice 5 and D is slice 6 of `docs/architecture/hotspots-2026-11-09/adr.md`,
unchanged; D also takes the table in `docs/config.md` and the `[limits]` block of `fleet.example.toml` (see `adr.md`).
