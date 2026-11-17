# The page draws the JSON screens (#927): before and after

`before/` is the old board (staging at cc09880), `after/` is the page that draws Drover's screens, both from the demo
export at 390 × 844, DPR 3, shot by `UI_AUDIT_VIEWS="queue lanes events" scripts/ui-audit.sh` on one machine in one
Chrome (chrome-headless-shell, Playwright's 1243 build). `scripts/pixel-diff.py before after`:

| view | pixels that differ |
|---|---|
| queue | 46.6 % |
| lanes | 26.0 % |
| events | 51.1 % |

Every difference is where the JSON differs from what the old tabs drew, or where the page's chrome changed by design:

- **Header:** `[?]` (the Queue legend) and `[🐎]` (Herd mode) are gone, as Herd mode and its `/herd.json` are; `[⚙]`
  opens the page's settings (the Fleets screen, theme, colours, alerts) instead of the System sheet, so its red host
  dot is gone with the Host tab.
- **Strip:** `last update hh:mm` shows while the tab has no live stream (always in the static demo).
- **Cards above every tab:** the `chat-cards` place (`/screens/cards`): each ask and needs-human card folded to its one
  line and main button, as the old needs-you cards on Lanes were; they now sit above Queue and Events too, and the old
  blocked-lane `prompt` card is not a card in the JSON.
- **Tab bar:** the manifest's places in order, `[queue] [events] [lanes]`; no `[chat·N]` (owner default: chat is
  cooee core's) and no Host tab.
- **Queue:** the screen's own blocks: the screen's `[fleets…]` and `[drive all now]` actions, the repo chips (`[all N]`
  first, `name N` with a red dot when it needs you, #1089), the stats row (merged · 7d, lanes, rounds / issue with
  sparklines), the counts row (needs you · in progress · queued · done today), then a
  card per fleet with its `driving` badge, its sections (`in progress`, `queued`) and rows whose title holds the issue
  number and whose badge is the stage. The old `git log --graph` tree, `[config]`, the stage chips line and the
  `in impl 1h 26m` time words are not in the JSON.
- **Lanes:** sections read `waiting`, `working`, `asleep` (#926, theme-dressed: `on the trail · working`) with no repo
  subgroups; a row's title is `<fleet> #<issue>` and its subtitle `role · host · now-line`, ellipsised.
- **Events:** a tab of its own now (it was System › activity, so the old shot is the System sheet): the screen's kind
  chips (`all`, merges, verdicts, lanes, asks) replace `all`/`important` and the fleet chips, one `last 24 h` section,
  and a row's age is its `value`; the old open row with its raw JSON is not in the screen.

Row sprites and lane avatars are the same theme symbols at the same sizes (24 px rows, 48 × 40 avatars), crisp
(the audit's smoothed-pixel check passes).
