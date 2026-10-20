//! E-books in the terminal app (`coxswain_core::ebook`): F3 on a MOBI, Kindle, FictionBook, LIT
//! or Palm book shows its title, authors and text in the built-in viewer, and *Convert e-book*
//! (F9, the action menu) makes it an EPUB, AZW3 or PDF next to it. Both run calibre on a
//! thread, the status line saying how far it is; a first pull of its image is asked first.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use coxswain_core::config::Action;
use coxswain_core::ebook;
use coxswain_core::t;

use crate::{App, Dialog, MenuItem, MenuRun, Searchable, SearchableMsg, failure};

/// What is to be made of which books (`None`: shown in the viewer), and the question's text
/// with whether it pulls, or why it cannot run.
pub(crate) type EbookPlan = (Option<&'static str>, Vec<PathBuf>, Result<(String, bool), String>);

impl App {
    /// *Convert e-book* on the books among the marked entries (or the one under the cursor):
    /// the format from a menu. While one runs, the action stops it.
    pub(crate) fn ebook_menu(&mut self) {
        if let Some((s, ..)) = &self.ebook {
            s.stop.store(true, Ordering::Relaxed);
            return self.status = Some(t!("searchable.stopping"));
        }
        // A book only in the cloud is left out: it is not downloaded to be read.
        let books: Vec<PathBuf> = self.panel().targets().into_iter().filter(|p| ebook::is_ebook(p) && !coxswain_core::cloud::unasked(p) && p.is_file()).collect();
        if books.is_empty() {
            return self.status = Some(t!("ebook.none"));
        }
        let items = ebook::TO
            .iter()
            .enumerate()
            .map(|(i, &to)| MenuItem { key: (i + 1).to_string(), label: coxswain_core::i18n::tr(&format!("ebook.to_{to}"), &[]), run: MenuRun::Ebook(to, books.clone()), group: None })
            .collect();
        self.dialog = Some(Dialog::Menu { title: t!("action.convert_ebook"), filter: String::new(), items, cursor: 0, direct: true, feature: "ebooks" });
    }

    /// F3 on a book: its text in the viewer, made by calibre (once; then from the cache).
    pub(crate) fn ebook_view(&mut self, book: PathBuf) {
        if self.ebook.is_some() {
            return self.status = Some(t!("ebook.busy_already"));
        }
        self.ebook_ask(None, vec![book]);
    }

    /// The question, worked out on a thread (asking podman for the image takes a moment). A
    /// book to view whose calibre is ready is shown without one.
    pub(crate) fn ebook_ask(&mut self, to: Option<&'static str>, books: Vec<PathBuf>) {
        let (tx, rx) = mpsc::channel();
        let cfg = self.cfg.preview.clone();
        std::thread::spawn(move || {
            let books: Vec<PathBuf> = match to {
                Some(to) => books.into_iter().filter(|b| ebook::formats_for(b).contains(&to)).collect(),
                None => books,
            };
            let plan = ebook::how(&cfg).map(|how| {
                let (about, pull) = ebook::about(&how);
                let what = match (to, books.as_slice()) {
                    (None, _) => t!("ebook.view_first"),
                    (Some(to), [one]) => {
                        let name = one.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        let target = ebook::target(one, to).file_name().unwrap_or_default().to_string_lossy().into_owned();
                        t!("ebook.confirm_one", "name" => name, "target" => target)
                    }
                    (Some(to), many) => t!("ebook.confirm_many", "n" => many.len(), "format" => to.to_uppercase()),
                };
                (format!("{what} {} {about}", t!("ebook.no_drm")), pull)
            });
            let _ = tx.send((to, books, plan));
        });
        self.ebook_plan = Some(rx);
    }

    pub(crate) fn poll_ebook_plan(&mut self) {
        let Some(rx) = &self.ebook_plan else { return };
        let Ok((to, books, plan)) = rx.try_recv() else { return };
        self.ebook_plan = None;
        if books.is_empty() {
            return self.status = Some(t!("ebook.none"));
        }
        let title = t!("action.convert_ebook");
        self.dialog = Some(match plan {
            Ok((_, false)) if to.is_none() => return self.ebook_start(None, books),
            Ok((text, _)) => Dialog::Ebook { title, text, to, books },
            Err(e) => Dialog::Message { title, text: format!("{e}\n\n{}", t!("searchable.settings_key", "key" => self.key_label(Action::Settings))) },
        });
    }

    pub(crate) fn ebook_start(&mut self, to: Option<&'static str>, books: Vec<PathBuf>) {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (cfg, flag, list) = (self.cfg.preview.clone(), stop.clone(), books.clone());
        std::thread::spawn(move || {
            let (said, at) = (tx.clone(), tx.clone());
            let image = cfg.images.get(ebook::TOOL).cloned().unwrap_or_default();
            let mut say = |s: ebook::Said| {
                let line = match s {
                    ebook::Said::Pulling(l) => format!("{} {l}", t!("convert.pulling", "image" => image.as_str())),
                    ebook::Said::Line(l) => l.to_string(),
                };
                let _ = said.send(SearchableMsg::Said(line));
            };
            let done = match to {
                Some(to) => ebook::run_all(&cfg, to, &list, &flag, |i| drop(at.send(SearchableMsg::At(i))), say),
                None => vec![view(&cfg, &list[0], &mut say)],
            };
            let _ = tx.send(SearchableMsg::Done(done));
        });
        let names = books.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
        self.ebook = Some((Searchable { names, at: 0, line: String::new(), shown: String::new(), started: Instant::now(), stop, rx }, to, books));
        self.poll_ebook();
    }

    /// How far calibre is, on the status line; when done, what came of it: the book in the
    /// viewer, or the files made.
    pub(crate) fn poll_ebook(&mut self) {
        // No key of its own by default: F9 lists it, and stops it from there.
        let key = match self.key_label(Action::ConvertEbook) {
            "" => format!("{} → {}", self.key_label(Action::Menu), self.action_label(Action::ConvertEbook)),
            k => k.to_string(),
        };
        let Some((s, to, _)) = &mut self.ebook else { return };
        let converting = to.is_some();
        let mut done = None;
        while let Ok(msg) = s.rx.try_recv() {
            match msg {
                SearchableMsg::At(i) => (s.at, s.line) = (i, String::new()),
                SearchableMsg::Said(l) => s.line = l,
                SearchableMsg::Done(d) => done = Some(d),
            }
        }
        let Some(done) = done else {
            let (name, n, all, seconds) = (s.names.get(s.at).cloned().unwrap_or_default(), s.at + 1, s.names.len(), s.started.elapsed().as_secs());
            let busy = match (converting, all > 1) {
                (false, _) => t!("ebook.reading", "name" => name, "seconds" => seconds),
                (true, true) => t!("ebook.busy_of", "name" => name, "n" => n, "all" => all, "seconds" => seconds),
                (true, false) => t!("ebook.busy", "name" => name, "seconds" => seconds),
            };
            let stopping = s.stop.load(Ordering::Relaxed);
            let tail = match (converting, stopping) {
                (_, true) => format!(" · {}", t!("searchable.stopping")),
                (true, false) => format!(" · {}", t!("searchable.stop_key", "key" => key)),
                (false, false) => String::new(),
            };
            let line = if s.line.is_empty() { String::new() } else { format!(" · {}", s.line) };
            let new = format!("{busy}{line}{tail}");
            if self.status.is_none() || self.status.as_ref() == Some(&s.shown) {
                self.status = Some(new.clone());
            }
            s.shown = new;
            return;
        };
        let (s, to, books) = self.ebook.take().expect("a run");
        if self.status.as_ref() == Some(&s.shown) {
            self.status = None;
        }
        // The image was used: what *Remove images not used for 90 days* goes by.
        if let Ok(ebook::How::Container { image, .. }) = ebook::how(&self.cfg.preview) {
            let mut st = coxswain_core::state::AppState::load();
            coxswain_core::images::used(&mut st, &image, coxswain_core::fs::now());
            let _ = st.save();
        }
        if to.is_none() {
            return match (done.into_iter().next(), books.first()) {
                (Some(Ok(text)), Some(book)) => {
                    let format = book.extension().unwrap_or_default().to_string_lossy().to_uppercase();
                    self.view_copy(&text, &book.clone(), t!("ebook.kind", "format" => format))
                }
                (Some(Err(why)), _) => self.dialog = Some(Dialog::Message { title: t!("ebook.failed_view"), text: why }),
                _ => {}
            };
        }
        let stopped = s.stop.load(Ordering::Relaxed);
        let (made, failed): (Vec<_>, Vec<_>) = done.into_iter().zip(s.names.iter()).partition(|(r, _)| r.is_ok());
        let made: Vec<PathBuf> = made.into_iter().filter_map(|(r, _)| r.ok()).collect();
        self.reload();
        if stopped {
            return self.status = Some(t!("ebook.stopped"));
        }
        if !failed.is_empty() {
            let why = failed.into_iter().map(|(r, n)| format!("{n}: {}", r.err().unwrap_or_default())).collect::<Vec<_>>().join("\n");
            self.status = None;
            return self.dialog = Some(failure(t!("ebook.failed"), &why));
        }
        self.status = Some(match made.as_slice() {
            [one] => t!("ebook.done_one", "name" => one.file_name().unwrap_or_default().to_string_lossy()),
            many => t!("ebook.done_many", "n" => many.len()),
        });
    }
}

/// The book's text for the viewer: from the cache when calibre made its preview before.
fn view(cfg: &coxswain_core::config::PreviewConfig, book: &std::path::Path, said: &mut dyn FnMut(ebook::Said)) -> Result<PathBuf, String> {
    let how = ebook::how(cfg)?;
    let dir = ebook::cache_dir(book).ok_or_else(|| t!("err.no_cache_folder"))?;
    let p = match ebook::shown(&dir) {
        Some(p) => p,
        None => ebook::preview(&how, book, &dir, Duration::from_secs(cfg.timeout.max(10)), said)?,
    };
    ebook::text_copy(&p)
}
