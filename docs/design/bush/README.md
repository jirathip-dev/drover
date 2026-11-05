# Bush theme: design mockup (#462, phase 1)

The bush theme is the Australian outback. Each role is a native animal: impl is a kangaroo bounding, fix a wombat
digging, rev a koala peering down from a gum, update an emu running. Idle is a koala asleep in the fork of a gum, done a
kookaburra laughing, blocked a wombat stuck in its burrow. It is built to the theme contract (#241, #370) and #423's
light/dark `scheme` pair, plus one optional key (`load_scene.kind = "sign"`, below). Farm, kitchen and garden are
untouched. As in the kitchen and garden, every pixel comes from one source, `art.mjs` (pixel maps, one character = one
pixel).

Open `index.html` over http from the repo root (`python3 -m http.server`; it uses ES modules). It follows the phone's
light/dark setting, and the clearing is day or night with it. `?s=light` or `?s=dark` pins one. `?still=1` or Reduce
Motion stops every loop.

## Files

| file | what |
|---|---|
| `index.html` | the mockup: the mob at the three sign levels, the 1440 line of clearings, the sign and its Host mini, the moments, Queue and Lanes at 390, Events, Host, ASCII, the icon, the look sheet, the schemes |
| `bush-{light,dark}-390.png` (DPR 2), `bush-{light,dark}-1440.png` | full-page shots of `index.html`, frame 1 (Reduce Motion) |
| `sheet.html`, `sprite-sheet-{light,dark}.png` | the 7 actions × 6 looks + frame 2; the 3 moments and their frame 2; the rail items (joey, egg, cockatoo); the sign mini at each level; every 16 px row sprite at 24 px and 4× |
| `icon/icon-{180,192,512}.png`, `icon/favicon-{32,16}.png` (+ `.svg`) | the roo-crossing sign icon |
| `art.mjs` | pixel maps: the animals, moments, the rail log, row sprites, the clearing scene, the sign and its mini, the icon, ASCII |
| `schemes/bush-{light,dark}.json` | the two proposed schemes (phase 2 copies them to `assets/schemes/`) |
| `gate.mjs`, `check.mjs` | serve.js's scheme gate, ported line for line (#423's copy, unchanged); `node check.mjs` exits 1 on a fail |
| `scheme.mjs`, `bush.css` | mockup only: applies a scheme as serve.js's head script does; the frame loop, day/night and sign layers |
| `shoot.mjs` | regenerates every PNG: `node shoot.mjs <chrome-headless-shell>` (Chrome over a pipe, via `../theme-370/cdp.mjs`) |

## What it shows

1. **The mob (the Herd), 390 px.** Each fleet page is that repo's clearing: ghost gums on red earth, a dirt track to a
   mesa on the horizon, a billabong at the front. The station gate over the track carries the repo's #162 colour on its
   name board, the way farm's paddocks do. The rail is a **hollow log**: each queued issue is a knot hole with a joey
   peeking out, and the hole that needs you has a cockatoo screeching on top. The bands are *on the move* (working),
   *having a kip* (idle), *home and hosed* (done) and *stuck* (blocked).
2. **1440: the clearings along the track.** One clearing per repo side by side (the existing `scene.line`), the current
   one lit. The sign is the host's load, not a repo's, so only the lit clearing carries it.
3. **The Fire Danger Rating sign = load.** A roadside sign: a half dial in the four AFDRS bands (Moderate green, High
   yellow, Extreme orange, Catastrophic red), a needle and the level word.
   - **Moderate** (load/limit < 0.5): the needle in the green.
   - **High** (≥ 0.5): the needle in the yellow, heat shimmer over the track, a few cicadas.
   - **Catastrophic** (spawns held): the needle in the red, a warm haze over the sky, a cloud of cicadas.

   No flames anywhere. The needle wobbles 5° on two frames. Reduce Motion shows a static sign; the shimmer and cicadas
   stay drawn but don't move. The Host card's 48 × 16 mini is the dial alone.
4. **Day and night.** bush-light draws the clearing by day (a pale sky and a white sun). bush-dark draws it at night: a
   deep indigo sky with the Southern Cross and its two pointers, and the earth dimmed.
5. **Moments.**
   - **merged:** *over the fence*. The kangaroo clears the station fence in one bound and a kookaburra on the post
     laughs (hop).
   - **review FAIL:** *swooped*. A magpie dives at the ducking roo (shake).
   - **needs you:** *screech*. A cockatoo screeches on the hollow log (pulse).

   On a row, the tag is the theme word in the age slot (#370).
6. **Rows.** Queue and Events rows use the role's animal at 16 px. Queued is a joey in the log, needs-you the cockatoo,
   merged the roo over a fence, FAIL the magpie.
7. **ASCII.** Lanes cells are the animals in 3 lines of ≤ 8 (the farm's widths): roo `~__( o>`, wombat `(___)o^`, koala
   `(oYo)||`, emu `(###)/`, the sleeping koala `(-Y-)`, the kookaburra `ha! ha!`, the stuck wombat's burrow. Row figures
   are 3 characters (`~R^` `.W:` `=K|` `E>>` `zK.` `ha!` `[W]`). The load is the needle on a dial: `(\  )` `( | )` `(  /)`.
   Loading is `~R^` / `_R_`, a bounding roo.
8. **Icon.** A yellow roo-crossing diamond with a black bounding kangaroo, on a grey post, sky over red earth. The favicon
   is the diamond alone. It stands apart from the dark-tile farm, kitchen and terminal icons and from garden's linen.
9. **Looks.** Eight looks, picked by the farm's stable hash. Each look is a coat set for all four animals (a red, grey,
   sandy or dusky roo; four wombat, three koala and three emu coats), so one lane keeps its colours from role to role.
   The repo colour is a drover's neckerchief on every animal.

## Words (`words`)

| key | bush | key | bush |
|---|---|---|---|
| `horse` / `horses` | roo / roos | `impl` `fix` `rev` `update` | bound, dig, peer, run |
| `herd` / `ranch` | mob / outback | `queued` | in the log |
| `barn`, `parked` | station (at the station) | `working` / `idle` / `done` | on the move / having a kip / home and hosed |
| `needs` | screech | `blocked/dead` / `dead` | stuck / gone bush |
| `merged` / `fail` | over the fence / swooped | `tab-queue` … `tab-events` | the log, the mob, the station, the track |
| `empty-queue` / `-lanes` / `-events` | nothing in the log / the mob's out of sight / nothing on the track | `loading` | bounding… |

Load levels: `moderate`, `high`, `catastrophic`.

## Contract addition (phase 2; optional, every other theme unchanged)

```json
{
  "scheme": { "light": "bush-light", "dark": "bush-dark" },
  "load_scene": { "kind": "sign", "levels": { "calm": "moderate", "busy": "high", "held": "catastrophic" },
                  "minis": { "calm": "sign-moderate", "busy": "sign-high", "held": "sign-catastrophic" } }
}
```

- **`load_scene.kind = "sign"`** is driven by the same load and gate data as weather, heat and light. The Herd gets
  `sign-moderate` / `sign-high` / `sign-catastrophic`, the way `heat` gets `heat-<level>`. `scene.svg` carries `.s-moderate`,
  `.s-high` and `.s-catastrophic` layers (the needle and its word, the cicadas) and `.s-up` (shimmer, shown at high and
  catastrophic). The test maps calm, busy and held to the three levels, and a theme without the key is unchanged.
- **Day and night follow the scheme** (open question 1): `.sky-day` / `.sky-night` layers in `scene.svg`. herd.css
  already has `.sky-night` and `#herd.night` for the farm's clock.
- `actions` are `a-bound` … `a-stuck`; `rail` is `{ "item": "q-joey", "needs": "q-cocky" }`; `moments` are merged
  `m-clear` hop, fail `m-swoop` shake, needs-you `m-screech` pulse; `row_sprites` are `r-*`; `loading` is `a-bound`
  (its two frames at ~8 fps read as the bound).

## Schemes (both pass the gate, `node check.mjs`)

| | bg / card | fg / mute | `--l` control | `--link` | `--g` `--a` `--r` | tightest check |
|---|---|---|---|---|---|---|
| bush-light | bleached ochre `#f5ecd8` / `#ebdfc4` | `#2b2118` / `#5d4e3b` | red earth `#983815` | dusk blue `#3a5a8c` | `#386826` `#765700` `#a3134d` | `--l` on the needs-you card 4.75 (min 4.5) |
| bush-dark | night indigo `#14152b` / `#1f2040` | `#f0e6d2` / `#aaa3bd` | ochre `#e89a4c` | starlight `#a9c4f0` | `#86c96e` `#f3dc6a` `#ff6f8e` | ΔE green ↔ amber 22.2 (min 20) |

Gum-leaf sage can't be the control colour, because the gate needs `--l` ≥ 20 ΔE from status green. So the sage stays in the
art (the gums, the spinifex), and red earth (light) and ochre (dark) are the controls. To stay ≥ 20 ΔE from red earth, light's
needs-you red leans crimson.

## Open questions for the owner

1. **The clearing's day/night follows the scheme, not the clock.** The issue's dark scheme is "outback night ... with the
   Southern Cross", so bush-dark draws the night sky and bush-light the day. The farm's `[A]` clock light would be hidden
   for bush, as it is for heat. **Recommend: as mocked.** The alternative is keeping `[A]` and the farm's clock.
2. **Joey or egg in the log.** "one joey or egg waiting in it per queued issue". I mocked a joey for every queued issue
   (the contract's single `rail.item`); the egg is drawn in the sheet as an alternative. Alternating them (say, an egg for
   an issue queued behind a `block --on`) needs a new `rail` key. **Recommend: joey only.**
3. **The sign shows on the lit clearing only** at 1440, since load is per host. This needs one class (`.fdr`) that
   herd.js hides on the neighbours. OK?
4. **Words not in the issue:** in the log, on the move, having a kip, home and hosed, stuck, gone bush (dead), screech,
   swooped, over the fence, and the tab names. They are data, so change any you like.
5. **The four AFDRS bands.** The dial shows all four real bands, including Extreme, but the needle only ever points at
   Moderate, High and Catastrophic. The alternative is a three-band dial, which isn't a real sign. **Recommend: four bands.**
6. **The magpie swoops a kangaroo** in the FAIL moment, whatever the issue's last role was. Per-role swoops would be four
   sprites instead of one. **Recommend: one.**

Not changed: farm, kitchen, terminal, serve.css, herd.css, serve.js. The mockup only links them.
