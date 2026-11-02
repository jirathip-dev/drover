# Pipeline (#173)

A fleet's pipeline is data in `fleet.toml`: named stages, each of a fixed kind, and where each outcome goes.
herdsman validates it when it loads the config. `drive`, the gates and the board all follow it.
Without a `[fleet.<n>.pipeline]`, a fleet runs the built-in pipeline, which is the impl → rev loop from before #173.

## Kinds

| kind | what it is | its outcomes |
|---|---|---|
| `build` ▲ | a lane that pushes code (`roles/impl.md`) | `push` |
| `check` ◆ | a lane that gives PASS or FAIL at the head (`roles/rev.md`, read-only) | `PASS`, `FAIL` |
| `gate` ◫ | herdsman's own: CI, PASS at the head, serial, mergeable | `ok`, `ci_red`, `conflict` |
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

## A custom pipeline: two checks

```toml
[fleet.atlas.role.sec]
harness = "claude"
model = "claude-opus-5-5"
prompt = "~/.config/herdsman/prompts/sec.md"   # appended to the check prompt

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
- `herdsman merge` needs a PASS from rev **and** sec at the PR's head (`STALE_VERDICT` names the missing one).
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
`herdsman doctor` lists the errors. The board shows them in the fleet's `roles` drawer, with a red line on its Queue
card.

## Events

`spawn` events carry `stage` and `check`, and `verdict` events carry `stage` (the stage that gave it) and `loop` (the
check whose rounds it counts). Events from before #173 fold as before: `rev` is the check and `impl` the build.

## The board

The fleet's `roles` drawer draws the pipeline as a node graph. It moves to #162's `[config]` sheet when that lands.
The graph shows each stage's kind, role and `n here`, the loops back with their round limits, and the rail to park.
It is read-only: edit `fleet.toml`. Each issue card starts with its chain (`○ ─ ▲ impl ─ ◆ rev ─ ◫ gates ─ ✓ merge`),
with the current stage lit and a check that PASSed at the head ticked. The mockup is in `docs/design/pipeline/` on
branch `design-173`.
