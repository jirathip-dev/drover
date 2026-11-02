# Events tab in the board's theme: design mockup (#170, phase 1)

`index.html` is the Events tab with time groups, coloured kind glyphs, fleet dots, plain words and filter chips.
It links `assets/serve.css` unchanged. One `<style>` block holds the new rules, which use existing tokens only and
move to `serve.css` in phase 2. A small inline script makes the chips work in the mockup. Open `index.html#all`
for the `[all]` view, and `index.html#fold` for the noise fold before and after.

Screenshots (headless Chrome, full page; 390 at DPR 3):

| file | what |
|---|---|
| `events-390.png`, `events-1440.png` | the default, `[important]`, with one row open (`#154 merged`) |
| `all-390.png`, `all-1440.png` | `[all]`: every kind, with routine rows muted |
| `fold-390.png` | the noise fold: today's line, then the proposed rows, on a real run from the log |

**Data:** the real event log, read on 2026-11-02 at 08:15 (+07). `[all]` shows the real last 50 events and
`[important]` the last 50 important ones, which reach back to yesterday 22:07, so every time group has real rows.
Fleet names are neutralised as in #162: `atlas` is the private app fleet and `relay` the paused one; `herdsman` is
this repo. Issue numbers, PR numbers, rounds, verdicts, codes, ages and the raw fields are real. Paths are `~`-masked,
and chat ids, the project id and migration file names are masked. **Illustrative:** the wording of atlas's two asks
(q52, q53) and their options is a paraphrase, because the real text describes the private app.

## The tab

```
[all] [important]  [● herdsman] [● atlas] [● relay]     ← chips; [important] pressed by default
50 important rows · back to yesterday 22:07 · all events, unfolded
NOW                                                  4  ← sticky group header, 11 px, count on the right
✓ ● #154  merged into staging · PR #161             3m  ← glyph · fleet dot · #N · plain words · age
        [#154] [PR #161] [checks]  08:11:02 · herdsman · merged     ← open: #169's links and the exact time
        ┌ { "ts": 1793581862, "type": "merged", … } ┐   ← the raw event, 11 px, wraps
✗ ● #160  failed · herdr: agent name cannot …       9m
```

### Rows

- **One row per event, 44 px tall.** The whole row is the tap target (`<details>`), except `#N`, which links to the
  issue. Columns: the glyph at 2ch, the fleet dot at 8 px, `#N` at 6ch, the words (wrapping to 2 lines at most, then an
  ellipsis), and the age at 11 px on the right (`age()` as today: `97s`, `9m`, `2h`).
- **Open,** the row shows `[#N] [PR #M] [checks]` as bracket links (#169: `checks` is `…/pull/M/checks`, shown
  only when the event knows its PR), the exact time, the fleet and the kind, then the raw event. In the raw
  event, a verdict `body` or `findings` field shows its first line plus `… (N more lines)`, and other long strings
  are cut at 160 characters.
- **The fleet dot** is #162's fleet colour (`.k-l` … `.k-a`, `--fk`). An event with no fleet has no dot.
- **In `[all]`,** routine rows (those `[important]` leaves out) put their words in `--mute`, while important rows
  keep `--fg`. So the two views share one weight rule: what matters reads brighter.

### Kinds: glyph, colour, words

The glyph takes its colour from the ANSI tokens that are already in `serve.css` (`.t-g` `.t-r` `.t-a` `.t-l` `.t-m`).
`imp` marks the kinds that `[important]` shows.

| kind | glyph | words (the row's text after `#N`) | imp |
|---|---|---|---|
| `merged` | ✓ green | `merged into staging · PR #161` | ● |
| `verdict` PASS | ✓ green | `review PASS · round 1` | ● |
| `verdict` FAIL | ✗ red | `review FAIL · round 1` | ● |
| `needs-human` | ! red | `needs you · 4 review rounds > limit 3` | ● |
| `needs-decision` | ! red | `asks you (q53) · <question>` (the `agent:` prefix dropped) | ● |
| `ask-waiting` | ! red | `still waiting on you · q53 for 35m` | ● |
| `notify-failed` | ✗ red | `alert failed · the merged alert timed out after 15s` | ● |
| `lane-dead` | ✗ red | `lane gone · impl lane … is gone` | ● |
| `blocked`, `driven` with a refusal code | ⊘ amber | #141's words: `waiting · no PASS for its head yet`, `waiting · CI not green yet`, `held · touches the database but wasn't queued --serial` | ● |
| `driven` `ROUND_LIMIT` / `WORKTREE_DIRTY` | ! red | `needs you · used its 4 review rounds` | ● |
| `driven` `ERROR`, `EXIT` n≠4 | ✗ red | `failed · herdr: agent target pane is not an available shell` (herdr's `message`, pane ids dropped) | ● |
| `driven` `EXIT` 4 | ▸ blue | `started · prompt not confirmed yet` (#141) | ● |
| `driven` capacity hold (`HOST_BUSY`, `SERIAL_BUSY`, `CAP`, `ALL_CAPPED`, `PAUSED`) | ⊘ amber | `held · the Mac is busy (load/cpu 3.09 over 2.5)`, `waiting · 3 database slots full` | Q1 |
| `driven` ok | · grey | `drive · started impl` / `started review` / `recorded the verdict` / `merged` | |
| `spawn` | ▸ blue | `impl started`, `review round 2 started`, `update round started`, `design started` | |
| `push-sent` | ↗ grey | `alert sent to 1 phone` | |
| `clean` | · grey | `lane cleaned (merged)` | |
| `stall`, `orphan-lane`, `conflict-predicted`, `heads-up`, `ready-unmerged`, `orch-*` | ! amber | `stalled · no event for 35m in fix(1)`, `conflict ahead with #149 · drive.rs` | Q3 |
| `decision` | ✓ grey | `you answered q53 · <choice>` | |
| `queue` | · grey | `queue · added #169`, `queue · moved #168 to the top` | |
| `lane-closed`, `supersede`, `override`, others | · grey (`override` ⊘ grey) | `lane closed · verdict recorded`, `replaced the live lane`, `forced past HOST_BUSY` | |

### Folds

Folds are counted in display order: newest first by `ts`, with ties kept in reverse log order.

- **The noise fold separates by kind.** A run of `clean` folds into one grey row, `clean ×4 · lanes cleaned`. A run of
  `push-sent` folds into `push-sent ×N`. A run of `notify-failed` folds into its own **red** row,
  `notify-failed ×N · alerts that never arrived`. A failure never shares a row with `clean`. A single event is just
  its row. An open fold lists what it holds, one 11 px line each.
- **`fold-390.png` shows why** on a real run (herdsman #42, 2026-10-31 10:53). The merge's alert timed out, and 4
  lanes were cleaned in the same second. Today the tab reads `5 × clean / notify-failed`. The proposed fold shows
  `clean ×4` plus a red `alert failed` row, which `[important]` also shows.
- **Same-pass holds fold:** one drive pass refusing several issues with the same capacity code becomes one row,
  `#2663 held ×4 · the Mac is busy (load/cpu 3.09 over 2.5)`. Opening it lists the other issues.
- **Twins:** a `blocked` event and the drive step's refusal for the same issue, code and `ts` show as one row.

### Time groups

| group | covers |
|---|---|
| `now` | under 5 minutes |
| `5m` | 5–59 minutes |
| `1h` | 60–119 minutes (ages still print in minutes up to 119m, so the header matches the column) |
| `earlier today` | 2 h ago back to local midnight |
| `yesterday` | the local calendar day before |
| `oct 31` … | older rows, by date (only `?all=1` reaches them) |

A group header is 11 px, uppercase, `--mute`, and on `--bg` with a bottom rule. It shows the name on the left and the
row count on the right. It is sticky under the page header, inside its own group, so the next group's header
replaces it as you scroll.

### Chips

- `[all] [important]` pick one of the two. The fleet chips (`[● herdsman] [● atlas] [● relay]`, one per configured
  fleet, in config order) narrow to the fleets that are pressed. When none is pressed, every fleet shows. Groups left
  empty by a fleet filter are hidden.
- They are the board's 28 px bracket controls with 44 px rings, 16 px apart, and they wrap at 390. Pressed is #141's
  pressed key: the `--l` fill with `--bg` text. A pressed fleet chip fills with its own `--fk` colour, and its dot
  turns `--bg`.
- Under the chips, one 11 px line: `50 important rows · back to yesterday 22:07 · all events, unfolded`. The link is
  today's `?all=1`.
- **Each filter shows its own last 50 rows** (a fold counts as one row). That is how `[important]` reaches back to
  yesterday. It does not filter today's 50 events, which would leave about 8 rows out of 15 minutes.
- Phase 2 keeps the choice per phone in `localStorage` (`events:filter` → `{"k":"imp","fl":["atlas"]}`), with
  try/catch, and falls back to `[important]` with every fleet.

## The rules (#63), measured on the mockup

At 390 and 1440, on `[important]`, `[all]` and `#fold`:

- `scrollWidth` equals the viewport, and no element passes either edge.
- The only text sizes are 13 and 11 px.
- Every chip and bracket link is 28 px tall, with the 44 px ring.
- Every row's summary is at least 44 px tall.
- Colours come only from the existing tokens. The raw panel uses `#161b22`, the surface `serve.css` already uses
  for code and blocks.

## Phase 2 notes

- **Words:** one `fn words(&Event) -> (glyph, tone, String)` in `serve.rs`. It reuses #141's refusal table, which is
  built for the Host tab's fleet rows, so the two tabs say the same thing. `plain()` stays as it is for notifiers and
  the CLI.
- **`events_tab`:** sort by `(ts, index)`, filter, fold, group, then render `details.er`. `NOISE` becomes the
  per-kind fold. The chips and `localStorage` are about 20 lines of `serve.js` (`assets/` does not count). The
  `src/` estimate is about +40 to +55, mostly the words match. If it runs over, the word strings can move into a
  `const` table.
- **#169 dependency:** the open row's `[#N] [PR #M] [checks]` should come from #169's shared link builder. If #170
  lands first, it shows `[#N] [PR #M]`, and `[checks]` follows #169.
- **Tests (from the brief):**
  - a golden test of glyph and tone per kind (the table above);
  - `[important]` drops `driven` ok, `spawn`, `clean`, `push-sent` and the capacity holds;
  - a run of `clean, notify-failed, clean` never produces a row that holds both kinds, and the failure row is red.
- **Gate 2:** a 390 px fixture with all 5 groups, one open row, a `clean ×N` fold and a red `notify-failed` row.
  `ui-audit` can assert that `.er .kg.t-r` exists and that no `.er` row contains both `clean` and `notify-failed`.

## Open questions for the owner

1. **Capacity holds out of `[important]`?** The brief lists refusals as important, but `HOST_BUSY` and
   `SERIAL_BUSY` are herdsman waiting for room, not something going wrong. Counting them, 14 of the last 50
   important rows were `HOST_BUSY`, and the view reached back only 2.5 h. The Host tab already shows the hold.
   **Recommend: leave the 5 capacity codes out** (as mocked); they stay in `[all]`, folded per pass. `DISK_LOW` stays
   in, because it needs `clean`.
2. **50 rows per filter** (as mocked) rather than filtering the last 50 events. **Recommend: as mocked.**
3. **Lane warnings** (`stall`, `conflict-predicted`, `orphan-lane`, `ready-unmerged`, `orch-stuck`): they are amber `!`
   and left out of `[important]` (65 stalls since yesterday). `stall` also sends a push, so a phone that opens Events
   from that alert would not find it under `[important]`. **Recommend: leave them out.** The alert itself names
   the issue. The alternative is to include `stall` and `ready-unmerged` only.
4. **Group bounds:** `1h` = 60–119 min and `earlier today` from 2 h. The brief names the groups but not their edges.
5. **Repeats across passes:** `#150 waiting · no PASS for its head yet` shows twice, 2 minutes apart (two passes).
   **Recommend: leave it.** Each is a real attempt. Folding by issue would hide that the drive retried.
6. **Fleet colours:** the mockup uses #162's neutral names (atlas amber, relay amber). The real names hash
   differently: `synergy` gives purple and `sendmeter` gives green, so on the real board no fleet dot shares the
   amber of `⊘`.
7. **`driven` ok rows in `[all]`** repeat the spawn, verdict or merge of the same pass (`drive · started impl` next to
   `impl started`). **Recommend: keep them in `[all]`.** It is the "everything" view, and they are muted. Dropping
   them would roughly halve `[all]`.

Observed while gathering data, outside this issue: `GET /` and `/?all=1` on the live board (127.0.0.1:7777) did not
answer within 60 s at 08:15, while `/frag?id=events` answered at once.
