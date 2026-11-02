//! Pick mode (`coxswain --pick`): other programs use Coxswain to choose files or folders. The
//! paths chosen go to a file (`--pick=FILE`) or to standard output, one per line; the exit code
//! says whether something was chosen. The apps ask `Request::judge` what Enter means.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::config::Action;
use crate::t;

/// What is asked for, from the command line after `--pick`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    /// Folders are chosen, not files.
    pub folder: bool,
    /// More than one may be chosen (the marked ones).
    pub multiple: bool,
    /// Extensions a file must have (lower case, no dot, `pgn.gz` too); empty: any.
    pub filter: Vec<String>,
    pub start: Option<PathBuf>,
    pub title: Option<String>,
    /// Where the paths are written; none: standard output.
    pub out: Option<PathBuf>,
}

/// What Enter does in pick mode.
#[derive(Debug, PartialEq)]
pub enum Verdict {
    /// These are chosen.
    Choose(Vec<PathBuf>),
    /// The entry under the cursor is opened as a folder, as usual.
    Enter,
    /// Nothing is chosen; the key of the status line text that says why.
    Refuse(&'static str),
}

impl Request {
    /// The flags after `--pick[=FILE]`.
    pub fn parse(out: Option<&str>, args: &[String]) -> Result<Request, String> {
        let mut r = Request { out: out.filter(|o| !o.is_empty()).map(PathBuf::from), ..Request::default() };
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut value = |name: &str| it.next().cloned().ok_or_else(|| t!("pick.needs_value", "option" => name));
            match a.as_str() {
                "--folder" => r.folder = true,
                "--multiple" => r.multiple = true,
                "--filter" => r.filter = extensions(&value(a)?),
                "--start" => r.start = Some(PathBuf::from(value(a)?)),
                "--title" => r.title = Some(value(a)?),
                other => return Err(t!("pick.unknown", "option" => other)),
            }
        }
        Ok(r)
    }

    /// Whether a file called `name` is one asked for.
    pub fn accepts(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        self.filter.is_empty() || self.filter.iter().any(|e| name.strip_suffix(e.as_str()).is_some_and(|rest| rest.ends_with('.')))
    }

    /// Enter: `marked` and `cursor` are (path, is a folder); `goes_in` says the cursor entry is
    /// one Enter opens as a folder (an archive, a disk image). Marked entries win over the cursor.
    pub fn judge(&self, marked: &[(PathBuf, bool)], cursor: Option<(PathBuf, bool)>, goes_in: bool) -> Verdict {
        let fits = |(p, dir): &(PathBuf, bool)| {
            if self.folder { *dir } else { !*dir && self.accepts(&p.file_name().unwrap_or_default().to_string_lossy()) }
        };
        let list: Vec<(PathBuf, bool)> = if marked.is_empty() { cursor.iter().cloned().collect() } else { marked.to_vec() };
        if marked.is_empty() && cursor.as_ref().is_some_and(|c| goes_in && !(self.folder && c.1)) {
            return Verdict::Enter;
        }
        let Some(first) = list.first() else { return Verdict::Refuse("pick.nothing") };
        if marked.is_empty() && !fits(first) {
            // A folder in file mode is opened; a file in folder mode is not a choice.
            return if first.1 { Verdict::Enter } else { Verdict::Refuse(if self.folder { "pick.not_folder" } else { "pick.not_wanted" }) };
        }
        let chosen: Vec<PathBuf> = list.iter().filter(|e| fits(e)).map(|(p, _)| p.clone()).collect();
        if chosen.len() != list.len() {
            return Verdict::Refuse(if self.folder { "pick.not_folder" } else { "pick.not_wanted" });
        }
        if chosen.len() > 1 && !self.multiple {
            return Verdict::Refuse("pick.one_only");
        }
        Verdict::Choose(chosen)
    }

    /// Write the paths, one per line, UTF-8, to the file, or to `stdout` (the program's own
    /// standard output, kept apart from the terminal the panels are drawn on).
    pub fn write(&self, paths: &[PathBuf], stdout: &mut dyn Write) -> io::Result<()> {
        let text: String = paths.iter().map(|p| format!("{}\n", p.to_string_lossy())).collect();
        match &self.out {
            Some(f) => std::fs::write(f, text),
            None => stdout.write_all(text.as_bytes()).and_then(|()| stdout.flush()),
        }
    }
}

/// `Game databases:pgn,pgn.gz` or `pgn,.pgn`: the extensions, lower case, without dots.
fn extensions(s: &str) -> Vec<String> {
    let list = s.rsplit_once(':').map_or(s, |(_, l)| l);
    list.split(',').map(|e| e.trim().trim_start_matches("*.").trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()).collect()
}

/// Whether `a` changes files, runs a command or writes settings: a picker only chooses, so
/// pick mode refuses these from every way to reach them (keys, the action menu, F9).
pub fn changes_files(a: Action) -> bool {
    use Action::*;
    matches!(
        a,
        Copy | Move | Rename | Delete | Undo | NewFolder | ClipCut | Paste | DeleteForever | CompareFolders | BatchRename | Replace
            | Attrs | Tag | Snapshots | Flags | Package | BinRestore | BinDelete | BinEmpty | Extract | Pack | DiskImages
            | SearchablePdf | ConvertMedia | EditPictures | ConvertGeodata | ExportModel | ConvertEbook | ExportKicad
            | MakeChecksums | SplitFile | JoinParts | AdocPdf | DrawioExport | ScanViruses | Duplicates | Transcribe
            | SwitchBranch | NewBranch | Fetch | Pull | GitTool | GitConfig | Properties | Edit | EditHere | CommandLine
            | UserMenu | Notes | Settings | SuggestTranslation
    )
}

/// A file inside an archive is unpacked to the cache, and that path is what is chosen; any other
/// path is returned as it is. The cache folder is the picker's own, left for the caller to read.
pub fn on_disk(path: &Path) -> io::Result<PathBuf> {
    let Some((archive, inner)) = crate::archive::split(path).filter(|(_, i)| !i.is_empty()) else { return Ok(path.to_path_buf()) };
    let base = crate::dirs::cache_dir().ok_or_else(|| io::Error::other("no cache folder"))?.join("picked");
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dest = base.join(format!("{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dest)?;
    crate::archive::copy_out(&archive, &inner, &dest, None, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flags_are_read() {
        let r = Request::parse(Some("out.txt"), &args(&["--folder", "--multiple", "--filter", "Games:PGN,.pgn.gz", "--start", "/x", "--title", "T"])).unwrap();
        assert!(r.folder && r.multiple && r.out == Some("out.txt".into()) && r.start == Some("/x".into()) && r.title.as_deref() == Some("T"));
        assert_eq!(r.filter, ["pgn", "pgn.gz"]);
        assert!(Request::parse(None, &args(&["--nope"])).is_err() && Request::parse(None, &args(&["--filter"])).is_err());
        assert!(Request::parse(Some(""), &[]).unwrap().out.is_none());
    }

    #[test]
    fn the_filter_matches_whole_extensions() {
        let r = Request::parse(None, &args(&["--filter", "pgn,pgn.gz"])).unwrap();
        assert!(r.accepts("a.PGN") && r.accepts("a.pgn.gz") && !r.accepts("apgn") && !r.accepts("a.gz") && !r.accepts("a.pgn.bak"));
    }

    #[test]
    fn enter_chooses_or_opens() {
        let file = |n: &str| (PathBuf::from(n), false);
        let dir = |n: &str| (PathBuf::from(n), true);
        let files = Request::default();
        assert_eq!(files.judge(&[], Some(file("/a")), false), Verdict::Choose(vec!["/a".into()]));
        assert_eq!(files.judge(&[], Some(dir("/d")), false), Verdict::Enter);
        assert_eq!(files.judge(&[], Some(file("/z.zip")), true), Verdict::Enter);
        assert_eq!(files.judge(&[file("/a"), file("/b")], None, false), Verdict::Refuse("pick.one_only"));
        assert_eq!(files.judge(&[], None, false), Verdict::Refuse("pick.nothing"));
        let many = Request { multiple: true, ..Request::default() };
        assert_eq!(many.judge(&[file("/a"), file("/b")], Some(dir("/d")), false), Verdict::Choose(vec!["/a".into(), "/b".into()]));
        assert_eq!(many.judge(&[file("/a"), dir("/d")], None, false), Verdict::Refuse("pick.not_wanted"));
        let only_pgn = Request { filter: vec!["pgn".into()], ..Request::default() };
        assert_eq!(only_pgn.judge(&[], Some(file("/a.txt")), false), Verdict::Refuse("pick.not_wanted"));
        let folders = Request { folder: true, ..Request::default() };
        assert_eq!(folders.judge(&[], Some(dir("/d")), false), Verdict::Choose(vec!["/d".into()]));
        assert_eq!(folders.judge(&[], Some(file("/a")), false), Verdict::Refuse("pick.not_folder"));
        assert_eq!(folders.judge(&[], Some(file("/z.zip")), true), Verdict::Enter);
    }

    #[test]
    fn the_paths_are_written_one_per_line() {
        let dir = std::env::temp_dir().join(format!("coxswain-test-pick-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out.txt");
        let r = Request { out: Some(out.clone()), ..Request::default() };
        r.write(&["/a b".into(), "/ü".into()], &mut Vec::new()).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "/a b\n/ü\n");
        let _ = std::fs::remove_dir_all(&dir);
        // Without a file, the paths go to the stream given (the program's own standard output).
        let mut piped = Vec::new();
        Request::default().write(&["/x".into(), "/y z".into()], &mut piped).unwrap();
        assert_eq!(String::from_utf8(piped).unwrap(), "/x\n/y z\n");
    }

    #[test]
    fn a_picker_changes_no_files() {
        use Action::*;
        for a in [Copy, Move, Rename, Delete, DeleteForever, Paste, NewFolder, Replace, BinDelete, BinEmpty, Edit, CommandLine, Pack, Extract, Settings, Undo] {
            assert!(changes_files(a), "{a:?}");
        }
        for a in [Open, Parent, Mark, MarkAll, View, Quit, Help, Menu, Refresh, SortName, TogglePreview, Search, CopyPath, ClipCopy] {
            assert!(!changes_files(a), "{a:?}");
        }
    }
}
