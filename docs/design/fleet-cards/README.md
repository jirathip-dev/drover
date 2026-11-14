# Fleet cards and the [config] sheet: design mockup (#162, phase 1)

`index.html` is the Queue tab with each fleet as its own card, plus one fleet's `[config]` sheet (open
`index.html#config`, or tap any `[config]`). It links `assets/serve.css` unchanged. One `<style>` block holds the new
rules, which use existing tokens only and move to `serve.css` in phase 2. The row markup is exactly what `serve.rs`
plus `serve.js` `graph()`/`tidy()` produce today, so the item rows are #115's graph rows, unchanged.

Screenshots (headless Chrome, full page): `fleet-cards-390.png` (DPR 3), `fleet-cards-1440.png`,
`config-sheet-390.png` and `config-sheet-1440.png`.

**Data:** read from `drover board`/`queue ls`/`status`/`asks` and `fleet.toml` on 2026-11-02 at 07:30. Fleet
names are neutralised: `atlas` is a private app fleet and `relay` is a private paused fleet; `drover` is this
repo. The issue numbers, states, ages, queue order, blockers, serial marks, round counts, verdicts, lane counts,
`done today` counts and every config value are real, with paths and MCP names masked (`db/migrations/**`, `db-dev`).
These parts are illustrative: atlas's and relay's issue titles, the PR numbers in the `done today` lists, and the
drive age (`3m ago`).

## The cards

```
┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓  ← 3 px bar in the fleet colour
┃ atlas ● drive · 3m ago  ! 3 need you   ┃  ← name in the fleet colour; drive dot + age; red count
┃ [drive now] [pause] [config]           ┃  ← wraps under the name at 390, right-aligned at 1440
┃ stall check: #2910 in 7m · 8 lanes · serial 3/3   ← one 11 px line (today's two <p class=m> lines)
┃ ● #2898 impl …                          ┃  ← #115 graph rows, unchanged
┃ ▸ done today: 16                        ┃  ← collapsed, now with a chevron
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┌ ▸ relay · paused · 1 lane     [resume] ┐  ← paused: one grey line, muted bar; a tap peeks its rows
```

- **Order:** fleets that need you first (any needs-human item or open ask), then by name. A paused fleet that needs
  you also goes first, still collapsed, with its red count on the grey line.
- **Need-you count:** needs-human items plus open asks for that fleet (atlas: #2684, #2782 and the #2910 decision).
- **Drive dot:** green when the last `drive` pass is within 10 minutes, amber when it is older, grey for paused
  fleets. The text shows `drive · <age>`.
- **Colour:** `sha256(name)[0] % 5` over `[--l, --p, --c, --g, --a]`, the same hash as the horse coats
  (`coat()`). Red is left out because it means "needs you" on the board. `[fleet.<n>] color = "l"|"p"|"c"|"g"|"a"`
  overrides it, and any other value is a config error. Today's names give atlas amber, drover blue and relay amber
  (relay shows grey while it is paused). With 5 colours, two fleets collide about 1 time in 5, which is what
  the override is for.
- **The old `roles` row** is gone. Its forms move into the `[config]` sheet.

## The [config] sheet

It is the lane sheet's box (#70 detents, drag and backdrop unchanged) with the fleet's bar on top, loaded from
`frag?id=config&fleet=…`. Each group has a header in 11 px caps with a muted hint on the right, then key/value rows
(key 11 px, value 13 px) that wrap and never scroll sideways.

| group | rows | editable |
|---|---|---|
| GENERAL | repo, local, integration, driver, default effort, colour, env, node_modules | none; `edit fleet.toml` |
| LIMITS | review/update rounds, stall, ask default, max lanes (impl/rev), line cap, serial paths, serial max + label, required checks | none; `edit fleet.toml` (see Q1) |
| ROLES | per role: `[harness ▾] [model ▾] [effort ▾] [set]`, then profile name, accounts and mcp as text | harness, model, effort, through the existing `set` op |
| ALERTS | the fleet's notify, or "global only"; the global notifier; its events | none; `edit fleet.toml` |
| HOSTS / NEEDS | a dashed placeholder, "after #157" | none |

- Rows that come from global `[limits]` carry an `all fleets` tag, so an edit there is not mistaken for a
  per-fleet one.
- **Harness picker:** `crate::set` already writes `harness`. The web `set` op only passes `model`/`effort` today, so
  phase 2 adds `harness` to that list. That adds no new key, validation or event. When the harness changes, the
  effort list follows `Provider::efforts` (dsh: `none`).
- Values that fall back to a default say so (`3 (default)`, `unset → none`).

## The rules, measured on the mockup

At 390 and 1440 px, on both the Queue tab and the sheet: no sideways scroll and nothing past either edge. Text is only
11 or 13 px. Every control is 28 px tall with a ≥ 44 px tap target (13 controls on the Queue view, 16 on the sheet).
The bar colours come only from the existing accent tokens.

## Open questions for the owner

1. **"Limits editable" conflicts with "no new writable keys".** Your Q2 lists limits as editable, but no `set`
   path exists for them today. `drover set` only writes role keys (harness/model/profile/effort), and the web only
   writes model/effort. Also, rounds, stall and ask-default are **global** `[limits]` for all fleets, not per fleet,
   and the line cap lives in the repo's `ci/cap.toml` (behind the owner-OK rule). The mockup shows them read-only.
   **Recommend: read-only now.** If you want them editable, that is a separate issue: a new drover set --limit flag
   with its own validation and event.
2. **`models` is unset in your `fleet.toml`.** So the web model picker offers only the current value, and any
   change is refused `BAD_VALUE` (`set` checks against `models`). The mockup shows the pickers as they would look
   with `models = ["claude-opus-5-5", "claude-sonnet-5-5"]`. **Recommend:** add that line (your edit; designers
   don't touch fleet config).
3. **Red left out of the colour list**, so a fleet bar never reads as "needs you". The list is then 5 colours
   (blue, purple, cyan, green, amber), and amber and green also mean states. **Recommend: as mocked.**
4. **Paused + needs you:** the mockup sorts it first but keeps it collapsed, with a red count on the grey line.
   Alternatively, it could expand automatically. **Recommend: as mocked.**
5. **1440:** the cards are full width, as the board is today. A two-column card grid on wide screens is possible,
   but it is not in the issue. Leave it?
6. **The red needs-you cards above every tab (`#needs`)** are left out of the screenshots so the cards are the
   subject. They stay as they are; the card count duplicates them on purpose, per fleet.
