Closes #1212

## For humans

When a remote lane that signs in through the main host's token proxy reports an auth error, the alert now tells you to check the proxy's token on the main host (and how to make a new one), then let the host back on — never to log in on the remote host, which could not fix it.
A single auth blip on such a lane no longer locks the host: if a call gets through the proxy again within 5 minutes (configurable), new lanes keep starting there.
Lanes that use the host's own login keep the old advice and the old hold.

## What changed

- `spawn`: the spawn event marks a lane that got a proxy key (`proxy: true`, via `lane_data`).
- `hosts::hold_alert`: a proxy lane's `signed-out` reason names the token proxy, `[proxy] token-file` on the main host, the provider's setup command (`claude setup-token`) and `drover hosts on <host>`; no `claude /login`. Host-login lanes unchanged. The alert carries `proxy: true`, and a proxy lane's alert key includes its last `proxy-ok`, so a later real failure alerts again.
- `proxy`: when upstream answers 2xx for a lane that has a `signed-out` with no `proxy-ok` since, the proxy logs one passive `proxy-ok` event (agent, host). `relay` now reports whether upstream answered 2xx.
- `hosts::login_hold` (LOGIN_NEEDED): a proxy lane's `signed-out` holds its host only once `[limits] proxy_retry_secs` (new, default 300) has passed with no `proxy-ok` for that lane since it. Usage limits and host-login sign-outs hold as before.
- `docs/config.md` row and regenerated `docs/workflow.md` for the new limit.

src/ net +45 (no budget given): the proxy needs a success signal and the hold needs to read it; no new deps.

## Repro

Commit 1 (`bd5a4f7c`) adds only `a_proxy_lanes_auth_error_names_the_proxy_and_holds_its_host_unless_a_proxy_call_clears_it` in `crates/cli/tests/remote_acp.rs`. It spawns a real proxy lane on `boat1`, makes shed report an `auth:` error, runs `check`, checks the `signed-out` reason, then (in the retry case) sends one real request through `proxy::serve_on` with the lane's own key to a fake upstream, and spawns a second lane on `boat1` an hour later: held without the retry, not held with it.

On staging + commit 1 it fails: `cargo test -p drover --test remote_acp a_proxy_lanes_auth` →
`impl lane sendmeter-impl-12 is signed out on boat1; new lanes there wait: run `claude /login` there, then `drover hosts on boat1``.
With the `proxy-ok` emission stubbed out at the head, the retry case fails with `REFUSED LOGIN_NEEDED`, so the hold is lifted by the proxy's success and nothing else.

## Evidence (head 3e986d55d267c43aa729ecfc4d9798c4a588348b, staging d0cba316 merged in)

- `cargo test -p drover --test remote_acp` → exit 0 at 3e986d55 (11/11, incl. the new test)
- `cargo test -p drover --test sim` → exit 0 at 3e986d55 (10 passed, 6 ignored)
- `cargo fmt --check` → exit 0 at 3e986d55
- CI's tests clippy line (`cargo clippy -p drover --tests --locked -- -D warnings -A …`) → exit 0 at 3e986d55

### Staging build fix

Staging's own CI fails `sim::the_hunt_finds_a_planted_bug_sooner_than_random_seeds_and_minimises_it` since c3d24d5d (#1195): drive now starts lanes again more often, so random seed 1003 hits the planted third-start bug in round 4, while the hunt's first four rounds were just its four fixed start seeds and could not beat that. The hunt now starts from seed 4 alone, so its mutations run from round 2; it finds the bug in round 2 (random seeds 1000–1001 don't), and minimises it as before. Raising the planted threshold back to 4 was tried: the hunt then finds nothing in 40 rounds (rng 3 and 5).

Earlier runs at 3fa578541a348765f357482c7c6e8c30bf6b9805:

- `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --locked -p drover-core --lib -p drover --test remote_acp --test shed --test host_words --test shed_native --test access --test brief_golden --test generic_harness --test doctor` → exit 101: every binary ok (remote_acp 11/11, including the new test and the existing host-login advice test in shed.rs) except `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which fails the same way with staging's `crates/core/src` (this host has a real `shed` on the timer's PATH). Environmental, not this change.
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` and the CI `--tests` clippy line → exit 0
- `cargo fmt --check` → exit 0
- `drover cap-check --config ci/cap.toml` → exit 0 (47298 / 53500)
- `scripts/timing-check.sh` → exit 0
- `drover arch-check --base origin/staging` → exit 1, only `just gen exited 127: just not found` (no `just` on this host); I ran gen's `diagram > docs/workflow.md` step by hand and committed the result; the `brief_golden` test passes unblessed, and limits aren't in the TS types.
- The full workspace `cargo test` could not link here: the shared disk hit 100% mid-run. CI runs it.

## Why (round 3)

Decided: proxy lanes are marked on the spawn event; their signed-out advice names the token proxy and its setup command, never the host login
Because: "the signed-out advice names the proxy (its token on the main host, `claude setup-token`)"
Decided: the proxy logs one passive proxy-ok when a signed-out lane's call gets a 2xx; login_hold skips a proxy sign-out inside proxy_retry_secs or followed by proxy-ok
Because: "A proxy lane auth error followed by a successful proxy request within the retry window does not set LOGIN_NEEDED"
Decided: staging build fix: the sim hunt starts from seed 4 alone so its mutations run from round 2, before random seed 1003 trips the planted bug
Because: crates/cli/tests/sim/main.rs:279
Rejected: planted threshold back to 4 — the hunt then finds nothing within 40 rounds
Rejected: delaying the signed-out alert in check — check is stateless and the AC asks for the proxy's own success as the signal
Check: host-login lanes keep `claude /login` advice and the immediate hold (shed.rs an_auth_failure_alerts_signed_out_once_and_holds_the_host)
Check: sim hunt finds I1 in round 2 and still minimises it
Owner: a one-off dropped proxy call no longer locks a remote host for hours, and the alert points at the proxy token instead of a useless login step

<sub>note on 3e986d5 · git log --notes=why</sub>

