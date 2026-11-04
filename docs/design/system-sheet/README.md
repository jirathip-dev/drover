# The System sheet (#440, phase 1: design)

The tab bar is `[queue] [lanes] [chat]`. On every tab, **[⚙]** sits in [◐]'s place and opens the **System** sheet:
one scrolling sheet with a section index on top. Its sections, in the owner's order:

1. Agent providers
2. Fleets
3. Hosts & health
4. Activity
5. Notifications & devices
6. Conductor
7. Appearance

[⚙] carries a red dot while a host card is red.

Everything here is shot from the real board: the `serve --export --demo` page, with `proposal.js` and `proposal.css`
loaded after serve.js and herd.js. Hosts & health and Activity are the Host and Events tabs' own parts. board.js draws
them from API v1 into the same `#host` and `#events` elements, which now sit inside the sheet. Fleets is each fleet's
[config] content, and Appearance is the look sheet's. The new parts (the provider cards, notifications, conductor
defaults) read `fixture/system.json`.

## Files

| file | what |
|---|---|
| `shots/tab-{queue,lanes,chat}-{390,1440}.png` | the three tabs, [⚙] top right with its dot |
| `shots/gear-dot-390.png`, `shots/gear-calm-390.png` | the header with a red host (the demo's `mini`: disk under its floor) and with none |
| `shots/system-<section>-390.png` | the sheet opened at each section (`#system/<section>`), one phone screen |
| `shots/system-all-{390,1440}.png` | the whole sheet, every section |
| `shots/system-{providers,fleets,hosts,activity,notifications}-1440.png` | the desktop layout: the index is a rail, the sections a column |
| `shots/providers-{use,move,addkey,signin}-390.png`, `providers-signin-1440.png` | #441's flows: [use]'s scope, the move prompt, [+ add key] refused, a sign-in |
| `proposal.js`, `proposal.css` | the design as a script on the real page. Phase 2 folds it into serve.rs `page()`, serve.js, serve.css and web/src |
| `fixture/system.json` | the providers (#441's proposed API shape), alerts and conductor data |
| `shoot.mjs`, `cdp.mjs` | the shots and the checks (`report.json`); no dependencies, Chrome over `--remote-debugging-pipe` |
| `report.json` | the measurements below |

To regenerate from the repo root:

```sh
HERDSMAN_HOME=demo FLEET_CONFIG=demo/fleet.toml FLEET_STATE=demo/events.jsonl herdsman serve --export /tmp/site --demo
cd docs/design/system-sheet && node shoot.mjs /tmp/site <chrome-headless-shell>
```

`shoot.mjs` makes three copies of the export:
- `today/`, as exported;
- `next/`, with the proposal and a Host fixture: the demo's `mini` plus a second host, `studio`, set live, so each card
  shows [sign in ▾] and [off];
- `calm/`, the same with no red host.

## The header and the tabs

```
herdsman  demo ●  [?]  [↻]  [🐎]  [⚙]•        ← [⚙] where [◐] was; • = a host card is red
…
[queue·4]        [lanes]        [chat·1]      ← three tabs, each a third of the bar
```

- **[⚙] is on every tab**, with the same one-width bracket as its neighbours. [?] stays on Queue only, as today.
- **The dot:** red, 8 px, on [⚙]'s top-right corner. It shows while any host card's dot is red, meaning past a limit,
  under a floor, or down. The same dot sits on the index's `[hosts]` chip. Its label says why:
  `system (host alert: mini: disk under floor)`. Amber does not dot (Q2).
- **Tabs:** `nav` keeps `queue`, `lanes`, `chat` with their counts (`queue·N`, `chat·N`). The Host and Events tabs
  are gone.

## The sheet

```
━━━━ (handle)
system                                      [close]
[providers] [fleets] [hosts]• [activity] [ale…   ← the index: one line, scrolls sideways, the current one filled
──────────────────────────────────────────────
agent providers            2 providers · 3 need you
┌ Anthropic Claude ─────────── order A → B → C ┐
…
```

- **A layer of its own (`#sys`), at the full detent.** It uses the board's sheet box (`.sh`): the handle, `--bg`, the
  edge line. A lane, legend or setup-terminal sheet (`#sheet`) still opens above it, so a sign-in started here streams
  over it (`providers-signin-390.png`).
- **The index** sits between the head and the scroller, so it is always in view:
  - at 390 it is one line of bracket chips that scrolls sideways;
  - the current section's chip is filled, like the tab bar's, and follows the scroll;
  - a tap scrolls to that section;
  - the chips use short names (`alerts`, `look`, Q5), and each section head carries the full name;
  - at ≥ 900 px it becomes a rail on the left.
- **A section head** uses the tab line's style: bold, with a muted summary on the right. The old tabs' theme words name
  their sections: `barn · hosts & health`, `logbook · activity` (kitchen: `the kitchen ·`, `the rail ·`). With a red
  host, the Hosts head turns red, like a red group.
- **[close], the handle, the backdrop or Esc** closes the sheet and puts back the tab's hash.
- **[⚙] always opens at Agent providers** (the AC's readback). A deep link opens at its section instead.

### 1. Agent providers (#441)

One card per provider, the host card's box, with one row per account or key:

```
Anthropic  Claude                       order A → B → C
A [active · 1st]                                     ok
  used by impl, rev, sec, conductor · 3 running lanes
  5h ▕███████░░░░░▏ 58%   7d ▕████░░░░░░░░▏ 34%  usage 2m ago
  [sign in again] [remove]
B [2nd]                                capped until 14:20
  …  [use] [sign in again] [remove]
┃C [3rd]                       token invalid: sign in again
  [+ add account]
DeepSeek  API keys                    order main → spare
main [active · 1st]                              ✓ $12.40
  used by impl · 2 running lanes · checked 12m ago
┃spare [2nd]                           invalid: replace it
  [+ add key]
  [get a key] platform.deepseek.com → API keys
```

- **A row:**
  - its name and its badge (`active · 1st`, or its place);
  - its state on the right: green `ok` or `✓ <balance>`, amber `capped until <time>`, red `token invalid: sign in
    again` or `invalid: replace it`. A red row also gets a red left rule;
  - the roles using it and its running lanes;
  - Claude's 5h and 7d usage as the host cards' 12-cell gauges, with the age of the status line (#258). A key shows when
    it was last checked.
- **[use]** shows on a row that is not first and not invalid (`providers-use-390.png`). It opens a panel under the row,
  not a second sheet:
  - `[every role]` or `[one fleet's role]`; the second shows the fleet chips, then that fleet's role chips;
  - `[use B for acme-api · rev]` and `[cancel]`;
  - a line under them: "moves B to the front there · new lanes use it at once · logged by you".
- **Then, if lanes run on another account,** one amber prompt replaces the panel (`providers-move-390.png`):
  - "move 3 running lanes to B?";
  - the lanes, each with its account;
  - "each finishes its turn, then resumes the same session on B";
  - `[move 3 lanes]` and `[keep them on A]`.
- **[+ add account] and [sign in again]** start `claude setup-token` in #303's setup terminal on the main host. Its
  sheet opens over System (`providers-signin-390.png`):
  - the screen, with an [open] link for the URL and a "paste the code" box;
  - "the token goes straight to the keychain, never shown";
  - the row appears, or turns `ok`, once the token is saved; the pane is cleared.
- **[+ add key]** opens a panel above the button (`providers-addkey-390.png`):
  - `[get a key]` with the path to DeepSeek's key page;
  - `name`, then `key` in a masked field;
  - `[check and save]`.

  A refused key saves nothing. The field stays filled, and a red line says why ("not saved: DeepSeek says 401
  Authentication Fails …"); the button reads `[check again]`.
- **[remove]** asks first, owner only: "remove Anthropic C? its token leaves the keychain".
- **Under the cards:** "secrets stay in the main host's keychain · remote lanes get theirs over ssh stdin · a conductor
  proposes, you tap".
- **Account names keep their case inside buttons** (`[use B …]`). Buttons are lowercase everywhere else (`.an`).
- **The Host tab's `accounts` and `deepseek` rows (SERVICES) move here.** They don't repeat in Hosts & health.

### 2. Fleets

- **One row per fleet:** `■ acme-api  staging · impl · rev · sec  ▸`, in the fleet's colour (`--fk`). It opens to that
  fleet's [config] content (#162), as it is today:
  - GENERAL, PIPELINE (the graph), LIMITS, ROLES with the `set` pickers, CONDUCTOR's #401 choices, ALERTS, HOSTS / NEEDS;
  - the path line on top;
  - an open row gets a 3 px left rule in its colour.
- **Rows start closed,** except the one a link names (`#system/fleets/acme-api`).
- **[config] on a Queue fleet card stays,** as a shortcut: it opens System at that fleet's row, open (Q1).
- On a live board a row's content loads when it opens (`frag?id=config&fleet=`). An export carries it inline
  (`template[data-cfg]`), as today.

### 3. Hosts & health

The Host tab's part, unchanged: HOSTS (a card per host, gauges, 24 h sparklines, charts on tap, [off]/[on]), FLEET
DISK, SERVICES and FLEETS, ranked and drawn by serve.js (`groups()`, `sparks()`). Three things leave it:

- **ALERTS** (push, alert devices, paired devices) goes to Notifications & devices.
- **The `accounts` and `deepseek` rows** go to Agent providers.
- **[sign in ▾] drops `claude`.** It keeps the other configured logins, such as `gh` (and `codex`, not a provider yet),
  per host. Its summary keeps one line beside the name.

The `theme` row stays in SERVICES: it is the board's theme file and whether it is valid, for every phone. Appearance
is this phone's pick (Q4).

### 4. Activity

The Events tab's part, unchanged: the chips (`[all] [important] [● fleet]…`), the time groups, [older], the kind
sprites and theme words. Its `/live` reads (`herd-ev`, `herd-refresh`) keep painting it while the sheet is closed.

### 5. Notifications & devices

The config sheet's groups (`.cg`, `.kv2`), two columns at ≥ 900 px:

- **PUSH:** the push row (`on · last alert 14m ago`; red or amber with its fix, as today), then `this phone
  [enable alerts]`. The button shows only in the home-screen app, as today.
- **ALERT DEVICES:** each subscription, `alerts on 14d ago`, [remove].
- **PAIRED DEVICES:** each device, `paired 14d ago · used 1m ago`, [remove]; `[pair another device]`, then
  `herdsman serve approve <code>`.
- **NOTIFIERS:** the global `[[notify]]` entries (web push / command, their events). A fleet's own notifiers stay in its
  Fleets row's ALERTS.
- **TELEGRAM BRIDGE:** on or off and when it last polled; the chat, whether `owner` is set, the topic count; the token
  as `keychain · TELEGRAM_BOT_TOKEN`, never its value. Off reads `off` plus the line that turns it on.

### 6. Conductor (#404)

- **CHAT DEFAULTS:** `[chat]`'s harness (`claude hermes dsh`), model (`models`), effort and account(s), as #401's chip
  rows; the pressed one is underlined. A tap writes `[chat]`. Over the chips: "a new thread starts with these · a
  thread keeps its harness · its composer changes model and effort". The head says `[chat] · set`, or
  `unset → each fleet's conductor`.
- **PER FLEET:** each fleet's conductor in one line (`set · claude · opus`, `default: impl · claude`), with an [edit]
  link to its Fleets row (`#system/fleets/<fleet>`), where #401's choices stay.

### 7. Appearance (#290)

The look sheet's content, unchanged: THEME (sprite rows), COLOURS (swatch rows, `(theme default)`, `N pass the gate`),
STYLE (pixel / ascii), then "this phone only · …". serve.js's own `lpick` applies a tap at once and the sheet stays open
(`report.json`: kitchen picked, still open). The keyboard works as before (↑/↓, Enter, Tab). At ≥ 900 px it is two
columns, as the look sheet is today.

## Links

| link | lands on |
|---|---|
| `#system`, `#system/<section>` | the current tab with the sheet open at that section: `providers`, `fleets`, `hosts`, `activity`, `notifications`, `conductor`, `appearance` |
| `#system/fleets/<fleet>` | Fleets, that fleet's row open |
| `#host` | Queue, with `#system/hosts` |
| `#events` | Queue, with `#system/activity` |
| `#config`, `#config/<fleet>` | Queue, with `#system/fleets[/<fleet>]` |
| a stored `tab` of `host` or `events` (localStorage; phase 2 also the `tab` cookie and `?tab=`) | Queue, sheet closed |

The hash follows the sheet: the index rewrites it (`replaceState`), and closing puts back `#<tab>`. Phase 2 does the
same in the head script, so `#host` never paints the old tab first.

## Measured (`report.json`)

| AC | result |
|---|---|
| the tab bar has exactly queue, lanes and chat | `["queue","lanes","chat"]` on all three tabs; no `#look` |
| [⚙] on every tab; the dot with a red fixture host, none without | `gear: true` on queue, lanes, chat; `next` (mini red) shows the dot, `calm` doesn't; the `[hosts]` chip follows it |
| [⚙] opens System at Agent providers | open, `#system/providers`, `[providers]` current, scrolled to 0 |
| every former Host control in its section | 2 host cards, 10 history rows, [sign in ▾] with `gh` and `codex` only, `studio [off]`, groups HOSTS, FLEET DISK, SERVICES, FLEETS |
| every former Events control | 114 rows and the 6 chips, in Activity |
| every former [config] control | 4 fleet rows; acme-api's 7 groups; 21 role pickers; 12 conductor choices |
| every former [◐] control | 3 theme, 8 colour and 2 style rows; a pick applies, the sheet stays open |
| ALERTS' controls | push, [enable alerts], 1 alert device, 3 paired devices, the Telegram bridge, in Notifications |
| nothing left behind | no `#look`, no host or events tab, `#host`/`#events` not in `main` |
| old routes redirect to `#system/<section>` | `#host` → `#system/hosts`; `#events` → `#system/activity`; `#config/acme-web` → `#system/fleets/acme-web` (row open); a Queue [config] tap → `#system/fleets/acme-api` (row open); each on the Queue tab |
| 390 px: nothing wider than the phone | page 390 px wide in every section; no visible element past the edge outside a clipping box. The scroller's `scrollWidth` reads 8 px over: a right-aligned bracket button's invisible tap ring (`::after`, inset −8 px), as on today's tabs |
| script errors | none |

## Phase 2: what changes

- **serve.rs `page()`:**
  - `pick` becomes `<button id=sysb … aria-haspopup=dialog>⚙<i class=sd hidden></i></button>`;
  - `nav` keeps three anchors;
  - `section#host` and `section#events` move into the sheet's markup (`#sys`), with the same ids;
  - the head script maps `host` and `events` to `queue` and opens the sheet for `#host`, `#events`, `#config`, `#system`;
  - `TABS` drops `host` and `events`; `frags` still serves their parts for the sheet.
- **serve.js:**
  - the sheet's open, close, index and scroll-spy, and the routes (`proposal.js` §2 and the end);
  - `lsheet()` becomes Appearance's renderer (`#look` and its focus return go);
  - `config()` becomes the Fleets row's loader;
  - `badge()` no longer touches host or events;
  - `alerts()`/`#push` move with ALERTS;
  - the dot follows `groups()` (it runs on every host draw).
- **web/src:**
  - `host.ts`: ALERTS, `accounts` and `deepseek` leave `tab()`, and a card's `setups` drop Claude's login (the server
    can drop it from `setups` instead, +0);
  - a new `system.ts` draws Agent providers, Notifications (from `HostTab`'s `push`, `key`, `subs`, `devices`, plus
    the new `telegram` and `notify`) and Conductor;
  - `npm run build` refreshes `web/dist`.
- **serve.css:** `proposal.css`.
- **`src/`, small:**
  - API v1 `host` gains `telegram` (on, chat, owner set, topics, last poll; never the token) and `notify` (the global
    notifiers in words), about +20;
  - Conductor reads `[chat]`, which comes with #404;
  - `providers` is #441's.
- **Tests that name the old UI:**
  - `crates/cli/tests/themes.rs` and `schemes.rs` (`id=look`, "one [◐]");
  - `ui-audit.js`'s #290 gate (`[◐]` on 4 tabs), now [⚙] on 3;
  - the Host and Events tab tests: same parts, new place.
- **New tests (the issue's AC):**
  - the tab bar is exactly queue, lanes, chat;
  - [⚙] on each tab, with the dot from a red fixture host and none from a green one;
  - each former Host, Events, [config] and [◐] control found inside its section (the `moved` list above is the
    checklist);
  - `#host`, `#events`, `#config/<fleet>` and a stored `host` tab land on `#system/<section>`.

`src/` net +0: this is a design lane (docs only).

## Open questions for the owner

1. **[config] on the Queue fleet card.** As drawn it stays, and opens System at that fleet's row. **Recommend:** keep
   it. It is one tap to the settings you look at most, from where you are. The alternative removes it: one control
   less on each card, and Fleets is two taps away.
2. **What lights the dot.** As drawn, only a red host card does: past a limit, under a floor, or down. A red service row
   (`tick: job missing`) or an invalid provider key does not; those show in their sections. **Recommend:** as drawn,
   so the dot stays rare and means "a host needs you". The alternatives add red services, or amber hosts.
3. **#441 and #404 come after.** Agent providers needs #441's store and actions; Conductor needs #404's `[chat]`.
   **Recommend:** #440 ships first.
   - Agent providers shows today's data (the Host tab's `accounts` meters and caps, the DeepSeek balance) as these
     cards, read-only, with no [use], [sign in again] or [+ add].
   - Conductor shows `unset → each fleet's conductor` plus the per-fleet lines.
   - #441 and #404 then fill in their parts of the approved design.
4. **The board's `theme` row stays in Hosts & health** (SERVICES): it is the server's theme and whether its file is
   valid. Appearance is this phone's pick. **Recommend:** as drawn. The alternative also moves that row into
   Appearance, as one line above THEME.
5. **Index chips use short names:** `providers fleets hosts activity alerts conductor look`. About 3.5 fit at 390 and
   the line scrolls; each section head has the full name. **Recommend:** as drawn. The alternative uses the full names
   and wraps the index to two lines, about 40 px more always on screen.
