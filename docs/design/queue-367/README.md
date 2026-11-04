# Queue tab: one repo at a time — design (#367, phase 1)

This mockup is drawn by the real board, not by hand. `fixture.py` writes a three-repo fixture: herdsman with 56 open
issues, synergy with 14 and berth with 1. It includes the 6-step `Decouple` chain, a branching 4-step chain, two asks,
two round limits and more than 40 queued rows. The herdsman titles echo that repo's open issues; everything else is
synthetic (`acme-example/*`). `serve --export --demo` renders the fixture, and `shoot.mjs` serves two copies of it:

- **today**: the export unchanged;
- **next**: the export with `queue.js` spliced into `serve.js` and `queue.css` appended to `serve.css`.

The prototype changes **assets only**. One fact comes from the fixture: each row's seconds in its phase (`data-a`).
That is phase 2's one server change (see *Phase 2*).

| shot | what it shows |
|---|---|
| `before-after-390.png` | the first screen, today vs proposed, same fixture, 390 × 844 |
| `queue-390.png`, `queue-full-390.png` | herdsman picked: the first screen, and the whole repo collapsed (1,301 px, was 5,040) |
| `chain-open-390.png` | both chains open: steps at one level, the side branch (#356) at two, titles in one column |
| `more-open-390.png` | `+ 32 more` opened in place; `bottom-390.png` is its end above the tab bar |
| `all-390.png`, `all-open-390.png` | the `all` chip: every repo's needs-you rows; one open on its ask card |
| `synergy-390.png` | after a tap on `[synergy 14]` |
| `queue-1440.png` / `today-1440.png` | 1440: one 960 px column, `[‹]` `[›]`, the Decouple chain open |
| `queue-paper-390.png` | the light scheme |
| `today-390.png` | today's first screen |

To regenerate the shots (from the repo root, with a release build):

```
python3 docs/design/queue-367/fixture.py /tmp/q367/fx
HERDSMAN_HOME=/tmp/q367/fx FLEET_CONFIG=/tmp/q367/fx/fleet.toml FLEET_STATE=/tmp/q367/fx/events.jsonl \
  ./target/release/herdsman serve --export /tmp/q367/site --demo
node docs/design/queue-367/shoot.mjs /tmp/q367/site /tmp/q367/fx <chrome-headless-shell>
node docs/design/queue-367/queue.test.js
```

## The tab

```
herdsman                         demo ● [?] [↻] [🐎] [◐]
[all 4•] [herdsman 56•] [synergy 14•] [berth 1]          ← sticky chips; • = something needs you
herdsman  ! 2 need you   [drive now] [pause] [config]
● drive · no pass yet                                  ▸
NEEDS YOU 2
     🐴 #331  ⏸ waiting: decision 18m
           asks: Run one drive pass per fleet in par…   ← the row opens on its ask card
IN PROGRESS 8
     🐴 #360  ready 1m
     🐴 #265  → #270 · ready 35m                     ▸   ← a chain: one row
           Decouple (6 steps) · next: #266
     🐴 #340  in impl 1h 47m
QUEUED 37
   1 🐴 #344  → #358                          [▼]   ▸
           Phase B: stat… (4 steps) · next: #344
   2 🐴 #371                             [▲]  [▼]
   …5
           + 32 more ▾                                  ← opens in place
BACKLOG  open issues not queued (#365)                ▸
▸ done today: 2
```

- **Chips (decision 1).** `[all N]` comes first, then each repo as `[name count]` in its colour. The count is the
  repo's open rows: needs you, in progress and queued (not the backlog). A red dot marks a repo with something for you.
  The chips stick under the header. They switch repos the ways #291's Lanes scene does:
  - a tap on a chip;
  - a touch swipe, at least 40 px and twice as far sideways as down, anywhere on the tab (a short repo leaves most of
    the screen empty);
  - a mouse drag;
  - a sideways wheel or trackpad swipe;
  - ←/→;
  - `[‹]` `[›]`, shown at 720 px and wider (a phone swipes).

  The pick is kept per phone (`localStorage` `queue:repo`). The first visit picks the first repo that needs you.
- **`all`** shows only the rows that need you, from every repo, each title prefixed with its repo's name.
- **Groups (decision 2):** NEEDS YOU (needs-human, a waiting decision, a dead lane), IN PROGRESS (impl, review, fix,
  ready), QUEUED, BACKLOG. Needs-you and in-progress rows keep the server's order (ready first, then working, each in
  queue order). Queued rows are in queue order.
  - QUEUED shows its first 5 slots, then `+ N more`, which opens the rest in place. Open, that line reads
    `show the first 5 ▴`.
  - Each queued slot shows its position (1, 2, …) in the rail, plus ▲/▼ on the row (no longer inside its card).
  - The `queued` state word is not repeated inside the QUEUED group.
  - A group's count is its rows, so a chain counts as one.
- **Chains (decision 3).** A chain is every row linked by `blocked by` to another row of the same repo. It shows as one
  row in the group of its most urgent step, at that step's place:
  - line 1: `#root → #end`, then the most urgent step's state and time;
  - line 2: `name (N steps) · next: #n`.

  The name is the series the titles share (`Decouple 3/7: …` → `Decouple`), else the first step's title. When space
  is short, the name gives way and `(6 steps) · next: #266` always shows. `next` is the first step still queued.
  Opened, the chain lists its steps in place. The main line (the longest path) sits at level 1. A side branch sits at
  level 2 under its step, and anything deeper stays at 2 with `after #b`. So a 6-step line is 6 rows at one level,
  which fixes the staircase. Steps have no ▲/▼, because the blocking sets their order. A blocker loop draws no chain.
- **One grid for every row (decision 4):**
  - a 4ch rail (position, or the chain elbow);
  - the 28 px dot or sprite;
  - `#n`;
  - the state with its time;
  - the title line, which starts at the `#n` column at every depth (measured: one column, x = 88 at 390, x = 316 at
    1440).
- **Time:** one unit style, at most two units, and never a second unit of 0: `45s`, `47m`, `1h 47m`, `2h`, `3d 4h`.
  The time names the phase it measures: `in impl 1h 47m`, `in review(1) 4m`, `ready 35m`, `needs-human 59m`.
  Queued rows have no time.
- **The needs-you cards** of rows on the Queue tab open inside those rows. The stack above the tabs keeps only cards
  with no row (it is unchanged on the other tabs). Without this, two asks push every running row below the fold
  (`before-after-390.png`, left).
- **Bottom:** on the Queue tab, the page pads the tab bar's height as drawn (a `ResizeObserver` writes `--navh`) plus
  16 px.
- **1440:** one repo in one 960 px column.

## Gate 2, measured (`report.json`)

| AC | result |
|---|---|
| running rows above the fold | all 8 herdsman in-progress rows end above the tab bar at 390 × 844 (today the first starts at y 1322 on a 5,040 px page). Opening a chain pushes later rows down, as it should. |
| chain collapsed and expanded | collapsed: `#265 → #270 · Decouple (6 steps) · next: #266`; opened by a tap; `chain-open-390.png` |
| `+N more` in place | `+ 32 more` opens at the same y (`inPlace: true`) |
| chips switch repos | tap → synergy · swipe left → berth · swipe right → synergy · → → berth · ← → synergy · tap `all` → all · reload keeps `all` · 1440: `[›]`, mouse drag, sideways wheel |
| last row above the tab bar | 33 px clear at 390 (today 31 px in headless Chrome: see Q3) |
| contrast ≥ 4.5:1 | the title line, in #244's eight schemes: github-dark 6.15 · solarized-dark 5.34 · gruvbox 5.63 · nord 5.78 · dracula 6.75 · paper 5.89 · farm 7.12 · kitchen 6.44 (today's are the same: see Q2) |
| no sideways scroll | none at 390 or 1440, collapsed, chain open, `+N more` open or `all`; nothing past the right edge |
| board audit (`ui-audit.js`) | no outliers at 390 or 1440, with two of #331's rules adjusted (see *Phase 2*) |
| tests | `queue.test.js`: 19 pass. They cover the time format, group order, chain summary text, steps at one level, the side branch at level 2, and a loop drawing no chain. |

## Phase 2 (build)

- Move `queue.js` into `serve.js`:
  - `qprep()` runs at the top of `tidy()`, before `graph()`;
  - `qtab()` and `qasks()` run at its end;
  - the pure part, `// #367 queue pure` … `// queue:end`, gets tested from `crates/server/tests` the way
    `chat_blocks.rs` runs the chat part.
- Append `queue.css` to `serve.css`.
- `graph()` can lose its staircase code (depth, clamp, elbows), since the chain rail replaces it. That is a
  net-negative `serve.js` change.
- **`src/`, about +5 lines** (the phase time needs it; the chain summaries don't):
  - `IssueState.since`: set in `fold()` when `state` changes;
  - `data-a=<now − since>` on the row in `queue_tab`.

  Today's age is the last event's (`last_ts`), so a kick or a ping would reset `in impl`.
- `ui-audit.js`:
  - #331's one-line-title rule checks only rows that are visible (a closed chain, `+ N more` and the other repos hide
    rows by design: 64 here);
  - "no clamped (4-deep) chain row" becomes "every row's title starts at one x".
- A `MutationObserver` on `#needs` re-runs `qasks()` when the needs stack refreshes alone, as `herd.js` does for its
  pages.

## Open questions for the owner

1. **Ask cards inside their rows on the Queue tab.** The issue didn't ask for this, but without it two asks fill the
   first screen. Cards with no row (a fleet-level ask) stay in the stack, and the other tabs don't change.
   **Recommend: yes.**
2. **Contrast.** `--mute` already clears 4.5:1 for the title line in every built-in scheme (5.34–7.12 on the
   background, 4.63–6.93 on a card). I tried a brighter mix (`--fg` at 72 %), but it *dropped* solarized-dark to
   3.59, so the mockup keeps `--mute`. Which scheme, or which user scheme file, was the recording in? A user scheme
   outside the gate would explain it. **Recommend:** keep `--mute`, and make phase 2 check `.tl` explicitly in
   #244's gate.
3. **The hidden last row.** I can't reproduce it: headless Chrome has no safe area, and today's padding (72 px plus
   the inset) is already 19 px more than the bar (53 px plus the inset). The fix pads by the bar's measured height, so
   it holds whatever the cause. Which device was it, in which mode (home-screen app or Safari), and at which text
   size? That would tell us whether something else covers the row.
4. **The real chain is 9 steps.** On today's queue #265 → … → #270 → #344 → #345 → #355 is one blocked line, so it
   would show as one row: `#265 → #355 · Decouple 2/7: JSON API… (9 steps) · next: #266`. The titles don't share one
   series, so the name falls back to the first title. The alternative is to split a chain where the series changes
   (`Decouple (6 steps)`, then `after #270 · Phase B/C … (3 steps)`). **Recommend: one row per blocked line.** It is
   simpler, and the steps are one tap away.
5. **Chip order.** The server sorts the fleet cards with "needs you" first, so the chips (and the swipe order) would
   reorder as asks come and go. **Recommend:** fleet.toml order. The server sort is about 1 line removed in phase 2;
   the red dot already marks repos that need you.
6. **Positions and ▲/▼.** The mockup shows positions as plain `1`, `2` … (not `#1`, so they can't be read as issue
   numbers). It puts ▲/▼ on every queued slot, including the ones `+ N more` opens, not only the first 5. Is that OK?
7. **Backlog** is a collapsed stub until #365 lands.
