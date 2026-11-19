Closes #1415

## For humans
The Herd screen now tells themes how loaded the host is in one word: calm, busy or held.
It uses the same rule the other load themes already follow: calm below half the load limit, busy from half or when memory is tight, held when new lanes are refused.
Themes may now use a "sign" style for the host load (the bush theme's Fire Danger Rating sign): moderate, high, catastrophic.
Themes that don't use the sign look exactly as before.

## What changed
- `herd_screen` (crates/server/src/screens.rs) sends `"load": "calm" | "busy" | "held"` in the scene block. One match picks both `sky` and `load` from the same load/limit ratio, memory pressure and gate. Per **DECISION q2384** plus the issue's pressure clause: held = gate set; busy = load/limit ≥ 0.5, or memory pressure at warn or critical; calm = everything else. `weather` is unchanged.
- Theme check (serve.rs) accepts `load_scene.kind = "sign"` with the shared calm/busy/held `levels`; its error text lists `sign`.
- `docs/themes.md` documents `sign` with calm → moderate, busy → high, held → catastrophic, plus the q2384 thresholds. `docs/serve.md` names the `load` field.
- Golden `crates/cli/tests/golden/cooee/herd.json` gains only `"load": "calm"`.
- The unit test is in `crates/server/src/herd_tests.rs` (`#[path]` from screens.rs), so screens.rs stays under its 2000-line limit (1999 after merging staging).
- Merged `origin/staging` into the branch.

src/ net +43 (budget: none stated): +10 production, +33 the herd load-level unit test.

## Evidence (head 1a68e32adf55a6e0877a2bc90f30753a4ea7b220)
Round 2 (review FAIL: pressure warn/critical under 0.5 read calm) — now busy; the warn arm with the 0.5 guard is gone.
- `cargo test -p drover-server herd`: exit 0 (`the_herd_scene_sends_the_load_level_of_the_host` ok: calm 0.4, busy 0.5 and 1.3, held with the gate, busy with memory warn at 0.1)
- `cargo test -p drover --test cooee_screens`: exit 0 (25 passed; golden herd.json unchanged, `"load": "calm"`)
- `cargo clippy -p drover-server --all-targets -- -D warnings`: exit 0
- `cargo fmt --check`: exit 0
- src/ net now +42 (one line removed this round).
