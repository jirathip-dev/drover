## Summary

Decision: `drover chat` is one full-screen terminal app with two tabs, Threads and Board. The top line holds the tabs, plus what is down on the right. Below it come the chat, then a fixed input box and one key-hint row. An answer takes two presses: a number, `a` or `d` opens a panel that shows exactly what will be sent or run, and Enter confirms. At 120 columns the thread list stays on the left and the chat sits beside it.
Screens: thread list, chat, an open choice, a run card selected for approve, the card refused, and shed down (being started; would not start). Each is drawn at 40x18 and 120x40.
Open questions: (1) The 120-column split view: keep it (recommended)? (2) While a choice or card is selected, its panel replaces the input box, which is empty by then anyway (recommended), instead of sitting above it. (3) With two asks open, the numbers answer the newest one (recommended). (4) List previews end in `…`, as on the phone. That is the one place text is shortened: OK?

## How it is drawn

`index.html` draws every screen as a terminal character grid. It uses the default scheme's tokens
(`assets/schemes/github-dark.json`) and board.css's monospace font. `screens.js` lays out every grid from one model
(the threads, the t139 story, the keys), with the rules the build follows: text wraps at word breaks and is never
cut, the bottom rows stay fixed, and the newest message is at the bottom. Any row wider than the grid stops the
render. A 40-column grid is 390 px wide and a 120-column grid is 1440 px wide, so the issue's 40x18 / 120x40 and the
repo's 390 / 1440 rule are the same shots.

`node docs/design/drover-chat/shoot.mjs <chrome-headless-shell>` (from the repo root) writes every PNG at DPR 2, and
exits 1 on any layout problem.

The story is today's outage. Self-update breaks serve (#1350). In t139 the owner asks why, answers q2170, and approves
run card r42, which restarts serve. Then shed goes down, and the owner's next message waits for it.

## Images

| PNG (40x18 · 120x40) | shows | decision |
|---|---|---|
| `list-40x18.png` · `list-120x40.png` | the thread list | Grouped per repo (`ALL REPOS` first, as on the phone), newest first in each group. Each row is: `●` unread · id · title · age · state (`!` waits on your choice, amber; `✻` working; `☾` asleep), then the last message, clamped with `…`. `›` and a raised row mark the selection. There is no input box on the list, so `j` `k` `q` are free. At 120 the open thread sits beside the list. |
| `chat-40x18.png` · `chat-120x40.png` | a reply streaming in, with text in the box | The thread header shows `‹`, id, title, then repo and state (at 40, the state's mark only). Messages have a left edge: amber for you, purple for the orchestrator. A fenced block is drawn as a box, a file as `▸ file <path>`, and system lines are grey `· …`. `✻ working… 14s` ticks while the orchestrator works. New lines appear at the bottom, and only changed cells are drawn. `▲ N lines above` shows when you have scrolled. |
| `choice-40x18.png` · `choice-120x40.png` | q2170 open, `1` pressed | The option turns inverse. The `answer q2170` panel shows exactly what is sent (`[1] …`, to q2170 in t139, as owner). Enter sends it the way `drover answer` does. Esc or ⌫ backs out. |
| `card-40x18.png` · `card-120x40.png` | run card r42, `a` pressed | The card shows its command, folder, why and touches, and `[a] approve [d] deny`. The `approve r42` panel shows the full command, the folder, the host, the sha and the time left. Enter runs it the way `drover card approve --sha` does. `d` opens the same panel as `deny r42`. |
| `refused-40x18.png` · `refused-120x40.png` | Enter on an expired card | The panel turns red and shows the CLI's own refusal (`REFUSED EXPIRED: …`, or `CARD_CHANGED`, `ALREADY_SETTLED`) and `nothing ran`. Esc or ⌫ closes it. |
| `shed-starting-40x18.png` · `shed-starting-120x40.png` | serve back, shed down | The top line's right side lists what is down, from doctor's serve and shed checks: `✗ serve` (red) and `↻ shed` (amber, being started). At 120 they are spelled out (`✗ serve down: the page is off, chat works`). An item clears when its check passes again: here serve has. While shed is down, one amber row under the top line says it is being started with the existing kickstart. Your message is logged but not yet delivered: `… logged · waits for shed` stands in place of the ✓. |
| `shed-failed-40x18.png` · `shed-failed-120x40.png` | shed would not start | The row turns red and gives the command to run (`launchctl kickstart gui/501/dev.shed.serve`, the real uid). The thread stays readable and scrollable, and the message still waits, then goes once shed answers. |

Answered asks and run cards stay in the thread, greyed, with no keys. The hint row offers `1-3 answer · a d card`
only while an ask or card is open.

Not drawn: the Board tab's content (#1354). Tab switches to it, and the frame (top line, tabs, hint row, panel) is
shared, so #1354 adds a tab without touching the chat.

## Keys (the test list)

Every key is on a phone keyboard. There are no F-keys, and Esc has a second key, ⌫, used while the input box is
empty. Ctrl keys come from the terminal app's Ctrl key (Blink, Termius).

| where | key | does |
|---|---|---|
| anywhere | Ctrl-C | quit; raw mode off, alternate screen left (also after an error) |
| anywhere | Tab | switch Threads / Board |
| list | ↑ ↓ or k j | move |
| list | Enter | open the thread (it is marked read) |
| list | q | quit |
| list, chat with the box empty | ? | key sheet (this table) |
| chat | any text key | type into the box |
| chat | Enter | send the line, as `drover thread say` does; the box clears only once it is logged; a failed send keeps the text and shows the error |
| chat | ← → | move the cursor in the box |
| chat | ⌫ | delete a character; with the box empty: back to the list |
| chat | Ctrl-U | clear the box |
| chat | ↑ ↓ | scroll a line; Ctrl-B / Ctrl-F scroll a page |
| chat, box empty | Esc | back to the list |
| chat, box empty | 1 … 9 | select that option of the open ask |
| chat, box empty | a / d | select approve / deny of the open card |
| a selection open | Enter | confirm: answer, approve or deny |
| a selection open | Esc or ⌫ | back out; nothing is sent |

With text in the box, `1`–`9`, `a`, `d`, `q`, `j`, `k` and `?` only type. At 120 columns, Enter on the list moves the
focus to the chat on its right, and Esc or ⌫ moves it back to the list. The panel that is not focused is grey.
