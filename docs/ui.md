# UI rules (#58, #63)

The phone page (`drover serve`) has one look. Two CI gates enforce it; a red gate blocks
`drover merge` (required checks, #24).

## Tokens

The first `:root` block of `web/src/board.css` (the page's CSS, #927) is the only place a raw px length may appear.

| token | value | use |
|---|---|---|
| `--fs` | 13px | all text |
| `--fs-meta` | 11px | secondary text: ages, reasons, captions |
| `--fs-input` | 16px | inputs, textareas, selects (iOS zooms below 16 px) |
| `--fs-code` | 32px | the pairing code's digits (#59) |
| `--ctl` | 28px | the visible height of every control |
| `--hit` | 44px | the minimum tap area |
| `--s1` … `--s4` | 4 / 8 / 12 / 16px | margins, paddings, gaps, offsets |

Other lengths are sums of tokens (`calc(var(--hit) + var(--s2))`); radii, borders and widths stay literal.

## Controls

- **Bracket style:** a control is its lowercase label in the accent colour between `[` and `]`
  (CSS pseudo-elements), 28 px tall, no fill; inverted while pressed. A select hides behind
  its `[opus ▾]` label.
- **Hit area:** the `]` pseudo-element spans an 8 px ring around the control, so the tap area is
  ≥ 44×44 px. Controls sit 16 px apart so rings never overlap. Rows (a screen's `row` blocks, a card's folded
  line) are full-width tap rows at least 44 px tall, not 28 px controls.
- **Choices (the one exception, owner-approved 2026-10-31):** ask and menu options are sentences:
  full width, wrapping, ≤ 4 lines then an ellipsis (full text in `title`), `[`/`]` on the first
  line, stacked 8 px apart, the recommended one first with an 11 px `(recommended)`.
- **Type scale:** text is 13 or 11 px, nothing else; inputs 16 px (iOS zooms below 16 px).
- **Labels, not sentences (#68):** a button holds its label (in a span that ellipsises, so `[`/`]` never
  overlap text). An action's reply line (the server's `toast`, or its refusal in red) is the 11 px toast under the
  header (#927). Unpaired, the header's `[pair]` opens the pairing panel (#59) in the flow under the header.
- **No text past the screen (#68):** every text box shrinks (`min-width:0`) and wraps anywhere (paths, shas,
  URLs, `code`); one-line cells (a row's subtitle, a card's folded line) ellipsise.
- Nothing scrolls sideways at 390 px.

## Gates

1. **Style lint** — `cargo test --test ui_tokens`: a sizing property (`font-size`, `font`,
   `height`, `min-height`, `line-height`, `margin*`, `padding*`, `gap`, `top/left/right/bottom`,
   `inset`) with a px value outside the token block, a font size that is not a `--fs` token, a control
   `height` that is not `--ctl`, or an inline `style=` with px in the rendered page fails (`file:line value`).
2. **Browser audit** — the `ui` CI job: headless Chrome opens the exported demo at 390×844, DPR 3. #927: the views
   are the screens the page draws: each tab by its place id (`queue`, `lanes`, `events`, #1072's standalone-only
   `threads`), `fleets` (the `[⚙]` screen's sheet), `lane` (a lane's sheet), `card` (the first card opened) and `thread`
   (the Threads tab's first thread), in kitchen, garden and terminal
   (`~<theme>`), at 1440 (`@1440`) and in every colour scheme (`queue^<scheme>`, the contact sheet), plus #725's
   `lifeboat/page`. `assets/ui-audit.js` (only in the `--demo` export, `?audit=<view>&check=1`) fails on a control not
   28 px tall (a choice: over 4 lines), text not 11/13 px (inputs 16), a tap area under 44 px, a box whose content is
   wider than it unless it ellipsises or scrolls by design, anything past the screen's right edge, sideways scroll, a
   row sprite not 24×24 or smoothed (#281), no screen drawn, a viewport meta other than
   `…,maximum-scale=1,user-scalable=no,viewport-fit=cover`, or `html` `touch-action` other than `manipulation`.
   Screenshots are uploaded as the `ui-390` artifact on every run.
   #266, #927: a look change is measured: shoot the views on staging and on the branch with
   `UI_AUDIT_VIEWS="queue lanes events threads thread" scripts/ui-audit.sh <site> <dir>` (one machine, one Chrome), then
   `scripts/pixel-diff.py <before> <after>` reports the share of each shot's pixels that differ (any channel by more
   than 8; over 0.1 % fails).
   A Chrome run that does not return in `UI_AUDIT_TIMEOUT` s (default 60; needs `timeout`) is killed and fails its view
   with its stderr tail (#71); the job itself stops at 20 min.

Run both locally:
```sh
cargo test --test ui_tokens
cargo build --release && DROVER_HOME=demo FLEET_CONFIG=demo/fleet.toml FLEET_STATE=demo/events.jsonl \
  ./target/release/drover serve --export site --demo && scripts/ui-audit.sh site ui-390
```
`scripts/ui-audit.sh` finds `google-chrome`/`chromium`; set `CHROME=<binary>` otherwise. #560: it and the WebKit
tests' `swift` run under `scripts/ui-lock.sh`, one audit at a time on the host (`~/.cache/drover/ui-audit.lock`,
run past once held 10 min; a dead holder lets go at once); a lane that waits logs `waiting for the UI audit lock (held by <lane>)`.

**Exceptions:** each is listed by name with its reason in `tests/ui_tokens.rs` (`EXCEPTIONS`). **Adding an
exception needs the owner's OK.**
