# The terminal app: Threads and Board (#1352, #1354)

```
drover board              the app on its Board tab (stdout a terminal, not in a lane)
drover board <fleet>      the same, on that fleet's queue
drover board | cat        the compact table, as before (a pipe, a lane, a test)
drover chat [thread]      the app on its Threads tab
```

One full-screen app with two tabs; Tab switches between them and each keeps its place. Threads is the orchestrator
chat (docs/orchestrator.md). Board is the phone's board drawn as text: Queue, Events and Lanes, the asks and cards,
the issue and verdict sheets, backlog, hosts, fleets and workflow. It needs no `drover serve`: the app builds the
phone's own screens (`crates/server/src/screens.rs`) in its own process through `serve::Local`, runs their actions
through the same handler (the same permissions and `REFUSED <CODE>` lines), and builds the screens on view again
when the log or the snapshot moves, so a change shows within a second or two. A screen, block or button added to
the screens later shows up here with no terminal change; a block type the app does not know draws its text.

Not offered: Scene (drawn on the phone only).
Images are named, and shown on the phone.

The design, with every key: `docs/design/terminal-board/README.md`; the architecture:
`docs/architecture/terminal-board/`. Every action takes two presses: its digit (or Enter, when the hot has one
action and nothing to open) shows exactly what will be sent or run, Enter sends it, Esc or ⌫ backs out. It works at
40 x 18 with only keys a phone keyboard has. Ctrl-C quits and restores the terminal; a running action finishes first.

The lane hook refuses `drover chat` in a lane (`OWNER_ONLY`); `drover board` in a lane prints the table, because
`DROVER_LANE` is set.
