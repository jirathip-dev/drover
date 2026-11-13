# The whole board in its theme (#370, phase 1: design)

Farm and kitchen now carry their theme on every tab. Each label pairs the theme word with the plain word,
`on the line · working`, with the plain part muted in `--fs-meta`. Every row sprite loops two frames. A merge, a FAIL
or a new ask plays on its own row. Empty tabs, the loading overlay, the Host load and push titles use the theme's
words. Terminal stays exactly as it is: shoot.mjs compares every terminal fixture with today's export, pixel for pixel.

Everything here is shot from the real board: the `serve --export --demo` page with the proposed theme files and
`proposal.js`/`proposal.css` loaded after serve.js and herd.js. The data is the synthetic demo.

## Files

| file | what |
|---|---|
| `shots/<theme>-<tab>-<width>.png` | **the fixtures:** farm, kitchen and terminal × Queue, Lanes, Host, Events × 390 (DPR 2) and 1440 (24 PNGs) |
| `sheet.html`, `sheet-390.png`, `sheet-1440.png` | the parts: the words table, every row sprite's two frames, the Host minis, the moments, empty tabs, loading, push samples |
| `shots/parts/` | the crops the sheet shows: moments (live and Reduce Motion), empty tabs, loading, a loop's two captures |
| `frames.mjs` | frame 2 of every row sprite and of the load minis; frame 1 comes from today's sources (`../kitchen/art.mjs`, `../farm-v2/make_sprites.py`) |
| `build.mjs` | writes `proposed/{farm,kitchen}/theme.json` and `sprites.svg` (today's files + this design); phase 2 copies them to `assets/themes/` |
| `proposal.js`, `proposal.css` | what phase 2 adds to serve.js / serve.css (and two lines to herd.js), as a script the mockup loads |
| `shoot.mjs`, `cdp.mjs` | the fixtures and the gate-2 measurements (`report.json`); no dependencies, Chrome over `--remote-debugging-pipe` |
| `report.json` | the measurements below |

To regenerate from the repo root:

```sh
DROVER_HOME=demo FLEET_CONFIG=demo/fleet.toml FLEET_STATE=demo/events.jsonl drover serve --export /tmp/site --demo
cd docs/design/theme-370 && node build.mjs && node shoot.mjs /tmp/site <chrome-headless-shell>
```

To view the sheet, serve the repo root (`python3 -m http.server`) and open `docs/design/theme-370/sheet.html`. The loops
run live; `?still=1` or Reduce Motion shows frame 1.

## 1. The words (`theme.json` `words`)

Words are data. The design extends the existing `words` key (#241), so a new theme only adds a file. Kitchen already
had the group words. A word the theme lacks shows the plain word alone, with no `·`. Terminal has none.

| key | where | farm | kitchen | plain |
|---|---|---|---|---|
| `needs` | Lanes group (today `blocked`), Queue needs-you rows, needs-you cards, Events asks/needs, push | at the gate | the bell | needs you |
| `working` | Lanes group | on the trail | on the line | working |
| `idle` | Lanes group | grazing | standing by | idle |
| `done` | Lanes group, Queue `done today: N` | in the stable | at the pass | done |
| `queued` | Queue rows | in the paddock | tickets | queued |
| `parked` | Lanes group, Queue rows | turned out | off the clock | parked |
| `dead` | Lanes group, Queue rows | lame | 86'd | dead |
| `impl` `fix` `rev` `update` | Queue rows, Events (`impl started`, `review PASS`) | plough, farrier, vet, haul | cook, chop, taste, carry | build, fix, review, catch up |
| `merged` | Events, the merged moment, push | over the fence | order up | merged |
| `fail` | Events FAIL rows, the FAIL moment | refused | sent back | FAIL |
| `tab-queue` … `tab-events` | each tab's first line | the paddock, the herd, barn, logbook | tickets, the line, the kitchen, the rail | the tab's name |
| `empty-queue` `empty-lanes` `empty-events` | an empty tab, over the server's plain line | the paddock is empty · no horses out · the logbook's blank | the kitchen's quiet: no tickets · nobody on the line · the rail's clear | no open work · no agents · no events |
| `loading` | under the loading overlay's sprite (#337's cook / trot, unchanged) | saddling up | firing up | loading |
| `load_scene.levels` (exists) | the Host card's load | clear, windy, storm | simmer, boil, flare-up | calm, busy, held |

How the pairs look on each tab (`shots/`):

- **Queue:** the tab line `tickets · queue`. Each row's state is `cook · build`, `taste · review(2)`,
  `the bell · waiting: decision` or `tickets · queued`. `at the pass · done today: 2` closes each fleet. The needs-you
  cards read `the bell · needs you`. The fleet chip `! 2 need you` stays plain, since it is a count, not a label.
- **Lanes:** each group reads `on the line · working (4)`. Lane rows keep their plain state on the right; the group
  header already pairs it.
- **Host:** the tab line `the kitchen · host`. Each host card's header gets its load mini and word, `simmer · calm`.
- **Events:** the tab line `the rail · events`. The kind's theme word comes first, in the row's tone:
  `order up merged into staging · PR #14`, `sent back review FAIL · round 3`, `the bell asks you (q4) · …`. The
  row's own words stay as they are.
- **The tab bar stays plain** (`[queue·4] [lanes] [host] [events]`): four themed pairs don't fit at 390, and
  navigation keeps its words (Q1).

The farm's `words` now also name the Herd's bands, `on the trail` and so on. That was already true for the kitchen.

## 2. Row sprites: two-frame loops

Every row sprite has two frames: farm's 7 actions plus `row-bale`, `row-bale-red`, `row-jump`, `row-refuse` (drawn in
farm-v2, now shipped), and kitchen's 7 plus `row-ticket` and `row-back`. Frame 1 is today's art, unchanged. Frame 2:

| farm | frame 2 | kitchen | frame 2 |
|---|---|---|---|
| plough | the legs gather, the furrow runs on | cook | the wok tossed up, the food in the air, the flames flicker |
| farrier | the forge flares, a spark at the hoof | chop | the knife comes down, the pieces jump |
| vet | the vet leans in, the arm lower | taste | the spoon dips into the bowl, steam |
| haul | the walk: the legs gather | carry | a walking bob, a pixel up |
| graze | the tail swishes, a bite of grass | wait | the eyes glance to the pass |
| stable | the legs gather, the tail lifts | serve | the plate lifted, a glint |
| rear | the front legs paddle, the neigh moves | bell | the hand drops, the bell rings |
| bale | straw lifts off | ticket | the ticket swings on its clip |
| red bale | bobs a pixel | back (FAIL) | the plate slides back under the dropping ✗ |
| jump | higher over the fence | | |
| refuse | the head tossed back, dust | | |

- The frames are the `.kf1`/`.kf2` groups herd.css already swaps by `--o1`/`--o2`, 0.6 s a frame. The row's `<svg>`
  runs `kfr` and the `<use>` inherits it. **No node is replaced:** the loop is CSS, and a kind change sets the
  `<use href>`.
- Loops run on the visible tab only: a hidden tab is `display:none`, and `html.bg` pauses the rest (#337/#351).
- **Reduce Motion:** frame 1, still.
- The ascii style's row figures (#335) stay as they are.
- **Queue and Events pick a sprite by kind** (`row_sprites` gains `queued`, `needs`, `merged`, `fail`):
  - Queue: queued → bale / ticket, needs you → red bale / bell;
  - Events: merged → jump / serve, FAIL → refuse / back, asks and needs → red bale / bell, PASS → vet / taste.

## 3. Moments on the row

`/live`'s `ev` already carries the moment kind (`k`) once per seq. The issue's row on the visible tab plays it for
2.4 s, once:

| kind | the row | tag (theme word) |
|---|---|---|
| merged | the sprite becomes `row_sprites.merged` (the horse over the fence / the plate) and hops 3× | `over the fence!` / `order up!` |
| fail | the whole row shakes | `refused` / `sent back` |
| needs-you | the sprite pulses, the row flashes red (18 %, easing out) | `at the gate` / `the bell` |

- The tag sits where the row's age is, for the 2.4 s.
- Under Reduce Motion the moment is still: the moment's sprite and the tag, with no hop, shake, pulse or fade.
- **Once per event:** a second call with the same seq is ignored.
- **A `/live` patch mid-moment doesn't restart it:** `frag()`'s morph drops the marks and the moment re-applies them
  with a negative delay. It resumes where it was.
- With no row in view (the Host tab, or a row scrolled off), today's `#mo` overlay plays as before (Q2).
- On Events the target is the newest row of that issue whose kind matches the moment, which is the event's own row.

## 4. Empty states and loading

An empty tab shows the theme line over the server's plain line:
- `the kitchen's quiet: no tickets` over `no open work`;
- `the paddock is empty` over `no open work`;
- `no horses out` over `no agents`;
- `the rail's clear` over `no events`.

The Events tab has no empty line today (it says `0 important rows`). Phase 2 adds `no events` in serve.js when the
list is empty.

The loading overlay keeps #337's sprite (already the cook / the trot) and adds the word under it:
`firing up · loading`.

## 5. Host and Events

- **Host:** each host card's header shows its load as the theme's `load_scene`, in a 48 × 16 mini drawn at 1.5×:
  - stove flames (three burners) for the kitchen, weather for the farm;
  - two frames each, so flames flicker and clouds drift;
  - beside it the level word, `simmer · calm`, `windy · busy`, `storm · held`;
  - the level is load/limit: under 0.5 calm, from 0.5 busy, held at or past the limit (phase 2: held = the spawn
    gate refuses, as in the Herd).

  These are new sprites, `load-calm|busy|held`, from the kitchen and farm-v2 minis (new key `load_scene.minis`). The
  16 px group-header minis (#241) stay as they are.
- **Events** reads as the rail or the logbook: the tab line, the kind's theme word on each row, and the sprites by kind.

## 6. Push (`sheet.html` §7)

| event | farm | kitchen | terminal (today's, unchanged) |
|---|---|---|---|
| merged | `Over the fence: #2957 merged` | `Order up: #2957 merged` | `drover` |
| needs-human | `At the gate: api #2961 needs you` | `The bell: api #2961 needs you` | `#2961 needs you: <why>` |
| needs-decision (owner ask) | `At the gate: api #2961 needs you` | `The bell: api #2961 needs you` | `drover` |

Only the title changes. The body stays today's (`plain(ev)`; for needs-human, the why and the first finding).

Each phone keeps its own theme in localStorage, so the server can't know it. Instead, the phone sends its two push
words when it turns alerts on, and again when it switches theme (subscribe already upserts by endpoint):

```
POST push/subscribe  endpoint, p256dh, auth, w_merged=Order up, w_needs=The bell     (none for terminal)
```

A sketch of the `src/` side (phase 2, not compiled; about +9 lines, budget +10):

```rust
// webpush.rs, Sub: the words this device's theme puts in a title (none: today's titles)
#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
pub words: BTreeMap<String, String>,
// subscribe():
words: ["merged", "needs"].into_iter().filter_map(|k| Some((k.into(), f.get(&format!("w_{k}"))?.clone()))).collect(),
// send(): the title per device
let themed = |s: &Sub| match (ev.kind.as_str(), ev.issue) {
    ("merged", Some(n)) => s.words.get("merged").map(|w| format!("{w}: #{n} merged")),
    ("needs-human" | "needs-decision", Some(n)) => s.words.get("needs").map(|w| format!("{w}: {} #{n} needs you", ev.fleet)),
    _ => None,
};
// …and the payload is built inside the loop: themed(s).unwrap_or_else(|| title.clone())
```

The tests phase 2 needs:
- `theme.json` words load with the fallback to plain: a word missing from the theme renders the plain word alone, and
  terminal renders today's labels;
- push uses the device's theme: two subs, one with `words` and one without, give two titles for one event;
- the farm and kitchen theme files still pass `crates/cli/tests/themes.rs`.

## Gate 2, measured (`report.json`)

| AC | result |
|---|---|
| 390 and 1440 fixtures, farm / kitchen / terminal × 4 tabs | 24 in `shots/` |
| every label uses theme word + plain word, terminal unchanged | the farm and kitchen labels are listed in `report.json` `labels`; all 8 terminal fixtures are **identical to today's export, pixel for pixel** |
| row sprites change frame between two captures without node replacement | farm and kitchen, Queue and Events: `--o1` flips within 0.3 s, the pixels change, and the sprite is the **same node across a `/live` `frag()` patch** between the captures |
| static under Reduce Motion | the same captures 1.3 s apart: frame 1, no pixel change, same node; moments still |
| moments play once per event | a second call with the same seq returns `again`. Patched mid-moment at ~1.15 s, each moment resumed 1.0–1.2 s in (no restart) and ended by 2.4 s, for all 3 kinds × 2 themes |
| contrast ≥ 4.5:1 for the muted plain word | measured on what is really under each visible `.pw`: farm **7.12** (on the bg), **7.35** (needs-you cards); kitchen **6.44**, **6.66** |

## Phase 2: what changes

- **Assets:**
  - `assets/themes/{farm,kitchen}/` = `proposed/`, regenerated by `build.mjs`: the words, the 2-frame rows, the 4 farm
    event rows, the minis, `row_sprites` event keys, `load_scene.minis`;
  - serve.js = `proposal.js` folded into `rows()`/`frag()`/`moment()`;
  - serve.css = `proposal.css`;
  - herd.js: two lines in `theme()` for the `hr-<kind>` and `hl-<level>` symbols;
  - docs/themes.md: the contract rows for `words` (the keys above), `row_sprites` kinds and `load_scene.minis`.
- **`src/`:**
  - push: about +9 (above);
  - `/live`'s `ev` gains `"f": e.fleet, "n": e.issue` on the existing `json!` line (+0);
  - **`validate()` must accept the four row kinds** (+1). `row_sprites` keys are limited to the 7 actions today, so the
    proposed theme.json would be refused as is and the theme would fall back to farm. The mockup didn't hit this: it
    swaps in the static files. `load_scene.minis` and the new `words` pass as they are;
  - Q3 covers the Events kind (+1).

## Open questions for the owner

1. **Tab bar plain, theme line in the tab.** At 390 the four tabs can't carry `the rail · events`, so each tab opens
   with its own line (`the rail · events`) and the bar keeps `[queue·4] [lanes] [host] [events]`. **Recommend:** as
   drawn. The alternative themes the bar at ≥ 900 px only.
2. **Moments on a row not in view.** The row plays it when it's on the visible tab. Otherwise today's top-right `#mo`
   overlay plays. **Recommend:** keep the overlay as that fallback. The alternative is no moment at all off-row.
3. **`src/` beyond the push budget.** The issue allows ≤ +10 `src/` lines, all for push wording. Two more lines are needed:
   - `validate()` accepting `row_sprites.queued|needs|merged|fail` (+1). Without it the event sprites can't ship;
   - a `data-k={kind}` on each Events row (+1). Without it serve.js reads the kind from the row's text
     (`review FAIL`, `merged into`), which breaks if that wording changes.

   **Recommend:** allow both (+2, total about +11).
4. **Words the issue didn't name:** `parked` (turned out / off the clock), `dead` (lame / 86'd), the Lanes tab
   (the herd / the line), the Queue tab (the paddock / tickets), the loading word (saddling up / firing up) and the
   Events and Lanes empty lines. **Recommend:** as drawn; each is one string in theme.json.
5. **Lanes `blocked` becomes `needs you`.** The issue's group is `the bell · needs you`, so today's `blocked (1)` header
   reads `the bell · needs you (1)` on themed boards. Terminal keeps `blocked`. **Recommend:** as drawn.

`src/` net +0: this is a design lane (docs only).
