1115
Closes #1100

## For humans

A new weekly "triage" helper tidies each fleet's open issues: it closes ones already done, duplicated or replaced (linking the proof), folds stacked amendments into one current spec, and labels unclear ones `stale?`.
It never touches an issue that is queued, being worked on, waiting on your answer, or labeled `keep`, and it cannot change code.
Each run also reads the repo's docs and agent prompts and lists any stale, contradicting or bloated passage in one `docs:` issue for a normal lane to fix; clean docs file nothing.
When it finishes you get one summary in the fleet's chat thread, with a one-line undo for every change. It runs on demand (`drover triage <fleet>`), or weekly where a fleet sets `triage = "weekly"`.

## What changed

- `drover triage <fleet>` (orchestrator/owner): files `triage: <fleet> week of <date>`, queues it on top and spawns a `triage` lane (role prompt `roles/triage.md`, brief `roles/brief-triage.md`, issue body `roles/triage-issue.md`; impl's harness unless `[fleet.<n>.role.triage]` is set). `tick` gets a `triage` step for fleets with `triage = "weekly"`.
- Lane verbs (only a triage lane, an orchestrator or the owner may run them):
  - `drover triage close <fleet> <n> --why done|duplicate|superseded --evidence <sha|#N|url>`: closes with one comment linking the evidence and its undo. A `done` sha must be on `origin/<integration>` (`NOT_ON_INTEGRATION`).
  - `drover triage fold <fleet> <n> --spec <file>`: the body becomes `## Spec (current)` with the old text in a `<details>` block (`NOTHING_TO_FOLD` without `## Amendments`). `drover amend` now ignores the folded block.
  - `drover triage label <fleet> <n> <label>…`.
  - Each verb refuses `PROTECTED` an issue that is queued, has a lane, waits on an open ask or is labeled `keep`, before any write.
- Hook: a triage lane is read-only on code (reviewer rules: no commit, push or merge). Its raw `gh issue close|edit|reopen` is refused (`TRIAGE_VERBS`), as is `gh issue comment` (as for every lane).
- `drover done --no-change "<line>"` on a triage issue builds the summary from the verbs' own events (closed / folded / flagged, each with its undo), posts it to the fleet's orchestrator thread and closes the triage issue. A triage lane that goes idle is prompted once, then closed, like a lesson lane.
- Docs pass (amendment q1756): `drover triage docs <fleet> --findings <file> [--root <checkout>]` files the run's one `docs:` issue, with one finding per line (`file:line: what is wrong: the proof`). An empty findings file files no issue. A second filing from the same lane is refused (`ALREADY_FILED`). A README with no non-goals heading gets that item first, for the owner to write. It logs its undo, and the summary includes it. The prompts (`roles/triage.md`, `roles/triage-issue.md`, `roles/brief-triage.md`, `roles/usage/35-triage.md`) name the scope (README, `docs/`, `roles/`, `.drover/rules.md`, `CLAUDE.md` / `AGENTS.md`). They tell the lane to never edit docs and to never propose a change to the README's intent section. Judging what is stale is the lane's job (owner, q1753); drover only files what it lists.
- Small reuse: `lesson::file` / `lesson::close` are shared with triage; `timer::weekly_due` stamps the weekly run.

Decided: the AC's "triage may `gh issue close|edit|comment`" is met through the drover verbs, because raw `gh` would skip the protected-issue rule ("Acts on GitHub, through drover verbs only"). Weekly runs are opt-in until the conventions issue turns them on for every fleet.

src/ net +472 (budget: none stated). It is one new module plus the triage CLI verbs and the weekly step.

**Line cap: within.** The owner's raise (#1201, decision q1803) is on staging: line_cap 53500. Merged with staging this PR is 48179 / 53500 (staging alone is 47707).

## Evidence (head 32594af7f3419838cb785804a35acdc1e0a52dde)

Fix round 4 (CI red at 5a2b3a1). In shard 1, the sim hunt test failed because random seed 1003 found the planted bug within the hunt's 5 rounds. In shard 2, a `fleet-test-*` dir was left behind. Neither test file is in this PR's diff. Staging since fixed the hunt test (#1259, "a timing-robust hunt test"). Staging's own CI fails test shards intermittently too (shard 3 and shard 4 in recent runs).
- Merged origin/staging. Conflicts: README.md takes staging's text (the docs sweep), which already lists the triage lane. hook.rs keeps triage's read-only rules and staging's new `cap_raise: false`.

Commands at 32594af:
- `cargo fmt --check`: exit 0.
- CI's two clippy commands (`--lib --bins -D warnings`, then the `--tests` one with its `-A` list): exit 0.
- `drover cap-check --config ci/cap.toml --base origin/staging`: exit 0.
- `drover arch-check --base origin/staging`: exit 1. Its only violation is that `just gen` can't run here (`just` is not installed). Ran the diagram and BLESS brief_golden steps by hand: exit 0, `git status` clean.
- `scripts/timing-check.sh`: exit 0. `drover lesson-check`: exit 0.
- `env -u DROVER_RULES -u DROVER_OUTBOX -u DROVER_LANE -u DROVER_HOST cargo test -p drover --test triage --test sim`: exit 0 (4 + 12 passed), no `fleet-test-*` dir left.
- Shard 2 (`scripts/test-shards.sh 2` targets, `cargo test -- --test-threads=4`, scratch TMPDIR): exit 101. The only failures were in cloud, handover and spawn. They fail because the scratch TMPDIR path makes socket paths too long (SUN_LEN) and because of the lane env. Rerun on the default TMPDIR (`cargo test -p drover --test handover --test cloud --test spawn`): exit 0 (7 + 32 + 28 passed), no new `fleet-test-*` dir.

## Evidence, round 3 (head 5a2b3a15884706a3c2329ff23eb67a28d36d6f5e)

Fix round 3 (CI red at 426b4d9: `scripts/test-shards.txt: in no shard: triage`. The same check failed all four test shards before cargo ran):
- `scripts/test-shards.txt` now puts `triage` in shard 1.
- Merged origin/staging (c3d24d5d), which brings in the #1201 cap raise.

Commands at 5a2b3a1:
- `scripts/test-shards.sh check`: exit 0.
- `cargo fmt --check`: exit 0.
- CI's two clippy commands (`--lib --bins -D warnings`, then the `--tests` one with its `-A` list): exit 0.
- `drover cap-check --config ci/cap.toml`: exit 0, 48179 / 53500.
- `drover arch-check --base origin/staging`: exit 1. Its only violation is that `just gen` can't run here (`just` is not installed). Ran the `gen` recipe's commands by hand (types, web build, diagram, BLESS brief_golden): exit 0, `git status` clean, so nothing generated drifts.
- `env -u DROVER_LANE -u DROVER_RULES -u DROVER_OUTBOX -u DROVER_HOST -u DROVER_PORT cargo test -q --locked -p drover --test triage --test brief_golden --test turn --test lesson`: exit 0 (4 + 3 + 11 + 12 passed).
- Not run here: the full workspace suite (CI's shards run it).

## Evidence, round 2b (head 426b4d9164d5fcbe5126ce209a1d707d2105e0f4)

Fix round 2 (CI `rust` arch-check red at c13f5f9; merge of origin/staging ccf7ced9):
- The arch-check doc gap at c13f5f9 is fixed. The triage verbs' flags (`--why`, `--evidence`, `--spec`, `--findings`, `--root`, labels) now live in `crates/core/src/triage.rs`, and README names them. Its Roles bullet and command table now include `triage`.
- Staging's #1147 size baselines meant `cli.rs`, its `dispatch`, `drive.rs` and `tests/hook.rs` could not grow, so:
  - the clap `Args`/verbs and their dispatch moved to `triage.rs`, and `cli.rs` keeps one variant and one arm;
  - `lesson::idle_done` replaces two multi-line `lesson::done(.., idle_note(..))` calls in `drive.rs`;
  - the triage hook test moved to `tests/triage.rs`;
  - two small subtractions in `cli.rs`: a redundant `let mut d = d;` and a shorter `lane()`.
- `roles/usage-tail.md` became `roles/usage/` on staging, so the triage section is now `roles/usage/35-triage.md` and its refusals are in `roles/usage/30-refusals/PROTECTED.md`.
- `env::var` reads now go through `crate::config::env` (#809 ban), and JSON indexing goes through `.at()`/`.put()` (clippy `indexing_slicing`).

Commands at 426b4d9:
- `cargo fmt --check`: exit 0.
- CI's two clippy commands (`cargo clippy --workspace --lib --bins --locked -- -D warnings`, then the `--tests` one with its `-A` list): exit 0.
- `drover arch-check --base origin/staging`: exit 1. Its only violation is that `just gen` can't run here (`just` is not installed). Ran its parts by hand:
  - `drover diagram` is byte-identical to `docs/workflow.md` (`cmp` exit 0);
  - `cargo test -p drover --test brief_golden`: exit 0;
  - no API types or web changed.
- `drover cap-check --config ci/cap.toml`: exit 1, 47467 / 47200 (see above).
- `env -u DROVER_LANE -u DROVER_RULES -u DROVER_OUTBOX -u DROVER_HOST -u DROVER_PORT -u FLEET_LANE cargo test -q --locked --workspace`: exit 101, with 156 binaries OK and 1 failure.
  - The failure is `service::a_service_killed_mid_spawn_leaves_no_stale_state_and_a_new_one_resumes`, a flake: it failed at line 310, then at 286 on a rerun, then passed twice (`cargo test -p drover --test service` exit 0, 0). It does not touch triage.
  - The lane's own `DROVER_*` env (outbox forwarding) must be unset, or the ask/answer tests forward to the lane's outbox.
- Not verified here: the first live runs on herdsman and cooee (after merge, for the owner to review).

## Evidence, round 2 (head c13f5f98bec6028826c57492cbfd64c2bad1ec48)

- `cargo test -q -p drover --test triage`: exit 0, 3 passed. Covers the docs pass: a removed-flag and non-goals fixture gives one `docs:` issue, a clean file gives none, and a second filing is refused `ALREADY_FILED`.
- CI `rust` failed on the arch-check doc gap, fixed above.

## Evidence, round 1 (head 146d374dd37f53a80eb1f11fdc2ac6f088facd58)

- `cargo test --locked -p drover --test triage --test lesson --test hook --test blocked_respawn --test hotspots --test amendments --test diagram --test brief_golden --test m3 --test offline_tick` (run with `--test drive` and `--no-fail-fast`): all passed except one `drive` test.
  - `--test triage`: 2 passed. Covers a fixture issue fixed on staging closed with its commit link, a commit not on staging refused, queued and `keep` issues untouched, a three-amendment issue folded, the summary with its undo lines posted to the thread, and the weekly tick.
  - `--test hook triage`: 1 passed.
- `drive_now_on_the_board_is_paired_only_and_toasts_its_line` is a pre-existing timing flake: it expects "· 0 s" and gets "· 0.1 s" on a loaded host. It failed 3 of 5 runs on clean `origin/staging` f549fe7 and 1 of 3 on this head.
- `cargo clippy --all-targets --locked`: exit 0.
- `cargo fmt`: applied.
- `drover diagram | diff docs/workflow.md -`: exit 0.
- `drover cap-check --config ci/cap.toml`: 45834 / 47200.
- `drover arch-check --base origin/staging`: the only violation is that `just types` can't run here (`just` is not installed). No API types changed.
- The full `cargo test` did not finish locally: it hit the 30-minute limit with 43 binaries passed and none failed. CI runs it in full.
- Not verified here: the first live runs on herdsman and cooee (after merge, for the owner to review).

## Why (round 1)

Decided: a triage lane changes issues only through `drover triage close|fold|label`; its raw `gh issue close|edit|reopen` is refused, so the protected-issue rule cannot be skipped
Because: "the hook allows them for this role alone"
Decided: each verb refuses PROTECTED an issue that is queued, has a lane, waits on an open ask or is labeled keep, before any write
Because: "an issue that is queued, has a live lane, waits on an open owner ask"
Decided: the lane is read-only on code by reusing the reviewer rules (no commit, push or merge), and reports nothing via `drover turn`
Because: crates/core/src/hook.rs:75
Decided: weekly runs are opt-in with `triage = "weekly"` in [fleet.<n>]; `drover triage <fleet>` runs it on demand; role defaults to impl's like lesson
Because: "First live run on `herdsman` and `cooee`; the owner reviews the summary"
Decided: `done --no-change` builds the summary from the verbs' own events, each with its undo, posts it to the fleet's orchestrator thread and closes the triage issue
Rejected: a free-text summary written by the lane — its undo lines could be wrong or missing
Rejected: the concurrency sim in tests/sim — it models drive races; the fixture test drives the real CLI verbs against a fake gh
Check: a `done` close needs a commit on origin/<integration> (NOT_ON_INTEGRATION otherwise); a later `drover amend` ignores the folded block
Owner: Triage closes, folds and flags stale issues weekly where turned on, and tells you in the thread how to undo each change.

<sub>note on 146d374 · git log --notes=why</sub>

## Why (round 2)

Decided: the docs pass files its one `docs:` issue through `drover triage docs <fleet> --findings <file>`; an empty findings file files nothing
Because: "files **one** `docs:` issue listing each stale, contradicting or bloated passage"
Decided: the lane judges the findings (one `file:line: what: proof` line each); drover only files them, refuses ALREADY_FILED for a second one from the same lane, and puts a missing README non-goals section first
Because: "triage judges what is too long, duplicated or obsolete"
Rejected: a mechanical removed-flag or contradiction detector in code — the owner made it the lane's judgment, with no new per-PR checks
Decided: the prompts name the scope and forbid editing docs or proposing a change to the README's intent section
Because: "it never proposes changes to that section in the `docs:` issue"
Check: the docs fixture test drives the real CLI verb against a fake gh: removed flag and non-goal findings give one issue with both, a clean file gives none
Owner: Each weekly triage run now also lists stale or contradicting docs in one issue for a normal lane to fix, and files nothing when the docs are clean.

<sub>note on c13f5f9 · git log --notes=why</sub>

## Why (round 3)

Decided: the triage test target runs in CI shard 1; test-shards.sh fails every shard while a target is in none
Because: scripts/test-shards.txt:191
Decided: no block on the cap raise; #1201 already merged it (line_cap 53500), so this PR is within the cap after merging staging
Because: q1803
Rejected: blocking this issue on #1201 per q1803 — #1201 is already merged, so waiting would hold a PR that already fits
Check: scripts/test-shards.sh check exits 0, and cap-check reads 48179 / 53500
Owner: The triage lane's tests now run in CI, and the PR fits under the raised line cap.

<sub>note on 5a2b3a1 · git log --notes=why</sub>

## Why (round 4)

Decided: triage is a scheduled lane like lesson: weekly from tick, on demand with `drover triage <fleet>`, acting on GitHub only through `drover triage close|fold|label|docs` verbs.
Because: "Acts on GitHub, through drover verbs only** (the hook allows them for this role alone)"
Decided: the hook treats a triage lane as read-only (no commits or pushes) and refuses raw `gh issue close|edit` there with TRIAGE_VERBS.
Because: "hook rules (triage may `gh issue close|edit|comment`, may not push code or merge)"
Decided: fix round 4 merges origin/staging; README takes staging's text, hook.rs keeps triage's rules plus staging's `cap_raise: false`.
Because: crates/core/src/hook.rs:79
Rejected: changing the sim hunt test here — it is outside this PR's diff and staging fixed it in #1259.
Check: the triage, sim, handover, cloud and spawn tests pass in CI at the new head, with no fleet-test-* dir left.
Owner: CI's red was a flaky sim test and a temp dir another test left; both came from staging, which this round merges in.

<sub>note on 32594af · git log --notes=why</sub>

