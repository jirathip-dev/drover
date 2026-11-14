995
a92f855421e390a3f1d2995b72ea9b1e7074c687
Closes #969

## For humans

After a design is revised, the board's approve card now shows the new round's pictures instead of the old ones. Verify screenshots work the same way.
An older card keeps showing the pictures from its own round, even after a later round replaced those files.

## What changed

- `assets/sw.js`: `design/` and `shot/` requests always go to the network, never `caches.match` / `caches.put`.
- Card image srcs carry their round: `/design/<fleet>/<n>/<i>/<ask id>` (`screens.rs`, `web/src/needs.ts`) and `/shot/<fleet>/<n>/<i>/<verdict sha>` (`needs.ts`; new `NeedIssue.shots_at`, types regenerated).
- The routes in `serve.rs`: a design URL with an ask id serves that ask's file at its `design-review` sha. A shot URL with a sha serves the last verdict at that sha. Both read the file with `git cat-file --batch` (spec on stdin, after a `runner::answers` disk probe) and fall back to the disk file. A URL with no round still serves the latest ask's or verdict's file from disk, as before.
- `verify::served` takes an optional sha.

Decided: `git cat-file --batch` instead of `git show`. Screenshots are usually untracked, and `git show` would print an error to serve's stderr on every request. With `--batch` the sha also never goes into argv.

## Repro

Commit 1 (46f49a8) adds only `crates/cli/tests/round_images.rs` and `tests/support/sw.mjs`. That test runs the real `sw.js` under node against the real server and drives two rounds each of design and verify.
- On base + commit 1: `cargo test -p drover --test round_images` → exit 101. Round 2's card returns round 1's bytes from the SW cache, in both tests.
- At head 2265d47: same command → exit 0.

## Evidence at a92f855421e390a3f1d2995b72ea9b1e7074c687

- `cargo test --locked -p drover --test round_images --test design_stage --test verify_stage` → 0
- `env -u DROVER_LANE cargo test --locked --no-fail-fast` → 101, only for `host_deadline::an_ssh_that_never_returns_costs_one_deadline_and_drive_still_starts` (a wall-clock deadline test under full-suite load). `env -u DROVER_LANE cargo test --locked -p drover --test host_deadline` → 0. Every other test binary passes.
- `cargo fmt --check` → 0; `cargo clippy --all-targets --locked -- -D warnings` → 0
- `drover arch-check --base origin/staging --pr-body <this body>` → 0 violations (fixes the CI red: `docs/serve.md` now names `shots_at`); `drover cap-check --config ci/cap.toml` → 0
- staging merged in at this head.

src/ net +40 (budget not stated): the round-keyed routes and the git read helper.

Also updated `design_stage.rs` / `verify_stage.rs` src assertions to the round-keyed srcs (the spec changes those srcs). The comment lines in commit 1's test were dropped in commit 3, because arch-check refuses new comments; the test logic is unchanged.

## Why (round 2)

Decided: sw.js lets design/ and shot/ go to the network, never caches.match or caches.put them
Because: assets/sw.js:16
Decided: card srcs end in the ask id (design) or the verify verdict's sha (shot); a URL without one still serves the latest round from disk
Because: "Old-style URLs (no round) still resolve to the latest ask's file"
Decided: a round URL reads the file at that round's commit with `git cat-file --batch` (spec on stdin), falling back to the disk file
Because: crates/server/src/serve.rs:565
Rejected: `git show` in argv — it prints "path does not exist" to serve's stderr on every shot (screenshots are untracked) and puts a URL segment in argv
Decided: fix round: docs/serve.md's `GET needs` row names `shots_at`, the arch-check stale-doc finding
Because: docs/serve.md:380
Check: cargo test -p drover --test round_images fails on base 46f49a8^ + test only, passes at head
Owner: design and verify pictures on cards now always show the round the card is about, never an older cached one

<sub>note on a92f855 · git log --notes=why</sub>

