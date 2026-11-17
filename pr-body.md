Closes #1092

## For humans
The Herd scene is back on the phone page: open [⚙] → Herd. It draws the scene from the pinned swag theme pack, with one figure per lane in state bands and the queue at the gate.
[‹] [›] switch repo, [☾] switches night, and the sky tints with host load (windy, fog, storm).
A theme of your own can bring its own scene. Farm and terminal have no scene file, so they get a simple drawn ranch.

## Parity
- Row 61 (Herd scene: per-repo ranch, state bands, queued at gate): restored by this PR.
- Row 62 (scene weather from host load, day/night, repo switching): restored by this PR.
- docs/parity.md still lists both as `open #1092` with no `parity_61`/`parity_62` test; flipping them to `kept` needs those named tests, so that is left to the parity sweep (the Herd's coverage is `--test herd` here).
- No other row changed or dropped.

## What changed
- `crates/server/src/screens.rs`: `GET /screens/herd`, built from the existing Herd read: one `scene` block with `weather` (sky/wind/line from host load, pressure, spawn gate) and `repos` (each fleet, then `others`: queued at the gate, lanes in working/idle/done/blocked bands as rows with figures). The page gets a [⚙] menu row; this is page-only, and cooee's manifest is unchanged.
- `web/src/herd.ts`: the `scene` renderer. It inlines `themes/<theme>/scene.svg`. Serve resolves that as own theme dir, then the vendored swag pack, otherwise 404. On a 404 it draws a small ranch. It also has [‹] [›] [☾] (kept on the phone), and the band rows use the existing renderers. `blocks.ts` exports its renderer table.
- Tests: golden `tests/golden/cooee/herd.json` (demo fleet export); `herd.rs`: the pinned-swag test (served kitchen/garden `scene.svg` match `SHA256SUMS`, SOURCE pins a 40-hex commit, farm 404s, a custom `themes/kitchen/scene.svg` overrides) and the WebKit test (open Herd from [⚙], 1 figure in sendmeter, kitchen's scene, [☾] → night, [›] → synergy with 2 figures).
- CI: the ui-webkit job runs `--test herd`, and changes to `herd.rs` trigger it.
- docs/serve.md: `/screens/herd`.

src/ net +81 (budget: none stated). That is the screen builder only; the drawing lives in web/.

## Evidence (head a2eeac3e9b13f7f10411ad8a55d2550dd835ad9b)
Fifth staging merge (#953, #1169): ci.yml takes staging's `ui/(drafts|repaint)\.mjs` trigger with herd.rs kept in it; main.ts takes staging's `morph` import beside `import './herd'`; web/dist rebuilt from the merged src. At a2eeac3e9b13f7f10411ad8a55d2550dd835ad9b: `npm run check && npm run lint && npm run build` → 0; `cargo fmt --check` → 0; CI's two clippy commands → 0 each; `drover cap-check --config ci/cap.toml` → 0 (119 lines headroom); `cargo test --locked -q --test herd --test page_drafts --test runner_limits --test cooee_screens --test demo_export` → 0; `cargo test --locked -q --workspace` → 101, only `answer_delivery` (9), which reads the lane's DROVER_OUTBOX/DROVER_HOST; `env -u DROVER_LANE -u DROVER_OUTBOX -u DROVER_HOST cargo test --locked -p drover --test answer_delivery` → 0 (9 passed).

Fourth staging merge (a064bd12e8e0ffa1ab8928ebb383b1594380f79c):
Fourth staging merge (#1186, #1181, #1101, …): screens.rs and main.ts keep both sides' imports; herd.ts moves to staging's strict `Blk` (`blks`/`obj`, `nodes()` in place of innerHTML, no non-null `!`); web/dist rebuilt from the merged src. At a064bd12e8e0ffa1ab8928ebb383b1594380f79c: `npm ci && npm run check && npm run lint && npm run build` → 0 each; `cargo fmt --check` → 0; CI's two clippy commands (`--lib --bins`, `--tests` with its -A list) → 0 each; `drover cap-check --config ci/cap.toml` → 0 (124 lines headroom); `env -u DROVER_LANE cargo test --locked -p drover --test herd|demo_export|cooee_screens|ui_tokens|page|page_drafts` → 0 each (7, 1, 3, 1, 13, 3 passed); `arch-check --base origin/staging` (built binary) → 1, only `just gen` exiting 127 (no `just` on this host).

Round 2 (form-only, at 1a5e50a3245647f372218c631e6f315766c827f8): PR body gained `## Parity`.

Third staging merge (1a5e50a3245647f372218c631e6f315766c827f8):
Third staging merge (#1086, #1145, #1136): ci.yml keeps #1086's threads_tab step and its trigger paths, with herd.rs added to the trigger; board.css keeps #1086's focus-screen and image-viewer rules beside the Herd rules; web/dist rebuilt from the merged src. At 1a5e50a3245647f372218c631e6f315766c827f8: `cargo fmt --check` → 0, clippy → 0, cap-check → 0 (45683 / 47200), `npm run check` → 0, `npm run build` → 0, `--test herd|demo_export|cooee_screens|ui_tokens|page|page_drafts` → 0 each.

Second staging merge (29c2c56034dea9f966f65b0b434ac2f17e572b3f):
Second staging merge: ci.yml keeps staging's parity_webkit trigger and steps beside the herd ones; web/dist rebuilt unchanged. At 29c2c56034dea9f966f65b0b434ac2f17e572b3f: `cargo fmt --check` → 0, clippy → 0, cap-check → 0, `npm run check` → 0, `--test herd|demo_export|cooee_screens|ui_tokens|page|page_drafts` → 0 each.

First staging merge (f296dd93583aed211a44b7ebc0bd1f2b48b34502):
After merging origin/staging (#1084): ci.yml keeps #1084's page_drafts step and trigger beside the herd ones; main.ts keeps `sent` and `import './herd'`; web/dist rebuilt. At f296dd93583aed211a44b7ebc0bd1f2b48b34502: `npm run check` → 0, `cargo fmt --check` → 0, clippy → 0, cap-check → 0, `--test herd|demo_export|cooee_screens|ui_tokens|page|page_drafts` → 0 each.

Before the merge (d4f40ae7a771da8197c5be8547b80c3caa5f7ccd):
- `cargo fmt --check` → 0; `cargo clippy --all-targets --locked -- -D warnings` → 0
- `drover cap-check --config ci/cap.toml` → 0 (45508 / 47200)
- `drover arch-check --base origin/staging` → 1, its only violation is `just types` exiting 127 because `just` isn't installed on this host (CI has it; no api.rs type changed)
- `cd web && npm run check && npm run build` → 0, dist committed
- `cargo test --locked -p drover --test herd|demo_export|cooee_screens|ui_tokens|page` → 0 each
- `cargo test --locked --no-fail-fast` (run at c91338021) → 101. Its 2 failures:
  - `ui_tokens` (raw `188px`): fixed in d4f40ae, now 0.
  - `self_update::pause_and_resume_are_logged_and_pause_stops_the_job`: the lane's `DROVER_LANE` env leaks into it. `env -u DROVER_LANE cargo test --test self_update` → 0.
  - Earlier run: `drive::drive_now_on_the_board_is_paired_only_and_toasts_its_line`, a wall-clock flake ("0.1 s" vs "0 s"). It passed 3/3 on rerun.
- WebKit: Playwright 1.65 on this host has no matching WebKit build, and installing browsers isn't allowed. The same herd test passed on Playwright Chromium via a temporary, uncommitted change to the helper. Real WebKit proof is the ui-webkit CI job.

## Why (round 2)

Decided: merged origin/staging; screens.rs and main.ts keep both sides' imports (Herd types and './herd' beside staging's Json, blks, isBlk, nodes)
Because: crates/server/src/screens.rs:6
Decided: herd.ts reads the scene block through staging's strict Blk helpers (blks, obj) and draws the swag svg with nodes() like el() now does
Because: web/src/herd.ts:20
Decided: the scene cache fetch returns its svg directly, so herd.ts has no non-null `!` for staging's ts-lint
Because: web/src/herd.ts:7
Rejected: hand-merging web/dist — it is rebuilt from the merged src with npm run build
Check: npm run check, lint and build exit 0; --test herd passes 7/7; ui-webkit runs the herd test at a064bd12
Owner: The Herd branch is caught up with staging again; the scene works the same, only its code follows the page's new stricter types.

<sub>note on a064bd1 · git log --notes=why</sub>

