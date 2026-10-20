//! What old files hold, said the same way in both apps: an old picture's format, size and
//! colours, a tracker module's channels and samples, how many messages a mailbox has, how
//! many pages a comic, a dBase table's records and fields, what a local model holds
//! (`local_models::facts`). The desktop app shows them under
//! the preview, the terminal app in Properties (**Alt+Enter**); F3 in the terminal app pages
//! them as text (`text_copy`), with a module's sample names and an Outlook message's text.
//! A neural network model's facts come from `neural`.

use std::path::{Path, PathBuf};

use crate::t;

/// The facts of `path` as (label, value) lines; `None` for a file of none of these kinds, and
/// for one only in the cloud (reading it would download it).
pub fn of(path: &Path) -> Option<Vec<(String, String)>> {
    if crate::cloud::unasked(path) {
        return None;
    }
    // A model; a Core ML package is a folder.
    if crate::neural::named(path) {
        return crate::neural::read(path).map(|m| crate::neural::facts(&m));
    }
    if !path.is_file() {
        return None;
    }
    if crate::pictures::named(path) {
        return crate::pictures::facts(path);
    }
    if crate::music::named(path) {
        return crate::music::read(path).map(|m| crate::music::facts(&m));
    }
    if crate::archive::is_image(path) {
        return crate::container_image::facts(path);
    }
    if let Some(f) = crate::local_models::facts(path) {
        return Some(f);
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "mbox" => {
            let (list, more) = crate::archive::list(path, usize::MAX >> 1).ok()?;
            let n = if more { format!("{}+", list.len()) } else { list.len().to_string() };
            Some(vec![(t!("facts.messages"), n)])
        }
        "cbz" => Some(vec![(t!("facts.pages"), crate::archive::pages(path).ok()?.len().to_string())]),
        "dbf" => {
            let (_, tb) = crate::dbf::open(path)?;
            Some(vec![
                (t!("facts.format"), "dBase".into()),
                (t!("preview.db_rows"), tb.records.to_string()),
                (t!("facts.fields"), tb.fields.len().to_string()),
                (t!("facts.code_page"), tb.code_page.to_string()),
                (t!("facts.updated"), tb.updated.clone()),
            ])
        }
        _ => None,
    }
}

/// Facts as aligned lines of text.
fn lines(facts: &[(String, String)]) -> String {
    let w = facts.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    facts.iter().map(|(k, v)| format!("{k}{}  {v}\n", " ".repeat(w - k.chars().count()))).collect()
}

/// What F3 shows in the terminal app for an old picture, a tracker module or an Outlook
/// message: its facts (and a module's sample names, a message's headers, text and
/// attachments) as text. `None`: the file is none of these, the viewer shows it as it is.
pub fn as_text(path: &Path) -> Option<String> {
    text_and_format(path).map(|(text, _)| text)
}

/// `as_text`, and the file's format for the viewer's title ("Amiga IFF ILBM").
fn text_and_format(path: &Path) -> Option<(String, String)> {
    if crate::cloud::unasked(path) {
        return None;
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if crate::pictures::named(path) {
        let f = crate::pictures::facts(path)?;
        return Some((format!("{name}\n\n{}\n{}\n", lines(&f), t!("facts.picture_tui")), f[0].1.clone()));
    }
    if crate::music::named(path) {
        let m = crate::music::read(path)?;
        let format = m.format.clone();
        let mut out = format!("{name}\n\n{}", lines(&crate::music::facts(&m)));
        if !m.names.is_empty() {
            out += &format!("\n{}\n", t!("facts.sample_names"));
            for n in &m.names {
                out += &format!("  {n}\n");
            }
        }
        return Some((out, format));
    }
    if crate::neural::named(path) {
        let m = crate::neural::read(path)?;
        return Some((crate::neural::text(&name, &m), m.format.clone()));
    }
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("msg")) {
        let m = crate::extract::msg::summary(path)?;
        let mut head = vec![
            (t!("preview.subject"), m.subject),
            (t!("preview.from"), m.from),
            (t!("preview.to"), m.to),
            (t!("preview.date"), m.date),
        ];
        for (i, (n, size)) in m.attachments.iter().enumerate() {
            let label = if i == 0 { t!("preview.attachments") } else { String::new() };
            head.push((label, format!("{} ({})", n.clone().unwrap_or_else(|| t!("facts.unnamed")), crate::settings::human(*size as u64))));
        }
        return Some((format!("{}\n{}\n", lines(&head), m.text), "Outlook MSG".into()));
    }
    None
}

/// `as_text` in a file of the cache, for the viewer: F3, with the file's format for its title.
/// `None` as `as_text`.
pub fn text_copy(path: &Path) -> Option<Result<(PathBuf, String), String>> {
    let (text, format) = text_and_format(path)?;
    Some((|| {
        let to = crate::archive::peek_folder()
            .map_err(|e| e.to_string())?
            .join(format!("{}.txt", path.file_name().unwrap_or_default().to_string_lossy()));
        std::fs::write(&to, text).map_err(|e| e.to_string())?;
        Ok((to, format))
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_and_pages_of_old_files() {
        let d = crate::pictures::tests::folder("facts");
        let m = d.join("launch.mod");
        std::fs::write(&m, crate::music::tests::protracker_bytes()).unwrap();
        let f = of(&m).unwrap();
        assert!(f.iter().any(|(_, v)| v.starts_with("ProTracker")), "{f:?}");
        let text = as_text(&m).unwrap();
        assert!(text.contains("greetings to all"), "the sample names: {text}");
        // Modula-2 source named .mod is no module: no facts, the viewer shows it.
        let src = d.join("Hello.mod");
        std::fs::write(&src, "MODULE Hello;\nEND Hello.\n").unwrap();
        assert!(of(&src).is_none() && as_text(&src).is_none());
        let p = d.join("x.pcx");
        std::fs::write(&p, crate::pictures::pcx::tests::pcx8(2, 2, &[1, 2, 3, 4])).unwrap();
        assert!(as_text(&p).unwrap().contains("PCX"));
        // F3's title names the format, not "plain text".
        assert!(text_and_format(&p).unwrap().1.contains("PCX"));
        let dbf = d.join("fleet.dbf");
        std::fs::write(&dbf, crate::dbf::tests::table(&[(b"Ada", "1.00", "20261019", true)])).unwrap();
        assert_eq!(of(&dbf).unwrap()[1].1, "1");
        let msg = d.join("launch.msg");
        crate::extract::msg::tests::message(&msg);
        assert!(as_text(&msg).unwrap().contains("trajectory.pdf"));
        assert!(of(&d.join("notes.txt")).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }
}
