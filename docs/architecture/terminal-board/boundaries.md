# Terminal board: module boundaries (#1354)

## Who may call whom

```
 crates/cli  main ──sets APP──▶ crates/core  cli: Cmd::Board, Cmd::Chat ──APP(Open)──▶ crates/chat  main

 crates/chat
   lib (frame: loop, tabs, top line, health)
     ├─▶ threads (today's chat) ─────────────────────────▶ crates/core  orchestrator, answer, card
     └─▶ board (stack, keys, pick)
            ├─▶ lay (pure: tree ─▶ lines and hots) ─▶ ui (wrap, box, tones, panel)
            ├─▶ feed thread ──▶ Local::refresh, stamp, screen ─┐
            └─▶ one action thread ──▶ Local::act ──────────────┤
                                                               ▼
 crates/server  serve::Local ─▶ Server::screen, Server::perform ─▶ the screen builders, the action handler ─▶ crates/core
                GET /screens/<x>, POST /action ─▶ the same two
```

New code goes in new files; the two big server files stand still or shrink:

```
crates/core/src/lib.rs               Open, APP (CHAT renamed)
crates/core/src/cli.rs               the Board and Chat arms
crates/server/src/screens_local.rs   new: Local, Place, Done, Who, Server::perform, the pruning
crates/server/src/screens.rs         the /action arm calls Server::perform; fewer lines than today
crates/server/src/serve.rs           one line: pub use of Local
crates/chat/src/lib.rs               main, App, the loop, the top line, health
crates/chat/src/ui.rs                new: wrap, boxed, clamp, tones, the panel (out of view.rs)
crates/chat/src/threads.rs           today's chat state and keys (out of lib.rs, unchanged)
crates/chat/src/threads/view.rs      today's chat drawing (view.rs, moved)
crates/chat/src/board.rs             new: Board, View, Ui, keys, the pick
crates/chat/src/board/feed.rs        new: the feed thread
crates/chat/src/board/lay.rs         new: the walk, the fallback, text, ticking, taps and sends
crates/chat/src/board/rows.rs        new: screen, sheet, section, list, row, stat, button
crates/chat/src/board/cards.rs       new: card, choice, run, images, diagram
crates/chat/src/board/forms.rs       new: form and its fields, chips, composer
crates/cli/src/main.rs               APP.set(drover_chat::main)
crates/cli/tests/local_screens.rs    new: the door: parity with HTTP, refusals, pruning
crates/cli/tests/board_tui.rs        new: the tab: every screen at 40 and 120, keys, actions, live
docs/terminal.md                     new: the app, its tabs and keys; docs/orchestrator.md links to it
```

| Module | Owns | May call | Must not |
|---|---|---|---|
| core `cli` | the `board` and `chat` arms, `Open`, the lane test, today's table | `APP` | name the chat or server crate |
| server `serve::Local` (new) | the door: `screen`, `act`, `places`, `home`, `refresh`, `stamp`; what the terminal is not offered | `Server`'s builders and handler, core | draw, format HTTP, listen, keep a job queue |
| server `Server::perform` (moved) | permission, `later`, host actions, the handler, the deferred deliveries | as the HTTP arm does today | know who draws the reply |
| chat `lib` | the terminal (init, restore), the loop, `Tab`, the top line, health | `threads`, `board`, `ui` | build a screen, run an action |
| chat `threads` | the Threads tab as built | core's `orchestrator`, `answer`, `card` | the server crate |
| chat `board` | places, the stack, `Ui`, keys, the pick | `lay`, `ui`, the feed's channel, one action thread | name a screen path or an action; call core's commands; wait on a thread |
| chat `board::feed` | the watch list, the rebuild rule | `Local::refresh`, `stamp`, `screen` | draw, touch `Ui` |
| chat `board::lay` | tree to lines and hots, the action bodies | `ui` | any I/O, the clock (`now` is passed), `Local` |
| chat `ui` | wrap, box, tones, glyphs, the panel | ratatui | know a block or a thread |

Rules that hold across the table:

- `ratatui` (and its crossterm) is used in `crates/chat` only. The server and core stay data.
- The app uses one item of the server crate: `serve::Local`. It opens no socket and formats no request.
- The Board names no screen path and no action name. Paths come from places and from `tap`s; action names come
  from the tree. `Local::home(fleet)` gives the first path.
- Only the feed thread and one action thread at a time call the door. The draw loop reads the last trees it was
  sent and never waits: a slow read shows as an old `as of` age on the screen, not as a frozen key.
- `lay` is the only place a `Send` is built, and `Local::act` the only place one goes.
- The Threads tab's own answers and cards keep going to core directly, as #1352 built them.

The four `[[ban]]` entries at the end of adr.md check the first three rules in CI. The Cargo manifests check the
rest of the first: only `crates/chat` lists ratatui.

## What each slice touches

| Slice | Files | Shared or near-full files |
|---|---|---|
| 1 the door | the server's new file, `screens.rs`, `serve.rs`, the door's test | `screens.rs` shrinks; `serve.rs` +1 line (baseline 3,594, today 3,464) |
| 2 the tab, read-only | core `lib.rs` and `cli.rs`, the binary, all of `crates/chat`, the tab's test, `.drover/arch.toml` | `cli.rs` and `lib.rs` are `shared`: a few lines each |
| 3 actions and docs | `board.rs`, `lay.rs`, `forms.rs`, `cards.rs`, `ui.rs`, the docs, the hook's test | none |

Slice 1 has no terminal code and slice 2 no server code, so they do not conflict with each other's reviews.
