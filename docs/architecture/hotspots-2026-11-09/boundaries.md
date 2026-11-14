# Boundaries and state

## Who calls whom

| caller | may call | never |
|---|---|---|
| `overlap::cap_check`, `overlap::cap_at`, `overlap::cap_view`, `check` (the `cap-near` alert), `serve`'s banner | `overlap::cap_at_sha` | read `ci/cap.toml` or the folder themselves |
| `overlap::cap_at_sha` | `runner` (git `ls-tree`, `show`), the filesystem for the working tree | events, config |
| `hook` | `CAP_PATHS` | know what a raise is: the exemption stays in `cli`'s policy build |
| `cli/<area>.rs` | its core module(s), `Ctx`, `Runner` | another area's file; `cli/mod.rs` is the only caller of an area's `run` |
| `cli/mod.rs` | every area's `run` | core modules directly, beyond what `main_with` needs |
| any emitter of an event | `Event::new(..).passive()` | `events::passive::LEGACY` (read by `Event::is_passive` only) |
| `events::fold`, `spawn`'s idle-pane reader | `Event::is_passive` | the list |
| `hotspots::find` | `runner` (git), `arch::converted`, the events | GitHub (only `run` does, as today) |
| `arch::check` | `generate` (`just types`), the baseline | — |

Core gains no layer edge; everything above is inside `crates/core`.

## The cap, as a statechart

```text
                 integration merges, lines grow
  [headroom] ───────────────────────────────▶ [near]  cap − lines < warn_within
      ▲                                          │  check: `cap-near` once per integration sha
      │                                          ▼
      │                                  [asked]  a lane's PR would cross: `drover ask --kind owner`
      │                                          │  orchestrator: one issue `cap: raise … to <N>`, queued on top;
      │                                          │  every asker blocked on it
      │                                          ▼
      │                                  [raising]  that lane adds ci/cap.d/<date>-<N>.toml (hook: only this lane)
      │                                          │  arch-check: a new file in the collected folder passes;
      │                                          │  a line added to ci/cap.toml fails (frozen)
      │                                          ▼
      └───────────────────────────────── [raised]  merged: cap_at_sha reads the new highest; blockers unblock
```

Two raises in flight: both files land; the highest wins; neither needs an update round.

## A passive kind, as a statechart

```text
  [new kind]  emitted with .passive()  ──▶  logged with "passive": true  ──▶  fold skips it, idle reader skips it
  [old kind]  logged before the flag   ──▶  no field (false on read)      ──▶  LEGACY names it: skipped the same way
  [old build on a remote host]  can only emit old kinds                   ──▶  LEGACY covers them
```

`LEGACY` only shrinks: a kind nobody logs any more (`drover events --jsonl` shows none in the retained log) may be
removed; adding one fails arch-check (`frozen`).

## The weekly report, as a statechart

```text
  tick (7 days) ──▶ find: merges at the tip ∖ converted (frozen, generated) ∖ not at the tip
                ──▶ ≥5 PRs? ──no──▶ "none", no issue
                      │yes
                      ▼
                open or update `hotspots: <fleet> week of <monday>`, queue --architect ──▶ this lane
```
