# ADR: test diet: the audit of the suite and the cut, under gatekeeper's test ratchet (#1760)

## Summary

Decision: drover builds no test cap. The ratchet is gatekeeper's test-health check (gatekeeper#99), which drover gets through that check's own adoption issue and which starts at the ratio of that day: 1.50 if it were today. This ADR is the audit and the cut: show a test's cover before it goes, then cut. The audit read all 288 test files: the excess is copied fixtures and tests of features on their way out, far more than duplicate tests. Its verdicts and the three retirements you approved take tests from 1.47 times src to about 1.05; 1.00 needs the page-client tests to move (#1665) and the builders used inside test bodies.
Slices: 1 the keep check, with cargo-llvm-cov; 2 slow tests off the PR path (2,011 s of test time per run to about 1,320 s, about 900 s once the cuts land); 3 and 4 the deletes; 5 what you retired: the pre-rename move, the form route once no client is found calling it, the sim owner tools, parity rows 2, 20 and 63; 6 shared builders, proven on one area; 7 to 10 the other areas; 11 one test binary per area (16 GB of test executables per lane to about 3); 12 the close, once gatekeeper's check is on.
Open questions: none.

## Status

Revised by the architect lane on 2026-11-25 after the owner's q3678 (the first version is `f3ea16fda`). Read on
staging `4cf52e4bf` and on gatekeeper's approved ADR for #99 (`docs/architecture/test-health/` on its branch
`issue-99`). Read `README.md` first, then this. `inventory.md` is the verdict per test file, `usage.md` how the
callers read, `shapes.md` the data shapes, `boundaries.md` who may call what and the statecharts, `issues.md`
the twelve issues as they are to be filed.

What q3678 changed: the test-cap slice is gone and nothing in drover counts test lines; the three retirements
and the three parity rows are decided and have a slice of their own (5); cargo-llvm-cov is approved; slow tests
move into whole files, as gatekeeper's check wants them, not into a module convention of drover's own.

## Question

drover has 94,937 lines of Rust tests against 64,744 lines of src (1.47). Every lane builds and runs them. Which
tests can go with every behaviour still covered, how is that shown before each delete, what ratio is reachable,
and what does the diet owe the ratchet that gatekeeper brings?

## What the audit found

Eleven read-only passes read every test body of every file, by area, and read both sides of every claimed
duplicate. `inventory.md` holds the result per file. Measured on staging `4cf52e4bf` and its CI run:

| Fact | Number |
|---|---|
| Test lines / src lines | 94,937 / 64,744 |
| The same by gatekeeper's count (no blank lines; a src file's `#[cfg(test)]` tail is test) | 90,260 / 61,322: 1.47 |
| Tests / test binaries | 2,064 / 259 (one binary per top-level file) |
| Lines outside any test body (helpers, fixtures) | 31,772 (34%) |
| The same helper written again | `kinds` in 54 files (36 byte-identical), `setup` in 63, `ev` in 39, `fake` in 35 |
| The same scripted replies | the host sample in 67 files, a PR's JSON 68 times, a real git origin and clone 9 times, a raw HTTP client in about 40 files |
| Summed test time of one CI run | 2,011 s; ten binaries take 60% of it |
| Tests over gatekeeper's 2 s budget | 143 of 2,060, and they take 1,603 s of the 2,011 s |
| One lane's build folder | 265 test executables, 16.2 GB of its 19.8 GB |

Six findings carry the plan:

1. **Duplicates are real but small: about 2,700 lines.** Most tests assert one distinct edge once. The clear
   cases: `drive_sim.rs` is a second, hand-written simulator whose three main-host tests restate one-pass tests
   (385 lines, 82 s per run); a handful of incident replays restate the rule beside them; the same operation is
   asserted through two routes that end in one handler (`api_v1_ops.rs` against `lanes_sheet.rs`,
   `lane_actions.rs`, `setup_terminal.rs`).
2. **Fixtures are the mass: about 4,700 lines of copies outside test bodies**, and more inside them (30 six-line
   verdict events in `drive.rs` alone). `common/` knows a fake runner and a fake shed, but not a fleet, an issue,
   a lane or a PR, so each file rebuilds those from strings.
3. **Many tests are rows of a table written as tests: about 4,000 lines.** `hook.rs` has 34 allow/deny tests and
   10 copies of the hook runner; `spawn_guard.rs` 10 admission rows; `protection.rs` 11 outputs of one verb.
4. **Implementation-detail and dead tests: about 1,600 lines.** Source greps (`term_adapter.rs`), doc-string pins
   the brief goldens already hold (`spawn.rs`), the pinned theme pack's own content re-tested (`themes.rs`, 250
   lines), tests that assert nothing (`settled_reads_cost` costs 41 s and checks an exit code), one-off
   migrations whose window has passed.
5. **The time is not where the lines are.** `sim` runs its 215 s hunt self-test on every PR; five hand-written
   HTTP clients omit `Connection: close` and wait out serve's 5 s read timeout per request (about 210 s per
   run); one pipeline test re-folds every prefix of a 1,295-event log (76 s); six release-only budgets also run
   in debug, where they return before their budget (52 s). Two perf budgets (#971, #1588) are enforced by no CI
   job at all.
6. **About 9,400 more lines leave with cuts already decided** besides the remote cut: threads (#1571) about
   6,700, the checks that move to gatekeeper (#1643, #1587) about 2,000, `compact` (#1574) 400, the shed split
   and #1756 about 400. The diet does not touch those files.

What the audit refuted, so nobody cuts there: `m3.rs` and `m3_fixes.rs` hold the only tests of `wait`, the PR
cache and `REFUSED MERGED`; the three reviewer-stall files are not duplicates of `drive.rs`; the pairing tests
have no twin; the silent-lane tests have no twin; the three shape layers over the API cover different routes.

## The numbers

| Step | src | tests | tests / src | gatekeeper's ceiling there |
|---|---|---|---|---|
| Today (`4cf52e4bf`) | 64,744 | 94,937 | 1.47 | 1.50 |
| After the rest of the remote cut (#1691, in flight) | 60,200 | 81,500 | 1.35 | 1.40 |
| After the other decided cuts (#1571, #1643, #1574, shed split parts) | 55,000 | 72,100 | 1.31 | 1.35 |
| After the audit's verdicts (slices 3, 4, 6 to 10) | 55,000 | 59,700 | 1.09 | 1.10 |
| After the owner's retirements (slice 5) | 54,800 | 57,600 | 1.05 | 1.05 or 1.10 |
| After #1665 and the builders inside test bodies | 54,800 | 54,800 or less | 1.00 | 1.00 |

Lines are all lines, as the audit counted them. gatekeeper leaves blank lines out, 5% of either side, so its
ratio is the same to a point. It counts on a grid of 0.05 and never sets a ceiling under 1.00. If drover's
gatekeeper config counts web's TypeScript (1,942 lines, no tests) as src, every ratio is 0.04 lower.

The diet's own 14,500 lines: deletes 4,300 (duplicate, implementation-detail, dead), the retirements 2,100,
tables 4,000, shared fixtures 4,700, less about 600 lines of new shared builders. Every "after" figure is an
estimate from counted lines; nothing was compiled. Slice 6 measures the builders on one area before the others
follow.

**The target is 1.00.** The audit and the retirements reach about 1.05. The last 2,800 lines are named in
decision 9, and nothing the audit found supports a target under 1.00.

## Options

How to cut:

| Option | Verdict |
|---|---|
| A. No audit: let the ratchet press and each lane delete what it must | No. A lane under a failing check deletes the test in its way, not the redundant one. #1650's "touched baselines shrink" has that shape and it is the lever that was not enough. |
| B. Delete only what the audit calls a duplicate | No. That is 2,700 lines of 94,937. |
| C. Audit, then deletes, tables and shared builders by area, each slice behind a keep check | **Yes.** |
| D. Let a coverage tool pick redundant tests | No. Line coverage says a line ran, not that anything asserted on it; it would keep `settled_reads_cost` and drop a table's only failing row. Coverage is used the other way round: as a check that nothing was lost. |

How a deleted test's cover is shown:

| Option | Verdict |
|---|---|
| gatekeeper's coverage gate alone | No. It compares one number, the whole repo's line coverage, with 0.1 point of room: about 55 lines here. A function can lose its only test inside that. It is also on only after adoption. |
| The reviewer alone | No. 4,300 lines of deletes are too many to judge by reading. |
| **A ledger, a line-by-line compare of cover at the base and the head, and for a duplicate the keeper shown failing** | **Yes** (below). gatekeeper's gate stays the standing rule; this is the diet's proof. |

Where slow tests go:

| Option | Verdict |
|---|---|
| A module named `nightly` inside any test file, ignored on the PR path (the first version of this ADR) | No. gatekeeper marks soak tests as whole files, because it selects and lints by file. Two conventions would be two ways to do one thing. |
| **Whole test targets that gatekeeper's `soak` key will name** | **Yes.** drover's `perf.rs` and `soak.rs` already are such files. |

## The ratchet is gatekeeper's

Decided by the owner (q3668, q3678). What drover gets, from gatekeeper#99's ADR, and what it means here:

| gatekeeper's rule | For the diet |
|---|---|
| Test lines over src lines stay under a ceiling: the lowest file of a folder of ceilings, on a 0.05 grid. No one can raise it. | No slice writes a cap, a count or a rule line for one. |
| The first ceiling is the ratio of the adoption day, rounded up. Adoption is gatekeeper#99's own issue for drover; it waits for gatekeeper 1.6.0 and for drover's move to gatekeeper (#1587). | The diet does not wait for it. A slice that lands first lowers the start. Only the close (slice 12) waits. |
| A ratio a whole step under the ceiling fails until the PR adds the next ceiling file, written by one command. | Each slice that crosses a 0.05 line commits that one generated file. The ratchet follows the diet by itself. |
| A new or changed test over 2 s fails its PR. A change to a helper of a test file changes every test of that file. Tests over 2 s at adoption are listed and only warn until changed. | A slice that rewrites a file owns every slow test in it. Slice 2 takes the worst first; slices 6 to 10 leave no test of their files over 2 s. |
| Changed tests run three times. | After adoption an area slice runs its area three times in CI. One more reason slice 2 goes first. |
| Perf and timing tests are whole files, run nightly. | Decision 11. |
| Line coverage of a PR may not be more than 0.1 point under the integration branch's. A deliberate drop is a file with the owner's decision. | Deleting covered src lowers the percent with no cover lost. Slice 5, if it lands after adoption, carries the drop file and names q3678. No other slice should need one. |

## Decisions

1. **drover builds no test cap** and files no issue for one. Adoption of gatekeeper's check is gatekeeper#99's
   issue. That issue's line about closing a cap slice of #1760 has nothing to close.
2. **The diet does not wait for the ratchet; the close does.** Slices 1 to 11 start as the table below says.
   Slice 12 reports the ceiling the diet ended on, so it needs one.
3. **Nothing is deleted before the keep check exists** (slice 1, "How cover is checked" below).
4. **Time first.** Slice 2 moves and repairs the slow tests before any line is cut: it needs no keep check, it is
   what lanes feel, and it is what gatekeeper's 2 s budget would stop the later slices on.
5. **A behaviour is asserted once, at the outermost layer that shows it.** A variant is a table row whose label is
   the old test's name. A screen is asserted as the JSON the server builds; how a client draws it is that
   client's test.
6. **Shared builders replace per-file fixtures** (`shapes.md`): a world (fleet, issue, lane, PR, clock), an HTTP
   client, a git repo, a bulk log, a hook runner. Slice 6 builds them and proves them on the gates area; its
   measured saving corrects the estimates of slices 7 to 10.
7. **One test binary per area**, for the files that set no process environment. Lanes run every test binary on
   their host, so the 29 files that set `DROVER_LANE` and the like keep a binary of their own until their lane
   comes from the test's context. 259 binaries become about 45.
8. **The owner's retirements are one slice (5), with their src** (q3678): the pre-rename move of the `herdsman`
   trees and its aliases; the form route, once the slice has found no client that calls it; the sim owner tools
   `--replay`, `--what-if` and `--metrics`; parity rows 2, 20 and 63, marked `dropped q3678`.
9. **The gap to 1.00 is closed by named cuts, not by pressure.** The page-client tests go to blocks' snapshot
   tool with #1665 (about 1,700); the builders inside test bodies (about 900, measured by slice 6);
   `host_words.rs` as a data file (196). Slice 12 reports what is left.
10. **The diet does not touch a file another decided cut deletes or rewrites.** `inventory.md` marks each.
11. **A slow test lives in a soak file: a whole test target, every test in it ignored by default**, as `soak.rs`
    is today. Three targets: `perf` (the release budgets), `soak` (the full-size runs), `sim_soak` (the hunt).
    A soak test may not exist without a smaller check of the same rule on the PR path. A nightly workflow runs
    them until adoption turns it into gatekeeper's nightly gate.
12. **Three bans keep the fixtures from growing back** (slice 12): a test file outside `common/` and `sim/`
    defines no runner, opens no socket by hand and defines no `kinds`.

## How cover is checked before a test goes

Three checks, each with its enforcer. A slice that removes a test passes all three.

| Check | What it proves | Enforcer |
|---|---|---|
| **The ledger.** The PR body's `## Tests removed` has one line per removed test: the test that keeps its behaviour, the table row that is it, or `none` with `impl` or `dead` and the reason. | every removed test was a decision | the keep job: test names at the base that are gone at the head must each have a line, and a named keeper or row label must exist at the head |
| **No lost cover.** Line coverage of src with the whole suite, at the base and at the head. A src line covered before and not after fails, unless the ledger lists it under `Uncovered on purpose`. | no src line lost its last test | the keep job (cargo-llvm-cov, approved in q3678; only src files the PR leaves unchanged are compared; a loss must repeat in a second head run) |
| **The keeper bites.** For each duplicate, the lane breaks the behaviour once (a one-line change of src, not committed) and shows the keeper fail: a `Kept-by: run` block with the change, the command and the failing output. | the keeper asserts the behaviour, not just runs the line | rev: FAIL without the blocks; rev re-runs three of them |

A slice that only moves tests onto builders or into an area binary removes no test name: its ledger is empty and
the keep job proves that. A table's rows carry the old test names as labels, so the job finds each by text.

The keep job runs only on a PR whose diff removes a `#[test]`. The base's coverage is cached by its sha. It is a
workflow of its own while drover's CI is workflows, and a gate of drover's own once #1587 has moved CI to
gatekeeper; cargo-llvm-cov comes from gatekeeper's pinned tools from 1.6.0 on.

## The impl issues, in order

| # | Issue | Starts after | Mark | Tests net | Test time per run |
|---|---|---|---|---|---|
| 1 | The keep check: the keep job and the ledger rule in the rev brief | approval | | 0 | |
| 2 | Slow tests off the PR path: the soak targets, release-only budgets, the five clients, the sim PR set, the pipeline subset | approval | `--perf` | 0 | 2,011 s to about 1,320 s |
| 3 | Deletes, flow half: drive, gates, lanes, hook and spawn | 1, and #1691's slices 2 to 9 | | −1,600 | |
| 4 | Deletes, board half: host, ops, asks, serve, screens, sim and harness | 3 | | −2,600 | −125 s |
| 5 | The retirements: the pre-rename move, the form route, the sim owner tools, parity rows 2, 20, 63 | 4 | | −2,200 | −115 s |
| 6 | The builders, and the gates area on them with its tables | 5 | | −500 | |
| 7 | The drive area | 6 | | −1,300 | |
| 8 | Lanes, hook and spawn | 7 | | −2,200 | |
| 9 | Host and ops | 8 | | −1,500 | |
| 10 | Asks, serve, screens, sim and perf | 9 | | −2,700 | |
| 11 | One test binary per area | 10 | `--perf` | −200 | build time and folder size, measured |
| 12 | The close: the three bans, the report of what is left | 11, and gatekeeper's check adopted | | 0 | |

Slices 3 to 12 are blocked each on the one before (`queue block --on`), because they rewrite the same files.
Slices 1 and 2 run beside everything. Slice 3 waits for the remote cut so no file is edited by two queues.
Per-slice figures are the audit's estimates; each PR states its measured `tests net` and the ratio at its head.

## Consequences

- **Nothing holds the gain until gatekeeper's check is on.** Between a slice and adoption the suite could grow
  back. The slices are serial and each PR states its ratio, so a rise is seen; the owner chose this over a cap
  drover would build and then delete.
- **After adoption a PR must keep the repo under the ceiling of the day**, 1.0 at the end, with at most 5% of
  src of room under it. A bug fix that adds only a test uses the room, or merges a test into a table. A PR that
  deletes src without its tests can go over.
- **The diet's slices are the ratchet's turns.** A slice that crosses a grid line commits one generated ceiling
  file; that is the only cap paperwork it has.
- **A slice that rewrites a test file answers for every slow test in it** once the 2 s budget is on. 143 tests
  are over it today. Slice 2 lists what is left by area; each area slice ends with none in its files.
- **Lanes build about 45 test binaries, not 259**, and the shared harness compiles about 45 times, not 245.
  Changing one test file recompiles its area, a few thousand lines.
- **A test's name in gatekeeper's lists carries its binary.** Slice 11 moves tests between binaries, so it lands
  with no test of a moved file on the slow list, and a test with an open flaky entry stays in its own binary
  until that entry is closed.
- **Table rows fail by label.** A failing row prints the old test's name; the runner shows one failing test per
  table, not one per row.
- **The sim's nightly metrics table goes with its tool** (slice 5): the `sim-metrics` artifact and the job
  summary's table are no longer written. The nightly run keeps its 2,000 seeds and its hunt.
- **Goldens and other non-Rust test files are not counted**, by gatekeeper as by the audit (13,100 lines).
  Moving assertions into a golden lowers the count without lowering the cost; rev treats a new golden as test
  lines when judging a diet slice.
- **#1650** already makes a touched size baseline shrink: slices 3, 7 and 8 lower the entries of `drive.rs`,
  `hook.rs`, `shed.rs` and `hosts.rs` as they go.
- **Found on the way, for their own issues:** the two perf budgets no CI job runs (#971, #1588: slice 2 puts
  them in a soak target); `docs/parity.md` has seven rows still `open` on merged issues; the remote cut's
  inventory does not name `flow_facts.rs` (all three tests judge another host) and would drop the only test
  that a `fix` orphan is adopted as impl (`orphans.rs`, its rule T2 keeps it with a local lane); gatekeeper's
  coverage gate reads a delete of covered src as a drop (the remote cut and #1571 will meet it before the diet
  does).

## Open questions

None. q3678 answered the four of the first version: no cap in drover; cargo-llvm-cov yes; the three retirements
yes, the form route once the slice has confirmed no client calls it; parity rows 2, 20 and 63 dropped.

## The `.drover/arch.toml` lines

This ADR turns no hot file into a folder, freezes nothing and generates nothing. The folder of ceilings is
gatekeeper's, and its `[[collect]]` lines come with the adoption PR, as gatekeeper#99's ADR lists them.

Slice 3 deletes `term_adapter.rs`, whose two tests grep src, and adds the bans that say the same (the test
ignores case for the last one; if a ban does not, the slice adds the capitalised form as a fourth):

```toml
[[ban]]
pattern = "[\"pane\", \""
paths = ["crates/*/src/**"]
except = ["crates/core/src/term.rs"]
reason = "#1760: only term.rs builds a pane's argv (was tests/term_adapter.rs)"

[[ban]]
pattern = "[\"agent\", \""
paths = ["crates/*/src/**"]
except = ["crates/core/src/term.rs"]
reason = "#1760: only term.rs builds an agent's argv (was tests/term_adapter.rs)"

[[ban]]
pattern = "herdr"
paths = ["crates/*/src/**"]
except = ["crates/core/src/config.rs"]
reason = "#1760: the retired runtime is named only by its load error (was tests/term_adapter.rs)"
```

Slice 12 adds the three bans that keep the fixtures shared, once no file is left that breaks them. A file that
still needs a runner or a raw socket of its own by then is named in `except`, with the reason in the PR body:

```toml
[[ban]]
pattern = "impl Runner for"
paths = ["crates/*/tests/**"]
except = ["crates/*/tests/common/**", "crates/*/tests/sim/**"]
reason = "#1760: a test scripts the world through common's builders; a second fake runner is a second fixture"

[[ban]]
pattern = "TcpStream::connect"
paths = ["crates/*/tests/**"]
except = ["crates/*/tests/common/**"]
reason = "#1760: a test talks to serve through common's client, which closes its connection"

[[ban]]
pattern = "fn kinds("
paths = ["crates/*/tests/**"]
except = ["crates/*/tests/common/**"]
reason = "#1760: the log is read through common; this helper was copied into 54 files"
```
