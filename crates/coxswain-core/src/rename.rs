//! Regex batch rename: plan first (for the live preview), then apply.

use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct Flags {
    pub case_insensitive: bool,
    /// Replace every match, not only the first.
    pub global: bool,
    /// Match against the whole name; otherwise only the stem (the extension is kept).
    pub whole_name: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Planned {
    pub from: String,
    pub to: String,
    /// Why this rename cannot happen, if it cannot.
    pub conflict: Option<String>,
}

/// `name` as a name field shows it: a control character (a newline, a tab, an escape) as its
/// picture from the U+2400 block (`␊`, `␉`, `␛`, `␡`), a C1 one (U+0080 to U+009F, which has
/// no picture) as `␦`. A text field would drop a newline and a terminal would act on an escape;
/// one picture for each character, so [`typed`] knows which ones the user left alone.
pub fn shown(name: &str) -> String {
    name.chars()
        .map(|c| match c as u32 {
            n @ 0..=0x1f => char::from_u32(0x2400 + n).unwrap_or(c),
            0x7f => '\u{2421}',
            0x80..=0x9f => '\u{2426}',
            _ => c,
        })
        .collect()
}

/// A control character's picture as the character itself: `␊` typed is a newline.
fn real(c: char) -> char {
    match c as u32 {
        n @ 0x2400..=0x241f => char::from_u32(n - 0x2400).unwrap_or(c),
        0x2421 => '\u{7f}',
        _ => c,
    }
}

/// The name a field that showed [`shown`]`(name)` holds once `typed` is in it, or `None` when
/// the text was not changed: then nothing is renamed. What the user did not touch (the same
/// text before and after the edit) stays the characters it was, a C1 one or a `␊` that was in
/// the name too; in what was typed, a picture becomes its control character.
pub fn typed(name: &str, typed: &str) -> Option<String> {
    let (old, new): (Vec<char>, Vec<char>) = (shown(name).chars().collect(), typed.chars().collect());
    if old == new {
        return None;
    }
    let orig: Vec<char> = name.chars().collect();
    let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - head;
    let tail = old.iter().rev().zip(new.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
    let out: String = orig[..head]
        .iter()
        .copied()
        .chain(new[head..new.len() - tail].iter().map(|&c| real(c)))
        .chain(orig[orig.len() - tail..].iter().copied())
        .collect();
    (out != name).then_some(out)
}

/// Compute new names for `selected`. `existing` is every name in the directory.
///
/// `replacement` supports regex groups (`$1`, `${name}`) and a counter: `{n}` is the
/// 1-based position in `selected`, `{n:3}` zero-pads it to 3 digits (20 at most).
/// Conflicts: an empty result, a path separator, two files mapping to the same name, or a
/// name taken by a file that is not itself being renamed away.
pub fn plan(
    selected: &[String],
    existing: &[String],
    pattern: &str,
    replacement: &str,
    flags: Flags,
) -> Result<Vec<Planned>, String> {
    // A control character's picture typed in a field is that character, as in a name.
    let (pattern, replacement): (String, String) = (pattern.chars().map(real).collect(), replacement.chars().map(real).collect());
    let (pattern, replacement) = (pattern.as_str(), replacement.as_str());
    let re = regex::RegexBuilder::new(pattern)
        .case_insensitive(flags.case_insensitive)
        .build()
        .map_err(|e| e.to_string())?;
    let counter = regex::Regex::new(r"\{n(?::(\d+))?\}").expect("valid");
    let numbered = regex::Regex::new(r"\$(\d+)").expect("valid");
    let mut out: Vec<Planned> = selected
        .iter()
        .enumerate()
        .map(|(i, from)| {
            let rep = counter.replace_all(replacement, |c: &regex::Captures| {
                // At most 20 digits: `{n:999999999}` would pad each name to a gigabyte.
                let w = c.get(1).map_or(0, |m| m.as_str().parse().unwrap_or(0)).min(20);
                format!("{:0w$}", i + 1)
            });
            // `$2_x` would mean a group named "2_x"; users mean group 2, so brace it.
            let rep = numbered.replace_all(&rep, "$${${1}}");
            // The extension is kept unless matching the whole name. A leading dot is not one.
            let (stem, ext) = match from.rfind('.') {
                Some(d) if d > 0 && !flags.whole_name => (&from[..d], &from[d..]),
                _ => (from.as_str(), ""),
            };
            let stem = if flags.global {
                re.replace_all(stem, rep.as_ref())
            } else {
                re.replace(stem, rep.as_ref())
            };
            Planned {
                from: from.clone(),
                to: format!("{stem}{ext}"),
                conflict: None,
            }
        })
        .collect();

    // A name is free if nobody else ends up with it; files not selected keep theirs. Where the
    // disk does not mind the case, `A.txt` is `a.txt`: the rename would write over it.
    let key = |name: &str| {
        if cfg!(any(windows, target_os = "macos")) {
            name.to_lowercase()
        } else {
            name.to_string()
        }
    };
    let chosen: HashSet<&str> = selected.iter().map(String::as_str).collect();
    let mut taken: HashMap<String, usize> = HashMap::new();
    for name in existing.iter().filter(|n| !chosen.contains(n.as_str())) {
        *taken.entry(key(name)).or_default() += 1;
    }
    for p in &out {
        *taken.entry(key(&p.to)).or_default() += 1;
    }
    for p in &mut out {
        p.conflict = if p.to.is_empty() {
            Some(crate::t!("rename.conflict_empty"))
        } else if p.to.contains(['/', '\\']) {
            Some(crate::t!("rename.conflict_separator"))
        } else if cfg!(windows) && windows_refuses(&p.to) {
            Some(crate::t!("rename.conflict_windows"))
        } else if taken[&key(&p.to)] > 1 {
            Some(crate::t!("rename.conflict_taken"))
        } else {
            None
        };
    }
    Ok(out)
}

/// A name Windows will not give a file: one with `<>:"|?*` or a control character in it, one
/// ending in a dot or a space (Windows would drop them, so the file gets another name than the
/// one shown), or a device's name (`CON`, `nul.txt`, `COM1`).
fn windows_refuses(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end()
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    device
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control())
}

/// Rename in `dir` as planned. Refuses if any entry has a conflict. Uses temporary names so
/// swaps (a -> b, b -> a) work. Unchanged entries are skipped. When a rename fails, every file
/// gets its old name back: all of the batch is done, or none of it.
pub fn apply(dir: &Path, plan: &[Planned]) -> Result<(), String> {
    if let Some(p) = plan.iter().find(|p| p.conflict.is_some()) {
        return Err(format!(
            "{}: {}",
            p.from,
            p.conflict.as_deref().unwrap_or("")
        ));
    }
    let moves: Vec<&Planned> = plan.iter().filter(|p| p.from != p.to).collect();
    // The plan was made for the preview: a name that has appeared since is not overwritten.
    for p in &moves {
        let to = dir.join(&p.to);
        if std::fs::symlink_metadata(&to).is_ok()
            && !moves.iter().any(|q| q.from == p.to)
            && !crate::fs::same_file(&dir.join(&p.from), &to)
        {
            return Err(format!(
                "{} -> {}: {}",
                p.from,
                p.to,
                crate::t!("rename.conflict_taken")
            ));
        }
    }
    let tmp = |i: usize| dir.join(format!(".coxswain-rename-{}-{i}", std::process::id()));
    let back = |from: usize, to: usize| {
        (from..to).for_each(|j| drop(std::fs::rename(tmp(j), dir.join(&moves[j].from))))
    };
    for (i, p) in moves.iter().enumerate() {
        if let Err(e) = std::fs::rename(dir.join(&p.from), tmp(i)) {
            back(0, i);
            return Err(format!("{}: {e}", p.from));
        }
    }
    for (i, p) in moves.iter().enumerate() {
        if let Err(e) = std::fs::rename(tmp(i), dir.join(&p.to)) {
            // The ones already under their new names go back through their temporary ones.
            (0..i).for_each(|j| drop(std::fs::rename(dir.join(&moves[j].to), tmp(j))));
            back(0, moves.len());
            return Err(format!("{} -> {}: {e}", p.from, p.to));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn tos(p: &[Planned]) -> Vec<&str> {
        p.iter().map(|x| x.to.as_str()).collect()
    }

    #[test]
    fn rename_stem_only_by_default() {
        let names = s(&["IMG_001.JPG", "IMG_002.JPG"]);
        let p = plan(&names, &names, "IMG_", "photo-", Flags::default()).unwrap();
        assert_eq!(tos(&p), ["photo-001.JPG", "photo-002.JPG"]);
        assert!(p.iter().all(|x| x.conflict.is_none()));
    }

    #[test]
    fn rename_groups_counter_flags() {
        let names = s(&["a-b.txt", "c-d.txt"]);
        let p = plan(
            &names,
            &names,
            r"(\w)-(\w)",
            "$2_$1_{n:2}",
            Flags::default(),
        )
        .unwrap();
        assert_eq!(tos(&p), ["b_a_01.txt", "d_c_02.txt"]);
        // A width past 20 digits is 20, not a gigabyte of zeros.
        let p = plan(&names, &names, ".*", "{n:999999999}", Flags::default()).unwrap();
        assert_eq!(tos(&p)[0], format!("{:020}.txt", 1));
        let p = plan(
            &s(&["aaa.md"]),
            &s(&["aaa.md"]),
            "A",
            "x",
            Flags {
                case_insensitive: true,
                global: true,
                whole_name: false,
            },
        )
        .unwrap();
        assert_eq!(tos(&p), ["xxx.md"]);
        let p = plan(&s(&["aaa.md"]), &s(&["aaa.md"]), "a", "x", Flags::default()).unwrap();
        assert_eq!(tos(&p), ["xaa.md"]);
        let p = plan(
            &s(&["x.tar.gz"]),
            &s(&["x.tar.gz"]),
            r"\.tar\.gz$",
            ".tgz",
            Flags {
                whole_name: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(tos(&p), ["x.tgz"]);
        assert!(plan(&names, &names, "(", "", Flags::default()).is_err());
    }

    #[test]
    fn rename_controls_shown_and_kept() {
        let name = "a\nb\tc\u{1b}d\u{7f}e\u{85}f";
        assert_eq!(shown(name), "a␊b␉c␛d␡e␦f");
        // Unchanged text never renames, whatever the field shows.
        assert_eq!(typed(name, &shown(name)), None);
        assert_eq!(typed("plain.txt", "plain.txt"), None);
        // An edit elsewhere keeps every control character, the C1 one too.
        assert_eq!(typed(name, "X␊b␉c␛d␡e␦f").as_deref(), Some("X\nb\tc\u{1b}d\u{7f}e\u{85}f"));
        assert_eq!(typed(name, "a␊b␉c␛d␡e␦f.txt").as_deref(), Some("a\nb\tc\u{1b}d\u{7f}e\u{85}f.txt"));
        // A picture typed is the character; one that was in the name stays a picture.
        assert_eq!(typed("ab", "a␊b").as_deref(), Some("a\nb"));
        assert_eq!(typed("a␊b", "a␊bc").as_deref(), Some("a␊bc"));
        // A control character taken out is gone; the same name typed back is no rename.
        assert_eq!(typed("a\nb", "ab").as_deref(), Some("ab"));
        assert_eq!(typed("a\nb", "a␊b"), None);
        // Typed in the batch fields, a picture is its character too.
        let names = s(&["one\ntwo.txt"]);
        let p = plan(&names, &names, "␊", "-", Flags::default()).unwrap();
        assert_eq!(tos(&p), ["one-two.txt"]);
    }

    #[test]
    fn rename_conflicts() {
        let existing = s(&["a.txt", "b.txt", "keep.txt"]);
        // Both map to the same name.
        let p = plan(
            &s(&["a.txt", "b.txt"]),
            &existing,
            ".*",
            "same",
            Flags::default(),
        )
        .unwrap();
        assert!(p.iter().all(|x| x.conflict.is_some()));
        // Taken by a file that stays.
        let p = plan(&s(&["a.txt"]), &existing, "a", "keep", Flags::default()).unwrap();
        assert!(p[0].conflict.is_some());
        // Two selected files mapping to one name collide.
        let p = plan(
            &s(&["a.txt", "b.txt"]),
            &existing,
            "^(a|b)$",
            "x",
            Flags::default(),
        )
        .unwrap();
        assert!(p.iter().all(|x| x.conflict.is_some()));
        let p = plan(
            &s(&["a.txt"]),
            &existing,
            r"^a\.txt$",
            "",
            Flags {
                whole_name: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(p[0].conflict.is_some(), "empty name");
        let p = plan(&s(&["a.txt"]), &existing, "a", "x/y", Flags::default()).unwrap();
        assert!(p[0].conflict.is_some(), "separator");
        assert!(windows_refuses("nul.txt") && windows_refuses("COM1") && windows_refuses("a?b"));
        assert!(windows_refuses("dot.") && windows_refuses("space "));
        assert!(
            !windows_refuses("console.txt") && !windows_refuses("COM0") && !windows_refuses("a.b")
        );
        let p = plan(&s(&["a.txt"]), &existing, "a", "aux", Flags::default()).unwrap();
        assert_eq!(p[0].conflict.is_some(), cfg!(windows), "a device's name");
        // Unchanged is not a conflict.
        let p = plan(&s(&["a.txt"]), &existing, "zzz", "q", Flags::default()).unwrap();
        assert_eq!((p[0].to.as_str(), p[0].conflict.is_none()), ("a.txt", true));
    }

    #[test]
    fn rename_apply_swaps() {
        let d = std::env::temp_dir().join(format!("coxswain-rename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a"), "A").unwrap();
        std::fs::write(d.join("b"), "B").unwrap();
        let swap = vec![
            Planned {
                from: "a".into(),
                to: "b".into(),
                conflict: None,
            },
            Planned {
                from: "b".into(),
                to: "a".into(),
                conflict: None,
            },
        ];
        apply(&d, &swap).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "B");
        assert_eq!(std::fs::read_to_string(d.join("b")).unwrap(), "A");
        let bad = vec![Planned {
            from: "a".into(),
            to: "c".into(),
            conflict: Some("x".into()),
        }];
        assert!(apply(&d, &bad).is_err());
        assert!(d.join("a").exists());
        // A rename that fails halfway gives the files already set aside their names back.
        let gone = vec![
            Planned {
                from: "a".into(),
                to: "c".into(),
                conflict: None,
            },
            Planned {
                from: "missing".into(),
                to: "d".into(),
                conflict: None,
            },
        ];
        assert!(apply(&d, &gone).is_err());
        // One that fails in the second round (no such folder) leaves the first one as it was.
        let late = vec![
            Planned {
                from: "a".into(),
                to: "c".into(),
                conflict: None,
            },
            Planned {
                from: "b".into(),
                to: "no-such/d".into(),
                conflict: None,
            },
        ];
        assert!(apply(&d, &late).is_err());
        assert!(d.join("a").exists() && d.join("b").exists() && !d.join("c").exists());
        let names: Vec<String> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names.iter().filter(|n| *n == "a" || *n == "b").count(),
            2,
            "{names:?}"
        );
        assert!(
            names.iter().all(|n| !n.starts_with(".coxswain-rename")),
            "{names:?}"
        );
        // A name that appeared after the preview is not overwritten.
        std::fs::write(d.join("c"), "C").unwrap();
        let stale = vec![Planned {
            from: "a".into(),
            to: "c".into(),
            conflict: None,
        }];
        assert!(apply(&d, &stale).is_err());
        assert_eq!(std::fs::read_to_string(d.join("c")).unwrap(), "C");
        assert!(d.join("a").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn rename_case_only_conflicts_where_case_does_not_count() {
        let existing = s(&["a.txt", "B.txt"]);
        let p = plan(&s(&["a.txt"]), &existing, "a", "b", Flags::default()).unwrap();
        assert_eq!(
            p[0].conflict.is_some(),
            cfg!(any(windows, target_os = "macos"))
        );
        // A file's own name in another case is free everywhere.
        let p = plan(&s(&["a.txt"]), &existing, "a", "A", Flags::default()).unwrap();
        assert!(p[0].conflict.is_none());
    }
}
