## Summary

Decision: the header gets a `[scene]` button that swaps the whole page between today's screens and Scene, which is the old Herd brought back: the same ranch, horses, bands, gate, weather and animations. The header stays; the tab bar becomes the weather line. `[A]`/`[☀]`/`[☾]` sits beside `[scene]` and shows only in Scene. Each phone remembers both. The Herd sheet goes.
Screens: Scene in pixel (day), Scene in ascii (night), and the settings sheet with its style choice back (pixel | ascii). Each is shown beside the old Herd at `c34235855^`, plus `clip.gif` of the motion.
Open questions: (1) In ascii, the old Herd kept its pixel horses. Should Scene draw the theme's ascii horses (as drawn here) or keep the pixel ones? (2) swag has no ascii frame for a queued bale, so `[#]` stands in. Should swag add one?

## Images

| Image | What it decides |
|---|---|
| `scene-pixel-390.png`, `scene-pixel-1440.png` | Scene mode, pixel style, daylight, windy. The header stays (`drover`, demo, dot, `[scene]` pressed, light `[☀]`, `[↻]`, `[⚙]`). Under it is the old Herd page unchanged: the repo strip (`acme-api · 6 here`, counts), the repo's needs-you card, the ranch strip with `[‹]` `[›]` and the repo dots, the paddock gate with bales, then the bands (on the trail, grazing, in the stable, at the gate) of horse cards. 1440 keeps the old wide grid of equal columns. The tab bar's place holds the weather line. |
| `old-herd-pixel-390.png`, `old-herd-pixel-1440.png` | The old Herd at `c34235855^`, same data, for side-by-side: everything below the header is the same drawing, only shifted down by the header. Only the chrome differs: the old one covered the header, with `[☀]` `[×]` in its strip. |
| `scene-ascii-390.png`, `scene-ascii-1440.png` | Scene mode, ascii style, night (`[☾]`): the night sky (stars, Milky Way, moon) and night palette. Each horse is the theme's 3-line ascii figure (`ascii.<act>`) in the 132 × 100 slot. A working horse gallops in two frames (legs `/` `\` swapped every 0.25 s) and roams like the pixel one. The loading mark runs `ascii_loading`. |
| `old-herd-ascii-390.png`, `old-herd-ascii-1440.png` | The old Herd at night with the ascii style picked: it kept pixel horses (ascii changed only rows, lanes, host and loading). This is open question 1. |
| `look-sheet-390.png`, `look-sheet-1440.png` | The board in screens mode (`[scene]` not pressed, no light button) with `[⚙]` open. **Style** comes back after theme: `pixel` (an impl horse) or `ascii` (`~h=`). It applies in place to Scene, rows, lanes, host and loading. The **screens** section loses its `Herd` row; Scene is reached only from the header. |
| `clip.gif` (390, 12 fps) | The motion. Pixel by day: the loading horse trots ("saddling up…"), then the tree canopy sways and the grass bends on the shared 5.2 s gust phase (faster with host load), clouds drift, and working horses trot (0.4 s stride) and roam. Ascii by night: the `ascii_loading` frames, then the stars drift, clouds pass and the ascii horses gallop. |

## Behaviour

- **Toggle:** `[scene]` is a pressed/unpressed header button (`aria-pressed`). It is never a sheet. The choice is kept per phone, so a reload opens where you left off, at any width. Scene fills the page under the header. `[↻]` and `[⚙]` keep working.
- **Light:** `[A]` → `[☀]` → `[☾]`, kept per phone. Auto is night before 07:00 or from 19:00 by the phone's clock, checked every minute and on resume (old #71).
- **Repos:** one page per repo, Others last. Swipe, `[‹]` `[›]`, the dots, ←/→, a wheel or a drag all switch it, and the last repo is remembered (old #291).
- **Weather:** host load sets the wind (canopy, grass and clouds ×1–3). The spawn gate refusing brings storm (rain, lightning at most every 20 s, manes blown). Memory pressure brings fog. Reduced motion keeps the tint only.
- **Taps:** a horse opens its lane sheet, and a needs-you horse opens its card. A bale opens its queued line.

## Data and art (for impl)

- The data is `/screens/scene` (the current `/screens/herd` renamed). Its `scene` block is `fixture.json`'s shape, which already matches `herd_screen` in `crates/server/src/screens.rs`, plus four things: each row's `figure.act` (impl, fix, rev, update, idle, done or blocked, which picks the action sprite or ascii figure), `figure.coat` (one of Corral's 8 coats), each repo's `needs` cards, and the block's `art` pin. No `/herd.json`, and no old board route comes back.
- The art is swag's **`legacy-board/farm`** at the pinned `68c01f0e…`, checked against its `SHA256SUMS`. `packs/` cannot be used yet: at that commit it holds only cooee's row sprites (`s-edit`, `s-run`, …), with no horse action poses, no `ascii`, `ascii_rows` or `ascii_loading`, and no `rail` bale. The scene backdrop (hills, barn, trees, fence, grass, night sky) is the old procedural drawing in code, as at `c34235855^`. A theme with a `scene.svg` (kitchen, garden) draws that instead, as before.
- Impl note: the old scene layer's class `.rs` now clashes with the board's row-subtitle `.rs`, so this mockup calls it `.rsn`.

## Files

`index.html` (query: `style`, `light`, `mode=screens`, `sheet=look`, `old=1`, `loading=1`, `t=<s>` for a frozen frame), `scene.js` (design-only port of the old `herd.js` drawing and its frame loop), `scene.css` (the old `herd.css` plus the header toggle, ascii figures and style rows), `fixture.json`, and `shoot.sh`. Running `docs/design/scene-mode/shoot.sh` renders every PNG and the clip, using Playwright's chrome-headless-shell plus `gif.mjs`, a no-dependency PNG→GIF encoder. The mockup reads the farm art from `assets/swag/farm/`, the vendored copy of the same pinned commit.
