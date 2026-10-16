//! Bills of materials: CycloneDX CBOMs today, SBOMs later. Read-only, and nothing leaves the
//! machine. Design: `design/bom-viewer.md`. Ported from cipherscape's DOM-free core.
//!
//! - `model`: the BOM as the views see it, whatever the file format.
//! - `ingest` and `xml`: CycloneDX JSON and XML to the model.
//! - `tree`: the tree the views show, by dependencies, source files or kind.
//! - `policy`, `status` and `assess`: how good each asset is, and why.
//! - `diff`: two versions of a BOM, compared.
//! - `view`: a BOM read and rated in one go, with names, filters and sunburst geometry for views.

pub mod assess;
pub mod diff;
pub mod ingest;
pub mod model;
pub mod policy;
pub mod status;
pub mod tree;
pub mod view;
mod xml;

use std::fmt;
use std::io::Read;
use std::path::Path;

pub use model::*;
pub use status::Status;
pub use tree::{Tree, TreeMode};

/// Larger files are shown as ordinary text or JSON instead.
pub const MAX_SIZE: u64 = 64 * 1024 * 1024;

/// Why a file could not be read as a BOM. Anything short of these loads, with an [`Issue`].
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    NotCycloneDx,
    InvalidJson {
        line: usize,
        message: String,
    },
    InvalidXml {
        line: usize,
        message: String,
    },
    /// XML with a DOCTYPE: refused, which rules out entity expansion and external entities.
    UnsafeXml,
    TooLarge(u64),
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&match self {
            Error::NotCycloneDx => crate::t!("err.bom.not_cyclonedx"),
            Error::InvalidJson { line, message } => {
                crate::t!("err.bom.invalid_json", "line" => line, "message" => message)
            }
            Error::InvalidXml { line, message } => {
                crate::t!("err.bom.invalid_xml", "line" => line, "message" => message)
            }
            Error::UnsafeXml => crate::t!("err.bom.unsafe_xml"),
            Error::TooLarge(_) => crate::t!("err.bom.too_large"),
            Error::Io(e) => e.clone(),
        })
    }
}

impl std::error::Error for Error {}

/// How much of a file [`sniff_head`] needs.
pub const SNIFF_BYTES: usize = 8192;

/// Whether a file is a CycloneDX BOM: by its name, else by its first bytes for JSON and XML.
pub fn sniff(path: &Path) -> bool {
    sniff_file(path, &[".json", ".xml"], sniff_name, sniff_head)
}

/// Whether the file at `path` is one `name` says it is by its (lowercase) name, else, with one
/// of the endings `exts` and at most `MAX_SIZE`, one `head` says it is by its first bytes.
pub(crate) fn sniff_file(
    path: &Path,
    exts: &[&str],
    name: fn(&str) -> bool,
    head: fn(&[u8]) -> bool,
) -> bool {
    let lower = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name(&lower) {
        return true;
    }
    if !exts.iter().any(|e| lower.ends_with(e)) {
        return false;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    if file.metadata().is_ok_and(|m| m.len() > MAX_SIZE) {
        return false;
    }
    let mut first = Vec::with_capacity(SNIFF_BYTES);
    file.take(SNIFF_BYTES as u64)
        .read_to_end(&mut first)
        .is_ok()
        && head(&first)
}

/// The text of the file at `path`, or `Err` with its size when it is over `MAX_SIZE`.
pub(crate) fn read_capped(path: &Path) -> std::io::Result<Result<String, u64>> {
    let file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size > MAX_SIZE {
        return Ok(Err(size));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(MAX_SIZE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SIZE {
        return Ok(Err(bytes.len() as u64));
    }
    Ok(Ok(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Names that say BOM: `*.cdx.json`, `*.cdx.xml`, `*.cbom.json`, `bom.json` and `bom.xml`.
pub fn sniff_name(name: &str) -> bool {
    let name = name.to_lowercase();
    [".cdx.json", ".cdx.xml", ".cbom.json"]
        .iter()
        .any(|e| name.ends_with(e))
        || name == "bom.json"
        || name == "bom.xml"
}

/// Whether the first bytes of a JSON or XML file are CycloneDX's.
pub fn sniff_head(head: &[u8]) -> bool {
    let head = &head[..head.len().min(SNIFF_BYTES)];
    let has = |needle: &[u8]| head.windows(needle.len()).any(|w| w == needle);
    (has(b"\"bomFormat\"") && has(b"\"CycloneDX\"")) || has(b"http://cyclonedx.org/schema/bom/")
}

/// Reads a CycloneDX file, JSON or XML by its first character.
pub fn load(path: &Path) -> Result<Bom, Error> {
    let text = read_capped(path)
        .map_err(|e| Error::Io(e.to_string()))?
        .map_err(Error::TooLarge)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    parse(&text, name.as_deref())
}

/// Reads CycloneDX from text, JSON or XML by its first character.
pub fn parse(text: &str, file_name: Option<&str>) -> Result<Bom, Error> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.trim_start().starts_with('<') {
        parse_xml(text, file_name)
    } else {
        parse_json(text, file_name)
    }
}

pub fn parse_json(text: &str, file_name: Option<&str>) -> Result<Bom, Error> {
    let doc: serde_json::Value = serde_json::from_str(text).map_err(|e| Error::InvalidJson {
        line: e.line(),
        message: e.to_string(),
    })?;
    ingest::from_value(&doc, Format::Json, file_name)
}

pub fn parse_xml(text: &str, file_name: Option<&str>) -> Result<Bom, Error> {
    ingest::from_value(&xml::to_json(text)?, Format::Xml, file_name)
}

#[cfg(test)]
mod tests;
