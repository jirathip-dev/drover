## Summary

Decision: the Board tab draws the phone's own screens as text. A line under the tabs picks Queue, Events or Lanes. Asks and cards sit above the Queue at 40 columns and in a right-hand column at 120. A sheet takes the whole screen at 40 and the right column at 120. The cursor's actions are numbered 1–9 in a bar at the bottom. A number opens a panel showing exactly what is sent or run, and Enter confirms it.
Screens: board, queue reorder, issue sheet, ask, run card approve, run card refused, backlog, hosts, lanes, events, the tab switch, what is down. Each is drawn at 40x18 and 120x40 (24 images). The issue lists 11; I added the refusal.
Open questions: (1) Should `m` (menu: Hosts, Fleets, Workflow) stay a key of its own (my pick), or go into the Queue's More buttons? (2) Should the panel show the raw action and values, `sends move to queue · before 45` (my pick: it is exactly what is sent), or only the button's words? (3) Should a long badge such as `▲ [◆ rev · r1] ◆ ◫` move to its own line at 40 columns (drawn so), or move under the title?

## How it is drawn

`screens.js` is a prototype of the ADR's `lay()`. It reads the phone's screen JSON by field name, and the frame
around it (top line, tabs, hint row, two-press panel) is #1352's. The JSON in `fixtures.js` comes from the demo
export (`drover serve --export <dir> --demo`). `fixtures.mjs` regenerates it and adds the trees the demo has no state
for: an ask card, a needs-human card, a run card, an issue sheet, backlog and two more hosts. Each is built in the
shape `crates/server/src/screens.rs` returns. `lay` names no screen and no action. Every text wraps and none is cut
(the `…` you see is in the server's own strings). A row past the grid stops the render.

`node docs/design/terminal-board/shoot.mjs <chrome-headless-shell>` (from the repo root) writes every PNG at DPR 2 and
exits 1 on any layout problem. A 40-column grid is 390 px wide and a 120-column grid is 1440 px wide, as in
`../drover-chat`. In these PNGs the browser leaves out the `│` side lines of a box. The built app draws them
with ratatui's borders.

## Images

| PNG (40x18 · 120x40) | shows | decision |
|---|---|---|
| `board-*` | the Queue: the screen's buttons, repo chips, stats, fleet cards with their rows; asks and cards | Line 2 is the place strip: `Queue · Events · Lanes`, with the current one underlined and `m menu` on the right. A row is: `›` cursor · figure glyph (`●` working, `!` needs you, `✗` failed, `✓` done, `○` idle) · title, with badge and age on the right in the badge's tone. A badge up to half the width sits beside the title, which wraps next to it; a longer one gets its own line. The subtitle sits under the title, grey. A fleet card has a left edge in its tone. Its Drive now / Pause are `[buttons]`. At 40 the folded cards (`▸ #21 · routine · …`) come first in the scroll; at 120 they get the right column, `asks and cards`. `▲ N lines above` and `▼ N more below` mark the scroll. The Board tab carries the screen's badge (`Board 5`). |
| `reorder-*` | cursor on queued #46, `1` (▲) pressed | A row's swipe actions are its numbered bar (`1 ▲  2 ▼  3 Top  4 Drop`). The digit opens the panel: `▲ · move`, then `sends move to queue · q-acme-web-46` and its values (`before 45 · fleet acme-web · issue 46`). Enter sends it. Esc or ⌫ backs out. Indented chain rows and `held until #46` (red) are the phone's. |
| `issue-*` | the #15 sheet, cursor on a verdict row | At 40 the sheet takes the screen under a `‹ title` bar. At 120 it opens in the right column, and the Queue stays on the left, greyed. Stage chips, status rows, verdict rows (Enter opens the verdict sheet), links (Enter shows the URL in a panel to tap or copy) and history are drawn from the sheet's blocks. |
| `ask-*` | q33 unfolded, `1` pressed | Enter on a folded card unfolds it. The question is bold. Options are numbered with the recommended one first and marked `★ recommended`. `[3] other…` asks for a line typed into the panel. A routine ask's default ticks: `⏱ takes the recommendation in 9m`. The picked option turns inverse. |
| `run-*` | run card r7, `1` pressed | The card shows the command, folder, touches, sha and time left, then `[1] approve  [2] deny`. The panel repeats what runs: the command, folder and time left, and what is sent (`choice approve · sha …`). Enter runs it. |
| `refused-*` | Enter on r7, refused | The panel turns red with the handler's own line (`REFUSED CARD_CHANGED: …`) and `nothing ran`. Enter, Esc or ⌫ closes it. |
| `backlog-*` | acme-web backlog, `has AC` on, typing a search | The filter chips are one hot: their bar numbers each filter. Enter on Search types into it in place, and Enter again runs the search. Each issue is a `[ ]` / `[x]` tick that Enter flips locally. `[Queue ticked]` is the sheet's done button, the only thing that sends. |
| `hosts-*` | cursor on gpu-box; a reap's toast | A host's actions (`1 Off`, `On`, `Reap N`) are its bar. A finished action's toast shows on one line above the bar (`✓ reaped 2 leftover processes on mini`) until the next key. The Token proxy facts show. Its "How to set" row is pruned, because the token is entered on a paired device only. |
| `lanes-*` | the Lanes place, cursor on the waiting lane | The same row rules: sections by state, figure glyphs, age. Enter opens the lane's sheet. |
| `events-*` | the Events place, cursor on an ask | The kind chips filter, the same as on the phone. Long event titles wrap in full. Enter opens the ask sheet, a PR link, or a lane's sheet. |
| `tab-*` | Tab pressed on the Board | Threads is #1352's tab, unchanged. While you are on Threads, the Board tab keeps its count in amber (`Board 5`). Tab goes back to the Board as you left it: same place, same cursor. |
| `down-*` | serve stopped, shed being started | The top line's right side lists what is down: `✗ serve`, `↻ shed` (spelled out at 120). An amber row says shed is being started. The board itself keeps working, because it builds the screens in-process. |

Not drawn: Herd (its menu row is absent) and the proxy-token entry (its row is pruned, not dead). An `images` block
is one line per image, `▸ image <alt> · shown on the phone`. Any block type the renderer does not know draws its
text fields. That rule is generic, so it has no screen of its own here.

## Keys (the test list)

Every key is on a phone keyboard, with no F-keys. Each Esc has ⌫ as a second key. Ctrl keys come from the terminal
app's Ctrl key (Blink, Termius). Threads keeps #1352's keys.

| where | key | does |
|---|---|---|
| anywhere | Ctrl-C | quit; the terminal is restored (also after an error); a running action finishes first |
| anywhere | Tab | switch Threads / Board; each keeps its place, cursor and scroll |
| Board | ↑ ↓ or k j | move the cursor from hot to hot (row, button, field, chips, choice, card) |
| Board | Ctrl-B / Ctrl-F | scroll a page |
| Board root | ← → or h l | the previous or next place: Queue, Events, Lanes |
| Board root | m | the menu sheet: Hosts, Fleets, Workflow |
| Board | Enter | follow the hot: open its sheet or screen, unfold a card, tick a box, type into a field, show a link; on a hot with one action and nothing to open, open that action's panel |
| Board | 1 … 9 | open the panel for that action of the hot (a swipe item, a chip, a choice option, approve or deny) |
| Board | Esc or ⌫ | close the sheet or screen on top (Done, Close, Cancel) |
| Board root | q | quit |
| Board | ? | key sheet (this table) |
| a field | any text key | type; ← → move, ⌫ deletes, Ctrl-U clears; Enter keeps it (a search runs), Esc undoes it |
| panel open | Enter | send it (run, for a run card) |
| panel open | Esc or ⌫ | back out; nothing is sent |
| panel, `other…` | any text key | type the line it sends |
| refused | Enter, Esc or ⌫ | close the refusal |

While the cursor is in a field, `q`, `m`, `j`, `k`, `h`, `l`, `?` and the digits only type.
