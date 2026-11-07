Closes #626

## For humans
herdsman can now hear from GitHub right away instead of waiting for its next poll. When CI finishes or a PR changes, GitHub calls one public web address. herdsman checks the call's signature, refreshes that PR and wakes the loop for that issue.
To turn it on, the owner saves the webhook secret, runs the one `tailscale funnel` line from docs/serve.md and adds the webhook in GitHub. Only that one web address is public; the board stays on the tailnet.
While webhooks are on, the background PR check slows to every 10 minutes. It still catches anything GitHub missed while the Mac slept.

## What changed
- **Funnel listener** (`crates/server/src/hooks.rs`, outside the frozen board files: `hooks::serve` starts it, then runs the unchanged `serve::serve`; `herdsman serve` and `herdsman service --board` call it): serve opens a second listener at `[serve] hooks_addr` (default `127.0.0.1:7778`; `""` turns it off; a `--read-only` serve doesn't open it). It answers `POST /hooks/github` only. Any other path gets a bare 404 and `GET /hooks/github` gets 405. A `Content-Length` over 25 MB gets 413 before any of the body is read, and a missing `Content-Length` gets 411. At most 4 deliveries run at once.
- **Signature + delivery** (`crates/core/src/webhook.rs`): checks `X-Hub-Signature-256` (HMAC-SHA256 of the raw body) with `hkdf::hmac` (hkdf is already a dependency, so nothing new) and `verify_slice`, which compares in constant time. The secret is `secret:GITHUB_WEBHOOK_SECRET`: a fleet's own, else `all`'s.
  - No secret: 503.
  - Bad or missing signature: 401. The body is never parsed; the only thing kept is a counter file (`webhook.rejected`).
  - `ping`: 200.
  - `pull_request`, `check_suite`, `check_run`, `status` or `push` naming an open issue of a fleet whose repo matches the payload and whose secret signed it: drops the cached PR read, re-reads that PR into the cache `herdsman pr` reads, marks the fleet's open-PR snapshot stale and calls `drive::kick(…, "webhook")`. The issue is matched by its branch, or by a PR number (a merge push's trailing `(#N)`). The board's snapshot then gets a new version, so serve re-renders at once instead of at its 15 s refresh.
  - Anything else: 202, ignored.
- **Catch-up poll**: new `[limits] webhook_poll_minutes` (default 10). In the fast step, a fleet with a webhook secret reuses its open-PR snapshot until it is that old. A delivery makes it stale at once. Kicks and fleets with no secret keep the per-minute ETag probes.
- **doctor**: `! webhook — no secret → herdsman secret request …`, or `✓ webhook — secret set`, plus `· N refused (bad signature)` when any were refused.
- **docs/serve.md**: new "GitHub webhooks" section with the exact line `tailscale funnel --bg --https=10000 --set-path /hooks/github http://127.0.0.1:7778/hooks/github`. It is on its own HTTPS port because Funnel publishes a whole port. Also `fleet.example.toml` keys.

Decisions: a separate listener rather than a route on the board, so nothing Funnel maps can reach the board. I hand-checked the signature with the existing hkdf→hmac re-export instead of adding a dependency.

src/ net +286 (budget: none stated in the issue): the listener, the signature check and the catch-up poll. Cap: 31779 / 35500 after merging staging, which raised `line_cap` to 35,500 (#658, owner q793). An earlier commit trimmed 13 lines of this PR's own code without changing behaviour.

Fix round 1: `board-freeze` was red because the listener lived in `crates/server/src/serve.rs` + `serve/hooks.rs`. It moved to `crates/server/src/hooks.rs` and `serve.rs` is byte-identical to staging; `git diff origin/staging --name-only | scripts/board-freeze.sh .brief.md` → exit 0.

## Tests (`crates/cli/tests/webhook.rs`)
- a signed `check_suite` completed: PR cache goes pending → green, one `gh pr view`, the drive kick for #7 is launched, the board version moves, and `herdsman pr` right after shows `ci green` with no `gh` call of its own
- missing / wrong-secret / other-body / zero / `sha1=` / empty signatures → 401 each, no `gh` call, cache untouched, counter = 6 shown by doctor
- unknown repo and unknown branch → 202, no calls; `ping` → 200
- no secret → 503, doctor `! webhook — no secret`
- GitHub's documented example signature verifies, and a changed body or upper-case hex fails
- real TCP Funnel listener: `/`, `/live`, `/api/v1/board`, `POST /`, `/send`, `/pair/request`, `/hooks/github/../../`, `/hooks/github?x=1`, `/a/app.js` → 404; GET hook → 405; 26 MB Content-Length → 413; signed ping → 200. docs' single `tailscale funnel` line maps only `/hooks/github` to the default listener
- catch-up: with a secret, the fast steps at +1, +2 and +9 min make no `gh` reads; +10 min re-queries; a delivery forces the next step to re-query; without the secret the probes return

Fix round 2: CI `rust` was red on the line cap (31632 / 31500). Staging has since raised the cap to 35,500 (#658), so merging `origin/staging` in fixes it. No source change in this PR.

## Evidence (head ff079345880d0be16d3435ddd7124c7e73aef55a)
Run at ff079345880d0be16d3435ddd7124c7e73aef55a (staging 2b96fbfae merged in) with `CARGO_TARGET_DIR=$HERDSMAN_SCRATCH_TARGET/target`.
- `cargo fmt --check` → exit 0
- `cargo build --release --locked` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (31779 / 35500)
- `herdsman lesson-check` → exit 0
- `git diff origin/staging --name-only | bash scripts/board-freeze.sh .brief.md` → exit 0
- `cargo test --no-fail-fast -p herdsman --test webhook --test doctor --test api_v1 --test cloud` → exit 0, 35 passed, 0 failed
- Full `cargo test --no-fail-fast` → could not finish locally at this head. The live `herdsman check` scratch sweep deleted the lane's scratch target mid-run three times; each time, 117–157 tests had passed with 0 failures before the binaries vanished. The shared lane target was clobbered by other lanes' concurrent builds. CI runs the full suite on this head.

Not verified here: `just html-lint` (`just` isn't installed in this lane; no `crates/server/src` change in this round), and AC3 live (needs the owner's secret, the Funnel line and the GitHub webhook).
