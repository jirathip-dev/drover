# test diet: the twelve issues, as they are to be filed (#1760)

The orchestrator files them on approval, in this order. Each names this folder as its ADR. `N` stands for the
number an issue gets. Estimates are the audit's; every PR states what it measured.

None of them builds a test cap. The ratchet is gatekeeper's test-health check; drover adopts it through
gatekeeper#99's own adoption issue for drover ("A1" below), which this list does not file.

Shared lines for the body of every one of them:

- The ADR: `docs/architecture/test-diet/` (decision, shapes, boundaries; `inventory.md` is the verdict per file).
- A slice that removes a test carries the ledger (`shapes.md`, section 2) and passes the keep job.
- The PR body states `tests net`, `src net`, the measured ratio of test lines to src lines and, from slice 2 on,
  the summed test time.
- Once A1 has merged: when gatekeeper's check says the ratio is a step under the ceiling, the PR commits the one
  ceiling file its `baseline` command writes. No slice edits or removes a ceiling file.
- An inventory verdict that proves wrong in the lane's hands is not applied: the test stays and the PR body says why.

## 1. The keep check: no test is deleted without its cover shown

Queue: plain. Starts on approval. cargo-llvm-cov is approved (q3678).

**Wanted.**
- A job of its own that runs only when a PR's diff removes a `#[test]`: a workflow, `keep.yml`, while drover's CI
  is workflows; a gate of drover's own if #1587 has moved CI to gatekeeper by then.
- It lists the test function names at the base that are gone at the head and fails for each one with no line in
  the PR body's `## Tests removed`, and for each named keeper or row label it cannot find at the head.
- It runs the suite with line coverage at the base (cached by sha) and at the head, and fails for each src line
  of an unchanged src file that was covered and no longer is, unless the body lists it under `Uncovered on
  purpose`. A loss must repeat in a second head run.
- cargo-llvm-cov pinned like cargo-nextest; from gatekeeper 1.6.0 on, the version its rust pack pins.
- `roles/rev.md` and the rev brief: a PR that removes a test FAILs without its ledger, and without a `Kept-by:
  run` block per keeper of a duplicate; rev re-runs three of them.

**AC.**
- A test PR that deletes a test with no ledger line fails; with the line it passes.
- A test PR that deletes the only test of a function fails with `lost cover:` and the file and lines.
- A PR that removes no test does not run the job.

## 2. Slow tests off the PR path

Queue: `--perf` (baseline: the summed nextest time of the four shards, 2,011 s on `4cf52e4bf`). Starts on approval.

**Wanted.** No test is deleted here. By the inventory's `soak` and time notes:
- Three soak targets, every test in them ignored by default (`shapes.md`, section 1): `perf` (today's file,
  less its three tests that are not budgets), `soak` (today's file, plus the full-size runs below), `sim_soak`
  (the hunt self-test; it shares the sim's modules). A workflow of its own, `soak.yml`, runs the three nightly
  in release; A1 later turns it into gatekeeper's nightly gate. The debug shards leave the three out.
- `sim`: six tests stay per PR (the fixed seeds with the incident assertion folded in, the gate scenarios run
  once, the three invariant self-tests, the replay-to-same-trace test); the hunt goes to `sim_soak`. The
  self-tests of the three owner tools stay where they are: slice 5 deletes them. 496 s to about 245 s.
- `pipeline.rs`: the replay runs over about 150 events that hold every state label and gate code per PR, the
  whole log in `soak`. 76 s to about 2 s.
- The five hand-written HTTP clients that omit `Connection: close` (`serve_workers.rs`, `cooee_token.rs`,
  `live.rs`, and the board reads of `shed.rs` and `lane_states/mod.rs`) send it, or read in process. About
  210 s, of which 66 s leaves with the remote cut anyway.
- Budgets that only mean something in release are ignored in debug (six in `perf.rs`, `screen_hub.rs`,
  `perf/screens.rs`); the release job runs the perf binary whole, with no list of names, until A1 moves it to
  the nightly gate. The two budgets no job runs today (#971, #1588) go to `soak`.
- Smaller PR-path sizes where the bound is a ratio: `ask_scale.rs` at 2,000 events, two compaction seeds, one
  kill-9 round, `serve_health.rs`'s slow read at 20 ms against a low limit. The full sizes go to `soak`.

**AC.** The rev's `Perf:` block: the same command before and after, about 1,320 s or less; the soak workflow
ran green once; no test name was removed (the keep job's count is 0). The PR body lists every test still over
2 s at its head, by inventory area: slices 6 to 10 each empty their part.

## 3. Deletes, flow half: drive, gates, lanes, hook and spawn

Queue: plain; blocked on 1 and on #1691's slices 2 to 9. About −1,600 lines.

**Wanted.** Every `delete` of the inventory's first four tables (`dup`, `impl`, `dead`), with the ledger. The
bans that replace `term_adapter.rs` (the exact lines are at the end of `adr.md`). `.drover/arch-baseline.txt`
entries of touched files lowered (#1650).

**AC.** The keep job is green; the ledger covers every removed test; each keeper has its `Kept-by: run` block.

## 4. Deletes, board half: host, ops, asks, serve, screens, sim and harness

Queue: plain; blocked on 3. About −2,600 lines, about −125 s.

**Wanted.** As slice 3, for the other tables: `drive_sim.rs` (one assertion moves to `disk.rs`), the tests that
assert nothing, the pack-content tests of `themes.rs` behind one loop, the dead helpers of `common/`. Not the
retirements and not the three parity tests: slice 5.

**AC.** As slice 3.

## 5. The retirements: what the owner dropped in q3678

Queue: plain; blocked on 4. About −2,200 test lines, about −115 s, src net negative.

**Wanted.** Each with its tests and its src in this PR; each removed test's ledger line reads `none (dead:
retired, q3678)`.
- **The pre-rename move.** The `migrate` verb, the `herdsman` aliases of the binary name and of the config,
  state and share trees, and the `.herdsman` twin of a repo's `.drover` folder. `migrate.rs` keeps its four
  tests that are not about the rename. `drover help --agent` and the usage doc's parts lose the verb (`just
  gen` for the goldens).
- **The form route.** First the check, written into the PR body: no client posts a form to `/`. Searched:
  `web/src`, the built board, the lifeboat page, the terminal board, and cooee's client in its repo. If one
  does, the route stays, this part is left out and the lane files an owner ask with the caller. If none does:
  the two `POST /` arms of the server and its paired-op guard go. The handler behind them stays, because the
  screens' actions call it. A form test whose operation an action test already asserts is deleted as `dup`
  with its keeper; one with no such twin is moved onto the action route, not deleted; an operation no screen
  action reaches goes with its tests.
- **The sim owner tools.** `sim/replay.rs`, `sim/whatif.rs`, `sim/metrics.rs`, their entry points and
  self-tests in `sim/main.rs`, the `--replay`, `--what-if` and `--metrics` arms of `just sim`, and the metrics
  step of the nightly sim workflow with its `sim-metrics` artifact. The fixed seeds, the gate scenarios, the
  invariants and the hunt stay.
- **Parity rows 2, 20 and 63.** `parity_2` and `parity_20` (`parity_webkit.rs`) and `parity_63`
  (`cooee_screens.rs`) are deleted; their rows in `docs/parity.md` read `dropped q3678`. The ledger names the
  test that holds each behaviour.

**AC.** The keep job is green. The PR body carries the form-route check with what was searched. `just sim` and
the nightly sim workflow run green without the three tools. If A1 has merged and the repo's line coverage
falls by more than gatekeeper's tolerance because covered src was deleted, the PR carries gatekeeper's
coverage drop file, its commit naming q3678.

## 6. The builders, and the gates area on them

Queue: plain; blocked on 5. About −500 lines net of about +600 of builders.

**Wanted.** `World`, `Client`, `Repo`, `Log` and `Hook` in `common` (`shapes.md`, section 4). The gates area
(the inventory's second table) rewritten on them, with its tables. No test name removed.

**AC.** The keep job counts 0 removed tests. The PR body gives, for the gates area, lines before and after split
into helpers and test bodies: the measured saving per test, which slices 7 to 10 use for their estimates. No
test of a rewritten file is over 2 s in CI.

## 7 to 10. The other areas on the builders

Queue: plain; each blocked on the one before.

| # | Areas (inventory tables) | About |
|---|---|---|
| 7 | drive loop | −1,300 |
| 8 | shed sessions and lane lifecycle; hook, spawn, lane verbs, jobs (the hook table) | −2,200 |
| 9 | check, clean, disk, host (the clean table); setup, state, config | −1,500 |
| 10 | asks, cards, queue; serve, API, streams; screens; simulation and perf (the one log builder) | −2,700 |

**Wanted, each.** The inventory's `table` and `merge` verdicts for its areas. Files that merge share one
fixture. No behaviour removed; a table row carries the old test's name.

**AC, each.** The keep job is green; test names gone are rows or in the ledger; the area's files define no
runner, HTTP client, repo builder or log reader of their own. No test of a rewritten file is over 2 s in CI:
it was made faster, or its full size went to `soak` with a small check left on the PR path.

## 11. One test binary per area

Queue: `--perf` (baseline: a full `cargo test --no-run` from clean, its wall time and the size of the test
executables in the build folder: 265 files, 16.2 GB on a lane today). Blocked on 10.

**Wanted.** Each area's files move under a folder of the area with a `main.rs` that lists them as modules
(`shapes.md`, section 6). A file that sets process environment, or needs a process of its own, keeps its own
binary; the PR body lists those and why. `scripts/test-shards.sh` already reads `main.rs` folders; the shard list
is rebalanced. CI steps that name a moved test binary are updated in the same PR. If A1 has merged: a test
named in a flaky entry stays in its own binary, and gatekeeper's slow list is rewritten by its own command.

**AC.** The rev's `Perf:` block: build time and folder size before and after. The keep job counts 0 removed
tests. The plain test run and the four nextest shards are green.

## 12. The close

Queue: plain; blocked on 11 and on A1 (gatekeeper#99's adoption issue for drover).

**Wanted.** The three bans of `adr.md` in `.drover/arch.toml`, with no file left that breaks them. `README.md`'s
test section, if it has one, says where a new test goes: its area's file, a row if the verb has a table.

**AC.** The PR body reports tests, src, the ratio, gatekeeper's ceiling, the summed test time, the binaries
built, the tests still on gatekeeper's slow list, and each cut of `adr.md`'s decision 9 as done or open with
its issue number. If the ceiling rests over 1.00, that list is the owner's next decision.
