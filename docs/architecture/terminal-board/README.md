# Terminal board: the app's second tab, drawn from the phone's screens (#1354)

## Summary

Decision: the Board tab is a second renderer of the phone's screens. The app builds each screen in its own process
with the code the phone page uses and draws the blocks as text; an action runs the phone's handler, with its
permissions and refusals. It reads again when the log or the snapshot changes, so it is live with serve stopped.
Slices: 1 the in-process door to the screens and actions; 2 the Board tab, read-only and live; 3 actions, forms, docs.
Open questions: (1) Threads stays as built, so the phone's thread extras stay off the terminal (my pick), or a
follow-up moves it onto the blocks? (2) a terminal write is logged like a phone write (my pick), or as `term`?

Status: proposed (architect lane, 2026-11-19). Read on staging `babffa58b`, with #1352 merged.

Read this page first (the callers), then [adr.md](adr.md), [shapes.md](shapes.md), [boundaries.md](boundaries.md).

Three words. A **screen** is the JSON tree of blocks `crates/server/src/screens.rs` builds. The **door** is the new
in-process way to it (`serve::Local`). A **hot** is one thing the cursor can stand on: a row, a button, a field.

## The callers, sketched first

### The owner

```
drover board              the app on its Board tab (stdout a terminal, not a lane)
drover board herdsman     the same, on that fleet
drover board | cat        today's table, unchanged (a pipe, a lane, a test)
drover chat [t12]         the app on its Threads tab, as today
Tab                       the other tab; each keeps its place
```

### The binary and the CLI arm

```rust
drover::APP.set(drover_chat::main).ok();

Cmd::Board { fleet } => {
    let app = APP.get().filter(|_| !in_lane());
    if !app.map_or(Ok(false), |app| app(ctx, r, &Open::Board(fleet.clone())))? {
        table(ctx, r, out, fleet)?;
    }
}
Cmd::Chat { thread } => {
    if !(APP.get().context("no app")?)(ctx, r, &Open::Threads(thread))? {
        bail!("drover chat needs a terminal");
    }
}
```

The app answers `false` when stdout is not a terminal and draws nothing. A test never sets `APP`, so it gets the table.

### The app: one frame, two tabs

```rust
pub fn main(ctx: &Ctx, r: &dyn Runner, open: &Open) -> Result<bool> {
    if !std::io::stdout().is_terminal() {
        return Ok(false);
    }
    let door = Local::new(ctx, r);
    let mut term = ratatui::init();
    std::thread::scope(|s| {
        let res = run(&mut App::new(s, ctx, r, &door, open), &mut term);
        ratatui::restore();
        res.map(|()| true)
    })
}

match self.tab {
    Tab::Threads => self.threads.key(k),
    Tab::Board => self.board.key(k),
}
```

`App` keeps the loop, the top line (tabs, what is down) and the health check. `threads` is today's chat, moved, not
changed. `board` is new.

### The feed: how the board stays live with serve stopped

```rust
loop {
    door.refresh(clock::now());
    let stamp = door.stamp();
    if stamp != seen || kicked || watch != watched || built.elapsed() >= REBUILD {
        for path in &watch {
            let tree = door.screen(clock::now(), path);
            if last.get(path) != Some(&tree) {
                tx.send(Note::Tree(path.clone(), tree.clone())).ok();
            }
        }
    }
    match rx.recv_timeout(TICK) { Ok(Ask::Watch(w)) => watch = w, Ok(Ask::Kick) => kicked = true, .. }
}
```

`stamp` is the log's last seq and the snapshot's stamp. `watch` is the paths of the views on screen. This is the
loop `/screens/<x>/stream` runs for one phone, without the socket and without the ops: the app swaps the whole tree
and keeps its cursor by block id. ratatui then writes only the cells that changed.

### Drawing: one pure function per screen

```rust
let laid = lay(&view.tree, width, &view.ui, now);
let sel = laid.hots.iter().find(|h| Some(&h.id) == view.ui.sel.as_ref());
put(f, area, window(&laid.lines, view.ui.top, area.height));
```

`lay` reads the tree by field name. It knows the block types of today and draws any other from its text fields.
It names no screen and no action, so a screen or a button added later needs no change here.

### A key press: the queue reorder

```
› #1354 board: the terminal board      #3      the cursor is on a queued row; its swipe actions are its keys
  1 ▲   2 ▼   3 Top   4 Drop
press 1 →  ┌ ▲ ─────────────────────────┐
           │ sends move                 │      exactly what will be sent
           │ fleet herdsman · issue 1354│
           │ before 1320                │
           └────────────────────────────┘
press ↵ →  door.act(now, &send)  →  "moved #1354 before #1320"   the row keeps the cursor: 1 ↵ again moves it again
press Esc instead: nothing is sent
```

```rust
let send = Send { screen: "queue".into(), block: "q-herdsman-1354".into(), action: "move".into(), values };
match door.act(now, &send.json()) {
    Ok(done) => Note::Done(done),
    Err(line) => Note::Refused(line),
}
```

A refusal comes back as its own line, `REFUSED <CODE>: why`, and the panel shows it.

### The door (server crate): the only new public surface

```rust
impl Local<'_> {
    pub fn new(ctx: &Ctx, r: &dyn Runner) -> Local<'_>;
    pub fn places(&self) -> Vec<Place>;
    pub fn home(fleet: Option<&str>) -> String;
    pub fn screen(&self, now: u64, path: &str) -> Option<Value>;
    pub fn act(&self, now: u64, body: &Value) -> Result<Done, String>;
    pub fn refresh(&self, now: u64);
    pub fn stamp(&self) -> (u64, Option<u64>);
}
```

`screen` is the builder `GET /screens/<x>` calls. `act` is the code `POST /action` runs, moved into one function
both call. No request is formatted, no socket is opened, nothing listens.

### The tests, by entry point

```rust
for path in every_screen(&door) {
    let tree = door.screen(now, &path).unwrap();
    for w in [40, 120] {
        let drawn = text(&lay(&tree, w, &Ui::default(), now));
        for s in texts(&tree) {
            assert!(drawn.contains(&squash(&s)), "{path} at {w}: {s}");
        }
    }
}

assert_eq!(door.screen(now, "/screens/queue"), Some(get(&server, "/screens/queue")));

let odd = json!({"type": "screen", "id": "x", "blocks": [{"type": "hologram", "title": "new", "blocks": [row("r1")]}]});
assert!(text(&lay(&odd, 40, &Ui::default(), now)).contains("new"));

key(&mut app, KeyCode::Char('1'));
assert!(screen(&mut t, &mut app).contains("sends move"));
assert!(kinds(&c, "queue").is_empty());
key(&mut app, KeyCode::Enter);
assert_eq!(kinds(&c, "queue").len(), 1);
```

`texts` walks a tree and returns each block's title, subtitle, value, badge, header, footer, label and markdown;
`lay` runs with every card unfolded, and `squash` drops markdown marks and folds spaces, since text is wrapped.

## Slices

Each slice is one PR that leaves staging green and the app usable. The ADR allows them, as the issue asks.

| # | Slice | The owner sees | `src/` net, about |
|---|---|---|---|
| 1 | The door: `serve::Local`, the action arm moved onto one function, places, the pruning | nothing new | +140 |
| 2 | The Board tab, read-only: the frame with two tabs, the feed, `lay` for every block, navigation, 40x18 | `drover board` live, every screen, no actions yet | +1,000 |
| 3 | Actions: the two-press panel shared with Threads, forms, choice notes, run cards, paging, docs, the hook test | the complete client; then the phone check | +650 |

About +1,800 lines against a headroom of 3,211 once the open PRs land (`drover cap herdsman`, 2026-11-19). The
cap raise of q2307 counted this issue; the impl lane asks only if its own PR would cross the cap.

## What the design stage decides, and what it may not

Design decides how it looks: glyphs, colours, the key letters, where the cards and the places sit, how a sheet is
framed at 120 columns. It may not change what [adr.md](adr.md) fixes: every text is wrapped and none is cut, an
action is a digit then Enter with the panel between them, Esc or Backspace backs out, state is kept by block id.
