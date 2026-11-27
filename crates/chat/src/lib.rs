mod board;
mod ui;

use anyhow::Result;
use drover_core::runner::Runner;
use drover_core::Ctx;
pub use drover_core::Open;
pub use drover_server::serve::Local;
use ratatui::crossterm::event::{self, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{backend::Backend, Frame, Terminal};
use std::io::IsTerminal;
use std::thread::Scope;
use std::time::Duration;

pub use board::text;

const TICK: Duration = Duration::from_millis(500);

/// `drover board`: the full-screen Board; `Ok(false)` when stdout is no terminal.
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

/// The full-screen app: the Board.
pub struct App<'s, 'e> {
    board: board::Board<'s, 'e>,
    /// Set by Ctrl-C, or `q` on the Board's root: the loop ends and the terminal is restored.
    pub quit: bool,
}

impl<'s, 'e> App<'s, 'e> {
    /// The app on `open`'s fleet; its feed runs on scope `s`, its screens come from `door`.
    pub fn new(s: &'s Scope<'s, 'e>, ctx: &'e Ctx, _: &'e dyn Runner, door: &'e Local<'e>, open: &Open) -> Self {
        let Open::Board(fleet) = open;
        App {
            board: board::Board::new(s, ctx, door, fleet.as_deref()),
            quit: false,
        }
    }

    /// Take the feed's results, waiting up to `wait`.
    pub fn tick(&mut self, wait: Duration) -> Result<()> {
        self.board.tick(wait);
        Ok(())
    }

    /// One key press: Ctrl-C here, the rest to the Board.
    pub fn key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        match (k.code, k.modifiers.contains(KeyModifiers::CONTROL)) {
            (KeyCode::Char('c'), true) => self.quit = true,
            _ => match self.board.key(&k) {
                board::Out::Quit => self.quit = true,
                board::Out::Stay => {}
            },
        }
    }

    /// The Board.
    pub fn draw(&self, f: &mut Frame) {
        self.board.draw(f, f.area());
    }
}
