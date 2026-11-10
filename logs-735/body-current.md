Closes #735

## For humans

CI now fails a pull request that adds a code comment. Two kinds still pass: a safety note right above unsafe code, and one line of documentation on a public function or type. The reason for a change goes in the commit's why note instead.
CI also fails when the docs or README name a file, line, link, command or flag that no longer exists. A change to a command-line flag, a config key or the API has to update a doc that names it, or say "no doc: why" in the PR body.

## What changed

- `herdsman arch-check` takes two new flags: `--base <rev>`, the revision whose added lines are judged, and `--pr-body <file>`.
  - `[comments] policy = "off"`: an added Rust or TypeScript `//` or `/* */` line fails. Two exceptions pass: `// SAFETY:` directly above an `unsafe` line, and one `///` or one-line `/** */` on a `pub`/`export` item (two in a row fail). `except` globs skip generated files.
  - `[docs]`: inline-code paths, `path:line`, relative links and `herdsman …` commands and flags in the `paths` docs must exist at the head, checked against this binary's own CLI when `cli = true`. With `--pr-body`, a changed `name:` line in a `watch` file needs a changed doc line naming it (`name` or `name-with-dashes`), or `no doc:` in the body.
- This repo turns it on in `.herdsman/arch.toml`. `ci.yml` and the shared `arch-check.yml` now check out with `fetch-depth: 2` and, on pull requests, pass `--base HEAD^1 --pr-body <event body>`.
- The impl brief (used for impl, fix and update rounds) says a non-obvious reason goes in the why note (`herdsman why`), never a comment. The rev brief says to read the notes. The brief goldens were regenerated; the only change is these 3 lines.
- 4 stale doc references were fixed so this repo's docs pass: `docs/architecture.md`, `docs/pipeline.md`, `docs/runtime-hc.md` and `docs/design/fleet-cards/README.md`.
- Scope 3 (stripping old comments) is a conductor-filed `--refactor` issue per repo, so it is not in this PR.

Decided:
- I dropped checking bare `--flag` spans: it gave 227 false hits on this repo (other tools' flags, CSS variables).
- A path counts only when its first folder exists at the repo root. `../` paths resolve from the doc's folder.
- A PR body edited after the push does not re-run CI, so `no doc:` must be in the body before the push it covers.

src/ net +271 (cap-check: 37064 at staging → 37335) (no line budget given in the issue): two text checks, plus the diff parser they share.

## Evidence (head d7e7adf5ed153963bcaea74be8c89b887918505e)

Round 2 fixes the red `layers` CI step: `crates/core/src/arch.rs` named clap's command type, which the step's `Command|Stdio|Child` grep reads as process spawning. The clap walk is now a closure; behaviour is unchanged.

- `bad=$(grep -rlwE 'Command|Stdio|Child' crates/core/src | grep -vxE 'crates/core/src/(runner|events|webpush)\.rs' || true)` (the CI step): prints nothing
- `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings`: exit 0
- `cargo test --locked --test arch_check`: exit 0 (7 passed, the AC2 fixtures)
- `cargo test --locked --no-fail-fast` (in the lane's environment, macOS): 3 failures, all in code this PR doesn't touch: two WebKit chat-tab tests (`chat_tab.rs`, 390px scroll-follow and older-messages) and `cloud.rs` bootstrap (an ssh/env-dependent fake-cloud run). Round 1 got these green with the lane variables unset. CI on Linux is the gate.
- `herdsman cap-check --config ci/cap.toml`: exit 0
- `herdsman arch-check --base $(git merge-base HEAD origin/staging) --pr-body body.md`: exit 0 (860 files, 0 violations)

## Why (round 1)

Decided: the comment and doc-touch checks judge only `git diff --base`; CI passes HEAD^1 of the PR merge commit (fetch-depth 2)
Because: "Only added lines are judged, so old comments don't fail."
Decided: `herdsman …` spans are checked against this binary's own clap tree (`[docs] cli = true`), flags only inside them
Rejected: checking bare `--flag` spans — 227 false hits on this repo (other tools' flags, CSS vars)
Decided: a path span counts only when its first folder exists at the repo root; `../` spans resolve from the doc's folder
Decided: `[docs] skip` globs for per-fleet-repo paths (`.herdsman/**`) that this repo's docs describe but do not have
Decided: a watched change is a `name:` line in a watched file; a changed doc line naming `name` or `name-with-dashes` covers it
Because: "must touch a doc naming it, or carry `no doc: <why>` in its body"
Decided: new CLI flags take `help = …` attributes instead of `///` (a `///` above a non-pub field is a new comment)
Because: crates/core/src/arch.rs:368
Rejected: stripping old comments here — scope 3 is one conductor-filed --refactor issue per repo
Check: a doc paths fix in docs/pipeline.md, runtime-hc.md, fleet-cards README and architecture.md only removed stale or other-repo names
Check: a PR body edited after push does not re-run CI, so `no doc:` must be there before the push
Owner: new comments now fail CI here, and docs may only name files, commands and flags that exist

<sub>note on ea6484a · git log --notes=why</sub>

## Why (round 2)

Decided: the clap tree walk is a closure over the built command inside stale_docs, so arch.rs names no clap type
Because: crates/core/src/arch.rs:401
Rejected: narrowing the CI layers grep to std::process — the check is shared policy, the closure is the smaller fix
Check: the CI layers grep over crates/core/src prints nothing at this head
Owner: the review's red CI check came from a type name in the new docs check; renamed away, behaviour unchanged

<sub>note on d7e7adf · git log --notes=why</sub>

