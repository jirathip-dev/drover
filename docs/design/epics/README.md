## Summary

Decision: an open epic is a new first section, "epics", in its repo's card on the Queue. It shows the parent with `k/n merged` and a ▰▱ bar, and under it each child's own row: ✓ merged, its live stage, or "dropped". It uses only the block kinds the screens already have, so the phone page and the terminal Board draw it with no new drawing code. When the last child lands, the repo's thread gets one `merged` row and a push ("acme-api #24 landed"), and the parent is closed with one comment.
Screens: the Queue with an open epic (page and terminal Board), and the repo thread with the landed row and its push. Each is drawn at 390 and 1440 wide (the terminal at 40x18 and 120x40).
Open questions: (1) Should a dropped child leave the count (2/4, with "1 dropped" beside it), as drawn, or count as done (3/5)? (2) Should a closed epic stay on the "done today" list? I left it off.

## How it is drawn

`epic.js` builds the epic blocks: a `section` holding a `list` of `row`s, with `badge`, `figure`, `indent`, a `link` row
for the fold, and a `sys` `card` holding `text` for the landed row. It patches them into the demo export's own screen
JSON (`drover serve --export <dir> --demo`). `shoot.mjs` serves that export on 127.0.0.1 and shoots the real
`board.js` (farm theme, pixel style). `term.html` draws the same patched Queue through `../terminal-board/screens.js`.
To redo every PNG, run `node docs/design/epics/shoot.mjs <chrome-headless-shell>` from the repo root. It exits 1 on a
sideways scroll or a grid layout problem. Ages come from the demo's fixed clock, so the page shows them as days.

## Images

| PNG | shows | decision |
|---|---|---|
| `queue-390.png`, `queue-1440.png` | The Queue page, scrolled to acme-api: epic #24 open, its five children listed. | `epics (N)` is the **first** section of the repo card, above in progress. The head row is `#24 Payments API v2`, with a `2/4 merged` badge (blue, the `done` tone), the epic's age, and the subtitle `▰▰▱▱ · 2 in progress · 1 dropped`. A tap opens the parent's issue sheet. The children are indented rows, in issue order. A merged child has the ✓ figure, `merged · PR #26` and its merge age. A live child has the same badge and subtitle as its in-progress row. A dropped child has a grey `dropped` badge and `closed unmerged · not counted`. `▾ fold #24's children` folds the epic to its head row, the same link and `fold` query as a queue chain. An epic starts open; once folded, it is kept folded the way a chain is. acme-web's epic is drawn folded (`▸ 3 children of #40`, `1/3 merged · ▰▱▱ · 2 queued`). |
| `term-40x18.png`, `term-120x40.png` | The terminal Board's Queue, with the cursor on the epic | The same JSON, drawn by the Board's existing rules: the head's badge sits on the right in blue, the children are indented with ✓ / ● / ○, and Enter opens the parent's sheet. A merged child sends figure state `done` rather than a new `merged` state, so both renderers draw ✓ with no change. |
| `landed-390.png`, `landed-1440.png` | acme-api's thread after #17, the last child, merges | The epic's one `merged` row (`kind: merged`, `issue` = the parent, `key` = `merged-epic-<fleet>-<n>`, so it is posted once) reads `acme-api #24 Payments API v2 landed: 4 issues`. Below that it lists each child with its PR, then `dropped: #13`, then the closing note. Its push (drawn on top as a banner) has the title `acme-api #24 landed` and the body `Payments API v2: 4 issues merged, 1 dropped`. That push replaces the last child's own merged push, so the owner gets one push, not two. The epic then leaves the Queue. |

## Not drawn, decided

- **`drover waiting`** prints each open epic as one line above its issues, with the same words as the head row:
  `epic acme-api #24 Payments API v2 · 2/4 merged · 2 in progress · 1 dropped · children #11 #12 #15 #17 (#13 dropped)`.
  It has no next command: an epic waits only on its children.
- **The compact `drover board | cat`** adds the same line under the repo's header.
- **The closing comment on the parent** (drover's one comment) reads: `Landed: 4 of 4 issues merged.`, one line per
  child (`- #11 Refunds endpoint: PR #26`), then `Dropped (closed unmerged): #13`. A dropped child never blocks the close.
  If every child is dropped, nothing is closed. The epic shows `0/0 merged · 3 dropped` until the owner closes it.
- **Contract keys:** the landed row is a `row-merged` shape, so `row-merged.keys` needs no new key. The Queue's epic
  section is screens JSON (section, list, row), whose kinds are already in the screens contract. The AC's ".keys"
  entry for it is `row-epic.keys`, a golden of the head row's fields: `id`, `title`, `badge.text`, `badge.tone`,
  `subtitle`, `tap.sheet`, `value.since`.
- **The in-progress rows** of epic children stay where they are, so a child shows both in its epic and in progress.
  The queue's order and its moves are untouched.
