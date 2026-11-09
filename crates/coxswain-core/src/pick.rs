//! Pick mode (`coxswain --pick`): other programs use Coxswain to choose files or folders. The
//! paths chosen go to a file (`--pick=FILE`) or to standard output, one per line; the exit code
//! says whether something was chosen. The apps ask `Request::judge` what Enter means.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::config::Action;
use crate::t;

/// The key of *Show all* in pick mode, in both apps: free in the default keys (a test holds
/// it to that), where Alt and a letter would start a quick search.
pub const SHOW_ALL_KEY: &str = "Alt+E";

/// What is asked for, from the command line after `--pick`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
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
    /// `--save NAME`: a new file's path is asked for, with this name to start from.
    pub save: Option<String>,
    /// *Show all* is on: the filter is lifted, any file may be chosen and none is dimmed.
    pub show_all: bool,
}

/// What *Save* does with the name typed, in the folder shown.
#[derive(Debug, PartialEq)]
pub enum Save {
    /// This path is chosen.
    Choose(PathBuf),
    /// A file of that name is there: asked first whether it may be replaced.
    Ask(PathBuf),
    /// Nothing is chosen; the key of the status line text that says why.
    Refuse(&'static str),
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
                "--save" => r.save = Some(value(a)?),
                other => return Err(t!("pick.unknown", "option" => other)),
            }
        }
        if r.save.is_some() && (r.folder || r.multiple) {
            return Err(t!("pick.save_alone"));
        }
        Ok(r)
    }

    /// Whether a file called `name` is one asked for.
    pub fn accepts(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        self.show_all || self.filter.is_empty() || self.filter.iter().any(|e| name.strip_suffix(e.as_str()).is_some_and(|rest| rest.ends_with('.')))
    }

    /// The status line: the program's own title, or what is asked for, with the filter.
    pub fn ask(&self) -> String {
        let what = self.title.clone().or_else(|| self.save.as_ref().map(|n| t!("pick.ask_save", "name" => n))).unwrap_or_else(|| {
            t!(match (self.folder, self.multiple) {
                (false, false) => "pick.ask_file",
                (false, true) => "pick.ask_files",
                (true, false) => "pick.ask_folder",
                (true, true) => "pick.ask_folders",
            })
        });
        if self.filter.is_empty() || self.show_all { what } else { format!("{what} ({})", self.filter.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(" ")) }
    }

    /// Whether the entry is shown dimmed: a file the filter does not take.
    pub fn dims(&self, name: &str, is_dir: bool) -> bool {
        !is_dir && !self.accepts(name)
    }

    /// *Save* with `name` typed in the folder `dir`; `replace`: the question was answered yes.
    pub fn save_to(&self, dir: &Path, name: &str, replace: bool) -> Save {
        let name = name.trim();
        if name.is_empty() {
            return Save::Refuse("pick.save_no_name");
        }
        if name == "." || name == ".." || name.contains(['/', '\\']) {
            return Save::Refuse("pick.save_bad_name");
        }
        // The program that asked writes the file: inside an archive there is no place for it.
        if crate::archive::split(dir).is_some() || !dir.is_dir() {
            return Save::Refuse("pick.save_not_here");
        }
        let path = dir.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if path.is_dir() || m.is_dir() => Save::Refuse("pick.save_is_folder"),
            Ok(_) if !replace => Save::Ask(path),
            _ => Save::Choose(path),
        }
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
        // A folder inside an archive has no place on the disk to give.
        if self.folder && chosen.iter().any(|p| crate::archive::split(p).is_some()) {
            return Verdict::Refuse("pick.nothing");
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

/// Standard output is kept for the paths alone: it becomes `to` (the terminal, or the null
/// device for the desktop app), and the stream it was is returned, not inherited by programs
/// started later (a helper holding the caller's pipe open would keep `$(…)` waiting).
pub fn take_stdout(to: std::fs::File) -> io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        // SAFETY: fd 1 is duplicated, close-on-exec, then pointed at `to`; the copy is owned by the File.
        let saved = unsafe { libc::fcntl(1, libc::F_DUPFD_CLOEXEC, 3) };
        if saved < 0 || unsafe { libc::dup2(to.as_raw_fd(), 1) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { std::fs::File::from_raw_fd(saved) })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{FromRawHandle, IntoRawHandle};
        use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
        use windows_sys::Win32::System::Console::{GetStdHandle, SetStdHandle, STD_OUTPUT_HANDLE};
        // SAFETY: the old standard output handle becomes the File's, not inherited; `to` replaces it.
        let old = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        if unsafe { SetStdHandle(STD_OUTPUT_HANDLE, to.into_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // Started with no standard output (from Explorer): the paths go nowhere, as they would.
        if old.is_null() || old == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return std::fs::OpenOptions::new().write(true).open("NUL");
        }
        unsafe { SetHandleInformation(old, HANDLE_FLAG_INHERIT, 0) };
        Ok(unsafe { std::fs::File::from_raw_handle(old) })
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
            | UserMenu | Notes | Settings | SuggestTranslation | DiskUse | PreviewWith
    )
}

/// A file inside an archive is unpacked to the cache (`picked/<run>/<n>/`), and that path is what
/// is chosen; any other path is returned as it is. Left for the caller to read: a run's copies go
/// a day after it ended (`archive::run_folder`, and at each start), or by Disk use's *Clear*.
pub fn on_disk(path: &Path) -> io::Result<PathBuf> {
    let Some((archive, inner)) = crate::archive::split(path).filter(|(_, i)| !i.is_empty()) else { return Ok(path.to_path_buf()) };
    // A folder each, so two chosen files of the same name both stay.
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dest = crate::archive::run_folder(PICKED, false)?.join(N.fetch_add(1, std::sync::atomic::Ordering::Relaxed).to_string());
    std::fs::create_dir_all(&dest)?;
    crate::archive::copy_out(&archive, &inner, &dest, None, false)
}

/// The cache's folder of files chosen inside archives.
pub(crate) const PICKED: &str = "picked";

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
        assert_eq!(Request::parse(None, &args(&["--save", "game.pgn"])).unwrap().save.as_deref(), Some("game.pgn"));
        assert!(Request::parse(None, &args(&["--save", "a", "--folder"])).is_err() && Request::parse(None, &args(&["--multiple", "--save", "a"])).is_err());
    }

    #[test]
    fn show_all_has_a_key_of_its_own() {
        let keys = crate::config::Config::default().keymap().unwrap();
        assert!(keys.keys().all(|k| !k.to_string().eq_ignore_ascii_case(SHOW_ALL_KEY)), "{SHOW_ALL_KEY} is taken");
    }

    #[test]
    fn show_all_lifts_the_filter_and_the_dimming() {
        let mut r = Request::parse(None, &args(&["--filter", "pgn"])).unwrap();
        assert!(r.dims("a.txt", false) && !r.dims("a.pgn", false) && !r.dims("txt", true));
        assert_eq!(r.judge(&[], Some(("/a.txt".into(), false)), false), Verdict::Refuse("pick.not_wanted"));
        r.show_all = true;
        assert!(!r.dims("a.txt", false));
        assert_eq!(r.judge(&[], Some(("/a.txt".into(), false)), false), Verdict::Choose(vec!["/a.txt".into()]));
    }

    #[test]
    fn save_asks_before_a_file_is_replaced() {
        let dir = std::env::temp_dir().join(format!("coxswain-test-pick-save-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("old.pgn"), "x").unwrap();
        let r = Request { save: Some("new.pgn".into()), ..Request::default() };
        assert_eq!(r.save_to(&dir, " new.pgn ", false), Save::Choose(dir.join("new.pgn")));
        assert_eq!(r.save_to(&dir, "old.pgn", false), Save::Ask(dir.join("old.pgn")));
        assert_eq!(r.save_to(&dir, "old.pgn", true), Save::Choose(dir.join("old.pgn")));
        // A folder is never given as the file, even when "replace" was said.
        assert_eq!(r.save_to(&dir, "sub", true), Save::Refuse("pick.save_is_folder"));
        assert_eq!(r.save_to(&dir, "", false), Save::Refuse("pick.save_no_name"));
        for bad in ["a/b", "a\\b", "..", "."] {
            assert_eq!(r.save_to(&dir, bad, false), Save::Refuse("pick.save_bad_name"), "{bad}");
        }
        assert_eq!(r.save_to(&dir.join("nowhere"), "a", false), Save::Refuse("pick.save_not_here"));
        assert_eq!(std::fs::read_to_string(dir.join("old.pgn")).unwrap(), "x", "nothing is written");
        assert!(!dir.join("new.pgn").exists());
        let _ = std::fs::remove_dir_all(&dir);
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

    /// Two files of one name chosen inside an archive both come back, each in its own folder
    /// under this run's in the cache's `picked/`, which Disk use lists and the start sweeps.
    #[test]
    fn files_in_archives_land_in_the_runs_picked_folder() {
        let d = std::env::temp_dir().join(format!("coxswain-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let zip = d.join("two.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&zip).unwrap());
        for (name, text) in [("a/x.txt", b"one"), ("b/x.txt", b"two")] {
            z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(text).unwrap();
        }
        z.finish().unwrap();
        let (a, b) = (on_disk(&zip.join("a").join("x.txt")).unwrap(), on_disk(&zip.join("b").join("x.txt")).unwrap());
        assert_eq!((std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap()), (b"one".to_vec(), b"two".to_vec()));
        let run = crate::helper::folder().unwrap().join(PICKED).join(crate::spill::run());
        assert!(a.starts_with(&run) && b.starts_with(&run), "{a:?} {b:?}");
        // A plain file is chosen as it is.
        assert_eq!(on_disk(&zip).unwrap(), zip);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
