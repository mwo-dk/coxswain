//! Plain text: no zero byte at its start, and UTF-8. Text, code, Markdown, logs,
//! configuration, and everything else that is written to be read as it is.

use std::io::Read;
use std::path::Path;

pub fn text(path: &Path, max: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 8192];
    let n = f.read(&mut head).ok()?;
    if head[..n].contains(&0) {
        return None;
    }
    let mut bytes = head[..n].to_vec();
    f.take(max).read_to_end(&mut bytes).ok()?;
    String::from_utf8(bytes).ok()
}
