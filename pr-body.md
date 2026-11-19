Closes #1335

## For humans
Drover used to treat a boat's model-call tunnel as working whenever its local ssh process was alive. Now it checks the tunnel from the boat's side, clears a port that a dead old session still holds, and restarts the tunnel itself.
A boat whose tunnel is down shows "tunnel down" and gets no new lanes until the tunnel answers again. A respawn moves off that boat.
A lane that died because the tunnel refused its connection says "ECONNREFUSED: proxy tunnel down", not "adapter crash".

## What changed
- `proxy::ensure` returns whether the tunnel answers. It counts as up only when the local `ssh -R` pid is alive **and** `curl 127.0.0.1:<port>`, run on the host (through `hosts::On`), gets an answer. Otherwise it kills the local ssh, frees the remote port, restarts the forward and probes once more, so a fresh tunnel that answers is up straight away.
- **Recovery choice: free the stale remote listener.** On the host, `ss -Htlnp "sport = :<port>"` (then `sudo -n ss`) finds the pid holding the port, and `kill` (then `sudo -n kill`) stops it. I didn't pick a per-attempt port because every lane's `ANTHROPIC_BASE_URL` would have to follow it. I didn't pick sshd `ClientAliveInterval` because it is a boat config change a lane can't make.
- `survey` keeps a `✗ tunnel down: …` health row. Placement already refuses an unhealthy host (`NO_HOST`), and the board/`hosts` show `unhealthy · tunnel down: …`. Doctor's proxy facts say `tunnel <host> down`.
- `place`: an unhealthy last host (`NO_HOST`) no longer holds a respawn, unless its worktree there has unpushed work. This fixes "respawns them onto the same dead host".
- New `Cause::Refused`, logged as `ECONNREFUSED: proxy tunnel down`. It covers a crash whose note or last words contain `ECONNREFUSED`. It is resumable, shows in `waiting`/`status` as `lane gone (ECONNREFUSED: proxy tunnel down)`, and is not counted as an adapter-crash wave.
- docs/hosts.md: tunnel paragraph updated.

src/ net +39 (budget: none given). It is small because it reuses the health row and the `NO_HOST` placement path.

## Evidence (head 1d5ccaf9c017e5f0b9a7c7d7f5558488f36f5ab9)
- Commit 1 (02b37d7f), test only, fails on staging: `cargo test --locked -p drover --test remote_acp -- far_end forward_that` exit 101 (boat1 shown `up`), `cargo test --locked -p drover --test shed_native -- econnrefused` exit 101 (cause `adapter crash`).
- At head: `cargo test -q --locked -p drover --test remote_acp -- far_end forward_that respawn_leaves` exit 0. `cargo test -q --locked -p drover --test shed_native -- econnrefused` exit 0. `cargo test -q --locked -p drover --test remote_acp` exit 0 (22 passed).
- `cargo test --workspace --locked --no-fail-fast` exit 101. The one failure is `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, and it fails the same way on untouched `origin/staging` in this lane (environmental: doctor's local shed PATH check). Every other test passes.
- The two CI clippy commands exit 0. `cargo fmt --check` exit 0. `drover cap-check --config ci/cap.toml` exit 0 (53541 / 55500). `drover arch-check --base origin/staging` reports one violation, `just` not installed in this lane (generated-file check); none of its inputs changed.
- Test setup edits after commit 1: the ECONNREFUSED test reads `waiting` 2 min after the spawn (waiting holds a fresh lane). The forward-failure test scripts the probe right after the restart as still failing. Three older remote_acp tests script the boat's probe as answering.
