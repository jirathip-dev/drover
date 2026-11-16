Closes #957

## For humans
CI now finishes in about 8 minutes instead of about 19. Compiled crates are cached between runs, the release build happens once, and the test suite runs beside the release checks with its test programs in parallel. The release build of drover also uses a tuned optimisation profile that makes the measured server paths 17–46% faster, for about 7% more build time.

## What changed
- `.github/workflows/ci.yml`
  - `Swatinem/rust-cache@v2` in every Rust job (rust, test, ui, ui-webkit). It keys on `Cargo.lock`, the rustc version and the job.
  - One release build: only `rust` builds release. `ui` builds debug for its demo export, the same profile its own `cargo test` steps compile, so it no longer does a second release build.
  - A new `test` job holds clippy, the debug suite and the temp-dir check, running beside `rust` (release build, perf, soak, and the gates that use the binary). The suite runs under `cargo nextest run --locked --no-fail-fast` with `CARGO_PROFILE_DEV_DEBUG=line-tables-only`.
- `Cargo.toml`: `[profile.release]` sets `lto = "thin"`, `codegen-units = 1`, `strip = true`.

Why the split and nextest: with only the cache, the cold run (43829040164) showed the rust job spending about 9 min compiling and about 9 min running tests in series. The warm run (43832072005, attempt 2) was still 15m15s, only 21% shorter: compiling the tests took 3m18s and 213 test binaries then ran one after another for about 11 min (sim alone 162 s). The cache can't shorten that. nextest runs 1497 tests (cargo test passed 1483). There are 0 doctests, so nothing is dropped. Test temp dirs are pid-scoped, so parallel processes don't collide.

## CI wall time and billed minutes
GitHub's timing API reports 0 billable ms for this public repo, so billed minutes here are the per-job durations, each rounded up to a minute as GitHub bills.

| run | commit | wall | rust | test | ui | billed min |
|---|---|---|---|---|---|---|
| before, old workflow (43826541527) | 2e79611 (= cbda564 code) | 19m13s | 19m10s | — | 5m27s | 29 |
| before, staging push (43823456023) | cbda564 | 19m24s | 19m20s | — | 4m24s | 28 |
| after, cold test-job cache (43836383016 #1) | be1828e | 7m25s | 7m15s | 7m22s | 2m01s | 22 |
| after, warm (43836383016 #2) | be1828e | 10m10s | 7m16s | 10m10s | 2m20s | 25 |
| **after, warm, final head (43839395426)** | **346b369** | **8m11s** | 8m03s | 8m08s | 2m34s | 24 |

Warm at the final head: 8m11s vs 19m13s, **57% shorter** (AC is ≥30%). The slowest warm sample (10m10s) is still 47% shorter. The spread between after-runs is runner speed, not the cache: the CPU-bound sim tests took 65 s on one runner and 168 s on another for the same test (`the_hunt_finds_a_planted_bug…`). Cache hits are confirmed: the warm test job compiled 6 crates vs 115 cold.

## Perf gate (release profile), `--perf`
Command (same as the `baseline:` commit 2e79611, macOS lane host, 3 sequential runs each, medians):
`cargo test --release --locked --test perf -- waits_and_the_lanes_tab_stay_fast_on_a_30k_event_log a_hundred_board_requests the_state_read_stays_under_300ms --nocapture --test-threads=1`

| metric | before (default profile) | after (thin LTO, cgu 1, strip) | change |
|---|---|---|---|
| #560 100 board requests, CPU | 253 ms (253/250/323) | 211 ms (227/210/211) | −17% |
| #447 snapshot::waits | 24 ms (24/24/38) | 13 ms (21/12/13) | −46% |
| #447 lanes reads | 24 ms (66/20/24) | 14 ms (14/11/15) | −42% |
| #859 /api/v1/state median | 11 ms (32/8/11) | 6 ms (6/5/7) | −45% |
| clean release build (`--test perf --no-run`) | 3m29s | 3m44s | +7% |

In CI (Linux, one run each, so within noise): 100 requests 241 → 225 ms CPU, waits 27 → 33 ms, lanes 11 → 8 ms, state median 11 → 10 ms. In CI the release steps take longer to compile with the profile (release build ~127 → ~130 s warm; perf test step 9 → 62 s; soak 125 → 178 s). Those steps are off the critical path now that `test` runs alongside.

Self-update still installs the build: `cargo build --release --locked -p drover` with the profile, then the stripped binary ad-hoc codesigned (`codesign -s - -f`, exit 0), `codesign -v` (exit 0), and `--version` run (exit 0). `cargo test --locked --test self_update` passed 10/10 (exit 0) with `DROVER_LANE` unset. With the lane's `DROVER_LANE` set, `pause_and_resume_are_logged_and_pause_stops_the_job` sees the lane as actor instead of `owner`. That is this lane's environment, not the change; in CI it passes.

## Evidence at head 346b3695c7dc031df04e3a2a73f14a306165c1f5
- CI run 43839395426 at 346b369: rust, test, ui, web, actionlint, changes all success. ui-webkit skipped (no page files touched).
- Local: perf gate ×3 on base and on the profile, exit 0 each. self_update exit 0 (DROVER_LANE unset).

src/ net +0 (budget n/a): only the workflow and Cargo.toml change.
