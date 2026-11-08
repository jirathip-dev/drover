Closes #631

## For humans

Installing a new herdsman build no longer breaks its access to the account tokens in the Keychain. Herdsman now reads its secrets through a small helper program that is rebuilt only when its own code changes, so you approve it once per host and later updates need nothing. A self-update whose new build can't read the tokens is never installed: the old build keeps running and you get one alert with the exact fix.

## What changed

- **`crates/keys` (`herdsman-keys`)**: a new, separately versioned binary that only does `get` / `set` / `delete` on `herdsman/*` items (value on stdin, never argv). When herdsman runs it (stdin not a terminal) it calls `SecKeychainSetUserInteractionAllowed(false)`, so a read that would need approval fails at once (exit 3) and never shows a dialog. Run from a terminal by the owner it may show the dialog, so a one-time Always Allow still works.
- **`runner::store_get/store_set`**: go through `herdsman-keys` when it's installed beside the binary, or at `$HERDSMAN_KEYS` (the install gates set this). Exit 1 means not set; exit 3 or a 5 s timeout means `KEYCHAIN_LOCKED`. Without the helper they fall back to the old in-process keyring.
- **`herdsman doctor --tokens`**: the token check. It reads each `[accounts]` token through the helper with no dialog and exits 1 on any ✗. If there is no helper it fails instead of reading in-process, which could pop a dialog.
- **`self-update`**: builds `-p herdsman -p herdsman-keys`. The helper's version is the sha256 of `crates/keys/{Cargo.toml,src/main.rs}`, stored in `herdsman-keys.src` beside it.
  - Hash unchanged: the helper is never copied again, so its cdhash stays the same.
  - First install: the helper is put straight into place.
  - Hash changed: the helper is staged as `herdsman-keys.new`.
  - The gate then runs `env HERDSMAN_KEYS=<helper> <new> doctor --tokens` after `--config-only`. If that fails, the old binary stays, there is no `profiles sync` and no serve restart, and `self-update-failed` fires once per head with doctor's ✗ line, which carries the fix.
- **`scripts/install-gate.sh`**: does the same helper placement and token check before signing and swapping.
- **Doctor / `KEYCHAIN_LOCKED` fix text** (`keychain_fix`): now gives the exact commands to authorise the helper (`codesign -dvvv … | grep CDHash` plus `security set-generic-password-partition-list … cdhash:<CDHash>`, or Always Allow from a terminal). It says "install it" when the helper is missing. **docs/troubleshooting.md §3** and **docs/setup.md** are updated to match (migration).

Decision: I used the brief's preferred helper shape rather than designated-requirement ACLs. A self-signed `herdsman-local` identity has no team ID, so the Keychain partition still keys on the cdhash, which changes with every build. A binary that is never rebuilt keeps one cdhash for good.

## Evidence (head f713ec2e8b36000c8e7eb62f417d6c571ea880b6)

Update round: `origin/staging` is merged in. It brings #658 (q793), which raises `line_cap` to 35500, so this PR now uses staging's cap and has no cap change or cap-history entry of its own. Owner decision q798's raise to 31600 is superseded.

- `cargo fmt --all --check` → exit 0
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (31646 / 35500)
- `cargo test -p herdsman --lib` → exit 0; `cargo test -p herdsman-keys` → exit 0
- `cargo test -p herdsman --test <t>` for `self_update`, `keychain`, `retired_keys`, `doctor`, `drive` and `cloud` → each exit 0 (with `HERDSMAN_LANE` unset)
- A full `cargo test --no-fail-fast` could not finish on this host. The shared target cache was swept mid-run, and the test binaries show as "never executed". CI is the full-suite gate for this head.
- New and changed tests:
  - `self_update::a_build_that_cannot_read_the_tokens_keeps_the_old_binary_alerts_once_and_leaves_serve` (AC2)
  - `self_update::the_keys_helper_is_reinstalled_only_when_its_source_changes` (AC3)
  - `retired_keys::install_gate_…`: the real `herdsman` binary, run as a subprocess, reads `herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_MAIN` through a fake installed helper; the current helper is not reinstalled; a helper that would prompt (exit 3) keeps the old binary (AC3 and the install-gate half of AC1 scope).

`src/ net +153 (budget: none stated)`: the new helper crate (~60 lines), helper routing in the store, `doctor --tokens`, and helper staging in self-update.

## Not verified

AC4 (live on the Mini: authorise the helper once, then two self-updates in a row leave doctor's account rows ✓ with no dialog) needs the owner at the host. I can't run that from a lane.

