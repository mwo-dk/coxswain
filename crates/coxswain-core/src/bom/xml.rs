//! CycloneDX XML to the CycloneDX JSON shape, so both formats share one reader (`ingest.rs`).
//! Checked against the specification's own paired XML and JSON test documents.
//!
//! Two steps: the elements become a plain JSON tree (attributes and children as keys,
//! repeated children as lists), and that tree is then reshaped where XML and JSON differ:
//! `<components><component/>…</components>` is a list, `<property name="k">v</property>`
//! is `{name, value}`, and so on.

use quick_xml::events::{BytesStart, Event};
use quick_xml::XmlVersion;
use serde_json::{Map, Number, Value};

use super::Error;

/// Elements whose children are a list of one element type.
const WRAPPERS: [(&str, &str); 11] = [
    ("components", "component"),
    ("services", "service"),
    ("cryptoFunctions", "cryptoFunction"),
    ("occurrences", "occurrence"),
    ("cipherSuites", "cipherSuite"),
    ("algorithms", "algorithm"),
    ("identifiers", "identifier"),
    ("properties", "property"),
    ("externalReferences", "reference"),
    ("hashes", "hash"),
    ("licenses", "license"),
];

/// Other elements that are always list items, by parent: a `<component>` is one under
/// `<components>` but a single object under `<metadata>`.
const LISTS: [(&str, &str); 7] = [
    ("component", "components"),
    ("tool", "tools"),
    ("dependency", "dependencies"),
    ("dependency", "dependency"),
    ("provides", "dependency"),
    ("certificationLevel", "algorithmProperties"),
    ("cryptoRef", "protocolProperties"),
];

/// Leaf values that are numbers in the JSON schema. Not `version`: that is a string except on `<bom>`.
const NUMERIC: [&str; 5] = ["line", "offset", "size", "classicalSecurityLevel", "nistQuantumSecurityLevel"];

/// Deeper than this is no BOM, and would only cost stack.
const MAX_DEPTH: usize = 128;

struct Element {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Element>,
    text: String,
}

pub fn to_json(xml: &str) -> Result<Value, Error> {
    // No DTDs: CycloneDX never needs one, and refusing them rules out entity expansion.
    if contains_doctype(xml) {
        return Err(Error::UnsafeXml);
    }
    let Some(root) = parse(xml)? else { return Ok(Value::Object(Map::new())) };
    if root.name != "bom" {
        return Ok(Value::Object(Map::new()));
    }
    let spec_version = root.attributes.iter().find_map(|(k, v)| {
        let ns = (k == "xmlns" || k.starts_with("xmlns:")).then_some(v.as_str())?;
        let version = ns.strip_prefix("http://cyclonedx.org/schema/bom/")?;
        version.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())).then(|| version.to_string())
    });
    let mut out = match convert(&plain(&root), "bom") {
        Value::Object(out) => out,
        _ => Map::new(),
    };
    if let Some(Value::String(v)) = out.get("version")
        && let Ok(n) = v.parse::<u64>()
    {
        out.insert("version".into(), n.into());
    }
    let mut doc = Map::new();
    doc.insert("bomFormat".into(), "CycloneDX".into());
    if let Some(v) = spec_version {
        doc.insert("specVersion".into(), v.into());
    }
    doc.extend(out);
    Ok(Value::Object(doc))
}

fn contains_doctype(xml: &str) -> bool {
    const NEEDLE: &[u8] = b"<!doctype";
    xml.as_bytes().windows(NEEDLE.len()).any(|w| w.eq_ignore_ascii_case(NEEDLE))
}

/// Reads the elements. Malformed XML is an error with its line; entities other than the five
/// XML ones and character references are kept as written, never looked up.
fn parse(xml: &str) -> Result<Option<Element>, Error> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<Element> = vec![];
    let mut root = None;
    let invalid = |at: u64, why: String| Error::InvalidXml { line: line_at(xml, at as usize), message: why };
    loop {
        let event = reader.read_event().map_err(|e| invalid(reader.error_position(), e.to_string()))?;
        match event {
            Event::Start(_) | Event::Empty(_) if stack.len() >= MAX_DEPTH || root.is_some() => {
                let why = if root.is_some() { "more than one root element" } else { "nested too deeply" };
                return Err(invalid(reader.buffer_position(), why.to_string()));
            }
            Event::Start(e) => stack.push(element(&e)),
            Event::Empty(e) => {
                let el = element(&e);
                match stack.last_mut() {
                    Some(parent) => parent.children.push(el),
                    None => root = Some(el),
                }
            }
            Event::End(_) => {
                let mut el = stack.pop().expect("the reader checks that ends match starts");
                el.text = el.text.trim().to_string();
                match stack.last_mut() {
                    Some(parent) => parent.children.push(el),
                    None => root = Some(el),
                }
            }
            Event::Text(t) => {
                if let Some(el) = stack.last_mut() {
                    el.text.push_str(&t.xml_content(XmlVersion::Implicit1_0));
                }
            }
            Event::CData(t) => {
                if let Some(el) = stack.last_mut() {
                    el.text.push_str(&t);
                }
            }
            Event::GeneralRef(r) => {
                if let Some(el) = stack.last_mut() {
                    match r.resolve_char_ref() {
                        Ok(Some(c)) => el.text.push(c),
                        _ => {
                            let name: &str = &r;
                            match quick_xml::escape::resolve_xml_entity(name) {
                                Some(s) => el.text.push_str(s),
                                None => el.text.push_str(&format!("&{name};")),
                            }
                        }
                    }
                }
            }
            Event::DocType(_) => return Err(Error::UnsafeXml),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(invalid(reader.buffer_position(), format!("<{}> is never closed", stack.last().unwrap().name)));
    }
    Ok(root)
}

fn element(e: &BytesStart) -> Element {
    let name = e.local_name().as_ref().to_string();
    let attributes = e
        .attributes()
        .flatten()
        .map(|a| {
            let key = a.key.as_ref().to_string();
            let value = a.normalized_value(XmlVersion::Implicit1_0).map(|v| v.into_owned());
            (key, value.unwrap_or_else(|_| a.value.to_string()))
        })
        .collect();
    Element { name, attributes, children: vec![], text: String::new() }
}

fn line_at(xml: &str, at: usize) -> usize {
    let at = at.min(xml.len());
    xml.as_bytes()[..at].iter().filter(|&&b| b == b'\n').count() + 1
}

/// Step one: an element as plain JSON. A leaf is its text; otherwise attributes (without
/// namespace declarations or prefixes) and children are keys, and text is `#text`.
fn plain(el: &Element) -> Value {
    if el.attributes.is_empty() && el.children.is_empty() {
        return Value::String(el.text.clone());
    }
    let mut out = Map::new();
    for (key, value) in &el.attributes {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let key = key.rsplit(':').next().unwrap_or(key);
        out.insert(key.to_string(), Value::String(value.clone()));
    }
    for child in &el.children {
        let value = plain(child);
        let always_list = is_list(&child.name, &el.name);
        match out.get_mut(&child.name) {
            Some(Value::Array(items)) => items.push(value),
            Some(one) => *one = Value::Array(vec![one.take(), value]),
            None if always_list => {
                out.insert(child.name.clone(), Value::Array(vec![value]));
            }
            None => {
                out.insert(child.name.clone(), value);
            }
        }
    }
    if !el.text.is_empty() {
        out.insert("#text".into(), Value::String(el.text.clone()));
    }
    Value::Object(out)
}

fn is_list(name: &str, parent: &str) -> bool {
    WRAPPERS.iter().any(|&(w, c)| c == name && w == parent) || LISTS.iter().any(|&(c, p)| c == name && p == parent)
}

/// Step two: reshape the plain tree where CycloneDX's XML and JSON differ.
fn convert(value: &Value, name: &str) -> Value {
    let obj = match value {
        Value::Array(items) => return Value::Array(items.iter().map(|v| convert(v, name)).collect()),
        Value::Object(obj) => obj,
        Value::String(s) if NUMERIC.contains(&name) => return number(s).map_or_else(|| value.clone(), Value::Number),
        _ => return value.clone(),
    };

    // <property name="k">v</property> is {name, value}
    if name == "property" {
        let mut out = Map::new();
        out.insert("name".into(), obj.get("name").cloned().unwrap_or(Value::Null));
        out.insert("value".into(), obj.get("#text").cloned().unwrap_or_else(|| "".into()));
        return Value::Object(out);
    }

    // <dependency ref="a"><dependency ref="b"/><provides ref="c"/></dependency> is {ref, dependsOn, provides}
    if name == "dependency" {
        let refs = |key| -> Vec<Value> {
            let items = obj.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            items.iter().map(|d| d.get("ref").cloned().unwrap_or(Value::Null)).collect()
        };
        let mut out = Map::new();
        out.insert("ref".into(), obj.get("ref").cloned().unwrap_or(Value::Null));
        for (key, field) in [("dependency", "dependsOn"), ("provides", "provides")] {
            let list = refs(key);
            if !list.is_empty() {
                out.insert(field.into(), Value::Array(list));
            }
        }
        return Value::Object(out);
    }

    let empty = Value::Array(vec![]);
    let mut out = Map::new();
    for (key, v) in obj {
        if key == "#text" {
            continue;
        }
        if let Some((_, child)) = WRAPPERS.iter().find(|(w, _)| w == key) {
            out.insert(key.clone(), convert(v.get(*child).unwrap_or(&empty), child));
        } else if key == "tools" && v.get("tool").is_some() {
            // the old <tools><tool/>…; the new form is a plain object
            out.insert(key.clone(), convert(&v["tool"], "tool"));
        } else if key == "dependencies" {
            out.insert(key.clone(), convert(v.get("dependency").unwrap_or(&empty), "dependency"));
        } else if key == "cryptoRef" {
            // <cryptoRef> repeats directly in protocolProperties
            out.insert("cryptoRefArray".into(), convert(v, key));
        } else {
            out.insert(key.clone(), convert(v, key));
        }
    }
    if out.is_empty()
        && let Some(text) = obj.get("#text")
    {
        return text.clone();
    }
    Value::Object(out)
}

fn number(s: &str) -> Option<Number> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "0"));
    let all_digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(whole) || !all_digits(fraction) {
        return None;
    }
    match s.parse::<i64>() {
        Ok(n) => Some(n.into()),
        Err(_) => s.parse::<f64>().ok().and_then(Number::from_f64),
    }
}
