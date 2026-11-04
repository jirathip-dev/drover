# Chat: harness pick at thread start, model/effort chip in the composer (#404, design)

The conductor still answers every thread, and the owner can choose two things about it:

- **Harness:** picked in a new thread, then fixed for that thread.
- **Model and effort:** changeable at any time, from the next turn.

Both live in the composer, as t3code does it. t3code's trigger is the model name followed by the effort word, muted.
It opens a model list beside an effort column headed by that model's name. The board doesn't use a system menu, so
this design reuses its own sheet picker: the look sheet's box and rows from #290. On a phone the lists stack; from
720 px they sit side by side, as t3code's do.

## The shots (all from the real board)

`shoot.mjs` takes the demo export and makes two copies. `today` is untouched. `next` is built the way phase 2 will
be: `serve.js` gains `run.js` and two one-line hooks (`serve.js.diff`), and `serve.css` gains `run.css`
(`serve.css.diff`). `mock.js` stands in for the HTML `chat.rs` will render, because the demo's events predate #404:
the `#runs` JSON, each composer's run attributes, the headers and the server side of `op=model`. The script shoots
each state at 390 × 844 (DPR 3) and at 1440 × 900, then measures each shot (`report.json`).

| file | what |
|---|---|
| `today-new-390.png` | Today: [+ thread]'s empty thread for acme-api |
| `new-390.png` | Proposed: the same thread. Under the unchanged `[+] box [send]` line, `[claude ▾] [opus · high ▾]`. The header reads `acme-api · claude · opus` |
| `new-sheet-390.png` | Either chip tapped: HARNESS (each with its default run, the default marked), MODEL and EFFORT, the current rows ticked |
| `new-hermes-sheet-390.png` | **hermes** tapped: the model and effort lists switch to hermes's, at its defaults |
| `new-hermes-390.png` | The sheet closed: `[hermes ▾] [kimi-k2.5 · high ▾]`, and the header follows |
| `new-dsh-390.png` | **dsh** picked: dsh takes no model or effort today, so there is no second chip, only the 11 px line `dsh sets its own model and effort` |
| `thread-390.png` | A started thread (t4, claude): only `[opus · high ▾]`. The harness is fixed, so it has no chip; it shows in the header |
| `model-390.png` | The chip tapped: MODEL and EFFORT only, the sheet titled with the thread |
| `model-picked-390.png` | **sonnet**, then **xhigh**: each tap applies at once, and the thread shows **one** line, `model opus · high → sonnet · xhigh · from the next turn · you` |
| `changed-390.png` | The sheet closed: chip `[sonnet · xhigh ▾]`, header `acme-api · claude · sonnet · awake` |
| `next-turn-390.png` | The next turn as `chat.rs` will render it: the owner's message, then `↻ resumed · same session (7f3c9…) · sonnet · xhigh`, then the reply |
| `hermes-390.png` | A thread started on hermes (t2): header `acme-api · hermes · kimi-k2.5 · asleep`, chip `[kimi-k2.5 · medium ▾]` |
| `guest-390.png` | An unpaired device: the chip is inert (disabled), like the rest of the composer |
| `new-sheet-1440.png`, `model-1440.png` | 1440: the lists side by side (three for a new thread), the sheet at half height |

Regenerate with:

```sh
HERDSMAN_HOME=demo FLEET_CONFIG=demo/fleet.toml FLEET_STATE=demo/events.jsonl \
  ./target/release/herdsman serve --export <site> --demo
node docs/design/chat-model-404/shoot.mjs <site> <chrome-headless-shell>
```

## The design

### The chips (`run.css`, `run.js`)

- **Where they sit:** one 44 px row under the composer's line. That line (`[+]`, box, `[send]`) is unchanged
  (#403/#405), and the attachments row stays above it. This is t3code's place for them: controls under the input.
- **New thread:** `[claude ▾] [opus · high ▾]`.
- **Started thread:** `[opus · high ▾]` only.
- **Chip style:** each chip is a bracket control (28 px, 44 px ring). The effort after `·` is muted, as t3code's
  trigger shows it. Effort `none` drops the suffix: `[opus ▾]`.
- **Nothing to pick:** when a harness offers one model or fewer and one effort or fewer (dsh today), its run shows
  as an 11 px muted line instead of a chip.
- **Unpaired device:** the chips are disabled, like the box (`guest-390.png`).
- **Header:** the thread header reads `repo · harness · model · state`, as today, but from the **thread's** run, not
  the fleet's conductor role. A new thread's header follows its picks. With no model (dsh) it reads
  `acme-api · dsh · awake`.

### The picker sheet

- **The sheet:** the board's own sheet (`sh lg rpk`), with the look sheet's rows (`.lko`: 44 px, the current row in
  the accent with a ✓, `(default)` in 11 px).
- **Groups:**
  - HARNESS: a new thread only. Each row shows that harness's default run; the hint reads `fixed once the thread
    starts`.
  - MODEL: hint is the harness.
  - EFFORT: hint is the model, as t3code heads its effort column with the model's name.
- **Keyboard:** each group is a `role=listbox`, with the look sheet's keys (↑/↓/Home/End, Tab, Enter/Space).
- **Applying a pick:** a tap applies at once and the sheet stays open, as the look sheet does.
  - In a new thread, the pick waits on the page (`RUNP`, repainted after a live swap, like ATT's thumbnails). The
    first message carries it: `op=new … harness, model, effort`.
  - A new harness resets the model and effort to that harness's defaults.
  - In a started thread, a pick POSTs `op=model` and is logged.
- **Footnote:** `from the next turn: the conductor resumes its session on it · logged in this thread` (in a new
  thread: `for this thread; your first message starts it on this run`).
- **1440:** the groups sit side by side, 320 px each, with the sheet at half height.

### What gets logged

- **`thread-new`** gains `harness`, `model` and `effort`: the run the thread starts on. The web logs it at the first
  message; `herdsman thread new --harness <h>` logs it at creation (model and effort are that harness's defaults). A
  thread with no `harness` (every thread before #404) runs on the defaults, and its header shows them.
- **`thread-model`** `{thread, model, effort, by}` is new: the owner's change. `POST /thread op=model` validates it
  against the thread's harness choices (else `BAD_VALUE`, as `conductor::set` does). The harness never changes.
- **Rendering:**
  - A `thread-model` shows as a `.qs` line: `model opus · high → sonnet · xhigh · from the next turn · you`.
  - Changes with no turn between them render as **one** line: the first change's "from" and the last one's "to".
    So two taps make one line, while each tap is still logged.
  - `conductor-start` gains `effort`. The `↻ resumed` line adds `· model · effort` when they differ from the
    previous start.
- **The thread's run:** the last `thread-model`, else `thread-new`'s fields, else the defaults. `conductor::start`
  starts or resumes on it, in place of `role.model` and `role.effort_or`. Claude takes `--resume <session> --model …
  --effort …`. The harness is the thread's, so the "only a session this harness started resumes" rule still holds.
- **When a change takes effect:** at the next message after it.
  - Conductor asleep: that message resumes it on the new run.
  - Conductor awake and idle: herdsman closes its pane, without the asleep line, and resumes the session on the new
    run with that message.
  - Conductor mid-turn: see Q4.

### Config: `[chat]`

```toml
[chat]                          # #404: the run new threads start on; each key unset falls back to the
harness = "claude"              # fleet's role.conductor, then its impl role (#401), as today
model = "opus"
effort = "high"
accounts = ["A", "B"]

[chat.models]                   # what the chip offers per harness; the first one is that harness's default model
claude = ["opus", "sonnet"]     # absent: the top-level `models`
hermes = ["kimi-k2.5", "gpt-5.6-luna"]   # Q1
```

- **The default harness** takes `[chat]`'s `model` and `effort`.
- **Any other harness** takes the first entry of its `[chat.models]` list. Its effort is `[chat].effort` if that
  harness supports it (`harness::efforts`), else `[limits].effort`, else `none`.
- **Efforts offered:** `harness::efforts(h)`.
- **Parse errors:** the parse refuses an unknown harness, an unknown effort, or a model outside the harness's list,
  with the same messages as the role checks.
- **The conductor's instructions** still reach each harness through its own channel (Claude's appended system
  prompt, the Hermes profile brief, dsh's brief), unchanged. The AC's three-harness test pins that.

## Gate 2, measured on this mockup (`report.json`)

- **The chips:** every shot measures 0 outliers. Each chip is 28 px with a 44 px ring and stays one line inside the
  screen, the dsh line ellipsises if it must, and text is 11 or 13 px. The composer is 105 px tall (61 px today),
  and nothing scrolls sideways.
- **The sheet:** every row is ≥ 44 px, text is 11/13 px, nothing passes the right edge, and each listbox has exactly
  one ticked row (`report.json` lists them).
- **One line per burst:** after sonnet then xhigh, the thread's last line is
  `model opus · high → sonnet · xhigh · from the next turn · you`, one line, and the header and chip follow.
- **The first message carries the run:** `new-dsh-390`'s `posts` is what `chatSay` would send:
  `{"op":"new","repo":"acme-api","harness":"dsh","model":"","effort":"none"}`.
- **The board's own audit** (`ui-audit.js`, #63 gate 2) with the chips in place shows **0 outliers** on `chat`,
  `chat-thread`, `chat-new`, `chat-send`, `chat-long`, `chat-kb`, `chat-thumbs`, `chat-back` and `chat-code` at
  390, and on `chat-thread` and `chat-new` at 1440. Those views already check the composer: #403's keyboard and
  long-text views and #405's thumbnails. The added row breaks none of them.
- **Page errors:** none.

## What phase 2 changes (estimate)

- **`assets/serve.js`:** `run.js` (about 100 lines) plus the two hooks in `serve.js.diff`. **`assets/serve.css`:**
  `run.css`, about 10 rules, tokens only.
- **`crates/core`:**
  - `config.rs`: `[chat]` and `[chat.models]`, with checks. About 35 lines.
  - `conductor.rs`: the thread's run (the last `thread-model`, else `thread-new`, else defaults) feeds `start`, and
    an awake conductor restarts on a change. About 35 lines.
  - `cli.rs`: `thread new --harness`. About 8 lines.
- **`crates/server/src/serve/chat.rs`:**
  - `#runs`, the composer's `data-fleet/h/m/e`, the header from the thread's run, the `thread-model` line with
    coalescing, `op=model` and `op=new`'s run. About 55 lines.
- **Total:** `src/` net about +135 against the issue's +150. The `ui-audit.js` views and tests come on top.
- **`ui-audit.js`** gains 3 views, so CI shoots them into `ui-390/`:
  - `chat-run-new`: a new thread's chips, the sheet open.
  - `chat-run-model`: a started thread's chip and its sheet, with one `.qs` line after two picks.
  - `chat-run-dsh`: the line in place of the chip.
- **Docs:** `docs/conductor.md` (harness, model and effort per thread) and `docs/config.md` (`[chat]`).

## Open questions for the owner

1. **Hermes models.** herdsman passes no `--model` to Hermes today: the Hermes profile owns its model (`hermes.rs`
   `start_cmd` passes `-p <profile>` and `--reasoning` only). A hermes thread can therefore switch effort but not
   model, unless the Hermes harness gains `--model <m>` and `[chat.models].hermes` lists the choices. The mockup
   shows that case with illustrative names. **Recommend:** add `--model` if the Hermes CLI takes it (a lane checks).
   Otherwise hermes threads show the profile's model in MODEL as a single row ("set by the hermes profile"), and
   only effort changes.
2. **dsh.** dsh takes neither a model nor an effort flag (`dsh.rs`: `dsh-tui --patch`; `harness::efforts("dsh")` is
   `none`). A dsh thread therefore gets the line, not a chip, and decision 3 can't apply to it until dsh takes
   them. **Recommend:** accept that for #404.
3. **Composer height.** The chip row adds 44 px (the composer grows from 61 to 105 px), also while the keyboard is
   up. The alternative is putting the chip at the right end of the `.tlv` status line above the composer: no
   extra height, but the status line ellipsises sooner, and the chip sits apart from the box. **Recommend: the row**
   (t3code's place; a phone at 390 × 844 with the keyboard up still shows about 6 lines of the thread).
4. **A change while the conductor is mid-turn.** Interrupting the turn is wrong.
   - (a) Recommended: the next message goes in as today, into the running session on the old run, and the change
     applies at the first message after that turn ends. The line still reads `from the next turn`.
   - (b) Hold the message (`sending…`) until the turn ends, then resume on the new run with it. This needs holding
     code the chat doesn't have.
5. **A CLI for the change.** The issue asks for `thread new --harness` only. `herdsman thread model <ID> --model M
   --effort E` would let a conductor thread or a script change a run (about 15 lines). **Recommend: not in #404.**
6. **Which harnesses the sheet lists.** The mockup lists all three (`harness::NAMES`). A harness not installed on
   the main host fails at its first message, with #397's start-error line and nothing else. Listing only the
   harnesses `doctor` passes would need a probe per sheet open. **Recommend: all three.**
