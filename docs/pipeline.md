# Pipeline (#173)

A fleet's pipeline is data in `fleet.toml`: named stages, each of a fixed kind, and where each outcome goes.
drover validates it when it loads the config. `drive`, the gates and the board all follow it.
Without a `[fleet.<n>.pipeline]`, a fleet runs the built-in pipeline, which is the impl → rev loop from before #173.

## Kinds

| kind | what it is | its outcomes |
|---|---|---|
| `build` ▲ | a lane that pushes code (`roles/impl.md`) | `push` |
| `check` ◆ | a lane that gives PASS or FAIL at the head (`roles/rev.md`, read-only) | `PASS`, `FAIL` |
| `gate` ◫ | drover's own: CI, PASS at the head, serial, mergeable | `ok`, `ci_red`, `conflict` |
| end | `merge` ✓ or `park` ! (needs you) | none |

New kinds need code. A target is a stage name, `merge` or `park`. A target of `<build>:update` is an update round
(the gate's `conflict`).

## The built-in pipeline

```toml
[[fleet.x.pipeline.stage]]
name = "impl"
kind = "build"
role = "impl"
next = { push = "rev" }

[[fleet.x.pipeline.stage]]
name = "rev"
kind = "check"
role = "rev"
rounds = 3              # [limits] review_rounds
next = { PASS = "gates", FAIL = "impl" }

[[fleet.x.pipeline.stage]]
name = "gates"
kind = "gate"
rounds = 3              # [limits] update_rounds
next = { ok = "merge", ci_red = "impl", conflict = "impl:update" }
```

- **Rounds:** `[limits] review_rounds` and `update_rounds` are the built-in pipeline's limits. A fleet with its own
  pipeline sets `rounds` on each check, which limits its reviews, and on the gate, which limits its update rounds.
  A gate without `rounds` takes `update_rounds`.
- **Out of rounds** always goes to `park` (needs-human). It is not an outcome you map.
- **Red CI** after a PASS is the gate's FAIL at the same head. It counts against the rounds of the check that reviews
  the fix (`ci_red` → impl → `push` → rev), as before #173. In the last round it goes to park at once.

## The verify stage (#417)

A fleet that defines `[fleet.<n>.role.verify]` gets one more check in the built-in pipeline:

```toml
# rev: next = { PASS = "verify", FAIL = "impl" }
[[fleet.x.pipeline.stage]]
name = "verify"
kind = "check"
role = "verify"
rounds = 3              # [limits] review_rounds, shared with rev
next = { PASS = "gates", FAIL = "impl" }
```

- **What it does:** a read-only lane (`roles/verify.md`) launches the app at the head, following the
  integration branch's `.drover/verify.md` (#958: a PR's own copy is under review, never its recipe), runs its Doctor, drives each acceptance criterion a user can see and records PASS or FAIL.
  Any visual deviation from the approved `docs/design/<slug>/` PNGs is a FAIL. Its screenshots (1440 and 390 wide)
  stay in its lane's folder, listed by path in `verdict.md`; the verdict event carries them, and the board's
  needs-you card shows them inline.
- **Rounds:** a verify FAIL counts in rev's rounds (the round of the head rev reviewed), so the third FAIL across both
  parks the issue. Its fix pushes a new head, which goes rev → verify again. A verify PASS is for its own head only.
- **When it runs:** for a head rev PASSed, if the issue had a design stage (#412), the PR touches the fleet's
  `ui_paths` (globs), or it is queued `--verify`. `queue add N --verify [on|off]` and `queue set N verify on|off`
  force it either way. Otherwise drive logs `verify-skipped` at that head and the issue goes on to the gates.
- **No recipe:** when it should run but the integration branch has no `.drover/verify.md`, drive logs `verify-no-recipe`, the
  Queue card says so, and the issue goes on to the gates. It never blocks. `roles/verify-recipe.md` is the brief
  for an impl lane that writes the recipe (Launch, Doctor, Drive, Evidence, Feature map, Known noise), proving each step.
- Fleets without a `verify` role run the pipeline above, unchanged. A custom pipeline places a check with role
  `verify` itself; it gets the same prompt and skip rules, and counts its own rounds.

## A custom pipeline: two checks

```toml
[fleet.atlas.role.sec]
harness = "claude"
model = "claude-opus-5-5"
prompt = "~/.config/drover/prompts/sec.md"   # appended to the check prompt

[[fleet.atlas.pipeline.stage]]
name = "impl"
kind = "build"
role = "impl"
next = { push = "rev" }

[[fleet.atlas.pipeline.stage]]
name = "rev"
kind = "check"
role = "rev"
rounds = 3
next = { PASS = "sec", FAIL = "impl" }

[[fleet.atlas.pipeline.stage]]
name = "sec"
kind = "check"
role = "sec"
rounds = 2
next = { PASS = "gates", FAIL = "impl" }

[[fleet.atlas.pipeline.stage]]
name = "gates"
kind = "gate"
rounds = 3
next = { ok = "merge", ci_red = "impl", conflict = "impl:update" }
```

- rev's PASS spawns a `sec` lane on the same head.
- `drover merge` needs a PASS from rev **and** sec at the PR's head (`STALE_VERDICT` names the missing one).
- A FAIL in either check goes to a fix round in impl. The new head is reviewed from rev again.
- Each check counts its own rounds: sec parks on its 2nd FAIL, whatever rev has used.

## Roles

A role is any name under `[fleet.<n>.role.<name>]` (letters, digits, `-`). Its lane gets its kind's prompt
(`roles/impl.md` for a build, `roles/rev.md` for a check), then its optional `prompt = "<file>"`. A check role's
profile has the reviewer's read-only hook. `design` stays owner-only and is never a stage.

## Validation

The first stage is the start. Each rule names the stage and its `fleet.toml` line:

| rule | check |
|---|---|
| `start` | the first stage is a `build` |
| `name` / `kind` | names are unique and not `merge`/`park`; the kind is `build`, `check` or `gate` |
| `unreachable` | every stage is reachable from the start |
| `no end` | every stage has a path to `merge` or `park` (out of rounds does not count) |
| `no limit` | every loop passes a check with `rounds` |
| `unmapped` | every outcome of the kind is mapped, to a stage that exists. `push` goes to a check, `PASS` to a check, gate or merge, `ok` to merge, and `FAIL`/`ci_red`/`conflict` to a build or park. |
| `no role` | every build and check names a role that is defined (and not `design`) |
| `self-review` | a check's role is not a build stage's role |
| `no gate` | every path to `merge` passes a gate |

A broken pipeline stops only its fleet. `drive` skips it, and `spawn`, `verdict`, `merge` and `set` refuse
`BAD_PIPELINE` with every rule it breaks. Lanes already running keep working, and the other fleets run.
`drover doctor` lists the errors. The board shows them in the Pipeline group of the fleet's `[config]` sheet, with a red line on its
Queue card.

## Events

`spawn` events carry `stage` and `check`, and `verdict` events carry `stage` (the stage that gave it) and `loop` (the
check whose rounds it counts). Events from before #173 fold as before: `rev` is the check and `impl` the build.

## Autonomy numbers (#962)

`drover stats autonomy <fleet> [--since YYYY-MM-DD] [--weeks N] [--json]` reads the whole event log, archived
events included, in one pass. It prints one row per week (weeks start on Monday) and a total over the window.
`--since` starts the window at that date, and `--weeks N` keeps the last N weeks, this one included. Given both,
the later start wins. `--json` prints the same numbers. `--explain` prints the owner-action table.

- **Owner action.** One row of the table in `crates/core/src/autonomy.rs` (`OWNER`):
  - an ask answered by the owner;
  - every `needs-human`;
  - a run card approved;
  - a manual `spawn`, `verdict` or `kill`;
  - an extra round;
  - a park or an unpark;
  - a `--force` override;
  - an `--owner-override` merge.

  "By the owner" means the event's `by` is set and is not `orchestrator-*`, `default`, `drive`, `lane` or
  `external`, and no orchestrator lane logged the event. Since #962, `spawn` (from the CLI), `verdict` and
  `lane-killed` events carry `by`: `drive` inside a drive pass, `orchestrator-<id>`, `lane`, else `owner`. Older
  spawns and verdicts carry no `by` and never count. An older `lane-killed` always counts, because only
  `drover kill` logs one.
- **Merged with no owner action.** An issue whose `merged` falls in the week and whose events up to that merge hold
  no owner action. Run-card approvals belong to a thread, not an issue, so they count in the week's owner actions
  but never break one issue's autonomy.
- **Owner actions per merge.** Every owner action logged in the week, by kind, divided by the week's merges.
- **Asks.** The `needs-decision` events of the week, by who answered them: `owner`, `orchestrator` (an
  `orchestrator-*` answer), `default` (a routine ask's timeout) or `open`.
- **Rounds.** Per merged issue, the highest `round` of its verdicts. The row gives the median and the count that
  reached round 3. A pushback is a `fix-done` (`drover done --no-change`) on a merged issue.
- **Respawns and restarts.** `respawn` events (drive restarts a lane that ended) and `supersede` events (a live
  lane replaced) on merged issues.
- **Queue to merge.** From the first `queue add` naming the issue (else its first event) to its merge, as a median
  and p90. Owner wait is the part of it spent on an owner-answered ask (ask to answer), on `needs-human`, or
  parked (until the next extend, unpark, spawn, decision, merge or close). It shows as a total and as a share of
  the summed queue-to-merge time.
- **Closed unmerged, parked.** `closed` and `parked` events of issues that never merged, in the week they happened.

## The board

The Pipeline group of the fleet's `[config]` sheet draws the pipeline as a node graph. `serve` sends the pipeline as
data (`data-pipe`: stages, outcomes, rounds, `n here` counts, broken rules), and assets/serve.js (removed in #927) draws it.
The graph shows each stage's kind, role and `n here`, the loops back with their round limits, and the rail to park.
It is read-only: edit `fleet.toml`. Each issue card starts with its chain (`○ ─ ▲ impl ─ ◆ rev ─ ◫ gates ─ ✓ merge`),
with the current stage lit and a check that PASSed at the head ticked. The mockup is in docs/design/pipeline/ on
branch `design-173`.
