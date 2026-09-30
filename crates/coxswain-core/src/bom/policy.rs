//! What an algorithm is, and how good it is.
//!
//! The catalog (`catalog.json`, vendored from cipherscape; see `README.md`) says how good each
//! algorithm is, per profile and year. Resolution turns an asset's name, OID and family into
//! catalog algorithms with the parameters the rules need. OIDs in real CBOMs are often wrong,
//! so OID, family and name are separate votes and the agreeing majority wins. A composite such
//! as SHA512withRSA resolves to all its parts, and is rated as the worst of them.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use super::model::AlgorithmInfo;
use super::status::Status;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    Hash,
    Symmetric,
    Asymmetric,
    Kem,
    Signature,
    Mac,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NumOps {
    pub lt: Option<u64>,
    pub lte: Option<u64>,
    pub gt: Option<u64>,
    pub gte: Option<u64>,
    pub eq: Option<u64>,
    #[serde(rename = "in")]
    pub one_of: Option<Vec<u64>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StrOps {
    pub eq: Option<String>,
    #[serde(rename = "in")]
    pub one_of: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    pub key_bits: Option<NumOps>,
    pub security_bits: Option<NumOps>,
    pub param_set: Option<StrOps>,
}

/// A parameter a rule needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Param {
    KeyBits,
    SecurityBits,
    ParamSet,
}

/// Where a rule comes from. `reference` is the profile's key for it, when it has one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimelineStep {
    /// The first year this status applies; 0 is since forever. Sorted.
    pub from: i32,
    pub status: Status,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub when: Option<Condition>,
    pub timeline: Vec<TimelineStep>,
    pub source: Source,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Remediation {
    pub summary: String,
    pub detail: Option<String>,
    pub when: Option<Condition>,
    /// The profiles this advice is for; `None` is all.
    pub profiles: Option<Vec<String>>,
    pub replace_with: Vec<String>,
    pub source: Source,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OidParams {
    pub param_set: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Algorithm {
    pub id: String,
    /// The name to show ("SHA-2"); aliases are only for matching.
    pub name: String,
    pub family: Family,
    pub aliases: Vec<String>,
    /// OIDs, with the parameters an OID implies (ML-KEM-768's OID implies "768").
    pub oids: BTreeMap<String, OidParams>,
    pub quantum_vulnerable: bool,
    pub fips_approved: Option<bool>,
    /// Rules by profile, and "*" for profiles that have none of their own. The first match wins.
    pub rules: BTreeMap<String, Vec<Rule>>,
    pub remediation: Vec<Remediation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub sources: BTreeMap<String, String>,
    /// Years where ratings change, for a year slider.
    pub milestones: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub catalog_version: String,
    /// `None` while the catalog is an unreviewed seed.
    pub last_reviewed: Option<String>,
    pub default_profile: String,
    pub profiles: BTreeMap<String, Profile>,
    pub algorithms: Vec<Algorithm>,
}

/// One catalog algorithm with the parameters found for it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetParams {
    pub algorithm_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_bits: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_bits: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param_set: Option<String>,
}

impl AssetParams {
    pub fn new(id: &str) -> AssetParams {
        AssetParams { algorithm_id: id.to_string(), ..Default::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Evaluation {
    pub status: Status,
    pub source: Option<Source>,
    pub note: Option<String>,
    /// When the status is unknown because the asset lacks what a rule depends on.
    pub missing: Vec<Param>,
}

impl Evaluation {
    fn unknown(source: Option<Source>, missing: Vec<Param>) -> Evaluation {
        Evaluation { status: Status::Unknown, source, note: None, missing }
    }
}

/// The catalog that ships with coxswain.
pub fn catalog() -> &'static Catalog {
    static CATALOG: LazyLock<Catalog> =
        LazyLock::new(|| serde_json::from_str(include_str!("catalog.json")).expect("the vendored catalog parses"));
    &CATALOG
}

pub struct Policy {
    catalog: &'static Catalog,
    by_id: HashMap<&'static str, &'static Algorithm>,
    aliases: HashMap<String, String>,
    /// OIDs the catalog knows: algorithm and implied parameter set.
    oids: HashMap<&'static str, (&'static str, Option<&'static str>)>,
}

/// The shipped catalog, indexed once.
pub fn policy() -> &'static Policy {
    static POLICY: LazyLock<Policy> = LazyLock::new(|| Policy::new(catalog()));
    &POLICY
}

impl Policy {
    pub fn new(catalog: &'static Catalog) -> Policy {
        let mut aliases = HashMap::new();
        let mut oids = HashMap::new();
        for a in &catalog.algorithms {
            for alias in &a.aliases {
                aliases.insert(normalize(alias), a.id.clone());
            }
            for (oid, p) in &a.oids {
                oids.insert(oid.as_str(), (a.id.as_str(), p.param_set.as_deref()));
            }
        }
        for (k, id) in FAMILY_ALIASES {
            aliases.entry(k.to_string()).or_insert_with(|| id.to_string());
        }
        let by_id = catalog.algorithms.iter().map(|a| (a.id.as_str(), a)).collect();
        Policy { catalog, by_id, aliases, oids }
    }

    pub fn catalog(&self) -> &'static Catalog {
        self.catalog
    }

    pub fn algorithm(&self, id: &str) -> Option<&'static Algorithm> {
        self.by_id.get(id).copied()
    }

    fn rules(&self, algorithm: &'static Algorithm, profile: &str) -> &'static [Rule] {
        algorithm.rules.get(profile).or_else(|| algorithm.rules.get("*")).map_or(&[], Vec::as_slice)
    }

    /// The status of one asset under a profile in a year.
    pub fn evaluate(&self, asset: &AssetParams, profile: &str, year: i32) -> Evaluation {
        let Some(algorithm) = self.algorithm(&asset.algorithm_id) else { return Evaluation::unknown(None, vec![]) };
        for rule in self.rules(algorithm, profile) {
            match matches(rule.when.as_ref(), asset) {
                Ok(false) => continue,
                // A rule that cannot be decided means no honest rating: never skip to a laxer rule.
                Err(missing) => return Evaluation::unknown(Some(rule.source.clone()), missing),
                Ok(true) => {}
            }
            let step = rule.timeline.iter().take_while(|s| s.from <= year).last();
            return match step {
                Some(step) => Evaluation {
                    status: step.status,
                    source: Some(rule.source.clone()),
                    note: rule.note.clone(),
                    missing: vec![],
                },
                None => Evaluation::unknown(Some(rule.source.clone()), vec![]),
            };
        }
        Evaluation::unknown(None, vec![])
    }

    /// The advice that fits this asset under a profile.
    pub fn remediation(&self, asset: &AssetParams, profile: &str) -> Vec<&'static Remediation> {
        let Some(algorithm) = self.algorithm(&asset.algorithm_id) else { return vec![] };
        algorithm
            .remediation
            .iter()
            .filter(|r| r.profiles.as_ref().is_none_or(|p| p.iter().any(|p| p == profile)))
            .filter(|r| matches(r.when.as_ref(), asset) == Ok(true))
            .collect()
    }

    pub fn milestones(&self, profile: &str) -> &'static [i32] {
        self.catalog.profiles.get(profile).map_or(&[], |p| p.milestones.as_slice())
    }
}

/// Whether the asset meets the condition, or which parameters it lacks to tell.
fn matches(when: Option<&Condition>, a: &AssetParams) -> Result<bool, Vec<Param>> {
    let Some(when) = when else { return Ok(true) };
    let mut missing = vec![];
    let mut ok = true;
    let mut number = |ops: &Option<NumOps>, value: Option<u64>, param| {
        if let Some(ops) = ops {
            match value {
                Some(v) => ok &= numeric(ops, v),
                None => missing.push(param),
            }
        }
    };
    number(&when.key_bits, a.key_bits, Param::KeyBits);
    number(&when.security_bits, a.security_bits, Param::SecurityBits);
    if let Some(ops) = &when.param_set {
        match &a.param_set {
            Some(v) => {
                ok &= ops.eq.as_ref().is_none_or(|e| e == v) && ops.one_of.as_ref().is_none_or(|l| l.contains(v));
            }
            None => missing.push(Param::ParamSet),
        }
    }
    if missing.is_empty() { Ok(ok) } else { Err(missing) }
}

fn numeric(o: &NumOps, v: u64) -> bool {
    o.lt.is_none_or(|x| v < x)
        && o.lte.is_none_or(|x| v <= x)
        && o.gt.is_none_or(|x| v > x)
        && o.gte.is_none_or(|x| v >= x)
        && o.eq.is_none_or(|x| v == x)
        && o.one_of.as_ref().is_none_or(|l| l.contains(&v))
}

/// Ignores case and punctuation: "SHA-1", "sha1" and "SHA_1" are all "sha1".
pub fn normalize(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

// ------------------------------------------------------------------------------------------
// Resolution. This part says what an OID or a name is; how good it is lives in the catalog.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SignalSource {
    Oid,
    Family,
    Name,
}

/// One vote on what an algorithm is.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Signal {
    pub source: SignalSource,
    pub ids: Vec<String>,
    /// Whether it sided with the winner; one that did not is reported.
    pub agreed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Resolution {
    Rated { parts: Vec<AssetParams>, signals: Vec<Signal> },
    /// Padding, a KDF, a random number generator or a hybrid combiner: nothing to rate on its own.
    NotRated { primitive: Option<String> },
    /// No vote at all, or votes that disagree without a name to settle it.
    Unresolved { signals: Vec<Signal> },
}

struct KnownOid {
    ids: &'static [&'static str],
    param_set: Option<(&'static str, &'static str)>,
    key_bits: Option<u64>,
    curve: Option<&'static str>,
}

const fn oid(ids: &'static [&'static str]) -> KnownOid {
    KnownOid { ids, param_set: None, key_bits: None, curve: None }
}

const fn oid_sha2(ids: &'static [&'static str], set: &'static str) -> KnownOid {
    KnownOid { ids, param_set: Some(("sha2", set)), key_bits: None, curve: None }
}

const fn oid_curve(curve: &'static str) -> KnownOid {
    KnownOid { ids: &["ecc"], param_set: None, key_bits: None, curve: Some(curve) }
}

/// OIDs the catalog does not list, mostly signature schemes that name two algorithms.
static KNOWN_OIDS: LazyLock<HashMap<String, KnownOid>> = LazyLock::new(|| {
    let mut m: HashMap<String, KnownOid> = [
        // PKCS#1 signature schemes: hash and RSA
        ("1.2.840.113549.1.1.4", oid(&["md5", "rsa"])),
        ("1.2.840.113549.1.1.5", oid(&["sha1", "rsa"])),
        ("1.2.840.113549.1.1.7", oid(&["rsa"])),  // RSAES-OAEP
        ("1.2.840.113549.1.1.10", oid(&["rsa"])), // RSASSA-PSS
        ("1.2.840.113549.1.1.11", oid_sha2(&["sha2", "rsa"], "256")),
        ("1.2.840.113549.1.1.12", oid_sha2(&["sha2", "rsa"], "384")),
        ("1.2.840.113549.1.1.13", oid_sha2(&["sha2", "rsa"], "512")),
        ("1.2.840.113549.1.1.14", oid_sha2(&["sha2", "rsa"], "224")),
        // ECDSA signature schemes
        ("1.2.840.10045.4.1", oid(&["sha1", "ecc"])),
        ("1.2.840.10045.4.3.1", oid_sha2(&["sha2", "ecc"], "224")),
        ("1.2.840.10045.4.3.2", oid_sha2(&["sha2", "ecc"], "256")),
        ("1.2.840.10045.4.3.3", oid_sha2(&["sha2", "ecc"], "384")),
        ("1.2.840.10045.4.3.4", oid_sha2(&["sha2", "ecc"], "512")),
        ("1.2.840.10040.4.3", oid(&["sha1", "dsa"])),
        // Named curves and ECDH
        ("1.3.132.0.33", oid_curve("secp224r1")),
        ("1.2.840.10045.3.1.7", oid_curve("secp256r1")),
        ("1.3.132.0.10", oid_curve("secp256k1")),
        ("1.3.132.0.34", oid_curve("secp384r1")),
        ("1.3.132.0.35", oid_curve("secp521r1")),
        ("1.3.132.1.12", oid(&["ecc"])), // id-ecDH
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    // The NIST AES arc 2.16.840.1.101.3.4.1.{1-9, 21-29, 41-49}: modes of AES-128, -192 and -256.
    for (base, bits) in [(0, 128), (20, 192), (40, 256)] {
        for mode in 1..=9 {
            let entry = KnownOid { ids: &["aes"], param_set: None, key_bits: Some(bits), curve: None };
            m.insert(format!("2.16.840.1.101.3.4.1.{}", base + mode), entry);
        }
    }
    m
});

/// `algorithmFamily` values that are not plain catalog aliases.
const FAMILY_ALIASES: [(&str, &str); 8] = [
    ("sha2", "sha2"),
    ("sha3", "sha3"),
    ("rsassapkcs1", "rsa"),
    ("rsaespkcs1", "rsa"),
    ("mlkem", "ml-kem"),
    ("mldsa", "ml-dsa"),
    ("slhdsa", "slh-dsa"),
    ("tripledes", "3des"),
];

/// Classical strength in bits, by normalized curve name.
fn curve_bits(curve: &str) -> Option<u64> {
    Some(match normalize(curve).as_str() {
        "p192" | "secp192r1" | "prime192v1" => 96,
        "p224" | "secp224r1" => 112,
        "p256" | "secp256r1" | "prime256v1" | "secp256k1" | "brainpoolp256r1" => 128,
        "p384" | "secp384r1" | "brainpoolp384r1" => 192,
        "p521" | "secp521r1" | "brainpoolp512r1" => 256,
        "curve25519" | "x25519" | "ed25519" => 128,
        "curve448" | "x448" | "ed448" => 224,
        _ => return None,
    })
}

const NOT_RATED_PRIMITIVES: [&str; 4] = ["kdf", "drbg", "combiner", "xof"];

static NOT_RATED_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(mgf1?|oaep|pss|pkcs1|pkcs5|pkcs7|nopadding|hkdf.*|pbkdf2.*|.*kdf|tlsprf.*|.*drbg|sha1prng|nativeprng|securerandom)$")
        .unwrap()
});
static WITH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(.+?)with(.+?)(encryption)?$").unwrap());
static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-_/\s]+").unwrap());
static CURVE_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(secp\d+[rk]1|prime\d+v1|brainpoolp\d+r1|p\d{3}|curve(25519|448))$").unwrap());
static LETTERS_DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^([a-z]+)(\d+)$").unwrap());

/// What an OID says about one of the algorithms it names.
#[derive(Clone, Debug, Default)]
struct OidHint {
    param_set: Option<&'static str>,
    key_bits: Option<u64>,
    curve: Option<&'static str>,
}

impl Policy {
    /// The algorithms an OID names, with what it says about each.
    fn oid_signal(&self, oid: Option<&str>) -> Option<(Vec<String>, HashMap<&'static str, OidHint>)> {
        let oid = oid?;
        if let Some(&(id, param_set)) = self.oids.get(oid) {
            let hint = OidHint { param_set, ..Default::default() };
            return Some((vec![id.to_string()], [(id, hint)].into_iter().collect()));
        }
        let known = KNOWN_OIDS.get(oid)?;
        let mut hints: HashMap<&'static str, OidHint> = HashMap::new();
        if let Some((id, set)) = known.param_set {
            hints.entry(id).or_default().param_set = Some(set);
        }
        if let Some(bits) = known.key_bits {
            hints.entry("aes").or_default().key_bits = Some(bits);
        }
        if let Some(curve) = known.curve {
            hints.entry("ecc").or_default().curve = Some(curve);
        }
        Some((known.ids.iter().map(|s| s.to_string()).collect(), hints))
    }

    /// What an algorithm asset is, from its name and what the BOM says about it.
    pub fn resolve(&self, name: &str, info: &AlgorithmInfo) -> Resolution {
        let primitive = info.primitive.as_deref().filter(|p| NOT_RATED_PRIMITIVES.contains(p));
        if primitive.is_some() || NOT_RATED_NAME.is_match(&normalize(name)) {
            return Resolution::NotRated { primitive: primitive.map(str::to_string) };
        }

        let parsed = parse_name(name, &self.aliases);
        let oid = self.oid_signal(info.oid.as_deref());
        let family_id = info.family.as_deref().and_then(|f| self.aliases.get(&normalize(f)));

        let mut signals = vec![];
        if let Some((ids, _)) = &oid {
            signals.push(Signal { source: SignalSource::Oid, ids: ids.clone(), agreed: false });
        }
        if let Some(id) = family_id {
            signals.push(Signal { source: SignalSource::Family, ids: vec![id.clone()], agreed: false });
        }
        if !parsed.ids.is_empty() {
            signals.push(Signal { source: SignalSource::Name, ids: parsed.ids.clone(), agreed: false });
        }
        if signals.is_empty() {
            return Resolution::Unresolved { signals };
        }

        // Majority vote: signals agree when they share an algorithm.
        let agreeing: Vec<Vec<usize>> = signals
            .iter()
            .map(|s| (0..signals.len()).filter(|&t| signals[t].ids.iter().any(|id| s.ids.contains(id))).collect())
            .collect();
        let mut best = 0;
        for i in 1..signals.len() {
            let more = agreeing[i].len() > agreeing[best].len();
            // On a tie the name wins: OIDs are copied between files by hand, and wrong more often.
            let tie_to_name = agreeing[i].len() == agreeing[best].len() && signals[i].source == SignalSource::Name;
            if more || tie_to_name {
                best = i;
            }
        }
        let winners = agreeing[best].clone();
        if winners.len() * 2 <= signals.len() && signals[best].source != SignalSource::Name {
            return Resolution::Unresolved { signals };
        }
        for &w in &winners {
            signals[w].agreed = true;
        }

        // The parts: the most detailed agreeing signal (a composite names more than a family).
        let mut ids: &[String] = &[];
        for &w in &winners {
            let s = &signals[w];
            if s.ids.len() > ids.len() || (s.ids.len() == ids.len() && s.source == SignalSource::Name) {
                ids = &s.ids;
            }
        }
        let oid_agreed = winners.iter().any(|&w| signals[w].source == SignalSource::Oid);
        let composite = ids.len() > 1;
        let param_set = info.param_set.as_deref();
        let param_number = param_set.filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).and_then(|s| s.parse::<u64>().ok());

        let parts = ids
            .iter()
            .map(|id| {
                let mut p = AssetParams::new(id);
                let hint = oid.as_ref().filter(|_| oid_agreed).and_then(|(_, hints)| hints.get(id.as_str()));
                let OidHint { param_set: oid_set, key_bits: oid_bits, curve: oid_curve } = hint.cloned().unwrap_or_default();
                let digits = parsed.digits.get(id).map(String::as_str);
                match id.as_str() {
                    "rsa" | "ffdh" | "dsa" => {
                        // parameterSetIdentifier is the hash size for RSA signature schemes, not the key.
                        let from_param = param_number.filter(|&n| !composite && n >= 512);
                        p.key_bits = parsed.numbers.iter().copied().find(|&n| n >= 512).or(from_param);
                    }
                    "aes" => {
                        let candidates = [digits.and_then(|d| d.parse().ok())]
                            .into_iter()
                            .chain(parsed.numbers.iter().map(|&n| Some(n)))
                            .chain([param_number, info.classical_security_level, oid_bits]);
                        p.key_bits = candidates.flatten().find(|n| [128, 192, 256].contains(n));
                    }
                    "ecc" => {
                        let curve = info.curve.as_deref().or(parsed.curve.as_deref()).or(oid_curve);
                        p.security_bits = curve.and_then(curve_bits);
                    }
                    "sha2" | "sha3" => {
                        let own = if composite { None } else { param_set };
                        p.param_set = pick(&[digits, oid_set, own], &["224", "256", "384", "512"]);
                    }
                    "hmac" => {
                        let digits = if digits == Some("1") { Some("160") } else { digits };
                        p.param_set = pick(&[digits, oid_set, param_set], &["160", "224", "256", "384", "512"]);
                    }
                    "ml-kem" | "ml-dsa" => {
                        let allowed: &[&str] = if id == "ml-kem" { &["512", "768", "1024"] } else { &["44", "65", "87"] };
                        let numbers: Vec<String> = parsed.numbers.iter().map(u64::to_string).collect();
                        let mut candidates = vec![digits];
                        candidates.extend(numbers.iter().map(|n| Some(n.as_str())));
                        candidates.extend([oid_set, param_set]);
                        p.param_set = pick(&candidates, allowed);
                    }
                    _ => {}
                }
                p
            })
            .collect();
        Resolution::Rated { parts, signals }
    }
}

fn pick(candidates: &[Option<&str>], allowed: &[&str]) -> Option<String> {
    candidates.iter().flatten().find(|c| allowed.contains(c)).map(|c| c.to_string())
}

#[derive(Debug, Default)]
struct ParsedName {
    ids: Vec<String>,
    /// Digits that came with an algorithm's own token: AES128 gives 128, SHA-384 gives 384.
    digits: HashMap<String, String>,
    /// Numbers standing on their own: RSA-2048 gives 2048.
    numbers: Vec<u64>,
    curve: Option<String>,
}

impl ParsedName {
    fn add(&mut self, id: &str, digits: Option<&str>) {
        if !self.ids.iter().any(|i| i == id) {
            self.ids.push(id.to_string());
        }
        if let Some(d) = digits {
            self.digits.entry(id.to_string()).or_insert_with(|| d.to_string());
        }
    }
}

fn trailing_digits(s: &str) -> Option<&str> {
    let start = s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    (start < s.len()).then(|| &s[start..])
}

/// The last run of digits anywhere in a string.
fn last_digits(s: &str) -> Option<&str> {
    let end = s.rfind(|c: char| c.is_ascii_digit())? + 1;
    trailing_digits(&s[..end])
}

/// Splits a name into catalog algorithms: the whole name, then "XwithY", then the longest run of
/// up to three tokens that is an alias ("ML" "KEM" is ml-kem, "AES128" is aes with 128).
fn parse_name(name: &str, aliases: &HashMap<String, String>) -> ParsedName {
    let mut out = ParsedName::default();
    if let Some(whole) = aliases.get(&normalize(name)) {
        out.add(whole, last_digits(name));
        return out;
    }

    if let Some(m) = WITH.captures(name) {
        for side in [&m[1], &m[2]] {
            let p = parse_name(side, aliases);
            for id in &p.ids {
                out.add(id, p.digits.get(id).map(String::as_str));
            }
            out.numbers.extend(p.numbers);
            if out.curve.is_none() {
                out.curve = p.curve;
            }
        }
        return out;
    }

    let tokens: Vec<&str> = SEPARATORS.split(name).filter(|t| !t.is_empty()).collect();
    let mut last: Option<String> = None;
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if t.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(n) = t.parse() {
                out.numbers.push(n);
            }
            i += 1;
            continue;
        }
        if CURVE_TOKEN.is_match(t) {
            out.curve = Some(t.to_string());
            i += 1;
            continue;
        }
        let mut matched = false;
        for w in (1..=3.min(tokens.len() - i)).rev() {
            let window = tokens[i..i + w].concat();
            if let Some(id) = aliases.get(&normalize(&window)) {
                out.add(id, trailing_digits(&window));
                last = Some(id.clone());
                i += w;
                matched = true;
                break;
            }
            if w == 1
                && let Some(m) = LETTERS_DIGITS.captures(&window)
                && let Some(base) = aliases.get(&normalize(&m[1]))
            {
                out.add(base, Some(&m[2]));
                last = Some(base.clone());
                i += 1;
                matched = true;
                break;
            }
        }
        if !matched {
            i += 1; // padding and version tokens (PKCS1, 1.5, GCM, …) say nothing about identity
        }
    }
    // "ML-KEM 1024": a number right after a parameterised algorithm is its parameter set.
    if let Some(last) = last
        && (last == "ml-kem" || last == "ml-dsa")
        && !out.digits.contains_key(&last)
        && !out.numbers.is_empty()
    {
        let n = out.numbers.remove(0);
        out.digits.insert(last, n.to_string());
    }
    out
}
