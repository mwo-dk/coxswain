//! Pictures on the terminal board: the chess pieces as the desktop app's own drawings (Uray M.
//! János's set, `pieces/`), where the terminal shows images. The terminal is asked once what it
//! can do: kitty's graphics protocol (kitty, Ghostty), drawn through its Unicode placeholders so
//! a picture is a run of characters in the frame like any other and goes when they are drawn
//! over; or sixel (foot, WezTerm, xterm with sixel, mlterm, tmux built with it), one picture per
//! square written in its top left cell. Inside tmux, kitty's pictures go through only where
//! `allow-passthrough` is on already; it is never turned on. Each picture is drawn with
//! resvg to the square's size in pixels, on the square's own colour, once per piece, colour and
//! size. Windows Terminal draws sixel too, and is asked through the console's input. Elsewhere
//! (no answer, the Linux console, Windows' own console) the board keeps its characters.

use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::Color;
use resvg::{tiny_skia, usvg};
use std::collections::HashMap;
use std::io::Write;
use std::num::NonZeroU16;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// How the terminal takes pictures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Kitty,
    Sixel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Support {
    pub kind: Kind,
    /// A character cell's size in pixels, width and height.
    pub cell: (u16, u16),
    /// Inside tmux: kitty's sequences go wrapped, for the terminal outside.
    tmux: bool,
}

/// Set once the app has the terminal: only then is it asked (never in a test).
static LIVE: AtomicBool = AtomicBool::new(false);
static SUPPORT: OnceLock<Option<Support>> = OnceLock::new();

pub fn live() {
    LIVE.store(true, Ordering::Relaxed);
}

/// What the terminal can show, asked the first time and kept.
pub fn support() -> Option<Support> {
    if !LIVE.load(Ordering::Relaxed) {
        return None;
    }
    *SUPPORT.get_or_init(ask)
}

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_default()
}

/// Kitty's answer to a query, sixel in the device attributes (4), the cell size (CSI 16 t),
/// and whether the device attributes, which every terminal sends and sends last, have come.
#[derive(Debug, Default, PartialEq)]
struct Reply {
    kitty: bool,
    sixel: bool,
    cell: Option<(u16, u16)>,
    done: bool,
}

fn parse(r: &[u8]) -> Reply {
    let s = String::from_utf8_lossy(r);
    let mut out = Reply { kitty: s.contains("_Gi=31;OK"), ..Reply::default() };
    // `ESC [ 6 ; height ; width t`
    if let Some(at) = s.find("\x1b[6;") {
        let rest = &s[at + 4..];
        if let Some(end) = rest.find('t') {
            let mut n = rest[..end].split(';').filter_map(|x| x.parse::<u16>().ok());
            if let (Some(h), Some(w)) = (n.next(), n.next())
                && h > 0
                && w > 0
            {
                out.cell = Some((w, h));
            }
        }
    }
    // `ESC [ ? 62 ; 4 ; … c`
    if let Some(at) = s.find("\x1b[?") {
        let rest = &s[at + 3..];
        if let Some(end) = rest.find('c')
            && rest[..end].bytes().all(|b| b.is_ascii_digit() || b == b';')
        {
            out.sixel = rest[..end].split(';').any(|x| x == "4");
            out.done = true;
        }
    }
    out
}

/// Wraps a sequence for tmux to hand to the terminal outside it.
fn wrap(tmux: bool, seq: &str) -> String {
    if tmux { format!("\x1bPtmux;{}\x1b\\", seq.replace('\x1b', "\x1b\x1b")) } else { seq.to_string() }
}

#[cfg(unix)]
fn ask() -> Option<Support> {
    let term = env("TERM");
    if term == "linux" || term == "dumb" || term.is_empty() {
        return None;
    }
    let tmux = !env("TMUX").is_empty();
    let ask_tmux = |args: &[&str]| {
        std::process::Command::new("tmux")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    // Through tmux, kitty's pictures need passthrough, which is the user's to turn on. Sixel
    // not: tmux keeps a picture only until a cell under it is written, and the cells under
    // one are never written here.
    let pass = tmux && matches!(ask_tmux(&["show", "-Apv", "allow-passthrough"]).as_str(), "on" | "all");
    // WezTerm, Konsole and VS Code's terminal answer kitty's query but draw no placeholders:
    // sixel there.
    let placeholders = !matches!(env("TERM_PROGRAM").as_str(), "WezTerm" | "vscode") && env("WEZTERM_EXECUTABLE").is_empty() && env("KONSOLE_VERSION").is_empty();
    // Inside tmux only kitty's pictures can be had, and the whole query goes to the terminal
    // outside, so that its answers come back in order, the device attributes last.
    if tmux && !(pass && placeholders) {
        return None;
    }
    let kitty = if placeholders { "\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\" } else { "" };
    let q = wrap(tmux, &format!("{kitty}\x1b[16t\x1b[c"));
    let reply = parse(&read_reply(&q)?);
    if !reply.done {
        return None;
    }
    let cell = reply.cell.or_else(cell_from_winsize)?;
    let kind = if reply.kitty {
        Kind::Kitty
    } else if reply.sixel && !tmux {
        Kind::Sixel
    } else {
        return None;
    };
    Some(Support { kind, cell, tmux: tmux && kind == Kind::Kitty })
}

/// Windows Terminal (1.22 and later) draws sixel; only it is asked, as the console it runs is
/// the one sure to answer at once. It draws a picture in cells of 10 × 20 of its own pixels,
/// whatever the font, and says so to `CSI 16 t`.
#[cfg(windows)]
fn ask() -> Option<Support> {
    if env("WT_SESSION").is_empty() {
        return None;
    }
    let reply = parse(&read_reply("\x1b[16t\x1b[c")?);
    (reply.done && reply.sixel).then(|| Support { kind: Kind::Sixel, cell: reply.cell.unwrap_or((10, 20)), tmux: false })
}

#[cfg(not(any(unix, windows)))]
fn ask() -> Option<Support> {
    None
}

/// The same on Windows: the answer comes through the console's input as key presses, with its
/// virtual terminal input on for the while; the console's mode is put back after.
#[cfg(windows)]
fn read_reply(q: &str) -> Option<Vec<u8>> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::{ENABLE_VIRTUAL_TERMINAL_INPUT, GetConsoleMode, INPUT_RECORD, KEY_EVENT, ReadConsoleInputW, SetConsoleMode};
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    // Where crossterm reads keys: the console's input, whatever standard input is.
    let conin = std::fs::OpenOptions::new().read(true).write(true).open("CONIN$").ok()?;
    let h = conin.as_raw_handle();
    let mut mode = 0;
    if unsafe { GetConsoleMode(h, &mut mode) } == 0 || unsafe { SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_INPUT) } == 0 {
        return None;
    }
    let got = (|| {
        let mut out = std::io::stdout();
        out.write_all(q.as_bytes()).ok()?;
        out.flush().ok()?;
        let until = std::time::Instant::now() + std::time::Duration::from_millis(1000);
        let mut units: Vec<u16> = vec![];
        loop {
            let left = until.saturating_duration_since(std::time::Instant::now()).as_millis() as u32;
            if left == 0 || unsafe { WaitForSingleObject(h, left) } != 0 {
                return None;
            }
            let mut recs = [INPUT_RECORD::default(); 64];
            let mut n = 0u32;
            if unsafe { ReadConsoleInputW(h, recs.as_mut_ptr(), recs.len() as u32, &mut n) } == 0 {
                return None;
            }
            for r in &recs[..n as usize] {
                if r.EventType as u32 == KEY_EVENT {
                    let k = unsafe { r.Event.KeyEvent };
                    let c = unsafe { k.uChar.UnicodeChar };
                    if k.bKeyDown != 0 && c != 0 {
                        units.push(c);
                    }
                }
            }
            let got = String::from_utf16_lossy(&units).into_bytes();
            if parse(&got).done {
                return Some(got);
            }
        }
    })();
    unsafe { SetConsoleMode(h, mode) };
    got
}

/// Writes the query and reads the answer, up to the device attributes or a second at most:
/// a terminal that does not answer is not waited for.
#[cfg(unix)]
fn read_reply(q: &str) -> Option<Vec<u8>> {
    use std::os::fd::AsRawFd;
    // Where crossterm reads keys: standard input when it is the terminal, else /dev/tty.
    let tty = (unsafe { libc::isatty(0) } == 0).then(|| std::fs::File::open("/dev/tty").ok()).flatten();
    let fd = tty.as_ref().map_or(0, |f| f.as_raw_fd());
    let mut out = std::io::stdout();
    out.write_all(q.as_bytes()).ok()?;
    out.flush().ok()?;
    let until = std::time::Instant::now() + std::time::Duration::from_millis(1000);
    let mut got = vec![];
    loop {
        let left = until.saturating_duration_since(std::time::Instant::now()).as_millis() as i32;
        if left <= 0 {
            return None;
        }
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut p, 1, left) } <= 0 {
            return None;
        }
        let mut buf = [0u8; 256];
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            return None;
        }
        got.extend_from_slice(&buf[..n as usize]);
        if parse(&got).done {
            return Some(got);
        }
    }
}

#[cfg(unix)]
fn cell_from_winsize() -> Option<(u16, u16)> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } != 0 || ws.ws_col == 0 || ws.ws_row == 0 {
        return None;
    }
    let (w, h) = (ws.ws_xpixel / ws.ws_col, ws.ws_ypixel / ws.ws_row);
    (w > 0 && h > 0).then_some((w, h))
}

/// The pieces' drawings, the desktop app's (a test keeps them the same).
fn svg(piece: char) -> &'static [u8] {
    match piece {
        'K' => include_bytes!("../pieces/wK.svg"),
        'Q' => include_bytes!("../pieces/wQ.svg"),
        'R' => include_bytes!("../pieces/wR.svg"),
        'B' => include_bytes!("../pieces/wB.svg"),
        'N' => include_bytes!("../pieces/wN.svg"),
        'P' => include_bytes!("../pieces/wP.svg"),
        'k' => include_bytes!("../pieces/bK.svg"),
        'q' => include_bytes!("../pieces/bQ.svg"),
        'r' => include_bytes!("../pieces/bR.svg"),
        'b' => include_bytes!("../pieces/bB.svg"),
        'n' => include_bytes!("../pieces/bN.svg"),
        _ => include_bytes!("../pieces/bP.svg"),
    }
}

/// A piece on a square of colour `bg`, `w` × `h` pixels: straight RGBA, opaque.
fn raster(piece: char, bg: (u8, u8, u8), w: u32, h: u32) -> Option<tiny_skia::Pixmap> {
    let tree = usvg::Tree::from_data(svg(piece), &usvg::Options::default()).ok()?;
    let mut pm = tiny_skia::Pixmap::new(w, h)?;
    pm.fill(tiny_skia::Color::from_rgba8(bg.0, bg.1, bg.2, 255));
    let side = w.min(h) as f32 * 0.95;
    let s = side / tree.size().width().max(tree.size().height());
    let t = tiny_skia::Transform::from_row(s, 0.0, 0.0, s, (w as f32 - side) / 2.0, (h as f32 - side) / 2.0);
    resvg::render(&tree, t, &mut pm.as_mut());
    Some(pm)
}

/// Kitty's row and column marks for placeholders (the first of its list: a square is at most
/// this many cells across).
const MARKS: [char; 39] = [
    '\u{305}', '\u{30D}', '\u{30E}', '\u{310}', '\u{312}', '\u{33D}', '\u{33E}', '\u{33F}', '\u{346}', '\u{34A}', '\u{34B}', '\u{34C}', '\u{350}', '\u{351}', '\u{352}', '\u{357}', '\u{35B}', '\u{363}', '\u{364}', '\u{365}', '\u{366}', '\u{367}', '\u{368}', '\u{369}', '\u{36A}', '\u{36B}', '\u{36C}', '\u{36D}', '\u{36E}', '\u{36F}', '\u{483}', '\u{484}', '\u{485}', '\u{486}', '\u{487}', '\u{592}', '\u{593}', '\u{594}', '\u{595}'
];

/// Image ids: one run per process, in kitty's 24 bits (the placeholder's colour), clear of
/// the small numbers other programs take.
static NEXT_ID: AtomicU32 = AtomicU32::new(0);

fn next_id() -> u32 {
    let base = 0x40_0000 + (std::process::id() & 0x3fff) * 0x100;
    (base + NEXT_ID.fetch_add(1, Ordering::Relaxed)) & 0xff_ffff
}

/// What a square takes: in kitty, an image id; in sixel, the sequence.
enum Made {
    Kitty(u32),
    Sixel(String),
}

/// A picture: the piece, the square's colour and its size in cells.
type Look = (char, (u8, u8, u8), u16, u16);

/// The pictures of one board, made as they are needed and kept until it closes; kitty's are
/// taken out of the terminal then.
pub struct Pieces {
    pub support: Support,
    made: HashMap<Look, Option<Made>>,
    /// The squares with a picture in the frame before and in this one (`finish`).
    before: Vec<Rect>,
    now: Vec<Rect>,
}

impl Pieces {
    pub fn new(support: Support) -> Pieces {
        Pieces { support, made: HashMap::new(), before: vec![], now: vec![] }
    }

    /// Draws `piece` over the cells of `area`, a square of colour `bg`. False when it could
    /// not be made: the characters stay.
    pub fn draw(&mut self, buf: &mut Buffer, area: Rect, piece: char, bg: Color) -> bool {
        let Color::Rgb(r, g, b) = bg else { return false };
        if area.width as usize > MARKS.len() || area.height as usize > MARKS.len() || area.is_empty() {
            return false;
        }
        let s = self.support;
        let key = (piece, (r, g, b), area.width, area.height);
        let made = self.made.entry(key).or_insert_with(|| {
            let (w, h) = (area.width as u32 * s.cell.0 as u32, area.height as u32 * s.cell.1 as u32);
            let pm = raster(piece, (r, g, b), w, h)?;
            Some(match s.kind {
                Kind::Kitty => {
                    let id = next_id();
                    transmit(s.tmux, id, &pm.encode_png().ok()?, area.width, area.height).ok()?;
                    Made::Kitty(id)
                }
                Kind::Sixel => Made::Sixel(sixel(pm.data(), w as usize, h as usize)),
            })
        });
        match made {
            None => false,
            Some(Made::Kitty(id)) => {
                let fg = Color::Rgb((*id >> 16) as u8, (*id >> 8) as u8, *id as u8);
                for dy in 0..area.height {
                    for dx in 0..area.width {
                        if let Some(c) = buf.cell_mut((area.x + dx, area.y + dy)) {
                            let mut sym = String::from('\u{10EEEE}');
                            sym.push(MARKS[dy as usize]);
                            sym.push(MARKS[dx as usize]);
                            c.set_symbol(&sym).set_fg(fg).set_bg(bg);
                        }
                    }
                }
                true
            }
            Some(Made::Sixel(seq)) => {
                self.now.push(area);
                for dy in 0..area.height {
                    for dx in 0..area.width {
                        if let Some(c) = buf.cell_mut((area.x + dx, area.y + dy)) {
                            if dx == 0 && dy == 0 {
                                c.set_symbol(seq).set_diff_option(CellDiffOption::ForcedWidth(NonZeroU16::MIN));
                            } else {
                                c.set_diff_option(CellDiffOption::Skip);
                            }
                        }
                    }
                }
                true
            }
        }
    }
}

impl Pieces {
    /// After a frame's pictures: a square whose sixel is gone is written again in full, as
    /// its cells may look the same as before to ratatui while the picture still covers them.
    pub fn finish(&mut self, buf: &mut Buffer) {
        for r in std::mem::take(&mut self.before) {
            if self.now.contains(&r) {
                continue;
            }
            for y in r.top()..r.bottom() {
                for x in r.left()..r.right() {
                    if let Some(c) = buf.cell_mut((x, y))
                        && c.diff_option == CellDiffOption::None
                    {
                        c.set_diff_option(CellDiffOption::AlwaysUpdate);
                    }
                }
            }
        }
        self.before = std::mem::take(&mut self.now);
    }
}

impl Drop for Pieces {
    fn drop(&mut self) {
        let mut out = String::new();
        for m in self.made.values() {
            if let Some(Made::Kitty(id)) = m {
                out.push_str(&wrap(self.support.tmux, &format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\")));
            }
        }
        if !out.is_empty() {
            let mut o = std::io::stdout();
            let _ = o.write_all(out.as_bytes()).and_then(|_| o.flush());
        }
    }
}

/// Sends a PNG to kitty as image `id`, with a virtual placement `cols` × `rows` cells that the
/// placeholders show; q=2: no answer comes back to be read as keys.
fn transmit(tmux: bool, id: u32, png: &[u8], cols: u16, rows: u16) -> std::io::Result<()> {
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();
    let mut out = String::new();
    for (i, c) in chunks.iter().enumerate() {
        let more = (i + 1 < chunks.len()) as u8;
        let head = if i == 0 { format!("a=T,U=1,f=100,i={id},c={cols},r={rows},q=2,m={more}") } else { format!("m={more}") };
        out.push_str(&wrap(tmux, &format!("\x1b_G{head};{}\x1b\\", std::str::from_utf8(c).unwrap_or(""))));
    }
    let mut o = std::io::stdout();
    o.write_all(out.as_bytes())?;
    o.flush()
}

/// An opaque RGBA picture as a sixel sequence: its own colours, the 255 most used of them
/// exactly (the square's, black and white among them) and the rest as the nearest of those.
fn sixel(rgba: &[u8], w: usize, h: usize) -> String {
    use std::fmt::Write as _;
    let px: Vec<(u8, u8, u8)> = rgba.chunks_exact(4).map(|p| (p[0], p[1], p[2])).collect();
    let mut count: HashMap<(u8, u8, u8), usize> = HashMap::new();
    for p in &px {
        *count.entry(*p).or_default() += 1;
    }
    let mut colours: Vec<((u8, u8, u8), usize)> = count.into_iter().collect();
    colours.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let palette: Vec<(u8, u8, u8)> = colours.iter().take(255).map(|c| c.0).collect();
    let mut index: HashMap<(u8, u8, u8), usize> = palette.iter().enumerate().map(|(i, c)| (*c, i)).collect();
    let near = |c: (u8, u8, u8)| {
        let d = |p: &(u8, u8, u8)| {
            let (a, b, e) = (c.0 as i32 - p.0 as i32, c.1 as i32 - p.1 as i32, c.2 as i32 - p.2 as i32);
            a * a + b * b + e * e
        };
        (0..palette.len()).min_by_key(|&i| d(&palette[i])).unwrap_or(0)
    };
    let idx: Vec<usize> = px
        .iter()
        .map(|c| match index.get(c) {
            Some(i) => *i,
            None => {
                let i = near(*c);
                index.insert(*c, i);
                i
            }
        })
        .collect();
    // P2=1: pixels not set leave what is there; the raster attributes give the exact size.
    let mut s = format!("\x1bP0;1;0q\"1;1;{w};{h}");
    for (i, c) in palette.iter().enumerate() {
        let pc = |v: u8| (v as u32 * 100 + 127) / 255;
        let _ = write!(s, "#{i};2;{};{};{}", pc(c.0), pc(c.1), pc(c.2));
    }
    for band in (0..h).step_by(6) {
        let mut first = true;
        for colour in 0..palette.len() {
            let bits: Vec<u8> = (0..w)
                .map(|x| (0..6).filter(|dy| band + dy < h && idx[(band + dy) * w + x] == colour).fold(0u8, |a, dy| a | (1 << dy)))
                .collect();
            if bits.iter().all(|&b| b == 0) {
                continue;
            }
            if !first {
                s.push('$');
            }
            first = false;
            let _ = write!(s, "#{colour}");
            let mut x = 0;
            while x < w {
                let mut run = 1;
                while x + run < w && bits[x + run] == bits[x] {
                    run += 1;
                }
                let ch = (63 + bits[x]) as char;
                if run > 3 {
                    let _ = write!(s, "!{run}{ch}");
                } else {
                    s.extend(std::iter::repeat_n(ch, run));
                }
                x += run;
            }
        }
        s.push('-');
    }
    s.push_str("\x1b\\");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_read() {
        let kitty = parse(b"\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[?62;22c");
        assert_eq!(kitty, Reply { kitty: true, sixel: false, cell: Some((10, 20)), done: true });
        let foot = parse(b"\x1b[6;17;8t\x1b[?62;4;22c");
        assert_eq!(foot, Reply { kitty: false, sixel: true, cell: Some((8, 17)), done: true });
        // Half an answer: wait for the rest.
        assert!(!parse(b"\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[?6").done);
        assert_eq!(wrap(true, "\x1b_Ga\x1b\\"), "\x1bPtmux;\x1b\x1b_Ga\x1b\x1b\\\x1b\\");
    }

    #[test]
    fn pieces_are_the_desktop_apps() {
        let gui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gui/public/vendor/chess-pieces");
        for n in ["K", "Q", "R", "B", "N", "P"] {
            for (side, p) in [("w", n.chars().next().unwrap()), ("b", n.to_ascii_lowercase().chars().next().unwrap())] {
                assert_eq!(std::fs::read(gui.join(format!("{side}{n}.svg"))).unwrap(), svg(p), "{side}{n}.svg");
            }
        }
    }

    #[test]
    fn a_piece_drawn_as_sixel() {
        let pm = raster('N', (0x76, 0x96, 0x56), 40, 34).unwrap();
        // The corner is the square's colour, the middle has the knight's black outline or white.
        assert_eq!(&pm.data()[..4], &[0x76, 0x96, 0x56, 0xff]);
        assert!(pm.data().chunks_exact(4).any(|p| p[..3] == [0, 0, 0]));
        let s = sixel(pm.data(), 40, 34);
        assert!(s.starts_with("\x1bP0;1;0q\"1;1;40;34") && s.ends_with("-\x1b\\"));
        // Six bands of six rows for 34.
        assert_eq!(s.matches('-').count(), 6);
        // In a frame: the sequence in the first cell taking one column, the rest skipped.
        let mut p = Pieces::new(Support { kind: Kind::Sixel, cell: (8, 17), tmux: false });
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        assert!(p.draw(&mut buf, Rect::new(2, 1, 5, 2), 'q', Color::Rgb(1, 2, 3)));
        assert!(buf[(2, 1)].symbol().starts_with("\x1bP"));
        assert_eq!(buf[(3, 1)].diff_option, CellDiffOption::Skip);
        assert_eq!(buf[(6, 2)].diff_option, CellDiffOption::Skip);
        assert_eq!(buf[(7, 2)].diff_option, CellDiffOption::None);
        p.finish(&mut buf);
        // The next frame without it: its cells are written again.
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        p.finish(&mut buf);
        assert_eq!(buf[(3, 1)].diff_option, CellDiffOption::AlwaysUpdate);
        assert_eq!(buf[(7, 2)].diff_option, CellDiffOption::None);
    }
}
