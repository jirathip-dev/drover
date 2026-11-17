# Boundaries and state

## Who calls whom

| caller | may call | never |
|---|---|---|
| `timer::tick` | `hotspots::daily` | `find` or `gh` itself |
| `hotspots::daily`, `hotspots::run` | `store`, then `file` when the week is up | — |
| `hotspots::store` | `find`, the json file | GitHub |
| `hotspots::file` | `gh` (the issue), `queue_change` | git |
| `hotspots::find`, `hotspots::sync` | `runner` (git), `arch::decided`, the events | read `shared`, `frozen` or `generated` themselves |
| `queue::serials` | the last `hot-files` event | arch.toml, the json file |
| `arch::check` | `generate` (`just gen`), the baseline, the `shared` cover check | know what the generator builds |
| any emitter of an alert | `Event::new(..).push()`, `check::fire_event` | `events::push::LEGACY` |
| `Notify::wants`, `check::fire_event` | `Event::is_push` | the list |
| `usage_doc` | the joined text from `OUT_DIR` | a file under `roles/` by name |
| core's build script | the filesystem under `roles/` | git beyond today's sha, the network |
| a gate script (follow-up A) | the built binary, `just`, `cargo` | another gate's output; setup (the workflow's) |

Core gains no layer edge: everything above but the gate scripts is inside `crates/core`.

## A file on the list, as a statechart

```text
  [quiet]  under the bar
     │  counted merges (≤ 40 files each) reach max(5, merges / 20)
     ▼
  [hot]  in hotspots-<fleet>.json; drive logs `hot-files`; a one-slot serial group `hot <file>`
     │                                │                               │
     │ churn ends                     │ the week's ADR lands          │ the week's ADR lands
     │ (next daily store)             │ frozen or generated           │ shared (needs a [[size]] rule)
     ▼                                ▼                               ▼
  [quiet]                          [converted]                     [shared]
                                   off the list, no group;         off the list, no group;
                                   arch-check holds the form       arch-check holds the size
```

The weekly issue lists `[hot]` only: files nobody has decided. A sweep moves nothing: it is not counted.

## The stored list, as a statechart

```text
  tick ──▶ .day older than 1 day? ──no──▶ nothing
                │ yes
                ▼
            store: find, write the json, stamp .day ──▶ drive's next pass: sync logs `hot-files` if it changed
                │
                ▼
            .at older than 7 days and any hot file? ──no──▶ nothing
                │ yes
                ▼
            file: open or update `hotspots: <fleet> week of <monday>`, queue --architect, stamp .at
```

## A generated file in a PR, as a statechart

```text
  [source changed] ──▶ lane runs `just gen`, commits the output ──▶ arch-check: rerun, same bytes ──▶ pass
  [source changed, output not rebuilt] ──▶ arch-check: differs ──▶ fail, names the file and the command
  [integration moved, the file conflicts] ──▶ update round: take either side, run `just gen`, commit ──▶ pass
```

## A pushable alert, as a statechart

```text
  [new kind]  emitted with .push()   ──▶ logged with "push": true ──▶ a phone with no `on` gets it; counts in the pass budget
  [old kind]  one of the 44          ──▶ no field                 ──▶ LEGACY names it: pushed the same way
  [a notifier with `on`]             ──▶ its own list decides, as today; the flag is not read
```

`LEGACY` only shrinks: adding a kind fails arch-check (`frozen`).
