Closes #1212

## For humans

When a remote lane that signs in through the main host's token proxy reports an auth error, the alert now tells you to check the proxy's token on the main host (and how to make a new one), then let the host back on — never to log in on the remote host, which could not fix it.
A single auth blip on such a lane no longer locks the host: if a call gets through the proxy again within 5 minutes (configurable), new lanes keep starting there.
Lanes that use the host's own login keep the old advice and the old hold.

## What changed

- Proxy lanes are recognised as staging's #1223 does (a remote lane on a `shed-acp` host); this branch no longer adds its own spawn-event mark.
- `hosts::hold_alert`: a proxy lane's `signed-out` reason names the token proxy, `[proxy] token-file` on the main host, the provider's setup command (`claude setup-token`) and `drover hosts on <host>`; no `claude /login`. Host-login lanes unchanged. The alert carries `proxy: true`, and a proxy lane's alert key includes its last `proxy-ok`, so a later real failure alerts again.
- `proxy`: when upstream answers 2xx for a lane that has a `signed-out` with no `proxy-ok` since, the proxy logs one passive `proxy-ok` event (agent, host). `relay` now reports whether upstream answered 2xx.
- `hosts::login_hold` (LOGIN_NEEDED): a proxy lane's `signed-out` holds its host only once `[limits] proxy_retry_secs` (new, default 300) has passed with no `proxy-ok` for that lane since it. Usage limits and host-login sign-outs hold as before.
- `docs/config.md` row and regenerated `docs/workflow.md` for the new limit.

src/ net +40 (no budget given): the proxy needs a success signal and the hold needs to read it; no new deps.

## Repro

Commit 1 (`bd5a4f7c`) adds only `a_proxy_lanes_auth_error_names_the_proxy_and_holds_its_host_unless_a_proxy_call_clears_it` in `crates/cli/tests/remote_acp.rs`. It spawns a real proxy lane on `boat1`, makes shed report an `auth:` error, runs `check`, checks the `signed-out` reason, then (in the retry case) sends one real request through `proxy::serve_on` with the lane's own key to a fake upstream, and spawns a second lane on `boat1` an hour later: held without the retry, not held with it.

On staging + commit 1 it fails: `cargo test -p drover --test remote_acp a_proxy_lanes_auth` →
`impl lane sendmeter-impl-12 is signed out on boat1; new lanes there wait: run `claude /login` there, then `drover hosts on boat1``.
With the `proxy-ok` emission stubbed out at the head, the retry case fails with `REFUSED LOGIN_NEEDED`, so the hold is lifted by the proxy's success and nothing else.

## Evidence (head b73095330ef15d920c366704a4f1888a1fbdeb22, staging merged in)

Round 5 (head b7309533): the FAIL at 10228a57 (test needed a real shed server) was fixed by 6626c1d0 (fake main-host shed); CI at 9cacca57 was fully green, test shards 1–4 included. Merged `origin/staging` (89dd3c99) again, clean, no conflicts.

- `cargo fmt --check && cargo test -p drover --test remote_acp` → exit 0 at b7309533: 15/15
- `XDG_RUNTIME_DIR=<empty dir> cargo test -p drover --test remote_acp a_proxy_lanes` (no shed server) → exit 0 at b7309533: 1/1

Earlier, at 9cacca57:

Update round: merged `origin/staging`. Conflict in `crates/core/src/hosts.rs` `hold_alert`: #1223 added its own proxy-lane test (remote + `shed-acp` runtime) and token-file advice. I kept #1223's test, dropped this branch's spawn-event `proxy` mark (one way to tell, not two), and kept this branch's advice (token proxy, `[proxy] token-file`, `claude setup-token`, host-on step) and alert key. #1223's new `remote_acp` test calls `login_hold` with the old two arguments; it now passes an hour past the retry window, so its "no hold" still means no hold.

- `cargo fmt --check` → exit 0 at 9cacca57
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` and CI's `--tests` clippy line → exit 0 at 9cacca57
- `XDG_RUNTIME_DIR=<empty dir> CARGO_INCREMENTAL=0 cargo test --locked --no-fail-fast -p drover --test remote_acp --test brief_golden --test sim` (lane env unset) → exit 0: remote_acp 15/15, brief_golden 3/3, sim 10/10
- same with `--test shed -p drover-core --lib` → exit 101: shed 59/60, the one failure is `a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which needs no `shed` on PATH and this host has one (as in earlier rounds)
- `cargo run -q --locked -p drover -- diagram` (just gen's step) → identical to `docs/workflow.md`
- the full workspace suite runs in CI (this host's disk filled during local builds)

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

## Why (round 4)

Decided: the proxy sign-out test attaches the suite's FakeShed as the main host's shed, so check reaches signed-out with no shed server on the runner
Because: crates/core/src/check.rs:73
Rejected: skipping the test when no shed server answers — the AC needs both cases covered in CI
Check: XDG_RUNTIME_DIR pointed at an empty dir, remote_acp passes 11/11; commit 1 plus this fixture still fails on the old advice
Owner: only the test changed; it now passes on CI runners that have no shed of their own

<sub>note on 6626c1d · git log --notes=why</sub>


