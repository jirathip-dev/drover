# ADR: test diet: the audit of the suite, the cut, and a test cap that only falls (#1760)

## Summary

Decision: first make "it may only fall" a rule, then show a test's cover before it goes, then cut. The audit read all 288 test files: the excess is copied fixtures and tests of features already on their way out, far more than duplicate tests. With the cuts already decided, its verdicts take tests from 147% of src to about 108%; the 100% cap needs the further cuts the ADR names, three of them the owner's call (question 3). The cap itself is one more line of the cap check: test lines at most a percent of src lines, the percent kept as files in a folder where the lowest wins and no file may be changed or removed.
Slices: 1 the test cap; 2 the keep check; 3 slow tests off the PR path (2,011 s of test time per run to about 1,200 s, about 900 s once the cuts land); 4 and 5 the deletes; 6 shared builders, proven on one area; 7 to 10 the other areas; 11 one test binary per area (16 GB of test executables per lane to about 3); 12 the close.
Open questions: (1) the cap check goes into drover's cap check now and moves to gatekeeper with it, rather than waiting for gatekeeper: yes? (2) cargo-llvm-cov as a pinned CI tool, in one job: yes? (3) retire three things: the pre-rename move, the form route no client in this repo calls, the three sim owner tools in the test tree? (4) approving this drops parity rows 2, 20 and 63, each held by another test.

## Status

Proposed by the architect lane on staging `4cf52e4bf`, 2026-11-25. Read `README.md` first, then this. `inventory.md`
is the verdict per test file, `usage.md` how the callers read, `shapes.md` the data shapes, `boundaries.md` who may
call what and the statecharts, `issues.md` the twelve issues as they are to be filed.

## Question

drover has 94,937 lines of Rust tests against 64,744 lines of src (147%). Every lane builds and runs them. Which
tests can go with every behaviour still covered, how is that shown before each delete, what ratio is reachable,
and what keeps the suite from growing back?

## What the audit found

Eleven read-only passes read every test body of every file, by area, and read both sides of every claimed
duplicate. `inventory.md` holds the result per file. Measured on staging `4cf52e4bf` and its CI run:

| Fact | Number |
|---|---|
| Test lines / src lines | 94,937 / 64,744 |
| Tests / test binaries | 2,064 / 259 (one binary per top-level file) |
| Lines outside any test body (helpers, fixtures) | 31,772 (34%) |
| The same helper written again | `kinds` in 54 files (36 byte-identical), `setup` in 63, `ev` in 39, `fake` in 35 |
| The same scripted replies | the host sample in 67 files, a PR's JSON 68 times, a real git origin and clone 9 times, a raw HTTP client in about 40 files |
| Summed test time of one CI run | 2,011 s; ten binaries take 60% of it |
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

| Step | src | tests | tests / src |
|---|---|---|---|
| Today (`4cf52e4bf`) | 64,744 | 94,937 | 147% |
| After the rest of the remote cut (#1691, in flight) | 60,200 | 81,500 | 135% |
| After the other decided cuts (#1571, #1643, #1574, shed split parts) | 55,000 | 72,100 | 131% |
| After this diet's audited verdicts | 55,000 | 59,700 | 108% |
| After the further cuts of decision 9 (question 3, #1665) | 54,000 | 54,000 or less | 100% |

The diet's own 12,400 lines: deletes 4,300 (duplicate, implementation-detail, dead), tables 4,000, shared
fixtures 4,700, less about 600 lines of new shared builders. Every "after" figure is an estimate from counted
lines; nothing was compiled. Slice 6 measures the builders on one area before the others follow.

**The target is 1.0×, and the audit alone does not reach it.** Nothing it found supports a lower target. The
gap of about 4,700 lines is listed, not hidden, in decision 9.

## Options

How to cut:

| Option | Verdict |
|---|---|
| A. No audit: set the cap at 1× and let each lane delete what it must | No. A lane under a failing cap deletes the test in its way, not the redundant one. #1650's "touched baselines shrink" has that shape and it is the lever that was not enough. |
| B. Delete only what the audit calls a duplicate | No. That is 2,700 lines of 94,937. |
| C. Audit, then deletes, tables and shared builders by area, each slice behind a keep check | **Yes.** |
| D. Let a coverage tool pick redundant tests | No. Line coverage says a line ran, not that anything asserted on it; it would keep `settled_reads_cost` and drop a table's only failing row. Coverage is used the other way round: as a check that nothing was lost. |

The shape of the cap:

| Option | Verdict |
|---|---|
| A fixed number of test lines | No. Deleting src would leave the test budget untouched, and a feature could not bring its tests. |
| **A percent of src lines** | **Yes.** A feature brings test lines in proportion to its code; deleting code takes test budget with it. |
| The percent as one line of `ci/cap.toml` | No. That file is frozen and lanes may not edit it; one number every diet slice edits is a hot file. |
| **The percent as files in a folder, the lowest wins, files append-only** | **Yes.** Adding a file can only lower it. `.drover/arch.toml`'s `append_only` already fails a PR that edits or removes one: "may only fall" needs no new rule code. |

Where the check lives:

| Option | Verdict |
|---|---|
| **In the cap check, wherever it lives that day: drover's `cap-check` now, gatekeeper's `core/cap` after #1643's G2 ports it with drover's cases** | **Yes** (question 1). Same step, same output, same merge-time check as the src cap. About 45 src lines, the only product code the diet adds. |
| Wait for gatekeeper | No. The diet's first slices would land with nothing holding the line. |
| A shell script as its own gate | No. A second place that counts src by its own globs. |

## Decisions

1. **The cap is a percent, `test_pct`, of src lines.** Test lines are every Rust line under a `tests` folder of
   the repo or of a crate; src lines are every Rust line under a `src` folder, capped or not. One ratio for the
   repo: a crate that leaves the line cap (#1755, #1759) stays in both counts with its tests.
2. **The ratchet is a folder.** Each step is one small file with `test_pct` and `why`; the lowest wins; the folder
   is `append_only`. It starts at 148 (today's 146.6, rounded up, plus one). There is no raise and no ask for
   one: over the cap, a lane deletes or merges tests.
3. **Each diet slice lowers it** to the percent measured at its head, rounded up, plus one. The one point is
   room for a PR that deletes src without tests; without it such a PR would fail.
4. **Nothing is deleted before the keep check exists** (slice 2, "How cover is checked" below).
5. **Time first.** Slice 3 moves and repairs the slow tests before any line is cut: it needs no keep check, and
   it is what lanes feel.
6. **A behaviour is asserted once, at the outermost layer that shows it.** A variant is a table row whose label is
   the old test's name. A screen is asserted as the JSON the server builds; how a client draws it is that
   client's test.
7. **Shared builders replace per-file fixtures** (`shapes.md`): a world (fleet, issue, lane, PR, clock), an HTTP
   client, a git repo, a bulk log, a hook runner. Slice 6 builds them and proves them on the gates area; its
   measured saving corrects the estimates of slices 7 to 10.
8. **One test binary per area**, for the files that set no process environment. Lanes run plain `cargo test`
   (nextest is not on the host), so the 29 files that set `DROVER_LANE` and the like keep a binary of their own
   until their lane comes from the test's context. 259 binaries become about 45.
9. **The gap to 100% is closed by named cuts, not by pressure.** In order of size: the page-client tests go to
   blocks' snapshot tool with #1665 (about 1,700); the sim owner tools leave the test tree (1,320, question 3);
   the form route (500 to 1,100, question 3); the builders inside test bodies (measured by slice 6); the
   pre-rename move (287, question 3); `host_words.rs` as a data file (196). Slice 12 reports what is left.
10. **The diet does not touch a file another decided cut deletes or rewrites.** `inventory.md` marks each.
11. **Slow tests live in a module named `nightly`**, ignored on the PR path and run by name pattern, so no
    workflow lists test names. A budget that only means something in a release build is ignored in debug.
12. **Three bans keep the fixtures from growing back** (slice 12): a test file outside `common/` and `sim/`
    defines no runner, opens no socket by hand and defines no `kinds`.

## How cover is checked before a test goes

Three checks, each with its enforcer. A slice that removes a test passes all three.

| Check | What it proves | Enforcer |
|---|---|---|
| **The ledger.** The PR body's `## Tests removed` has one line per removed test: the test that keeps its behaviour, the table row that is it, or `none` with `impl` or `dead` and the reason. | every removed test was a decision | the keep job: test names at the base that are gone at the head must each have a line, and a named keeper or row label must exist at the head |
| **No lost cover.** Line coverage of src with the whole suite, at the base and at the head. A src line covered before and not after fails, unless the ledger lists it under `Uncovered on purpose`. | no src line lost its last test | the keep job (cargo-llvm-cov; only src files the PR leaves unchanged are compared; a loss must repeat in a second head run) |
| **The keeper bites.** For each duplicate, the lane breaks the behaviour once (a one-line change of src, not committed) and shows the keeper fail: a `Kept-by: run` block with the change, the command and the failing output. | the keeper asserts the behaviour, not just runs the line | rev: FAIL without the blocks; rev re-runs three of them |

A slice that only moves tests onto builders or into an area binary removes no test name: its ledger is empty and
the keep job proves that. A table's rows carry the old test names as labels, so the job finds each by text.

The keep job runs only on a PR whose diff removes a `#[test]`. The base's coverage is cached by its sha.

## The impl issues, in order

| # | Issue | Starts after | Mark | Tests net | Test time per run |
|---|---|---|---|---|---|
| 1 | The test cap: the rule in the cap check, the folder at 148, the rules line, the impl role's lines | approval | | +40 | |
| 2 | The keep check: the keep job and the ledger rule in the rev brief | approval | | 0 | |
| 3 | Slow tests off the PR path: the `nightly` module, release-only budgets, the five clients, the sim PR set, the pipeline subset | approval | `--perf` | 0 | 2,011 s to about 1,200 s |
| 4 | Deletes, flow half: drive, gates, lanes, hook and spawn | 2, and #1691's slices 2 to 9 | | −1,600 | |
| 5 | Deletes, board half: host, ops, asks, serve, screens, sim and harness | 4 | | −2,700 | −125 s |
| 6 | The builders, and the gates area on them with its tables | 5 | | −500 | |
| 7 | The drive area | 6 | | −1,300 | |
| 8 | Lanes, hook and spawn | 7 | | −2,200 | |
| 9 | Host and ops | 8 | | −1,500 | |
| 10 | Asks, serve, screens, sim and perf | 9 | | −2,700 | |
| 11 | One test binary per area | 10 | `--perf` | −200 | build time and folder size, measured |
| 12 | The close: the three bans, the ratchet at its end value, the report of what is left | 11 | | 0 | |

Slices 4 to 12 are blocked each on the one before (`queue block --on`), because they rewrite the same files.
Slices 1 to 3 run beside everything. Slice 4 waits for the remote cut so no file is edited by two queues.
Per-slice figures are the audit's estimates; each PR states its measured `tests net` and the new percent.

## Consequences

- **A PR can fail the test cap by deleting src.** Fewer src lines mean a smaller test budget. The one point of
  room (decision 3) covers a small one; a large delete of code takes its tests with it, which is the rule's
  point. At the end state the room is whatever lies between the suite and 100%.
- **During the diet a feature PR may bring at most about 1.3 test lines per src line**, falling to 1.0. A bug fix
  that adds only a test uses the room, or merges a test into a table.
- **The ratchet cannot be raised, even by mistake.** A file with too low a number fails its own PR's cap check,
  so it cannot merge. Removing a file needs two PRs (the glob out of `append_only`, then the file): deliberate.
- **Lanes build about 45 test binaries, not 259**, and the shared harness compiles about 45 times, not 245.
  Changing one test file recompiles its area, a few thousand lines.
- **Table rows fail by label.** A failing row prints the old test's name; nextest shows one failing test per
  table, not one per row.
- **Goldens and other non-Rust test files are not counted** (13,100 lines). Moving assertions into a golden
  lowers the count without lowering the cost; rev treats a new golden as test lines when judging a diet slice.
- **Inline `#[cfg(test)]` modules in src count as src** (789 lines today), as the line cap already counts them.
- **`.drover/rules.md` stays at 30 lines**: the cap line is widened, not added (`issues.md`, slice 1).
- **#1643** ports the test rule with the cap check: one line more in its G2 acceptance. If G2 merges before
  slice 1 starts, slice 1 is filed on gatekeeper and drover bumps its pin.
- **#1650** already makes a touched size baseline shrink: slices 4, 7 and 8 lower the entries of `drive.rs`,
  `hook.rs`, `shed.rs` and `hosts.rs` as they go.
- **Found on the way, for their own issues:** the two perf budgets no CI job runs (#971, #1588: slice 3 puts
  them in `nightly`); `docs/parity.md` has seven rows still `open` on merged issues; the remote cut's inventory
  does not name `flow_facts.rs` (all three tests judge another host) and would drop the only test that a `fix`
  orphan is adopted as impl (`orphans.rs`, its rule T2 keeps it with a local lane).

## Open questions

1. **Where the test rule lives until gatekeeper owns the cap check.** I chose drover's cap check now, about 45
   src lines, ported with it by #1643. Say if it should wait for gatekeeper instead: then slices 3 to 12 run
   with no ratchet until it lands.
2. **cargo-llvm-cov** as a pinned CI tool, installed like cargo-nextest, used by the keep job only. It adds no
   crate. Without it the keep check is the ledger and the reviewer.
3. **Three retirements only the owner can make.** Each frees test lines and src:
   - the pre-rename move of the `herdsman` trees and its aliases (287 test lines; the rename merged 2026-11-12
     and this host's old trees have been links since 2026-11-13);
   - the form route `POST /`, which the page no longer calls (`web/src` posts to `/action`; cooee not checked):
     500 to 1,100 test lines;
   - the sim owner tools `--replay`, `--what-if` and `--metrics`, which live in the test tree as ignored tests
     (1,320 lines with their self-tests): keep them counted, move them to a tool crate, or drop them.
4. **Parity rows.** `parity_2` (ignored since #1084 merged), `parity_20` and `parity_63` are each held whole by
   another test. Deleting one needs `dropped q<N>` in `docs/parity.md`: the approval of this ADR is that
   decision, unless you say otherwise.

## The `.drover/arch.toml` lines

Slice 1 widens one key and adds one folder:

```toml
append_only = ["crates/cli/tests/golden/contract/v1/*.keys", "ci/test-cap.d/*.toml"]

[[collect]]
dir = "ci/test-cap.d"
glob = "*.toml"
```

It also makes the append-only finding's text general: today it ends "a breaking change is /api/v2", which is
only true of the contract.

Slice 12 adds the three bans, once no file is left that breaks them. A file that still needs a runner or a raw
socket of its own by then is named in `except`, with the reason in the PR body:

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

No file is frozen and nothing is generated by this ADR. The folder of ratchet files is the only list it adds,
and it is collected from the start.
