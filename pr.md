Closes #709

## For humans
If fleet.toml is broken, the board still loads with its last good data and shows a red line on every tab saying what is wrong and on which line.
You get one alert per new config error, and a "config ok" note once the file is fixed. No restart is needed.
A new health address tells a phone or script whether the config is ok or broken; no answer means the process is down.

## What changed
- `Config::load`: a load error is the one line `config invalid: <error> (fleet.toml:<line>)` (the toml span, or the first line holding the value a check quotes). tick, drive, check, the banner and health all show it.
- `Config::loaded` / `Config::kept`: each good load keeps its text as `fleet.good.toml` beside the state log; serve's reads (every `Config::load` in crates/server) fall back to it.
- Board: `needs.config` (API v1, additive) → a red `.cfgbad` line in the needs strip above every tab; serve's 15 s re-render pushes it, so it appears and clears without a restart.
- `check` (and `tick`, which runs it) alert `config-invalid` once per distinct error, through the last good config's notifiers (added to the webpush kinds), and log `config-ok` once it loads again.
- `GET /api/v1/health` → `{ok, config: "ok"|"invalid", error}`; documented in docs/serve.md.
- Scope item 1 (degraded serve on a locked store) dropped per amendment q857.

## Repro
Commit 1 (80330b6cb) is only `crates/cli/tests/config_invalid.rs`; `cargo test -p herdsman --test config_invalid` fails there (`/api/v1/health` → 403, no route) and passes at head. Two later edits to that test: `cargo fmt`, and the expected role in the message is `conductor` (the validator reports the inherited role first, as in the issue's own example `fleet.berth.role.conductor: account 'C'…`), not `impl` as I first guessed.

## Other test changes
- `board_errors.rs`: expected the old `TOML parse error` wording; now `config invalid: … (fleet.toml:1)`, the format this issue specifies.
- `golden/api_v1/needs.json`: re-blessed for the new `config` field only (the bless also rewrote seven unrelated fixtures' volatile values; reverted).
- `.gitignore`: `/demo/fleet.good.toml`, the copy the demo-export test (whose state log is the committed `demo/`) now leaves.

## Size
src/ net +94 (budget: none stated): one message builder, the last-good fallback, the alert, the health route; the serve call-site swap is net 0 (trimmed from +98). Cap raised to 38028 on staging (q967, PR #761; q968 decided no further trim).

## Merges
staging merged four times (the latest brings #707, no conflict; an earlier one conflicted in crates/server/src/serve/v1.rs: staging's `lane_list` refactor kept, its reads switched to the last-good fallback; staging's new lifeboat page reads it too).

## Fix round 2
- `ui_tokens`: the banner used a raw `#fff`; it is now a red border and red text on the scheme's `--r` token, like `.pend`/`.ask b`.
- `board-freeze`: the board files are the banner the issue's AC2 asks for; the gate needs a `board-fix` line in the issue. Asked as q973 (amend #709 with a `board-fix` line; the issue is itself a fix of #559). It stays red until that amendment lands.
- `usage::secret_in_config_is_refused` (found locally by the full run): the one-line message had turned a `SECRET_IN_CONFIG` refusal into a plain error (exit 1). A refusal now keeps its code with the one line as its text (`REFUSED SECRET_IN_CONFIG: config invalid: … (fleet.toml)`), exit 3 as before.
- `ui`: the one outlier, `loading~kitchen` `[↻] busy moved`, is a known flake in the loading audit. The same line also failed staging's run 42642603752 and #710's run 42650085142; this diff changes nothing in that view when the config loads.

## Evidence (at 4293df9853576f9ad3eacf5c8bb1385df4cb381a)
- `cargo build --release` → 0
- `cargo clippy --all-targets -- -D warnings` → 0
- `cargo test -p herdsman --test ui_tokens --test config_invalid --test board_errors --test api_v1_contract` → 0
- web `npm run build` → 0 (dist rebuilt for the CSS change)
- `herdsman cap-check --config ci/cap.toml` → 0 (36595 / 38028)
- `env -u HERDSMAN_LANE cargo test -p herdsman --test usage --test config_invalid --test board_errors` → 0
- `cargo test` (with the lane's `HERDSMAN_LANE` set) → 101, only at `self_update::pause_and_resume_are_logged_and_pause_stops_the_job`: it records `by` from that variable (`herdsman:709:impl` ≠ `owner`); `env -u HERDSMAN_LANE cargo test -p herdsman --test self_update` → 0
- `env -u HERDSMAN_LANE cargo test --no-fail-fast` → 101: 157 suites ok, 1 failing: `outbox` (`herdr agent list failed` under the parallel load; it passed in the run before this commit). `env -u HERDSMAN_LANE cargo test -p herdsman --test outbox` → 0 three times in a row

Not verified: AC3 live (locking the host Keychain) is moot under amendment q857; the banner was not checked on a live board.
