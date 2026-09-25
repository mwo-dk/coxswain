//! Regex batch rename: plan first (for the live preview), then apply.

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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Planned {
    pub from: String,
    pub to: String,
    /// Why this rename cannot happen, if it cannot.
    pub conflict: Option<String>,
}

/// Compute new names for `selected`. `existing` is every name in the directory.
///
/// `replacement` supports regex groups (`$1`, `${name}`) and a counter: `{n}` is the
/// 1-based position in `selected`, `{n:3}` zero-pads it to 3 digits.
/// Conflicts: an empty result, a path separator, two files mapping to the same name, or a
/// name taken by a file that is not itself being renamed away.
pub fn plan(selected: &[String], existing: &[String], pattern: &str, replacement: &str, flags: Flags) -> Result<Vec<Planned>, String> {
    let _ = (selected, existing, pattern, replacement, flags);
    todo!()
}

/// Rename in `dir` as planned. Refuses if any entry has a conflict. Uses temporary names so
/// swaps (a -> b, b -> a) work. Unchanged entries are skipped.
pub fn apply(dir: &Path, plan: &[Planned]) -> Result<(), String> {
    let _ = (dir, plan);
    todo!()
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
        let p = plan(&names, &names, r"(\w)-(\w)", "$2_$1_{n:2}", Flags::default()).unwrap();
        assert_eq!(tos(&p), ["b_a_01.txt", "d_c_02.txt"]);
        let p = plan(&s(&["aaa.md"]), &s(&["aaa.md"]), "A", "x", Flags { case_insensitive: true, global: true, whole_name: false }).unwrap();
        assert_eq!(tos(&p), ["xxx.md"]);
        let p = plan(&s(&["aaa.md"]), &s(&["aaa.md"]), "a", "x", Flags::default()).unwrap();
        assert_eq!(tos(&p), ["xaa.md"]);
        let p = plan(&s(&["x.tar.gz"]), &s(&["x.tar.gz"]), r"\.tar\.gz$", ".tgz", Flags { whole_name: true, ..Default::default() }).unwrap();
        assert_eq!(tos(&p), ["x.tgz"]);
        assert!(plan(&names, &names, "(", "", Flags::default()).is_err());
    }

    #[test]
    fn rename_conflicts() {
        let existing = s(&["a.txt", "b.txt", "keep.txt"]);
        // Both map to the same name.
        let p = plan(&s(&["a.txt", "b.txt"]), &existing, ".*", "same", Flags::default()).unwrap();
        assert!(p.iter().all(|x| x.conflict.is_some()));
        // Taken by a file that stays.
        let p = plan(&s(&["a.txt"]), &existing, "a", "keep", Flags::default()).unwrap();
        assert!(p[0].conflict.is_some());
        // Two selected files mapping to one name collide.
        let p = plan(&s(&["a.txt", "b.txt"]), &existing, "^(a|b)$", "x", Flags::default()).unwrap();
        assert!(p.iter().all(|x| x.conflict.is_some()));
        let p = plan(&s(&["a.txt"]), &existing, "a", "", Flags { whole_name: true, ..Default::default() }).unwrap();
        assert!(p[0].conflict.is_some(), "empty name");
        let p = plan(&s(&["a.txt"]), &existing, "a", "x/y", Flags::default()).unwrap();
        assert!(p[0].conflict.is_some(), "separator");
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
            Planned { from: "a".into(), to: "b".into(), conflict: None },
            Planned { from: "b".into(), to: "a".into(), conflict: None },
        ];
        apply(&d, &swap).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "B");
        assert_eq!(std::fs::read_to_string(d.join("b")).unwrap(), "A");
        let bad = vec![Planned { from: "a".into(), to: "c".into(), conflict: Some("x".into()) }];
        assert!(apply(&d, &bad).is_err());
        assert!(d.join("a").exists());
        std::fs::remove_dir_all(d).unwrap();
    }
}
