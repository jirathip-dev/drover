# Hosts history — design mockup (#234, phase 1)

`index.html` is the Host tab with one HOSTS section: one card per host, main first, then the others A→Z. Each card's
load/cpu, swap-out, memory, disk and lanes rows carry a 24 h sparkline, and tapping a row opens its chart. The page
uses `assets/serve.css` unchanged (linked relatively) plus one `<style>` block that uses tokens only. That block moves
to `serve.css` in phase 2. SERVICES, FLEETS and ALERTS are the demo export's markup, unchanged.

The history is a fixture: 24 h × 1 row a minute × 3 hosts. The script at the end draws it with inline SVG and no
library, which is the shape of phase 2's `assets` JS. A tab ([1h] [6h] [24h]) redraws the chart, and a finger on the
plot moves the read-out.

| view | PNG | shows |
|---|---|---|
| `#` | `hosts-390.png` (full page, DPR 3), `hosts-1440.png` | the new order: HOSTS (mini · main, air, studio), FLEET DISK + leftovers, SERVICES, FLEETS, ALERTS |
| `#chart` | `chart-24h-390.png` (one phone screen), `chart-1440.png` | mini's load/cpu, 24 h: peak `14:52 · ▶ 3.71 · 9 lanes` |
| `#chart-1h` | `chart-1h-390.png` | the same row at 1 h (raw minutes) |
| `#mem` | `chart-mem-390.png` | mini's memory: free GB, the 2 GB floor, the pressure-warn band, lanes |
| `#off` | `host-off-390.png` | air turned off: grey, `off · draining 1 lane`, `[on]`, rows still live |

The fixture's story: mini ran 9 lanes from 13:30 to 15:40. During that window load/cpu went over 2.5, swap-out burst
past 2000, and pressure went to warn. Its disk fell from 19 to 12 GB free over the day (amber, floor 10). air is quiet.
studio stopped answering at 16:40: its card is grey, shows its last values with `· 2h ago`, and its sparklines end in
a dotted line.

## The tab

```
● HOSTS                                        ▾   ← one group; its header sums the worst rows when closed
┌ ● mini [main]  up · admits lanes        [off] ┐  ← card head: dot (worst row), name, `main`, state words, [off]/[on]
│ load/cpu   ▕██████░░░░░░▏ 1.20 / 2.5        ▸ │  ← #183's label | value | caret, unchanged
│            ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄/\/\┄┄┄┄┄┄┄┄┄┄  │  ← 24 h sparkline under the value, red past the limit
│ swap-out   … memory … disk … (each with one)    │
│ top memory 2.1G claude (lane …) · …           │  ← text rows: no sparkline
│ lanes      3 atlas-impl-2898 · …            ▸ │  ← purple step line
│ tags       os:macos arch:arm64 xcode fast     │
└───────────────────────────────────────────────┘
┌ ● air …  ┐ ┌ ● studio offline · last seen 2h ago ┐   ← A→Z after main; side by side at 1440 (#162's 56ch grid)
● FLEET DISK   fleet disk · [remove orphans] · leftovers [reap]
● SERVICES  ● FLEETS  ● ALERTS                         ← unchanged
```

- **Same rows on every card** (Q1): state (in the head), load/cpu, swap-out, memory, disk, top memory, lanes, tags.
  The only main-host difference is the `main` tag. The old MACHINE rows `fleet disk`, `remove orphans` and
  `leftovers` move to FLEET DISK, because they are fleet-wide, not per host. `spawns admitted/held` becomes the card
  head's words: `up · admits lanes`, or `held · load/cpu 3.10 > 2.5`, which wraps under the name at 390.
- **Memory is one row everywhere:** a gauge of free GB against `min_free_mem_gb` (┃ floor tick, as on disk), then the
  pressure word. Today main shows only the pressure word, and remote hosts only free GB.
- **Off / offline:** grey, so it never pages. An off host keeps its live rows, because it is still monitored. An
  offline host shows its last values with their age, and its history up to then.

## The sparkline and the chart

- **Sparkline:** 120 points (24 h, 12-minute buckets), 20 px tall, under the value. The value line is in the text
  colour, with its part past the limit in red (clipped at the limit, so a peak reads as red without a legend). The
  limit is dashed. Lanes are a purple step line. Memory adds a band along the floor (amber warn, red critical).
  Missing data (offline) is a dotted line. Y scales are fixed per metric (load 0–4, swap 0–4000, free space 0 to
  about the disk size), so two cards compare at a glance.
- **Chart (tap the row):** it opens inline under the row, full card width, and the row stays the 44 px tap target
  that closes it.
  - `[1h] [6h] [24h]` are #170's chip style: pressed means accent text plus an underline. 1 h is the raw minutes; 6 h
    and 24 h are 120 buckets.
  - The read-out sits on the tabs' line. It shows the window's worst point by default (`14:52 · ▶ 3.71 · 9 lanes`);
    a touch moves it. The time is the worst row's own minute, not the bucket's middle.
  - The plot is 3 × 44 px tall. The left axis shows 0, the limit (in the text colour) and the top; the right axis
    shows lanes 0/5/10 in purple. The x axis has clock times and `now`. A legend sits under it.
  - Axis labels are HTML at %-positions over a `preserveAspectRatio=none` SVG with non-scaling strokes, so text never
    stretches and every length is a token.

## Rules, measured on the mockup

Headless Chrome ran at 390 × 844 DPR 3 and at 1440 on every view above. It measured no sideways scroll, no box past
the right edge, text only 11 or 13 px, every button 28 px tall, and no text overflowing its box without an
ellipsis. Colours are the existing tokens (`--fg`, `--mute`, `--r`, `--a`, `--p`, `--l`). Rows and tabs keep #58's
44 px tap targets.

## Phase 2 notes (from reading the code)

- **Remote values are already sampled.** `hosts::survey` calls `host::sample(&on)`, which reads load/cpu,
  `kern.memorystatus_vm_pressure_level` and two `vm_stat`s (swap-out) over the host's SSH. Today it keeps only load,
  mem and disk in `hosts.json`. Saving `pressure` and `swapout` too gives every card the same rows with no extra SSH.
  On Linux, pressure and swap-out are `None` and show `–`.
- **Size bound:** a row with the issue's keys (`{"ts":…,"host":"mini","load_cpu":1.23,"pressure":0,"swap_out":1834,
  "mem_free_gb":9.4,"disk_free_gb":12.3,"lanes":3}`) is about 125 B, so 1,440 rows come to about 176 KB per host,
  not 100 KB. See Q4.

## Open questions for the owner

1. **Remote hosts' resolution.** `survey` runs in the 5-minute `check`. The fast step (every 60 s) only samples the
   main host. "No extra SSH" therefore means remote rows arrive every 5 min (288 a day), and their 1 h chart has 12
   points. **Recommend: as is (5 min for remote hosts, 1 min for main).** The other choice is a `survey` in each fast
   step: one SSH per host per minute.
2. **Top memory on remote hosts** needs one more SSH (`top -l 1` + `ps`, macOS only; `ps --sort=-rss` on Linux).
   **Recommend: read it in the 5-minute survey and show its age** (`· 4m ago`, as on air). The other choice is to keep
   it main-only and show "main only" in grey on the other cards.
3. **"Downsampling keeps the max" for free space.** For free memory and free disk, the bad peak is the minimum. The
   mockup keeps the **worst** row per bucket: the max of load, swap, lanes and pressure, the min of free GB.
   **Recommend: as mocked**, with the AC's test reading "keeps the worst per bucket (max; min for free space)".
4. **≈100 KB per host vs the spec's keys** (≈176 KB). (a) Keep the keys as specified and set the bound at ~180 KB per
   host (2 hosts ≈ 360 KB). (b) Use short keys (`{"ts","h","l","p","s","m","d","n"}`), ≈76 B a row, ≈107 KB per
   host. **Recommend (a):** readable rows, still tiny, and the AC test asserts the bound we pick.
5. **A fixed order instead of problem-first.** Today `serve.js groups()` moves red and amber groups to the top. Q1
   gives a fixed order, so the mockup keeps it fixed. Groups still open themselves when red or amber, and a closed
   one's header still sums its worst rows. Cards never reorder: an offline host stays in its A→Z place and is no
   longer moved last. **Recommend: as mocked.**
6. **Data path.** Sparklines ride in `/frag?id=host` as 120 numbers per row (5 rows × 3 hosts ≈ 9 KB). An open
   chart's 1 h / 6 h / 24 h series comes from `frag?id=hist&host=…&k=…` on tap, which only reads the small file
   (#160). **Recommend: as described**, not sending all three ranges on every refresh.
7. **Main host name with no `[hosts]`.** The card title is the `[hosts]` entry without `machine`. With no `[hosts]` at
   all, use `hostname -s`. **Recommend: `hostname -s`.**
8. **1440:** the cards sit side by side in #162's 56ch auto-fit grid (3 across). This is not in the issue; keep it?
