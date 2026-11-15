## Summary

Decision: Threads becomes a fourth tab, `[queue] [events] [lanes] [threads·N]`, in the standalone page only, drawn from a JSON screen like the other tabs. One list shows every repo's threads, "all repos" first, and tapping a thread opens it in the same tab: your messages, the orchestrator's replies, its commands as closed one-line rows, asks as choice rows, and a send box. N counts the threads waiting on your choice.
Screens: the thread list, an open thread, and a new thread (plus each one's other states).
Open questions: (1) At 1440 the old [chat] tab showed the list and the thread side by side. Here the thread replaces the list, centred at 760 px, as every other tab works. Recommended: keep it that way for v1. (2) The tab goes last, so the plugin's three tabs keep their order. The old tab sat in the middle. Recommended: last.

## How it is drawn

All shots come from the **real screens page**. `shoot.mjs` exports the demo, serves `fixtures/*.json` as
`/screens/threads…`, adds `threads.css` and `preview.js` to the page shell, and shoots each view at full height (390 at
DPR 3, 1440 at DPR 1). The fixtures are the JSON the server would send, so they are the spec for `screens.rs`. The
data is made up, in the demo's fleets.

`node docs/design/threads-tab/shoot.mjs <chrome-headless-shell>` (repo root; it needs `drover` on PATH)

The design sticks to the block set the page already draws, plus three additions, each of them generic:

| addition | where | what it does |
|---|---|---|
| a Threads place, standalone only | page shell (`PL`), not `cooee-plugin.toml` | the tab; the Cooee plugin never shows it |
| `form.submit` `{label, action, values}` | `blocks.ts` form | draws `[send]` / `[start]`, which posts the form's fields (the composer) |
| `screen.focus: true` | `main.ts` | hides the cards place while a thread is open, so the conversation keeps the screen (as the old tab did at 390) |

`preview.js` fakes these three in the mockup only. `threads.css` holds the new rules (existing tokens only). They move
to `web/src/board.css`.

## The blocks

- **List** (`/screens/threads`, `badge` = the number of threads waiting on your choice): one `section` per repo,
  "all repos" first and then the rest by name. Each holds a `list` of thread rows, its older threads folded in a `card`
  (`▸ N older threads`), and `[+ thread]`. A row is:
  `figure` (the orchestrator: `needs` = waiting on your choice, `working`, `idle` = awake or asleep) · title ·
  `badge` "new" if unread · `subtitle` (`! choose` in amber, `✻ working`, `☾ asleep`, then the last message) · `value` the age ·
  `tap: {open: /screens/threads/<id>}` · `swipe: [archive]`. Archived threads fold under `▸ N archived` at the foot.
  Their rows open the thread (read only) and swipe to `[restore]`.
- **Thread** (`/screens/threads/<id>`, `focus`, its own `stream`): the page's `‹ back`, then the title with `[archive]`
  (or `[restore]`) as the screen's action. Next comes one row: repo · harness · model, the state, and awake/asleep. Then:
  - a message is a `card`, `kind: you | orch`, `title` the author, `meta` the time (`✓ 09:40` once logged) and a
    `text` block. `kind` sets the left edge: amber for you, purple for the orchestrator.
  - a command is a `card` `kind: tool` that has only a `fold` line (`$ drover queue acme-api add 52 --after 21`). It
    starts closed, and a tap shows its output.
  - a system line (resumed, asleep, account switch) is a `card` `kind: sys` with muted text.
  - an ask is a `card` `kind: ask` (amber `tone: warn` while open) with the question and a `choice` block (at most 4
    options, recommended first, plus an `other` box) posting `answer`. Once answered it keeps a `✓ choice · who · when` line.
  - the composer is a `form` (one text field) whose `submit` posts `thread-say`.
- **New thread** (`/screens/threads/new/<repo>`): the repo row, one line explaining what happens, and the composer with
  `[start]`. It posts `thread-new`, and the reply's `screen` op swaps in the new thread (the page already handles that). Nothing
  is logged before the first message (#405).
- **Sending:** while the post is in flight, `[send]` shows the page's busy spinner and the message reads `sending…`. The
  reply appends your message with `✓ time`. A delivery that fails later comes in on the stream as your message turned red
  (`tone: bad`, the reason, `[retry]` → `thread-retry`).

Actions reuse the existing `POST /thread` ops (`new`, `say`, `answer`, `read`, `retry`) and `thread archive|restore`,
behind `/action` names `thread-new`, `thread-say`, `answer`, `thread-retry`, `thread-archive` and `thread-restore`.
Opening a thread marks it read, as before. Out of scope for v1, as the issue says: attachments, queued messages, and
config and secret cards.

## Images

| image | decision it shows |
|---|---|
| `list-390.png` | the 4th tab `[threads·1]` (standalone only), sections per repo with "all repos" first, rows with the orchestrator's sprite, `new`, `! choose`, `✻ working`, `☾ asleep`, last message and age, `[+ thread]` per repo, older threads and archived ones folded |
| `list-open-390.png` | both folds open (older threads, archived) and a row swiped to `[archive]` |
| `thread-390.png` | an open thread: the cards place hidden, `‹ back`, title and `[archive]`, the repo · harness · model row, you/orchestrator messages, closed command rows, two answered asks, a system line, and the open ask with 3 choices (recommended first), `other`, and the composer |
| `thread-states-390.png` | a working thread: `✓` sent, a routine ask answered by default, a failed delivery in red with `[retry]`, and a message `sending…` |
| `new-390.png` | `[+ thread]` in acme-web: the repo row, the hint, the composer with `[start]` |
| `list-1440.png` | the list at desktop width, same blocks |
| `thread-1440.png` | the thread at desktop width, centred at 760 px instead of the old side-by-side (open question 1) |
