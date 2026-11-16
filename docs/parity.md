# Parity rows (#553, #1094)

Every row of the #553 parity list (rows 1–90 and the reliability rows R1–R27, whose tests are `parity_r<n>`), the test that guards it on the drover
page and its status. CI's parity step (`scripts/parity-check.sh`) reads this table:

- `kept`: the page has it and its test, `parity_<row>` (a Rust test in `crates/cli/tests`, a WebKit one in
  `crates/cli/tests/parity_webkit.rs`, or a check in `scripts/ui-audit.sh` / `assets/ui-audit.js`), must exist;
- `page-only`: as `kept`, for a block cooee does not draw;
- `cooee`: the row lives in cooee, not on the page;
- `open #N`: the page lacks it; #N brings it back with its test (a WebKit one may already sit there, `#[ignore]`d,
  red until #N lands); `open` with no number: nothing filed yet, the owner decides port or drop;
- `dropped q<N>`: the owner dropped it in q<N>.

A PR that deletes a parity test, or a row, fails CI unless the row reads `dropped q<N>`. A test may guard several
rows; each of them names it. A UI PR's body carries a `## Parity` section naming the rows it changes or drops
(`roles/rev.md`).

| row | feature | test | status |
|---|---|---|---|
| 1 | Home Screen app: icon, full-screen, no zoom, offline paint | parity_1 | kept |
| 2 | Live stream, sync dot, gapless reconnect, scroll never jumps (#65) | parity_2 | open #1084 |
| 3 | Themed loading strip with "as of Xm ago" | parity_3 | kept |
| 4 | Short self-clearing toasts; no text runs off screen | — | open |
| 5 | Pull to refresh and ↻ refresh, reload on resume | — | open |
| 6 | Pair phone by code approved on host; list and remove devices | parity_6 | kept |
| 7 | Unpaired devices read-only (forms and composer off, pair hint) | — | open |
| 8 | Web push: enable, truthful status, test push, themed titles | parity_8 | kept |
| 9 | Push tap opens its thread or row; no push for open thread | parity_9 | open #1088 |
| 10 | Themes as data: farm, kitchen, garden, terminal, moments | parity_10 | kept |
| 11 | Colour schemes: 8 presets plus user scheme files | parity_11 | kept |
| 12 | Appearance per phone: theme, colours, pixel or ascii style | — | open |
| 13 | Settings sheet shell: section index, deep links, alert dot | — | open |
| 14 | Report mode: tap an element, add a note, send | — | open |
| 15 | Thread list per repo: unread dot, state, last line, badge | parity_15 | kept |
| 16 | Start a thread by typing; auto title, then retitle | — | open |
| 17 | Swipe to archive, 5 s undo, restore | — | open |
| 18 | Message blocks: colours, markdown, system lines, folded tool runs | — | open #1086 |
| 19 | Fenced code blocks with a copy button | — | open #1086 |
| 20 | Several attachments per message, thumbnails, image viewer (#406) | parity_20 | open #1086 |
| 21 | Instant send: sending…/✓, red retry on failure | — | open #1087 |
| 22 | Growing composer, pinned send, smart follow-scroll | — | open #1087 |
| 23 | iOS keyboard: composer sits on keyboard, page never jumps | parity_r4 | open #1086 |
| 24 | Back button and edge swipe; side-by-side layout ≥1000 px (#403) | parity_24 | open #1088 |
| 25 | Working pill with timer and [stop]; reconnecting/offline states | — | open #1087 |
| 26 | Queue or steer during a turn; edit or remove queued | — | open #1087 |
| 27 | Per-thread drafts, picked files included | — | open #1084 |
| 28 | Long-press menu: copy, copy as markdown, quote | — | open #1086 |
| 29 | Sticky thread header; last 30 messages, older on scroll (#408) | parity_29 | open #1086 |
| 30 | Choice rows in chat, "other" box, answered ✓ line | parity_15 | open #1088 |
| 31 | Conductor-answered asks keep [change] for 30 min | — | open #1088 |
| 32 | Config-proposal card: diff, apply/discard, stale, waiting bar | — | open #1088 |
| 33 | Secret request card with masked input | — | open #1088 |
| 34 | Harness pick at thread start; model/effort chip in composer | — | open #1087 |
| 35 | Handoff chips, flow-back lines, pick-a-thread choice row | — | open #1088 |
| 36 | One repo at a time: chips, swipe, needs dot, [all] | — | open #1089 |
| 37 | Fleet cards in their colour; paused folds; needs-you first | parity_37 | kept |
| 38 | Blocked chains as a tree; chain collapsed to one row | parity_38 | kept |
| 39 | Groups, queue place numbers, "+N more", stage times, done today | parity_38 | open #1089 |
| 40 | Issue and PR titles; links to issue, PR, checks | parity_40 | open #1085 |
| 41 | Reorder rows ▲/▼ | parity_41 | kept |
| 42 | Pause and resume a fleet | parity_42 | kept |
| 43 | Drive now, plus per-fleet drive result line with details | — | open |
| 44 | Pipeline stage chips on each row | parity_44 | kept |
| 45 | Issue card sections; its lanes newest first, tap opens sheet | parity_45 | kept |
| 46 | "waiting: decision" with options; link to the asking thread | — | open #1088 |
| 47 | Backlog: search, filters, tick issues, [queue N] | parity_47 | kept |
| 48 | Legend sheet [?] and serial-group 🔒 tags | parity_38 | kept |
| 49 | Lanes grouped by state, then by repo, newest first | parity_49 | kept |
| 50 | Lane row: name and age, waiting reason, one-line "now:" | parity_50 | kept |
| 51 | Parked, dead and host-offline states with plain reasons | — | open |
| 52 | Conductor sessions listed as lanes; tap opens thread | — | open #1088 |
| 53 | Non-drover agents get only [close pane] | parity_53 | kept |
| 54 | iOS-style bottom sheet: drag, 3 snap heights, flick, no flash | parity_r25 | open |
| 55 | Transcript as role blocks; rolling log; load more | — | open |
| 56 | Summary view with a full-conversation toggle | — | open |
| 57 | Reply box to a lane (rate limit, paused/reviewer refusals) | — | open |
| 58 | "/" command menu in the reply box | — | open |
| 59 | Stop, Kill with dry-run confirm, Clean with reasons | parity_59 | kept |
| 60 | Lane figures animated by state (sprite, ascii, glyph) | — | open |
| 61 | Herd scene: per-repo ranch, state bands, queued at gate | — | open #1092 |
| 62 | Scene weather from host load, day/night, repo switching | — | open #1092 |
| 63 | Needs-you cards above every tab: options, free text, count badge | parity_63 | kept |
| 64 | Routine vs owner tag, default-answer countdown | — | open |
| 65 | Answer a blocked lane's menu by tap (stale-menu guard) | — | open |
| 66 | Permission-prompt card: reason, command, auto-deny countdown | — | open |
| 67 | Needs-human card: merge anyway, one more round, park | parity_67 | kept |
| 68 | [later] and [later all] hide cards on this phone | — | open |
| 69 | Design review: PNGs inline, approve/revise/reject | parity_69 | kept |
| 70 | Verify screenshots on cards (lane-folder images only) | parity_70 | kept |
| 71 | Alert when an answer doesn't reach its lane | parity_71 | kept |
| 72 | Host cards: gauges, status dots, aligned rows, top memory | — | open #1091 |
| 73 | 24 h sparklines; tap for a 1h/6h/24h chart | — | open #1091 |
| 74 | Host health and services: herdr, ssh, logins, version, drift, timers | — | open #1091 |
| 75 | Turn a host on or off | — | open #1091 |
| 76 | Host tags; cloud host spend and budget | — | open #1091 |
| 77 | Fleet disk: budget, orphan folders, leftovers, [reap] | — | open #1091 |
| 78 | Agent providers: account 5h/7d meters, caps, DeepSeek balance | — | open #1091 |
| 79 | Provider sign-in terminal, add key, [use] to switch | — | open #1091 |
| 80 | Fleets: per-repo config sheet with pipeline graph | parity_80 | kept |
| 81 | Role model/effort pickers | parity_81 | kept |
| 82 | Conductor role and new-thread chat defaults | — | open |
| 83 | Activity: time groups, kind glyphs, [important] filter, older | parity_83 | kept |
| 84 | Notifiers list and Telegram bridge status | — | open |
| 85 | Real config load errors shown plainly | parity_85 | kept |
| 86 | Host and Events as tabs; [◐] look button (drop? in #553) | — | open |
| 87 | Lanes mode button; fixed Corral horse art (drop? in #553) | — | open |
| 88 | Queue as a table with per-row buttons (drop? in #553) | — | open |
| 89 | Server-rendered HTML tabs and `/frag` fragments (drop? in #553) | — | open |
| 90 | Public demo as a static export (drop? in #553) | — | open |
| R1 | A keychain read waits forever on an invisible macOS prompt | parity_r1 | kept |
| R2 | A retired or unknown config key stops the whole service | parity_r2 | kept |
| R3 | One 401 latches the page into "unpaired"; a DB hiccup reads as not paired | parity_r3 | kept |
| R4 | iOS keyboard: composer gap, page jump, thread loses its place (#443) | parity_r4 | open #1086 |
| R5 | A full-screen loader hides the page on slow loads | parity_r5 | kept |
| R6 | Requests do quadratic work over the whole event log | parity_r6 | kept |
| R7 | Per-request live I/O (ssh, pane reads) on page loads | parity_r7 | kept |
| R8 | A broad `pkill`/`killall` from one agent kills every agent | parity_r8 | kept |
| R9 | Superseding an agent logs its close after the new start | parity_r9 | kept |
| R10 | An ask made during startup is hidden | — | open |
| R11 | A start that times out still comes up unlogged | parity_r11 | kept |
| R12 | A first-start prompt (trust folder) blocks a session silently | — | cooee |
| R13 | A safety guard fails open on bad input | parity_r13 | kept |
| R14 | A retried side effect repeats after a crash | parity_r14 | kept |
| R15 | A stuck turn or lane is never noticed | parity_r15 | kept |
| R16 | A duplicate id makes the UI leak a copy per refresh | — | open |
| R17 | Untrusted text is turned into file paths or images | parity_r17 | kept |
| R18 | Concurrent writes share one temp file name | parity_r18 | kept |
| R19 | Stale lock or marker files from dead processes block work forever | — | open |
| R20 | A delivery waits on another forever | — | cooee |
| R21 | An answer to a gone or restarted session is lost silently | parity_71 | kept |
| R22 | A remote machine's agents can't reach the main host | — | cooee |
| R23 | One unreachable host fails every action | — | open |
| R24 | Disk fills from per-session build folders and leftovers | parity_r24 | kept |
| R25 | Sheets and pages flash or jump on iPhone | parity_r25 | kept |
| R26 | Background processes left after an agent ends hold memory | parity_r26 | kept |
| R27 | "Working" flickers: agent state guessed from the terminal screen | — | cooee |
