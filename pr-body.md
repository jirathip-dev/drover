Closes #702

## For humans
Run cards already show in the phone chat, in order, with approve and deny, and their result after settling. That came with #669, which merged a few hours before this bug was seen, so the board serving the phone then was most likely an older build.
This PR adds the two tests the issue asks for, so the chat can't silently drop cards or their results again.

## What changed
- `crates/cli/tests/conductor_cards.rs`: two tests (AC3), sharing one setup. A thread holds a conductor reply, run card `r1`, then another reply.
  - `a_run_card_between_two_replies_renders_in_order_in_the_chat_tab`: API v1 `chat` JSON has `msg, run, msg` in order. In the chat tab HTML, the reply comes first, then the card's command, why and touches, then the open `[approve]`/`[deny]` form, then the second reply (AC1).
  - `a_settled_run_cards_result_renders_in_the_chat_tab`: after `herdsman card approve r1 --sha …` (the CLI path from the repro), the row shows `✓ approved by owner · exit 0` with the output tail and no buttons. The thread's result line `run r1 approved by owner, ran in /srv/app: exit 0` renders after it (AC2).
- No production change: `crates/server/src/serve/chat.rs` (`run_card`, the `("run-card", _)` arm) and `web/src/chat.ts` (`case 'run'`) already meet AC1/AC2 at this base.

## Repro
This issue is queued `--repro`, but at the current base the bug no longer reproduces: #669 (merge 43e369057, 2026-11-08 10:30 +0700) added the run-card row before the 13:08 live repro. Since the new tests pass at the base, commit 1 is not a failing test. Instead it is checked against the commit before #669:
```
# a887fa0c8 (first parent of 43e369057) with this PR's test file, in $HERDSMAN_SCRATCH_TARGET
cargo test -p herdsman --test conductor_cards -- a_run_card_between a_settled
exit 101
  left: [("msg", "before the card"), ("msg", "after the card")]
 right: [("msg", "before the card"), ("run", "uptime"), ("msg", "after the card")]
<pre class=rc>uptime</pre> not in <div class=cvp data-t="t1">…
```
That matches the live report exactly: both replies show, the card does not. The board running now (`herdsman 0.1.0 (c51fc1d3b)`, restarted 2026-11-09 17:37) includes #669.

## Evidence (head af5338012faa3773f10bf28d6b043f65a5f028f3)
- `cargo test -p herdsman --test conductor_cards`: exit 0 (7 passed)
- `cargo clippy -p herdsman --tests -- -D warnings`: exit 0
- `cargo fmt --check`: exit 0

src/ net +0 (budget n/a): tests only; the fix already shipped in #669.
