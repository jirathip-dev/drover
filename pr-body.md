Closes #1165

## For humans
Remote Linux boats now update drover by downloading the binary that main's CI built for that exact commit, after checking its checksum, instead of compiling from source for about 6 minutes.
If the download is missing, fails or does not match its checksum, the boat compiles from source as before and its sync log says why. Macs and other machines keep compiling.

## What changed
- `.github/workflows/ci.yml`: on a push to `main`, the `rust` job publishes `target/release/drover` as `drover-x86_64-unknown-linux-gnu` plus `drover-x86_64-unknown-linux-gnu.sha256` to a prerelease tagged with the commit sha (`gh` CLI, `GITHUB_TOKEN`, job permission `contents: write`; a re-run re-uploads with `--clobber`).
- `hosts::install_line` (used by `hosts bootstrap` and host sync): on `uname` Linux/x86_64 it curls the asset and its `.sha256` from `<repo>/releases/download/<sha>/`, runs `sha256sum -c`, and installs atomically into `<root>/bin/drover` (the same root as `cargo install`). Anything else (other OS or arch, a missing asset or failed download, a checksum mismatch, a failed install) prints `drover: <why>; cargo install instead` to the sync log and runs the unchanged `cargo install`. The `herdsman` link and `profiles sync` follow as before.
- No new dependency: curl and sha256sum on the host, `gh` in CI.

src/ net +7 (budget none given): one shell line grows into the download/verify/fallback chain.

## Evidence (head d433b6546ffdc1be3da0678c2d156d0b4f376e89)
- `cargo test --locked --test cloud --test hosts --test host_sync`: exit 0 (32 + 6 + 47 passed). The new test `a_linux_x86_64_host_installs_main_cis_checked_binary_and_falls_back_to_cargo` runs the real `install_line` under `sh` with stub `uname`/`curl`/`cargo` and the real `sha256sum`: a good checksum installs the downloaded binary without calling cargo; a bad checksum installs cargo's build, never the download; a missing asset falls back to cargo; Darwin/arm64 never downloads.
- `cargo fmt --check` and both `cargo clippy` invocations from ci.yml: exit 0.
- `drover cap-check --config ci/cap.toml --base origin/staging`: exit 0 (52207 / 53500).
- `drover arch-check --base HEAD^1`: exit 1, its only violation being that `just` is not installed in this lane (`just gen` exit 127). CI runs it.

## Not verified here
- The release step's first run and the "sync under 1 min" `host-sync` timing: both need a push to `main`, which only happens after merge. actionlint isn't installed in this lane, so CI's actionlint job checks the workflow.
