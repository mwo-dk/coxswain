//! A status for every node of the tree, and why.
//!
//! Resolution runs once per BOM ([`resolve`]); assessment is cheap enough to run again for
//! another profile or year ([`assess`]). A crypto asset is rated on its own:
//!
//! - an algorithm by the catalog, as the worst of its parts;
//! - a key by its own size through its algorithm, else it takes its algorithm's status;
//! - a certificate by what it is signed with, its key and its expiry date;
//! - a protocol by its cipher suites' algorithms.
//!
//! Components, groups and the root take the worst asset beneath them.

use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

use super::model::*;
use super::policy::{AssetParams, Evaluation, Policy, Resolution, SignalSource};
use super::status::Status;
use super::tree::Tree;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unresolved {
    /// No name, OID or family the catalog knows, or votes that disagree.
    UnknownAlgorithm,
    KeyNamesNoAlgorithm,
    /// A certificate with no signature algorithm, key or validity dates.
    NoCertificateFacts,
    NoCipherSuites,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Reason {
    /// Rated by a catalog rule.
    Rule { params: AssetParams, evaluation: Evaluation },
    /// Padding, a KDF, a random number generator or a combiner.
    NotRated { primitive: Option<String> },
    Unresolved { why: Unresolved },
    /// A key with no size of its own takes the status of its algorithm.
    Inherited { node: u32, status: Status },
    /// A certificate or protocol includes the assets it refers to.
    Reference { edge: EdgeKind, node: u32, status: Status },
    /// A certificate's expiry: expired is disallowed, and soon is deprecated.
    #[serde(rename_all = "camelCase")]
    Lifecycle { not_valid_after: String, days_left: i64, status: Status },
    /// A component or group: the asset beneath it that sets its status.
    Rollup { node: u32, status: Status },
}

impl Reason {
    fn status(&self) -> Status {
        match self {
            Reason::Rule { evaluation, .. } => evaluation.status,
            Reason::NotRated { .. } => Status::NotRated,
            Reason::Unresolved { .. } => Status::Unknown,
            Reason::Inherited { status, .. }
            | Reason::Reference { status, .. }
            | Reason::Lifecycle { status, .. }
            | Reason::Rollup { status, .. } => *status,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Assessment {
    pub status: Status,
    pub reasons: Vec<Reason>,
}

fn fold(reasons: Vec<Reason>) -> Assessment {
    let status = reasons.iter().fold(Status::NotRated, |s, r| s.worst(r.status()));
    Assessment { status, reasons }
}

/// What to rate by.
#[derive(Clone, Debug)]
pub struct Context {
    pub profile: String,
    pub year: i32,
    /// Seconds since the Unix epoch.
    pub now: i64,
    /// Certificates that expire within this many days are deprecated.
    pub expiry_warning_days: i64,
}

impl Context {
    /// The catalog's default profile, this year, today.
    pub fn today(policy: &Policy) -> Context {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        Context {
            profile: policy.catalog().default_profile.clone(),
            year: year_of(now),
            now,
            expiry_warning_days: 90,
        }
    }
}

/// Resolves every algorithm once. Also returns the issues that come of it: names nothing knows,
/// and OIDs or families outvoted by the rest.
pub fn resolve(bom: &Bom, policy: &Policy) -> (Vec<Option<Resolution>>, Vec<Issue>) {
    let mut issues = vec![];
    let resolutions = bom
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let Some(Crypto::Algorithm(info)) = &n.crypto else { return None };
            let r = policy.resolve(&n.label, info);
            match &r {
                Resolution::Unresolved { signals } => {
                    let why = if signals.is_empty() {
                        "not a known algorithm".to_string()
                    } else {
                        let votes: Vec<String> = signals.iter().map(|s| format!("{} says {}", source(s.source), s.ids.join("+"))).collect();
                        format!("conflicting signals: {}", votes.join(", "))
                    };
                    issues.push(Issue {
                        severity: Severity::Info,
                        code: IssueCode::UnresolvedAlgorithm,
                        message: format!("\"{}\" couldn't be identified ({why}).", n.label),
                        node: Some(i as u32),
                    });
                }
                Resolution::Rated { parts, signals } => {
                    let outvoted: Vec<String> = signals
                        .iter()
                        .filter(|s| !s.agreed)
                        .map(|s| {
                            let oid = match (s.source, &info.oid) {
                                (SignalSource::Oid, Some(oid)) => format!(" {oid}"),
                                _ => String::new(),
                            };
                            format!("{}{oid} suggests {}", source(s.source), s.ids.join(" + "))
                        })
                        .collect();
                    if !outvoted.is_empty() {
                        let winner: Vec<&str> = parts.iter().map(|p| p.algorithm_id.as_str()).collect();
                        issues.push(Issue {
                            severity: Severity::Warning,
                            code: IssueCode::OidMismatch,
                            message: format!(
                                "\"{}\": {}, but the other signals say {}.",
                                n.label,
                                outvoted.join("; "),
                                winner.join(" + ")
                            ),
                            node: Some(i as u32),
                        });
                    }
                }
                Resolution::NotRated { .. } => {}
            }
            Some(r)
        })
        .collect();
    (resolutions, issues)
}

fn source(s: SignalSource) -> &'static str {
    match s {
        SignalSource::Oid => "oid",
        SignalSource::Family => "family",
        SignalSource::Name => "name",
    }
}

/// The statuses of a tree, and what explains them.
#[derive(Clone, Debug)]
pub struct Assessed {
    /// By tree node.
    pub status: Vec<Status>,
    /// By tree node: the asset that sets a component's or group's status.
    cause: Vec<Option<u32>>,
    /// By BOM node: a crypto asset's own assessment.
    own: Vec<Option<Assessment>>,
}

impl Assessed {
    /// Why a node has its status, for the details box.
    pub fn explain(&self, i: u32) -> Assessment {
        if let Some(Some(own)) = self.own.get(i as usize) {
            return own.clone();
        }
        let status = self.status[i as usize];
        let reasons = match self.cause[i as usize] {
            Some(c) if status != Status::NotRated => vec![Reason::Rollup { node: c, status: self.status[c as usize] }],
            _ => vec![],
        };
        Assessment { status, reasons }
    }
}

fn is_crypto(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Algorithm | NodeKind::Material | NodeKind::Certificate | NodeKind::Protocol)
}

/// Algorithms whose size is a key length: a key's own `size` is theirs.
const SIZED: [&str; 4] = ["rsa", "ffdh", "dsa", "aes"];

/// How long a chain of assets referring to each other may be before it counts as circular.
const MAX_CHAIN: usize = 64;

enum Memo {
    Todo,
    Pending,
    Done(Assessment),
}

struct Assessor<'a> {
    bom: &'a Bom,
    policy: &'a Policy,
    resolutions: &'a [Option<Resolution>],
    ctx: &'a Context,
    out: Vec<Vec<Edge>>,
    memo: Vec<Memo>,
    /// The time certificates are checked against: now, or the start of a future year.
    at: i64,
}

impl Assessor<'_> {
    fn own(&mut self, i: u32, depth: usize) -> Status {
        match &self.memo[i as usize] {
            Memo::Done(a) => return a.status,
            // assets that refer to each other in a circle
            Memo::Pending => return Status::Unknown,
            Memo::Todo if depth > MAX_CHAIN => return Status::Unknown,
            Memo::Todo => {}
        }
        self.memo[i as usize] = Memo::Pending;
        let a = self.leaf(i, depth);
        let status = a.status;
        self.memo[i as usize] = Memo::Done(a);
        status
    }

    fn rate(&self, params: AssetParams) -> Reason {
        let evaluation = self.policy.evaluate(&params, &self.ctx.profile, self.ctx.year);
        Reason::Rule { params, evaluation }
    }

    fn references(&mut self, i: u32, depth: usize, kinds: &[EdgeKind]) -> Vec<Reason> {
        let edges: Vec<Edge> = self.out[i as usize].iter().copied().filter(|e| kinds.contains(&e.kind)).collect();
        edges
            .into_iter()
            .map(|e| {
                let status = self.own(e.to, depth + 1);
                Reason::Reference { edge: e.kind, node: e.to, status }
            })
            .collect()
    }

    fn leaf(&mut self, i: u32, depth: usize) -> Assessment {
        let bom = self.bom;
        let node = &bom.nodes[i as usize];
        let unresolved = |why| fold(vec![Reason::Unresolved { why }]);
        match (&node.crypto, node.kind) {
            (_, NodeKind::Algorithm) => match &self.resolutions[i as usize] {
                Some(Resolution::Rated { parts, .. }) => fold(parts.iter().map(|p| self.rate(p.clone())).collect()),
                Some(Resolution::NotRated { primitive }) => {
                    Assessment { status: Status::NotRated, reasons: vec![Reason::NotRated { primitive: primitive.clone() }] }
                }
                _ => unresolved(Unresolved::UnknownAlgorithm),
            },
            (crypto, NodeKind::Material) => {
                let targets: Vec<u32> = self.out[i as usize]
                    .iter()
                    .filter(|e| matches!(e.kind, EdgeKind::UsesAlgorithm | EdgeKind::DependsOn))
                    .filter(|e| bom.nodes[e.to as usize].kind == NodeKind::Algorithm)
                    .map(|e| e.to)
                    .collect();
                let size = match crypto {
                    Some(Crypto::Material(m)) => m.size,
                    _ => None,
                };
                if let Some(size) = size {
                    let sized: Vec<Reason> = targets
                        .iter()
                        .filter_map(|&t| match &self.resolutions[t as usize] {
                            Some(Resolution::Rated { parts, .. }) => parts.iter().find(|p| SIZED.contains(&p.algorithm_id.as_str())),
                            _ => None,
                        })
                        .map(|part| self.rate(AssetParams { key_bits: Some(size), ..part.clone() }))
                        .collect();
                    if !sized.is_empty() {
                        return fold(sized);
                    }
                }
                if targets.is_empty() {
                    return unresolved(Unresolved::KeyNamesNoAlgorithm);
                }
                let reasons = targets
                    .into_iter()
                    .map(|t| Reason::Inherited { node: t, status: self.own(t, depth + 1) })
                    .collect();
                fold(reasons)
            }
            (crypto, NodeKind::Certificate) => {
                let mut reasons = self.references(i, depth, &[EdgeKind::SignedWith, EdgeKind::HasKey]);
                let not_after = match crypto {
                    Some(Crypto::Certificate(c)) => c.not_valid_after.as_deref(),
                    _ => None,
                };
                if let Some(nva) = not_after
                    && let Some(expires) = parse_time(nva)
                {
                    let days_left = (expires - self.at).div_euclid(86_400);
                    let status = if days_left < 0 {
                        Status::Disallowed
                    } else if days_left <= self.ctx.expiry_warning_days {
                        Status::Deprecated
                    } else {
                        Status::Acceptable
                    };
                    reasons.push(Reason::Lifecycle { not_valid_after: nva.to_string(), days_left, status });
                }
                if reasons.is_empty() { unresolved(Unresolved::NoCertificateFacts) } else { fold(reasons) }
            }
            (_, NodeKind::Protocol) => {
                let reasons = self.references(i, depth, &[EdgeKind::UsesAlgorithm]);
                if reasons.is_empty() { unresolved(Unresolved::NoCipherSuites) } else { fold(reasons) }
            }
            _ => Assessment { status: Status::NotRated, reasons: vec![] },
        }
    }
}

/// Rates every node of the tree.
pub fn assess(bom: &Bom, tree: &Tree, policy: &Policy, resolutions: &[Option<Resolution>], ctx: &Context) -> Assessed {
    let mut out = vec![vec![]; bom.nodes.len()];
    for &e in &bom.edges {
        out[e.from as usize].push(e);
    }
    let at = if ctx.year > year_of(ctx.now) { start_of_year(ctx.year) } else { ctx.now };
    let mut a = Assessor {
        bom,
        policy,
        resolutions,
        ctx,
        out,
        memo: (0..bom.nodes.len()).map(|_| Memo::Todo).collect(),
        at,
    };

    // Crypto assets carry their own status; the rest take the worst crypto asset beneath them.
    // An asset under another asset (an algorithm under the key that uses it) counts toward the
    // nearest node above that is not one.
    let n = tree.len();
    let crypto = |i: u32| (i as usize) < bom.nodes.len() && is_crypto(bom.nodes[i as usize].kind);
    let rank = |i: u32| match bom.nodes.get(i as usize).map(|n| n.kind) {
        Some(NodeKind::Algorithm) => 0,
        Some(NodeKind::Certificate) => 1,
        Some(NodeKind::Protocol) => 2,
        Some(NodeKind::Material) => 3,
        _ => 9,
    };
    let mut status = vec![Status::NotRated; n];
    let mut cause: Vec<Option<u32>> = vec![None; n];
    // Reverse pre-order visits every node after everything beneath it.
    for &v in tree.order.iter().rev() {
        if crypto(v) {
            status[v as usize] = a.own(v, 0);
        }
        let mut p = tree.parent[v as usize];
        while let Some(q) = p
            && crypto(q)
        {
            p = tree.parent[q as usize];
        }
        let Some(p) = p else { continue };
        let (s, prev) = (status[v as usize], status[p as usize]);
        let merged = prev.worst(s);
        let this_cause = if crypto(v) { v } else { cause[v as usize].unwrap_or(v) };
        let current = cause[p as usize];
        // On a tie, an asset rated on its own explains better than one that inherits (DSA over a DSA key).
        let better = s == merged && current.is_some_and(|c| rank(this_cause) < rank(c));
        if merged != prev || (current.is_none() && s != Status::NotRated) || better {
            status[p as usize] = merged;
            cause[p as usize] = Some(this_cause);
        }
    }

    let own = a
        .memo
        .into_iter()
        .map(|m| match m {
            Memo::Done(a) => Some(a),
            _ => None,
        })
        .collect();
    Assessed { status, cause, own }
}

// Dates, in UTC, without a date crate: the days-from-civil algorithm (Howard Hinnant).

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(super) fn year_of(unix: i64) -> i32 {
    let z = unix.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2)) as i32
}

pub(super) fn start_of_year(year: i32) -> i64 {
    days_from_civil(year as i64, 1, 1) * 86_400
}

/// An ISO 8601 date or date and time: "2017-11-22", "2017-11-22T07:59:59Z", with an optional
/// fraction and offset. No offset is read as UTC.
pub(super) fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let part = s.get(r)?;
        part.bytes().all(|b| b.is_ascii_digit()).then(|| part.parse().ok())?
    };
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    if s.get(4..5) != Some("-") || s.get(7..8) != Some("-") || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut secs = days_from_civil(y, m, d) * 86_400;
    let rest = &s[10..];
    if rest.is_empty() {
        return Some(secs);
    }
    if !rest.starts_with(['T', 't', ' ']) {
        return None;
    }
    let (h, min) = (num(11..13)?, num(14..16)?);
    let sec = if s.get(16..17) == Some(":") { num(17..19)? } else { 0 };
    secs += h * 3600 + min * 60 + sec;
    let tail = s[if s.get(16..17) == Some(":") { 19 } else { 16 }..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match tail {
        "" | "Z" | "z" => 0,
        t if t.len() == 6 && (t.starts_with('+') || t.starts_with('-')) && &t[3..4] == ":" => {
            let (oh, om): (i64, i64) = (t[1..3].parse().ok()?, t[4..6].parse().ok()?);
            let o = oh * 3600 + om * 60;
            if t.starts_with('+') { o } else { -o }
        }
        _ => return None,
    };
    Some(secs - offset)
}

