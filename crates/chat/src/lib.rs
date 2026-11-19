mod view;

use anyhow::Result;
use drover_core::config::Config;
use drover_core::events::{working, Event, Json};
use drover_core::runner::Runner;
use drover_core::{card, clock, doctor, orchestrator, shed, Ctx};
use ratatui::crossterm::event::{self, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{backend::Backend, Terminal};
use serde_json::Value;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::Scope;
use std::time::Duration;

const TICK: Duration = Duration::from_millis(500);
const CHECK_SECS: u64 = 10;
const TRIES: u8 = 3;
const NO_JOB: &str = "shed serve --detach";

/// `drover chat [thread]`: the thread list, or `thread` open, full screen until Ctrl-C or `q`.
pub fn main(ctx: &Ctx, r: &dyn Runner, thread: Option<&str>) -> Result<()> {
    if let Some(id) = thread {
        orchestrator::find(&working(&ctx.state)?, id)?;
    }
    let mut term = ratatui::init();
    std::thread::scope(|s| {
        let res = run(&mut App::new(s, ctx, r, thread), &mut term);
        ratatui::restore();
        res
    })
}

fn run<B: Backend>(app: &mut App, term: &mut Terminal<B>) -> Result<()>
where
    B::Error: Send + Sync + 'static,
{
    while !app.quit {
        app.tick(Duration::ZERO)?;
        term.draw(|f| app.draw(f))?;
        if event::poll(TICK)? {
            if let event::Event::Key(k) = event::read()? {
                app.key(k);
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Shed {
    Up,
    Starting(&'static str),
    Failed(&'static str),
}

struct Wait {
    text: String,
    t: Event,
    line: String,
    tries: u8,
}

enum Note {
    Health(Vec<String>, bool, Option<&'static str>),
    Delivered(Wait, Result<(), String>),
    Done(Result<String, String>),
}

#[derive(Clone)]
enum Act {
    Answer {
        fleet: String,
        ask: String,
        n: u64,
        text: String,
    },
    Card {
        card: Value,
        approve: bool,
    },
}

struct Pick {
    act: Act,
    busy: bool,
    refused: Option<String>,
}

/// One `drover chat` screen: what it shows, the keys' state, and the jobs it runs off the draw loop.
pub struct App<'s, 'e> {
    s: &'s Scope<'s, 'e>,
    ctx: &'e Ctx,
    r: &'e dyn Runner,
    born: u64,
    tx: Sender<Note>,
    rx: Receiver<Note>,
    checked: Option<u64>,
    checking: bool,
    known: bool,
    /// Set by Ctrl-C or `q`: the loop ends and the terminal is restored.
    pub quit: bool,
    board: bool,
    open: Option<String>,
    sel: usize,
    rows: Vec<Value>,
    items: Vec<Value>,
    input: String,
    cursor: usize,
    scroll: std::cell::Cell<usize>,
    pick: Option<Pick>,
    error: Option<String>,
    waiting: Vec<Wait>,
    serve: Vec<String>,
    shed: Shed,
    sheet: Option<usize>,
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or_default()
}

impl<'s, 'e> App<'s, 'e> {
    /// A chat on the list, or on thread `open`; its jobs run on scope `s`.
    pub fn new(s: &'s Scope<'s, 'e>, ctx: &'e Ctx, r: &'e dyn Runner, open: Option<&str>) -> Self {
        let (tx, rx) = channel();
        App {
            s,
            ctx,
            r,
            born: clock::now(),
            tx,
            rx,
            checked: None,
            checking: false,
            known: false,
            quit: false,
            board: false,
            open: open.map(Into::into),
            sel: 0,
            rows: vec![],
            items: vec![],
            input: String::new(),
            cursor: 0,
            scroll: std::cell::Cell::new(0),
            pick: None,
            error: None,
            waiting: vec![],
            serve: vec![],
            shed: Shed::Up,
            sheet: None,
        }
    }

    fn now(&self) -> Ctx {
        let now = self.ctx.now + clock::now().saturating_sub(self.born);
        Ctx {
            now,
            ..self.ctx.clone()
        }
    }

    /// Start a health check when one is due, take the jobs' results (waiting up to `wait` for the first), reload the log.
    pub fn tick(&mut self, wait: Duration) -> Result<()> {
        let ctx = self.now();
        if !self.checking && self.checked.is_none_or(|t| ctx.now >= t + CHECK_SECS) {
            self.check(&ctx);
        }
        let first = self.rx.recv_timeout(wait).ok();
        let notes: Vec<Note> = first.into_iter().chain(self.rx.try_iter()).collect();
        for n in notes {
            self.apply(n);
        }
        self.load(&self.now())
    }

    fn load(&mut self, ctx: &Ctx) -> Result<()> {
        let cfg = Config::load(&ctx.config)?;
        let mut out = Vec::new();
        orchestrator::ls(ctx, &cfg, &mut out, None, true)?;
        let events = working(&ctx.state)?;
        let mut rows: Vec<Value> = serde_json::from_slice(&out)?;
        rows.retain(|r| !orchestrator::archived(&events, s(r, "thread")));
        let key = |r: &Value| (s(r, "repo") != "all", s(r, "repo").to_string());
        rows.sort_by(|a, b| key(a).cmp(&key(b)).then(b["at"].as_u64().cmp(&a["at"].as_u64())));
        self.sel = self.sel.min(rows.len().saturating_sub(1));
        self.rows = rows;
        let shown = self
            .open
            .clone()
            .or_else(|| self.rows.get(self.sel).map(|r| s(r, "thread").into()));
        self.items = shown
            .as_deref()
            .map_or_else(Vec::new, |id| orchestrator::items(&events, id));
        if let Some(t) = self.open.as_deref().and_then(|id| orchestrator::find(&events, id).ok()) {
            orchestrator::mark_read(ctx, &cfg, &events, t)?;
        }
        Ok(())
    }

    fn check(&mut self, ctx: &Ctx) {
        (self.checking, self.checked) = (true, Some(ctx.now));
        let (ctx, tx, r) = (ctx.clone(), self.tx.clone(), self.r);
        self.s.spawn(move || {
            let note = match Config::load(&ctx.config) {
                Ok(cfg) => {
                    let serve = doctor::serve_down(&ctx, &cfg, r);
                    let up = cfg.shed().is_none_or(|b| shed::line(r, &b.wire).starts_with('✓'));
                    let job = if up { None } else { shed::start_job(r).map(|j| j.0) };
                    Note::Health(serve, up, job)
                }
                Err(e) => Note::Health(vec![format!("✗ config — {e:#}")], true, None),
            };
            tx.send(note).ok();
        });
    }

    fn apply(&mut self, n: Note) {
        match n {
            Note::Health(serve, up, job) => {
                (self.checking, self.known, self.serve) = (false, true, serve);
                self.shed = match (up, self.shed, job) {
                    (true, ..) => Shed::Up,
                    (false, Shed::Starting(c) | Shed::Failed(c), _) => Shed::Failed(c),
                    (false, Shed::Up, Some(c)) => Shed::Starting(c),
                    (false, Shed::Up, None) => Shed::Failed(NO_JOB),
                };
                if up {
                    for w in std::mem::take(&mut self.waiting) {
                        self.deliver(w);
                    }
                }
            }
            Note::Delivered(w, Err(e)) if w.tries >= TRIES => self.error = Some(format!("not delivered: {e}")),
            Note::Delivered(w, Err(_)) => {
                self.waiting.push(w);
                self.checked = None;
            }
            Note::Delivered(_, Ok(())) => {}
            Note::Done(res) => match (self.pick.as_mut(), res) {
                (Some(p), Err(e)) => (p.busy, p.refused) = (false, Some(e)),
                (_, _) => self.pick = None,
            },
        }
    }

    fn deliver(&mut self, mut w: Wait) {
        w.tries += 1;
        let (ctx, tx, r) = (self.now(), self.tx.clone(), self.r);
        self.s.spawn(move || {
            let cfg = Config::load(&ctx.config);
            let res = cfg.and_then(|cfg| orchestrator::deliver(&ctx, &cfg, r, &w.t, &w.line));
            tx.send(Note::Delivered(w, res.map(drop).map_err(|e| format!("{e:#}"))))
                .ok();
        });
    }

    fn send(&mut self) {
        let Some(id) = self.open.clone() else { return };
        let ctx = self.now();
        let text = self.input.trim().to_string();
        let cfg = Config::load(&ctx.config);
        match cfg.and_then(|cfg| orchestrator::post(&ctx, &cfg, (&id, &text, &[]), ("owner", false))) {
            Ok((t, line)) => {
                (self.input, self.cursor, self.error) = (String::new(), 0, None);
                self.scroll.set(0);
                self.deliver(Wait {
                    text,
                    t,
                    line,
                    tries: 0,
                });
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    fn open_ask(&self) -> Option<&Value> {
        self.items
            .iter()
            .rev()
            .find(|i| i["ask"].is_string() && i["open"] == true)
    }

    fn open_card(&self) -> Option<&Value> {
        self.items
            .iter()
            .rev()
            .find(|i| i["card"].is_object() && i["open"] == true)
    }

    fn select(&self, c: char) -> Option<Act> {
        if let Some(n) = c.to_digit(10).filter(|n| *n > 0).map(u64::from) {
            let a = self.open_ask()?;
            let o = a["options"].as_array()?.iter().find(|o| o["n"] == n)?;
            let (fleet, ask, text) = (s(a, "fleet").into(), s(a, "ask").into(), s(o, "text").into());
            return Some(Act::Answer { fleet, ask, n, text });
        }
        let item = self.open_card()?;
        let mut card = item["card"].clone();
        card.put("ts", item["ts"].clone());
        matches!(c, 'a' | 'd').then_some(Act::Card {
            card,
            approve: c == 'a',
        })
    }

    fn confirm(&mut self) {
        let Some(p) = self.pick.as_mut() else { return };
        if p.refused.is_some() {
            self.pick = None;
            return;
        }
        if std::mem::replace(&mut p.busy, true) {
            return;
        }
        let act = p.act.clone();
        let (ctx, tx, r) = (self.now(), self.tx.clone(), self.r);
        self.s.spawn(move || {
            let res = Config::load(&ctx.config).and_then(|cfg| match &act {
                Act::Answer { fleet, ask, n, .. } => {
                    let how = ("owner", None);
                    drover_core::answer(&ctx, &cfg, r, &mut Vec::new(), (fleet, ask), &n.to_string(), how)
                        .map(|()| format!("answered {ask}"))
                }
                Act::Card { card: c, approve } => {
                    let sha = approve.then(|| s(c, "sha").chars().take(7).collect::<String>());
                    card::settle(&ctx, &cfg, r, s(c, "id"), *approve, sha.as_deref())
                }
            });
            tx.send(Note::Done(res.map_err(|e| format!("{e:#}")))).ok();
        });
    }

    /// One key press, as the design's key list (docs/design/drover-chat/README.md) maps it.
    pub fn key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if k.kind == KeyEventKind::Release {
            return;
        }
        match (k.code, ctrl) {
            (KeyCode::Char('c'), true) => self.quit = true,
            (KeyCode::Tab, _) => self.board = !self.board,
            (KeyCode::Up | KeyCode::Char('k'), _) if self.sheet.is_some() => {
                self.sheet = self.sheet.map(|n| n.saturating_sub(1))
            }
            (KeyCode::Down | KeyCode::Char('j'), _) if self.sheet.is_some() => self.sheet = self.sheet.map(|n| n + 1),
            (KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('?'), _) if self.sheet.is_some() => self.sheet = None,
            _ if self.sheet.is_some() => {}
            (KeyCode::Char('q'), false) if self.board => self.quit = true,
            _ if self.board => {}
            (KeyCode::Enter, _) if self.pick.is_some() => self.confirm(),
            (KeyCode::Esc | KeyCode::Backspace, _) if self.pick.is_some() => self.pick = None,
            _ if self.pick.is_some() => {}
            _ if self.open.is_some() => self.chat_key(k.code, ctrl),
            (KeyCode::Up | KeyCode::Char('k'), false) => self.sel = self.sel.saturating_sub(1),
            (KeyCode::Down | KeyCode::Char('j'), false) => {
                self.sel = (self.sel + 1).min(self.rows.len().saturating_sub(1));
            }
            (KeyCode::Enter, _) => {
                self.open = self.rows.get(self.sel).map(|r| s(r, "thread").into());
                self.scroll.set(0);
            }
            (KeyCode::Char('q'), false) => self.quit = true,
            (KeyCode::Char('?'), false) => self.sheet = Some(0),
            _ => {}
        }
    }

    fn chat_key(&mut self, code: KeyCode, ctrl: bool) {
        let empty = self.input.is_empty();
        let byte = |s: &str, at: usize| s.char_indices().nth(at).map_or(s.len(), |(i, _)| i);
        match (code, ctrl) {
            (KeyCode::Enter, _) if !self.input.trim().is_empty() => self.send(),
            (KeyCode::Esc | KeyCode::Backspace, _) if empty => (self.open, self.error) = (None, None),
            (KeyCode::Backspace, _) if self.cursor > 0 => {
                self.cursor -= 1;
                self.input.remove(byte(&self.input, self.cursor));
            }
            (KeyCode::Left, _) => self.cursor = self.cursor.saturating_sub(1),
            (KeyCode::Right, _) => self.cursor = (self.cursor + 1).min(self.input.chars().count()),
            (KeyCode::Char('u'), true) => (self.input, self.cursor) = (String::new(), 0),
            (KeyCode::Up, _) => self.scroll.set(self.scroll.get() + 1),
            (KeyCode::Down, _) => self.scroll.set(self.scroll.get().saturating_sub(1)),
            (KeyCode::PageUp, _) | (KeyCode::Char('b'), true) => self.scroll.set(self.scroll.get() + 10),
            (KeyCode::PageDown, _) | (KeyCode::Char('f'), true) => {
                self.scroll.set(self.scroll.get().saturating_sub(10))
            }
            (KeyCode::Char('?'), false) if empty => self.sheet = Some(0),
            (KeyCode::Char(c), false) if empty && self.select(c).is_some() => {
                let act = self.select(c);
                self.pick = act.map(|act| Pick {
                    act,
                    busy: false,
                    refused: None,
                });
            }
            (KeyCode::Char(c), false) => {
                self.input.insert(byte(&self.input, self.cursor), c);
                self.cursor += 1;
            }
            _ => {}
        }
    }
}
