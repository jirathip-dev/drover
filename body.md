Closes #1057

Built to the approved ADR in `docs/architecture/orchestrator-chat-profile/` (q1714). The issue's board-fix amendment (q1714, confirmed q1715) covers the 3 router lines in `serve.rs`.

## For humans

Cooee can now ask drover for its orchestrators as ready-made chat profiles. Each one carries your own orchestrator instructions file, read again on every request. Editing that file changes both drover threads and cooee chats, with no second copy and no restart.
A cooee chat started from one of these profiles runs under the same drover guard as an orchestrator thread: the same refusals (merging stays gated) and never more powers.

## What changed

- `GET /cooee/profiles` and `POST /cooee/thread-started` (plugin token only) in a new `crates/server/src/cooee.rs`; `serve.rs` gets the module line and one `/cooee/` arm.
- One profile per orchestrator (`default`, then each `[orchestrator.<id>]`). Fields:
  - name = its id, label = its name, agent = the role's ACP agent;
  - settings = the orchestrator lane profile (through the new `Provider::chat_settings`);
  - env `DROVER_LANE=<fleet>:-:orchestrator`, `DROVER_ORCHESTRATOR=<id>`, `DROVER_THREAD={thread}`.
  
  A profile stays hidden, with a `why`, until its hook is proven (`unguarded`).
- The instructions file is `<state dir>/chat/<id>.md`, rewritten on each request when its text changed (temp file, then rename). It holds the role part, `roles/orchestrator/chat.md`, the usage doc and the seat lines. It ends with the same `standing()` call as the thread brief (instructions, memory, groups, skills).
- `roles/orchestrator.md` is split into `roles/orchestrator/{role,thread,chat}.md`. A thread still gets role + thread. A chat gets role + chat, with no `thread reply`, `--typed`, `card post`, `config propose`, `secret request` or `thread handoff`.
- `thread-started` logs `orchestrator-attached` with `via: profile`, once per thread. `route()` skips those links, so lane asks never go to a cooee chat.
- `[orchestrator.<id>]` ids are checked at load (`config::orchestrator_ids`): lowercase letters, digits and `-`, and not `default`.
- Docs: `docs/cooee-profiles.md`. `.drover/arch.toml` gets no lines, because the ADR ends with none.

src/ net +271 (budget ~+150). The ADR estimated low: the routes file, the brief split and the profile type each came out larger. Round 3 trimmed 12 lines: the profile type now derives Default and keeps one `hidden()` helper.

**Line cap:** within it. q1802 answered with a queue-wide raise. That raise (#1201, 53,500) is now on staging, so it is merged in here. `drover cap-check --config ci/cap.toml` at the head: exit 0, 47994 / 53500.

## Evidence (head 2797f6ca361d9909577d1ecb592fa7d4727bf03a)

Round 4: CI's red `rust` check was the line cap. This round merges origin/staging d0cba31, which carries the cap raise (#1201). The merge was clean. Staging grew `serve.rs` to 3591 lines, so its size baseline becomes 3594 (staging's 3591 plus the same 3 router lines that q1714 allows).

Results at 2797f6c:
- `cargo fmt --all --check`: exit 0.
- Both CI clippy commands (`--lib --bins -D warnings`, then `--tests` with CI's `-A` list): exit 0.
- `drover cap-check --config ci/cap.toml`: exit 0 (47994 / 53500).
- `drover arch-check`: exit 1. The one violation is `just gen` exiting 127 because `just` is not installed on this host. The `serve.rs` size violation is fixed by the baseline line.
- `CARGO_INCREMENTAL=0 cargo test --locked -p drover-core --lib`: exit 0. `-p drover-server --lib`: exit 0. `-p drover --test cooee_profiles --test setup_neutral --test cooee_screens`: exit 0.
- Not run here: the full `cargo test --workspace`. The host's disk filled up while linking it (ENOSPC, 1 GB free), so CI's test shards are the full-run evidence.

## Earlier evidence (head bd4341e097e6204cbc376d8c157bf1f740d2eed7)

Round 3 fixes the review finding and merges origin/staging up to 80a3be3 (#1181, #1186 and others):
- `scripts/test-shards.txt`: `2 cooee_profiles` (the target was in no shard). `scripts/test-shards.sh check` exit 0.
- `crates/cli/tests/cooee_profiles.rs`: the started-chat test clears `DROVER_OUTBOX`, `DROVER_HOST` and `DROVER_LANE` first, so it also passes in a lane shell.
- Staging's #809 on/off wording is carried into `orchestrator/brief.rs`.
- Staging's new hotspots ADR (`docs/architecture/hotspots-2026-11-09/`) now names `roles/orchestrator/role.md`.
- Both orchestrator id checks moved into `config::orchestrator_ids`. `Config::parse` fits staging's 170 baseline again, so that raise is dropped. The baselines raise only `serve.rs` (+2) and `serve.rs:route` (+1), the router lines q1714 allows.

Results:
- `cargo fmt --check`: exit 0.
- Both CI clippy commands: exit 0 (lib/bins with `-D warnings`; tests with CI's `-A` list).
- `cargo test --workspace --locked --no-fail-fast` with the lane env unset: exit 0, all 217 test binaries ok. That run was at b9a9d5c. bd4341e differs from it only by a 2-line `hold()` change and two baseline lines.
- At bd4341e: `cargo test --locked -p drover --test cooee_profiles --test setup_neutral --test orchestrator_lanes --test config_invalid` exit 0, and `cargo test --locked -p drover-core --lib` exit 0. `cooee_profiles` alone in the lane env also exits 0.
- A first workspace run without `--no-fail-fast` hit one `service.rs` timing failure ("never: the spawn reached GitHub"). It passes when rerun on its own (exit 0), and passed in the full run.
- `drover arch-check --base origin/staging`: exit 1. The only violation is `just gen` exiting 127, because `just` is not installed on this host.
- `drover lesson-check`: exit 0.
- `drover cap-check --config ci/cap.toml`: exit 1 (over the cap, see above).

## Live check (drover side, release binary at 8e41038a8, before the staging merge)

Setup: a scratch config (fleet `herdsman`, impl harness on the ACP provider, `[orchestrator] instructions = ~/claude-conductor/CLAUDE.md`), then `drover profiles sync`, then `drover serve` on `$DROVER_PORT`.

- `GET /cooee/profiles` without the token answered 401. With it, the answer was one profile, `default` / `Conductor`, agent `claude-acp`, env as above.
- The served instructions file contains the full text of `~/claude-conductor/CLAUDE.md` (a 41 KB brief).
- I ran the served settings' hook with the served env (`DROVER_THREAD=live-1`) on a Bash merge command (`gh pr` + `merge 5`). It answered `permissionDecision: deny`: "drover: merging is gated: run `drover merge <fleet> <issue>`".
- `POST /cooee/thread-started {"thread":"live-1","profile":"default"}` answered `live-1 is default's chat on herdsman`.

Not verified: a real cooee chat on a persona that names `drover/default`. It needs cooee's paired issue (a persona takes its text from the profile it names), which has not landed. Per the ADR, the provider's config folder used as cooee's settings variable also needs one sign-in there.

## Why (round 1)

Decided: update round after #927 and #1069: #927's edit to roles/orchestrator.md is carried into the split roles/orchestrator/role.md; #1069 merged clean
Because: roles/orchestrator/role.md:21
Decided: the chat brief is rebuilt and written on each GET /cooee/profiles, only when its text changed
Because: "editing that file changes the served text with no restart"
Decided: thread-started links once with via: profile and route() skips such links, so lane asks never reach a cooee chat
Because: crates/core/src/orchestrator.rs:477
Rejected: restoring roles/orchestrator.md from staging — the split files replace it; two copies would drift
Check: role.md carries staging's #927 line; the live cooee chat waits on cooee's paired persona issue
Owner: Staging's latest changes are merged in; cooee chat profiles work as before.

<sub>note on 8bd50dc · git log --notes=why</sub>

## Why (round 2)

Decided: merge origin/staging; profile() stays in orchestrator/brief.rs with staging's #809 no-indexing change, and staging's #973 cap line goes into roles/orchestrator/role.md
Because: roles/orchestrator/role.md:34
Decided: raise the serve.rs baselines by the amendment's router lines; the id check moves out of Config::parse
Because: q1714
Rejected: restoring roles/orchestrator.md from staging — the split files replace it; two copies would drift
Check: shed.rs missing-shed test fails only on hosts with shed installed; just gen ran by hand with no diff
Owner: Staging's latest changes are merged in; cooee chat profiles work as before.

<sub>note on 012576b · git log --notes=why</sub>


