//! Names in the order of the language Coxswain speaks (`i18n::language`): Danish puts Æ Ø Å
//! after Z, Swedish Å Ä Ö, German Ä with A, Spanish ñ after n, Czech ch after h. Numbers go by
//! their value (file2 before file10), case does not count, and names that are still alike go by
//! their code points, so the order is total. ICU4X's collator does the work, with the tables of
//! Coxswain's languages only (`data`, made by `tools/collation-data.sh`).

use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::preferences::CollationNumericOrdering;
use icu_collator::{Collator, CollatorPreferences};
use icu_provider::{DataError, DataProvider, DataRequest, DataResponse};
use std::cell::RefCell;
use std::cmp::Ordering;

/// The baked tables name the locale fallback by its 1.x crate's name.
mod icu_locale_fallback {
    pub use icu_locale::fallback::*;
}

#[allow(unused_imports, dead_code, clippy::all)]
mod baked {
    use super::icu_locale_fallback;
    include!("data/mod.rs");
    pub struct Tables;
    impl_data_provider!(Tables);
}

/// The collation tables, and the normalizer's, which the rest of the build has already.
struct Tables;

macro_rules! from {
    ($from:expr => $($marker:ty),*) => {$(
        impl DataProvider<$marker> for Tables {
            fn load(&self, req: DataRequest) -> Result<DataResponse<$marker>, DataError> {
                $from.load(req)
            }
        }
    )*};
}
from!(baked::Tables => icu_collator::provider::CollationDiacriticsV1,
    icu_collator::provider::CollationJamoV1, icu_collator::provider::CollationMetadataV1,
    icu_collator::provider::CollationReorderingV1, icu_collator::provider::CollationRootV1,
    icu_collator::provider::CollationSpecialPrimariesV1, icu_collator::provider::CollationTailoringV1);
from!(icu_normalizer::provider::Baked => icu_normalizer::provider::NormalizerNfdDataV1,
    icu_normalizer::provider::NormalizerNfdTablesV1);

/// The collator for `language` (a code from `i18n::LANGUAGES`), the root order where it has
/// none of its own.
fn collator(language: &str) -> Collator {
    let mut options = CollatorOptions::default();
    // Letters and accents count, case does not.
    options.strength = Some(Strength::Secondary);
    let make = |code: &str| {
        let locale: icu_locale::Locale = code.parse().ok()?;
        let mut prefs = CollatorPreferences::from(&locale);
        prefs.numeric_ordering = Some(CollationNumericOrdering::True);
        Collator::try_new_unstable(&Tables, prefs, options).ok()
    };
    make(language)
        .or_else(|| make("und"))
        .expect("the root collation is baked in")
}

thread_local! {
    /// Per thread: a collator is not `Send`, and it is quick to make (its tables are baked in).
    static CURRENT: RefCell<Option<(&'static str, Collator)>> = const { RefCell::new(None) };
}

/// `f` with the collator of the language Coxswain speaks now.
fn with<R>(f: impl FnOnce(&Collator) -> R) -> R {
    let language = crate::i18n::language();
    CURRENT.with_borrow_mut(|slot| {
        if slot.as_ref().is_none_or(|(l, _)| *l != language) {
            *slot = Some((language, collator(language)));
        }
        let (_, c) = slot.as_ref().expect("just set");
        f(c)
    })
}

/// A key that sorts as `cmp` does, byte by byte: worth it where a name is compared many times
/// (a sort of a big folder; a comparison costs about as much as making a key). The sort key, a
/// zero (which no sort key holds), and the name itself, for the names that are still alike.
pub fn key(name: &str) -> Vec<u8> {
    with(|c| key_with(c, name))
}

/// `key` for each of `names`, on every core.
pub fn keys<S: AsRef<str> + Sync>(names: &[S]) -> Vec<Vec<u8>> {
    use rayon::prelude::*;
    names.par_iter().map(|n| key(n.as_ref())).collect()
}

fn key_with(c: &Collator, name: &str) -> Vec<u8> {
    let mut k = Vec::with_capacity(name.len() * 3 + 1);
    let Ok(()) = c.as_borrowed().write_sort_key_to(name, &mut k);
    k.push(0);
    k.extend_from_slice(name.as_bytes());
    k
}

/// Each of `names`' place in the language's order (alike names alike): for the desktop app's
/// page, which sorts a listing again by itself.
pub fn ranks(names: &[String]) -> Vec<u32> {
    let keys = keys(names);
    let mut order: Vec<usize> = (0..names.len()).collect();
    order.sort_unstable_by(|&i, &j| keys[i].cmp(&keys[j]));
    let mut ranks = vec![0; names.len()];
    for (r, &i) in order.iter().enumerate() {
        ranks[i] = r as u32;
    }
    ranks
}

/// `a` against `b` in the language's order, numbers by their value; total.
pub fn cmp(a: &str, b: &str) -> Ordering {
    with(|c| c.as_borrowed().compare(a, b)).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(language: &str, names: &[&str]) -> Vec<String> {
        let c = collator(language);
        let mut v: Vec<&str> = names.to_vec();
        v.sort_by_cached_key(|n| key_with(&c, n));
        let by_cmp = {
            let mut w = names.to_vec();
            w.sort_by(|a, b| c.as_borrowed().compare(a, b).then_with(|| a.cmp(b)));
            w
        };
        assert_eq!(v, by_cmp, "keys and comparisons agree for {language}");
        v.into_iter().map(String::from).collect()
    }

    fn order(language: &str, want: &[&str]) {
        let mut shuffled = want.to_vec();
        shuffled.reverse();
        shuffled.rotate_left(want.len() / 2);
        assert_eq!(sorted(language, &shuffled), want, "{language}");
    }

    #[test]
    fn every_language_has_its_alphabet() {
        for l in crate::i18n::LANGUAGES {
            let _ = collator(l.code);
        }
        for l in ["da", "nb", "nn"] {
            order(l, &["Zebra", "Æble", "Østers", "Ål"]);
        }
        for l in ["sv", "fi"] {
            order(l, &["Zebra", "Ål", "Ähre", "Öl"]);
        }
        for l in ["de", "de-AT", "de-CH"] {
            order(l, &["Abend", "Ähre", "Apfel", "Bär", "Ofen", "Öl", "Zebra"]);
        }
        order("es-AR", &["nube", "nzz", "ñandú", "oso"]);
        order("cs", &["hrad", "chata", "id", "řeka", "sova", "šach"]);
        order(
            "pl",
            &[
                "las", "łódź", "most", "osa", "ósmy", "pies", "zebra", "źrebię", "żaba",
            ],
        );
        order(
            "et",
            &[
                "sõber", "šokk", "zebra", "žürii", "tool", "väli", "õun", "ämber", "öö", "üks",
            ],
        );
        order("lt", &["cukrus", "čia", "sala", "šuo", "zebra", "žuvis"]);
        order("lv", &["cena", "čūska", "gads", "ģimene", "zebra", "žurka"]);
        order("el", &["αλφα", "βήτα", "γάμμα", "ωμέγα"]);
        order(
            "uk",
            &[
                "гора",
                "ґанок",
                "дім",
                "ехо",
                "єнот",
                "иній",
                "іній",
                "їжак",
                "яблуко",
            ],
        );
        order("en-GB", &["Æble", "Ångström", "apple", "Zebra"]);
        // Japanese: kana by the gojūon, before kanji; Korean: Hangul by its letters.
        order("ja", &["あさ", "かき", "さくら", "大阪"]);
        order("ko", &["가방", "나무", "다리", "하늘"]);
        order("he", &["אבא", "בית", "גשם", "תפוח"]);
        order("fa", &["آب", "باران", "پدر", "تهران"]);
    }

    #[test]
    fn numbers_by_value_case_ignored_ties_by_code_point() {
        for l in ["en-GB", "da", "ja"] {
            order(l, &["File1", "file2", "file010", "file10", "file10a"]);
            order(l, &["a", "B", "c"]);
            order(l, &["_build", "2024", "app"]);
        }
        // Alike but for case: code points decide, so no two names are equal.
        order("da", &["Ærø 2", "ærø 2", "ærø 10"]);
        assert_eq!(cmp("x", "x"), Ordering::Equal);
        let names = ["file10", "file2", "B", "a"].map(String::from);
        assert_eq!(ranks(&names), [3, 2, 1, 0]);
    }
}
