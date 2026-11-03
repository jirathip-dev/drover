# Kitchen theme: design mockup (#239, phase 1)

This is issue C of #241's plan: the kitchen built to the theme contract (actions, looks, `load_scene: heat`, rail,
moments, row sprites, icons, ascii, modes) plus #244 Q2's whole-board colour scheme. `index.html` is the mockup.
`sheet.html` is the sprite sheet. Both link `assets/serve.css` and `assets/herd.css` unchanged. `theme.css` is the
proposed `themes/kitchen/theme.css`. Every pixel comes from one source, `art.mjs` (pixel maps, one character = one
pixel), so the mockup and the phase-2 theme files can't drift apart.

Open `index.html` over http from the repo root (`python3 -m http.server`; it uses ES modules). Live, the sprites
loop their two frames. With `prefers-reduced-motion` everything is static.

## Files

| file | what |
|---|---|
| `index.html` | the mockup, in 8 sections (below) |
| `kitchen-390.png` (DPR 2), `kitchen-1440.png` | full-page shots of `index.html` |
| `sheet.html`, `sprite-sheet.png` | 8 actions × 3 looks + frame 2, the rail ticket and its red variant, the 5 project trims, the 16 px row sprites at 1× and 3× |
| `icon/icon-{180,192,512}.png`, `icon/favicon-{32,16}.png` (+ `.svg` sources) | the theme icon |
| `theme/sprites.svg`, `theme/scene.svg`, `theme/theme.json` | the phase-2 theme files, generated, filled to #241's keys |
| `art.mjs` | pixel maps, looks, heat colours, the station scene, the Host miniature, the icon, the board scheme |
| `build.mjs` | `node build.mjs` writes `theme/` and `icon/*.svg` |
| `contrast.mjs` | `node contrast.mjs`: WCAG ratios and hue distances, today's board vs the kitchen scheme |
| `board-fixture.html` | the demo export's Queue and Host markup (fake data), used by section 8 |
| `shoot.sh` | regenerates everything (`CHROME=<chrome-headless-shell> docs/design/kitchen/shoot.sh` from the repo root) |

## What the mockup shows

1. **The Herd, 390 px, at calm, busy and burning.** One page per fleet, as today. Each page is that fleet's
   **station**: a kitchen wall with its #162 colour on the hood stripe and the counter trim. Under it, the **ticket
   rail** holds one ticket per queued issue. Then come the bands: *on the line* (working, each chef by role),
   *standing by* (idle), *at the pass* (done, a steel shelf with heat lamps) and *the bell* (blocked). The bands keep the
   farm's order, so `patch()` is unchanged.
2. **1440 px: the stations in a line.** The scene band draws one station per fleet side by side, with the current
   page's lit and centred. Swiping walks along the line. At 390 px only the current station fits.
3. **Fire.** Calm (load/limit < 0.5) has three small blue gas flames. Busy (≥ 0.5) has taller orange flames and steam.
   Held (the spawn gate refuses) has flames up the wall, smoke under the ceiling, a red glow over the floor, and the
   standby chefs fan. Reduced motion shows one static frame. Flames swap 2 frames every 0.6 s, small pixels only. The
   glow eases 75 → 100 % over 4 s. There is no flash of any kind (the farm's storm lightning has no kitchen
   counterpart).
4. **Moments**, silent, about 2 s, then back to the action:
   - **merged:** the chef rings and the plate slides out with a bubble, "order up!";
   - **review FAIL:** the taster sends the plate back ("✗ sent back");
   - **needs you:** the ticket on the rail turns red and flutters ±3° on a 1.6 s cycle.
5. **The other tabs.**
   - **Queue and Events rows:** a 16 px row sprite in the cell the state glyph had (`var(--s4)`), with the plain
     state word as `title` and `aria-label`.
   - **Host:** the MACHINE card header gets the heat scene in miniature (48 × 16), at all three levels.
   - **Lanes `[ascii]`:** the farm's 48 × 40 cell next to the kitchen's glyph over its action word.
6. **The icon:** a pixel chef in a toque tossing a wok over a blue flame, on the board's dark tile `#1f2a38`. It comes
   in 512, 192 and 180, with a 32/16 favicon (toque and face only, so it reads at 16). The section also mocks a Safari
   tab in light and dark, and a home screen with the farm icon next to the kitchen one.
7. **The `[theme]` picker** with kitchen selected, and the one-time re-add hint. #241 builds both; they're shown here
   only for the kitchen's entry and icon.
8. **The whole board in the kitchen scheme: Queue (with its needs-you cards) and Host**, on the demo's real markup,
   with a contrast table.

## The contract, filled (`theme/theme.json`)

| #241 key | kitchen |
|---|---|
| `actions` | impl `chef-cook` (wok toss) · fix `chef-chop` · rev `chef-taste` (spoon, eyes shut) · update `chef-carry` (crate, walking) · idle `chef-wait` (arms crossed) · done `chef-serve` (plate out) · blocked `chef-bell` (rings, waves) |
| extra | `chef-fan`: an idle chef's pose while heat is held |
| `looks` | 8 looks keyed by the farm coat's `--c`, so it's the same `sha256(name)` pick as the coats with no new hash. Each look is a hat (toque / bandana / cap via `--ht/--hb/--hk`), a skin tone (4), hair and an apron colour (4). `--fk` is the project colour on the apron's bib and hem trim |
| `load_scene` | `heat`: calm < 0.5 ≤ busy; held = `gate`. `heat-calm|busy|held` on the page sets `--f1/--f2` (flame) and `--steam`, shared by the scene, the cook's burner and the Host miniature |
| `rail` | `chef-ticket` on its clip; needs-you = `.needs-t` (red header, pink paper, flutter) |
| `moments` | merged `order-up`, fail `sent-back`, needs-you `needs-t` |
| `row_sprites` | `row-cook/chop/taste/carry/wait/serve/bell`, `row-ticket` (queued, red when needs-human), `row-back` (FAIL) |
| `icons` | `icon-180/192/512.png`, `favicon.png` |
| `ascii` | `♨ cook`, `♨ chop`, `♨ taste`, `♨ carry`, `· wait`, `✓ serve`, `! bell`, `✗ out`: ≤ 5 characters, 30 px at the farm's 10 px, so no column reflows |
| `modes` | `scene`, `ascii` |
| `board` | the scheme below |

**Events rows → sprite:** merged → serve; PASS → serve; FAIL → sent back; spawn → the role's action; asks and needs → bell.
Alert, drive and host events keep their glyph.

## The board scheme (#244 Q2)

The scheme is charcoal `#1a1816` with a faint 24 px tile grid, cream text `#efe3cb`, copper `#e89a62` for controls,
links and the active tab, and brass `#c9a55a` for the title, card edges and group names. Status colours stay green
`#6cc96e`, amber `#f4d24e` and red `#ff5c84`. I pushed red toward crimson so it never reads as a copper control.

- **Contrast:** every token is ≥ 5.5 : 1 on the page, the card and a needs-you card (`node contrast.mjs`; AA is 4.5).
- **Hue distance:** red/copper is ΔE 51, red/amber 89, amber/copper 39 (today: 110 / 57 / 116).
- **Two literal colours:** `serve.css` still has `#30363d` (edges) and `#161b22` (fills). If #241 turns them into
  tokens (`--edge`, `--card`; the values stay the same, so terminal stays pixel-identical), a scheme becomes pure
  variables. Until then, `theme.css` covers the selectors that use them.

## Phase 2 (after approval): what the build needs

- **Theme files:** copy `theme/` to `assets/themes/kitchen/`, plus `theme.css` and the icons (it replaces #172's 4
  plain poses). The farm has no `theme.css` and keeps its files, so it stays pixel-identical.
- **herd.js:**
  - resolve the sprite from `actions` (role, then state, then farm);
  - set the look's vars from `looks[coat --c]` and `--fk` from the fleet colour;
  - drive `--o1/--o2` (frames) the way it drives `--gait`, and put `heat-*` on `#herd`;
  - switch idle chefs to `fan` while held;
  - draw the rail from `rail`, and the line of stations at widths over 390;
  - play the moments.
- **Data that #241's per-role resolution needs, and herd.json doesn't carry today:**
  - a lane `stage` (`impl|fix|update|rev`): a fix round's lane has `role: "impl"`. `stage_at()` already computes it;
    about +1 line;
  - the fleet's `color` (+1);
  - for the Lanes avatars' trim, the fleet on the lane row (+1).
- **Sprite size:** `sprites.svg` is 90 KB of `<rect>` runs (the farm's is 9.5 KB); it gzips well. If that matters, the
  builder can merge runs into one `<path>` per colour.
- **Audit:** chefs stay 132 px, tickets 53 px, and the rail sits above the tickets (no overlap with the "post").
  `herd-kitchen` gains calm/busy/held shots and a reduced-motion check (static glow).

## Open questions for the owner

1. **"Each project gets its own kitchen station along the line" (Q5).** I read project = fleet. Each fleet's Herd
   page is its station (as each is its own pasture today), and at 1440 the scene band shows the whole line. The
   alternative is one page with every fleet's station side by side, which drops the per-fleet pages and their swipe.
   **Recommend: as mocked.**
2. **Moments need data `/live` doesn't send.** `/live` emits `frag`, `herd` and `toast` only, with no typed merged
   or verdict-FAIL event, and diffing herd snapshots can't tell a merge from a kill. **Recommend:** #241 adds a short
   `moments` list to herd.json (merged / FAIL / needs-human since the last push, from the event log, about +8 lines).
   It's the contract's `moments` "played from /live events", so it belongs in A, not here.
3. **The re-add hint can't quite do what Q7 says.** The brief has the apple-touch-icon follow the server default, so
   re-adding gives the *server's* theme icon, not the phone's pick. On iOS, a Home Screen app's storage is also
   separate from Safari's, so a pick made in the app isn't the one Safari sees. **Recommend:**
   - swap `<link rel=apple-touch-icon>` live to the phone's theme as well as the favicon (JS only), so re-adding from
     Safari after picking there captures the chosen icon;
   - keep the manifest on the server default (Android);
   - show the hint only in the installed app, once, when its theme differs from the one recorded at its first launch,
     with the text as mocked ("In Safari: Share → Add to Home Screen").
4. **Blocked and dead have no word in Q6.** Q6 lists six words, none for blocked. I used `bell` for blocked and `out`
   for dead. OK?
5. **Herd labels.** A working chef's label shows the action word (`♨ cook · 70s`, `♨ chop · fix 2`) instead of
   `working`. Idle and done keep today's wait and PR text. Band names come from `words` (tickets, on the line,
   standing by, at the pass, the bell): 5 new keys and a one-line lookup in herd.js. **Recommend: as mocked.**
6. **Done and blocked styling differs from the farm.** The farm dims a done horse and tints a blocked one red. A
   serving chef isn't dimmed, and the bell chef keeps its colours (the red band and label say it). **Recommend: as
   mocked.**
7. **Fleet colours vs the scheme.** #162's fleet colours are the accent tokens `--l --p --c --g --a`, and `--l` is
   also the control colour. Under the kitchen scheme a "blue" fleet turns copper, the same as the controls, and fleet
   colours differ between phones on different themes. **Recommend:** #241 gives fleet colours their own 5 tokens
   (same values today), so a fleet looks the same on every theme and never matches the controls.
8. **Night (`[A]` light).** The kitchen is indoors, so I didn't design a night look. **Recommend:** the kitchen
   ignores day/night and hides `[A]`, or keeps it and dims the wall tiles. Your pick.
9. **Q4's "nothing scary".** Held is the strongest state: wall flames on both sides of the range, smoke and a red floor
   glow. It never flashes. If it still reads as too much, the first thing to drop is the wall flames.
