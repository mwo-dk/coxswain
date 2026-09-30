//! CycloneDX 1.6 and 1.7, in the JSON shape, to a [`Bom`]. XML is turned into this shape first
//! (see `xml.rs`), so both formats share this one reader.
//!
//! Lenient on purpose: real CBOMs have dangling refs, missing bom-refs and odd shapes. Those
//! become issues. Only a document that is not a CycloneDX BOM at all is refused.

use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::model::*;
use super::Error;

type Obj = Map<String, Value>;

/// The spec versions whose crypto we read in full. Others load with a warning.
const SUPPORTED: [&str; 2] = ["1.6", "1.7"];

struct PendingRef<'a> {
    from: u32,
    target: &'a Value,
    kind: EdgeKind,
    field: &'static str,
}

struct Reader<'a> {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    issues: Vec<Issue>,
    by_ref: HashMap<String, u32>,
    pending: Vec<PendingRef<'a>>,
}

pub fn from_value(doc: &Value, format: Format, file_name: Option<&str>) -> Result<Bom, Error> {
    let doc = match doc.as_object() {
        Some(doc) if doc.get("bomFormat").and_then(Value::as_str) == Some("CycloneDX") => doc,
        _ => return Err(Error::NotCycloneDx),
    };
    let mut r = Reader { nodes: vec![], edges: vec![], issues: vec![], by_ref: HashMap::new(), pending: vec![] };
    let spec_version = text(doc.get("specVersion")).unwrap_or("unknown").to_string();
    if !SUPPORTED.contains(&spec_version.as_str()) {
        r.issues.push(Issue {
            severity: Severity::Warning,
            code: IssueCode::UnsupportedSpecVersion,
            message: format!(
                "CycloneDX {spec_version} isn't fully supported (1.6 and 1.7 are). Cryptographic details may be missing."
            ),
            node: None,
        });
    }

    let metadata = obj(doc.get("metadata"));
    let mut properties = BTreeMap::new();
    for p in list(metadata.and_then(|m| m.get("properties"))) {
        if let (Some(name), Some(value)) = (p.get("name").and_then(Value::as_str), p.get("value").and_then(Value::as_str)) {
            properties.insert(name.to_string(), value.to_string());
        }
    }

    // The root: the application the BOM describes, or one made up (CBOMkit output has none).
    match metadata.and_then(|m| obj(m.get("component"))) {
        Some(meta) => {
            r.nodes.push(component_node(meta, NodeKind::Application, "#root".into()));
            r.register_ref(meta, 0);
        }
        None => r.nodes.push(Node::synthetic("#root".into(), NodeKind::Application, root_name(&properties, file_name))),
    }

    // Components, depth first; a nested component gets a `contains` edge from its parent.
    r.walk(doc.get("components"), None);

    for d in list(doc.get("dependencies")) {
        let name = display(d.get("ref"));
        let Some(&from) = d.get("ref").and_then(Value::as_str).and_then(|s| r.by_ref.get(s)) else {
            r.issues.push(Issue {
                severity: Severity::Warning,
                code: IssueCode::DanglingRef,
                message: format!("dependencies: \"{name}\" is not a component in this BOM."),
                node: None,
            });
            continue;
        };
        for t in array(d.get("dependsOn")) {
            r.pending.push(PendingRef { from, target: t, kind: EdgeKind::DependsOn, field: "dependsOn" });
        }
        for t in array(d.get("provides")) {
            r.pending.push(PendingRef { from, target: t, kind: EdgeKind::Provides, field: "provides" });
        }
    }

    let mut seen = HashSet::new();
    for p in std::mem::take(&mut r.pending) {
        let Some(&to) = p.target.as_str().and_then(|s| r.by_ref.get(s)) else {
            r.issues.push(Issue {
                severity: Severity::Warning,
                code: IssueCode::DanglingRef,
                message: format!(
                    "{}: {} refers to \"{}\", which is not in this BOM.",
                    r.nodes[p.from as usize].label,
                    p.field,
                    display(Some(p.target))
                ),
                node: Some(p.from),
            });
            continue;
        };
        let edge = Edge { from: p.from, to, kind: p.kind };
        if to != p.from && seen.insert(edge) {
            r.edges.push(edge);
        }
    }

    Ok(Bom {
        source: Source {
            format,
            spec_version,
            serial_number: text(doc.get("serialNumber")).map(str::to_string),
            timestamp: text(metadata.and_then(|m| m.get("timestamp"))).map(str::to_string),
            file_name: file_name.map(str::to_string),
            properties,
        },
        nodes: r.nodes,
        edges: r.edges,
        issues: r.issues,
    })
}

impl<'a> Reader<'a> {
    fn walk(&mut self, components: Option<&'a Value>, parent: Option<u32>) {
        for c in list(components) {
            let idx = self.nodes.len() as u32;
            self.nodes.push(component_node(c, kind_of(c), format!("#anon-{idx}")));
            self.register_ref(c, idx);
            if let Some(parent) = parent {
                self.edges.push(Edge { from: parent, to: idx, kind: EdgeKind::Contains });
            }
            self.collect_crypto_refs(c, idx);
            self.walk(c.get("components"), Some(idx));
        }
    }

    fn register_ref(&mut self, c: &Obj, idx: u32) {
        let Some(bom_ref) = text(c.get("bom-ref")) else {
            if idx != 0 && c.get("type").and_then(Value::as_str) == Some("cryptographic-asset") {
                self.issues.push(Issue {
                    severity: Severity::Info,
                    code: IssueCode::MissingBomRef,
                    message: format!("\"{}\" has no bom-ref, so nothing can reference it.", self.nodes[idx as usize].label),
                    node: Some(idx),
                });
            }
            return;
        };
        if self.by_ref.contains_key(bom_ref) {
            self.issues.push(Issue {
                severity: Severity::Warning,
                code: IssueCode::DuplicateBomRef,
                message: format!("bom-ref \"{bom_ref}\" is used more than once; the first one wins."),
                node: Some(idx),
            });
            return;
        }
        self.by_ref.insert(bom_ref.to_string(), idx);
    }

    fn collect_crypto_refs(&mut self, c: &'a Obj, from: u32) {
        let Some(cp) = obj(c.get("cryptoProperties")) else { return };
        let mut add = |target: Option<&'a Value>, kind, field| {
            if let Some(target) = target {
                self.pending.push(PendingRef { from, target, kind, field });
            }
        };
        let cert = obj(cp.get("certificateProperties"));
        add(cert.and_then(|p| p.get("signatureAlgorithmRef")), EdgeKind::SignedWith, "signatureAlgorithmRef");
        add(cert.and_then(|p| p.get("subjectPublicKeyRef")), EdgeKind::HasKey, "subjectPublicKeyRef");
        let mat = obj(cp.get("relatedCryptoMaterialProperties"));
        add(mat.and_then(|p| p.get("algorithmRef")), EdgeKind::UsesAlgorithm, "algorithmRef");
        let secured_by = obj(mat.and_then(|p| p.get("securedBy")));
        add(secured_by.and_then(|p| p.get("algorithmRef")), EdgeKind::Uses, "securedBy.algorithmRef");
        let proto = obj(cp.get("protocolProperties"));
        for suite in array(proto.and_then(|p| p.get("cipherSuites"))) {
            for a in array(obj(Some(suite)).and_then(|s| s.get("algorithms"))) {
                add(Some(a), EdgeKind::UsesAlgorithm, "cipherSuites.algorithms");
            }
        }
        for r in array(proto.and_then(|p| p.get("cryptoRefArray"))) {
            add(Some(r), EdgeKind::Uses, "cryptoRefArray");
        }
    }
}

fn component_node(c: &Obj, kind: NodeKind, fallback_key: String) -> Node {
    let bom_ref = text(c.get("bom-ref")).map(str::to_string);
    let cp = obj(c.get("cryptoProperties"));
    let props = |name| obj(cp.and_then(|p| p.get(name)));
    let crypto = match kind {
        NodeKind::Algorithm => {
            Some(Crypto::Algorithm(algorithm_info(props("algorithmProperties"), text(cp.and_then(|p| p.get("oid"))))))
        }
        NodeKind::Certificate => Some(Crypto::Certificate(certificate_info(props("certificateProperties")))),
        NodeKind::Material => Some(Crypto::Material(material_info(props("relatedCryptoMaterialProperties")))),
        NodeKind::Protocol => Some(Crypto::Protocol(protocol_info(props("protocolProperties")))),
        _ => None,
    };
    Node {
        key: bom_ref.clone().unwrap_or(fallback_key),
        kind,
        label: text(c.get("name")).or(bom_ref.as_deref()).unwrap_or("(unnamed)").to_string(),
        version: owned(c.get("version")),
        purl: owned(c.get("purl")),
        component_type: matches!(kind, NodeKind::Component | NodeKind::Application)
            .then(|| text(c.get("type")).unwrap_or("library").to_string()),
        bom_ref,
        synthetic: false,
        group: None,
        occurrences: occurrences(c),
        crypto,
        raw: Value::Object(c.clone()),
    }
}

fn kind_of(c: &Obj) -> NodeKind {
    if c.get("type").and_then(Value::as_str) != Some("cryptographic-asset") {
        return NodeKind::Component;
    }
    match obj(c.get("cryptoProperties")).and_then(|p| p.get("assetType")).and_then(Value::as_str) {
        Some("algorithm") => NodeKind::Algorithm,
        Some("certificate") => NodeKind::Certificate,
        Some("protocol") => NodeKind::Protocol,
        // related-crypto-material, and asset types from 1.5 or unknown ones
        _ => NodeKind::Material,
    }
}

fn algorithm_info(p: Option<&Obj>, oid: Option<&str>) -> AlgorithmInfo {
    let get = |name| p.and_then(|p| p.get(name));
    let curve = text(get("ellipticCurve")).or(text(get("curve")));
    AlgorithmInfo {
        oid: oid.map(str::to_string),
        family: owned(get("algorithmFamily")),
        primitive: owned(get("primitive")),
        param_set: owned(get("parameterSetIdentifier")),
        curve: curve.map(|c| c.rsplit('/').next().unwrap_or(c).to_string()),
        mode: owned(get("mode")),
        padding: owned(get("padding")),
        functions: array(get("cryptoFunctions")).filter_map(Value::as_str).map(str::to_string).collect(),
        classical_security_level: get("classicalSecurityLevel").and_then(Value::as_u64),
        nist_quantum_security_level: get("nistQuantumSecurityLevel").and_then(Value::as_u64),
    }
}

fn certificate_info(p: Option<&Obj>) -> CertificateInfo {
    let get = |name| owned(p.and_then(|p| p.get(name)));
    CertificateInfo {
        subject: get("subjectName"),
        issuer: get("issuerName"),
        not_valid_before: get("notValidBefore"),
        not_valid_after: get("notValidAfter"),
        format: get("certificateFormat"),
    }
}

fn material_info(p: Option<&Obj>) -> MaterialInfo {
    let get = |name| p.and_then(|p| p.get(name));
    MaterialInfo { kind: owned(get("type")), size: get("size").and_then(Value::as_u64), state: owned(get("state")) }
}

fn protocol_info(p: Option<&Obj>) -> ProtocolInfo {
    let get = |name| p.and_then(|p| p.get(name));
    ProtocolInfo {
        kind: owned(get("type")),
        version: owned(get("version")),
        cipher_suites: array(get("cipherSuites")).map(|s| owned(obj(Some(s)).and_then(|s| s.get("name")))).collect(),
    }
}

fn occurrences(c: &Obj) -> Vec<Occurrence> {
    let evidence = obj(c.get("evidence"));
    list(evidence.and_then(|e| e.get("occurrences")))
        .filter_map(|o| {
            Some(Occurrence {
                location: owned(o.get("location"))?,
                line: o.get("line").and_then(Value::as_u64),
                offset: o.get("offset").and_then(Value::as_u64),
                context: owned(o.get("additionalContext")),
            })
        })
        .collect()
}

/// The name of a made-up root: the repository in CBOMkit's gitUrl, else the file name.
fn root_name(properties: &BTreeMap<String, String>, file_name: Option<&str>) -> String {
    if let Some(git) = properties.get("gitUrl") {
        let git = git.strip_suffix(".git").unwrap_or(git).trim_end_matches('/');
        if !git.is_empty() {
            return git.rsplit('/').find(|s| !s.is_empty()).unwrap_or(git).to_string();
        }
    }
    let Some(name) = file_name else { return "CBOM".into() };
    let lower = name.to_ascii_lowercase();
    let cut = [".cdx.json", ".cdx.xml", ".json", ".xml"].iter().find(|e| lower.ends_with(*e)).map_or(0, |e| e.len());
    name[..name.len() - cut].to_string()
}

fn obj(v: Option<&Value>) -> Option<&Obj> {
    v.and_then(Value::as_object)
}

fn array(v: Option<&Value>) -> impl Iterator<Item = &Value> {
    v.and_then(Value::as_array).into_iter().flatten()
}

/// The objects in a list; anything else in it is skipped.
fn list(v: Option<&Value>) -> impl Iterator<Item = &Obj> {
    array(v).filter_map(Value::as_object)
}

/// A string that is not empty.
fn text(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn owned(v: Option<&Value>) -> Option<String> {
    text(v).map(str::to_string)
}

/// A value as a message shows it: a string without quotes, anything else as JSON.
fn display(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => "undefined".into(),
    }
}
