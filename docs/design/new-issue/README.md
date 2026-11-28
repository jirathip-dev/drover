## Summary

Decision: [New issue] is one card at the end of the cards place, so it sits above every tab on the page and shows in cooee's cards place as well. Folded, it is one line. Opened, it has a repo picker, a line of text and [+] to attach a screenshot. The scope lane's questions and its draft come up as ordinary ask cards. The draft card has approve (files and queues it), revise (a typed note) and drop.
Screens: the open [New issue] card with a line and a screenshot; a scope question with a second line for the same repo waiting; the draft to approve.
Open questions: (1) [drop] on a scoping row stops its lane mid-way. Keep it, or allow dropping only at the draft? (2) The terminal board draws the same blocks; it is not drawn here (HTML only). Is that enough for the AC "draws on the board"?

## What each image shows

The PNGs are the real page: this branch's `web/dist/board.js` and `board.css`, drawing the demo export's tabs (`screens/`) and one fixture of the cards place per state (`screens/cards-<state>.json`). `drive.js` loads a state and taps through to it with `index.html?view=new|scoping|draft`. To shoot again, run `sh docs/design/new-issue/shoot.sh`. 390 shots use DPR 2 at 844 px tall (1300 for scoping, 1600 for draft, so the whole card shows); 1440 shots are 1440 × 1000.

| image | decision it shows |
|---|---|
| `new-390.png`, `new-1440.png` | **[New issue] is a card in the cards place**, after the asks and needs-human cards, so it never pushes them down. Folded, it reads `+ new issue · a line or a screenshot, for any repo`. Opened, it shows one plain line on what happens next, then the page's existing **composer**: `[+]` attaches (here `orders.png`, shown as a thumbnail with `[×]`), the line goes in the text field, `[scope]` sends, and `[repo acme-web ▾]` is the composer's own picker chip, listing the fleet's repos. |
| `scoping-390.png`, `scoping-1440.png` | **The questions are ordinary ask cards.** Each card is titled `acme-web · new issue`, with the meta line `scope · question 2`. It gives the question in bold, one short paragraph of what the lane found in the code (with a file:line), and the answers given so far. The choices put the recommended option first, then `other…`, then `[later]`. Folded, the card's main button is the recommendation, as for any ask. **One scope lane per repo:** the [New issue] card's fold reads `acme-web: scoping, 1 waits`, and opened it lists the running scope (`scoping`, answered/open count, age) and the second line (`waits`, `starts once the one above is filed or dropped`). Each row has a `[drop]` button (shown on swipe or tap) behind a confirm. |
| `draft-390.png`, `draft-1440.png` | **The draft is an ask card** titled `acme-web · new issue draft`. It shows the issue as it will be filed: the title, then **Problem / Wanted / AC / Notes**. Notes says how it will be queued (`--repro` for a bug; `--design` / `--architect` when the draft says so). The attached screenshot is shown under the text and goes into the issue. The choices are `[approve: file and queue --repro · recommended]` and `[drop]` (with a confirm: `Nothing is filed. The scope lane exits.`). The free-text field is **revise**: its placeholder reads `revise: what should change?`, and what you type goes to the lane as the note. |

## The blocks (for impl)

Everything here is a block the page already draws (`card` with `fold`, `text`, `list`/`row` with `badge`, `subtitle` and `swipe`, `composer` with `chips` and attachments, `choice` with `recommended`, `other` and per-option `confirm`, and `images`). No renderer or CSS change is needed.

- **The card:** `id: "new-issue"`, `kind: "new-issue"`, the last card in `/screens/cards`. It is always present, folded unless opened. The fold line summarises scope lanes per repo (`acme-web: scoping, 1 waits`, `draft ready`).
- **Composer:** `action: "new-issue"`, `label: "scope"`, chip `repo` (the fleets' repos, default the last one used), and `values.req` so a double send files one scope. The sent values are `text`, `repo`, and `name`/`data` for attachments, exactly what the thread composer posts. The screenshot reaches the scope lane's brief the same way a thread attachment reaches an orchestrator (AC 3).
- **Waiting:** a second [New issue] for a repo whose scope lane is live is accepted and listed as `waits`. It starts when the first one is filed or dropped. Nothing is refused, so the line is never lost.
- **Asks:** the scope lane files its questions and the draft through `drover ask` like any lane. They come up in the cards place as asks with `scope · …` fold lines. Approve files exactly the draft's body and queues it. Drop files nothing. Revise sends the note to the lane, which posts a new draft ask.
- **Not here:** the lane's brief and hook (read-only, no spawn or merge) are impl's. They have no screen.

## Files

`index.html` is the export's page shell pointed at `../../../web/dist/`. `screens/` holds the demo export's tab screens plus the three cards fixtures, `themes/farm/` is the theme the page loads, `orders-shot.png` is the illustrative screenshot, and `drive.js` and `shoot.sh` drive and shoot the states.
