//! Translations, shared by the terminal and the desktop app.
//!
//! Each language is a flat JSON catalogue in `locales/` (`"key": "text"`), compiled into the
//! binary. British English (`en-GB`) is the reference: every key exists there, and any key a
//! language lacks falls back to it. The other English variants hold only the words that
//! differ. Texts take `{name}` placeholders; a text that depends on a count is an object with
//! one entry per plural category of its language (`one`, `few`, `many`, `zero`, `other`).

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Every language: code, name in that language, flag (a `flag-icons` name).
pub const LANGUAGES: &[(&str, &str, &str)] = &[
    ("en-GB", "English (United Kingdom)", "gb"),
    ("en-AU", "English (Australia)", "au"),
    ("en-CA", "English (Canada)", "ca"),
    ("en-NZ", "English (New Zealand)", "nz"),
    ("da", "Dansk", "dk"),
    ("sv", "Svenska", "se"),
    ("fi", "Suomi", "fi"),
    ("et", "Eesti", "ee"),
    ("lv", "Latviešu", "lv"),
    ("lt", "Lietuvių", "lt"),
    ("de", "Deutsch", "de"),
    ("fr", "Français", "fr"),
    ("it", "Italiano", "it"),
    ("nl", "Nederlands", "nl"),
    ("es-AR", "Español (Argentina)", "ar"),
    ("ca", "Català", "es-ct"),
    ("eu", "Euskara", "es-pv"),
    ("he", "עברית", "il"),
];

/// Languages written right to left: the desktop app mirrors its layout for them.
pub fn is_rtl(code: &str) -> bool {
    code == "he"
}

pub const REFERENCE: &str = "en-GB";

fn source(code: &str) -> &'static str {
    match code {
        "en-GB" => include_str!("../locales/en-GB.json"),
        "en-AU" => include_str!("../locales/en-AU.json"),
        "en-CA" => include_str!("../locales/en-CA.json"),
        "en-NZ" => include_str!("../locales/en-NZ.json"),
        "da" => include_str!("../locales/da.json"),
        "sv" => include_str!("../locales/sv.json"),
        "fi" => include_str!("../locales/fi.json"),
        "et" => include_str!("../locales/et.json"),
        "lv" => include_str!("../locales/lv.json"),
        "lt" => include_str!("../locales/lt.json"),
        "de" => include_str!("../locales/de.json"),
        "fr" => include_str!("../locales/fr.json"),
        "it" => include_str!("../locales/it.json"),
        "nl" => include_str!("../locales/nl.json"),
        "es-AR" => include_str!("../locales/es-AR.json"),
        "ca" => include_str!("../locales/ca.json"),
        "eu" => include_str!("../locales/eu.json"),
        "he" => include_str!("../locales/he.json"),
        _ => "{}",
    }
}

/// The language to use for a BCP 47 tag, the nearest one Coxswain has: the language itself in
/// any region, else a close relative, else British English.
pub fn nearest(tag: &str) -> &'static str {
    let tag = tag.replace('_', "-");
    let lower = tag.to_ascii_lowercase();
    let lang = lower.split(['-', '.', '@']).next().unwrap_or("");
    let region = lower.split(['-', '.', '@']).nth(1).unwrap_or("");
    if lang == "en" {
        return match region {
            "au" => "en-AU",
            "ca" => "en-CA",
            "nz" => "en-NZ",
            // American English gets Canadian, the nearest in spelling; every other English
            // gets British.
            "us" | "" => "en-CA",
            _ => "en-GB",
        };
    }
    match lang {
        "da" | "nb" | "nn" | "no" => "da", // Norwegian reads closest to Danish
        "sv" => "sv",
        "fi" => "fi",
        "et" => "et",
        "lv" => "lv",
        "lt" => "lt",
        "de" | "gsw" | "lb" => "de",
        "fr" => "fr",
        "it" => "it",
        "nl" | "fy" | "af" => "nl",
        "es" | "gl" | "an" => "es-AR",
        "ca" | "oc" => "ca",
        "eu" => "eu",
        "he" | "iw" => "he",
        _ => REFERENCE,
    }
}

/// `"auto"` (or empty): the system's languages, first one Coxswain has; else the given code.
pub fn resolve(pref: &str) -> &'static str {
    if !pref.is_empty() && pref != "auto" {
        return nearest(pref);
    }
    let system: Vec<String> = sys_locale::get_locales().collect();
    // The first system language Coxswain has itself wins over nearest-relative matches.
    system
        .iter()
        .map(|l| nearest(l))
        .find(|&code| code != REFERENCE)
        .or_else(|| system.first().map(|l| nearest(l)))
        .unwrap_or(REFERENCE)
}

type Map = HashMap<String, serde_json::Value>;

fn catalogues() -> &'static HashMap<&'static str, Map> {
    static C: OnceLock<HashMap<&'static str, Map>> = OnceLock::new();
    C.get_or_init(|| LANGUAGES.iter().map(|(code, _, _)| (*code, serde_json::from_str(source(code)).unwrap_or_default())).collect())
}

/// The fallback chain for a language: itself, then (for English variants) British English.
fn chain(code: &str) -> [&str; 2] {
    [code, REFERENCE]
}

/// The whole catalogue for `code`, with fallbacks filled in, for the desktop app.
pub fn catalogue(code: &str) -> Map {
    let mut out = catalogues().get(REFERENCE).cloned().unwrap_or_default();
    if code != REFERENCE {
        if let Some(own) = catalogues().get(code) {
            out.extend(own.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
    }
    out
}

static CURRENT: RwLock<&'static str> = RwLock::new(REFERENCE);

pub fn set_language(code: &str) {
    if let Ok(mut c) = CURRENT.write() {
        *c = nearest(code);
    }
}

pub fn language() -> &'static str {
    CURRENT.read().map(|c| *c).unwrap_or(REFERENCE)
}

fn lookup(code: &str, key: &str) -> Option<&'static serde_json::Value> {
    chain(code).into_iter().find_map(|c| catalogues().get(c)?.get(key))
}

fn fill(text: &str, args: &[(&str, &str)]) -> String {
    args.iter().fold(text.to_string(), |s, (k, v)| s.replace(&format!("{{{k}}}"), v))
}

/// The text for `key` in the current language, `{name}` placeholders filled from `args`. A
/// missing key shows as the key itself, so it is easy to spot.
pub fn tr(key: &str, args: &[(&str, &str)]) -> String {
    match lookup(language(), key) {
        Some(serde_json::Value::String(s)) => fill(s, args),
        Some(serde_json::Value::Object(o)) => o.get("other").and_then(|v| v.as_str()).map_or_else(|| key.to_string(), |s| fill(s, args)),
        _ => key.to_string(),
    }
}

/// The text for `key` and count `n`, in the plural form the current language uses for it.
/// `{n}` is available as a placeholder.
pub fn trn(key: &str, n: u64, args: &[(&str, &str)]) -> String {
    let lang = language();
    let count = n.to_string();
    let mut all: Vec<(&str, &str)> = vec![("n", count.as_str())];
    all.extend_from_slice(args);
    match lookup(lang, key) {
        Some(serde_json::Value::Object(o)) => {
            let form = o.get(plural(lang, n)).or_else(|| o.get("other")).and_then(|v| v.as_str()).unwrap_or(key);
            fill(form, &all)
        }
        Some(serde_json::Value::String(s)) => fill(s, &all),
        _ => key.to_string(),
    }
}

/// The CLDR plural category of a whole number `n` in `lang` (the same as the browser's
/// `Intl.PluralRules`, which the desktop app uses).
pub fn plural(lang: &str, n: u64) -> &'static str {
    let (m10, m100) = (n % 10, n % 100);
    match lang.split('-').next().unwrap_or(lang) {
        "lv" => {
            if m10 == 0 || (11..=19).contains(&m100) {
                "zero"
            } else if m10 == 1 && m100 != 11 {
                "one"
            } else {
                "other"
            }
        }
        "lt" => {
            if m10 == 1 && !(11..=19).contains(&m100) {
                "one"
            } else if (2..=9).contains(&m10) && !(11..=19).contains(&m100) {
                "few"
            } else {
                "other"
            }
        }
        // French: 0 and 1 are singular; a million and up is "many".
        "fr" => {
            if n < 2 {
                "one"
            } else if n != 0 && n % 1_000_000 == 0 {
                "many"
            } else {
                "other"
            }
        }
        "he" => match n {
            1 => "one",
            2 => "two",
            _ => "other",
        },
        // Spanish, Italian and Catalan also use "many" for round millions.
        "es" | "it" | "ca" if n != 0 && n % 1_000_000 == 0 => "many",
        _ => {
            if n == 1 {
                "one"
            } else {
                "other"
            }
        }
    }
}

/// `tr` as a macro, for brevity: `t!("status.copied", "what" => name)`.
#[macro_export]
macro_rules! t {
    ($key:expr) => { $crate::i18n::tr($key, &[]) };
    ($key:expr, $($k:literal => $v:expr),+ $(,)?) => { $crate::i18n::tr($key, &[$(($k, &$v.to_string())),+]) };
}

/// `trn` as a macro: `tn!("items", 3)`, `tn!("files.found", n, "dir" => d)`.
#[macro_export]
macro_rules! tn {
    ($key:expr, $n:expr) => { $crate::i18n::trn($key, $n as u64, &[]) };
    ($key:expr, $n:expr, $($k:literal => $v:expr),+ $(,)?) => { $crate::i18n::trn($key, $n as u64, &[$(($k, &$v.to_string())),+]) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i18n_nearest_language() {
        for (tag, want) in [
            ("da_DK.UTF-8", "da"),
            ("en-US", "en-CA"),
            ("en", "en-CA"),
            ("he-IL", "he"),
            ("iw", "he"),
            ("en_AU", "en-AU"),
            ("en-IE", "en-GB"),
            ("nb-NO", "da"),
            ("de-CH", "de"),
            ("gsw", "de"),
            ("fr-CA", "fr"),
            ("es-ES", "es-AR"),
            ("es-MX", "es-AR"),
            ("ca-ES-valencia", "ca"),
            ("eu-ES", "eu"),
            ("sv-FI", "sv"),
            ("fy-NL", "nl"),
            ("ja-JP", "en-GB"),
            ("", "en-GB"),
        ] {
            assert_eq!(nearest(tag), want, "{tag}");
        }
        assert_eq!(resolve("lt"), "lt");
    }

    #[test]
    fn i18n_plural_categories() {
        // Same answers as CLDR / Intl.PluralRules.
        assert_eq!([0, 1, 2, 5].map(|n| plural("en-GB", n)), ["other", "one", "other", "other"]);
        assert_eq!([0, 1, 2, 1_000_000].map(|n| plural("fr", n)), ["one", "one", "other", "many"]);
        assert_eq!([1, 2, 5, 11, 21, 22, 25].map(|n| plural("lt", n)), ["one", "few", "few", "other", "one", "few", "few"]);
        assert_eq!([0, 1, 2, 10, 11, 21, 111].map(|n| plural("lv", n)), ["zero", "one", "other", "zero", "zero", "one", "zero"]);
        assert_eq!([1, 2, 3, 20].map(|n| plural("he", n)), ["one", "two", "other", "other"]);
    }

    #[test]
    fn i18n_catalogues_are_complete_and_well_formed() {
        let reference = catalogue(REFERENCE);
        assert!(!reference.is_empty());
        let placeholders = |s: &str| {
            let mut v: Vec<String> = s.split('{').skip(1).filter_map(|p| p.split_once('}').map(|x| x.0.to_string())).collect();
            v.sort();
            v
        };
        let texts = |v: &serde_json::Value| -> Vec<String> {
            match v {
                serde_json::Value::String(s) => vec![s.clone()],
                serde_json::Value::Object(o) => o.values().filter_map(|x| x.as_str().map(String::from)).collect(),
                _ => vec![],
            }
        };
        for (code, _, _) in LANGUAGES {
            let own: Map = serde_json::from_str(source(code)).unwrap_or_else(|e| panic!("{code}.json: {e}"));
            for (key, v) in &own {
                let Some(r) = reference.get(key) else { panic!("{code}: unknown key {key}") };
                // The same placeholders as British English, in every plural form.
                let want = texts(r).first().map(|s| placeholders(s)).unwrap_or_default();
                for t in texts(v) {
                    let mut have = placeholders(&t);
                    have.retain(|p| p != "n");
                    let mut want = want.clone();
                    want.retain(|p| p != "n");
                    assert_eq!(have, want, "{code}: {key}: {t}");
                }
                // A counted text needs "other" in any language.
                if let serde_json::Value::Object(o) = v {
                    assert!(o.contains_key("other"), "{code}: {key} lacks \"other\"");
                }
            }
        }
    }
}
