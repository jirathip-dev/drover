Closes #1348

## For humans

When drover merges a pull request that raises the line cap, the merge commit now keeps the `Cap:` line from the branch's commits.
The release PR's cap check then passes for the next cap raise. The one existing commit on staging without it (6f5a256) stays as it is.

## What changed

- `drover merge` asks gh for the PR's `commits` as well, and `merge_message` appends every `Cap:` trailer from their bodies after the facts line, verbatim. It matches them with cap-check's own `cap_trailer`.
- Regression test `a_squash_merge_keeps_the_cap_trailer_so_cap_check_passes` (crates/cli/tests/merge.rs): it drives `drover merge`, builds a real squash commit on a temp git repo from the subject and body drover passes to `gh pr merge`, then runs `drover cap-check --config ci/cap.toml --base <pre-merge>`.

src/ net +8 (no budget given): a few lines to collect the trailers, plus the gh field list.

## Evidence (head cafe6b72dfc56c46f72d049f85c2695bc1960fed)

- Repro at 6c397029 (test only): `cargo test -p drover --test merge a_squash_merge_keeps` → exit 101 (`c2884f3 edits the line cap without a Cap: … trailer`)
- `cargo test -p drover --test merge` → exit 0 (40 passed)
- `cargo fmt --check` + both CI clippy commands from ci.yml → exit 0
- `cargo test --workspace --locked -j 4 --no-fail-fast` → exit 101: 239 test binaries ok. Failures are all from this host, not from this change:
  - `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`: doctor expects macOS `sysctl` and a signed-in `gh` on this Linux lane host.
  - 6 doctest targets: `rustdoc` is not installed on this host ("could not execute process `rustdoc`"). CI uses nextest, which doesn't run doctests.
