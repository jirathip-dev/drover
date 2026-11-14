# The look sheet: one `[◐]` on every tab (#290, phase 1)

The owner asked for two things (2026-11-03): the colours picker is missing on the Queue tab, and could the board have its
own picker instead of the iOS wheel? This design merges `[◐]` theme and `[▦]` colours into one header control, `[◐]`,
on all 4 tabs. It opens a **look** sheet: the board's own bottom sheet, the same box as the lane chat, `[?]` and
`[config]`, with two lists:

- **THEME:** farm, kitchen, terminal. Each row shows the theme's 24 px row sprite and its name, and the current one is
  ticked.
- **COLOURS:** every scheme that passes the #244 gate. Each row shows a 5-swatch strip (bg, fg, green, amber, red) and the
  name. The current one is ticked, and the current theme's default says `(theme default)`.

A tap applies the row at once, in place. The sheet stays open, and its backdrop is light, so the board behind it shows
the change.

## The shots (all from the real board)

`shoot.mjs` takes the demo export and builds a copy the way phase 2 will: `serve.rs`'s header swap, then
`serve.css.diff` and `serve.js.diff` (generated from `look.css` and `look.js`). It shoots that copy with headless Chrome
and measures every shot (`report.json`). The demo data is the committed synthetic `demo/`, unchanged. The only framing:
the demo banner and needs-you cards are removed, so the board fills each shot.

| file | what |
|---|---|
| `headers-390.png` | The header on each tab at 390 px, today (left) and proposed (right), plus the Queue header offline (its fullest case) |
| `look-390.png` | Queue tab, `[◐]` tapped: farm with its default colours, both ticked, and `(theme default)` on farm |
| `look-kitchen-390.png` | Then **kitchen** tapped: the whole board and the sheet are recoloured in place, and the colours follow the theme (kitchen ✓, default) |
| `look-paper-390.png` | Then **terminal**, then **paper** tapped: a picked scheme (light) beats the theme's default, which stays marked on github-dark |
| `look-1440.png` | 1440 × 900, opened from the keyboard (`[◐]` focused, Enter): the two lists side by side, sheet at half height, focus ring on the current theme |
| `look-keys-1440.png` | Then Tab, ↓, ↓, Enter, ↓: gruvbox is applied and ticked, and the focus ring has moved on to kitchen |

Regenerate with:

```sh
DROVER_HOME=<demo copy> FLEET_CONFIG=<demo copy>/fleet.toml FLEET_STATE=<demo copy>/events.jsonl \
  ./target/release/drover serve --export <site> --demo && scripts/host-fixtures.sh <site>
node docs/design/pickers/shoot.mjs <site> <chrome-headless-shell>
```

## Gate 2, measured on this mockup (`report.json`)

- **`[◐]` on all 4 tabs at 390:** it is visible on Queue, Lanes, Host and Events, and every header stays one line (48 px,
  last control's right edge at 378 = 390 − 12 padding). The Queue header holds `[?] [↻] [🐎] [◐]`, offline too.
- **The board's own audit** (`ui-audit.js` `measure`, #63) on each tab with the new header: **0 outliers** (queue 31
  controls, lanes 21, host 21, events 26). `[◐]` is a 28 px control with its 44 px ring like the rest.
- **The sheet with both sections:** each tab opens two `role=listbox` lists (`theme` 3 options, `colours` 8), each
  with exactly one `aria-selected=true` option. Every row is ≥ 44 px, the text is only 11 or 13 px, and nothing passes
  the right edge (0 outliers on all 4 tabs and at 1440).
- **No native select:** `select#theme` and `select#scheme` count 0 in every shot. The `.pk` select style stays,
  because `[config]`'s role pickers still use it.
- **No reload flash:** every switch (kitchen, terminal, paper, gruvbox by keyboard) keeps a `window.kept` marker set
  before it, so the page never reloaded, and the sheet is still open after it. The theme and colours repaint in place
  (#206/#244: `html[data-theme]`, the `#sch` token block, `theme()`).
- **Keyboard (1440):** Enter on `[◐]` opens the sheet with focus on the current theme. Tab moves to COLOURS, which
  lands on its current row (roving `tabindex`). ↑/↓/Home/End move within a list, and Enter or Space applies. Esc
  closes it (the existing sheet handler), and focus is back on `[◐]` (measured). A tap opens it without moving focus, so phones get no
  focus ring.

## What phase 2 changes

- **`src/serve.rs` (net −4 lines):** the header's two `<label class=pk>` selects become one button, and the `opts`
  list goes. The theme names still reach the page as `SD.t`'s keys, the same data source as today.
  ```rust
  -        let opts: String = names.iter().map(|n| format!("<option>{}</option>", esc(n))).collect();
  -        let pick = format!(
  -            "<label class=pk id=thp><span>◐</span><select id=theme aria-label=theme>{opts}</select></label>\
  -             <label class=pk id=scp><span>▦</span><select id=scheme aria-label=colours></select></label>"
  -        );
  +        let pick = "<button id=look type=button aria-haspopup=dialog aria-label=\"look: theme and colours\">◐</button>";
  ```
- **`assets/serve.css`** (`serve.css.diff`): the Queue-tab hide of `#thp,#scp` goes, `#look` takes `#thp`'s margin,
  and `look.css` is added (the rows, art, swatches, light backdrop, side by side ≥ 720 px). Only tokens are used, so the
  "no raw colour" lint holds.
- **`assets/serve.js`** (`serve.js.diff`): both picker blocks become `look.js`, built on the same `SD`, `choose`, `fails`,
  `block`, `put`/`get`, `theme()` and `colours()`. The sheet is the shared one (`show`, `close`, `.sh.lg` hides the lane
  parts, as `[?]` does). Also two lines in `sync()` (Q5) and one in `close()`.
- **`assets/ui-audit.js`**: `theme-switch` taps the sheet rows instead of setting `thp.value`. A new `look` view (roots:
  `#sheet`) opens the sheet and runs `measure` on it, and `scripts/ui-audit.sh` adds `look` and `look~kitchen` to its list,
  so CI shoots `ui-390/look.png`: the sheet fixture gate 2 asks for.
- **Docs:** `docs/themes.md` "Picking one" (the `[◐]` sheet, not a picker), and `docs/ui.md` if it lists the header.

Size: serve.css +28 −2, serve.js +90 −20, so assets JS/CSS go from 3,031 to about 3,127 lines. That budget is
warn-only and already past its 1,500. `src/` is net −4.

The patched page loaded all 4 tabs, every switch and the keyboard run with no script errors (`report.json` `errors: []`).

## Decisions made here (each is an open question below)

The CSS class `.lk` (Queue, `serve.css:471`) and herd.js's top-level `const look` were already taken. So the button is
`#look`, the sheet is `.sh.look`, the rows are `.lko`, and the functions are `lsheet`/`lpick`/`lrow`/`lsprite`.

## Open questions for the owner

1. **"Assets only" vs one line in `serve.rs`.** The selects are written by `serve.rs:1326`, so removing them needs that
   edit (net −4, above). Keeping `src/` untouched would mean serve.js swapping them after load: the selects would flash
   in the header for a frame, and they would still be in the HTML. **Recommend: the serve.rs edit.**
2. **A light backdrop for this sheet** (the scrim at 35 %, as shot) so the board behind it shows each pick. Every
   other sheet keeps the full scrim. **Recommend: as shot.**
3. **Picking the `(theme default)` row forgets the colours pick**, so the colours follow the theme again on the next
   theme switch. Today, once a phone picks a scheme it can never get back to "follow the theme". The alternative is an
   explicit `follow theme` row. **Recommend: as built (no extra row).**
4. **The home-screen-icon toast becomes a permanent note** at the sheet's foot ("this phone only · the home-screen
   icon keeps its install-time look"). The toast sits under the header at z-index 2, so the sheet (z 3) would cover it.
   **Recommend: the note.**
5. **The offline note becomes `as of 14:05`** (its title stays `last update 14:05`). Offline, the live Queue header is
   at its fullest: `drover`, the note, the dot and four controls. `last update 14:05` plus `[◐]` wrapped the title one
   letter per line. In the demo it already wraps to 3 lines today (`headers-390.png`, bottom left, shot without the
   demo tag). Proposed: the title never wraps, the shorter note fits whole (measured: not cut), and it ellipsises only as
   a last resort. This is outside the issue's letter, but the AC's "Queue header fits at 390" needs it.
   **Recommend: accept.**
6. **The theme row's art** is the theme's `impl` row sprite (farm's plough horse, kitchen's cook). A theme without row
   sprites (`terminal`) shows its favicon tile (`>_`), and one with neither shows the board icon. The sheet fetches each
   theme's `theme.json` and `sprites.svg` once, on first open (the service worker caches them). **OK?**
7. **Arrows move focus; only Enter/Space applies** (no selection-follows-focus), because each theme switch refetches
   sprites and rebuilds the Herd. A tap applies at once. **Recommend: as built.**
8. **1440:** the two lists sit side by side (each at most 420 px), and the sheet opens at half height so the board's
   top half stays visible for the preview. On a phone it opens at the mid detent (78 %), with the header and the first
   Queue rows still visible above it. **OK?**
