//! Git status via `git status --porcelain=v2`, the stable machine format.

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Glyphs;

/// Ordered by how loudly it should show; a directory takes the loudest of its contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Ignored,
    Untracked,
    Added,
    Renamed,
    Deleted,
    Modified,
    Conflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct FileStatus {
    pub kind: Kind,
    /// The change is in the index (at least partly).
    pub staged: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Summary {
    pub branch: String,
    /// Short commit id when HEAD is detached.
    pub detached: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub modified: u32,
    pub deleted: u32,
    pub untracked: u32,
    pub conflicts: u32,
    pub stash: u32,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub root: PathBuf,
    pub summary: Summary,
    /// Absolute path -> status, for files and (aggregated) their parent directories.
    pub files: HashMap<PathBuf, FileStatus>,
}

impl Status {
    /// Run git in `dir`. `None` when `dir` is not inside a work tree or git is missing.
    pub fn read(dir: &Path) -> Option<Status> {
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .env("GIT_OPTIONAL_LOCKS", "0")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| o.stdout)
        };
        let root = git(&["rev-parse", "--show-toplevel"])?;
        let root = PathBuf::from(String::from_utf8_lossy(&root).trim_end());
        let out = git(&["status", "--porcelain=v2", "--branch", "--show-stash", "-z", "-unormal", "--ignored=matching"])?;
        Some(Status::parse(&root, &String::from_utf8_lossy(&out)))
    }

    pub fn parse(root: &Path, out: &str) -> Status {
        let mut st = Status { root: root.to_path_buf(), ..Default::default() };
        let s = &mut st.summary;
        let mut records = out.split('\0');
        while let Some(rec) = records.next() {
            let mut f = rec.splitn(2, ' ');
            let (tag, rest) = (f.next().unwrap_or(""), f.next().unwrap_or(""));
            let (xy, path) = match tag {
                "#" => {
                    let (k, v) = rest.split_once(' ').unwrap_or((rest, ""));
                    match k {
                        "branch.head" => s.branch = v.into(),
                        "branch.oid" => s.detached = Some(v.chars().take(7).collect()),
                        "branch.upstream" => s.upstream = Some(v.into()),
                        "branch.ab" => {
                            let mut ab = v.split(' ').map(|n| n[1..].parse().unwrap_or(0));
                            (s.ahead, s.behind) = (ab.next().unwrap_or(0), ab.next().unwrap_or(0));
                        }
                        "stash" => s.stash = v.parse().unwrap_or(0),
                        _ => {}
                    }
                    continue;
                }
                // Ordinary: XY sub mH mI mW hH hI path
                "1" => (&rest[..2], rest.splitn(8, ' ').nth(7)),
                // Renamed: ... Xscore path, then the original path as its own record.
                "2" => {
                    records.next();
                    (&rest[..2], rest.splitn(9, ' ').nth(8))
                }
                // Unmerged: XY sub m1 m2 m3 mW h1 h2 h3 path
                "u" => ("UU", rest.splitn(10, ' ').nth(9)),
                "?" => ("??", Some(rest)),
                "!" => ("!!", Some(rest)),
                _ => continue,
            };
            let Some(path) = path else { continue };
            let (x, y) = (xy.as_bytes()[0], xy.as_bytes()[1]);
            let kind = match (tag, x, y) {
                ("u", ..) => Kind::Conflict,
                ("?", ..) => Kind::Untracked,
                ("!", ..) => Kind::Ignored,
                (_, _, b'D') | (_, b'D', b'.') => Kind::Deleted,
                (_, _, b'M' | b'T') => Kind::Modified,
                (_, b'A', _) => Kind::Added,
                (_, b'R' | b'C', _) => Kind::Renamed,
                _ => Kind::Modified,
            };
            let staged = matches!(tag, "1" | "2") && x != b'.';
            match kind {
                Kind::Conflict => s.conflicts += 1,
                Kind::Untracked => s.untracked += 1,
                Kind::Ignored => {}
                _ => {
                    s.staged += staged as u32;
                    s.modified += (y == b'M' || y == b'T') as u32;
                    s.deleted += (y == b'D') as u32;
                }
            }
            // Untracked/ignored directories are reported as "dir/".
            let abs = root.join(path.trim_end_matches('/'));
            st.files.insert(abs.clone(), FileStatus { kind, staged });
            if kind != Kind::Ignored {
                for dir in abs.ancestors().skip(1).take_while(|d| d.starts_with(root) && *d != root) {
                    let e = st.files.entry(dir.to_path_buf()).or_insert(FileStatus { kind, staged });
                    e.kind = e.kind.max(kind);
                    e.staged |= staged;
                }
            }
        }
        st
    }

    /// Status of a path, inheriting from an untracked or ignored ancestor directory.
    pub fn get(&self, path: &Path) -> Option<FileStatus> {
        if let Some(s) = self.files.get(path) {
            return Some(*s);
        }
        path.ancestors()
            .skip(1)
            .take_while(|d| d.starts_with(&self.root))
            .find_map(|d| self.files.get(d).filter(|s| matches!(s.kind, Kind::Untracked | Kind::Ignored)))
            .copied()
    }

    /// One-line, oh-my-posh style: ` main ↑1 ↓2  3  1 ?4`.
    pub fn prompt(&self, g: &Glyphs) -> String {
        let s = &self.summary;
        let head = match (&s.detached, s.branch.as_str()) {
            (Some(oid), "(detached)") => oid.clone(),
            _ => s.branch.clone(),
        };
        let mut out = format!("{} {head}", g.branch);
        for (n, glyph) in [
            (s.ahead, &g.ahead),
            (s.behind, &g.behind),
            (s.staged, &g.staged),
            (s.modified, &g.modified),
            (s.deleted, &g.deleted),
            (s.untracked, &g.untracked),
            (s.conflicts, &g.conflict),
            (s.stash, &g.stash),
        ] {
            if n > 0 {
                out += &format!(" {glyph}{n}");
            }
        }
        if s.staged + s.modified + s.deleted + s.untracked + s.conflicts == 0 {
            out += &format!(" {}", g.clean);
        }
        out
    }
}

impl Kind {
    pub fn glyph(self, g: &Glyphs) -> &str {
        match self {
            Kind::Ignored => &g.ignored,
            Kind::Untracked => &g.untracked,
            Kind::Added => &g.staged,
            Kind::Renamed => &g.renamed,
            Kind::Deleted => &g.deleted,
            Kind::Modified => &g.modified,
            Kind::Conflict => &g.conflict,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = concat!(
        "# branch.oid 0123456789abcdef\0",
        "# branch.head main\0",
        "# branch.upstream origin/main\0",
        "# branch.ab +2 -1\0",
        "# stash 3\0",
        "1 .M N... 100644 100644 100644 aaa bbb src/lib.rs\0",
        "1 A. N... 000000 100644 100644 aaa bbb src/new file.rs\0",
        "2 R. N... 100644 100644 100644 aaa bbb R100 docs/b.md\0docs/a.md\0",
        "u UU N... 100644 100644 100644 100644 aaa bbb ccc x.txt\0",
        "? notes/\0",
        "! target/\0",
    );

    #[test]
    fn git_parse_porcelain_v2() {
        let root = Path::new("/r");
        let st = Status::parse(root, OUT);
        let s = &st.summary;
        assert_eq!((s.branch.as_str(), s.ahead, s.behind, s.stash), ("main", 2, 1, 3));
        assert_eq!((s.staged, s.modified, s.untracked, s.conflicts), (2, 1, 1, 1));
        let k = |p: &str| st.get(&root.join(p)).map(|f| f.kind);
        assert_eq!(k("src/lib.rs"), Some(Kind::Modified));
        assert_eq!(k("src/new file.rs"), Some(Kind::Added));
        assert_eq!(k("src"), Some(Kind::Modified));
        assert_eq!(k("docs/b.md"), Some(Kind::Renamed));
        assert_eq!(k("docs/a.md"), None);
        assert_eq!(k("x.txt"), Some(Kind::Conflict));
        assert_eq!(k("notes/deep/f"), Some(Kind::Untracked));
        assert_eq!(k("target/debug/x"), Some(Kind::Ignored));
        assert_eq!(k("README.md"), None);
        assert!(st.get(&root.join("src/new file.rs")).unwrap().staged);
        assert_eq!(st.prompt(&Glyphs::ascii()), "git: main ^2 v1 +2 ~1 ?1 !1 $3");
    }

    #[test]
    fn git_read_this_repo() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        if let Some(st) = Status::read(here) {
            assert!(here.starts_with(&st.root));
        }
        assert!(Status::read(Path::new("/")).is_none());
    }
}
