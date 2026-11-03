# Themes (#172, #241, #244)

A theme is one folder `themes/<name>/` that every tab of the phone page draws from: the Lanes avatars, a 16 px
sprite on each Queue and Events row, a load scene on each Host group, the Herd, the moments, the icon. It is data
only: `theme.json`, `sprites.svg`, and optionally `scene.svg` and its icons. No theme ships CSS or JS.

Built in: `farm` (horses, the ranch herd.js draws), `kitchen` (pixel chefs at a counter) and `terminal` (today's
plain board). Your own go in `<config dir>/themes/<name>/` next to fleet.toml; one named like a built-in wins,
file by file.

## Picking one

Each phone picks its own with the header's `[◐]` (kept in localStorage, applied before the first paint like the tab,
so there is no flash; switching applies in place, no reload, and swaps the favicon). A phone that never picked gets
`[serve] theme = "<name>"` (default `farm`). The Host tab's `theme` row shows the board's default: green, or amber
with the first problem when its theme.json breaks the contract (then farm is used). iOS keeps the home-screen icon it
saved at install; the first switch says so once.

## The contract (`theme.json`)

Every key is optional. What a theme lacks is filled in by the server (`/themes/<name>/theme.json` comes resolved):

| key | what | when missing |
|---|---|---|
| `actions` | sprite id per action: `impl`, `fix`, `rev`, `update` (a lane at work, by role), `idle`, `done`, `blocked` | the theme's `poses` sprite for that action's pose (`working` for the four roles), then farm's action |
| `poses` | sprite id per pose: `working`, `idle`, `done`, `blocked` (the Herd's horses) | farm's |
| `looks` | a list of variant sets, `{"--c": "#…", "--d": …}`; a lane takes one by a stable hash of its name. Sprites may also use `var(--fk)`, the fleet's colour | the built-in coats |
| `load_scene` | `{"kind": "weather" \| "heat" \| "none", "levels": {"calm", "busy", "held"}}`: the Host group headers' miniature (clouds and rain, or flame height) at the group's status (green, amber, red); `levels` are the words its label reads. `none`: text only | farm's (`weather`: clear, windy, storm) |
| `rail` | `{"item", "needs"}`: the sprite of a queued issue at the Herd's gate, and its needs-you variant | `item`: the `idle` action; `needs`: the item |
| `moments` | `{"merged" \| "fail" \| "needs-you": {"sprite": id, "anim": "hop" \| "shake" \| "pulse"}}`, played for 2.4 s when the log gets that event (`moment` on `/live`). Silent; reduced motion shows a still frame. `false`: none | farm's |
| `row_sprites` | sprite id per action for the 16 px Queue and Events row sprite (its title and label is the plain state word). `false`: rows keep their state glyph | the action's sprite |
| `icons` | `180`, `192`, `512`, `favicon`: one of `icon-180.png`, `icon-192.png`, `icon-512.png` in the folder | the same names; a file the folder lacks is the board's icon |
| `ascii` | per action, the Lanes `[ascii]` mode's words: at most 3 lines of 8 narrow characters (no CJK, emoji or control characters) | farm's ASCII horses |
| `modes` | the Lanes modes offered, from `scene`, `ascii`, `glyph`; one mode hides the button | `["scene", "ascii"]` |
| `scheme` | the colour scheme a phone gets until it picks one (below) | `github-dark` |
| `glyphs`, `words`, `scene` | the Herd's label marks per lane state, label words (`{"horses": "chefs"}`), and `scene.weather: false` / `scene.palette` (CSS custom properties on the Herd) | farm's, key by key |

A key of the wrong shape is an error naming it (`` `ascii.impl` must be at most 3 lines of 8 narrow characters ``);
the whole theme is then farm.

## Files

- `sprites.svg`: one `<g id=…>` per sprite id, drawn in a 132 × 100 box; coats are the CSS vars `--c`, `--d`, `--m`,
  `--b`; a `.leg` group swings with the trot, `var(--m)` fills skew in a storm. A sprite id the sheet lacks is farm's.
- `scene.svg` (optional): the Herd strip's background, `viewBox="0 0 390 320"`.
- `icon-180.png`, `icon-192.png`, `icon-512.png` (optional).

`serve` serves `/themes/<name>/<file>` for these files only (an unknown or invalid theme is farm's files);
`--export` writes every built-in and the board's theme to `themes/`. Debug: `?theme=<name>` for one load, and in the
demo `?nopose=<pose>`.

## Terminal

`terminal` is the board before themes: state glyphs on every row, the Lanes tab in glyph mode only (no mode button),
no scene miniature, no moments, the load as text; its icon is a green `>_` on the dark tile:

```json
{ "load_scene": { "kind": "none" }, "moments": false, "row_sprites": false, "modes": ["glyph"] }
```

## Colour schemes (#244)

A scheme is the board's colours: one JSON file of every colour token the tabs use, so nothing else changes. Built in
(`assets/schemes/`): `github-dark` (the board before schemes, colour for colour), `solarized-dark`, `gruvbox`, `nord`,
`dracula`, `paper` (light, for daylight), `farm` (night green and soil, wheat-gold controls, sky-blue links) and
`kitchen` (charcoal tile, copper controls, brass links, warm cream text). Your own go in
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

**Picking one:** the header's `[▦]` (next to `[◐]`, off the Queue tab like it) lists every scheme that passes, per
phone, kept in localStorage. A phone that never picked gets its theme's `scheme`: farm `farm`, kitchen `kitchen`,
terminal `github-dark`. The page paints the phone's last scheme before the first paint (no flash; else the board
theme's built-in default), and the iOS status bar takes its `--bg`. The Host tab's `theme` row names this phone's
theme and colours. Debug: `?scheme=<name>` for one load.
