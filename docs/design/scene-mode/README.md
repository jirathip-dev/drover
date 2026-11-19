## Summary

Decision: the header gets a `[scene]` button that swaps the whole page between today's screens and Scene, which is the old Herd brought back in pixel only: the same ranch, horses, bands, gate, weather and animations. The header stays, and the tab bar becomes the weather line. `[A]`/`[☀]`/`[☾]` sits beside `[scene]` and shows only in Scene. Each phone remembers both choices. The Herd sheet goes. Scene keeps its pixel horses whatever the board style is (#1306).
Screens: Scene by day, Scene by night and the screens view with `[scene]` off. Each Scene image sits beside the old Herd at `c34235855^`, and `clip.gif` shows the motion.
Open questions: none

## Images

| Image | What it decides |
|---|---|
| `scene-day-390.png`, `scene-day-1440.png` | Scene by day, windy. The header stays: `drover`, demo, the dot, `[scene]` pressed, the light button `[☀]`, `[↻]` and `[⚙]`. Below it is the old Herd page, unchanged: the repo strip (`acme-api · 6 here` and counts), the repo's needs-you card, the ranch strip with `[‹]` `[›]` and the repo dots, the paddock gate with its bales, then the bands of horse cards (on the trail, grazing, in the stable, at the gate). At 1440 it keeps the old wide grid of equal columns. The weather line sits where the tab bar was. |
| `old-herd-day-390.png`, `old-herd-day-1440.png` | The old Herd at `c34235855^` with the same data, for comparison. Everything below the header is the same drawing, only moved down by the header. The difference is the frame: the old Herd covered the header and put `[☀]` and `[×]` in its own strip. |
| `scene-night-390.png`, `scene-night-1440.png` | Scene at night (`[☾]`): the night sky with stars, the Milky Way and the moon, and the night colours. The horses are the same pixel sprites. |
| `old-herd-night-390.png`, `old-herd-night-1440.png` | The old Herd at night, same data, for comparison. |
| `screens-390.png`, `screens-1440.png` | Screens mode: the board as it is today, with `[scene]` not pressed and no light button. Nothing else on the board changes. The settings sheet loses its `Herd` row (Scene is reached only from the header) and gets no style row: that is #1306. |
| `clip.gif` (390, 12 fps) | The motion. First paint: the loading horse trots ("saddling up…"). By day: the tree canopy sways and the grass bends on a shared 5.2 s gust (faster when host load is high), clouds drift, and working horses trot (0.4 s stride) and roam. By night: the stars drift, clouds pass and the horses keep trotting. |

## Behaviour

- **Toggle:** `[scene]` is a pressed/unpressed header button (`aria-pressed`). It is never a sheet. The choice is kept per phone, so a reload opens where you left off, at any width. Scene fills the page under the header. `[↻]` and `[⚙]` keep working.
- **Light:** `[A]` → `[☀]` → `[☾]`, kept per phone. Auto is night before 07:00 or from 19:00 by the phone's clock, checked every minute and on resume (old #71).
- **Repos:** one page per repo, Others last. Swipe, `[‹]` `[›]`, the dots, ←/→, a wheel or a drag all switch it, and the last repo is remembered (old #291).
- **Weather:** host load sets the wind (canopy, grass and clouds ×1–3). The spawn gate refusing brings storm (rain, lightning at most every 20 s, manes blown). Memory pressure brings fog. Reduced motion keeps the tint only.
- **Taps:** a horse opens its lane sheet, and a needs-you horse opens its card. A bale opens its queued line.

## Data and art (for impl)

- The data is `/screens/scene` (the current `/screens/herd` renamed). Its `scene` block is `fixture.json`'s shape, which already matches `herd_screen` in `crates/server/src/screens.rs`, plus four things: each row's `figure.act` (impl, fix, rev, update, idle, done or blocked, which picks the action sprite), `figure.coat` (one of Corral's 8 coats), each repo's `needs` cards, and the block's `art` pin. No `/herd.json`, and no old board route comes back.
- The art is swag's **`legacy-board/farm`** at the pinned `68c01f0e…`, checked against its `SHA256SUMS`. `packs/` cannot be used yet: at that commit it holds only cooee's row sprites (`s-edit`, `s-run`, …), with no horse action poses, no loading horse and no `rail` bale. The scene backdrop (hills, barn, trees, fence, grass, night sky) is the old procedural drawing in code, as at `c34235855^`. A theme with a `scene.svg` (kitchen, garden) draws that instead, as before.
- Impl note: the old scene layer's class `.rs` now clashes with the board's row-subtitle `.rs`, so this mockup calls it `.rsn`.

## Files

`index.html` (query: `light`, `mode=screens`, `old=1`, `loading=1`, `t=<s>` for a frozen frame), `scene.js` (design-only port of the old `herd.js` drawing and its frame loop), `scene.css` (the old `herd.css` plus the header toggle and light button), `fixture.json`, and `shoot.sh`. Running `docs/design/scene-mode/shoot.sh` renders every PNG and the clip, using Playwright's chrome-headless-shell plus `gif.mjs`, a no-dependency PNG→GIF encoder. The mockup reads the farm art from `assets/swag/farm/`, the vendored copy of the same pinned commit.
