# test diet: the audit of the suite and the cut, under gatekeeper's test ratchet (#1760)

## Summary

Decision: drover builds no test cap. The ratchet is gatekeeper's test-health check (gatekeeper#99), which drover gets through that check's own adoption issue and which starts at the ratio of that day: 1.50 if it were today. This ADR is the audit and the cut: show a test's cover before it goes, then cut. The audit read all 288 test files: the excess is copied fixtures and tests of features on their way out, far more than duplicate tests. Its verdicts and the three retirements you approved take tests from 1.47 times src to about 1.05; 1.00 needs the page-client tests to move (#1665) and the builders used inside test bodies.
Slices: 1 the keep check, with cargo-llvm-cov; 2 slow tests off the PR path (2,011 s of test time per run to about 1,320 s, about 900 s once the cuts land); 3 and 4 the deletes; 5 what you retired: the pre-rename move, the form route once no client is found calling it, the sim owner tools, parity rows 2, 20 and 63; 6 shared builders, proven on one area; 7 to 10 the other areas; 11 one test binary per area (16 GB of test executables per lane to about 3); 12 the close, once gatekeeper's check is on.
Open questions: none.

## What the owner's revision of 2026-11-25 changed

| The owner said | Where it landed |
|---|---|
| Drop slice 1; the ratchet is gatekeeper#99's check; no slice builds a test cap in drover | The cap slice, its folder, its rule line and its 45 src lines are gone. `adr.md`, "The ratchet is gatekeeper's", says what that check asks of each slice. |
| It starts at today's ratio | gatekeeper writes the first ceiling at the measured ratio rounded up to its 0.05 grid: 1.50 on today's tree (1.47 measured by its count). |
| cargo-llvm-cov: yes | Slice 1, the keep check, as designed. |
| Retire the pre-rename move, the form route (the slice confirms no client calls it), the sim owner tools; drop parity rows 2, 20, 63 | A slice of their own, 5, with their src. The inventory rows are verdicts now, not questions. |
| Renumber the slices | Twelve again: the cap slice left, the retirements became one. Slices 6 to 12 keep their numbers. |

Reading gatekeeper#99's approved ADR changed two more things. Slow tests move into whole soak files, the way
that check marks them, in place of a `nightly` module of drover's own. And its 2 s per-test budget makes a
slice that rewrites a test file answer for every slow test in it, so slice 2 now lists them by area.

## Read in this order

| File | What |
|---|---|
| this page | the audit and the plan on one screen |
| `adr.md` | question, findings, the numbers, options, what gatekeeper's ratchet asks of the diet, the twelve decisions, how cover is checked, the issues in order, consequences, the `arch.toml` lines |
| `inventory.md` | every test file: keep, table, merge, delete or leaves, with the reason; the duplicate clusters; the copied fixtures |
| `usage.md` | how it reads afterwards: a slice under gatekeeper's check, the ledger, a test on the builders, the soak run |
| `shapes.md` | what gatekeeper counts here and the soak files, the ledger, the builders, a table test, the folder layout |
| `boundaries.md` | which test module may call what, the layers of assertion, who owns which check, three statecharts |
| `issues.md` | the twelve issues as they are to be filed |

Read on staging `4cf52e4bf` and its CI run, 2026-11-25. Every test body was read; nothing was compiled or run, so
each "after" figure is an estimate from counted lines.

## The audit on one screen

```
 94,937 test lines                                         64,744 src lines            1.47
 ├─ 13,400  other hosts            leave with #1691 (in flight)
 ├─  9,400  threads, repo checks, compact, shed parts      leave with #1571, #1643, #1574
 │                                                         src after those cuts: 55,000  1.31
 ├─  4,300  duplicate, implementation-detail, dead tests   deleted, each with a ledger line
 ├─  2,100  the owner's retirements                        deleted with their src
 ├─  4,000  tests that are rows of a table                 one table test each
 ├─  4,700  fixtures copied from file to file              shared builders (+600 new lines)
 └─ 57,600  left                                           src 54,800                    1.05
     └─ 2,800 to 1.00: page-client tests (#1665), the builders inside test bodies,
        one word table as a data file
```

Three things the numbers do not show:

- **The suite is mostly sound.** Nine tests in ten assert one distinct behaviour at the CLI or API edge, once.
  True duplicates are about 2,700 lines. Whoever expected a third of the suite to be redundant tests will not
  find it; the redundancy is in the setup around the tests.
- **The time is in few tests**, not in the line count: 143 tests over 2 s take 1,603 of the 2,011 s. The
  simulator's own self-tests, five HTTP clients that wait out a 5 s timeout on every request, a quadratic
  replay, release budgets that run in debug for nothing. Slice 2 takes about 700 s off every run without
  deleting a test.
- **The build is 259 links.** One lane's build folder holds 16.2 GB of test executables. Slice 11 makes that
  about 45 binaries.

## The plan on one screen

| | Today | After |
|---|---|---|
| What stops tests growing | nothing | gatekeeper's ceiling on test lines over src lines, which only falls (not built here) |
| What a lane does over the ceiling | | adds its case as a row, or deletes a redundant test with its ledger line; there is no raise |
| Deleting a test | a reviewer's judgement | a ledger line, no src line losing its cover (CI), and for a duplicate the keeper shown failing |
| A fleet, an issue, a lane, a PR in a test | rebuilt from strings in each file: `setup` in 63 files | one shared world; a test names the facts that matter |
| Many tests that differ in one input | one function each | one table; a row's label is the old test's name |
| Slow and scale tests | on every PR | in three soak files, run nightly, with a small check of each left on the PR path |
| Test binaries | 259, one per file | about 45, one per area |
| Summed test time per run | 2,011 s | about 900 s |
| Tests / src | 1.47 | about 1.05 from the audit and the retirements; 1.00 with the cuts of decision 9 |

## What the owner decides

Approve, revise or reject. There is no open question. Three things to know before approving:

1. **The diet runs without a ratchet until gatekeeper's check is adopted.** That adoption waits for gatekeeper
   1.6.0 and for drover's move to gatekeeper (#1587). Slices 1 to 11 do not wait for it; the close does.
2. **The audit and the retirements reach about 1.05, not 1.00.** The rest is named in `adr.md`'s decision 9 and
   mostly waits on #1665. Until then gatekeeper's ceiling rests at 1.05 or 1.10.
3. **The retirements take the sim's nightly metrics table with them**: it is the `--metrics` tool's output.
