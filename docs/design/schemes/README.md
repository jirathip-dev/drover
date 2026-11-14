# Colour schemes: the contact sheet (#244, phase 1)

> **Approved 2026-11-03 ("Approve all with fixes"), built in phase 2.** The open questions below took their
> recommendations: these token names, brass links, no tile texture, lifted palettes, `theme-color` from `--bg`, the
> added gates, and the gate in `assets/serve.js` (`src/` only serves the files). The owner's fixes changed the values
> and the gate, so `assets/schemes/` (copied to `schemes/` here) and the board's gate are the source of truth; the PNGs
> and `report.json` are phase 1's. The CI `ui` job shoots Queue and Host in every scheme from the real CSS
> (`ui-390/queue-scheme-<name>.png`, `busy-host-ranked-scheme-<name>.png`).
>
> 1. Accent/link ↔ status ΔE ≥ **20** (was 10): farm's wheat `--l` `#e2bf5e → #ecdba6`, kitchen's brass `--link`
>    `#c9a55a → #c4a88f`.
> 2. Green ↔ amber ≥ 25 in gruvbox (`--g` `#a9b665 → #8fbf5a`, 28.1) and solarized-dark (`#83a622 → #74b041`, 26.8);
>    nord's red `#e0707a → #ff6b76` (4.53:1). gruvbox `--mute` `#a89984 → #ad9e88` for the card below.
> 3. The gate also measures the tinted surfaces: `--fg`, `--mute`, `--l` (option buttons) ≥ 4.5:1 and `--r` ≥ 3:1 on
>    the needs-you card; `--r`/`--a` ≥ 3:1 on the red/amber group headers.
> 4. The Host tab's `theme` row names the phone's theme and scheme (serve.js), not the board default.

`index.html` is the contact sheet: **Queue** and **Host** at 390 px for each of the 8 built-in schemes, with the gate
result, swatches and tightest numbers above each pair. `contact-390.png` (DPR 2) and `contact-1640.png` are the
sheet rendered at 390 px and 1640 px. `<scheme>-queue-390.png` and `<scheme>-host-390.png` are the individual phones
(390×900 at DPR 2).

**Every phone is the real board.** `shoot.mjs` takes the demo export, rewrites its `serve.css` the way phase 2 will
(`serve.css.diff`), inlines one scheme's token block first in `<head>` (as `serve.rs` will in phase 2: no flash) and
screenshots it with headless Chrome over CDP. It has no dependencies. Regenerate everything with:

```sh
DROVER_HOME=<demo copy> FLEET_CONFIG=<demo copy>/fleet.toml FLEET_STATE=<demo copy>/events.jsonl \
  ./target/release/drover serve --export <site> --demo && scripts/host-fixtures.sh <site>
node docs/design/schemes/check.mjs            # the gate alone: PASS/FAIL per scheme
node docs/design/schemes/shoot.mjs <site> <chrome-headless-shell>   # the phones, report.json, index.html, the sheets
```

**Data:** the committed synthetic `demo/`, with one change in a scratch copy: a PASS verdict for acme-api #15, so the
Queue has a **ready** row (the demo has none). Nothing in the repo's `demo/` changed. Host is `scripts/host-fixtures.sh`'s
`busy` fixture, so it has the **red bar** (load/cpu 4.24 over 2.5, MACHINE red). The shots are framed without changing
any style: the demo banner is removed, Queue keeps only the first **needs-you card** above the fleet cards, and Host drops
the needs-you cards so its groups fill the screen. Each Queue shows a needs-you card, `waiting`/`needs-human`/`dead` in red,
**ready** in green, a **working** row (`impl`) in amber, the fleet bars, and the active tab as a pressed key (`--bg` on
`--l`). Each Host shows a red group and bar, amber groups and green values.

## Gate 2's pixel identity: done

`github-dark` with the proposed `serve.css` is **pixel-identical** to today's board on both views. `shoot.mjs`
shoots today's untouched export and the proposed one under `github-dark`, compares the PNG bytes, and fails if they
differ. The result is recorded in `report.json` (`"identical": {"queue": true, "host": true}`).

## The token set

A scheme is one JSON file: `{ "label", "color-scheme": "dark" | "light", "tokens": { … } }` with these 19 colours, all
required:

| token | today (`github-dark`) | use |
|---|---|---|
| `--bg` | `#0d1117` | page, header, tab bar |
| `--card` (new) | `#161b22` | raised surfaces: chat blocks, `pre`, Queue cards, the textarea, the pipeline boxes. Today it is a literal in serve.css 8 times |
| `--edge` (new) | `#30363d` | box borders: fleet cards, sheets, the handle. Today it is a literal 11 times |
| `--line` | `#21262d` | row rules |
| `--scrim` (new) | `#000000aa` | the sheet's backdrop (today `#000a`) |
| `--fg` | `#c9d1d9` | text |
| `--mute` | `#8b949e` | secondary text |
| `--l` | `#58a6ff` | the accent: `[controls]`, the pressed control and active tab fill, the focus flash |
| `--link` (new) | `#58a6ff` | links (`a`). Today these are `--l`. Split so farm can have wheat controls and sky-blue links |
| `--g` `--a` `--r` | `#3fb950` `#d29922` `#f85149` | status: ok / working or waiting / needs you |
| `--p` `--c` | `#bc8cff` `#39c5cf` | the assistant's chat block, event kinds, terminal colours |
| `--fk-l` … `--fk-a` (new) | = `--l --p --c --g --a` | the 5 fleet colours (#162's hash, `[fleet.<n>] color`). Split from the accent and status colours so a scheme can recolour fleets without recolouring controls |

`--l`, `--g`, `--a`, `--r`, `--p`, `--c` and `--line` keep their names, so serve.css changes only where a literal colour
was (7 swaps, `serve.css.diff`, 20 lines out of 412). After the swaps serve.css has **no raw colour** outside the
scheme. `shoot.mjs` checks this, and it is the check the #63 lint gains in phase 2.

## The gate (`check.mjs`)

A scheme is refused unless every check passes. The board will run the same checks on load:

| check | minimum | why |
|---|---|---|
| all 19 tokens present, `#rrggbb` or `#rrggbbaa`; `color-scheme` is dark or light | – | completeness |
| `--fg`, `--mute` on `--bg` and `--card` | 4.5:1 | the issue's text gate |
| `--l`, `--link` and the 5 `--fk-*` on `--bg` and `--card` | 4.5:1 | **added:** they are text too (controls, links, fleet names) |
| `--g`, `--a`, `--r` on `--bg` and `--card` | 3:1 | the issue's status gate |
| `--bg` on `--l` | 4.5:1 | **added:** the pressed control and the active tab's label |
| `--fg` on the needs-you tint (`--r` 15 % over `--bg`), and `--r` on it | 4.5:1 / 3:1 | **added:** the red card is the one tinted surface carrying body text |
| ΔE2000 between each pair of `--g`, `--a`, `--r` | ≥ 20 | the issue's "distinguishable" gate |
| ΔE2000 between `--l`/`--link` and each status colour | ≥ 10 | **added:** a control must never read as a warning (wheat vs amber, copper vs red) |

The tightest result of each built-in, per check group:

| scheme | gate | fg/mute | accent/link | fleet | status | pressed | red-card text | status ΔE | accent ΔE |
|---|---|---|---|---|---|---|---|---|---|
| `github-dark` | PASS | 5.62 | 6.85 | 6.81 | 5.16 | 7.49 | 4.82 | 34.6 | 48.2 |
| `solarized-dark` | PASS | 4.63 | 4.61 | 4.61 | 3.11 | 5.33 | 3.29 | 21.8 | 50.7 |
| `gruvbox` | PASS | 4.72 | 4.88 | 4.78 | 3.82 | 5.48 | 3.63 | 21.6 | 19.7 |
| `nord` | PASS | 4.65 | 4.64 | 4.67 | 3.24 | 6.24 | 3.31 | 21.6 | 28.1 |
| `dracula` | PASS | 5.94 | 5.20 | 5.20 | 3.99 | 5.90 | 3.78 | 30.9 | 36.0 |
| `paper` | PASS | 5.39 | 5.76 | 4.62 | 4.62 | 6.29 | 4.33 | 29.3 | 44.1 |
| `farm` | PASS | 6.93 | 9.11 | 7.91 | 6.96 | 10.06 | 5.69 | 20.4 | 16.7 |
| `kitchen` | PASS | 5.88 | 6.94 | 7.11 | 5.48 | 7.78 | 4.89 | 28.1 | 13.7 |

("red-card text" is the lower of the two needs-you-tint checks, so it shows `--r` on the tint, which has a minimum of 3.)

**Refusals:** `refused/` holds two crafted bad schemes. `node check.mjs refused` refuses both and gives the reason:

```
FAIL missing-token  missing or not #rrggbb[aa]: --fk-a
FAIL solarized-canonical --mute on --bg 2.79 < 4.5; --l on --bg 4.08 < 4.5; … --r on --card 2.81 < 3; … ΔE2000 --g ↔ --a 18.07 < 20
```

## The schemes

Each scheme keeps its source palette's hues. Where the source values fail the gate, they are lifted just far enough
to pass:

- **`github-dark`:** today's values, unchanged.
- **`solarized-dark`:** base03/base02 backgrounds and base1 text. The canonical accents fail 4.5:1 on base03; that is
  the canonical scheme refused above. Blue, violet, cyan, green and yellow are lifted toward white just enough to pass
  (blue `#268bd2 → #4fa1db`). Mute uses lifted base0 `#8e9d9f`, because base01 is 2.8:1. Green is nudged greener (`#859900 → #83a622`) so it
  stays ≥ 20 ΔE from yellow.
- **`gruvbox`:** the canonical dark palette, except green: `#b8bb26` is only 17 ΔE from gruvbox yellow, so it is
  gruvbox-material's `#a9b665`. The accent is gruvbox blue `#83a598`.
- **`nord`:** Polar Night backgrounds and Snow Storm text. Mute is a custom `#a7b1c2`, because nord3 is too dark for
  text. Links are frost `#8fb4d8`, lifted from nord9. Red `#e0707a` and green `#9ccc8c` are lifted from nord11/14 so
  that red passes 3:1 and green stays ≥ 20 ΔE from yellow.
- **`dracula`:** the canonical palette. Mute is `#a9b1d6`, because the comment colour `#6272a4` is 3.0:1. Amber is
  Dracula orange `#ffb86c`, because its yellow reads as lime. The accent is purple and links are cyan.
- **`paper`** (light): warm paper `#fbf8f1`, ink `#24292f`, and GitHub-light-style status colours darkened for the
  cream card. The scrim is a translucent brown.
- **`farm`:** **#242's values** (the shared theme repo's design/farm-v2, on branch `design-242`): night-green `#0f1a13`, soil cards
  `#1d1a12`, cream text, **wheat-gold controls** `#e2bf5e`, **sky-blue links** `#7cc4f0`. Amber is moved to orange
  `#f29b4b` so a working row never reads as a wheat control.
- **`kitchen`:** **#239's values** (the shared theme repo's design/kitchen/art.mjs, on branch `design-239`): charcoal `#1a1816`, tile
  cards `#24201c`, warm cream text, **copper controls** `#e89a62`, a brass-dark `--edge`, a yellow amber `#f4d24e` and a
  pink-red `#ff5c84`, each clear of copper. **Brass `#c9a55a` is the link colour here** (see Q2).

Status colours stay green, amber and red in every scheme: the hue order never changes, only the shade.

## Phase 2, as this mockup assumes it

- `assets/schemes/<name>.json` holds these 8 files unchanged. User files go in `<config dir>/schemes/<name>.json`, and a
  user file wins over a built-in with the same name.
- `theme.json` gets a `scheme` key: farm → `farm`, kitchen → `kitchen`, and terminal (#241) → `github-dark`. A theme
  without the key gets `github-dark`.
- `serve.rs` inlines the default scheme's block (`:root{--bg:…;color-scheme:…}`) at the top of `<head>`, as
  `shoot.mjs` does. The phone's `[colours]` pick, kept in localStorage with its block, is applied by the existing #206
  head script before the first paint, so there is no flash.
- The picker sits next to `[theme]`. It lists every scheme that passes the gate. Failing ones are left out and named in
  a Host-tab warning with their reasons, and the phone falls back to the theme's default.
- The #63 lint gains "no raw colour in serve.css" (the comment-blanking regex in `shoot.mjs`).

## Not in a scheme

- **The Herd scene** (`herd.css`, sky, pasture, horses and chefs) is theme art and keeps its own colours. Only the
  board tabs take the scheme.
- **The tab bar and status bar colour:** `<meta name=theme-color>` and the manifest are a fixed `#1f2a38`. See Q5.
- **Disabled controls** (`[drive now]` in the demo) are `--mute` at 60 % opacity. They are under 4.5:1 in every scheme,
  today's included. WCAG exempts disabled controls, so the gate does too.

## Open questions for the owner

1. **Token names.** #242 (farm) proposed `--sf`, `--edge` and `--ac` for controls with `--l` for links. #239
   (kitchen) proposed `card`, `--edge`, `--brass`, and `--l` for both controls and links. This sheet uses the issue's
   names (`--card`, `--link`) and keeps `--l` as the control accent, so its 12 other `var(--l)` uses (buttons, tabs, `.grp code`, chips…) don't change.
   **Recommend: this set.** The farm and kitchen build issues then use these names. Their colour values are used
   here unchanged.
2. **Kitchen brass.** #239 puts brass on the header title and the Host group names, which needs an extra `--title`
   token. Here brass is the link colour and the dark-brass `--edge`, so the set stays at 19. **Recommend: brass on
   links**, unless you want the brass title. That is one more token, and every scheme would then need to define it.
3. **Kitchen tile texture.** #239 also draws a faint tile grid over the background (`background-image`). That isn't a
   colour, so it doesn't fit "a scheme is a token set". **Recommend: leave it out.** The kitchen's Herd scene already
   has its checker floor.
4. **The source palettes are lifted, not exact.** Canonical Solarized fails the gate badly (above), and Nord's and
   Dracula's comment greys fail as `--mute`. **Recommend: accept the lifted values,** as the gate requires. Anyone who
   wants the exact values can add a user file, but the gate will refuse it.
5. **Status-bar colour.** The iOS status bar and the PWA chrome use `theme-color` `#1f2a38`, which is a dark bar even
   over `paper`. **Recommend:** in phase 2 the head script sets `<meta name=theme-color>` to the scheme's `--bg`, and
   the manifest keeps `#1f2a38` (it can't change per phone).
6. **The added gates** (accent, link and fleet colours as text, the pressed tab, the red card, accent ΔE ≥ 10) go
   beyond the issue's list. Every built-in passes them, and they catch the failures the owner's farm and kitchen
   briefs invite (wheat vs amber, copper vs red). **Recommend: keep them.**
7. **Where the gate runs vs the src budget.** CIEDE2000 and the contrast maths are about 40 lines, more than the
   issue's `src/ ≈ +25`. **Recommend:** `src/` only serves the schemes (built-in and user files, about 20 lines), and
   the gate runs in `assets/serve.js` (outside the line cap), which filters the picker and writes the Host warning.
   A Rust test would run that JS under node, as `tests/chat_blocks.rs` does. Alternatively, the gate goes in Rust and the
   budget is about +60.
8. **The tightest margins:** farm's amber↔red is ΔE 20.4 (#242's values; the threshold is 20) and gruvbox's
   accent↔green is 19.7 (threshold 10, so it passes easily). Solarized's red is 3.11:1 on its card, against a minimum
   of 3. These pass, but a later tweak could tip them. Fine as they are?
