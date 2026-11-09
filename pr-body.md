Closes #761

## For humans
The code-size limit goes from 36,740 to 38,028 lines, as the owner decided (q967).
That is room for every open PR and queued issue, plus a 500-line margin, so the queue (#734 first) can move again.

## What changed
- `ci/cap.toml`: `line_cap = 38028` (nothing else in the file changes).
- `docs/cap-history/2026-11-09-38028.md`: records the raise, q967 and the `herdsman cap herdsman` numbers.

`src/ net +0 (budget +0)`: config and docs only.

## Evidence (head a0c264bfaa1ff794dbf89ed67a0f2787752baf2a)
`cargo build --release` then `herdsman cap-check --config ci/cap.toml` → exit 0:
```
src lines (formatted): 36681 / cap 38028
web lines (TypeScript, soft budget, not capped): 882
```
