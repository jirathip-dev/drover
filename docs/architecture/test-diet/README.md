# test diet: the audit of the suite, the cut, and a test cap that only falls (#1760)

## Summary

Decision: first make "it may only fall" a rule, then show a test's cover before it goes, then cut. The audit read all 288 test files: the excess is copied fixtures and tests of features already on their way out, far more than duplicate tests. With the cuts already decided, its verdicts take tests from 147% of src to about 108%; the 100% cap needs the further cuts the ADR names, three of them the owner's call (question 3). The cap itself is one more line of the cap check: test lines at most a percent of src lines, the percent kept as files in a folder where the lowest wins and no file may be changed or removed.
Slices: 1 the test cap; 2 the keep check; 3 slow tests off the PR path (2,011 s of test time per run to about 1,200 s, about 900 s once the cuts land); 4 and 5 the deletes; 6 shared builders, proven on one area; 7 to 10 the other areas; 11 one test binary per area (16 GB of test executables per lane to about 3); 12 the close.
Open questions: (1) the cap check goes into drover's cap check now and moves to gatekeeper with it, rather than waiting for gatekeeper: yes? (2) cargo-llvm-cov as a pinned CI tool, in one job: yes? (3) retire three things: the pre-rename move, the form route no client in this repo calls, the three sim owner tools in the test tree? (4) approving this drops parity rows 2, 20 and 63, each held by another test.

## Read in this order

| File | What |
|---|---|
| this page | the audit and the plan on one screen; what the owner decides |
| `adr.md` | question, findings, the numbers, options, the twelve decisions, how cover is checked, the issues in order, consequences, the open questions, the `arch.toml` lines |
| `inventory.md` | every test file: keep, table, merge, delete or leaves, with the reason; the duplicate clusters; the copied fixtures |
| `usage.md` | how it reads afterwards: a PR over the cap, a step of the ratchet, the ledger, a test on the builders, the nightly run |
| `shapes.md` | the ratchet file, what is counted, the check's output, the ledger, the builders, a table test, the folder layout |
| `boundaries.md` | which test module may call what, the layers of assertion, the cap's callers, four statecharts |
| `issues.md` | the twelve issues as they are to be filed |

Read on staging `4cf52e4bf` and its CI run, 2026-11-25. Every test body was read; nothing was compiled or run, so
each "after" figure is an estimate from counted lines.

## The audit on one screen

```
 94,937 test lines                                         64,744 src lines            147%
 ├─ 13,400  other hosts            leave with #1691 (in flight)
 ├─  9,400  threads, repo checks, compact, shed parts      leave with #1571, #1643, #1574
 │                                                         src after those cuts: 55,000  131%
 ├─  4,300  duplicate, implementation-detail, dead tests   deleted, each with a ledger line
 ├─  4,000  tests that are rows of a table                 one table test each
 ├─  4,700  fixtures copied from file to file              shared builders (+600 new lines)
 └─ 59,700  left                                                                         108%
     └─ 4,700 to the cap: page-client tests (#1665), the sim owner tools, the form
        route, the builders inside test bodies, the pre-rename move        the owner's calls
```

Three things the numbers do not show:

- **The suite is mostly sound.** Nine tests in ten assert one distinct behaviour at the CLI or API edge, once.
  True duplicates are about 2,700 lines. Whoever expected a third of the suite to be redundant tests will not
  find it; the redundancy is in the setup around the tests.
- **The time is in ten binaries**, not in the line count: the simulator's own self-tests, five HTTP clients
  that wait out a 5 s timeout on every request, a quadratic replay, release budgets that run in debug for
  nothing. Slice 3 takes about 800 s of 2,011 s off every run without deleting a test.
- **The build is 259 links.** One lane's build folder holds 16.2 GB of test executables. Slice 11 makes that
  about 45 binaries.

## The plan on one screen

| | Today | After |
|---|---|---|
| What stops tests growing | nothing | the cap check fails a PR whose tests are over a percent of src; the percent is the lowest file of a folder, and no file there can be changed or removed |
| What a lane does over the cap | | adds its case as a row, or deletes a redundant test with its ledger line; there is no raise |
| Deleting a test | a reviewer's judgement | a ledger line, no src line losing its cover (CI), and for a duplicate the keeper shown failing |
| A fleet, an issue, a lane, a PR in a test | rebuilt from strings in each file: `setup` in 63 files | one shared world; a test names the facts that matter |
| Many tests that differ in one input | one function each | one table; a row's label is the old test's name |
| Slow and scale tests | on every PR | in `nightly` modules, with a small check of each left on the PR path |
| Test binaries | 259, one per file | about 45, one per area |
| Summed test time per run | 2,011 s | about 900 s |
| Tests / src | 147% | 108% from the audit; 100% with the further cuts of decision 9 |

## What the owner decides

Approve, revise or reject, and answer four questions (`adr.md`, "Open questions"):

1. **The cap check now, in drover** (about 45 src lines, ported to gatekeeper with the rest of the cap check by
   #1643), or wait for gatekeeper and run the diet unguarded until then. I recommend now.
2. **cargo-llvm-cov** as a CI tool for the keep job. Without it, a deleted test is checked by the ledger and
   the reviewer only. I recommend yes.
3. **Three retirements**, each yes or no: the pre-rename move of the `herdsman` trees (287 test lines), the form
   route `POST /` that the page no longer calls (500 or more), the sim owner tools `--replay`, `--what-if` and
   `--metrics` that sit in the test tree (1,320: keep counted, move out, or drop).
4. **Parity rows 2, 20 and 63** are dropped by approving, since another test holds each.

One thing to know before approving: **the audit does not reach 1.0× by itself.** It reaches about 1.08×. The
rest is named in `adr.md`'s decision 9 and mostly waits on question 3 and on #1665. If those answers are no,
the ratchet comes to rest above 100 and stays there until a lower step is added.
