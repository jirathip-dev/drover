# Garden theme: design mockup (#423, phase 1)

The garden theme is a cozy houseplant shop. Each issue is a plant that grows leaf by leaf as it moves through the
pipeline. A shopkeeper tends it by role: water (impl), prune (fix), inspect (rev), repot (update). A blocked plant
wilts. It is built to the existing theme contract (#241, #370) plus four optional keys (below). Farm and kitchen are
untouched. Every pixel comes from one source, `art.mjs`: the keeper and the tools are pixel maps (one character = one
pixel), as the kitchen's are; the leaves are drawn per species by one function onto the same pixel grid.

## Revision 1: finer pixel art (q365)

The first mockup (105ea9d) drew at 4 screen px per art pixel, the farm's and the kitchen's grid. This revision draws at
**2**, so there is twice the detail in each direction, still crisp, with no smoothing. Farm and kitchen are unchanged.

| sprite | box | grid (was) | one pixel |
|---|---|---|---|
| lane (keeper + plant), moments | 132 × 100 | 66 × 50 (33 × 25) | 2 CSS px |
| row sprites (Queue, Events, the look sheet) | the board's 24 px slot | 24 × 24 (16 × 16 at 1.5×) | 1 CSS px, 2 device px on a phone |
| the shop scene | 392 × 188 | 196 × 94 (98 × 47) | 2 |
| the Host mini | 72 × 24 | 72 × 24 (48 × 16 at 1.5×) | 1 |
| icon 512 / favicon 32 / 16 | | 64 / 32 / 16 (32 / 16 / 16 zoomed) | 8 / 1 / 1 |

What the extra pixels carry:
- **Leaves:** an outline, a lit half toward the window and a shaded half, a midrib, and per species its markings.
  Monstera: three slits a side that widen toward the margin. Philodendron: pale side veins and a gloss streak.
  Peperomia: the watermelon bands. Alocasia: an arrow shape with wavy margins and thick white veins. Pothos: golden
  variegation streaks. Calathea: dark centres with pink pinstripes. Yellowed and wilted leaves keep their shape in
  ochre and brown.
- **Pots and soil:** a lip light on the rim, the shadow under it, a lit stripe and a shaded side on the body, and
  specks in the soil. The jar has a water line, roots and a glass highlight. The wrap has folds, a twine bow and a tag.
- **The keeper:** eyes with a shine, blush, brows, and a mouth for each mood (look, smile, eyes shut, worried). Hair
  has a highlight and shade. The straw hat has a weave and a band, the headscarf has dots and a knot, the bun has a tie.
  The shirt has a collar. The apron has straps, a bib pocket, a waist tie and a skirt pocket. The boots have soles.
  The tools are drawn at the same scale: a can with a rose and a spray, scissors, a lens with a glint, a trowel and a
  soil bag, and a mug with a leaf on it.
- **The shop:** brick showing through the plaster, a ceiling beam, a bevelled window frame, houses and a street lamp
  across the street, a hedge with a leafy top, wood grain on the shelves, brackets, a panelled counter. The shelf and
  sill plants are drawn with the same leaf function as the lanes.

Open `index.html` over http from the repo root (`python3 -m http.server`; it uses ES modules). It follows the phone's
light/dark setting. `?s=light` or `?s=dark` pins one. `?still=1` or Reduce Motion stops every loop.

## Files

| file | what |
|---|---|
| `index.html` | the mockup: the shop at sun / overcast / rain, the 1440 shop wall, the window light, moments, Queue and Lanes at 390, Events, Host, ASCII, the icon, the look sheet, the schemes |
| `garden-{light,dark}-390.png` (DPR 2), `garden-{light,dark}-1440.png` | full-page shots of `index.html`, frame 1 (Reduce Motion) |
| `sheet.html`, `sprite-sheet-{light,dark}.png` | 7 stages × 6 species; 7 keeper actions × looks + frame 2; the 3 moments; every 24 × 24 row sprite at 24 px and 4× |
| `icon/icon-{180,192,512}.png`, `icon/favicon-{32,16}.png` (+ `.svg`) | the monstera-leaf icon |
| `art.mjs` | pixel maps and the leaf function: keeper, leaves per species, stages, actions, moments, row sprites, the shop scene, the window minis, the icon, ASCII |
| `schemes/garden-{light,dark}.json` | the two proposed schemes (phase 2 copies them to `assets/schemes/`) |
| `gate.mjs`, `check.mjs` | serve.js's scheme gate, ported line for line; `node check.mjs` exits 1 on a fail |
| `scheme.mjs`, `garden.css` | mockup only: applies a scheme as serve.js's head script does; the frame loop and light layers |
| `build.mjs` | phase 2: writes `assets/themes/garden/` (sprites.svg, scene.svg, theme.json, icons) and `assets/schemes/garden-*.json` from `art.mjs`: `node docs/design/garden/build.mjs` |
| `shoot.mjs` | regenerates every PNG: `node shoot.mjs <chrome-headless-shell>` (Chrome over a pipe, via `../theme-370/cdp.mjs`) |

## What it shows

1. **The shop (the Herd), 390 px.** Each fleet page is that repo's corner of the shop: a window on the left and
   three shelves of pots on the right, the shelf edges and counter trim in the repo's #162 colour. The rail is the
   **counter**: one cutting in a jar per queued issue, and a needs-you jar carries the red thirsty bubble. The bands
   are *being tended* (working), *resting* (idle), *ready for sale* (done) and *wilting* (blocked). Each lane shows the
   keeper doing the role, next to the issue's plant at its stage.
2. **1440: the shop wall.** One bay of shelves per repo side by side (the existing `scene.line`), with the current one
   lit.
3. **Window light = load.** Sun (load/limit < 0.5): blue sky, the sun, a sunbeam across the counter. Overcast (≥ 0.5):
   grey cloud, the room 10 % dimmer. Rain (spawns held): dark cloud, rain running down the glass on two frames, the room
   22 % dimmer. Reduce Motion: one static frame. The Host card's 72 × 24 mini is the same window.
4. **Growth.** Queued is a cutting in a jar with roots in water. Impl is rooted in a pot with two small leaves. Review
   and fix have a few leaves; fix also shows a snipped stem. Ready is the full plant with its mature leaf (the monstera's
   is split). Merged is wrapped in kraft paper with twine and a tag. Blocked droops: yellowed leaves hang over the rim
   and one has fallen. The leaves sway on the two-frame loop.
5. **Species.** Monstera, philodendron, peperomia (watermelon stripes), alocasia (pale veins), pothos (golden
   variegation, it trails at ready), calathea (pink feathering). The species is `issue % 6`, so neighbouring issue numbers
   always differ.
6. **Moments.** Merged: *sold!*, the plant wrapped, the shop bell rings (hop). Review FAIL: *leaf drop*, a yellowed
   leaf falls (the row shakes). Needs you: *thirsty*, a red drop bubble over the pot (pulse). On the row, the tag is the
   theme word in the age slot (#370).
7. **Rows.** Queue rows use the stage × species sprite. Events rows use the role's tool next to a small plant: can,
   scissors, lens, trowel and soil bag, mug.
8. **ASCII.** Lanes cells are the tool next to a `\|/` plant over a `\_/` pot (3 lines × ≤ 8). Row figures are
   3 characters by stage (`[v]` `.v.` `vYv` `vY,` `\Y/` `/Y\` `,v,`) and by action (`~.Y` `8<Y` `oY.` `=uY`). The load is
   `-O-` `~~~` `','`. Loading is `~.Y` / `~:Y`.
9. **Icon.** A two-tone pixel monstera leaf (three slits a side, two holes) on a linen tile over a terracotta pot rim.
   It stands apart from the farm, kitchen and terminal icons, which all sit on a dark tile.

## Words (`words`)

| key | garden | key | garden |
|---|---|---|---|
| `horse` / `horses` | plant / plants | `impl` `fix` `rev` `update` | water, prune, inspect, repot |
| `herd`, `ranch` | shop | `queued` | cuttings |
| `barn`, `parked` | greenhouse (the back room) | `working` / `idle` / `done` | being tended / resting / ready for sale |
| `needs` | thirsty | `blocked/dead` / `dead` | wilting / composted |
| `merged` / `fail` | sold / leaf drop | `tab-queue` … `tab-events` | the counter, the shelves, the shop, the ledger |
| `empty-queue` / `-lanes` / `-events` | no cuttings on the counter / the shop's quiet / nothing in the ledger | `loading` | watering… |

Load levels: `sun`, `overcast`, `rain`.

## Contract additions (phase 2; all optional, farm and kitchen unchanged)

```json
{
  "scheme": { "light": "garden-light", "dark": "garden-dark" },
  "stages": { "queued": "plant-queued", "impl": "plant-impl", "review": "plant-review", "fix": "plant-fix",
              "ready": "plant-ready", "merged": "plant-merged", "blocked": "plant-blocked" },
  "species": [ { "id": "monstera", "--lf": "#3f8f45", "--ld": "#2a6a34", "--lv": "#6fb85f" }, "… six in all" ],
  "load_scene": { "kind": "light", "levels": { "calm": "sun", "busy": "overcast", "held": "rain" },
                  "minis": { "calm": "light-sun", "busy": "light-overcast", "held": "light-rain" } }
}
```

- **`stages`**: a sprite per issue state, drawn in the same 132 × 100 box *under* the lane's `actions` sprite (keeper on
  the left, plant on the right), and as the Queue row sprite. With `species`, the id looked up is
  `<stage id>-<species id>` first, then `<stage id>`. A theme without `stages` keeps today's role pose (the AC's
  fallback test). `blocked` is a seventh key beside the six in the issue, for the wilt.
- **`species`**: variant sets like `looks`, but picked by **issue number** (`n % length`), not by lane name. A set
  is its CSS vars plus an `id` (the sprite suffix), because the species differ in leaf shape, not only colour.
- **`load_scene.kind = "light"`**: `scene.svg` carries `.l-sun` / `.l-overcast` / `.l-rain` layers and the Herd gets
  `light-<level>`, the way `heat` gets `heat-<level>`. No night and no `[A]`, as for heat.
- **`scheme` as `{light, dark}`**: the head script picks by `prefers-color-scheme` and follows it live. A phone's own
  COLOURS pick still wins. A single name works as today.

## Schemes (both pass the gate, `node check.mjs`)

| | bg / card | fg / mute | `--l` control | `--link` | `--g` `--a` `--r` | tightest check |
|---|---|---|---|---|---|---|
| garden-light | linen `#f7f2e6` / `#eee6d3` | `#2a2b22` / `#5b5a49` | terracotta `#9a3f1a` | sage-teal `#1f6470` | `#33721c` `#7a5f00` `#a8164f` | `--l` on the needs-you card 4.70 (min 4.5) |
| garden-dark | moss `#181c15` / walnut `#24231b` | `#ece4cf` / `#a7a28a` | lamp amber `#eda055` | `#93c9c0` | `#74c760` `#f5df63` `#ff6f7a` | ΔE link ↔ green 22.0 (min 20) |

Leaf green can't be the control colour: the gate needs `--l` ≥ 20 ΔE from status green. So greens are status and
plants, and terracotta / lamp amber are the controls. To stay ≥ 20 ΔE from terracotta, light's needs-you red leans rose
(`#a8164f`).

## Open questions for the owner

1. **The scene in dark mode.** The shop art keeps its own colours in both schemes, as herd.css's scenes do today: a warm
   plaster wall and wood counter. Recommend: as mocked. The alternative is an evening palette for dark (walnut wall,
   lamp glow), which means `scene.palette` also takes `{light, dark}`.
2. **The species pick is `n % 6`, not a hash.** That is stable, and neighbouring issues never share a species, so rows
   tell apart best. A hash looks less patterned but sometimes repeats on neighbours. Recommend: `n % 6`.
3. **"A full plant with a split leaf" for every species.** Only the monstera literally splits. I read it as each
   species' mature leaf (philodendron's big heart, alocasia's arrow, and so on). OK?
4. **`blocked` as a seventh stage key.** The issue lists six stages, but "a blocked plant wilts" needs its own sprite.
   Recommend: the optional `stages.blocked`; without it, the blocked action draws over the issue's last stage.
5. **Words not in the issue.** composted (dead), the ledger (Events), the shelves (Lanes tab), resting, ready for sale,
   being tended. Change any you like; they are data.
6. **Dark leaves on garden-dark rows.** Alocasia and calathea are the darkest greens. On the moss background they read
   by the pot and the pale veins, not the leaf outline (see `sprite-sheet-dark.png`). Recommend: as mocked. The
   alternative is a 1 px light outline on row sprites in the dark scheme only.
7. **Row sprites at 1 CSS px per pixel (revision 1).** At 24 px, 2 CSS px per pixel would mean a 12 × 12 grid, coarser
   than the old 16 × 16. So rows use 24 × 24 at 1 CSS px, which is 2 device px on a phone, as the revision asks. The
   lanes use 2 CSS px. Recommend: as mocked.

Not changed: farm, kitchen, terminal, serve.css, herd.css, serve.js. The mockup only links them.
