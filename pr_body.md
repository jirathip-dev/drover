Closes #737

## For humans
New lanes no longer get refused as "unguarded" depending on whether profiles were last synced through the `~/.local/bin` link or the real installed binary: both now count as the same program.
Doctor and the profile sync check now point out a profile whose safety hook runs a different herdsman binary, naming it.

## Change
- Hook commands now name the running binary with symlinks resolved, via `std::fs::canonicalize` (`harness::hook_command`).
- `spawn::unguarded` accepts a hook whose program resolves to the running binary (symlink or real path), not only a byte-equal string (`harness::guards`).
- `profiles sync --check` and a new doctor row, `profile hooks`, name each profile hook that runs another binary (`spawn::stray_hooks`).

## Repro
Commit 1 (fe4e586) holds only the regression test:
`cargo test -p herdsman --test shed a_hook_synced_through_a_symlink_guards_a_spawn_through_the_real_path`: exit 101 at commit 1 (`REFUSED ACP_UNGUARDED: profile fleet-impl has no herdsman PreToolUse hook`), exit 0 at head.

## Evidence (head e645f287286a28c7e5c245bf4f2fda21d11cf475)
Merged `origin/staging` twice in this round. The only conflict was `crates/cli/tests/shed.rs`, where both sides' tests are kept. Every command below ran with `HERDSMAN_LANE`, `HERDSMAN_ACCOUNT`, `HERDSMAN_HOST` and `CHROME` unset, because a lane's own environment otherwise leaks into `self_update`'s "by owner" assertion.
- `cargo build --release`: exit 0
- `cargo clippy --all-targets -- -D warnings`: exit 0
- `herdsman cap-check --config ci/cap.toml`: exit 0 (36786 / cap 38028)
- `cargo test --no-fail-fast -- --skip lanes_and_sheet_fit_390px`: exit 0, 158 suites ok, 0 failed. This includes `shed::a_hook_synced_through_a_symlink_guards_a_spawn_through_the_real_path`, `shed::a_hook_naming_another_binary_refuses_acp_unguarded`, `doctor::a_hook_naming_another_binary_is_named` and staging's `shed::a_codex_rev_and_a_claude_impl_spawn_with_their_own_agent_and_login`.
- Not verified here: `lanes_sheet::lanes_and_sheet_fit_390px` (the board's 390px screenshot test through WebKit) hung twice on this host, once more than 30 minutes, and another lane's run (impl-712) hung on it at the same time. This PR touches no board code. CI's run is the evidence for it.

src/ net +56 (no budget stated): three small helpers, plus the doctor row and the check loop.

## Why (round 1)

Decided: hook commands name ctx.exe canonicalized, and spawn accepts a hook whose program resolves to the running binary
Because: "`unguarded` accepts a hook whose program resolves to the same file as the running binary (symlink or real path)"
Because: crates/core/src/harness/mod.rs:274
Because: crates/core/src/spawn.rs:541
Decided: profiles sync --check and a new doctor row "profile hooks" name each profile hook that runs another binary
Because: crates/core/src/spawn.rs:106
Rejected: canonicalizing ctx.exe once in Ctx::from_env — tests set ctx.exe directly, so the regression would bypass it
Rejected: comparing only canonical strings — a hook synced before this fix still names the symlink path and must guard
Check: a_hook_synced_through_a_symlink_guards_a_spawn_through_the_real_path fails at commit 1 and passes at head
Check: a_hook_naming_another_binary_refuses_acp_unguarded and doctor's a_hook_naming_another_binary_is_named pass
Owner: New lanes no longer get refused depending on whether profiles were synced through the ~/.local/bin link or the real binary.

<sub>note on 125f383 · git log --notes=why</sub>

