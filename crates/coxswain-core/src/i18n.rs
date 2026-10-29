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
    /// A fresh translation, until this day (`YYYY-MM-DD`, four weekly releases after its first):
    /// marked in the list, with a way to help improve it. Sent to the desktop app as `new`.
    #[serde(rename = "new", serialize_with = "new_now")]
    pub new_until: Option<&'static str>,
}

impl Language {
    /// Whether it is still marked as a fresh translation today.
    pub fn new(&self) -> bool {
        still_new(&self.new_until)
    }
}

fn still_new(until: &Option<&'static str>) -> bool {
    until.is_some_and(|day| chrono::Utc::now().format("%Y-%m-%d").to_string().as_str() < day)
}

fn new_now<S: serde::Serializer>(until: &Option<&'static str>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_bool(still_new(until))
}

const fn lang(
    code: &'static str,
    name: &'static str,
    flag: &'static str,
    group: &'static str,
    new_until: Option<&'static str>,
) -> Language {
    Language {
        code,
        name,
        flag,
        group,
        new_until,
    }
}

/// Every language, by region and by its own name within one: the list both apps show.
pub const LANGUAGES: &[Language] = &[
    lang("da", "Dansk", "dk", "language.group.nordic", None),
    lang("et", "Eesti", "ee", "language.group.nordic", None),
    lang("lv", "Latviešu", "lv", "language.group.nordic", None),
    lang("lt", "Lietuvių", "lt", "language.group.nordic", None),
    lang(
        "nb",
        "Norsk (bokmål)",
        "no",
        "language.group.nordic",
        Some("2026-11-15"),
    ),
    lang(
        "nn",
        "Norsk (nynorsk)",
        "no",
        "language.group.nordic",
        Some("2026-11-15"),
    ),
    lang("fi", "Suomi", "fi", "language.group.nordic", None),
    lang("sv", "Svenska", "se", "language.group.nordic", None),
    lang("ca", "Català", "es-ct", "language.group.western", None),
    lang("de", "Deutsch", "de", "language.group.western", None),
    lang(
        "de-AT",
        "Deutsch (Österreich)",
        "at",
        "language.group.western",
        None,
    ),
    lang(
        "de-CH",
        "Deutsch (Schweiz)",
        "ch",
        "language.group.western",
        None,
    ),
    lang(
        "en-GB",
        "English (United Kingdom)",
        "gb",
        "language.group.western",
        None,
    ),
    lang("eu", "Euskara", "es-pv", "language.group.western", None),
    lang("fr", "Français", "fr", "language.group.western", None),
    lang(
        "fr-BE",
        "Français (Belgique)",
        "be",
        "language.group.western",
        None,
    ),
    lang(
        "fr-CH",
        "Français (Suisse)",
        "ch",
        "language.group.western",
        None,
    ),
    lang("it", "Italiano", "it", "language.group.western", None),
    lang(
        "it-CH",
        "Italiano (Svizzera)",
        "ch",
        "language.group.western",
        None,
    ),
    lang("nl", "Nederlands", "nl", "language.group.western", None),
    lang(
        "nl-BE",
        "Nederlands (België)",
        "be",
        "language.group.western",
        None,
    ),
    lang("cs", "Čeština", "cz", "language.group.central", None),
    lang("pl", "Polski", "pl", "language.group.central", None),
    lang("uk", "Українська", "ua", "language.group.central", None),
    lang("el", "Ελληνικά", "gr", "language.group.mediterranean", None),
    lang("he", "עברית", "il", "language.group.middle_east", None),
    lang("fa", "فارسی", "derafsh", "language.group.middle_east", None),
    lang("hy", "Հայերեն", "am", "language.group.caucasus", None),
    lang("ka", "ქართული", "ge", "language.group.caucasus", None),
    lang(
        "en-CA",
        "English (Canada)",
        "ca",
        "language.group.americas",
        None,
    ),
    lang(
        "es-AR",
        "Español (Argentina)",
        "ar",
        "language.group.americas",
        None,
    ),
    lang(
        "es-BO",
        "Español (Bolivia)",
        "bo",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-CL",
        "Español (Chile)",
        "cl",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-CO",
        "Español (Colombia)",
        "co",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-CR",
        "Español (Costa Rica)",
        "cr",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-CU",
        "Español (Cuba)",
        "cu",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-EC",
        "Español (Ecuador)",
        "ec",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-SV",
        "Español (El Salvador)",
        "sv",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-GT",
        "Español (Guatemala)",
        "gt",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-HN",
        "Español (Honduras)",
        "hn",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-MX",
        "Español (México)",
        "mx",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-NI",
        "Español (Nicaragua)",
        "ni",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-PA",
        "Español (Panamá)",
        "pa",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-PE",
        "Español (Perú)",
        "pe",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-PR",
        "Español (Puerto Rico)",
        "pr",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-DO",
        "Español (República Dominicana)",
        "do",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "es-UY",
        "Español (Uruguay)",
        "uy",
        "language.group.americas",
        None,
    ),
    lang(
        "es-VE",
        "Español (Venezuela)",
        "ve",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "fr-CA",
        "Français (Canada)",
        "ca",
        "language.group.americas",
        Some("2026-11-29"),
    ),
    lang(
        "en-AU",
        "English (Australia)",
        "au",
        "language.group.asia",
        None,
    ),
    lang(
        "en-NZ",
        "English (New Zealand)",
        "nz",
        "language.group.asia",
        None,
    ),
    lang(
        "zh-TW",
        "中文（台灣）",
        "tw",
        "language.group.asia",
        Some("2026-11-29"),
    ),
    lang(
        "zh-SG",
        "中文（新加坡）",
        "sg",
        "language.group.asia",
        Some("2026-11-29"),
    ),
    lang("ja", "日本語", "jp", "language.group.asia", None),
    lang("ko", "한국어", "kr", "language.group.asia", None),
];

/// The language `code` (one of `LANGUAGES`).
pub fn find(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|l| l.code == code)
}

/// Where to suggest a better word for a translation.
pub const IMPROVE_URL: &str = "https://github.com/mwo-dk/coxswain/blob/master/docs/customise/languages.md#improving-a-translation";

/// *Suggest a better translation*: a new GitHub issue in the browser, its title and body
/// naming only the language `code` and this version. Coxswain sends nothing itself.
pub fn suggest_url(code: &str) -> String {
    let title = format!("Translation ({code}): ");
    let body = format!(
        "Language: {code}\nCoxswain {}\n\nWhere the text shows, and what it says now:\n\n\nWhat it should say:\n",
        env!("CARGO_PKG_VERSION")
    );
    // Everything but the unreserved characters percent-encoded, byte by byte.
    let enc = |s: &str| {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    };
    format!("https://github.com/mwo-dk/coxswain/issues/new?title={}&body={}", enc(&title), enc(&body))
}

/// Languages written right to left: the desktop app mirrors its layout for them.
pub fn is_rtl(code: &str) -> bool {
    matches!(code, "he" | "fa")
}

/// `s` with its digits in the current language's own: Persian writes ۰–۹. Only for numbers
/// shown as numbers (counts, sizes, dates), never paths, keys or config values.
pub fn digits(s: String) -> String {
    digits_in(language(), s)
}

/// `secs` (since 1970) as local date and time, `2026-10-16 14:05`, in the language's digits.
pub fn date_time(secs: u64) -> String {
    local(secs, "%Y-%m-%d %H:%M")
}

/// `secs` (since 1970) as local date, `2026-10-16`, in the language's digits.
pub fn date(secs: u64) -> String {
    local(secs, "%Y-%m-%d")
}

fn local(secs: u64, format: &str) -> String {
    let t = crate::fs::epoch(secs);
    digits(
        chrono::DateTime::<chrono::Local>::from(t)
            .format(format)
            .to_string(),
    )
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
    s.chars()
        .map(|c| {
            if c.is_ascii_digit() {
                char::from_u32('۰' as u32 + (c as u32 - '0' as u32)).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
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
        "nb" => include_str!("../locales/nb.json"),
        "nn" => include_str!("../locales/nn.json"),
        "fi" => include_str!("../locales/fi.json"),
        "et" => include_str!("../locales/et.json"),
        "lv" => include_str!("../locales/lv.json"),
        "lt" => include_str!("../locales/lt.json"),
        "de" => include_str!("../locales/de.json"),
        "de-AT" => include_str!("../locales/de-AT.json"),
        "de-CH" => include_str!("../locales/de-CH.json"),
        "fr" => include_str!("../locales/fr.json"),
        "fr-BE" => include_str!("../locales/fr-BE.json"),
        "fr-CA" => include_str!("../locales/fr-CA.json"),
        "fr-CH" => include_str!("../locales/fr-CH.json"),
        "it" => include_str!("../locales/it.json"),
        "it-CH" => include_str!("../locales/it-CH.json"),
        "nl" => include_str!("../locales/nl.json"),
        "nl-BE" => include_str!("../locales/nl-BE.json"),
        "es-AR" => include_str!("../locales/es-AR.json"),
        "es-BO" => include_str!("../locales/es-BO.json"),
        "es-CL" => include_str!("../locales/es-CL.json"),
        "es-CO" => include_str!("../locales/es-CO.json"),
        "es-CR" => include_str!("../locales/es-CR.json"),
        "es-CU" => include_str!("../locales/es-CU.json"),
        "es-EC" => include_str!("../locales/es-EC.json"),
        "es-SV" => include_str!("../locales/es-SV.json"),
        "es-GT" => include_str!("../locales/es-GT.json"),
        "es-HN" => include_str!("../locales/es-HN.json"),
        "es-MX" => include_str!("../locales/es-MX.json"),
        "es-NI" => include_str!("../locales/es-NI.json"),
        "es-PA" => include_str!("../locales/es-PA.json"),
        "es-PE" => include_str!("../locales/es-PE.json"),
        "es-PR" => include_str!("../locales/es-PR.json"),
        "es-DO" => include_str!("../locales/es-DO.json"),
        "es-UY" => include_str!("../locales/es-UY.json"),
        "es-VE" => include_str!("../locales/es-VE.json"),
        "ca" => include_str!("../locales/ca.json"),
        "eu" => include_str!("../locales/eu.json"),
        "he" => include_str!("../locales/he.json"),
        "pl" => include_str!("../locales/pl.json"),
        "cs" => include_str!("../locales/cs.json"),
        "uk" => include_str!("../locales/uk.json"),
        "el" => include_str!("../locales/el.json"),
        "ja" => include_str!("../locales/ja.json"),
        "ko" => include_str!("../locales/ko.json"),
        "zh-TW" => include_str!("../locales/zh-TW.json"),
        "zh-SG" => include_str!("../locales/zh-SG.json"),
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
        "da" => "da",
        // Norwegian without a written standard named (`no`, `no_NO`) is Bokmål, as most write it.
        "nb" | "no" => "nb",
        "nn" => "nn",
        "sv" => "sv",
        "fi" => "fi",
        "et" => "et",
        "lv" => "lv",
        "lt" => "lt",
        // Liechtenstein writes Swiss Standard German, as does Swiss German (gsw).
        // South Tyrol (de-IT) writes Austrian German.
        "de" => match region {
            "at" | "it" => "de-AT",
            "ch" | "li" => "de-CH",
            _ => "de",
        },
        "gsw" => "de-CH",
        "lb" => "de",
        "fr" => match region {
            "ca" => "fr-CA",
            "be" => "fr-BE",
            "ch" | "li" => "fr-CH",
            _ => "fr",
        },
        "it" => match region {
            "ch" => "it-CH",
            _ => "it",
        },
        // Flemish is Dutch as Belgium writes it; West Flemish (vls) readers read it.
        "nl" if region == "be" => "nl-BE",
        "vls" => "nl-BE",
        "nl" | "fy" | "af" => "nl",
        // Latin America writes tú, except the vos of Argentina, Uruguay and Paraguay: its own
        // entry where it has one, else Chilean Spanish (vos: Argentinian). Spain, Galician and Aragonese stay on
        // Argentinian Spanish.
        "es" => match region {
            "cl" | "419" | "us" => "es-CL",
            "bo" => "es-BO",
            "ec" => "es-EC",
            "co" => "es-CO",
            "cr" => "es-CR",
            "cu" => "es-CU",
            "sv" => "es-SV",
            "gt" => "es-GT",
            "hn" => "es-HN",
            "mx" => "es-MX",
            "ni" => "es-NI",
            "pa" => "es-PA",
            "pe" => "es-PE",
            "pr" => "es-PR",
            "do" => "es-DO",
            "ve" => "es-VE",
            "uy" => "es-UY",
            _ => "es-AR",
        },
        "gl" | "an" => "es-AR",
        "ca" | "oc" => "ca",
        "eu" => "eu",
        "he" | "iw" => "he",
        "pl" => "pl",
        "cs" | "sk" => "cs", // Slovak readers read Czech
        "uk" => "uk",
        "el" => "el",
        "ja" => "ja",
        "ko" => "ko",
        // Chinese by its script.
        "zh" => {
            let parts: Vec<&str> = lower.split(['-', '.', '@']).collect();
            if parts.contains(&"hant")
                || (!parts.contains(&"hans")
                    && parts.iter().any(|p| matches!(*p, "tw" | "hk" | "mo")))
            {
                "zh-TW"
            } else {
                "zh-SG"
            }
        }
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
    de.replace('ß', "ss")
        .replace('ẞ', "SS")
        .replace('„', "«")
        .replace('“', "»")
}

/// A language's catalogue (de-CH: German made Swiss, under its own texts), parsed the first
/// time it is asked for: a start reads the two or three files its language falls back
/// through, not all of them.
fn cat(code: &str) -> Option<&'static Map> {
    const N: usize = LANGUAGES.len();
    static C: [OnceLock<Map>; N] = [const { OnceLock::new() }; N];
    let i = LANGUAGES.iter().position(|l| l.code == code)?;
    Some(C[i].get_or_init(|| {
        let parse = |s: &str| -> Map { serde_json::from_str(s).unwrap_or_default() };
        let own = parse(source(code));
        if code != "de-CH" {
            return own;
        }
        let mut all = parse(&swiss(source("de")));
        all.extend(own);
        all
    }))
}

/// The language a variant takes what it lacks from: its base language (`de-AT` → `de`); for
/// Spanish, Argentinian (Coxswain's first), and the Spanish that writes tú takes Chilean first.
fn parent(code: &str) -> Option<&'static str> {
    match code {
        "es-AR" => None,
        // Uruguay writes vos, as Argentina does.
        "es-CL" | "es-UY" => Some("es-AR"),
        _ if code.starts_with("es-") => Some("es-CL"),
        _ => find(code.split('-').next().unwrap_or(code))
            .filter(|l| l.code != code)
            .map(|l| l.code),
    }
}

/// The fallback chain for a language: itself, its parents (`parent`), then British English.
pub(crate) fn chain(code: &str) -> Vec<&str> {
    let mut v = vec![code];
    let mut c = code;
    while let Some(p) = parent(c) {
        v.push(p);
        c = p;
    }
    v.push(REFERENCE);
    v
}

/// The whole catalogue for `code`, with fallbacks filled in, for the desktop app.
pub fn catalogue(code: &str) -> Map {
    let mut out = Map::new();
    for c in chain(code).into_iter().rev() {
        if let Some(own) = cat(c) {
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
    chain(code).into_iter().find_map(|c| cat(c)?.get(key))
}

fn fill(text: &str, args: &[(&str, &str)]) -> String {
    args.iter().fold(text.to_string(), |s, (k, v)| {
        s.replace(&format!("{{{k}}}"), v)
    })
}

/// The text for `key` in the current language, `{name}` placeholders filled from `args`. A
/// missing key shows as the key itself, so it is easy to spot.
pub fn tr(key: &str, args: &[(&str, &str)]) -> String {
    match lookup(language(), key) {
        Some(serde_json::Value::String(s)) => fill(s, args),
        Some(serde_json::Value::Object(o)) => o
            .get("other")
            .and_then(|v| v.as_str())
            .map_or_else(|| key.to_string(), |s| fill(s, args)),
        _ => key.to_string(),
    }
}

/// The text for `key` in the language `code`, whatever the current one is (the docs' English).
pub fn tr_in(code: &str, key: &str) -> String {
    lookup(code, key).and_then(|v| v.as_str()).map_or_else(|| key.to_string(), str::to_string)
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
            let form = o
                .get(plural(lang, n))
                .or_else(|| o.get("other"))
                .and_then(|v| v.as_str())
                .unwrap_or(key);
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
        "ja" | "ko" | "zh" => "other",
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
    fn suggest_url_names_only_the_language_and_version() {
        let url = suggest_url("de-CH");
        assert!(url.starts_with("https://github.com/mwo-dk/coxswain/issues/new?title=Translation%20%28de-CH%29%3A%20&body=Language%3A%20de-CH%0ACoxswain%20"), "{url}");
        assert!(url.contains(env!("CARGO_PKG_VERSION")));
        assert!(!url.contains(' ') && !url.contains('\n'));
    }

    #[test]
    fn i18n_new_marks_end_on_their_day() {
        for l in LANGUAGES {
            if let Some(day) = l.new_until {
                assert!(
                    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").is_ok(),
                    "{}: {day}",
                    l.code
                );
            }
        }
        assert!(still_new(&Some("9999-12-31")));
        assert!(!still_new(&Some("2000-01-01")));
        assert!(!still_new(&None));
    }

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
            ("nb-NO", "nb"),
            ("nb", "nb"),
            ("no", "nb"),
            ("no-NO", "nb"),
            ("nb_NO.UTF-8", "nb"),
            ("nn", "nn"),
            ("nn-NO", "nn"),
            ("nn_NO.UTF-8", "nn"),
            ("de-CH", "de-CH"),
            ("de_CH.UTF-8", "de-CH"),
            ("de-LI", "de-CH"),
            ("de_AT.UTF-8", "de-AT"),
            ("de-AT", "de-AT"),
            ("de-IT", "de-AT"),
            ("de_IT.UTF-8", "de-AT"),
            ("de-DE", "de"),
            ("de-LU", "de"),
            ("de", "de"),
            ("gsw", "de-CH"),
            ("lb", "de"),
            ("fr-CA", "fr-CA"),
            ("fr_CA.UTF-8", "fr-CA"),
            ("fr-BE", "fr-BE"),
            ("fr_BE.UTF-8", "fr-BE"),
            ("fr-CH", "fr-CH"),
            ("fr-LI", "fr-CH"),
            ("fr_CH.UTF-8", "fr-CH"),
            ("fr-FR", "fr"),
            ("fr-LU", "fr"),
            ("fr", "fr"),
            ("it-CH", "it-CH"),
            ("it_CH.UTF-8", "it-CH"),
            ("it-IT", "it"),
            ("it-SM", "it"),
            ("nl-BE", "nl-BE"),
            ("nl_BE.UTF-8", "nl-BE"),
            ("vls", "nl-BE"),
            ("vls-BE", "nl-BE"),
            ("nl-NL", "nl"),
            ("nl-SR", "nl"),
            ("es-ES", "es-AR"),
            ("es-MX", "es-MX"),
            ("es_MX.UTF-8", "es-MX"),
            ("es-CL", "es-CL"),
            ("es_CL.UTF-8", "es-CL"),
            ("es-CO", "es-CO"),
            ("es-PE", "es-PE"),
            ("es-VE", "es-VE"),
            ("es-EC", "es-EC"),
            ("es_EC.UTF-8", "es-EC"),
            ("es-BO", "es-BO"),
            ("es-CU", "es-CU"),
            ("es-DO", "es-DO"),
            ("es-PR", "es-PR"),
            ("es-GT", "es-GT"),
            ("es-HN", "es-HN"),
            ("es-SV", "es-SV"),
            ("es-NI", "es-NI"),
            ("es-CR", "es-CR"),
            ("es-PA", "es-PA"),
            ("es-419", "es-CL"),
            ("es-US", "es-CL"),
            ("es-AR", "es-AR"),
            ("es_AR.UTF-8", "es-AR"),
            ("es-UY", "es-UY"),
            ("es_UY.UTF-8", "es-UY"),
            ("es-PY", "es-AR"),
            ("es", "es-AR"),
            ("gl-ES", "es-AR"),
            ("an", "es-AR"),
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
            ("zh-TW", "zh-TW"),
            ("zh_TW.UTF-8", "zh-TW"),
            ("zh-HK", "zh-TW"),
            ("zh_MO", "zh-TW"),
            ("zh-Hant", "zh-TW"),
            ("zh-Hant-CN", "zh-TW"),
            ("zh-SG", "zh-SG"),
            ("zh_CN.UTF-8", "zh-SG"),
            ("zh-MY", "zh-SG"),
            ("zh", "zh-SG"),
            ("zh-Hans", "zh-SG"),
            ("zh-Hans-HK", "zh-SG"),
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
        assert_eq!(pick(&langs(&["ru", "tr"])), "en-GB");
        assert_eq!(pick(&langs(&["ru", "zh-Hant-TW"])), "zh-TW");
        assert_eq!(pick(&[]), "en-GB");
    }

    #[test]
    fn i18n_languages_are_listed_by_region() {
        let reference = catalogue(REFERENCE);
        let mut seen: Vec<&str> = vec![];
        for l in LANGUAGES {
            assert!(
                reference.contains_key(l.group),
                "{}: no text for {}",
                l.code,
                l.group
            );
            // A region's languages stand together.
            if seen.last() != Some(&l.group) {
                assert!(
                    !seen.contains(&l.group),
                    "{} is apart from its region",
                    l.code
                );
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
        for (variant, base) in [
            ("fr-CA", "fr"),
            ("es-CL", "es-AR"),
            ("fr-BE", "fr"),
            ("fr-CH", "fr"),
            ("it-CH", "it"),
            ("nl-BE", "nl"),
        ] {
            assert_eq!(chain(variant), [variant, base, REFERENCE]);
            assert_eq!(catalogue(variant).len(), catalogue(REFERENCE).len());
        }
        assert_eq!(catalogue("fr-BE")["common.close"], catalogue("fr")["common.close"]);
        assert_eq!(chain("es-MX"), ["es-MX", "es-CL", "es-AR", REFERENCE]);
        assert_eq!(chain("es-AR"), ["es-AR", REFERENCE]);
        assert_eq!(chain("es-UY"), ["es-UY", "es-AR", REFERENCE]);
        assert_eq!(chain("fr"), ["fr", REFERENCE]);
        // Tú from Chile, computadora of its own.
        let mx = catalogue("es-MX");
        assert_eq!(mx.len(), catalogue(REFERENCE).len());
        assert!(mx["sync.not_here"].as_str().unwrap().contains("esta computadora"));
        assert!(catalogue("es-CO")["sync.not_here"].as_str().unwrap().contains("este computador"));
        assert_eq!(mx["app.cmd_placeholder"], catalogue("es-CL")["app.cmd_placeholder"]);
        assert_eq!(chain("de"), ["de", REFERENCE]);
        assert_eq!(ch["common.close"], "Schliessen");
    }

    #[test]
    fn i18n_spanish_with_tu_has_no_vos() {
        // A text added to es-AR with vos needs its tú form in es-CL, or Chile and the countries
        // after it read vos. Only forms that are never anything else are looked for.
        let vos = regex::Regex::new(
            r"(?i)\b(vos|podés|tenés|querés|apretá|instalá|usá|hacé|poné|mantené|volvé|elegís|pedís)\b",
        )
        .unwrap();
        assert!(vos.is_match(catalogue("es-AR")["feature.privacy.a2"].as_str().unwrap()));
        for code in ["es-CL", "es-MX", "es-CO"] {
            for (key, v) in catalogue(code) {
                assert!(!vos.is_match(&v.to_string()), "{code}: {key}: {v}");
            }
        }
        let fr_ca = catalogue("fr-CA");
        assert_eq!(fr_ca["action.action_menu"], "Que puis-je faire avec ceci?");
    }

    #[test]
    fn i18n_swiss_german_has_no_sharp_s() {
        let own: Map = serde_json::from_str(source("de-CH")).unwrap();
        for (key, v) in catalogue("de-CH") {
            // Only a text of its own may keep ß, such as the font sample that shows the letter.
            if !own.contains_key(&key) {
                assert!(
                    !v.to_string().contains(['ß', 'ẞ', '„', '“']),
                    "de-CH: {key}: {v}"
                );
            }
        }
        assert!(
            catalogue("de")["common.close"]
                .as_str()
                .unwrap()
                .contains('ß')
        );
    }

    #[test]
    fn i18n_plural_categories() {
        // Same answers as CLDR / Intl.PluralRules.
        assert_eq!(
            [0, 1, 2, 5].map(|n| plural("en-GB", n)),
            ["other", "one", "other", "other"]
        );
        assert_eq!(
            [0, 1, 2, 1_000_000].map(|n| plural("fr", n)),
            ["one", "one", "other", "many"]
        );
        assert_eq!(
            [1, 2, 1_000_000].map(|n| plural("es-CL", n)),
            ["one", "other", "many"]
        );
        for fr in ["fr-CA", "fr-BE", "fr-CH"] {
            assert_eq!(
                [0, 1, 2, 1_000_000].map(|n| plural(fr, n)),
                ["one", "one", "other", "many"]
            );
        }
        assert_eq!(
            [1, 2, 5, 11, 21, 22, 25].map(|n| plural("lt", n)),
            ["one", "few", "few", "other", "one", "few", "few"]
        );
        assert_eq!(
            [0, 1, 2, 10, 11, 21, 111].map(|n| plural("lv", n)),
            ["zero", "one", "other", "zero", "zero", "one", "zero"]
        );
        assert_eq!(
            [1, 2, 3, 20].map(|n| plural("he", n)),
            ["one", "two", "other", "other"]
        );
        let pl_uk = [0, 1, 2, 4, 5, 11, 12, 14, 21, 22, 25, 101, 112, 122];
        assert_eq!(
            pl_uk.map(|n| plural("pl", n)),
            [
                "many", "one", "few", "few", "many", "many", "many", "many", "many", "few", "many",
                "many", "many", "few"
            ]
        );
        assert_eq!(
            pl_uk.map(|n| plural("uk", n)),
            [
                "many", "one", "few", "few", "many", "many", "many", "many", "one", "few", "many",
                "one", "many", "few"
            ]
        );
        assert_eq!(
            [0, 1, 2, 4, 5, 22].map(|n| plural("cs", n)),
            ["other", "one", "few", "few", "other", "other"]
        );
        assert_eq!(
            [0, 1, 2].map(|n| plural("el", n)),
            ["other", "one", "other"]
        );
        assert_eq!(
            [0, 1, 2].map(|n| plural("ja", n)),
            ["other", "other", "other"]
        );
        assert_eq!(
            [0, 1, 2].map(|n| plural("ko", n)),
            ["other", "other", "other"]
        );
        for zh in ["zh-TW", "zh-SG"] {
            assert_eq!([0, 1, 2].map(|n| plural(zh, n)), ["other", "other", "other"]);
        }
        assert_eq!([0, 1, 2].map(|n| plural("fa", n)), ["one", "one", "other"]);
        assert_eq!([0, 1, 2].map(|n| plural("hy", n)), ["one", "one", "other"]);
        assert_eq!(
            [0, 1, 2].map(|n| plural("ka", n)),
            ["other", "one", "other"]
        );
    }

    #[test]
    fn i18n_persian_digits() {
        assert_eq!(
            digits_in("fa", "2026-10-06 12.5 KB".into()),
            "۲۰۲۶-۱۰-۰۶ ۱۲.۵ KB"
        );
        assert_eq!(digits_in("ka", "2026".into()), "2026");
        assert!(is_rtl("fa") && is_rtl("he") && !is_rtl("hy"));
        assert_eq!(caps("ძებნა ab"), "ძებნა AB");
    }

    /// Every text the code names with a literal key (`t!("…"`, `tn!`, `tr(`, `trn(`, and the
    /// desktop app's `t('…'`, `tn(`) is in British English, and a Rust call gives each of its
    /// `{placeholders}` (`{n}` comes with a count). Keys made at run time are not checked here.
    #[test]
    fn i18n_every_text_the_code_names_is_there() {
        use std::path::{Path, PathBuf};
        fn walk(d: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, exts, out);
                } else if p.extension().is_some_and(|x| exts.iter().any(|e| x == *e)) {
                    out.push(p);
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = vec![];
        for c in std::fs::read_dir(root.join("crates"))
            .into_iter()
            .flatten()
            .flatten()
        {
            walk(&c.path().join("src"), &["rs"], &mut files);
        }
        // Not there in the packaged crate.
        walk(&root.join("gui/src"), &["js", "svelte", "ts"], &mut files);
        let reference = catalogue(REFERENCE);
        let named = regex::Regex::new(r#""([A-Za-z_0-9]+)"\s*(=>|,)"#).unwrap();
        let wants = |v: &serde_json::Value| -> Vec<String> {
            let forms: Vec<&str> = match v {
                serde_json::Value::Object(o) => o.values().filter_map(|f| f.as_str()).collect(),
                v => v.as_str().into_iter().collect(),
            };
            let mut all: Vec<String> = forms
                .iter()
                .flat_map(|s| {
                    s.split('{')
                        .skip(1)
                        .filter_map(|p| p.split_once('}').map(|x| x.0.to_string()))
                })
                .collect();
            all.sort();
            all.dedup();
            all
        };
        let mut bad = vec![];
        for f in &files {
            let text = std::fs::read_to_string(f).unwrap_or_default();
            let rust = f.extension().is_some_and(|x| x == "rs");
            let calls: &[&str] = if rust {
                &["t!(", "tn!(", "tr(", "trn("]
            } else {
                &["t(", "tn("]
            };
            for call in calls {
                for (i, _) in text.match_indices(call) {
                    if text[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '.') {
                        continue;
                    }
                    let line = &text[text[..i].rfind('\n').map_or(0, |p| p + 1)..i];
                    if line.contains("//") || line.trim_start().starts_with('*') {
                        continue;
                    }
                    let rest = text[i + call.len()..].trim_start();
                    let Some(q) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
                        continue;
                    };
                    let Some(end) = rest[1..].find(q) else {
                        continue;
                    };
                    let key = &rest[1..1 + end];
                    if !key.contains('.') || key.ends_with('.') || key.contains([' ', '{', '$']) {
                        continue;
                    }
                    let Some(v) = reference.get(key) else {
                        bad.push(format!("{}: no text {key}", f.display()));
                        continue;
                    };
                    if !rust {
                        continue;
                    }
                    // The call, to its closing bracket, strings skipped.
                    let (mut depth, mut in_str, mut esc, mut stop) = (1, false, false, rest.len());
                    for (j, c) in rest.char_indices() {
                        match (in_str, esc, c) {
                            (true, true, _) => esc = false,
                            (true, false, '\\') => esc = true,
                            (_, false, '"') => in_str = !in_str,
                            (false, _, '(' | '[') => depth += 1,
                            (false, _, ')' | ']') => {
                                depth -= 1;
                                if depth == 0 {
                                    stop = j;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    let span = &rest[1 + end..stop];
                    // `tr(key, args)` with its arguments made elsewhere: not checked.
                    if call.starts_with("tr")
                        && !span.contains("&[")
                        && !span.trim_start_matches([',', ' ']).is_empty()
                    {
                        continue;
                    }
                    let given: Vec<&str> = named
                        .captures_iter(span)
                        .map(|c| c.get(1).unwrap().as_str())
                        .collect();
                    let counted = call.starts_with("tn") || call.starts_with("trn");
                    let lacks: Vec<String> = wants(v)
                        .into_iter()
                        // `{settings}` and other action names: keys filled by features::with_keys.
                        .filter(|p| (!counted || p != "n") && !given.contains(&p.as_str()))
                        .filter(|p| !crate::config::Action::ALL.iter().any(|a| a.name() == p))
                        .collect();
                    if !lacks.is_empty() {
                        bad.push(format!(
                            "{}: {key} without {}",
                            f.display(),
                            lacks.join(", ")
                        ));
                    }
                }
            }
        }
        // Named at run time, so each by hand: every action's name.
        for a in crate::config::Action::ALL {
            if !reference.contains_key(&format!("action.{}", a.name())) {
                bad.push(format!("no text action.{}", a.name()));
            }
        }
        assert!(files.len() > 50, "the sources are where this test looks");
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    #[test]
    fn i18n_catalogues_are_complete_and_well_formed() {
        let reference = catalogue(REFERENCE);
        assert!(!reference.is_empty());
        let placeholders = |s: &str| {
            let mut v: Vec<String> = s
                .split('{')
                .skip(1)
                .filter_map(|p| p.split_once('}').map(|x| x.0.to_string()))
                .collect();
            v.sort();
            v
        };
        let texts = |v: &serde_json::Value| -> Vec<String> {
            match v {
                serde_json::Value::String(s) => vec![s.clone()],
                serde_json::Value::Object(o) => o
                    .values()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect(),
                _ => vec![],
            }
        };
        for code in LANGUAGES.iter().map(|l| l.code) {
            let own: Map =
                serde_json::from_str(source(code)).unwrap_or_else(|e| panic!("{code}.json: {e}"));
            // A language of its own (not a variant that holds only its differences) has every
            // text; theme names may stay British English.
            if !code.starts_with("en") && chain(code).len() == 2 {
                let missing: Vec<&String> = reference
                    .keys()
                    .filter(|k| !own.contains_key(*k) && !k.starts_with("theme."))
                    .collect();
                assert!(missing.is_empty(), "{code} lacks {missing:?}");
            }
            for (key, v) in &own {
                let Some(r) = reference.get(key) else {
                    panic!("{code}: unknown key {key}")
                };
                // The same placeholders as British English, in every plural form.
                let want = texts(r)
                    .first()
                    .map(|s| placeholders(s))
                    .unwrap_or_default();
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
