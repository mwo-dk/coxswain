//! Two provenance files compared: what differs between two builds. v0.2 and v1 compare alike,
//! since both reach the model in v1's names.

use super::model::*;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Area {
    Builder,
    Parameter,
    Dependency,
    Subject,
    Signer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Added,
    Removed,
    Changed,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Change {
    pub area: Area,
    /// What changed: `buildType`, `external.workflow.ref`, a dependency's URI, a subject's name.
    pub key: String,
    pub kind: Kind,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// One statement of each file, paired.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Pair {
    pub before: usize,
    pub after: usize,
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub pairs: Vec<Pair>,
    /// Statements with no partner, by index.
    pub only_before: Vec<usize>,
    pub only_after: Vec<usize>,
}

impl Diff {
    /// How many changes of an area, over all pairs.
    pub fn count(&self, area: Area) -> usize {
        self.pairs.iter().flat_map(|p| &p.changes).filter(|c| c.area == area).count()
    }
}

/// Statements are paired by a shared subject name; two files of one statement each always pair.
pub fn diff(before: &Attestations, after: &Attestations) -> Diff {
    let mut out = Diff::default();
    let mut taken = vec![false; before.entries.len()];
    for (j, a) in after.entries.iter().enumerate() {
        let i = if before.entries.len() == 1 && after.entries.len() == 1 {
            Some(0)
        } else {
            before.entries.iter().enumerate().position(|(i, b)| {
                !taken[i]
                    && b.statement.predicate_type == a.statement.predicate_type
                    && b.statement.subjects.iter().any(|s| a.statement.subjects.iter().any(|t| s.label() == t.label()))
            })
        };
        match i {
            Some(i) => {
                taken[i] = true;
                out.pairs.push(Pair { before: i, after: j, changes: entry(&before.entries[i], a) });
            }
            None => out.only_after.push(j),
        }
    }
    out.only_before = taken.iter().enumerate().filter(|(_, t)| !**t).map(|(i, _)| i).collect();
    out
}

fn entry(b: &Entry, a: &Entry) -> Vec<Change> {
    let mut out = Vec::new();
    let resources = |area, out: &mut Vec<Change>, b: &[Resource], a: &[Resource], key: fn(&Resource) -> String, value: fn(&Resource) -> String| {
        let bm: BTreeMap<String, String> = b.iter().map(|r| (key(r), value(r))).collect();
        let am: BTreeMap<String, String> = a.iter().map(|r| (key(r), value(r))).collect();
        maps(area, out, &bm, &am);
    };
    resources(Area::Subject, &mut out, &b.statement.subjects, &a.statement.subjects, |r| r.label().to_string(), digests);
    if let (Predicate::Provenance(b), Predicate::Provenance(a)) = (&b.statement.predicate, &a.statement.predicate) {
        let builder = |p: &Provenance| {
            let mut m = BTreeMap::from([("builder.id".to_string(), p.builder.id.clone()), ("buildType".to_string(), p.build_type.clone())]);
            m.extend(p.builder.version.iter().map(|(k, v)| (format!("builder.version.{k}"), v.clone())));
            m
        };
        maps(Area::Builder, &mut out, &builder(b), &builder(a));
        let params = |p: &Provenance| {
            let mut m = BTreeMap::new();
            flatten("external", &p.external, &mut m);
            flatten("internal", &p.internal, &mut m);
            m
        };
        maps(Area::Parameter, &mut out, &params(b), &params(a));
        resources(Area::Dependency, &mut out, &b.dependencies, &a.dependencies, dependency_key, dependency_value);
    }
    if let (Some(bs), Some(as_)) = (&b.signer, &a.signer) {
        let claims = |s: &Signer| {
            let mut m: BTreeMap<String, String> = s.claims.iter().map(|(c, v)| (format!("{c:?}"), v.clone())).collect();
            m.insert("identity".into(), s.identity.clone());
            m
        };
        maps(Area::Signer, &mut out, &claims(bs), &claims(as_));
    }
    out
}

fn maps(area: Area, out: &mut Vec<Change>, b: &BTreeMap<String, String>, a: &BTreeMap<String, String>) {
    for (k, bv) in b {
        match a.get(k) {
            None => out.push(Change { area, key: k.clone(), kind: Kind::Removed, before: Some(bv.clone()), after: None }),
            Some(av) if av != bv => out.push(Change { area, key: k.clone(), kind: Kind::Changed, before: Some(bv.clone()), after: Some(av.clone()) }),
            Some(_) => {}
        }
    }
    for (k, av) in a.iter().filter(|(k, _)| !b.contains_key(*k)) {
        out.push(Change { area, key: k.clone(), kind: Kind::Added, before: None, after: Some(av.clone()) });
    }
}

/// A dependency by what it is, not which version: its URI without `@ref`, else its name.
fn dependency_key(r: &Resource) -> String {
    match r.uri.as_deref() {
        Some(u) if u.starts_with("git+") => u.rsplit_once('@').filter(|(head, _)| head.contains("://")).map_or(u, |(head, _)| head).to_string(),
        Some(u) => u.split('?').next().unwrap_or(u).to_string(),
        None => r.label().to_string(),
    }
}

fn dependency_value(r: &Resource) -> String {
    let reference = r.uri.as_deref().and_then(|u| u.strip_prefix(&dependency_key(r))).unwrap_or("");
    let d = digests(r);
    [reference.trim_start_matches(['@', '?']), &d].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ")
}

fn digests(r: &Resource) -> String {
    r.digest.iter().map(|(a, d)| format!("{a}:{d}")).collect::<Vec<_>>().join(" ")
}

/// Leaves of a JSON value as `prefix.a.b[0]` → text.
fn flatten(prefix: &str, v: &Value, out: &mut BTreeMap<String, String>) {
    match v {
        Value::Object(o) => o.iter().for_each(|(k, v)| flatten(&format!("{prefix}.{k}"), v, out)),
        Value::Array(a) => a.iter().enumerate().for_each(|(i, v)| flatten(&format!("{prefix}[{i}]"), v, out)),
        Value::Null => {}
        Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}
