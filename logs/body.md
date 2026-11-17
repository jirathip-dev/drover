Closes #1178

## For humans
When a reviewer keeps writing a failing review without its proof, drover now re-reviews once with a fresh reviewer. If that reviewer does the same at the same head, drover hands the issue to the owner as needs-human instead of starting reviewers again and again.

## Cause
`drive::no_proof` (crates/core/src/drive.rs): a reviewer's FAIL refused NO_PROOF is told once, then superseded by a fresh reviewer, which writes the same FAIL and is superseded in turn, with no bound. At 01a7b51 the hunt's plan gave rev-103-r1 four spawns in 41 minutes (t+1676, 2540, 3114, 4153), each `proof-refused → supersede → spawn` with no push or verdict in between. Parked seed 1069 names the same cause.

## Fix
If an earlier reviewer on the same head was already refused NO_PROOF (a `proof-refused` with that sha logged before the current spawn), drive escalates `needs-human` (code NO_PROOF) instead of spawning another re-review. A new push still gets a fresh review. The first re-review still happens, so seed 1's incident test is unchanged. "Earlier" is ordered by log position, not ts, because a kick's frozen clock (#370) can stamp the refusal and the next spawn in the same second.

## Repro
- Commit 1 (ae197f75) adds two PLANS entries: the issue's minimised plan verbatim, plus the same fleet with explicit scripts (`impl P 300 1 1 | rev V 600 0 0 0` ×6: every reviewer writes a FAIL without Proof and ignores the refusal).
- The issue's plan verbatim **passes on the current base** 80a3be3c. It failed at 01a7b51 (exit 101, I9 rev-103-r1). The 8 commits since then (#1186's starts in every pass, etc.) shift the scheduler's rng, so rev-103 now draws a `Hang` script and never writes a proofless FAIL. The cause was hidden, not fixed. That is why the second plan pins the scripts.
- The scripted plan fails on base plus commit 1: `cargo test -p drover --test sim fixed_seeds_hold_every_invariant` → exit 101, `I9 churn: drover-rev-106-r1 spawned 4 times within an hour, nothing new in between`.
- At head it passes: each reviewer gets one supersede, then a single `needs-human` per issue.

I9 (crates/cli/tests/sim/invariants.rs) is unchanged.

## Evidence (head 641c56797a933db3f753f55bade9083a7fc4bb39)
- `cargo test -p drover --test sim` → exit 0 (10 passed, 6 ignored; includes fixed seeds + both new plans)
- `DROVER_SIM_HUNT_MINUTES=2 DROVER_SIM_HUNT_RNG=1794830539 cargo test -p drover --test sim hunt -- --ignored --exact` → exit 0. Capped at 2 minutes: the host was at load 11/8 CPUs. The 10-minute hunt runs in the nightly sim workflow.
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` → exit 0; `cargo fmt --all --check` → exit 0
- `drover arch-check --config .drover/arch.toml --base origin/staging` → the drive.rs size violation (CI's red `rust` check at dbfbd9d8) is gone: drive.rs is 2185 lines, at its baseline. Locally it still reports 1 violation, `just gen` exited 127, because `just` is not installed on this host; CI has it.
- `cargo test -p drover --test drive --test proof`: 4 merge-label tests failed at the previous head, identically with the fix reverted (pre-existing, not touched here).

src/ net +9 (budget: none stated): one guard plus its escalate call in `no_proof`.

## Why (round 1)

Decided: a reviewer refused NO_PROOF at a head where an earlier reviewer was already refused NO_PROOF goes to needs-human instead of another supersede.
Because: "Drive never spawns the same rev lane again and again with nothing new in between"
Because: crates/core/src/drive.rs:1570
Rejected: loosening I9 or a ts-window count — the issue forbids loosening I9, and a frozen kick clock (#370) makes ts ordering unreliable, so log position is used.
Rejected: only the issue's plan as the repro — it passes on base 80a3be3c because the rng now gives rev-103 a Hang script; a second plan pins the reviewer scripts.
Check: commit 1 fails I9 on base (rev-106-r1), head passes with one supersede then needs-human per issue.
Check: drive.rs test failures on merge labels are identical with the fix reverted.
Owner: a reviewer that keeps skipping its proof now gets one fresh retry, then lands on your card instead of looping.

<sub>note on dbfbd9d · git log --notes=why</sub>

