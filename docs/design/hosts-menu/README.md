## Summary

Decision: Hosts is a row under [⚙] › screens. It opens a sheet listing hosts as bare rows (name, on/off, live lanes), main host first, then A→Z. Tapping a row shows its buttons, and each one asks before it acts. There are no charts or gauges.
Screens: the [⚙] menu with the Hosts row; the Hosts sheet; boat1 turned off (the confirm, then the result).
Open questions: (1) [reap] is on the main host's row only, because today's reap clears leftovers on the main host. Keep it that way, or add a per-host reap (a new op)? (2) The main host has no [off], because today's host op refuses it. Keep that?

## What each image shows

The PNGs are the real page: `web/dist/board.js` and `board.css` from this branch, drawing the fixture JSON in `screens/`
(the demo export's tabs, plus `screens/hosts` and `screens/hosts-off`). `drive.js` taps through to each state:
`index.html?view=menu|hosts|confirm|off`. 390 shots are 390 × 844 at DPR 3; 1440 shots are 1440 × 900 at DPR 2,
taken with chrome-headless-shell 1243.

| image | decision it shows |
|---|---|
| `menu-390.png`, `menu-1440.png` | Hosts is a menu row under [⚙] › screens, after drover's Fleets row, with the subtitle `on/off and reap, per host`. Nothing about hosts is added to the tabs or the header. |
| `hosts-390.png`, `hosts-1440.png` | The sheet `Hosts` has one section, `hosts (2)`. Each row shows the name, an `on`/`off` badge (green or amber), the kind (`main host` / `cloud host`) and the live lane count on the right. Main comes first, then the others A→Z. Both rows are tapped open here: mini shows `[reap 2]` (it is shown only while leftovers exist, and the subtitle counts them); boat1 shows `[off]`. The footer says history lives in Beacon. |
| `confirm-390.png`, `confirm-1440.png` | [off] asks first, using the page's own confirm: `Turn boat1 off?` · `No new lanes go to boat1; its 1 live lane finishes there.` · [cancel] [turn off]. [on] and [reap] ask the same way, with their own words. |
| `off-390.png`, `off-1440.png` | After [turn off], the sheet redraws from the action's answer: boat1's badge reads `off` (amber), its subtitle reads `cloud host · no new lanes, 1 lane finishing`, and its button is now `[on]`. |

## The screen, in blocks

The design uses only blocks the page already draws (`sheet`, `section`, `list`, `row` with `badge`, `subtitle`, `value`
and `swipe`, and `confirm`), the same way the Lanes tab's [stop] and the Queue's [drop] work. No renderer or CSS
changes are needed.

- **Row:** title = host name; badge = `on` (tone ok) or `off` (tone warn); subtitle = kind (`main host`, `cloud host`,
  `remote host`), plus `· N leftovers` on main when there are any, plus `· no new lanes, N lane(s) finishing` when off;
  value = live lanes on that host (the same count as the lane rows' `host` word).
- **Buttons (row swipe):** remote hosts get `Off` (bad tone) or `On`, which runs the existing `host` op with
  `{host, to}` and logs `host-off` / `host-on`. Main gets `Reap N`, which runs the existing `reap` op, only while
  there are leftovers. Every button carries a `confirm`.
- **After an action:** the answer redraws the whole sheet (`screen` op) and the toast reads `boat1 off` / `boat1 on` /
  `reaped N leftover process(es)`, using the ops' existing lines.
- **Not on it:** load, memory, disk, swap, sparklines, charts, services, tags, providers, sign-in, spend. The full
  Hosts screen already on this branch (gauges, sparklines, charts, sign-in, providers; its golden is
  `crates/cli/tests/golden/page/hosts.json`) should be cut down to this.

## Open questions, with my recommendation

1. **[reap] on the main host only.** `reap` kills processes still running in closed lanes' worktrees, but only lanes
   spawned on the main host; it takes no host. A per-host [reap] on a remote row would need a new op that reaps over
   SSH. **Recommend: main only, as drawn.** The AC says to call the existing op.
2. **No [off] on main.** `host` refuses `to=off` for the main host (`MAIN_HOST: edit fleet.toml by hand`).
   **Recommend: no button**, rather than a button that always refuses. If main is ever held by a sign-in, `on`
   still works through the same op; it would show as `[on]` then.
