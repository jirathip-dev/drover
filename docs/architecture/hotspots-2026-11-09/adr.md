# ADR: hotspots week of 2026-11-09 — convert the central lists, fix the signal (#973)

## Summary

Decision: the hot files are central lists every feature appends to, so convert them with the repo's own #890 tooling instead of splitting code by hand: the line cap moves into one file per raise, the agent-help line budget is retired, the dispatch of the CLI splits into one file per command area, a new event kind marks itself passive where it is emitted, and the hotspots report stops counting files that no longer exist.
Slices: 1 the report (ghost rows, distinct conflict pairs, the generated types file), 2 cap raises as a collected folder, 3 retire the help budget, 4 passive events by flag, 5 the CLI dispatch split (`--refactor`), 6 the limits table from one macro (`--refactor`), 7 the function-size ratchet.
Open questions: whether to keep any budget on `help --agent` after slice 3 (I retire it), whether the size ratchet in slice 7 goes on now or waits a week, and whether slices 5 and 6 go as their own issues (recommended) or into this PR.

Status: proposed (architect lane, 2026-11-13). Read `README.md` first.

## Question

The week's report names 273 hot files. Which of them are structural, what structure removes the collision, and what in
the report itself is noise that would send next week's architect after the wrong files?

## Where it stands

- `ci/cap.toml` is raised by 27 of its 29 merges: one number, 396 `conflict-predicted` events. The note beside each
  raise already is one file per change (`docs/architecture.md:161`): only the number still collides.
- `crates/core/src/cli.rs:921` is a budget bumped per feature (33 of 99 merges), with a comment that lists every bump.
  A limit that is raised on demand every time guards nothing; a rule nothing enforces is retired.
- `crates/core/src/events.rs:415` lists 71 passive kinds; every new alert or note appends one. The list exists so the
  fold (`crates/core/src/events.rs:911`) and the idle-pane reader (`crates/core/src/spawn.rs:1090`) can skip them.
- `crates/core/src/config.rs:583` (`Limits`), its `Default` and `crates/core/src/config.rs:778` (`LIMIT_DOCS`) take
  a hunk each per new key, by #844's rule that every timing is a key.
- `check`, `spawn` and `pass` are 543, 659 and 362 lines, and `dispatch` 985; this repo's `.drover/arch.toml` has
  no `[[size]]`, no `frozen`, no `[[collect]]`, no `generated`: the tooling built in #734 and #890 is unused here.
- 67 rows are paths removed on Nov 3 (`src/`, `tests/`); `crates/core/src/hotspots.rs` never checks the tip.
- `conflict-predicted` fires per new pair of heads (`crates/core/src/overlap.rs:48`), so the count is pushes, not
  pairs: 442 on `justfile` from one long-lived PR.
- The old board's files are frozen by `scripts/board-freeze.sh` since Nov 12; their rows decay by Nov 26.

## Options

1. **Wait for decay and raise the threshold.** The ghost and board rows vanish by themselves; set `HOT` to 10.
   Leaves the cap, the budget and the lists colliding; hides the next hotspot behind a higher bar.
2. **Split the giant functions by hand, one a week.** Fewer hunks per function, but a split chosen by a lane is a
   refactor of live code with no rule that keeps it split; next week's features grow the halves.
3. **Convert the central lists with #890's own tooling and fix the signal (chosen).** Each list becomes one file
   per entry (cap), a flag at the source (events), a one-line macro entry (limits) or is retired (budget); the
   frozen/collected/generated keys make hotspots stop reporting them; a `[[size]]` ratchet keeps the giants from
   growing so new code lands in new functions.
4. **Move the lists into the event log or config.** The cap as a `[limits]` key, the passive set in fleet.toml: moves
   the collision from a source file to the owner's config and makes the tests environment-dependent.

## Decision

- **The cap (slice 2).** The effective cap is the highest `line_cap` among `ci/cap.toml` and `ci/cap.d/*.toml` at the
  sha in question. A raise adds `ci/cap.d/<date>-<cap>.toml` with `line_cap` and a `why` string that is the note
  (the orchestrator's `cap: raise … to <N>` issue, as today); `ci/cap.toml` keeps `[fleet.herdsman]`, today's
  `line_cap` as the floor and `warn_within`, and is listed `frozen`; the folder is a `[[collect]]`. One reader
  (`overlap::cap_at_sha`) serves `cap-check` (the working tree), `cap_at` and `cap_view` (a sha). The hook's
  `CAP_FILE` refusal covers the folder too, with the same exemption (an issue titled `cap:`). Highest, not sum: two
  raises filed from the same measurement merge cleanly and never double. `docs/cap-history/` keeps its 46 notes and
  gets no new ones.
- **The help budget (slice 3).** `HELP_AGENT_MAX` and the `<=` assertion go; the completeness assertion stays. The
  context guard this leaves is the weekly table itself: `roles/usage-tail.md` is on it.
- **Passive events (slice 4).** `Event` gains `passive: bool` (serde default, skipped when false, so the on-disk
  rows and the TS type only gain an optional field). `Event::passive(self)` sets it; `check::fire`, the notifier
  paths and the `promise`, `why` and `turn-end` writers call it. The fold and the idle-pane reader test
  `e.passive || passive::LEGACY.contains(kind)`. The list moves to `events/passive.rs` under `crates/core/src/` as the frozen
  record of kinds logged before the flag; a new kind added to it fails arch-check.
- **The dispatch (slice 5, `--refactor`).** `crates/core/src/cli.rs` becomes `cli/mod.rs` (`Cli`, `Cmd`, `main_with`,
  `usage_doc`, a `dispatch` of one-line arms) plus one file per nested enum (`queue`, `thread`, `hosts`, `config`,
  `card`, `secret`, `profiles`, `lesson`, `runners`, `wip`, `ask`, `serve`, `accounts`, `push`, `stats`, `keys`) and
  `hook.rs` for the hook, pre-push and lane-event arms; the remaining flat commands go to the file of the core module
  they call (`lane.rs`: spawn, adopt, verdict, merge, turn, done, extend, park, unpark, clean, kill, stop, close;
  `owner.rs`: drive, tick, timer, setup, doctor, check, migrate, report, serve, service). Pure move; `lib.rs`'s
  re-exports keep every test path.
- **The limits (slice 6, `--refactor`).** A `limits!` macro takes `name: type = default, "doc";` per key and emits
  the struct (with serde defaults), `Default` and `LIMIT_DOCS`; `config show` and `crates/cli/tests/limits.rs` are
  unchanged. Keys stay in today's order; the macro is the only new code.
- **The ratchet (slice 7).** `[[size]]` on `crates/*/src/**/*.rs`, 150 lines a function and 2000 a file, tests
  excluded, baseline written once. A grown offender fails; a new function passes. Owner's call whether now.
- **The signal (slice 1).** `find` keeps only files in `git ls-tree -r --name-only origin/<integration>`; `conflicts`
  counts distinct `(issue, other)` pairs from the events' `other` field. `assets/types/api.d.ts` is `generated`
  with `generate = "just types"`, and the three CI lines that regenerate and diff it are deleted. `web/dist/` is
  left alone: frozen by the board rule, it decays.

## Consequences

- Next week's table loses the 67 ghosts, the cap file, the types file and (by decay) the board; what remains is the
  real churn, and the conflict column means pairs.
- A raise PR touches one new file: no conflict, no update round. The highest-wins rule means a raise filed on a stale
  measurement is harmless.
- A new event kind is one line at its emit site. The frozen list can only shrink.
- `cli.rs` keeps 56 hunks a fortnight on `enum Cmd` (one line per command is the floor for clap); its dispatch arms
  spread over 18 files.
- Behaviour is unchanged in every slice but the help budget, which is deleted, and the cap reader, which returns the
  same number until the first raise file exists. Rough size: slices 1–4 src net ≈ +90 against ≈ −60 (the budget
  comment, the list's doc lines, CI's types step); 5 and 6 are moves.
- Docs: `docs/architecture.md`'s size-cap paragraph names the folder; `docs/config.md`'s hotspots paragraph says the
  tip filter and pair count; `roles/orchestrator.md`, `roles/impl.md` and `roles/usage-tail.md` name the `cap.d` folder under `ci/`.
- Not this week: `roles/usage-tail.md`, `fleet.example.toml` and the docs are prose every feature extends; a
  collected folder for prose moves the conflict, not the growth. `config.rs`'s `WEBPUSH_ON` (16 hunks) could take the
  same flag as `passive` later (an alert declares itself pushable); left for a week with cleaner numbers.

## Open questions for the owner

1. **A budget on `help --agent`.** I retire it (a bumped-on-demand number guards nothing). The alternative is a per
   section cap on `roles/usage-tail.md` lines, which forces rewrites instead of bumps. Recommend: retire.
2. **The size ratchet now or later.** On now, every lane touching `check`, `spawn`, `pass`, `present`, `scan` or
   `waits` must carve its change into a new function from the first day. Recommend: on now, baseline once.
3. **Slices 5 and 6 as follow-up issues** queued `--refactor` after this PR merges, or in this PR. Recommend: follow-ups.
