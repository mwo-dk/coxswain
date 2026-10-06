//! Translations, shared by the terminal and the desktop app.
//!
//! Each language is a flat JSON catalogue in `locales/` (`"key": "text"`), compiled into the
//! binary. British English (`en-GB`) is the reference: every key exists there, and any key a
//! language lacks falls back to it. The other English variants hold only the words that
//! differ; so do the German ones, which fall back to German first. Swiss German is German with
//! every ß written ss, made when the catalogues load, plus its own file. Texts take `{name}` placeholders; a text that depends on a count is an object with
//! one entry per plural category of its language (`one`, `few`, `many`, `zero`, `other`).

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// A language Coxswain speaks.
#[derive(Debug, serde::Serialize)]
pub struct Language {
    pub code: &'static str,
    /// Its name in itself.
    pub name: &'static str,
    /// A `flag-icons` name, or `derafsh`: Persian's banner, drawn for Coxswain (`docs/flags/`).
    pub flag: &'static str,
    /// The region it is listed under: a `language.group.*` key.
    pub group: &'static str,
    /// A fresh translation: marked in the list, with a way to help improve it.
    pub new: bool,
}

const fn lang(code: &'static str, name: &'static str, flag: &'static str, group: &'static str, new: bool) -> Language {
    Language { code, name, flag, group, new }
}

/// Every language, by region and by its own name within one: the list both apps show.
pub const LANGUAGES: &[Language] = &[
    lang("da", "Dansk", "dk", "language.group.nordic", false),
    lang("et", "Eesti", "ee", "language.group.nordic", false),
    lang("lv", "Latviešu", "lv", "language.group.nordic", false),
    lang("lt", "Lietuvių", "lt", "language.group.nordic", false),
    lang("fi", "Suomi", "fi", "language.group.nordic", false),
    lang("sv", "Svenska", "se", "language.group.nordic", false),
    lang("ca", "Català", "es-ct", "language.group.western", false),
    lang("de", "Deutsch", "de", "language.group.western", false),
    lang("de-AT", "Deutsch (Österreich)", "at", "language.group.western", false),
    lang("de-CH", "Deutsch (Schweiz)", "ch", "language.group.western", false),
    lang("en-GB", "English (United Kingdom)", "gb", "language.group.western", false),
    lang("eu", "Euskara", "es-pv", "language.group.western", false),
    lang("fr", "Français", "fr", "language.group.western", false),
    lang("it", "Italiano", "it", "language.group.western", false),
    lang("nl", "Nederlands", "nl", "language.group.western", false),
    lang("cs", "Čeština", "cz", "language.group.central", true),
    lang("pl", "Polski", "pl", "language.group.central", true),
    lang("uk", "Українська", "ua", "language.group.central", true),
    lang("el", "Ελληνικά", "gr", "language.group.mediterranean", true),
    lang("he", "עברית", "il", "language.group.middle_east", false),
    lang("fa", "فارسی", "derafsh", "language.group.middle_east", true),
    lang("hy", "Հայերեն", "am", "language.group.caucasus", true),
    lang("ka", "ქართული", "ge", "language.group.caucasus", true),
    lang("en-CA", "English (Canada)", "ca", "language.group.americas", false),
    lang("es-AR", "Español (Argentina)", "ar", "language.group.americas", false),
    lang("en-AU", "English (Australia)", "au", "language.group.asia", false),
    lang("en-NZ", "English (New Zealand)", "nz", "language.group.asia", false),
    lang("ja", "日本語", "jp", "language.group.asia", true),
    lang("ko", "한국어", "kr", "language.group.asia", true),
];

/// The language `code` (one of `LANGUAGES`).
pub fn find(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|l| l.code == code)
}

/// Where to suggest a better word for a translation.
pub const IMPROVE_URL: &str = "https://github.com/mwo-dk/coxswain/blob/master/docs/customise/languages.md#improving-a-translation";

/// Languages written right to left: the desktop app mirrors its layout for them.
pub fn is_rtl(code: &str) -> bool {
    matches!(code, "he" | "fa")
}

/// `s` with its digits in the current language's own: Persian writes ۰–۹. Only for numbers
/// shown as numbers (counts, sizes, dates), never paths, keys or config values.
pub fn digits(s: String) -> String {
    digits_in(language(), s)
}

/// `s` in capitals, except Georgian: it has no capitals in running text, and its Mtavruli
/// letters (what Unicode uppercases Mkhedruli to) are missing from most fonts.
pub fn caps(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if ('\u{10D0}'..='\u{10FF}').contains(&c) {
            out.push(c);
        } else {
            out.extend(c.to_uppercase());
        }
    }
    out
}

fn digits_in(lang: &str, s: String) -> String {
    if lang != "fa" {
        return s;
    }
    s.chars().map(|c| if c.is_ascii_digit() { char::from_u32('۰' as u32 + (c as u32 - '0' as u32)).unwrap_or(c) } else { c }).collect()
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
        "de-AT" => include_str!("../locales/de-AT.json"),
        "de-CH" => include_str!("../locales/de-CH.json"),
        "fr" => include_str!("../locales/fr.json"),
        "it" => include_str!("../locales/it.json"),
        "nl" => include_str!("../locales/nl.json"),
        "es-AR" => include_str!("../locales/es-AR.json"),
        "ca" => include_str!("../locales/ca.json"),
        "eu" => include_str!("../locales/eu.json"),
        "he" => include_str!("../locales/he.json"),
        "pl" => include_str!("../locales/pl.json"),
        "cs" => include_str!("../locales/cs.json"),
        "uk" => include_str!("../locales/uk.json"),
        "el" => include_str!("../locales/el.json"),
        "ja" => include_str!("../locales/ja.json"),
        "ko" => include_str!("../locales/ko.json"),
        "fa" => include_str!("../locales/fa.json"),
        "ka" => include_str!("../locales/ka.json"),
        "hy" => include_str!("../locales/hy.json"),
        _ => "{}",
    }
}

/// A language's own texts, without what it takes from another.
#[cfg(test)]
pub(crate) fn own(code: &str) -> Map {
    serde_json::from_str(source(code)).unwrap_or_default()
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
        // Liechtenstein writes Swiss Standard German, as does Swiss German (gsw).
        "de" => match region {
            "at" => "de-AT",
            "ch" | "li" => "de-CH",
            _ => "de",
        },
        "gsw" => "de-CH",
        "lb" => "de",
        "fr" => "fr",
        "it" => "it",
        "nl" | "fy" | "af" => "nl",
        "es" | "gl" | "an" => "es-AR",
        "ca" | "oc" => "ca",
        "eu" => "eu",
        "he" | "iw" => "he",
        "pl" => "pl",
        "cs" | "sk" => "cs", // Slovak readers read Czech
        "uk" => "uk",
        "el" => "el",
        "ja" => "ja",
        "ko" => "ko",
        // Dari (Afghan Persian) has its own code; Tajik is Persian in Cyrillic, so not here.
        "fa" | "prs" => "fa",
        "ka" => "ka",
        "hy" => "hy",
        _ => REFERENCE,
    }
}

/// `"auto"` (or empty): the system's languages, first one Coxswain has; else the given code.
pub fn resolve(pref: &str) -> &'static str {
    if !pref.is_empty() && pref != "auto" {
        return nearest(pref);
    }
    pick(&sys_locale::get_locales().collect::<Vec<_>>())
}

/// The first of the system's languages Coxswain has (any English counts: British English is
/// also the fallback for the ones it lacks), else the nearest to the first one.
fn pick(system: &[String]) -> &'static str {
    system
        .iter()
        .find(|l| nearest(l) != REFERENCE || l.to_ascii_lowercase().starts_with("en"))
        .or(system.first())
        .map_or(REFERENCE, |l| nearest(l))
}

type Map = HashMap<String, serde_json::Value>;

/// Swiss Standard German from German: ss for ß, and «guillemets» for „quotes“ (keys are
/// ASCII, so only texts change).
fn swiss(de: &str) -> String {
    de.replace('ß', "ss").replace('ẞ', "SS").replace('„', "«").replace('“', "»")
}

fn catalogues() -> &'static HashMap<&'static str, Map> {
    static C: OnceLock<HashMap<&'static str, Map>> = OnceLock::new();
    C.get_or_init(|| {
        let parse = |s: &str| -> Map { serde_json::from_str(s).unwrap_or_default() };
        LANGUAGES
            .iter()
            .map(|l| {
                let mut own = parse(source(l.code));
                if l.code == "de-CH" {
                    let mut all = parse(&swiss(source("de")));
                    all.extend(own);
                    own = all;
                }
                (l.code, own)
            })
            .collect()
    })
}

/// The fallback chain for a language: itself, its base language for a regional variant that
/// has one (`de-AT` → `de`), then British English.
fn chain(code: &str) -> Vec<&str> {
    let base = code.split('-').next().unwrap_or(code);
    let mut v = vec![code];
    if base != code && catalogues().contains_key(base) {
        v.push(base);
    }
    v.push(REFERENCE);
    v
}

/// The whole catalogue for `code`, with fallbacks filled in, for the desktop app.
pub fn catalogue(code: &str) -> Map {
    let mut out = Map::new();
    for c in chain(code).into_iter().rev() {
        if let Some(own) = catalogues().get(c) {
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
    let count = digits(n.to_string());
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
        // Polish, Ukrainian: 2–4 (not 12–14) is "few", other whole numbers "many"; Ukrainian
        // also takes 21, 31, … as "one". "other" is for fractions.
        "pl" | "uk" => {
            if n == 1 || (lang.starts_with("uk") && m10 == 1 && m100 != 11) {
                "one"
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                "few"
            } else {
                "many"
            }
        }
        // Czech: 2–4 is "few"; "many" is only for fractions.
        "cs" => match n {
            1 => "one",
            2..=4 => "few",
            _ => "other",
        },
        "ja" | "ko" => "other",
        // Persian and Armenian: 0 and 1 are singular (CLDR: i = 0 or n = 1).
        "fa" | "hy" => {
            if n < 2 {
                "one"
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
            ("de-CH", "de-CH"),
            ("de_CH.UTF-8", "de-CH"),
            ("de-LI", "de-CH"),
            ("de_AT.UTF-8", "de-AT"),
            ("de-AT", "de-AT"),
            ("de-DE", "de"),
            ("de-LU", "de"),
            ("de", "de"),
            ("gsw", "de-CH"),
            ("lb", "de"),
            ("fr-CA", "fr"),
            ("es-ES", "es-AR"),
            ("es-MX", "es-AR"),
            ("ca-ES-valencia", "ca"),
            ("eu-ES", "eu"),
            ("sv-FI", "sv"),
            ("fy-NL", "nl"),
            ("pl_PL.UTF-8", "pl"),
            ("cs_CZ", "cs"),
            ("sk-SK", "cs"),
            ("uk_UA.UTF-8", "uk"),
            ("el_GR", "el"),
            ("el-CY", "el"),
            ("ja_JP.UTF-8", "ja"),
            ("ko_KR", "ko"),
            ("fa_IR.UTF-8", "fa"),
            ("fa-AF", "fa"),
            ("prs", "fa"),
            ("ka_GE.UTF-8", "ka"),
            ("hy_AM", "hy"),
            ("ru-RU", "en-GB"),
            ("", "en-GB"),
        ] {
            assert_eq!(nearest(tag), want, "{tag}");
        }
        assert_eq!(resolve("lt"), "lt");
        let langs = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // British English first stays British English; an unknown first language is skipped.
        assert_eq!(pick(&langs(&["en-GB", "da"])), "en-GB");
        assert_eq!(pick(&langs(&["en_IE.UTF-8", "da"])), "en-GB");
        assert_eq!(pick(&langs(&["ru", "da"])), "da");
        assert_eq!(pick(&langs(&["ru", "zh"])), "en-GB");
        assert_eq!(pick(&[]), "en-GB");
    }

    #[test]
    fn i18n_languages_are_listed_by_region() {
        let reference = catalogue(REFERENCE);
        let mut seen: Vec<&str> = vec![];
        for l in LANGUAGES {
            assert!(reference.contains_key(l.group), "{}: no text for {}", l.code, l.group);
            // A region's languages stand together.
            if seen.last() != Some(&l.group) {
                assert!(!seen.contains(&l.group), "{} is apart from its region", l.code);
                seen.push(l.group);
            }
            assert_ne!(source(l.code), "{}", "{} has no catalogue", l.code);
            assert_eq!(nearest(l.code), l.code);
        }
    }

    #[test]
    fn i18n_german_variants_fall_back_to_german() {
        let de = catalogue("de");
        let (at, ch) = (catalogue("de-AT"), catalogue("de-CH"));
        // Every key, and German where the variant says nothing of its own.
        assert_eq!(at.len(), catalogue(REFERENCE).len());
        assert_eq!(ch.len(), at.len());
        assert_eq!(at["common.close"], de["common.close"]);
        assert_eq!(chain("de-AT"), ["de-AT", "de", REFERENCE]);
        assert_eq!(chain("en-AU"), ["en-AU", REFERENCE]);
        assert_eq!(chain("de"), ["de", REFERENCE]);
        assert_eq!(ch["common.close"], "Schliessen");
    }

    #[test]
    fn i18n_swiss_german_has_no_sharp_s() {
        let own: Map = serde_json::from_str(source("de-CH")).unwrap();
        for (key, v) in catalogue("de-CH") {
            // Only a text of its own may keep ß, such as the font sample that shows the letter.
            if !own.contains_key(&key) {
                assert!(!v.to_string().contains(['ß', 'ẞ', '„', '“']), "de-CH: {key}: {v}");
            }
        }
        assert!(catalogue("de")["common.close"].as_str().unwrap().contains('ß'));
    }

    #[test]
    fn i18n_plural_categories() {
        // Same answers as CLDR / Intl.PluralRules.
        assert_eq!([0, 1, 2, 5].map(|n| plural("en-GB", n)), ["other", "one", "other", "other"]);
        assert_eq!([0, 1, 2, 1_000_000].map(|n| plural("fr", n)), ["one", "one", "other", "many"]);
        assert_eq!([1, 2, 5, 11, 21, 22, 25].map(|n| plural("lt", n)), ["one", "few", "few", "other", "one", "few", "few"]);
        assert_eq!([0, 1, 2, 10, 11, 21, 111].map(|n| plural("lv", n)), ["zero", "one", "other", "zero", "zero", "one", "zero"]);
        assert_eq!([1, 2, 3, 20].map(|n| plural("he", n)), ["one", "two", "other", "other"]);
        let pl_uk = [0, 1, 2, 4, 5, 11, 12, 14, 21, 22, 25, 101, 112, 122];
        assert_eq!(
            pl_uk.map(|n| plural("pl", n)),
            ["many", "one", "few", "few", "many", "many", "many", "many", "many", "few", "many", "many", "many", "few"]
        );
        assert_eq!(
            pl_uk.map(|n| plural("uk", n)),
            ["many", "one", "few", "few", "many", "many", "many", "many", "one", "few", "many", "one", "many", "few"]
        );
        assert_eq!([0, 1, 2, 4, 5, 22].map(|n| plural("cs", n)), ["other", "one", "few", "few", "other", "other"]);
        assert_eq!([0, 1, 2].map(|n| plural("el", n)), ["other", "one", "other"]);
        assert_eq!([0, 1, 2].map(|n| plural("ja", n)), ["other", "other", "other"]);
        assert_eq!([0, 1, 2].map(|n| plural("ko", n)), ["other", "other", "other"]);
        assert_eq!([0, 1, 2].map(|n| plural("fa", n)), ["one", "one", "other"]);
        assert_eq!([0, 1, 2].map(|n| plural("hy", n)), ["one", "one", "other"]);
        assert_eq!([0, 1, 2].map(|n| plural("ka", n)), ["other", "one", "other"]);
    }

    #[test]
    fn i18n_persian_digits() {
        assert_eq!(digits_in("fa", "2026-10-06 12.5 KB".into()), "۲۰۲۶-۱۰-۰۶ ۱۲.۵ KB");
        assert_eq!(digits_in("ka", "2026".into()), "2026");
        assert!(is_rtl("fa") && is_rtl("he") && !is_rtl("hy"));
        assert_eq!(caps("ძებნა ab"), "ძებნა AB");
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
        for code in LANGUAGES.iter().map(|l| l.code) {
            let own: Map = serde_json::from_str(source(code)).unwrap_or_else(|e| panic!("{code}.json: {e}"));
            // A language of its own (not a variant that holds only its differences) has every
            // text; theme names may stay British English.
            if !code.starts_with("en") && chain(code).len() == 2 {
                let missing: Vec<&String> = reference.keys().filter(|k| !own.contains_key(*k) && !k.starts_with("theme.")).collect();
                assert!(missing.is_empty(), "{code} lacks {missing:?}");
            }
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
