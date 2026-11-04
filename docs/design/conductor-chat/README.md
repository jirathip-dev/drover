# Conductor chat: the [chat] tab, design mockup (#355, step 0)

`index.html` is the board with a fifth tab, `[chat]`: repos with their conductor threads, one thread open with choice
rows, image bubbles, a file, a link, a config-propose card and a secret-request card (Amendment A). It links
`assets/serve.css` unchanged and inlines the farm scheme's tokens as `serve.rs` writes them. One `<style>` block holds
the new rules, which use existing tokens only and move to `serve.css` when the tab is built (step 1). Static: the
hash picks the view.

| shot | view | what it shows |
|---|---|---|
| `chat-list-390.png` | (none) | the repo cards with their threads |
| `chat-new-390.png` | `#new` | `[+ thread]` opened in acme-web: a title and `[start]` |
| `chat-thread-390.png` | `#t` | one thread from the first message to the open choice and the composer |
| `chat-guest-390.png` | `#guest` | the same thread on a device that is not a paired owner device |
| `chat-states-390.png` | `#states` | every card's other states, captioned |
| `chat-1440.png` | `#t` at 1440 | the list and the thread side by side |

`node docs/design/conductor-chat/shoot.mjs <chrome-headless-shell>` (repo root, no deps) shoots them at full height
(390 px at DPR 3, guest at 2, 1440 at 1) and measures each view against docs/ui.md's gate 2. With `PEEK=<dir>` it also
writes 844 px slices.

**Data:** illustrative, in the demo's fleets (`acme-api`, `acme-web`, `docs`), so nothing private is shown. The story
is the AC's live readback: a grill (3 choices, one answered from Telegram), a filed issue (#52) queued after #21, a
mockup sent back, then a config proposal and a secret request. `IMG_2041.PNG` is `../farm-v2/board-events-390.png`
(a real board screenshot). `rate-limits-docs-390.png` is the conductor's "mockup", made from
`fixtures/rate-limits-docs.html`. The account names, times, ids (`q31`, `c8`) and the working directory
`~/conductor/acme-api` are made up.

## The list

```
┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓  ← fleet card (#162): the repo's colour bar; "all repos" uses the text colour
┃ acme-api 3 threads          [+ thread] ┃
┃ ● Rate limits for /v1/search [tg]   2m ┃  ← unread dot (link blue), title (ellipsised), Telegram-bridged, age
┃   ! choose conductor: Return 429 wi…   ┃  ← 11 px: the state if not plain awake, then the last message
┃ ● Idempotency keys: retry window   40m ┃
┃   ✻ working conductor: reading #17's…  ┃
┃ ▸ 4 older threads                      ┃  ← the 3 newest show; the rest fold
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
```

- **Order:** "all repos" first, then repos by name. Inside a card, threads are newest first.
- **States:** `! choose` (red) means an open choice or owner card waits for you, `✻ working` (amber) means the
  conductor is mid-turn, and `☾ asleep` (grey) means its lane was closed after 30 min idle. Awake and idle shows nothing.
  A read thread's title is muted.
- **New thread:** `[+ thread]` opens a title input and `[start]` in that card. Starting spawns the conductor lane for
  that repo (or for every repo, from "all repos").
- **The tab:** `[chat·N]`, where N (red, as on `[queue·N]`) is the number of threads where `! choose` is set. Unread
  replies don't count: they are only the blue dots.
- **Needs-you stack:** on the list it shows folded (#154's `n need you ▸`). Inside a thread at 390 it is hidden, so the
  conversation keeps the screen.

## A thread

- **Thread bar** (sticky under the header): `[‹]` back to the list (phone only), the title, and one 11 px line:
  repo · harness · model · state (green dot when awake) · `tg topic` when bridged.
- **Blocks are #138's chat blocks:** `you` in the accent colour, `conductor` purple, and its commands as amber tool runs
  that start closed (`$ gh issue create …`, `$ herdsman queue acme-api add 52 --after 21`, `Read uploads/IMG_2041.PNG`).
  Routine verbs are visible this way without being loud. A block's time sits on the right, and a message that came
  in from Telegram says `via telegram`.
- **Images:** shown at the bubble's width and at most 320 px tall. A tall screenshot is cut at the bottom, and a tap
  opens it whole (not mocked). The name and size are under the image. The owner's attachment and the conductor's
  mockup PNG look the same.
  **Files** are one 44 px row (`▤ 429-body.json  412 B`). **Links** are plain links.
- **Choices** (`herdsman ask --thread`): the question in the conductor's block with an 11 px `q33 · 3 of 3`, then
  #63 A's choice rows, recommended first, and an `other: type an answer` box with `[send]`. A tap answers straight
  into the thread. Once answered, only a green `✓ 1. per API key · you · 09:14` line stays (also `✓ other: "…"`, and a
  routine ask's `by: default after 30 min · [change]`).
- **Owner cards are amber**, meaning "waits for the owner" inside a thread. Red stays the board's needs-you stack.
  - **Config proposal:** a title, `repo · fleet.toml · N changes`, the diff in the tool-output panel (#138's
    `-`/`+` tints, `@@ [table]` muted), `✓ validates · on apply: backup, herdsman doctor, a config-applied event`,
    `[apply] [discard]`, and who may act. Once settled it folds to one line: `✓ config c8 applied by owner · 09:40 ·
    backup … · doctor ✓` (green edge, the diff behind ▸) or `✕ discarded` (grey). A doctor failure after apply rolls
    back and says so in red on the card. A conductor that runs the apply itself gets a status line `→ REFUSED
    OWNER_ONLY`.
  - **Secret request:** `🔒 secret request RESEND_API_KEY · acme-api`, the why, a masked input, `[save] [dismiss]`,
    and where the value goes. Once settled, a status line `🔒 secret RESEND_API_KEY set ✓ (by owner)`. The value
    never reaches the page again.
  - **Login:** `account C signed out` with `[open setup terminal]` (#303) and `[later]`.
- **System lines** are #138's quiet status lines: thread started, `☾ asleep after 30 min idle`, `↻ resumed · same
  session (7f3c…) · 46 messages`, and `⇄ switched to account B (A resting until 18:40) · same session` (amber edge). A
  harness that could not resume says it started from a handover summary.
- **Live line and composer** (sticky at the bottom): `● waiting for your answer · sleeps after 30 min idle` (#105's
  line), then `[+]` attach (camera roll or files; paste or drop at 1440), the text box, `[send]`. A picked image waits
  above the box as a 56 px thumbnail with its name and `[×]` (in `#states`).
- **Not a paired owner device** (`#guest`): the thread reads the same, but the choices and composer are inert, `[apply]`
  is disabled next to `[pair this phone]`, and the secret card has no input, only "only a paired owner device can set a
  secret".

## 1440

The list is a 400 px sticky column on the left. The open thread sits beside it, its blocks capped at 100ch (the
chat-blocks README's Q6), and the selected thread row is tinted with the repo's colour bar on its left. `[‹]` is
hidden. The needs-you line stays above both.

## The rules, measured on the mockup

`shoot.mjs` passes all six views: no sideways scroll and nothing past the right edge, text only 11 or 13 px (inputs
16), every bracket control 28 px tall with a ≥ 44 px tap target, choice rows ≤ 4 lines, thread rows, file rows,
settled cards and older-threads rows ≥ 44 px, and every cut line ellipsised. Five tabs fit at 390 px. Step 1 should add
`chat`, `chat-thread` and `chat-states` views to `ui-audit.js`.

## Open questions for the owner

1. **Tab place.** `[chat]` is last (after `[events]`), so the tabs you use today keep their places. First would say
   "this is the main way in". **Recommend: last.**
2. **Repo cards vs repo chips.** Per the issue, the mockup stacks every repo's card, each with its 3 newest threads.
   #367's chip row (one repo at a time, swipe between them) scales better past about 6 repos. **Recommend: cards now;
   switch to chips if the list gets long.**
3. **Owner cards amber, not red.** A pending config or secret card is "waits for you", which the board shows in red.
   Red cards inside a conversation read as errors, so the mockup uses amber in the thread. The thread row's
   `! choose`, the `[chat·N]` count and the push stay red. **Recommend: as mocked.** Related: should pending owner
   cards also join the top needs-you stack? **Recommend: no.** One place per decision: lane asks stay in the stack,
   and a conductor's asks and cards stay in its thread.
4. **Answered choices fold to the answer.** The alternative keeps every row with the picked one marked, which costs
   about 3 lines per past question. **Recommend: fold.**
5. **The conductor's commands are visible** as closed tool runs. Hiding them would make the thread read like a plain
   chat, but you would lose the record of what it ran (issues filed, queue moves, spawns). **Recommend: show, closed.**
6. **Where a conductor lives.** It needs a working directory without a worktree. The mockup says `~/conductor/<repo>`
   (and `~/conductor/all`). **Pick the path**, or name it in `[conductor]`.
7. **Push text.** Proposed: `acme-api · Rate limits for /v1/search: <first line of the reply>`, and for a choice
   `… asks: <question>`. A tap opens the thread. No push for tool runs or system lines.
8. **Not in the issue, left out:** renaming or archiving a thread, search across threads, and the full-screen image
   viewer's look. Say if any belongs in step 1 or 2.
9. **The Telegram side** (step 3) has no board UI beyond the `tg` marks. The issue's mapping (topic ↔ thread, choice
   rows → inline keyboard, images both ways) needs no mockup. Starting a thread from Telegram (a new forum topic)
   would create it under "all repos" unless the topic name starts with a repo name. **Confirm that rule.**
