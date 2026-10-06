//! Build provenance and other in-toto attestations: SLSA provenance v0.2 and v1, bare or in DSSE
//! envelopes or Sigstore bundles, one or many per file. Read-only, nothing is verified, and
//! nothing leaves the machine. Design: `docs/design/provenance-viewer.md`.
//!
//! - `model`: what the views see, whatever the wrapping and the SLSA version.
//! - `ingest`: the file formats to the model.

pub mod ingest;
pub mod model;

use std::fmt;
use std::io::Read;
use std::path::Path;

pub use model::*;

/// Larger files are shown as ordinary text or JSON instead.
pub const MAX_SIZE: u64 = 64 * 1024 * 1024;

/// How much of a file [`sniff_head`] needs.
pub const SNIFF_BYTES: usize = 8192;

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
        match self {
            Error::NotAttestation => write!(f, "This file holds no in-toto attestation."),
            Error::InvalidJson { line, message } => write!(f, "This file isn't valid JSON (line {line}): {message}"),
            Error::TooLarge(size) => write!(f, "This file is too large to show as provenance ({size} bytes)."),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Whether a file holds attestations: by its name, else by its first bytes. Callers ask this
/// before `bom::sniff`, since a statement may carry a CycloneDX predicate.
pub fn sniff(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if sniff_name(&name) {
        return true;
    }
    if ![".json", ".jsonl", ".ndjson"].iter().any(|e| name.ends_with(e)) {
        return false;
    }
    let Ok(file) = std::fs::File::open(path) else { return false };
    if file.metadata().is_ok_and(|m| m.len() > MAX_SIZE) {
        return false;
    }
    let mut head = Vec::with_capacity(SNIFF_BYTES);
    file.take(SNIFF_BYTES as u64).read_to_end(&mut head).is_ok() && sniff_head(&head)
}

/// Names that say attestation: `*.intoto.jsonl`, `*.intoto.json`, `*.sigstore.json`,
/// `*.sigstore`, `*.dsse.json`, `*.provenance.json`, `*.build.slsa` and `provenance.json`.
pub fn sniff_name(name: &str) -> bool {
    let name = name.to_lowercase();
    [".intoto.jsonl", ".intoto.json", ".sigstore.json", ".sigstore", ".dsse.json", ".provenance.json", ".build.slsa"]
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
    let io = |e: std::io::Error| Error::Io(e.to_string());
    let file = std::fs::File::open(path).map_err(io)?;
    let size = file.metadata().map_err(io)?.len();
    if size > MAX_SIZE {
        return Err(Error::TooLarge(size));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(MAX_SIZE + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 > MAX_SIZE {
        return Err(Error::TooLarge(bytes.len() as u64));
    }
    parse(&String::from_utf8_lossy(&bytes))
}

pub fn parse(text: &str) -> Result<Attestations, Error> {
    ingest::from_text(text)
}

#[cfg(test)]
mod tests;
