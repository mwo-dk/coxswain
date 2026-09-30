//! Bills of materials: CycloneDX CBOMs today, SBOMs later. Read-only, and nothing leaves the
//! machine. Design: `docs/design/bom-viewer.md`. Ported from cipherscape's DOM-free core.
//!
//! - `model`: the BOM as the views see it, whatever the file format.
//! - `ingest` and `xml`: CycloneDX JSON and XML to the model.
//! - `tree`: the tree the views show, by dependencies, source files or kind.

pub mod ingest;
pub mod model;
pub mod tree;
mod xml;

use std::fmt;
use std::io::Read;
use std::path::Path;

pub use model::*;
pub use tree::{Tree, TreeMode};

/// Larger files are shown as ordinary text or JSON instead.
pub const MAX_SIZE: u64 = 64 * 1024 * 1024;

/// Why a file could not be read as a BOM. Anything short of these loads, with an [`Issue`].
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    NotCycloneDx,
    InvalidJson { line: usize, message: String },
    InvalidXml { line: usize, message: String },
    /// XML with a DOCTYPE: refused, which rules out entity expansion and external entities.
    UnsafeXml,
    TooLarge(u64),
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::NotCycloneDx => write!(f, "This file is not a CycloneDX BOM."),
            Error::InvalidJson { line, message } => write!(f, "This file isn't valid JSON (line {line}): {message}"),
            Error::InvalidXml { line, message } => write!(f, "This file isn't valid XML (line {line}): {message}"),
            Error::UnsafeXml => write!(f, "This XML contains a DOCTYPE declaration, which is refused for safety."),
            Error::TooLarge(size) => write!(f, "This file is too large to show as a BOM ({size} bytes)."),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Reads a CycloneDX file, JSON or XML by its first character.
pub fn load(path: &Path) -> Result<Bom, Error> {
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
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    parse(&String::from_utf8_lossy(&bytes), name.as_deref())
}

/// Reads CycloneDX from text, JSON or XML by its first character.
pub fn parse(text: &str, file_name: Option<&str>) -> Result<Bom, Error> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.trim_start().starts_with('<') { parse_xml(text, file_name) } else { parse_json(text, file_name) }
}

pub fn parse_json(text: &str, file_name: Option<&str>) -> Result<Bom, Error> {
    let doc: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| Error::InvalidJson { line: e.line(), message: e.to_string() })?;
    ingest::from_value(&doc, Format::Json, file_name)
}

pub fn parse_xml(text: &str, file_name: Option<&str>) -> Result<Bom, Error> {
    ingest::from_value(&xml::to_json(text)?, Format::Xml, file_name)
}

#[cfg(test)]
mod tests;
