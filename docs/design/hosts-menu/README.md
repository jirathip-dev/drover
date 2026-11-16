## Summary

Decision: [⚙] gets a **Hosts** row that opens a bare list: one row per host, main first, then A→Z, each with its name, an on/off badge and its live lane count. Tapping a row shows its buttons: [off] or [on] on other hosts, [reap] on the main host, each with a confirm. Nothing else (no gauges, charts or health).
Screens: the [⚙] sheet with the new Hosts row, the Hosts list with the buttons showing, and the [off] confirm.
Open questions: (1) The issue asks for [reap] on every host, but today's `reap` only clears leftover processes on the main host. Recommended: [reap] on main only for now; reaping on other hosts would be a new feature in its own issue. (2) The main host can't be turned off from the page (`MAIN_HOST`, by hand in fleet.toml), so its row has no [off]. Recommended: keep it that way.

## How it is drawn

Every shot comes from the **real screens page** (`web/dist`), as in `../threads-tab`. `shoot.mjs` exports the demo,
serves `fixtures/hosts.json` as `/screens/hosts` and adds `preview.js` to the page shell. `preview.js` adds the Hosts
menu row (the line `cooee-plugin.toml` would get) and taps through to each view. For the confirm shot only, it drops the
demo flag so the confirm can open. Nothing gets posted.

`node docs/design/hosts-menu/shoot.mjs <chrome-headless-shell>` (repo root; it needs `drover` on PATH). The 390 shots are
taken at DPR 3, the 1440 shots at DPR 1.

The screen uses only blocks the page already draws (`sheet`, `section`, `list`, `row` with `badge`, `value` and
`swipe` actions, each action's `confirm`), so `web/` doesn't change. Impl adds:

- a `[[places]]` `menu-row` in `cooee-plugin.toml`: label `Hosts`, subtitle `on, off and reap per host`, screen `/screens/hosts`;
- `host` and `reap` in its `[permissions]` `actions`, with `reap` also listed in `destructive`;
- `/screens/hosts` in `screens.rs`, plus `host` / `reap` in `action()`, both calling the existing ops (`hosts::toggle` with `by: "web"`, `check::leftovers`).

| image | shows | decision |
|---|---|---|
| `settings-390.png`, `settings-1440.png` | [⚙] › screens: `drover`, then **Hosts** | Hosts is a menu row, not a tab (#927 dropped the Host tab) |
| `hosts-390.png`, `hosts-1440.png` | the list with every row's buttons showing: mini (main, on, 3 lanes, [reap]), air (off, 0 lanes, [on]), boat-1 (cloud, on, 2 lanes, [off]) | main first, then A→Z; one line per host; the badge is the on/off state, the value is the live lane count, the subtitle is `main`, `cloud` or `no new lanes`; [off] and [reap] are red; a footer explains off |
| `confirm-390.png`, `confirm-1440.png` | [off] on boat-1: "Turn boat-1 off? No new lanes go to boat-1; its 2 live lanes finish there." [cancel] [turn off] | each button asks first; [on] says "New lanes may go to air again.", [reap] says leftovers get TERM, then KILL, and live lanes aren't touched |

## The JSON

- `fixtures/hosts.json`: the three-host screen in the shots.
- `fixtures/golden-two-host.json`: a draft of the AC's golden, for a two-host fleet with one cloud host (`main`, then
  `boat-1`). Impl's test fixes the exact words.

A row's tap does nothing but show its buttons (`swipe`, revealed by a tap or a left swipe, as on lane rows). The sheet
sets `stream`, so a host switched from the CLI updates while the sheet is open. Lane rows keep their `host` word, and
nothing else about hosts appears on the page.
