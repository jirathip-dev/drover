# ADR: the terminal board draws the phone's screens in-process (#1354)

## Summary

Decision: the Board tab is a second renderer of the phone's screens. The app builds each screen in its own process
with the code the phone page uses and draws the blocks as text; an action runs the phone's handler, with its
permissions and refusals. It reads again when the log or the snapshot changes, so it is live with serve stopped.
Slices: 1 the in-process door to the screens and actions; 2 the Board tab, read-only and live; 3 actions, forms, docs.
Open questions: (1) Threads stays as built, so the phone's thread extras stay off the terminal (my pick), or a
follow-up moves it onto the blocks? (2) a terminal write is logged like a phone write (my pick), or as `term`?

Status: proposed (architect lane, 2026-11-19). Read `README.md` first.

## Question

How does a terminal draw every screen the phone draws, run every action they offer and stay live, with no
`drover serve`, so that a screen, block or button added to the screens later shows up with no terminal change?

## Where it stands

- `crates/server/src/screens.rs:1439` builds every screen from a path and a query. `crates/server/src/screens.rs:1683`
  runs every action. Both are private methods of `Server`, reached only through HTTP text
  (`crates/server/src/screens.rs:1625`).
- `Server` needs no socket: `crates/server/src/serve.rs:746` makes one from a context and a runner, and the static
  export already uses one that never listens.
- The permission check, the `later` and host actions and the reply's status sit inline in the HTTP arm
  (`crates/server/src/screens.rs:1648`), so nothing else can run "the same handler" today.
- A stream (`crates/server/src/screens.rs:1897`) is a loop: every 500 ms, if the log's last seq moved or 30 s
  passed, build the screen again and send the difference.
- The snapshot the screens replay is taken by the timer's fast step, not by serve.
- `crates/chat` is the app since #1352: `crates/chat/src/lib.rs:342` toggles a `board` flag that draws one grey
  line. It depends on core only. Its two-press panel calls core's `answer` and `card::settle` directly.
- `web/src/blocks.ts` is the phone's renderer: one function per block type. `header-strip` and `menu-row` are places
  from the manifest, not blocks; `toggle`, `picker` and `secret` are form fields; `diagram` has no phone renderer.
- `crates/server/src/screens.rs` is 1,990 lines of the 2,000 a file may have.

## Options

1. **Speak HTTP to a `Server` in the same process.** Format a request, parse the reply. It works today, but it is
   HTTP in all but the socket: pairing cookies, Host checks and status codes for a caller who is the owner at a
   shell. The issue rules it out.
2. **A terminal board of its own over core** (as the chat tab is). A second definition of every screen: each new
   row, badge or button is written twice. The #927 rule exists to stop this.
3. **Move the screen builders into core.** The right home in the long run, but they are 2,000 lines tied to
   `Server`'s caches, and `screens.rs` and `serve.rs` are files every board PR touches. Not this issue.
4. **A door on the server crate, a generic renderer in the app (chosen).** One small public type runs the builder
   and the handler in-process. The app draws whatever tree comes back.
5. **Typed blocks** (a Rust enum per block type, parsed from the JSON). A new block or field would then need a
   terminal change before it shows. The renderer reads the JSON tree by field name instead.

## Decision

### The door

`serve::Local` wraps a `Server` that never listens. It is paired by definition: the owner at a shell on the main host.

- `screen(now, path)` runs the same builder as `GET /screens/<x>`, in the same request scope, so a build never
  starts a process. `/screens/cards` comes back wrapped as a screen with id `cards`.
- `act(now, body)` runs the code of `POST /action`: the `permitted` check, then `later`, a host action or the
  handler, then the deliveries the handler deferred. That code moves out of the HTTP arm into one function,
  `Server::perform`, that the arm and the door both call. The body is the phone's:
  `{screen, block, action, values}`.
- A refusal is returned as its line, `REFUSED <CODE>: why`. An action a paired phone is refused as `NOT_PERMITTED`
  is refused here too. The door has no job queue, so an action runs to its end and its result is the real one.
- `refresh(now)` reads again what serve's background threads read (shed, remote hosts, the hosts sheet's facts),
  each at serve's own interval for it.
- What this client does not offer is pruned here, so the app stays generic: the Herd place; any row, button or menu
  item that opens the proxy-token sheet; the Threads place (the app has the tab). A `thread` tap that names a lane
  becomes that lane's sheet. Workflow is added as a menu place: the phone page has no row that opens it.

### The renderer

`lay(tree, width, ui, now)` is a pure function to lines and hots (shapes.md). Every text is wrapped; none is cut.

| Block | Drawn as | Hot |
|---|---|---|
| `screen` | title; its `actions` as a bar of buttons (a `menu` flattened, as the phone does); its blocks; `badge` on the tab label | |
| `sheet` | a view pushed on the stack: title, blocks, `done` as the last button; `cancel` names what Esc does | `done` |
| `section` | `header` in bold, blocks, `footer` in grey | |
| `card` | an edge in its tone; title, badge, meta, subtitle; blocks; `buttons`. With `fold`: its one `line` until opened | the folded line: Enter opens, `main` is its action |
| `list` | its rows in order, whatever their type | |
| `row` | figure glyph, title, then value and badge; subtitle under it; `indent` two columns a level; a chevron for `accessory` | `tap`; each `swipe` item an action |
| `text` | `markdown` wrapped: fences boxed, bold kept, a link as its text and URL | |
| `button` | its label, coloured by `style` | `tap` or its action |
| `form` | its fields, then `submit` as a button | `submit` |
| `toggle` field | a ticked or empty box and its label | Enter flips it; nothing is sent |
| `text` field | label and value, the placeholder in grey | Enter edits it in place; nothing is sent |
| `picker` field | label and value | its options are its actions; nothing is sent |
| `secret` field | not drawn: one line, "enter it on a paired device"; its form gets no `submit` | |
| `chips` | its options on a wrapped line, the selected ones reversed | with an `action`, each option an action |
| `choice` | its options numbered, the recommended one first and marked; `default` as a ticking line | each option an action; `other` asks for a typed line |
| `stat` | `label value` pairs on a wrapped line; `spark` as eighth-block bars | |
| `run` | the command, `dir`, `touches`, `sha`, time left | approve, deny |
| `images` | one line an item with its `alt`, and "shown on the phone" | |
| `diagram` | its `source` in a code box: the Mermaid text, which carries the live counts | |
| `composer` | a text field with its send action (its queue action while `working`); `queued` as rows; `chips` as pickers; no files | as a field and a button |
| `scene` | not drawn: one line, "Herd is drawn on the phone" (its place is absent, so it is not reached) | |
| `menu-row`, `header-strip` | places, not blocks: `Local::places()`. Header-strip places are the Board's screens, menu rows open as sheets | a row each |
| any other | every text field it has, in the order of the text-key list; then its `blocks`, `rows`, `fields`, `buttons`, `items`, `options` | if it has `tap` or `action`, as a button |

Nothing in `lay` can panic: every read is by name with an empty default, and the workspace lints refuse the rest.

**Tones and badges.** One table maps a tone to a style, as the phone's stylesheet does: `ok`, `working` green;
`warn`, `needs` amber; `bad`, `fail` red; `accent`, `done` blue; `mute`, `idle` and any unknown tone grey. A badge
is its text in its tone. A figure is the terminal theme's glyph for its `state` (`●` working, `!` needs, `✗` fail,
`✓` done, `○` idle); `coat`, `tool` and `grow` pick sprites on the phone and are not drawn. Colour never carries
meaning alone: the selection is a marker and reverse video, a state is a glyph or a word.

**Ticking values.** `{since, label}`, `{since}` and `{until, label}` are formatted at draw time by the phone's rules
(`web/src/blocks.ts:49`), from the app's clock. The loop draws every 500 ms, so ages tick with no rebuild.

**`tap`.** `sheet` pushes a view. `open` pushes one too, but replaces the top view when it is the same screen with
another query (`+5 more`, a fold) or carries `keep`. `thread` switches to the Threads tab on that thread. `url` and
`copy` open a panel with the full text, for the terminal's own link tap and selection. An unknown `tap` is not a hot.

**`action` and `values`.** `lay` builds for each action the body the phone would post, by the phone's rules:

| Source | `block` | `values` |
|---|---|---|
| button, swipe item | the button's or the row's id | its `values` |
| choice option | the choice's id | the choice's `values`, the option's, and `choice` |
| choice `other` | the choice's id | the choice's `values`, `choice` (`other.for`) and `other` (the typed line) |
| run | the run's id | `choice` (approve or deny) and `sha` |
| chips | the chips' id | its `name` with the next selection (the phone's `all` and `multi` rule) |
| folded card's `main` | `main.block` | `main.values` and `choice` (`main.value`) |
| screen action, menu item | none | its `values` |
| form `submit` | the form's id | `submit.values` and every field |
| sheet `done` | the sheet's id | `done.values` and every field in the sheet |

`screen` is the id of the tree the block was drawn in. Every action is two presses: its digit opens the panel,
which shows the label, the `confirm` title and message when there are any, the action and its values; Enter sends
it, Esc or Backspace drops it. Local edits (a toggle, a field, a picker) send nothing until `submit` or `done`.
One action is in flight at a time. Its reply's `toast` shows on one line; `close` pops the sheet; a `screen` op
replaces the view's tree; the other ops are not applied, because the feed is kicked and the rebuilt tree is the truth.

**Navigation.** The Board is a stack of views over one root place (Queue, Events or Lanes), with the cards above
the root as on the phone. Esc or Backspace pops one view. A sheet is a view marked `sheet`: at 40 columns it takes
the screen; the design decides its frame when there is room. Tab switches tabs and both keep their stack, cursor
and scroll. A view's cursor, scroll, opened cards and edits are kept by block id and field name, so a rebuild that
moves a row keeps the cursor on it.

**The streams, as an in-process refresh.** One feed thread per app. Every 500 ms it calls `refresh`, then builds
the views on screen again if the log's last seq or the snapshot's stamp moved, if the app kicked it (an own action,
a navigation) or after 30 s, and sends the trees that differ. The app swaps a tree whole; ratatui's buffer diff
writes only the changed cells, so there is no full redraw and no flicker. A change shows in about a second.

- A view replaced by an action's `screen` op follows the new tree's `stream` path when it has one (a filter on
  Queue or Events). Without one, the view's own path is built once more: the same tree means the view stays
  live (Hosts after a host action); another means it is held as replied until it is closed (a filtered backlog).
- A block with `more` loads its next page when the cursor reaches its last row; the view's path then carries the
  `to=<cursor>` the stream uses, so a rebuild keeps every loaded row.
- A tree with a block whose id is `loading` (a lane's transcript not read yet) is built again on the next tick,
  as the phone fetches it again each second.
- A path that no longer builds (a closed lane's sheet) shows one line and pops on Esc.

### Where the code lives

The door is one new file in the server crate, a child of the screens module. The app stays `crates/chat`: one
frame, a `threads` module (today's code, moved) and a `board` module (boundaries.md). `ratatui` and its crossterm
stay in that crate only. `drover board` opens the app when stdout is a terminal and it is not in a lane; else it
prints today's table through today's code. The hook keeps refusing `drover chat` in a lane and never refuses
`drover board`, whose table lanes read: there the lane variable alone decides, and a hook test holds both lines.

### The chat tab stays as built

The Threads tab does not move onto the threads screen's blocks. Reasons:

- It is typing-first: every printable key goes to the box. On the Board every printable key is a command. A generic
  `composer` inside a block list needs a focus mode the shipped chat was designed to avoid.
- Its design was approved and built in #1352, merged the day this was written. Moving it is a rewrite of an
  approved design with nothing new to see, and this issue adds no screens or actions.
- It needs no server crate and no screen: with a broken board it still works, which is why #1352 exists.

What the two tabs share: the frame, the top line, the two-press panel's drawing and the wrap, box and tone helpers.
A `thread` tap on the Board lands in the Threads tab. Config proposals, asks and run cards are on the Board too,
through the cards. The phone's thread extras (new thread, archive, queue and steer, the model pick) stay phone and
CLI only. `composer` is still drawn by `lay`, so a move later is a change of one tab's source (open question 1).

## Consequences

- One definition holds: a new screen, row, badge, button or action needs no terminal change. A new block type is
  drawn from its text fields until someone gives it a nicer renderer.
- A parity test compares `Local::screen` with the HTTP body for every screen, so the two cannot drift.
- `screens.rs` gets smaller (the action arm moves out); `serve.rs` gains one line, under its baseline.
- `crates/chat` now depends on the server crate. The binary already links both.
- A terminal write is logged as the screens log it today (`by: owner`, `via: cooee` or `web`): the log says "through
  the screens", not which client. Naming the client needs a `via` threaded through the handler (open question 2).
- Images, the Herd scene and secret entry are not drawn. A design ask's PNGs are named, and seen on the phone.
- An action runs to its end even after quit (Drive now can take a while): the terminal is restored first, then the
  process waits for it. An action is never cut in half.
- The app copies six client rules from `web/src/blocks.ts` and `web/src/main.ts`: the ticking formats, the chips'
  next selection, the action bodies above, `more` paging, the `loading` re-read and the flattening of menus. Each
  has a test.
- About +1,800 lines of `src/`. The phone's themes and sprites have no terminal twin.

## `.drover/arch.toml`

No hot file is converted: the board adds files, and `lay` is split by block family so no one file collects every
change. The impl lane adds these lines in slice 2, so the boundaries are checked and not only written:

```toml
[[ban]]
pattern = "ratatui"
paths = ["crates/*/src/**"]
except = ["crates/chat/src/**"]
reason = "#1354: only the terminal app draws; the screens stay data"

[[ban]]
pattern = "/screens/"
paths = ["crates/chat/src/**"]
reason = "#1354: the terminal names no screen: paths and places come from serve::Local"

[[ban]]
pattern = "TcpStream"
paths = ["crates/chat/src/**"]
reason = "#1354: the terminal calls the screens in-process (serve::Local), never over HTTP"

[[ban]]
pattern = "Server"
paths = ["crates/chat/src/**"]
reason = "#1354: the terminal uses serve::Local, the screens' in-process door, never the HTTP server"
```
