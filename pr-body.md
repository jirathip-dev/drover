Closes #715

## For humans
The phone board no longer goes dead while a thread's conductor is starting up. When you answer a question or settle a card in a thread whose conductor is asleep, the board replies right away and wakes the conductor in the background. If that wake-up fails or times out, the thread shows a red "not delivered" line with a retry button, and your message stays in the thread.

## What changed
- `Server::send` holds the per-device send-limit lock only to check and record the send. It is released before `/thread`, `/setup`, `/answer`, `/act`, `/report` and `/send` run.
- `conductor::say` can now defer: inside `conductor::deferred(..)` it logs the message and returns the delivery instead of starting the conductor. The board wraps `POST /thread` and the API v1 synchronous ops in it. That covers thread asks (`answer`) and run, config and secret cards, which used to start a conductor on the request thread while holding the lock. This is the incident in #715. The CLI path is unchanged and still delivers synchronously.
- Deliveries (the existing #403 `later` queue) now run after the request has released its slot in the 16-request pool, so no counted request thread waits on a conductor launch. `Server::respond`, which tests use, still delivers before returning.
- #717 (amendment q864): a failed background start shows `✗ not delivered: …` in the thread (the existing `chat::deliver` path), and the triggering message stays logged. Before this change, a thread answer whose start failed got a 409 instead.
- No new UI text was added for a thread whose conductor is starting (the board is frozen to fixes only). The thread shows the logged message at once and the conductor's state as before.

## Repro
Commit 1 (713b8ef78) is the test only: `cargo test -p herdsman --test conductor_cold_start`.
- On staging + commit 1, exit 101: device B's send got no reply within 2 s while A's answer was blocked in `herdr agent start`, and the failed start came back as `409 Conflict … herdr agent start failed`.
- At head, exit 0.

## Evidence (head d73a0b3a7317aa6b854d3afb83587c34a0b94e8a)
- `cargo build --release --locked` → exit 0
- `cargo clippy --all-targets --locked -- -D warnings` → exit 0
- `env -u HERDSMAN_LANE cargo test --locked` → exit 0 (1181 passed, 0 failed). With the lane's `HERDSMAN_LANE` set, `self_update::pause_and_resume_are_logged_and_pause_stops_the_job` records `by: herdsman:715:impl`. That comes from the environment only and is unrelated to this change; it passes with the variable unset, as in CI.
- `cargo test -p herdsman --test conductor_cold_start --test chat_tab --test conductor_adopt --test conductor_cards --test asks --test lanes_sheet --test config_card` → exit 0 (the existing 10-sends-a-minute test in lanes_sheet still passes)
- `herdsman cap-check --config ci/cap.toml` → exit 0 (34533 / 35500)
- `cargo fmt --check` → exit 0

src/ net +37 (no budget stated): a scoped lock, the thread-local deferral in `say`, and moving the delivery after the slot is freed.
