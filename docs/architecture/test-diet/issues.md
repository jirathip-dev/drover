# test diet: the twelve issues, as they are to be filed (#1760)

The orchestrator files them on approval, in this order. Each names this folder as its ADR. `N` stands for the
number an issue gets. Estimates are the audit's; every PR states what it measured.

Shared lines for the body of every one of them:

- The ADR: `docs/architecture/test-diet/` (decision, shapes, boundaries; `inventory.md` is the verdict per file).
- A slice that removes a test carries the ledger (`shapes.md`, section 4) and passes the keep job.
- The PR body states `tests net`, `src net`, the measured percent and, from slice 3 on, the summed test time.
- An inventory verdict that proves wrong in the lane's hands is not applied: the test stays and the PR body says why.

## 1. The test cap: test lines at most a percent of src, in a folder that only falls

Queue: plain. Starts on approval.

**Problem.** Tests are 147% of src and nothing stops them growing. #1650 only shrinks a baseline that is touched.

**Wanted.**
- The cap check counts every Rust line under a `tests` folder (repo root and each crate) against every Rust line
  under a `src` folder, and fails when tests are over `test_pct` percent of src.
- `test_pct` is the lowest value among the files of a new folder, `test-cap.d`, beside `cap.d`. One file per step,
  with `test_pct` and `why`. No file: no test cap. A file that does not parse: the check fails.
- The first step, 148, with the day's measured figures in its `why`.
- `.drover/arch.toml`: the folder's files are `append_only` and the folder is collected (the exact lines are at
  the end of `adr.md`). The append-only finding's text no longer speaks of the API.
- `.drover/rules.md`: the line-cap line becomes "don't push src/ over ci/cap.toml's line_cap or tests over the
  lowest test_pct of ci/test-cap.d, edit ci/cap.toml from a lane, or change or remove a test-cap step (enforced
  by: CI line cap, the lane hook, CI arch-check `append_only`)". Still 30 lines.
- `roles/impl.md`, beside the line-cap rule: over the test cap a lane merges a case into a table or deletes a
  redundant test with its ledger line; there is no raise and no ask for one. `just gen` for the brief goldens.

**AC.**
- A test: a tree whose tests are over the percent fails the cap check with the `test lines:` line, and passes
  one line under it.
- A test: with two steps in the folder the lower one is in force, whatever their names.
- A test: a PR that changes or removes a step fails arch-check; one that adds a step passes.
- The merge check on a PR merged with the integration branch fails the same way (`STAGING_WOULD_BREAK`).
- `src/ net` about +45 (the only product code of the diet). If #1643's G2 has merged, this issue is filed on
  gatekeeper instead and drover bumps its pin.

## 2. The keep check: no test is deleted without its cover shown

Queue: plain. Starts on approval. Needs the owner's yes to cargo-llvm-cov (question 2).

**Wanted.**
- A workflow of its own, `keep.yml`, that runs only when a PR's diff removes a `#[test]`.
- It lists the test function names at the base that are gone at the head and fails for each one with no line in
  the PR body's `## Tests removed`, and for each named keeper or row label it cannot find at the head.
- It runs the suite with line coverage at the base (cached by sha) and at the head, and fails for each src line
  of an unchanged src file that was covered and no longer is, unless the body lists it under `Uncovered on
  purpose`. A loss must repeat in a second head run.
- `roles/rev.md` and the rev brief: a PR that removes a test FAILs without its ledger, and without a `Kept-by:
  run` block per keeper of a duplicate; rev re-runs three of them.

**AC.**
- A test PR that deletes a test with no ledger line fails; with the line it passes.
- A test PR that deletes the only test of a function fails with `lost cover:` and the file and lines.
- A PR that removes no test does not run the job.

## 3. Slow tests off the PR path

Queue: `--perf` (baseline: the summed nextest time of the four shards, 2,011 s on `4cf52e4bf`). Starts on approval.

**Wanted.** No test is deleted here. By the inventory's `nightly` and time notes:
- A `nightly` module convention and a workflow of its own that runs those modules in release by name pattern.
- `sim`: six tests stay per PR (the fixed seeds with the incident assertion folded in, the gate scenarios run
  once, the three invariant self-tests, the replay-to-same-trace test); the hunt, what-if, replay and metrics
  self-tests go to the sim workflow. 496 s to about 128 s.
- `pipeline.rs`: the replay runs over about 150 events that hold every state label and gate code per PR, the
  whole log nightly. 76 s to about 2 s.
- The five hand-written HTTP clients that omit `Connection: close` (`serve_workers.rs`, `cooee_token.rs`,
  `live.rs`, and the board reads of `shed.rs` and `lane_states/mod.rs`) send it, or read in process. About
  210 s, of which 66 s leaves with the remote cut anyway.
- Budgets that only mean something in release are ignored in debug (six in `perf.rs`, `screen_hub.rs`,
  `perf/screens.rs`); the release job runs the perf binary whole, with no list of names. The two budgets no
  job runs today (#971, #1588) go to `nightly`.
- Smaller PR-path sizes where the bound is a ratio: `ask_scale.rs` at 2,000 events, two compaction seeds, one
  kill-9 round, `serve_health.rs`'s slow read at 20 ms against a low limit.

**AC.** The rev's `Perf:` block: the same command before and after, about 1,200 s or less; the nightly workflow
ran green once; no test name was removed (the keep job's count is 0).

## 4. Deletes, flow half: drive, gates, lanes, hook and spawn

Queue: plain; blocked on 2 and on #1691's slices 2 to 9. About −1,600 lines.

**Wanted.** Every `delete` of the inventory's first four tables (`dup`, `impl`, `dead`), with the ledger. The two
arch bans that replace `term_adapter.rs`. `.drover/arch-baseline.txt` entries of touched files lowered (#1650).
A new step of the ratchet: the measured percent, rounded up, plus one.

**AC.** The keep job is green; the ledger covers every removed test; each keeper has its `Kept-by: run` block.

## 5. Deletes, board half: host, ops, asks, serve, screens, sim and harness

Queue: plain; blocked on 4. About −2,700 lines, about −125 s.

**Wanted.** As slice 4, for the other tables: `drive_sim.rs` (one assertion moves to `disk.rs`), the tests that
assert nothing, the pack-content tests of `themes.rs` behind one loop, the dead helpers of `common/`, the three
parity tests of question 4 with their `dropped q<N>` rows. The pre-rename move and the form route only if the
owner said yes to each (question 3); their src goes in the same PR. A new step of the ratchet.

**AC.** As slice 4.

## 6. The builders, and the gates area on them

Queue: plain; blocked on 5. About −500 lines net of about +600 of builders.

**Wanted.** `World`, `Client`, `Repo`, `Log` and `Hook` in `common` (`shapes.md`, section 6). The gates area
(the inventory's second table) rewritten on them, with its tables. No test name removed.

**AC.** The keep job counts 0 removed tests. The PR body gives, for the gates area, lines before and after split
into helpers and test bodies: the measured saving per test, which slices 7 to 10 use for their estimates. A new
step of the ratchet.

## 7 to 10. The other areas on the builders

Queue: plain; each blocked on the one before.

| # | Areas (inventory tables) | About |
|---|---|---|
| 7 | drive loop | −1,300 |
| 8 | shed sessions and lane lifecycle; hook, spawn, lane verbs, jobs (the hook table) | −2,200 |
| 9 | check, clean, disk, host (the clean table); setup, state, config | −1,500 |
| 10 | asks, cards, queue; serve, API, streams; screens; simulation and perf (the one log builder) | −2,700 |

**Wanted, each.** The inventory's `table` and `merge` verdicts for its areas. Files that merge share one
fixture. No behaviour removed; a table row carries the old test's name. A new step of the ratchet.

**AC, each.** The keep job is green; test names gone are rows or in the ledger; the area's files define no
runner, HTTP client, repo builder or log reader of their own.

## 11. One test binary per area

Queue: `--perf` (baseline: a full `cargo test --no-run` from clean, its wall time and the size of the test
executables in the build folder: 265 files, 16.2 GB on a lane today). Blocked on 10.

**Wanted.** Each area's files move under a folder of the area with a `main.rs` that lists them as modules
(`shapes.md`, section 8). A file that sets process environment, or needs a process of its own, keeps its own
binary; the PR body lists those and why. `scripts/test-shards.sh` already reads `main.rs` folders; the shard list
is rebalanced. CI steps that name a moved test binary are updated in the same PR.

**AC.** The rev's `Perf:` block: build time and folder size before and after. The keep job counts 0 removed
tests. `cargo test` and the four nextest shards are green.

## 12. The close

Queue: plain; blocked on 11.

**Wanted.** The three bans of `adr.md` in `.drover/arch.toml`, with no file left that breaks them. The last step
of the ratchet: the measured percent rounded up, and 100 if the suite is under it. `README.md`'s test section,
if it has one, says where a new test goes: its area's file, a row if the verb has a table.

**AC.** The PR body reports tests, src, the percent, the summed test time, the binaries built, and each cut of
`adr.md`'s decision 9 as done, declined by the owner, or open with its issue number. If the percent is over
100, that list is the owner's next decision.
