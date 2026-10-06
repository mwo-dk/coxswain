//! Provenance as the views see it. Every wrapping (statement, envelope, bundle, JSON Lines) and
//! both SLSA versions are turned into this, so the views, the checks and the diff never see the
//! raw formats.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub use super::signer::{Claim, Signer};

/// What a file holds: one entry per statement, and what was wrong with it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Attestations {
    pub entries: Vec<Entry>,
    pub issues: Vec<Issue>,
}

/// How a statement was wrapped in the file.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "form", rename_all = "camelCase")]
pub enum Wrapping {
    Statement,
    Envelope,
    Bundle { media_type: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Its line in a JSON Lines file, from 1.
    pub line: Option<usize>,
    pub wrapping: Wrapping,
    /// The envelope's signatures, as they are. Nothing here says whether one is valid.
    pub signatures: Vec<Signature>,
    /// Who signed, from a bundle's certificate. Read, not verified.
    pub signer: Option<Signer>,
    /// A bundle signed with a key rather than a certificate: the key's hint.
    pub public_key: Option<String>,
    pub log: Vec<LogEntry>,
    pub statement: Statement,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Signature {
    pub keyid: Option<String>,
    /// The signature's length in bytes, decoded.
    pub length: usize,
}

/// A transparency log (Rekor) entry from a bundle.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub index: Option<u64>,
    /// Seconds since 1970, when the log took the entry.
    pub integrated_time: Option<i64>,
    /// `dsse`, `intoto`, `hashedrekord`.
    pub kind: Option<String>,
    pub has_proof: bool,
    pub has_promise: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Statement {
    pub subjects: Vec<Resource>,
    pub predicate_type: String,
    pub predicate: Predicate,
    /// The decoded statement as it is, for the Statement view (asked for on its own).
    #[serde(skip)]
    pub raw: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Predicate {
    Provenance(Box<Provenance>),
    Vsa(Vsa),
    /// A CycloneDX document, for the BOM viewer.
    Bom { bom: Value },
    Other { predicate: Value },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SlsaVersion {
    #[serde(rename = "0.2")]
    V0_2,
    #[serde(rename = "1")]
    V1,
}

/// SLSA provenance, v0.2 mapped to v1's names.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub version: SlsaVersion,
    pub build_type: String,
    pub builder: Builder,
    pub external: Value,
    pub internal: Value,
    pub dependencies: Vec<Resource>,
    pub invocation: Option<String>,
    pub started: Option<String>,
    pub finished: Option<String>,
    pub byproducts: Vec<Resource>,
    /// v0.2 only: which parts the builder vouches are complete.
    pub completeness: Option<Completeness>,
    /// v0.2 only.
    pub reproducible: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Builder {
    pub id: String,
    pub version: BTreeMap<String, String>,
    pub dependencies: Vec<Resource>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Completeness {
    pub parameters: bool,
    pub environment: bool,
    pub materials: bool,
}

/// A verification summary attestation: someone else's verdict on a resource.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Vsa {
    pub verifier: String,
    pub time: Option<String>,
    pub resource: Option<String>,
    pub policy: Option<String>,
    /// `PASSED` or `FAILED`, as the file says.
    pub result: Option<String>,
    pub levels: Vec<String>,
}

/// An in-toto ResourceDescriptor: a subject, a dependency or a byproduct.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub name: Option<String>,
    pub uri: Option<String>,
    /// Algorithm to hex digest: `sha256`, `sha512`, `gitCommit`, `sha1`, …
    pub digest: BTreeMap<String, String>,
    pub download: Option<String>,
    pub media_type: Option<String>,
    /// v0.2's `invocation.configSource`, the first dependency once mapped.
    pub config_source: bool,
    /// v0.2's `entryPoint` of the config source: the workflow file.
    pub entry_point: Option<String>,
}

impl Resource {
    /// What to call it: its name, else its URI.
    pub fn label(&self) -> &str {
        self.name.as_deref().or(self.uri.as_deref()).unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueCode {
    /// A line of a JSON Lines file, or the whole file, that is not JSON.
    InvalidJson,
    /// None of the forms: not a statement, envelope or bundle.
    NotAttestation,
    BadBase64,
    /// An envelope whose payload is not an in-toto statement.
    NotInToto,
    /// The predicate lacks a field it must have, or has one of the wrong type.
    BadPredicate,
    SubjectWithoutDigest,
    UnknownStatementType,
    /// A bundle's certificate that could not be read.
    BadCertificate,
}

/// Something wrong with the file that did not stop the rest from loading.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Issue {
    pub code: IssueCode,
    pub message: String,
    /// The JSON Lines line it is on, from 1.
    pub line: Option<usize>,
}
