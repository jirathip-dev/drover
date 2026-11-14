# Themes (#172, #241, #244, #928)

A theme is one folder `themes/<name>/` that every tab of the phone page draws from: the Lanes avatars, a 24 px
sprite on each Queue and Events row, a load scene on each Host group, the Herd, the moments, the icon. It is data
only: `theme.json`, `sprites.svg`, and optionally `scene.svg` and its icons. No theme ships CSS or JS.

Built in: `farm`, `kitchen` and `garden`, the packs drover shares with cooee, and `terminal` (today's plain board,
drover's own). The packs' art, build scripts and theme gate (contrast, required sprites) live in the shared repo
[jirathip-dev/swag](https://github.com/jirathip-dev/swag): change a sprite there. Drover embeds its `legacy-board/`
folder at the commit `assets/swag/SOURCE` pins, each file checked against `assets/swag/SHA256SUMS` by the themes test;
`scripts/swag.sh <commit>` moves the pin (#928). Your own go in `<config dir>/themes/<name>/` next to fleet.toml; one
named like a built-in wins, file by file.

## Picking one

Each phone picks its own in the **look** sheet: the header's `[◐]`, on every tab, opens the board's own bottom sheet
with three lists, THEME (each theme's row sprite, else its icon), COLOURS (below) and STYLE (#335: `pixel`, the sprites,
or `ascii`, the theme's ASCII figures on the Lanes scene, the Queue and Events rows, the Host miniature and the loading
overlay; default `pixel`, hidden for a theme without `ascii` in `modes`, like terminal), the current row of each ticked (#290).
A tap, or Enter on a focused row (↑/↓ move), applies it in place, no reload, and the sheet stays open. The pick is kept
in localStorage and applied before the first paint like the tab, so there is no flash; a switch swaps the favicon. A
phone that never picked gets `[serve] theme = "<name>"` (default `farm`). The Host tab's `theme` row shows the board's
default: green, or amber with the first problem when its theme.json breaks the contract (then farm is used). iOS keeps
the home-screen icon it saved at install; the sheet's foot says so.

## The contract (`theme.json`)

Every key is optional. What a theme lacks is filled in by the server (`/themes/<name>/theme.json` comes resolved):

| key | what | when missing |
|---|---|---|
| `actions` | sprite id per action: `impl`, `fix`, `rev`, `update` (a lane at work, by role), `idle`, `done`, `blocked` | the theme's `poses` sprite for that action's pose (`working` for the four roles), then farm's action |
| `poses` | sprite id per pose: `working`, `idle`, `done`, `blocked` (the Herd's horses) | farm's |
| `looks` | a list of variant sets, `{"--c": "#…", "--d": …}`; a lane takes one by a stable hash of its name. Sprites may also use `var(--fk)`, the fleet's colour | the built-in coats |
| `stages` | #423: sprite id per growth stage of the lane's issue: `queued`, `impl`, `review`, `fix`, `ready`, `merged`, `blocked` (stuck: blocked, dead or needing you). A lane draws it under its `actions` sprite in the same 132 × 100 box; a queued issue at the Herd's rail is its `queued` plant (with `rail.needs` over it while it needs you); a Queue row draws `<id>-row`. With `species`, `<id>-<species id>` (and `<id>-<species id>-row`) first, then `<id>` | none: the action alone, as before |
| `species` | #423: a list of variant sets like `looks`, each with an `id` (its sprites' suffix): an issue takes one by its number (`n % length`), so it never changes and neighbours differ | none |
| `load_scene` | `{"kind": "weather" \| "heat" \| "light" \| "none", "levels": {"calm", "busy", "held"}, "fan": id}`: the Host group headers' miniature (clouds and rain, or flame height) at the group's status (green, amber, red); `levels` are the words its label reads. #370 `minis`: `{"calm", "busy", "held"}` sprite ids drawn 48 × 16 on each Host card's header beside its level word (`simmer · calm`; load/limit under 0.5 calm, from 0.5 busy, at the limit held). `none`: text only. `heat` also heats the Herd: `heat-calm` (load/limit < 0.5), `heat-busy`, `heat-held` (the spawn gate refuses) on `#herd`, no night and no `[A]`; idle lanes draw `fan` while held. #423 `light`: `light-calm`, `light-busy`, `light-held` on `#herd` show `scene.svg`'s `.l-sun`, `.l-overcast` or `.l-rain` layer (the shop window), no night and no `[A]` | farm's (`weather`: clear, windy, storm) |
| `rail` | `{"item", "needs"}`: the sprite of a queued issue at the Herd's gate, and its needs-you variant | `item`: the `idle` action; `needs`: the item |
| `moments` | `{"merged" \| "fail" \| "needs-you": {"sprite": id, "anim": "hop" \| "shake" \| "pulse"}}`, played for 2.4 s when the log gets that event (an `ev` with its `k`, fleet `f` and issue `n` on `/live`, once per seq). #370: on the issue's row on the visible tab (merged: the `row_sprites.merged` sprite hops; fail: the row shakes; needs-you: the sprite pulses, the row flashes), its theme word as a tag where the age is; a `/live` patch mid-moment resumes it. No row in view: the top-right overlay. Silent; reduced motion shows a still frame. `false`: none | farm's |
| `row_sprites` | sprite id per action for the Queue and Events row sprite (16 × 12 art drawn at 24 px, 1.5× with crisp pixels, #281; its title and label is the plain state word), and (#370) per kind: `queued`, `needs` (a Queue row waiting or needing you; an Events ask), `merged`, `fail` (Events rows, and the row's moment). Each loops its `.kf1`/`.kf2` frames on the visible tab (frame 1 under reduced motion). `false`: rows keep their state glyph | the action's sprite; a kind: farm's |
| `loading` | the sprite id the loading overlay runs while a load, refresh or action takes over 300 ms (#337): 96 px, centred near the top of the content, ~8 fps via `steps()` (a horse's trot by its `.leg`s, the `.kf1`/`.kf2` frames with the flame); reduced motion: one still frame | the spinning `↻` (not farm's) |
| `icons` | `180`, `192`, `512`, `favicon`: one of `icon-180.png`, `icon-192.png`, `icon-512.png` in the folder | the same names; a file the folder lacks is the board's icon |
| `ascii` | per action, the Lanes ascii style's words: at most 3 lines of 8 narrow characters (no CJK, emoji or control characters) | farm's ASCII horses |
| `modes` | the Lanes modes offered, from `scene`, `ascii`, `glyph`: with `ascii` the look sheet offers STYLE (`pixel` is `scene`); there is no Lanes mode button (#335) | `["scene", "ascii"]` |
| `ascii_rows` | per action, the ascii style's Queue and Events row figure in the sprite's slot: one line of at most 3 narrow characters (farm `~h=`, kitchen `c~@`); its title and label is the state word | farm's |
| `ascii_load` | `{"calm", "busy", "held"}`: the ascii style's Host miniature, one line of at most 5 narrow characters (farm `\O/` `~~~` `///`, kitchen's flame) | farm's |
| `ascii_loading` | the ascii style's loading overlay: two frames, one per line, swapped by CSS (farm `~h~` / `>h<`) | farm's |
| `scheme` | the colour scheme a phone gets until it picks one (below); #423: or `{"light": name, "dark": name}`, picked by the phone's `prefers-color-scheme` and followed live | `github-dark` |
| `glyphs`, `words`, `scene` | the Herd's label marks per lane state, label words (`{"horses": "chefs"}`; also a band's name, `queued` … `blocked/dead`, and a working lane's word by action, `impl` … `update`; #370: the board's labels on every tab, each shown as `<theme word> · <plain word>`, the plain one muted in `--fs-meta`: the groups `needs`, `working`, `idle`, `done`, `queued`, `parked`, `dead`, the roles `impl`, `fix`, `rev`, `update`, `merged`, `fail`, each tab's first line `tab-queue` … `tab-events`, the empty tabs `empty-queue`, `empty-lanes`, `empty-events`, and `loading` under the loading overlay; push titles use `merged` and `needs`), and `scene.weather: false` / `scene.line: true` (each fleet's page shows every fleet's `scene.svg` side by side, its own in the middle) / `scene.palette` (CSS custom properties on the Herd; herd.css reads `--done` for the done sprites' dimming, `--qw` for a queued slot's width, `--flutter` for a needs-you rail item's animation) | `glyphs`, `scene`: farm's, key by key; `words`: the plain word (#370: never farm's) |

A key of the wrong shape is an error naming it (`` `ascii.impl` must be at most 3 lines of 8 narrow characters ``);
the whole theme is then farm.

## Files

- `sprites.svg`: one `<g id=…>` per sprite id, drawn in a 132 × 100 box (#423: or `data-box="0 0 24 24"`, its own, so a
  row sprite or Host mini keeps whole pixels); coats are the CSS vars `--c`, `--d`, `--m`,
  `--b`; a `.leg` group swings with the trot, `var(--m)` fills skew in a storm. A sprite id the sheet lacks is farm's.
  `var(--fk)` is the fleet's colour, `.kf1` / `.kf2` groups are a two-frame loop (swapped every 0.6 s under `heat`, frame 1
  under reduced motion), `var(--f1)` / `var(--f2)` the flame colours by heat level, `.ksteam` shows from busy.
- `scene.svg` (optional): the Herd strip's background, `viewBox="0 0 390 320"`.
- `icon-180.png`, `icon-192.png`, `icon-512.png` (optional).

`serve` serves `/themes/<name>/<file>` for these files only (an unknown or invalid theme is farm's files);
`--export` writes every built-in and the board's theme to `themes/`. Debug: `?theme=<name>` for one load, and in the
demo `?nopose=<pose>`.

## Terminal

`terminal` is the board before themes: state glyphs on every row, the Lanes tab in glyph mode only (no STYLE in the look sheet),
no scene miniature, no moments, the load as text; its icon is a green `>_` on the dark tile:

```json
{ "load_scene": { "kind": "none" }, "moments": false, "row_sprites": false, "modes": ["glyph"] }
```

## Colour schemes (#244)

A scheme is the board's colours: one JSON file of every colour token the tabs use, so nothing else changes. Built in
(`assets/schemes/`): `github-dark` (the board before schemes, colour for colour), `solarized-dark`, `gruvbox`, `nord`,
`dracula`, `paper` (light, for daylight), `farm` (night green and soil, wheat-gold controls, sky-blue links),
`kitchen` (charcoal tile, copper controls, brass links, warm cream text), and #423's `garden-light` (linen, terracotta
controls, sage-teal links) and `garden-dark` (moss and walnut, lamp-amber controls). Your own go in
`<config dir>/schemes/<name>.json` (letters, digits and `-`); one named like a built-in wins. The Herd scene
(herd.css) is theme art and keeps its own colours. Contact sheet and values: `docs/design/schemes/`.

```json
{ "label": "Nord", "color-scheme": "dark", "tokens": { "--bg": "#2e3440", "--card": "#3b4252", … } }
```

| token | use |
|---|---|
| `--bg`, `--card`, `--edge`, `--line`, `--scrim` | page; raised surfaces (cards, `pre`, chat blocks); box borders; row rules; the sheet's backdrop |
| `--fg`, `--mute` | text, secondary text |
| `--l`, `--link` | the accent (`[controls]`, the pressed control, the active tab), links |
| `--g`, `--a`, `--r` | status: ok, working or waiting, needs you |
| `--p`, `--c` | the assistant's chat block, event kinds, terminal colours |
| `--fk-l`, `--fk-p`, `--fk-c`, `--fk-g`, `--fk-a` | the five fleet colours |

All 19, each `#rrggbb` or `#rrggbbaa`, and `color-scheme` `dark` or `light`. **The gate** (serve.js, on load) refuses
a scheme unless: text (`--fg`, `--mute`, `--l`, `--link`, the fleet colours) is ≥ 4.5:1 on `--bg` and `--card`; the
status colours ≥ 3:1 on both; `--bg` ≥ 4.5:1 on `--l` (the pressed control); on the tinted surfaces, `--fg`, `--mute`
and `--l` (the option buttons) ≥ 4.5:1 and `--r` ≥ 3:1 on the needs-you card (`--r` 15 % over `--bg`), and `--r`/`--a`
≥ 3:1 on the red/amber group headers (12 %); green, amber and red ≥ 20 apart (CIEDE2000), and `--l` and `--link` ≥ 20
from each of them, so a control never reads as a warning. A refused scheme is left out of the picker and named on the
Host tab with its first reason; a phone that picked it gets its theme's default, then `github-dark`.

**Picking one:** the look sheet's COLOURS list (`[◐]`) shows every scheme that passes, each as `Aa` in its fg on its
bg, then its green, amber and red chips, per phone, kept in localStorage. A phone that never picked gets its theme's `scheme`, marked
`(theme default)`: farm `farm`, kitchen `kitchen`, garden `garden-light` or `garden-dark` by the phone's light/dark
setting, terminal `github-dark`; picking that row forgets the pick, so the
colours follow the theme again. The page paints the phone's last scheme before the first paint (no flash; else the board
theme's built-in default, a `{light, dark}` one by media query), and the iOS status bar takes its `--bg`. The Host tab's `theme` row names this phone's
theme and colours. Debug: `?scheme=<name>` for one load.
