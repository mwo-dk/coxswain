//! The spike's plugin: spike-games behind the `.wit` contract, built for wasm32-unknown-unknown
//! (no WASI: it imports nothing) and made a component by `spike-host componentize`.
use std::cell::RefCell;

wit_bindgen::generate!({ world: "plugin", path: "../wit" });

use exports::coxswain::spike::grid::{self, Cell, Colour};
use exports::coxswain::spike::reader::{self, Job, Row, Span};
use spike_games as g;

struct Plugin;

thread_local! {
    static ROWS: RefCell<Vec<g::Row>> = const { RefCell::new(Vec::new()) };
    static GRID: RefCell<g::grid::Grid> = RefCell::new(g::grid::Grid::default());
}

fn job(j: Job) -> g::Job {
    match j {
        Job::PgnScan => g::Job::PgnScan,
        Job::PgnParse => g::Job::PgnParse,
        Job::ScidTable => g::Job::ScidTable,
        Job::ScidDecode => g::Job::ScidDecode,
    }
}

fn row(r: g::Row) -> Row {
    Row { white: r.white, black: r.black, event: r.event, site: r.site, date: r.date, result: r.result, eco: r.eco, plies: r.plies }
}

fn slices(files: &[Vec<u8>]) -> Vec<&[u8]> {
    files.iter().map(|f| f.as_slice()).collect()
}

impl reader::Guest for Plugin {
    fn split(j: Job, files: Vec<Vec<u8>>, parts: u32) -> Vec<Span> {
        g::split(job(j), &slices(&files), parts).into_iter().map(|(start, end)| Span { start, end }).collect()
    }
    fn work(j: Job, files: Vec<Vec<u8>>, span: Span) -> Vec<Row> {
        g::work(job(j), &slices(&files), (span.start, span.end)).into_iter().map(row).collect()
    }
    fn load(j: Job, files: Vec<Vec<u8>>) -> u32 {
        let rows = g::all(job(j), &slices(&files));
        let n = rows.len() as u32;
        ROWS.with(|r| *r.borrow_mut() = rows);
        n
    }
    fn page(start: u32, count: u32) -> Vec<Row> {
        ROWS.with(|r| {
            let r = r.borrow();
            let s = (start as usize).min(r.len());
            r[s..(s + count as usize).min(r.len())].iter().cloned().map(row).collect()
        })
    }
    fn positions(pgn: Vec<u8>) -> Vec<String> {
        g::positions(&pgn)
    }
}

fn colour(c: g::grid::Colour) -> Colour {
    use g::grid::Colour as C;
    match c {
        C::Text => Colour::Text,
        C::Dim => Colour::Dim,
        C::Accent => Colour::Accent,
        C::LightSquare => Colour::LightSquare,
        C::DarkSquare => Colour::DarkSquare,
        C::WhitePiece => Colour::WhitePiece,
        C::BlackPiece => Colour::BlackPiece,
        C::Highlight => Colour::Highlight,
        C::Background => Colour::Background,
    }
}

impl grid::Guest for Plugin {
    fn open(pgn: Vec<u8>) -> u32 {
        GRID.with(|gr| gr.borrow_mut().open(&pgn))
    }
    fn draw(width: u16, height: u16) -> Vec<Cell> {
        GRID.with(|gr| gr.borrow().draw(width, height).into_iter().map(|c| Cell { ch: c.ch, fg: colour(c.fg), bg: colour(c.bg) }).collect())
    }
    fn key(name: String) -> bool {
        GRID.with(|gr| gr.borrow_mut().key(&name))
    }
}

export!(Plugin);
