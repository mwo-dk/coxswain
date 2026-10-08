//! Diagrams written as text, Mermaid, Graphviz and PlantUML, read as what they say: besides
//! their words, one sentence per arrow, "Browser to Entra ID: sign in", with the names the
//! nodes are shown by rather than their ids. Search by meaning then finds a diagram by what
//! flows where, not only by the words in its boxes.
//!
//! Pragmatic line patterns, not full grammars: a line they do not know adds nothing, and the
//! diagram's own text is always kept.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

/// Lines of sentences at most per diagram: a generated graph can have thousands of arrows.
const MOST: usize = 2000;

/// A sentence per arrow, then `text`. The sentences come first: search by meaning gives vectors
/// to the start of a file only, and what flows where is what a diagram is about.
fn with(text: String, sentences: Vec<String>) -> String {
    if sentences.is_empty() {
        return text;
    }
    let mut out = String::with_capacity(text.len() + sentences.len() * 24);
    for s in sentences.into_iter().take(MOST) {
        out.push_str(&s);
        out.push('\n');
    }
    out.push_str(&text);
    out
}

fn sentence(from: &str, to: &str, label: &str) -> String {
    let label = label.trim();
    if label.is_empty() { format!("{from} to {to}.") } else { format!("{from} to {to}: {label}.") }
}

// ---------------------------------------------------------------- Mermaid

/// A node with its shape: `A[Sign in]`, `B((Token))`, `C{Valid?}`, `D>Note]` …
static SHAPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\b([A-Za-z0-9_.]+)(?:\[\[|\[\(|\(\(\(|\(\(|\(\[|\[/|\[\\|\{\{|\[|\(|\{|>)"?([^\]\)\}"]*?)"?(?:\]\]|\)\]|\)\)\)|\)\)|\]\)|/\]|\\\]|\}\}|\]|\)|\})"#).unwrap());
/// A flowchart arrow, with its label written either way: `-->|label|` or `-- label -->`.
static FLOW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*(?:--\s+([^-|>][^->|]*?)\s+-->|-\.\s+([^.]+?)\s+\.->|==\s+([^=]+?)\s+==>|<?(?:-->|---|-\.->|-\.-|==>|===|--[ox]|~~~)[>]?)\s*(?:\|([^|]*)\|)?\s*").unwrap());
/// A sequence message: `Alice->>Bob: Hello`, `A-->>B: done`, `A-xB: no`.
static SEQ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*([^\s:>+-][^:>]*?)\s*(?:-->>|->>|-->|->|--x|-x|--\)|-\))\s*[+-]?\s*([^:]+?)\s*:\s*(.*)$").unwrap());
/// `participant A as Alice`, `actor U as User`.
static ALIAS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:participant|actor)\s+(\S+)\s+as\s+(.+?)\s*$").unwrap());

/// A Mermaid diagram's text with a sentence per arrow.
pub fn mermaid(text: String) -> String {
    let sentences = mermaid_sentences(&text);
    with(text, sentences)
}

fn mermaid_sentences(text: &str) -> Vec<String> {
    let mut names: HashMap<String, String> = HashMap::new();
    for c in SHAPE.captures_iter(text) {
        let label = c[2].trim();
        if !label.is_empty() {
            names.entry(c[1].to_string()).or_insert_with(|| label.replace("<br>", " ").replace("<br/>", " "));
        }
    }
    for line in text.lines() {
        if let Some(c) = ALIAS.captures(line) {
            names.insert(c[1].to_string(), c[2].trim().to_string());
        }
    }
    let name = |id: &str| names.get(id.trim()).cloned().unwrap_or_else(|| id.trim().to_string());
    let mut out = vec![];
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("%%") || line.is_empty() {
            continue;
        }
        if let Some(c) = SEQ.captures(line) {
            out.push(sentence(&name(&c[1]), &name(&c[2]), &c[3]));
            continue;
        }
        // A flowchart line: the shapes become their ids, then the arrows split it.
        let bare = SHAPE.replace_all(line, "$1");
        let mut last: Option<String> = None;
        let mut rest = bare.as_ref();
        while let Some(m) = FLOW.captures(rest) {
            let whole = m.get(0).unwrap();
            let (before, after) = (&rest[..whole.start()], &rest[whole.end()..]);
            let from = last.take().unwrap_or_else(|| before.trim().to_string());
            let label = (1..=4).filter_map(|i| m.get(i)).map(|g| g.as_str()).next().unwrap_or("");
            let to: String = after.split(|c: char| c.is_whitespace() || c == '-' || c == '=' || c == '.' || c == '<').next().unwrap_or("").to_string();
            if from.is_empty() || to.is_empty() || from.contains(' ') {
                break;
            }
            out.push(sentence(&name(&from), &name(&to), label));
            rest = &after[to.len()..];
            last = Some(to);
        }
    }
    out
}

/// The Mermaid blocks of a Markdown file get their sentences too.
pub fn markdown(text: String) -> String {
    let mut sentences = vec![];
    let mut block: Option<String> = None;
    for line in text.lines() {
        let fence = line.trim_start();
        match &mut block {
            None if fence.starts_with("```mermaid") || fence.starts_with("~~~mermaid") => block = Some(String::new()),
            Some(b) if fence.starts_with("```") || fence.starts_with("~~~") => {
                sentences.extend(mermaid_sentences(b));
                block = None;
            }
            Some(b) => {
                b.push_str(line);
                b.push('\n');
            }
            None => {}
        }
    }
    with(text, sentences)
}

// ---------------------------------------------------------------- Graphviz

/// `a [label="Sign in"]`, `"a b" [shape=box, label="…"]`.
static DOT_NODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^\s*("[^"]+"|[A-Za-z0-9_.]+)\s*\[[^\]]*?\blabel\s*=\s*"((?:[^"\\]|\\.)*)""#).unwrap());
/// One step of an edge chain: `a -> b`, `"x" -- "y"`, with the edge's attributes at the end.
static DOT_EDGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"("[^"]+"|[A-Za-z0-9_.]+)\s*(?:->|--)\s*("[^"]+"|[A-Za-z0-9_.]+)"#).unwrap());
static DOT_LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\blabel\s*=\s*"((?:[^"\\]|\\.)*)""#).unwrap());

/// A Graphviz graph's text with a sentence per edge.
pub fn graphviz(text: String) -> String {
    let names: HashMap<String, String> = DOT_NODE.captures_iter(&text).map(|c| (c[1].trim_matches('"').to_string(), c[2].replace("\\n", " "))).collect();
    let name = |id: &str| {
        let id = id.trim_matches('"');
        names.get(id).cloned().unwrap_or_else(|| id.to_string())
    };
    let mut sentences = vec![];
    for statement in text.split([';', '\n']) {
        let label = DOT_LABEL.captures(statement.rsplit_once('[').map_or("", |r| r.1)).map(|c| c[1].replace("\\n", " ")).unwrap_or_default();
        // A chain a -> b -> c: each step, the regex starting again at the node it ended on.
        let mut at = 0;
        while let Some(c) = DOT_EDGE.captures_at(statement, at) {
            sentences.push(sentence(&name(&c[1]), &name(&c[2]), &label));
            at = c.get(2).unwrap().start();
        }
    }
    with(text, sentences)
}

// ---------------------------------------------------------------- PlantUML

/// `Alice -> Bob : Hello`, `[API] --> [Entra ID] : validates`, `A <- B`, with `as` names.
static UML: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*("[^"]+"|\[[^\]]+\]|[\w.]+)\s*(<?-+>?>?|<?\.+>?|<?-\[[^\]]*\]->?)\s*("[^"]+"|\[[^\]]+\]|[\w.]+)\s*(?::\s*(.*))?$"#).unwrap());
static UML_ALIAS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*(?:participant|actor|boundary|control|entity|database|collections|queue|component|node|rectangle|class|interface)\s+("[^"]+"|\[[^\]]+\]|[\w.]+)\s+as\s+([\w.]+)"#).unwrap());

/// A PlantUML diagram's text with a sentence per arrow.
pub fn plantuml(text: String) -> String {
    let clean = |s: &str| s.trim_matches(|c| c == '"' || c == '[' || c == ']').to_string();
    let names: HashMap<String, String> = text.lines().filter_map(|l| UML_ALIAS.captures(l)).map(|c| (c[2].to_string(), clean(&c[1]))).collect();
    let name = |id: &str| names.get(id).cloned().unwrap_or_else(|| clean(id));
    let sentences = text
        .lines()
        .filter_map(|l| UML.captures(l))
        .map(|c| {
            let (a, b) = (name(&c[1]), name(&c[3]));
            let label = c.get(4).map_or("", |m| m.as_str());
            // An arrow pointing left says it the other way round.
            if c[2].starts_with('<') && !c[2].ends_with('>') { sentence(&b, &a, label) } else { sentence(&a, &b, label) }
        })
        .collect();
    with(text, sentences)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(text: &str) -> Vec<String> {
        text.lines().filter(|l| l.contains(" to ")).map(String::from).collect()
    }

    #[test]
    fn diagram_mermaid_flowcharts_and_sequences_say_where_things_go() {
        let flow = "flowchart LR\n  B[Browser] -->|sign in| E((Entra ID))\n  E -- token --> A{API}\n  A --> DB[(Orders)] --> R[Report]\n  %% a comment\n";
        assert_eq!(said(&mermaid(flow.into())), ["Browser to Entra ID: sign in.", "Entra ID to API: token.", "API to Orders.", "Orders to Report."]);

        let seq = "sequenceDiagram\n  participant U as User\n  actor E as Entra ID\n  U->>E: authorize\n  E-->>U: code\n  U->>+API: exchange code for token\n";
        assert_eq!(said(&mermaid(seq.into())), ["User to Entra ID: authorize.", "Entra ID to User: code.", "User to API: exchange code for token."]);

        let md = "# Login\n\nHow it works:\n\n```mermaid\ngraph TD\n  A[App] --> B[Entra ID]\n```\n\nNot a diagram: a --> b\n";
        let read = markdown(md.into());
        assert!(read.ends_with(md), "the text stays");
        // The sentences come first, so a long file's arrows still get vectors.
        let long = format!("{}\n{md}", "words ".repeat(2000));
        assert!(crate::meaning::passages(&markdown(long), "flow.md").first().is_some_and(|p| p.text.starts_with("App to Entra ID.")));
        assert_eq!(said(&read), ["App to Entra ID.", "Not a diagram: a --> b"].map(String::from).into_iter().filter(|s| s.ends_with('.')).collect::<Vec<_>>());
        assert_eq!(mermaid("just words, no arrows".into()), "just words, no arrows");
    }

    #[test]
    fn diagram_graphviz_and_plantuml_say_where_things_go() {
        let dot = "digraph auth {\n  browser [label=\"Browser\"];\n  entra [shape=box, label=\"Entra ID\"];\n  browser -> entra [label=\"sign in\"];\n  entra -> api -> db;\n  \"a b\" -- c\n}\n";
        assert_eq!(said(&graphviz(dot.into())), ["Browser to Entra ID: sign in.", "Entra ID to api.", "api to db.", "a b to c."]);

        let uml = "@startuml\nparticipant \"Web app\" as W\nactor User\nUser -> W : opens\nW --> [Entra ID] : redirects\nW <- [Entra ID] : token\n@enduml\n";
        assert_eq!(said(&plantuml(uml.into())), ["User to Web app: opens.", "Web app to Entra ID: redirects.", "Entra ID to Web app: token."]);
    }

    #[test]
    fn diagram_of_any_bytes_never_panics_and_stays_bounded() {
        let many: String = (0..5000).map(|i| format!("n{i} -> n{}\n", i + 1)).collect();
        assert!(said(&graphviz(many.clone())).len() <= MOST);
        for junk in ["-->|", "[[[[(((", "A -- -->", "->>:", "\"unclosed -> [label=\"", "[a] <-> [b] : x"] {
            let _ = (mermaid(junk.into()), graphviz(junk.into()), plantuml(junk.into()), markdown(format!("```mermaid\n{junk}")));
        }
    }
}
