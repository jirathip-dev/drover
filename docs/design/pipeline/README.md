# Pipeline graph: design mockup (#173, phase 1)

`index.html` draws the fleet's pipeline as a node graph in #162's `[config]` sheet, plus where each issue is in its
own card. It links `assets/serve.css` unchanged. One `<style>` block holds #162's rules (design-162, not merged yet)
and the new #173 rules. The new rules use existing tokens only and move to `serve.css` in phase 2. The page is static.
The hash picks the view, and a 5-line script opens the TOML source beside the graph when the window is ≥ 900 px wide.

| view | open | screenshots (headless Chrome, full page; 390 at DPR 3) |
|---|---|---|
| the default pipeline in herdsman's `[config]` sheet | `index.html` | `default-390.png`, `default-1440.png` |
| a custom pipeline: impl → rev → sec → gates → merge | `index.html#custom` | `custom-390.png`, `custom-1440.png` |
| a refused pipeline, as the board shows it | `index.html#error` | `error-390.png`, `error-1440.png` |
| the issue card's "where it is now" line | `index.html#card` | `card-390.png`, `card-1440.png` |

**Data:** the default view and herdsman's cards come from `herdsman board`, `herdsman pr`, the event log and
`fleet.toml`, read on 2026-11-02 at 09:05 (+07). The real data includes the role models (impl opus-5-5, rev sonnet-5-5,
effort unset), the rounds (review 3, update 3 by default), the per-stage counts (6 in impl, 1 in rev, 1 at gates,
7 queued), and #141, #149, #165 and #168 with their states, PRs, verdict rounds and shas.
**Illustrative:** everything on `atlas` (#162's neutral name for the private app fleet). That covers its `sec`
role and stage, its counts, its two issues' stages, and the broken pipeline in `#error`.

## The graph

```
   ○ queued  7 waiting
   │ start if unblocked · serial slot · host room
  ┌▲ impl  build ─────────────── 6 here ┐◂──────┐   ← header: kind glyph, name, kind, how many issues are here
  │ role impl · claude · opus-5-5 · …   │       │   ← role line, 11 px
  └─────────────────────────────────────┘       │
   │ push  a new head                           │   ← the spine: the main outcome, straight down
  ┌◆ rev  check ──────────────── 1 here ┐       │
  │ role rev · claude · sonnet-5-5 · …  │       │
  │ FAIL → fix in impl · ≤3 rounds ─────┼───────┤   ← an outcome that loops: a leader out to the loop rail
┌┄┼┄ 3rd FAIL → park                    │       │   ← out of rounds: dashed red, out to the park rail
┆ └─────────────────────────────────────┘       │
┆  │ PASS  at the exact head                    │
┆ ┌◫ gates  gate · herdsman ──── 1 here ┐       │
┆ │ CI green · PASS at head · serial …  │       │
┆ │ CI red → fix in impl ───────────────┼───────┤
┆ │ conflict → update in impl · ≤3 ─────┼───────┘
├┄┼┄ 4th conflict → park                │
┆ └─────────────────────────────────────┘
┆  │ ok
┆ ┌✓ merge  into staging ───────────────┐
┆ └─────────────────────────────────────┘
└▸┌! park  needs you ─────────── 0 here ┐
  └─────────────────────────────────────┘
    on its card: one more round, or park for good
```

- **Kinds** have a fixed glyph and colour: `▲` build (pushes code, amber), `◆` check (PASS or FAIL, purple), `◫` gate
  (herdsman's own, cyan), `✓` merge (green) and `!` park (red, the board's "needs you" colour). A legend line sits
  under the graph.
- **Outcome words** take the colours they already have on the board: `PASS`/`ok` green, `FAIL`/`CI red` red,
  `conflict` amber, `push` plain.
- **The spine** is each stage's main outcome (`push`, `PASS`, `ok`) when it leads to the next stage in file order. It
  is drawn straight down, with the word beside the arrow.
- **Every other outcome is a line inside its box** (`FAIL → fix in impl · ≤3 rounds`). A leader runs from it to the
  box edge, and a rail picks it up there:
  - **Right rails** loop back. Each target gets one rail, and every outcome that goes there joins it. The arrowhead
    `◂` enters the target's header. Rails are grey, and the label is in the box, so the rail itself carries no text.
  - **The left rail** goes to `park`. Every loop with a limit has an "out of rounds" line (`3rd FAIL → park`,
    `4th conflict → park`), drawn dashed and red down to `park`.
- **Round limits** are on the loop's own line (`≤3 rounds`, `≤3`), so a shared rail never has to say whose limit it
  is.
- **`n here`** counts the issues whose current stage this is, from the fold that already exists. Park counts
  needs-human issues.
- **Read-only:** the graph has no controls. Edits happen in `fleet.toml`. The source sits beside the graph at 1440, and
  under it, collapsed, at 390.

### Layout (what phase 2 implements)

It is deterministic, with no layout engine. It is a CSS grid where every text line is one row, `--ln` tall (24 px =
`--s4 + --s2`). So every rail meets a line exactly at its middle.

1. Order: `queued`, then the stages in file order, then `merge`, a blank row, then `park`.
2. Box lines: header, role line, any error lines (`✗ …`), one line per non-spine outcome, and one "out of rounds"
   line for a stage with `rounds`.
3. Columns: `2ch` (the park rail), `minmax(0,1fr)` (boxes), then one `2ch` column per right-rail target, the
   shortest span innermost. So `grid-template-columns` and each item's `grid-row`/`grid-area` are written inline, with
   no px.
4. Elements: `.nb` box lines (`.t` top, `.b` bottom), `.pl` spine rows, `.ex` a horizontal (`.in` adds the arrowhead),
   `.ey` a rail (middle of its first row to middle of its last), and `.ek` a gap where a horizontal crosses an inner
   rail. `#error` shows one crossing: qa's `FAIL → impl` hops over the qa rail.
5. Every line is one line: it ellipsises, and its full text is in `title`. The graph is vertical at every width, so
   nothing scrolls sideways. At 390 the box column is about 40 characters wide, with 2 rails.

## The custom example (`#custom`)

`impl → rev → sec → gates → merge`. The two checks each FAIL back to impl (rev ≤3, sec ≤2), and each has its own
"out of rounds" line to park. The gate's line reads `PASS from rev + sec at head`. The status line above the graph
says `merge needs a PASS from rev and sec at the same head`.

- `sec` is a new role (`[fleet.atlas.role.sec]`). It appears in the ROLES group with the same `set` form, tagged
  `check: sec`.
- A role's tag names the stages that use it, which replaces today's fixed `ROLES` list. `design` stays
  conductor-only and is never a stage.

## A refused pipeline (`#error`)

- **On the Queue tab,** the fleet card gets a red line: `! BAD_PIPELINE · 4 errors in fleet.toml · nothing moves
  until it is fixed`. The fleet's need-you count includes it, its drive dot is red (`drive refused`) and
  `[drive now]` is disabled.
- **In the sheet,** the PIPELINE header's hint turns red (`BAD_PIPELINE · 4 errors`). A red box says what stops and
  what doesn't. Then comes one line per broken rule: its rule word in red, a plain sentence, and `fleet.toml:<line>`.
- **The graph is still drawn as written.** A broken stage has a red edge with its `✗` reasons inside. An unreachable
  one (`gates`, and `merge` after it) is dashed and dimmed. A missing spine says `✗ nothing leads to gates`.
- **The example:** rev's PASS was pointed at a new `qa` check instead of `gates`. `qa` has no `rounds`, and `docs`
  uses an undefined `writer` role. That gives all four of the brief's error types on one realistic edit:

```
$ herdsman drive
REFUSED BAD_PIPELINE: fleet atlas: 4 errors in [fleet.atlas.pipeline]
  unreachable  stage gates can't be reached: no outcome leads to it (fleet.toml:69)
  no end       stage qa can't reach an end (nor can impl, rev, docs): every path comes back to qa (fleet.toml:57)
  no limit     loop qa → docs → qa has no round limit: set rounds on qa (fleet.toml:61)
  no role      stage docs: role writer is not defined; add [fleet.atlas.role.writer] (fleet.toml:66)
```

### The rules, as the board words them

| rule word | check | sentence |
|---|---|---|
| `start` | the first stage is the start and is a `build` | `the first stage, X, is a check: a queued issue needs code first` |
| `unreachable` | every stage is reachable from the start | `stage X can't be reached: no outcome leads to it` |
| `no end` | every stage has a path to `merge` or `park` | `stage X can't reach an end: every path comes back to X` |
| `no limit` | every cycle passes a stage with `rounds` | `loop A → B → A has no round limit: set rounds on A` |
| `unmapped` | build: `push`; check: `PASS`, `FAIL`; gate: `ok`, `ci_red`, `conflict` | `check X has no FAIL outcome` |
| `no role` | every stage's role exists under `[fleet.<n>.role.*]` | `stage X: role R is not defined; add [fleet.<n>.role.R]` |
| `self-review` | a check's role differs from the build roles on its path | `check X uses role impl, which also builds: a check can't review its own work` |
| `no gate` (Q4) | every path to `merge` passes a gate | `rev's PASS goes straight to merge: route it through a gate` |

## The issue card: where it is now (`#card`)

The open row's card (`.cd`, #115) gets one line on top: `○ ─ ▲ impl ─ ◆ rev ─ ◫ gates ─ ✓ merge`.

- The current stage is lit: a 1 px border in its kind's colour, on a 16 % tint of it.
- Stages before it are `--fg`. A check that has a PASS at the current head is ticked `✓`.
- Later stages are `--mute`.
- The lit chip carries the mode or round when it has one (`impl · update`, `rev · r1`, `sec · r1`, `impl · fix`).
- The line wraps at 390 and never scrolls.
- Under it, one 11 px line says what the item waits on, in the words the board already uses.

Real examples from 09:05:

| issue | lit | line under it |
|---|---|---|
| #168 | `impl` | `building · not pushed yet` |
| #165 | `rev · r1` | `review round 1 of 3 · waiting for its verdict` |
| #141 | `gates`, rev ✓ | `rev PASS r1 at the head · waiting for CI` |
| #149 (`fix(1)+u1`) | `impl · update`, rev **not** ticked | `update round 1 of 3 · rev PASS r1 was for 0d1357f; the merged head gets a new review` |

On atlas, `#2909` sits at `sec · r1` with rev ticked, and `#2911` is in `impl · fix` after `sec FAIL r1`.

## The rules (#63), measured on the mockup

At 390 and 1440, on all four views, with tap rings off as `ui-audit` measures:

- the page's `scrollWidth` equals the viewport;
- no element passes either edge;
- the only text sizes are 13 and 11 px;
- every control is 28 px tall.

The only overflow left is #162's hidden `<select>` under `[opus ▾]`, which is its 44 px tap area by design.
Colours come from the existing tokens, plus `#161b22`/`#30363d`, which `serve.css` already uses for boxes. No new
token.

## Phase 2 notes

- **Rendering:**
  - `serve.rs` gets `fn pipeline_graph(&Pipeline, counts) -> String`, which is the layout above at about 70 lines.
  - The card line comes from the fold's `stage` (≈ 15 lines).
  - The rules above go to `serve.css`. There is no JS beyond the one `matchMedia` that opens the source on wide
    screens.
- **State words:** `review(n)` stays for the default `rev`. Other check stages show `<stage>(n)` (`sec(1)` in
  `#card`), and their title says what it means. Old events fold as before (`impl`/`rev`, no `stage` field).
- **Gate 2:** a 390 fixture for the default and the custom example. `ui-audit` can assert that every `.ey` starts and
  ends on a row middle, which it does by construction. It can also assert that no `.nb` line wraps (`scrollHeight ≤
  --ln`).
- **`ci_red` with 2 checks:** today a red CI after a PASS is a FAIL at the same head, so no new round starts, and the
  fix's re-review starts one. With 2 checks, that FAIL belongs to the gate, not to either check. Each check counts
  only its own reviews. "Red CI in the last round goes to park at once" stays as today, which the golden test must
  pin.

## Open questions for the owner

1. **The update loop's limit:** today it is `[limits] update_rounds`, which is global. The mockup puts it on the gate
   (`rounds = 3` there limits `conflict → update`), so every loop's limit lives in the pipeline. The global value
   becomes the default for the built-in pipeline. **Recommend: as mocked.**
2. **Out of rounds always goes to `park`,** implicitly, so it is not a configurable outcome. That keeps the config to
   one `rounds` number per loop. **Recommend: implicit.**
3. **The start:** the first stage, and it must be a `build`. The alternative is an explicit
   `[fleet.<n>.pipeline] start = "impl"`. **Recommend: the first stage.** It is one less key, and "exactly one start"
   becomes true by construction.
4. **A rule the brief doesn't list: every path to `merge` passes a gate.** `herdsman merge` checks CI and PASS at the
   head in code anyway, but without a gate stage `ci_red`/`conflict` have nowhere to route. **Recommend: add it** as
   the 8th rule (`no gate`).
5. **`BAD_PIPELINE` scope:** the brief says "every command". The mockup refuses per fleet: atlas stops, herdsman
   keeps running. `serve` still renders, because it has to show the error, and `doctor` lists the errors.
   **Recommend: per fleet.**
6. **A new role's prompt:** its kind's built-in prompt (`impl.md` for build, `rev.md` for check), plus an optional
   `prompt = "<file>"` per role, appended to it (the `sec` example). **Recommend: as mocked.** No new dependency.
7. **`n here` counts on the boxes** are not in the brief. They are cheap (the fold already has the state) and answer
   "where is everything" at a glance. Keep them, or drop them?
8. **LIMITS rows:** `review rounds` and `update rounds` stay, as the built-in pipeline's defaults, with a pointer
   `→ rev's rounds, see Pipeline`. Once a fleet has its own `[pipeline]`, they could show `n/a (pipeline)` instead.
   **Recommend: as mocked.**
9. **Depends on #162:** this group lives in #162's `[config]` sheet, which is designed but not merged. If #162 slips,
   the graph can sit in today's `roles` drawer for one release.
