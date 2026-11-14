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
- At head 5749779: same command → exit 0.

## Evidence at 574977913872f39515c0f383692958face22a1e6

- `cargo test -q -p drover --test round_images --test design_stage --test verify_stage` → 0
- `cargo test -q --workspace` → 101, only for `cloud::bootstrap_installs_what_is_missing_step_by_step_and_opens_the_logins_in_a_pane` (this PR touches neither the test nor `hosts.rs`'s install command; it fails the same way after `touch build.rs`). Every other test binary passes.
- `cargo fmt --all --check` → 0; `cargo clippy -q --workspace --all-targets -- -D warnings` → 0
- `cargo run -q -p drover -- arch-check --base origin/staging --pr-body <this body>` → 0 (0 violations); `drover cap-check --config ci/cap.toml` → 0 (44483 / 45400)
- origin/staging (2bfb337, #611) merged in at this head; `docs/serve.md` and `web/dist/board.js` conflicts resolved, `npm run build` + `npm run check` → 0.

src/ net +40 (budget not stated): the round-keyed routes and the git read helper.

Also updated `design_stage.rs` / `verify_stage.rs` src assertions to the round-keyed srcs (the spec changes those srcs). The comment lines in commit 1's test were dropped in commit 3, because arch-check refuses new comments; the test logic is unchanged.
