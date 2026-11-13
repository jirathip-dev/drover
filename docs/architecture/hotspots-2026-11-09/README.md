# hotspots: herdsman week of 2026-11-09 (#973)

## Summary

Decision: the hot files are central lists every feature appends to, so convert them with the repo's own #890 tooling instead of splitting code by hand: the line cap moves into one file per raise, the agent-help line budget is retired, the dispatch of the CLI splits into one file per command area, a new event kind marks itself passive where it is emitted, and the hotspots report stops counting files that no longer exist.
Slices: 1 the report (ghost rows, distinct conflict pairs, the generated types file), 2 cap raises as a collected folder, 3 retire the help budget, 4 passive events by flag, 5 the CLI dispatch split (`--refactor`), 6 the limits table from one macro (`--refactor`), 7 the function-size ratchet.
Open questions: whether to keep any budget on `help --agent` after slice 3 (I retire it), whether the size ratchet in slice 7 goes on now or waits a week, and whether slices 5 and 6 go as their own issues (recommended) or into this PR.

Read this file, then `adr.md`, `shapes.md`, `boundaries.md`.

## Where it stands (origin/staging 7ed65121, 14 days to 2026-11-13)

433 PRs merged. The table in the issue has 273 rows; 67 of them are paths that no longer exist (the Nov 3 move from
`src/` and `tests/` into `crates/`), and the frozen old board (`crates/server/src/serve.rs`, `assets/serve.js`,
`web/dist/`, frozen by `scripts/board-freeze.sh` since Nov 12) fills eight more that decay on their own by Nov 26.

What is left, by where the hunks land (every merge's diff on origin/staging, grouped by the function or item it hit):

| file | merges | what collides | fix |
|---|---|---|---|
| `crates/core/src/cli.rs` | 99 | 35 merges added an arm to the 985-line `dispatch`; 33 bumped `HELP_AGENT_MAX` and its 40-issue comment | slices 3, 5 |
| `ci/cap.toml` | 29 (27 raise the number) | one line, 396 conflict events; the note beside it already is one file per raise | slice 2 |
| `crates/core/src/events.rs` | 57 | 28 hunks on the `PASSIVE` list of event kinds | slice 4 |
| `crates/core/src/config.rs` | 73 | 44 hunks on `Limits`, its `Default` and `LIMIT_DOCS`: three places per key | slice 6 |
| `crates/core/src/check.rs`, `spawn.rs`, `drive.rs` | 71, 74, 70 | one function each (543, 659, 362 lines) takes most hunks | slice 7 |
| `assets/types/api.d.ts` | 36 | generated, hand-merged | slice 1 |
| `roles/usage-tail.md`, `fleet.example.toml`, `docs/*.md` | 70, 67, 51–78 | prose and examples every feature extends | not this week (see adr) |

## How the callers read, after

A cap raise is one new file, never an edit (the orchestrator's `cap: raise … to <N>` issue adds it; the hook lets
only that lane write under the folder):

```toml
# ci/cap.d/2026-11-11-45400.toml
line_cap = 45400
why = """
41,000 → 45,400: headroom for the whole queue (#866). Measured by `drover cap drover` against staging 27e3949:
40,986 of 41,000. Open PRs +1,087; fourteen queued issues with no PR, estimated +2,790; projects 44,863 plus 500.
"""
```

`cap-check`, `drover cap`, the `cap-near` alert and the page banner read one function, the effective cap at a sha:
the highest `line_cap` among `ci/cap.toml` and `ci/cap.d/*.toml`.

```rust
let cap = overlap::cap_at_sha(r, &dir, &sha)?;   // Cap { lines: 45_400, warn_within: 500, from: "ci/cap.d/2026-11-11-45400.toml" }
```

A new event kind says it is passive where it is born; nothing is added to a list:

```rust
emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "build-freed", d).passive())?;
```

`check.rs`'s `fire` (every alert) and the notifier paths call `.passive()` once, so most kinds get it for free.
The old `PASSIVE` list moves to its own file and is frozen: it names the kinds logged before the flag existed.

A new subcommand lands in its area's file; the top-level `dispatch` arm is one line:

```rust
// crates/core/src/cli/mod.rs
Cmd::Queue { fleet, cmd } => queue::run(cmd, &fleet, ctx, r, out)?,
// crates/core/src/cli/queue.rs holds `enum QueueCmd` and `pub fn run`
```

A new `[limits]` key is one line, read by the struct, its default and `config show`:

```rust
limits! {
    review_rounds: u32 = 3, "review rounds before an issue goes needs-human (ROUND_LIMIT)";
    stall_minutes: u64 = 30, "a working lane with no event this long is stalled";
}
```

The weekly report lists only files at the integration tip, and counts the PR pairs a file collided in, not the
events (one long-lived PR re-pushed against `justfile` 442 times; it is one pair).

## Slices, in order

1. **The signal** (`crates/core/src/hotspots.rs`, `.drover/arch.toml`, `.github/workflows/ci.yml`): drop paths not
   in `git ls-tree` at the integration tip; count distinct `(issue, other)` pairs; list `assets/types/api.d.ts` as
   `generated` with `generate = "just types"` and delete CI's own stale-types step (one way to check).
2. **Cap raises as a folder** (`crates/core/src/overlap.rs`, `crates/core/src/hook.rs`, `.drover/arch.toml`,
   `roles/orchestrator.md`, `roles/impl.md`, `roles/usage-tail.md`, `docs/architecture.md`): the folder, the one
   reader, the hook's refusal widened to the folder, `ci/cap.toml` frozen. Old notes in `docs/cap-history/` stay.
3. **Retire the help budget** (`crates/core/src/cli.rs`, `crates/cli/tests/spawn.rs`): delete `HELP_AGENT_MAX` and
   the `<=` assertion; the completeness half of the test stays.
4. **Passive by flag** (`crates/core/src/events.rs`, `check.rs`, `spawn.rs`): the `passive` field, `.passive()`,
   the frozen list file, the fold and the idle-pane reader checking both.
5. **Dispatch split**, queued `--refactor`: `cli.rs` becomes a folder, one file per nested command enum and per
   area; tests change by path only.
6. **One `limits!` macro**, queued `--refactor`: struct, `Default` and `LIMIT_DOCS` from one entry per key.
7. **Size ratchet** (`.drover/arch.toml` plus the baseline): `[[size]]` at 150 lines a function and 2000 a file,
   `drover arch-check --write-baseline` once; the giants may only shrink.

Slices 1–4 fit this issue's PR (each its own commit). 5 and 6 are large pure moves that conflict with every open PR
while they wait for review, so they go as two follow-up issues queued `--refactor`, after this PR merges. 7 is a
two-line config change but changes every lane's day, so it is an owner call (open question 2).
