# ADR: one scheduled-job mechanism for lesson, triage, deps and hotspots (#1756)

## Summary

Decision: a scheduled job is one config entry (how often, an optional check command, the issue's title and body, the
role, the queue place). One module files its issue, queues it and starts its lane, with at most one open issue per
job. Lesson, triage, deps, advisory and hotspots become five built-in entries; their lane verbs and reports stay.
Slices: S1 the mechanism with lesson, triage, deps and advisory (about -150), S2 hotspots (about -95), S3 the old
config keys go (about -25). The series nets about -270, not -1,000: 790 of the four modules' 1,388 lines are lane verbs.
Open questions: one, not blocking: also cut the shadow lesson checks (#510, about -190 more, unused in this repo)?

Read [README.md](README.md) first, then this, [shapes.md](shapes.md), [boundaries.md](boundaries.md).

## Question

Four recurring jobs each have a module, a verb, a `tick` hook and a config key. Each files a fixed issue on a
schedule, queues it and starts its role. How does that become one mechanism driven by config, and how much code does
it really remove?

## Where it stands (staging `456176c71`)

The four modules hold 1,388 lines of three kinds:

| Kind | Lines | What |
|---|---|---|
| The filing mechanism, written four times | about 480 | the schedule stamp, the title, the body, `gh issue create`, the queue entry, the spawn, "one open", the hand verb, the lane's end without a PR |
| Lane verbs and reports | about 790 | `lesson digest`, the rules checks (`lesson-check`, `lesson apply`, the shadow history), the prose-only rule; `triage close`, `fold`, `label`, `docs`; the hot-file finder, its report and drive's `hot-files` |
| The advisory parsers | about 120 | cargo-deny, pnpm and npm JSON to an id, a package and a fixed version |

The mechanism also leaks outside the modules: `check` has a lessons-only watchdog, `hook` has three
orchestrator-only rules, `spawn` and `lesson` sniff issue titles to know a job's issue, drive reads the last spawn's
role to respawn one, and `timer` has four steps. Triage's weekly run calls `lesson::file`, and `done --no-change`
goes through `lesson::done` to reach triage.

`[hygiene] triage = "weekly"` is a default that nothing reads: doctor prints it, and triage runs only on the fleet's
own `triage` key.

## Options

**What a job is**

1. Keep four modules, share helpers. `lesson::file` is already that. It removes little and every new job is a new
   module, verb, hook and key.
2. A trait with one implementation per job. Typed, but a new job is still code, and the four jobs stay four files of
   glue.
3. **A config entry (chosen).** A job is data: schedule, check, title, body, role, queue place. A new job is a few
   lines of the fleet's config.

**How a job decides there is nothing to do** (lesson's quiet week, no hot file, no advisory)

1. No precondition: always spawn, the lane closes itself. A daily advisory lane and a weekly architect lane that
   mostly find nothing: spend for nothing.
2. A closed list of built-in probes in Rust (`fails`, `hotspots`, `advisories`). Typed, but the mechanism then knows
   its jobs, and a config-only job can have no precondition.
3. **A check command (chosen).** The entry names a command. It passes: nothing to do. It fails: the job files, and
   the command's output is the report in the issue. This is how CI reads a check, `cargo deny check advisories` and
   `pnpm audit` already are such checks, and drover already runs the repo's commands (`premerge`, `done_checks`).

**Where "at most one open" is read**

1. A GitHub title search for both rules, as deps and hotspots do today. One call per run, and the prefix rule ties
   unrelated jobs together (`deps: ` holds the monthly run while an advisory is open).
2. **The log for "is one open", GitHub for "does this title exist" (chosen).** `job-filed` names the job's issues and
   the fold knows which are done: no call, exact per job. The title rule needs GitHub, because issues filed before
   the upgrade or by hand have no `job-filed`.

**Where the schedule is kept**

1. Derived from the log. One source, but the first tick after the upgrade would see no history and run every job at
   once, the monthly deps bump included.
2. **Today's stamp files through `timer::due` (chosen).** The keys already are `<job>-<fleet>`: `lesson-`, `triage-`,
   `deps-`, `advisory-` and `hotspots-`. No state migrates.

**Advisories**

1. One issue per advisory id, parsed (today). It needs the 120 lines of parsers, and three advisories on one day are
   three lanes and three PRs on one lockfile.
2. **One issue for every current advisory, with the tool's own output (chosen).** No parser. One lane, one PR.

## Decision

- **One module, `jobs`,** about 300 lines: the entry type and its layers, `tick`, `run`, the role and `empty`
  lookups, `close`, and the `job` verb (a fleet: its list; a fleet and a name: run it now). Shapes in
  [shapes.md](shapes.md), the run's steps in [README.md](README.md), the statecharts in
  [boundaries.md](boundaries.md).
- **Entries in three layers:** the built-in file, then `[job.<name>]`, then `[fleet.<n>.job.<name>]`, merged key by
  key and checked at load.
- **Five built-in entries**, one file each in a `jobs` folder under `roles/`: lesson, triage, deps, advisory,
  hotspots. Their issue bodies are today's issue templates, moved into the entries.
- **A check is the only precondition.** It runs in a checkout of the integration head. Lesson's check is its digest
  with a minimum FAIL count, hotspots' is the hot-file report, advisory's is the repo's audit tool.
- **At most one:** nothing while an issue of the job is open, and never a title that exists.
- **A run by hand** ignores the check's verdict, keeps its report, obeys "at most one" and leaves the stamp alone.
- **`empty`** is the one lane-end rule: a job's lane that ends with nothing pushed is prompted once and closed with
  the entry's line and the lane's undo list.
- **The mechanism names no job.** The role prompts, the triage brief and the hook's triage rules stay keyed on the
  role. `jobs.rs` never calls `lesson`, `triage` or `hotspots`.
- **Not in scope:** soak (it files issues nobody is spawned for, and runs detached for minutes), `compact`, `clean`,
  the hot-file finder and drive's `hot-files`, the rules checks.

## Every verb, config key and event

**Verbs**

| Today | Verdict | After |
|---|---|---|
| `drover lesson <fleet>` | generalise | the `job` verb with the fleet and `lesson` |
| `drover triage <fleet>` | generalise | the `job` verb with `triage` |
| `drover deps <fleet>` | generalise | the `job` verb with `deps` (and `advisory`) |
| `drover hotspots [fleet]`, the filing half | generalise | the `job` verb with `hotspots` |
| `drover hotspots [fleet]`, the report | keep | report only, stores the list, exits 1 with any hot file |
| `drover lesson digest` | keep | gains a minimum-FAILs flag that sets the exit code |
| `drover lesson apply`, `drover lesson-check` | keep | unchanged |
| `drover triage close`, `fold`, `label`, `docs` | keep | unchanged |
| `drover done --no-change` | keep | closes any `empty` job's issue through `jobs::close` |
| `GET lessons`, `GET lessons/<fleet>/digest`, `POST lesson` (API v1) | keep | same paths and keys, read from `jobs` |
| the hook's three orchestrator-only rules (lesson, triage, deps) | generalise | one rule: running a job by hand |

**Config keys**

| Today | Verdict | After |
|---|---|---|
| `[fleet.<n>] lessons` | generalise | `[fleet.<n>.job.lesson] every_days` (built in: 7) |
| `[fleet.<n>] hotspots` | generalise | `[fleet.<n>.job.hotspots] every_days` (built in: 1) |
| `[fleet.<n>] triage` | generalise | `[fleet.<n>.job.triage] every_days` (built in: 0) |
| `[fleet.<n>] deps` | generalise | `[fleet.<n>.job.deps] every_days` and `[fleet.<n>.job.advisory] every_days` (built in: 0) |
| `[hygiene] triage` | delete | nothing reads it; the triage entry is the one place |
| `[fleet.<n>.role.lesson]`, `.triage`, `.deps` | keep | any role a job names falls back to impl's harness when unset |
| `[hygiene] issue_spec = "triage"` | keep | not a schedule |
| (new) `[job.<name>]`, `[fleet.<n>.job.<name>]` | | the entry, [shapes.md](shapes.md) |

**Events**

| Today | Verdict | After |
|---|---|---|
| `lesson-skipped` | delete | a skip is a `tick.log` line; old ones stay in the log, unread |
| `lessons-missed` (the `check` alert) | delete | `job-failed`, for every job, at once instead of 8 days late |
| `lesson-prompted` | keep, for every `empty` job | the name stays: the passive list is frozen and old logs carry it |
| `lesson-shadow` | keep | the rules checks |
| `triage-closed`, `triage-folded`, `triage-labeled`, `triage-docs` | keep | their `undo` feeds the closing list |
| `closed` | keep | carries the undo list |
| `hot-files` | keep | drive's serial groups |
| `spawn` with role `lesson`, `triage`, `deps`; the queue's `add` | keep | |
| (new) `job-filed`, `job-failed` | | [shapes.md](shapes.md) |

**State files:** the five `.at` stamps keep their names. `hotspots-<fleet>.day` is no longer read. One job
checkout per fleet replaces the advisory checkout `deps-<fleet>/`; S1 removes the old worktree.

## How the config migrates

| A fleet has | It becomes |
|---|---|
| `lessons = "off"` | `[fleet.<n>.job.lesson]` `every_days = 0` |
| `lessons` unset or `"weekly"` | nothing: built in |
| `hotspots = "off"` | `[fleet.<n>.job.hotspots]` `every_days = 0` |
| `triage = "weekly"` | `[fleet.<n>.job.triage]` `every_days = 7` |
| `deps = "monthly"` | `[fleet.<n>.job.deps]` `every_days = 30` and `[fleet.<n>.job.advisory]` `every_days = 1` |
| `[hygiene] triage` | delete the line |

1. S1 and S2 still read an old key when the fleet's entry sets no `every_days`. So nothing turns on or off by itself.
2. Each old key still set is a `Config::warnings` row (doctor's `!` row, the board's line, `config-warning`) that
   names its replacement lines. Never a load error.
3. The S1 and S2 PR bodies carry `## Rollout` with the new lines, so the owner gets a config card (#1479).
4. S3 deletes the old keys and their reading. It starts only after those rollouts are applied.

## Consequences

- A new recurring job is config. No module, verb, hook or key.
- The mechanism is tested once, against a made-up entry, instead of four times.
- A refused spawn no longer starts the issue as `impl` later: drive reads the role from `job-filed`. The title
  sniffing in `lesson` and `spawn` goes.
- Lesson and triage wait while their last issue is open. The monthly deps run no longer waits for an advisory issue,
  and no longer loses its month when it is held.
- Lesson's "fleet under a week old" guard goes. The minimum of 2 FAIL verdicts stays, as the check.
- A lessons run by hand twice on one day is refused the second time: the title exists.
- Advisories lose the parsed package and fixed version in the issue; the lane reads the tool's output. An advisory
  with no fix is filed again each day until the repo's audit config ignores it, which is already the role's rule and
  an owner ask.
- A check that fails for another reason (the advisory database is unreachable) files one issue that day. Its lane
  finds nothing and closes it through `empty`. Today's parser files nothing in that case.
- A check is the owner's command, run on the main host outside a lane's sandbox, as `premerge` checks are. The
  built-in checks only read.
- The report goes into a public issue. The checkout's path is replaced before filing, and `PUBLIC_LEAK` still guards
  the text. Lesson's body does not use its report: the digest quotes the owner.
- An open hotspots issue's body is no longer rewritten when the list changes.
- cooee-threads D4 deletes one caller less: `triage::done` and its thread reply go in S1. No order between the two.
- The passive `job` event and `drive::job` (drive's plan kick) are another thing with a similar name. Not renamed.

## Slices

Each is one PR, src/ net negative. Line numbers are estimates, each within about 50.

| Slice | Builds | Deletes | src/ net |
|---|---|---|---|
| **S1** mechanism, lesson, triage, deps, advisory | `jobs` (about 300), the entry layers in `config` (25), the digest's exit flag (6), four built-in entries, the old keys read as overrides with their warnings, one `jobs` step in `tick`, one hook rule | `lesson`: run, cli, file, the stamp, weekly, done, close, prompt, idle close, the no-PR test (185). `triage`: run, weekly, done (75). `deps.rs` whole, its title search moved to `gh` (185). The hand verbs in `cli` (10), two hook rules (8), `lessons-missed` (11), three `tick` steps (6) | about -150 |
| **S2** hotspots | the hotspots entry, the report's exit code (3) | `hotspots`: file, daily, the title constant (85). The title sniff and brief appendix in `spawn` (10). One `tick` step (2) | about -95 |
| **S3** old keys | nothing | the four fleet keys, their reading and warnings, `[hygiene] triage` | about -25 |

- S1 cannot be split into net-negative PRs. The mechanism alone is +325; with lesson and triage, which share one
  lane-end path, it is still about +45. With deps and advisory it is about -150.
- Tests: each slice re-points the filing tests at the `job` verb and keeps the lane verbs' tests as they are. S1
  adds the mechanism's own tests on a made-up entry: due, check passes, check fails, one open, title exists, spawn
  refused, `empty` close with an undo list, the three layers, each load error.
- Docs, split per area: one new page for jobs in `docs/config/` and one usage part under `roles/usage/`; the
  lessons, triage and deps usage parts and the hotspots page lose their filing sentences. Each slice reruns
  `just gen` for `fleet.example.toml` and the brief goldens.

**Target for the series: about -270 src/ lines** (staging is 65,464 of a 65,963 cap). Not -1,000: the mechanism that
is written four times is about 480 lines and its replacement about 300; the parsers add -120; the special cases
outside the modules add about -50; config and exit codes cost about +20.

## Open question

**Cut the shadow lesson checks (#510)?** About -190 more: `checks`, `failing`, `record`, `review`, `apply`, the
shadow half of `lesson-check`, the `lesson apply` verb, the premerge hook in `gate`, `lesson-shadow`. In this repo
`.drover/rules.md` has never held a `shadow since` or `active since` line (`git log -S` finds none, and the 30-day
digest lists no check). I could not see the other fleets' repos. It is a feature cut, and this issue keeps `lesson
apply`, so it is in no slice. Queue it as its own issue, or leave it. The ADR stands either way.

Beyond that, -1,000 would need the triage verbs (about 300) and the digest and the hot-file finder (about 260) gone
as well. I do not propose either: lanes and drive use them.

## `.drover/arch.toml` lines

S1 adds them in the PR that creates the folder and the module.

```toml
[[collect]]
dir = "roles/jobs"
glob = "*.toml"

[[ban]]
pattern = "lesson::"
paths = ["crates/core/src/jobs.rs"]
reason = "#1756: the job mechanism names no job; a job is an entry in roles/jobs/ or the fleet's config"

[[ban]]
pattern = "triage::"
paths = ["crates/core/src/jobs.rs"]
reason = "#1756: the job mechanism names no job; a job is an entry in roles/jobs/ or the fleet's config"

[[ban]]
pattern = "hotspots::"
paths = ["crates/core/src/jobs.rs"]
reason = "#1756: the job mechanism names no job; a job is an entry in roles/jobs/ or the fleet's config"
```

No file is frozen and none is generated: the built-in entries are a collected folder from the start, and the four
`tick` steps become one, so `timer.rs` stops being where a new job adds a line.
