//! The one reader of game files' text, for every format: PGN, PDN and xiangqi (`games::scan`),
//! KIF and CSA (`shogi`), PBN (`bridge`) by lines, SGF (`go`) by its pieces. It reads through a
//! buffer, never the whole file, and knows the byte each line or piece starts at, so a pass
//! over a collection of millions of games keeps one game's worth at most, and a game is read
//! again later from where it starts.

use std::io::{self, BufRead};

/// The longest line kept: the rest of a longer one is read past, not kept, so that a file of
/// one endless line cannot fill the memory.
pub const MAX_LINE: usize = 1 << 20;
/// The most of one SGF value kept (a comment); the rest is read past.
pub const MAX_VALUE: usize = 128 << 10;
/// The most of one SGF property's values kept in all (`AB[aa][ab]…`), and how many values.
pub const MAX_PROP: usize = 1 << 20;
pub const MAX_VALUES: usize = 4096;

/// A piece of SGF.
#[derive(Debug, PartialEq)]
pub enum Sgf {
    Open,
    Close,
    Node,
    /// A property: its name (its capital letters: FF[3]'s `AddBlack` is `AB`) and its values,
    /// still escaped.
    Prop(String, Vec<Vec<u8>>),
}

pub struct Lexer<R> {
    r: R,
    /// The byte the next line or piece starts at.
    pub at: u64,
}

impl<R: BufRead> Lexer<R> {
    pub fn new(r: R) -> Lexer<R> {
        Lexer { r, at: 0 }
    }

    fn fill(&mut self) -> io::Result<&[u8]> {
        loop {
            match self.r.fill_buf() {
                Ok(_) => return self.r.fill_buf(),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn consume(&mut self, n: usize) {
        self.r.consume(n);
        self.at += n as u64;
    }

    fn peek(&mut self) -> io::Result<Option<u8>> {
        Ok(self.fill()?.first().copied())
    }

    /// The next line into `line` (its end of line kept), at most `MAX_LINE` bytes of it: how
    /// many bytes it had, 0 at the end.
    pub fn line(&mut self, line: &mut Vec<u8>) -> io::Result<usize> {
        line.clear();
        let mut n = 0;
        loop {
            let buf = self.fill()?;
            if buf.is_empty() {
                return Ok(n);
            }
            let (take, done) = match memchr::memchr(b'\n', buf) {
                Some(i) => (i + 1, true),
                None => (buf.len(), false),
            };
            let room = MAX_LINE.saturating_sub(line.len());
            line.extend_from_slice(&buf[..take.min(room)]);
            self.consume(take);
            n += take;
            if done {
                return Ok(n);
            }
        }
    }

    /// The next piece of SGF with the byte it starts at; what is no SGF is skipped. `keep`:
    /// whether a property's values are wanted; when not, each is an empty one (a pass over the
    /// games reads only their first node's).
    pub fn sgf(&mut self, keep: bool) -> io::Result<Option<(u64, Sgf)>> {
        loop {
            let at = self.at;
            let Some(c) = self.peek()? else { return Ok(None) };
            match c {
                b'(' | b')' | b';' => {
                    self.consume(1);
                    let tok = match c {
                        b'(' => Sgf::Open,
                        b')' => Sgf::Close,
                        _ => Sgf::Node,
                    };
                    return Ok(Some((at, tok)));
                }
                c if c.is_ascii_uppercase() => {
                    let mut name = String::new();
                    while let Some(c) = self.peek()?
                        && c.is_ascii_alphabetic()
                    {
                        if c.is_ascii_uppercase() && name.len() < 8 {
                            name.push(c as char);
                        }
                        self.consume(1);
                    }
                    let (mut values, mut kept) = (vec![], 0usize);
                    loop {
                        while self.peek()?.is_some_and(|c| c.is_ascii_whitespace()) {
                            self.consume(1);
                        }
                        if self.peek()? != Some(b'[') {
                            break;
                        }
                        self.consume(1);
                        // Read on past the rest: a 52 × 52 board's setup needs 2704.
                        let room = if keep && values.len() < MAX_VALUES { MAX_VALUE.min(MAX_PROP - kept) } else { 0 };
                        let v = self.value(room)?;
                        kept += v.len();
                        if values.len() < MAX_VALUES {
                            values.push(v);
                        }
                    }
                    if values.is_empty() {
                        continue;
                    }
                    return Ok(Some((at, Sgf::Prop(name, values))));
                }
                _ => {
                    // Skipped at once up to the next byte that may start a piece.
                    let buf = self.fill()?;
                    let n = buf.iter().position(|&c| matches!(c, b'(' | b')' | b';' | b'A'..=b'Z')).unwrap_or(buf.len()).max(1);
                    self.consume(n);
                }
            }
        }
    }

    /// A value after its `[`, up to its `]` (read past), at most `room` bytes of it kept.
    fn value(&mut self, room: usize) -> io::Result<Vec<u8>> {
        let mut v = vec![];
        let mut escaped = false;
        loop {
            let buf = self.fill()?;
            if buf.is_empty() {
                return Ok(v);
            }
            let mut end = None;
            for (i, &c) in buf.iter().enumerate() {
                if escaped {
                    escaped = false;
                } else if c == b'\\' {
                    escaped = true;
                } else if c == b']' {
                    end = Some(i);
                    break;
                }
            }
            let take = end.unwrap_or(buf.len());
            let keep = take.min(room.saturating_sub(v.len()));
            v.extend_from_slice(&buf[..keep]);
            self.consume(take + end.is_some() as usize);
            if end.is_some() {
                return Ok(v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_pieces_with_their_bytes() {
        // A tiny buffer: lines and values across its edges.
        let text = b"[Event \"x\"]\r\n1.e4 *\nlast";
        let mut l = Lexer::new(io::BufReader::with_capacity(3, &text[..]));
        let mut line = vec![];
        let mut seen = vec![];
        loop {
            let at = l.at;
            if l.line(&mut line).unwrap() == 0 {
                break;
            }
            seen.push((at, String::from_utf8(line.clone()).unwrap()));
        }
        assert_eq!(seen, [(0, "[Event \"x\"]\r\n".into()), (13, "1.e4 *\n".into()), (20, "last".into())]);
        let sgf = b"x(;FF[4]AddBlack[aa] [b\\]b]C[a\\\\];B[cc])";
        let mut l = Lexer::new(io::BufReader::with_capacity(2, &sgf[..]));
        let mut toks = vec![];
        while let Some(t) = l.sgf(true).unwrap() {
            toks.push(t);
        }
        assert_eq!(
            toks,
            [
                (1, Sgf::Open),
                (2, Sgf::Node),
                (3, Sgf::Prop("FF".into(), vec![b"4".to_vec()])),
                (8, Sgf::Prop("AB".into(), vec![b"aa".to_vec(), b"b\\]b".to_vec()])),
                (27, Sgf::Prop("C".into(), vec![b"a\\\\".to_vec()])),
                (33, Sgf::Node),
                (34, Sgf::Prop("B".into(), vec![b"cc".to_vec()])),
                (39, Sgf::Close),
            ]
        );
        // Values not wanted: empty, as many.
        let mut l = Lexer::new(&b"(;AB[aa][bb]"[..]);
        let last = std::iter::from_fn(|| l.sgf(false).unwrap()).last().unwrap();
        assert_eq!(last.1, Sgf::Prop("AB".into(), vec![vec![], vec![]]));
    }

    /// An endless line, an endless value, endless values: only so much kept.
    #[test]
    fn endless_input_stays_small() {
        let mut l = Lexer::new(io::BufReader::new(io::Read::take(io::repeat(b'x'), 3 * MAX_LINE as u64)));
        let mut line = vec![];
        assert_eq!(l.line(&mut line).unwrap(), 3 * MAX_LINE);
        assert_eq!(line.len(), MAX_LINE);
        let big = [&b"(;C["[..], &vec![b'a'; 4 * MAX_VALUE], b"]AB", &b"[aa]".repeat(1_000_000), b")"].concat();
        let toks: Vec<_> = std::iter::from_fn({
            let mut l = Lexer::new(&big[..]);
            move || l.sgf(true).unwrap()
        })
        .collect();
        let Sgf::Prop(_, c) = &toks[2].1 else { panic!() };
        assert_eq!(c[0].len(), MAX_VALUE);
        let Sgf::Prop(_, ab) = &toks[3].1 else { panic!() };
        assert_eq!(ab.len(), MAX_VALUES);
        assert_eq!(toks.last().unwrap().1, Sgf::Close);
    }
}
