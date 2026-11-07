Closes #577

## For humans
A verify lane's profile now carries the verifier's instructions instead of the reviewer's, so its system prompt and its brief no longer disagree. It keeps the same read-only safety hook as before. The sync output names the prompt it wrote, e.g. `(verify)`.

## What changed
- `spawn.rs`: `prompt_kind(f, role)` holds the rule `lane_prompt` already used (`lesson` and a check `verify` get their own prompt, any other role its family's); `lane_prompt` and `profiles_sync` both use it.
- `Harness::sync` takes the prompt kind next to the hook family. Claude writes `role.md` from the prompt kind and prints it in the sync line. The hook stays the family's (read-only `rev` for verify). Hermes/dsh ignore the new argument (they write no prompt).
- Regression test `profiles_sync_gives_a_verify_profile_its_own_prompt` (crates/cli/tests/spawn.rs): a `verify` role on its own Claude profile → `role.md` starts with `# ROLE: VERIFIER`, `settings.json` carries `hook --role rev`, and the sync line says `(verify)`.

src/ net +33 (budget: none stated). Most of it is rustfmt splitting the three `sync` signatures across lines once they gained a parameter.

## Repro
Commit 1 (34c7cdaf) holds only the test. At that commit, `cargo test -p herdsman --test spawn profiles_sync_gives_a_verify_profile_its_own_prompt` exits 101: the sync line reads `fleet-verify (rev)`. At the head it passes.

## Staging merge
`cli.rs` passes the new `sync` argument at the call site staging added. Staging has since fixed the `thread_handoff.rs` test the same way (cc7f4c473); the merge takes staging's version, so this PR no longer changes that file.

## Evidence (head 65af465da733758c50391f2735e88a4c17520e95)
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (31244 / 31500)
- `cargo test -p herdsman --test spawn --test thread_handoff --test verify_stage` (lane env `HERDSMAN_OUTBOX/HOST/LANE` unset) → exit 0 (26 + 13 + 16 passed, incl. the regression test)
- Full `cargo test`: the CI `rust` job runs it at this head. Locally the host deletes the scratch build dir partway through a full run.
