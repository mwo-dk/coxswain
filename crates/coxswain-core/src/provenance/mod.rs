//! Build provenance and other in-toto attestations: SLSA provenance v0.2 and v1, bare or in DSSE
//! envelopes or Sigstore bundles, one or many per file. Read-only, nothing is verified, and
//! nothing leaves the machine. Design: `design/provenance-viewer.md`.
//!
//! - `model`: what the views see, whatever the wrapping and the SLSA version.
//! - `ingest`: the file formats to the model.
//! - `signer`: who signed, from a Sigstore certificate.
//! - `check`: the subjects against the files here, the source commit against a checkout here.
//! - `diff`: two provenance files compared.
//! - `view`: the build's facts and builder names, for both apps.

pub mod check;
pub mod diff;
pub mod ingest;
pub mod model;
pub mod signer;
pub mod view;

use std::fmt;
use std::path::Path;

pub use model::*;

/// The sizes a BOM is read within hold for attestations too.
pub use crate::bom::{MAX_SIZE, SNIFF_BYTES};

/// Why a file could not be read as attestations. Anything short of these loads, with an [`Issue`].
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    NotAttestation,
    InvalidJson { line: usize, message: String },
    TooLarge(u64),
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&match self {
            Error::NotAttestation => crate::t!("err.provenance.not_attestation"),
            Error::InvalidJson { line, message } => {
                crate::t!("err.bom.invalid_json", "line" => line, "message" => message)
            }
            Error::TooLarge(_) => crate::t!("err.provenance.too_large"),
            Error::Io(e) => e.clone(),
        })
    }
}

impl std::error::Error for Error {}

/// Whether a file holds attestations: by its name, else by its first bytes. Callers ask this
/// before `bom::sniff`, since a statement may carry a CycloneDX predicate.
pub fn sniff(path: &Path) -> bool {
    crate::bom::sniff_file(
        path,
        &[".json", ".jsonl", ".ndjson"],
        sniff_name,
        sniff_head,
    )
}

/// Names that say attestation: `*.intoto.jsonl`, `*.intoto.json`, `*.sigstore.json`,
/// `*.sigstore`, `*.dsse.json`, `*.provenance.json`, `*.build.slsa` and `provenance.json`.
pub fn sniff_name(name: &str) -> bool {
    let name = name.to_lowercase();
    [
        ".intoto.jsonl",
        ".intoto.json",
        ".sigstore.json",
        ".sigstore",
        ".dsse.json",
        ".provenance.json",
        ".build.slsa",
    ]
    .iter()
    .any(|e| name.ends_with(e))
        || name == "provenance.json"
}

/// Whether the first bytes of a JSON file are an envelope's, a statement's or a bundle's.
pub fn sniff_head(head: &[u8]) -> bool {
    let head = &head[..head.len().min(SNIFF_BYTES)];
    let has = |needle: &[u8]| head.windows(needle.len()).any(|w| w == needle);
    (has(b"\"payloadType\"") && has(ingest::IN_TOTO.as_bytes()))
        || (has(b"\"_type\"") && has(ingest::STATEMENT.as_bytes()))
        || has(ingest::BUNDLE.as_bytes())
}

pub fn load(path: &Path) -> Result<Attestations, Error> {
    let text = crate::bom::read_capped(path)
        .map_err(|e| Error::Io(e.to_string()))?
        .map_err(Error::TooLarge)?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Attestations, Error> {
    ingest::from_text(text)
}

#[cfg(test)]
mod tests;
