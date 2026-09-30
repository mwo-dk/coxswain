//! The BOM as the views see it. Every input format is turned into this, so the tree, the
//! ratings and the diff never see raw CycloneDX.

use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Application,
    /// Made by coxswain: a directory, a source file or a kind of asset.
    Group,
    Component,
    Algorithm,
    Certificate,
    Protocol,
    /// Keys, secrets and other related crypto material.
    Material,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EdgeKind {
    /// Component to component, and crypto to crypto (CBOMkit: key to algorithm).
    DependsOn,
    /// Component to the crypto asset it provides (CycloneDX 1.6).
    Provides,
    /// Nested components.
    Contains,
    /// Certificate to its signature algorithm.
    SignedWith,
    /// Certificate to its subject public key.
    HasKey,
    /// Material or protocol to an algorithm.
    UsesAlgorithm,
    /// Other crypto references (cryptoRefArray, securedBy).
    Uses,
    /// Source file group to an asset, for occurrences after the first (made by the tree).
    OccursIn,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Occurrence {
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// For example the call that uses the asset: "java.security.KeyFactory#getInstance(…)".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlgorithmInfo {
    pub oid: Option<String>,
    /// CycloneDX 1.7 `algorithmFamily`.
    pub family: Option<String>,
    pub primitive: Option<String>,
    pub param_set: Option<String>,
    /// 1.6 `curve` or 1.7 `ellipticCurve`, without its namespace ("secg/secp521r1" is "secp521r1").
    pub curve: Option<String>,
    pub mode: Option<String>,
    pub padding: Option<String>,
    pub functions: Vec<String>,
    pub classical_security_level: Option<u64>,
    pub nist_quantum_security_level: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateInfo {
    pub subject: Option<String>,
    pub issuer: Option<String>,
    pub not_valid_before: Option<String>,
    pub not_valid_after: Option<String>,
    pub format: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MaterialInfo {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub size: Option<u64>,
    pub state: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolInfo {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub version: Option<String>,
    /// The names of its cipher suites; a suite without a name is `None`.
    pub cipher_suites: Vec<Option<String>>,
}

/// What a node knows about its cryptography, by kind.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Crypto {
    Algorithm(AlgorithmInfo),
    Certificate(CertificateInfo),
    Material(MaterialInfo),
    Protocol(ProtocolInfo),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupKind {
    Directory,
    File,
    Kind,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GroupInfo {
    #[serde(rename = "type")]
    pub kind: GroupKind,
    /// The whole source path, for directories and files.
    pub path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    /// Unique within the BOM: the bom-ref, or a made-up "#…" key.
    pub key: String,
    pub kind: NodeKind,
    pub label: String,
    pub bom_ref: Option<String>,
    pub version: Option<String>,
    pub purl: Option<String>,
    /// The CycloneDX component type (library, application, file, …) of a component.
    pub component_type: Option<String>,
    /// Made by coxswain, not found in the file: a root the file lacks, or a group.
    pub synthetic: bool,
    pub group: Option<GroupInfo>,
    pub occurrences: Vec<Occurrence>,
    pub crypto: Option<Crypto>,
    /// The node as the file has it, for the details box.
    #[serde(skip)]
    pub raw: serde_json::Value,
}

impl Node {
    pub(crate) fn synthetic(key: String, kind: NodeKind, label: String) -> Node {
        Node {
            key,
            kind,
            label,
            bom_ref: None,
            version: None,
            purl: None,
            component_type: None,
            synthetic: true,
            group: None,
            occurrences: vec![],
            crypto: None,
            raw: serde_json::Value::Null,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Edge {
    pub from: u32,
    pub to: u32,
    pub kind: EdgeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueCode {
    DanglingRef,
    DuplicateBomRef,
    MissingBomRef,
    OidMismatch,
    UnresolvedAlgorithm,
    AmbiguousAlgorithm,
    UnsupportedSpecVersion,
}

/// Something wrong with the file that did not stop it from loading.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Issue {
    pub severity: Severity,
    pub code: IssueCode,
    pub message: String,
    /// The node it is about, when there is one.
    pub node: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Json,
    Xml,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub format: Format,
    pub spec_version: String,
    pub serial_number: Option<String>,
    pub timestamp: Option<String>,
    pub file_name: Option<String>,
    /// `metadata.properties`, such as CBOMkit's gitUrl and commit.
    pub properties: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Bom {
    pub source: Source,
    /// `nodes[0]` is always the root, an application.
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub issues: Vec<Issue>,
}
