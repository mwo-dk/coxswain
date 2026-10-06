//! Statements, DSSE envelopes, Sigstore bundles and JSON Lines of them, to the model. Lenient:
//! a broken line or field becomes an [`Issue`] and the rest still loads.

use super::model::*;
use super::Error;
use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use serde_json::Value;
use std::collections::BTreeMap;

pub const IN_TOTO: &str = "application/vnd.in-toto+json";
pub const BUNDLE: &str = "application/vnd.dev.sigstore.bundle";
pub const STATEMENT: &str = "https://in-toto.io/Statement/";
pub const SLSA_V1: &str = "https://slsa.dev/provenance/v1";
pub const SLSA_V0_2: &str = "https://slsa.dev/provenance/v0.2";
pub const VSA: &str = "https://slsa.dev/verification_summary/";
pub const CYCLONEDX: &str = "https://cyclonedx.org/bom";

/// The whole text: one JSON document, else JSON Lines.
pub fn from_text(text: &str) -> Result<Attestations, Error> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Attestations::default();
    match serde_json::from_str::<Value>(text) {
        Ok(v) => {
            if let Some(e) = entry(&v, None, &mut out.issues) {
                out.entries.push(e);
            }
        }
        Err(whole) => {
            let mut first_error = None;
            for (i, l) in text.lines().enumerate() {
                let l = l.trim();
                if l.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(l) {
                    Ok(v) => {
                        if let Some(e) = entry(&v, Some(i + 1), &mut out.issues) {
                            out.entries.push(e);
                        }
                    }
                    Err(e) => {
                        first_error.get_or_insert((i + 1, e.to_string()));
                        issue(&mut out.issues, IssueCode::InvalidJson, format!("Not JSON: {e}"), Some(i + 1));
                    }
                }
            }
            if out.entries.is_empty() {
                return Err(match first_error {
                    // Not one line was JSON: report the file as one document.
                    Some(_) if out.issues.iter().all(|i| i.code == IssueCode::InvalidJson) => {
                        Error::InvalidJson { line: whole.line(), message: whole.to_string() }
                    }
                    _ => Error::NotAttestation,
                });
            }
        }
    }
    if out.entries.is_empty() {
        return Err(Error::NotAttestation);
    }
    Ok(out)
}

fn issue(issues: &mut Vec<Issue>, code: IssueCode, message: String, line: Option<usize>) {
    issues.push(Issue { code, message, line });
}

/// One statement, envelope or bundle.
pub fn entry(v: &Value, line: Option<usize>, issues: &mut Vec<Issue>) -> Option<Entry> {
    if let Some(mt) = str_at(v, "mediaType").filter(|m| m.starts_with(BUNDLE)) {
        let Some(env) = v.get("dsseEnvelope") else {
            let m = "A Sigstore bundle with a plain signature, not an attestation".to_string();
            issue(issues, IssueCode::NotAttestation, m, line);
            return None;
        };
        let mut e = envelope(env, line, issues)?;
        e.wrapping = Wrapping::Bundle { media_type: mt.to_string() };
        let vm = v.get("verificationMaterial");
        let cert = vm
            .and_then(|m| m.pointer("/certificate/rawBytes").or_else(|| m.pointer("/x509CertificateChain/certificates/0/rawBytes")))
            .and_then(Value::as_str);
        if let Some(cert) = cert {
            match decode(cert).ok_or_else(|| "not base64".to_string()).and_then(|der| super::signer::read(&der)) {
                Ok(s) => e.signer = Some(s),
                Err(m) => issue(issues, IssueCode::BadCertificate, format!("The certificate could not be read: {m}"), line),
            }
        }
        e.public_key = vm.and_then(|m| m.pointer("/publicKey/hint")).and_then(Value::as_str).map(str::to_string);
        e.log = vm.and_then(|m| m.get("tlogEntries")).and_then(Value::as_array).map(|a| a.iter().map(log_entry).collect()).unwrap_or_default();
        return Some(e);
    }
    if v.get("payloadType").is_some() {
        return envelope(v, line, issues);
    }
    if v.get("_type").is_some() {
        let statement = statement(v, line, issues);
        return Some(Entry { line, wrapping: Wrapping::Statement, signatures: vec![], signer: None, public_key: None, log: vec![], statement });
    }
    let m = "Not an in-toto statement, a DSSE envelope or a Sigstore bundle".to_string();
    issue(issues, IssueCode::NotAttestation, m, line);
    None
}

fn envelope(v: &Value, line: Option<usize>, issues: &mut Vec<Issue>) -> Option<Entry> {
    let pt = str_at(v, "payloadType").unwrap_or("");
    if pt != IN_TOTO {
        issue(issues, IssueCode::NotInToto, format!("The payload is {pt:?}, not an in-toto statement"), line);
        return None;
    }
    let Some(bytes) = str_at(v, "payload").and_then(decode) else {
        issue(issues, IssueCode::BadBase64, "The payload is not valid base64".into(), line);
        return None;
    };
    let st: Value = match serde_json::from_slice(&bytes) {
        Ok(st) => st,
        Err(e) => {
            issue(issues, IssueCode::InvalidJson, format!("The payload is not JSON: {e}"), line);
            return None;
        }
    };
    let signatures = v
        .get("signatures")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|s| Signature {
                    keyid: str_at(s, "keyid").filter(|k| !k.is_empty()).map(str::to_string),
                    length: str_at(s, "sig").and_then(decode).map_or(0, |b| b.len()),
                })
                .collect()
        })
        .unwrap_or_default();
    let statement = statement(&st, line, issues);
    Some(Entry { line, wrapping: Wrapping::Envelope, signatures, signer: None, public_key: None, log: vec![], statement })
}

/// DSSE allows standard and URL-safe base64, with or without padding.
pub fn decode(s: &str) -> Option<Vec<u8>> {
    let cfg = GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent);
    let s = s.trim();
    GeneralPurpose::new(&alphabet::STANDARD, cfg)
        .decode(s)
        .or_else(|_| GeneralPurpose::new(&alphabet::URL_SAFE, cfg).decode(s))
        .ok()
}

fn statement(v: &Value, line: Option<usize>, issues: &mut Vec<Issue>) -> Statement {
    let t = str_at(v, "_type").unwrap_or("");
    if !t.starts_with(STATEMENT) {
        issue(issues, IssueCode::UnknownStatementType, format!("Unknown statement type {t:?}"), line);
    }
    let subjects: Vec<Resource> = array(v, "subject").iter().map(resource).collect();
    for s in subjects.iter().filter(|s| s.digest.is_empty()) {
        issue(issues, IssueCode::SubjectWithoutDigest, format!("{} has no digest", s.label()), line);
    }
    let predicate_type = str_at(v, "predicateType").unwrap_or("").to_string();
    let p = v.get("predicate").cloned().unwrap_or(Value::Null);
    let predicate = if predicate_type.starts_with(SLSA_V1) {
        Predicate::Provenance(Box::new(v1(&p, line, issues)))
    } else if predicate_type.starts_with(SLSA_V0_2) {
        Predicate::Provenance(Box::new(v0_2(&p, line, issues)))
    } else if predicate_type.starts_with(VSA) {
        Predicate::Vsa(vsa(&p))
    } else if predicate_type.starts_with(CYCLONEDX) {
        Predicate::Bom { bom: p }
    } else {
        Predicate::Other { predicate: p }
    };
    Statement { subjects, predicate_type, predicate, raw: v.clone() }
}

fn v1(p: &Value, line: Option<usize>, issues: &mut Vec<Issue>) -> Provenance {
    let bd = p.get("buildDefinition").unwrap_or(&Value::Null);
    let rd = p.get("runDetails").unwrap_or(&Value::Null);
    let build_type = str_at(bd, "buildType").unwrap_or("").to_string();
    let id = rd.pointer("/builder/id").and_then(Value::as_str).unwrap_or("").to_string();
    if build_type.is_empty() || id.is_empty() {
        issue(issues, IssueCode::BadPredicate, "SLSA v1 provenance without a buildType or builder.id".into(), line);
    }
    let builder = Builder {
        id,
        version: rd.pointer("/builder/version").map(strings).unwrap_or_default(),
        dependencies: rd.pointer("/builder/builderDependencies").and_then(Value::as_array).map(|a| a.iter().map(resource).collect()).unwrap_or_default(),
    };
    let meta = rd.get("metadata").unwrap_or(&Value::Null);
    Provenance {
        version: SlsaVersion::V1,
        build_type,
        builder,
        external: bd.get("externalParameters").cloned().unwrap_or(Value::Null),
        internal: bd.get("internalParameters").cloned().unwrap_or(Value::Null),
        dependencies: array(bd, "resolvedDependencies").iter().map(resource).collect(),
        invocation: str_at(meta, "invocationId").map(str::to_string),
        started: str_at(meta, "startedOn").map(str::to_string),
        finished: str_at(meta, "finishedOn").map(str::to_string),
        byproducts: array(rd, "byproducts").iter().map(resource).collect(),
        completeness: None,
        reproducible: None,
    }
}

/// v0.2, in v1's names: configSource is the first dependency, parameters are external, the
/// environment and buildConfig are internal.
fn v0_2(p: &Value, line: Option<usize>, issues: &mut Vec<Issue>) -> Provenance {
    let build_type = str_at(p, "buildType").unwrap_or("").to_string();
    let id = p.pointer("/builder/id").and_then(Value::as_str).unwrap_or("").to_string();
    if build_type.is_empty() || id.is_empty() {
        issue(issues, IssueCode::BadPredicate, "SLSA v0.2 provenance without a buildType or builder.id".into(), line);
    }
    let inv = p.get("invocation").unwrap_or(&Value::Null);
    let mut dependencies = Vec::new();
    if let Some(cs) = inv.get("configSource").filter(|c| c.is_object()) {
        let mut r = resource(cs);
        r.config_source = true;
        r.entry_point = str_at(cs, "entryPoint").map(str::to_string);
        dependencies.push(r);
    }
    dependencies.extend(array(p, "materials").iter().map(resource));
    let mut internal = serde_json::Map::new();
    for (from, to) in [(inv.get("environment"), "environment"), (p.get("buildConfig"), "buildConfig")] {
        if let Some(v) = from.filter(|v| !v.is_null()) {
            internal.insert(to.to_string(), v.clone());
        }
    }
    let meta = p.get("metadata").unwrap_or(&Value::Null);
    let flag = |k: &str| meta.pointer(&format!("/completeness/{k}")).and_then(Value::as_bool).unwrap_or(false);
    Provenance {
        version: SlsaVersion::V0_2,
        build_type,
        builder: Builder { id, ..Builder::default() },
        external: inv.get("parameters").cloned().unwrap_or(Value::Null),
        internal: if internal.is_empty() { Value::Null } else { Value::Object(internal) },
        dependencies,
        // The spec says `buildInvocationId`; slsa-github-generator writes `buildInvocationID`.
        invocation: str_at(meta, "buildInvocationId").or_else(|| str_at(meta, "buildInvocationID")).map(str::to_string),
        started: str_at(meta, "buildStartedOn").map(str::to_string),
        finished: str_at(meta, "buildFinishedOn").map(str::to_string),
        byproducts: vec![],
        completeness: meta.get("completeness").map(|_| Completeness {
            parameters: flag("parameters"),
            environment: flag("environment"),
            materials: flag("materials"),
        }),
        reproducible: meta.get("reproducible").and_then(Value::as_bool),
    }
}

fn vsa(p: &Value) -> Vsa {
    Vsa {
        verifier: p.pointer("/verifier/id").and_then(Value::as_str).unwrap_or("").to_string(),
        time: str_at(p, "timeVerified").map(str::to_string),
        resource: str_at(p, "resourceUri").map(str::to_string),
        policy: p.pointer("/policy/uri").and_then(Value::as_str).map(str::to_string),
        result: str_at(p, "verificationResult").map(str::to_string),
        levels: array(p, "verifiedLevels").iter().filter_map(Value::as_str).map(str::to_string).collect(),
    }
}

fn resource(v: &Value) -> Resource {
    Resource {
        name: str_at(v, "name").map(str::to_string),
        uri: str_at(v, "uri").map(str::to_string),
        digest: v.get("digest").map(strings).unwrap_or_default(),
        download: str_at(v, "downloadLocation").map(str::to_string),
        media_type: str_at(v, "mediaType").map(str::to_string),
        config_source: false,
        entry_point: None,
    }
}

fn log_entry(v: &Value) -> LogEntry {
    // Bundles write these 64-bit numbers as strings; older ones as numbers.
    let num = |k: &str| v.get(k).and_then(|n| n.as_i64().or_else(|| n.as_str().and_then(|s| s.parse().ok())));
    LogEntry {
        index: num("logIndex").and_then(|n| u64::try_from(n).ok()),
        integrated_time: num("integratedTime"),
        kind: v.pointer("/kindVersion/kind").and_then(Value::as_str).map(str::to_string),
        has_proof: v.get("inclusionProof").is_some_and(Value::is_object),
        has_promise: v.get("inclusionPromise").is_some_and(Value::is_object),
    }
}

/// An object's values as text: strings as they are, anything else as JSON.
fn strings(v: &Value) -> BTreeMap<String, String> {
    v.as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().map_or_else(|| v.to_string(), str::to_string))).collect())
        .unwrap_or_default()
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}
