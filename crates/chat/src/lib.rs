mod board;
mod threads;
mod ui;

use anyhow::Result;
use drover_core::events::working;
use drover_core::runner::Runner;
pub use drover_core::Open;
use drover_core::{orchestrator, Ctx};
pub use drover_server::serve::Local;
use ratatui::crossterm::event::{self, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::{backend::Backend, Frame, Terminal};
use std::io::IsTerminal;
use std::thread::Scope;
use std::time::Duration;

pub use board::text;

const TICK: Duration = Duration::from_millis(500);

/// `drover chat` and `drover board`: the full-screen app on `open`'s tab; `Ok(false)` when stdout is no terminal.
pub fn main(ctx: &Ctx, r: &dyn Runner, open: &Open) -> Result<bool> {
    if let Open::Threads(Some(id)) = open {
        orchestrator::find(&working(&ctx.state)?, id)?;
    }
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

#[derive(PartialEq)]
enum Tab {
    Threads,
    Board,
}

/// The full-screen app: the Threads and Board tabs under one top line; Tab switches, each keeps its place.
pub struct App<'s, 'e> {
    tab: Tab,
    threads: threads::Threads<'s, 'e>,
    board: board::Board<'s, 'e>,
    /// Set by Ctrl-C, or `q` on the thread list or the Board's root: the loop ends and the terminal is restored.
    pub quit: bool,
}

impl<'s, 'e> App<'s, 'e> {
    /// The app on `open`'s tab; its jobs and the Board's feed run on scope `s`, its screens come from `door`.
    pub fn new(s: &'s Scope<'s, 'e>, ctx: &'e Ctx, r: &'e dyn Runner, door: &'e Local<'e>, open: &Open) -> Self {
        let (tab, thread, fleet) = match open {
            Open::Threads(t) => (Tab::Threads, t.as_deref(), None),
            Open::Board(f) => (Tab::Board, None, f.as_deref()),
        };
        App {
            tab,
            threads: threads::Threads::new(s, ctx, r, thread),
            board: board::Board::new(s, ctx, door, fleet),
            quit: false,
        }
    }

    /// Take the jobs' and the feed's results, waiting up to `wait` on the open tab's, and reload the threads.
    pub fn tick(&mut self, wait: Duration) -> Result<()> {
        let (t, b) = if self.tab == Tab::Board {
            (Duration::ZERO, wait)
        } else {
            (wait, Duration::ZERO)
        };
        self.board.tick(b);
        self.threads.tick(t)
    }

    /// One key press: Ctrl-C and Tab here, the rest to the open tab.
    pub fn key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        match (k.code, k.modifiers.contains(KeyModifiers::CONTROL)) {
            (KeyCode::Char('c'), true) => self.quit = true,
            (KeyCode::Tab, _) => {
                self.tab = if self.tab == Tab::Board {
                    Tab::Threads
                } else {
                    Tab::Board
                };
            }
            _ if self.tab == Tab::Threads => {
                self.threads.key(k);
                self.quit |= self.threads.quit;
            }
            _ => match self.board.key(&k) {
                board::Out::Quit => self.quit = true,
                board::Out::Thread(t) => {
                    self.tab = Tab::Threads;
                    self.threads.open = Some(t);
                }
                board::Out::Stay => {}
            },
        }
    }

    /// The top line (tabs, what is down), then the open tab.
    pub fn draw(&self, f: &mut Frame) {
        let a = f.area();
        let top = self
            .threads
            .top(usize::from(a.width), self.tab == Tab::Board, self.board.badge());
        let h = u16::try_from(top.len()).unwrap_or(a.height).min(a.height);
        ui::put(f, a, top);
        let rest = Rect {
            y: a.y + h,
            height: a.height - h,
            ..a
        };
        match self.tab {
            Tab::Threads => self.threads.draw(f, rest),
            Tab::Board => self.board.draw(f, rest),
        }
    }
}
