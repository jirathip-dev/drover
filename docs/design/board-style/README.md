## Summary

Decision: the [⚙] settings sheet gets a **style** group (terminal | ascii | pixel) under theme and colours, kept per phone; only each row's figure slot changes, the JSON rows, words and badges stay as they are.
Screens: the three tabs (Queue, Events, Lanes) in each style; the settings sheet with Hosts, the loading strip and moments in each style; the old board next to today's board and the proposal.
Open questions: (1) Should a new phone start on pixel (today's sprites, now moving) or terminal? I recommend pixel when the theme has art, terminal otherwise. (2) The terminal *theme* has no art, so I'd show ascii and pixel greyed out under it. OK?

# #1306 board style: terminal | ascii | pixel

Garden theme, garden-light colours, the demo's data. The pages link the real `web/src/board.css`; `proposed.css` holds
only the rules the board would add. The art is loaded at render time from the pinned swag files
(`assets/swag/garden`, `jirathip-dev/swag legacy-board at 68c01f0`); the mockup copies no sprite art.

| image | what it shows |
|---|---|
| `tabs-1440.png`, `tabs-390.png` | **Queue, Events and Lanes in each style** (side by side at 1440, stacked at 390). **terminal** is today's glyph look (`● ! ✗ ✓ ○`). **ascii** puts the theme's 3-character `ascii_rows` figure in the 24 px slot, and a Lanes avatar shows the multi-line `ascii` figure (a working lane swaps its legs in two frames, #334). **pixel**: a Queue row shows its issue's plant at its stage (the species is picked by issue number, #423). An Events row shows the action's sprite, with `row-sold` / `row-fall` / `row-thirsty` for merged, FAIL and needs-you. A Lanes avatar shows the keeper doing the role's action over the issue's plant, in the look picked by the lane's #292 hash. Every sprite loops its `kf1`/`kf2` frames (`kfr`, 1.2 s); the frame strip at the top of the pixel column shows both frames. |
| `look-1440.png`, `look-390.png` | **The settings sheet's style group**: three rows, each with its own preview and a ✓ on the current pick. A tap repaints in place: no reload, kept in localStorage `style`. Each column then shows what that pick draws. **Hosts sheet:** a load miniature before each host (pixel: `light-sun` / `light-overcast` / `light-rain`; ascii: `ascii_load`; terminal: none), with the level's theme word as its tag. **Loading strip:** terminal `↻`, ascii the two `ascii_loading` frames, pixel the `loading` sprite with the theme's loading word. **Moments** (paused mid-play): merged hops, FAIL shakes, needs-you pulses and tints the row, each with the theme word as a tag. When the issue's row is off-screen, the `#mo` mark hops in the corner. Terminal has no moments, as before. `prefers-reduced-motion` freezes all of it on frame 1. |
| `compare-1440.png`, `compare-390.png` | **Comparison.** The old board (c34235855~1, built and exported with `--demo`) in pixel and ascii (Queue, Lanes). Today's board in garden: its sprites are already drawn but stand still, with no keeper and no host miniature. Then the proposal. The raw old shots are in `old/`. |

## What the rows must carry (for impl's test)

Today's screen JSON already gives a Queue row `figure: {state, grow, tool}`. The art also needs:

- **Queue row:** the issue number, to pick the plant's species. Today it is only inside `title`.
- **Lanes row:** `tool` (which action), `grow` (the issue's stage) and the issue number for the plant, plus the lane key for the look hash. Today the figure is `{coat, state}` only.
- **Events row:** the event's kind (merged, fail, needs, or the lane action), so the right sprite and moment play. Today it is `{state}` only.
- **Hosts row:** a load level (`calm` | `busy` | `held`). Today there is none.

## Rendering

`shoot.mjs` (Chrome headless shell over the DevTools protocol, localStorage set before load):
`python3 -m http.server 8463` from the repo root, then for each page `node shoot.mjs <chrome-headless-shell>
http://127.0.0.1:8463/docs/design/board-style/<page>.html <out>.png <1440|390>`. The 390 shots use DPR 2.
