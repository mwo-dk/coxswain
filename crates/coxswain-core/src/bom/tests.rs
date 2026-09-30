use super::*;
use std::path::PathBuf;

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/bom/testdata").join(path)
}

fn open(path: &str) -> Bom {
    load(&fixture(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

const REAL: [&str; 9] = [
    "bom-examples/algorithm.cdx.json",
    "bom-examples/certificate.cdx.json",
    "bom-examples/example-with-dependencies.cdx.json",
    "bom-examples/key.cdx.json",
    "bom-examples/protocol.cdx.json",
    "cbomkit/flick.cdx.json",
    "cbomkit/kafka.cdx.json",
    "cbomkit/keycloak.cdx.json",
    "cbomkit/theia-unknown-key-size.cdx.json",
];

fn by_label<'a>(bom: &'a Bom, label: &str) -> (u32, &'a Node) {
    let i = bom.nodes.iter().position(|n| n.label == label).unwrap_or_else(|| panic!("no node {label}"));
    (i as u32, &bom.nodes[i])
}

fn by_key(bom: &Bom, key: &str) -> u32 {
    bom.nodes.iter().position(|n| n.key == key).unwrap_or_else(|| panic!("no node {key}")) as u32
}

fn codes(bom: &Bom) -> Vec<IssueCode> {
    bom.issues.iter().map(|i| i.code).collect()
}

fn algorithm(node: &Node) -> &AlgorithmInfo {
    match &node.crypto {
        Some(Crypto::Algorithm(a)) => a,
        other => panic!("{} is not an algorithm: {other:?}", node.label),
    }
}

#[test]
fn every_fixture_has_a_root_and_edges_between_nodes() {
    for path in REAL {
        let bom = open(path);
        assert_eq!(bom.nodes[0].kind, NodeKind::Application, "{path}");
        for e in &bom.edges {
            assert!((e.from as usize) < bom.nodes.len() && (e.to as usize) < bom.nodes.len(), "{path}");
        }
    }
}

#[test]
fn refuses_what_is_not_cyclonedx() {
    assert_eq!(parse_json(r#"{"spdxVersion": "SPDX-2.3"}"#, None), Err(Error::NotCycloneDx));
    assert_eq!(parse_json("[]", None), Err(Error::NotCycloneDx));
    assert!(matches!(parse_json("{\n  \"bomFormat\": ", None), Err(Error::InvalidJson { line: 2, .. })));
}

#[test]
fn cbomkit_output_gets_a_root_named_after_the_repository() {
    let bom = open("cbomkit/keycloak.cdx.json");
    assert_eq!(bom.nodes[0].label, "keycloak");
    assert!(bom.nodes[0].synthetic);
    assert_eq!(bom.source.properties["gitUrl"], "https://github.com/keycloak/keycloak");

    // only crypto assets, linked key to algorithm
    assert_eq!(bom.nodes.len(), 57);
    assert!(bom.nodes[1..].iter().all(|n| n.kind != NodeKind::Component));
    assert_eq!(bom.edges.len(), 36);
    assert!(bom.edges.iter().all(|e| e.kind == EdgeKind::DependsOn));
    assert_eq!(bom.issues, vec![]);

    // evidence, with the API call
    let (_, rsa) = by_label(&bom, "RSA-2048");
    assert_eq!(rsa.occurrences.len(), 3);
    assert_eq!(rsa.occurrences[0].location, "core/src/main/java/org/keycloak/jose/jwk/JWKParser.java");
    assert_eq!(rsa.occurrences[0].line, Some(136));
    assert!(rsa.occurrences[0].context.as_deref().unwrap().contains("KeyFactory#getInstance"));
    assert_eq!(bom.nodes.iter().map(|n| n.occurrences.len()).sum::<usize>(), 103);
}

#[test]
fn a_root_without_git_url_is_named_after_the_file() {
    let bom = parse(r#"{"bomFormat": "CycloneDX", "specVersion": "1.6"}"#, Some("app.cdx.json")).unwrap();
    assert_eq!(bom.nodes[0].label, "app");
    let bom = parse(r#"{"bomFormat": "CycloneDX", "specVersion": "1.6"}"#, None).unwrap();
    assert_eq!(bom.nodes[0].label, "CBOM");
}

#[test]
fn components_depend_on_and_provide() {
    let bom = open("bom-examples/example-with-dependencies.cdx.json");
    assert_eq!(bom.source.spec_version, "1.7");
    assert_eq!((bom.nodes[0].key.as_str(), bom.nodes[0].label.as_str()), ("acme-application", "Acme Application"));
    assert!(!bom.nodes[0].synthetic);
    let edge = |from, to| {
        let (from, to) = (by_key(&bom, from), by_key(&bom, to));
        bom.edges.iter().find(|e| e.from == from && e.to == to).map(|e| e.kind)
    };
    assert_eq!(edge("acme-application", "crypto-library"), Some(EdgeKind::DependsOn));
    assert_eq!(edge("crypto-library", "aes128gcm"), Some(EdgeKind::Provides));
    assert_eq!(edge("crypto-library", "some-library"), Some(EdgeKind::DependsOn));
}

#[test]
fn reads_algorithm_family_and_strips_the_curve_namespace() {
    let bom = open("bom-examples/algorithm.cdx.json");
    let a = algorithm(by_label(&bom, "ECDH-secp521r1").1);
    assert_eq!(a.family.as_deref(), Some("ECDH"));
    assert_eq!(a.curve.as_deref(), Some("secp521r1"));
    assert_eq!(a.oid.as_deref(), Some("1.3.132.0.35"));
}

#[test]
fn assets_without_a_bom_ref_are_reported_and_kept() {
    let bom = open("bom-examples/algorithm.cdx.json");
    assert_eq!(codes(&bom).iter().filter(|&&c| c == IssueCode::MissingBomRef).count(), 2);
    assert!(by_label(&bom, "AES-128-GCM-128-12").1.key.starts_with("#anon-"));
}

#[test]
fn a_dangling_reference_is_an_issue() {
    let bom = open("bom-examples/protocol.cdx.json");
    let dangling: Vec<_> = bom.issues.iter().filter(|i| i.code == IssueCode::DanglingRef).collect();
    assert_eq!(dangling.len(), 1);
    assert!(dangling[0].message.contains("crypto/algorithm/rsa-2048@1.2.840.113549.1.1.1"));
}

#[test]
fn a_certificate_links_its_signature_algorithm_and_key() {
    let bom = open("bom-examples/certificate.cdx.json");
    let (cert, node) = by_label(&bom, "google.com");
    let Some(Crypto::Certificate(info)) = &node.crypto else { panic!("not a certificate") };
    assert_eq!(info.not_valid_after.as_deref(), Some("2017-11-22T07:59:59Z"));
    assert_eq!(info.format.as_deref(), Some("X.509"));
    let out: Vec<_> =
        bom.edges.iter().filter(|e| e.from == cert).map(|e| (e.kind, bom.nodes[e.to as usize].label.as_str())).collect();
    assert_eq!(out, [(EdgeKind::SignedWith, "SHA512withRSA"), (EdgeKind::HasKey, "RSA-2048")]);
    assert_eq!(by_label(&bom, "RSA-2048").1.kind, NodeKind::Material);
}

#[test]
fn other_spec_versions_load_with_a_warning() {
    let bom = parse_json(r#"{"bomFormat": "CycloneDX", "specVersion": "1.4", "components": [{"type": "library", "name": "x"}]}"#, None)
        .unwrap();
    assert_eq!(codes(&bom), [IssueCode::UnsupportedSpecVersion]);
    assert_eq!(bom.nodes.len(), 2);
}

#[test]
fn a_duplicate_bom_ref_keeps_the_first() {
    let bom = parse_json(
        r#"{"bomFormat": "CycloneDX", "specVersion": "1.6", "components": [
            {"type": "library", "name": "a", "bom-ref": "x"},
            {"type": "library", "name": "b", "bom-ref": "x"}],
            "dependencies": [{"ref": "x", "dependsOn": ["x", "nope"]}]}"#,
        None,
    )
    .unwrap();
    assert_eq!(codes(&bom), [IssueCode::DuplicateBomRef, IssueCode::DanglingRef]);
    assert_eq!(bom.edges, vec![]); // a self edge is dropped
}

// XML. The specification ships its test documents in XML and JSON; where they are true twins,
// both must give the same model.

fn comparable(bom: &Bom) -> (String, Vec<Node>, Vec<Edge>, Vec<IssueCode>) {
    let nodes = bom.nodes.iter().map(|n| Node { raw: serde_json::Value::Null, ..n.clone() }).collect();
    (bom.source.spec_version.clone(), nodes, bom.edges.clone(), codes(bom))
}

#[test]
fn xml_parses_to_the_same_model_as_json() {
    for name in ["cryptography-implementation-1.6", "cryptography-full-1.7", "cryptography-certificate-advanced-1.7"] {
        let xml = open(&format!("spec/{name}.xml"));
        let json = open(&format!("spec/{name}.json"));
        assert_eq!(xml.source.format, Format::Xml);
        assert!(xml.nodes.len() > 1, "{name}");
        assert_eq!(comparable(&xml), comparable(&json), "{name}");
    }
}

#[test]
fn xml_crypto_ref_in_protocol_properties_is_a_uses_edge() {
    let bom = open("spec/cryptography-full-1.6.xml");
    let (from, to) = (by_key(&bom, "asset-2"), by_key(&bom, "asset-4"));
    assert!(bom.edges.contains(&Edge { from, to, kind: EdgeKind::Uses }));
    let a = algorithm(&bom.nodes[by_key(&bom, "asset-1") as usize]);
    assert_eq!(a.curve.as_deref(), Some("brainpoolP160r1"));
    assert_eq!(a.functions, ["keygen", "encrypt", "decrypt", "tag"]);
    assert_eq!(a.classical_security_level, Some(128));
}

#[test]
fn xml_metadata_component_nested_dependencies_and_provides() {
    let bom = open("spec/cryptography-implementation-1.7.xml");
    assert_eq!(bom.nodes[0].key, "acme-application");
    let k = |key| by_key(&bom, key);
    for edge in [
        Edge { from: k("acme-application"), to: k("crypto-library"), kind: EdgeKind::DependsOn },
        Edge { from: k("crypto-library"), to: k("aes128gcm"), kind: EdgeKind::Provides },
        Edge { from: k("draftietftlshybriddesign13"), to: k("mlkem1024"), kind: EdgeKind::DependsOn },
    ] {
        assert!(bom.edges.contains(&edge), "{edge:?}");
    }
}

#[test]
fn xml_spec_version_comes_from_the_namespace() {
    let bom = parse(r#"<bom xmlns="http://cyclonedx.org/schema/bom/1.7" version="1"/>"#, None).unwrap();
    assert_eq!(bom.source.spec_version, "1.7");
    assert_eq!(codes(&bom), []);
}

#[test]
fn xml_with_a_doctype_is_refused() {
    let bomb = r#"<?xml version="1.0"?><!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;">]><bom xmlns="http://cyclonedx.org/schema/bom/1.6"><components/></bom>"#;
    assert_eq!(parse(bomb, None), Err(Error::UnsafeXml));
    assert_eq!(parse("<?xml version=\"1.0\"?>\n<!doctype bom><bom/>", None), Err(Error::UnsafeXml));
}

#[test]
fn malformed_xml_says_where() {
    let err = parse("<bom>\n  <components>\n</bom>", None).unwrap_err();
    assert!(matches!(err, Error::InvalidXml { line: 3, .. }), "{err:?}");
    assert!(matches!(parse("<bom>\n<components>", None), Err(Error::InvalidXml { .. })));
    let deep = format!("<bom>{}", "<a>".repeat(1000));
    assert!(matches!(parse(&deep, None), Err(Error::InvalidXml { .. })));
}

#[test]
fn xml_entities_are_the_five_and_character_references_only() {
    let doc = r#"<bom xmlns="http://cyclonedx.org/schema/bom/1.6"><components><component type="library"><name>a&amp;b&#233;&nope;</name></component></components></bom>"#;
    assert_eq!(parse(doc, None).unwrap().nodes[1].label, "a&bé&nope;");
}

#[test]
fn xml_properties_become_name_and_value() {
    let doc = r#"<bom xmlns="http://cyclonedx.org/schema/bom/1.6"><metadata><properties>
        <property name="gitUrl">https://github.com/acme/rocket.git</property></properties></metadata></bom>"#;
    let bom = parse(doc, None).unwrap();
    assert_eq!(bom.nodes[0].label, "rocket");
}

// The tree

fn check_tree(t: &Tree, path: &str) {
    assert_eq!(t.parent[0], None, "{path}");
    let mut seen = vec![false; t.len()];
    for &v in &t.order {
        if v != 0 {
            let p = t.parent[v as usize].unwrap();
            assert!(seen[p as usize], "{path}: parent after child");
            assert_eq!(t.depth[v as usize], t.depth[p as usize] + 1, "{path}");
        }
        seen[v as usize] = true;
    }
    assert!(seen.iter().all(|&s| s), "{path}: a node is not in the tree");
    assert_eq!(t.order.len(), t.len(), "{path}");
}

fn labels(t: &Tree, bom: &Bom, i: u32) -> Vec<String> {
    t.children[i as usize].iter().map(|&c| t.node(bom, c).label.clone()).collect()
}

fn find(t: &Tree, bom: &Bom, label: &str) -> u32 {
    (0..t.len() as u32).find(|&i| t.node(bom, i).label == label).unwrap_or_else(|| panic!("no node {label}"))
}

#[test]
fn every_fixture_makes_a_tree_in_every_mode() {
    for path in REAL {
        let bom = open(path);
        for mode in tree::modes(&bom) {
            check_tree(&tree::build(&bom, mode), path);
        }
    }
}

#[test]
fn scanner_output_is_grouped_by_source_file() {
    let bom = open("cbomkit/keycloak.cdx.json");
    assert_eq!(tree::modes(&bom), [TreeMode::Files, TreeMode::Flat]);
    let t = tree::build(&bom, TreeMode::Files);
    let top = labels(&t, &bom, 0);
    for want in ["core/src/main/java/org/keycloak/jose", "services/src/main/java/org/keycloak", "crypto"] {
        assert!(top.iter().any(|l| l == want), "{want} not in {top:?}");
    }
    assert_eq!(top.len(), 12);

    // single-child directory chains are folded
    for (g, group) in t.groups.iter().enumerate() {
        let i = bom.nodes.len() + g;
        if group.group.as_ref().unwrap().kind == GroupKind::Directory {
            let children = &t.children[i];
            let only_a_file = children.len() == 1
                && t.node(&bom, children[0]).group.as_ref().is_some_and(|g| g.kind == GroupKind::File);
            assert!(children.len() > 1 || only_a_file, "{} is a chain", group.label);
        }
    }

    // under the file of the first occurrence; other files get an edge
    let rsa = find(&t, &bom, "RSA-2048");
    assert_eq!(t.node(&bom, t.parent[rsa as usize].unwrap()).label, "JWKParser.java");
    let others: Vec<_> = t
        .cross_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::OccursIn && e.to == rsa)
        .map(|e| t.node(&bom, e.from).label.as_str())
        .collect();
    assert_eq!(others, ["RSAKeyValueType.java"]); // two occurrences in one file are one edge

    assert!(t.groups.iter().all(|g| g.group.as_ref().unwrap().kind != GroupKind::Kind));
}

#[test]
fn the_dependency_tree_is_followed_when_there_is_one() {
    let bom = open("bom-examples/example-with-dependencies.cdx.json");
    assert_eq!(tree::modes(&bom)[0], TreeMode::Dependencies);
    let t = tree::build(&bom, TreeMode::Dependencies);
    assert_eq!(labels(&t, &bom, 0), ["Crypto library"]);
    let mut below = labels(&t, &bom, find(&t, &bom, "Crypto library"));
    below.sort();
    assert_eq!(below, ["AES-128-GCM-128-12", "Some library"]);
    assert_eq!(t.cross_edges, vec![]);
}

#[test]
fn referenced_assets_hang_under_their_referrer_before_kind_groups() {
    let bom = open("bom-examples/certificate.cdx.json");
    assert_eq!(tree::modes(&bom), [TreeMode::Flat]);
    let t = tree::build(&bom, TreeMode::Flat);
    assert_eq!(labels(&t, &bom, 0), ["Certificates"]);
    assert_eq!(labels(&t, &bom, find(&t, &bom, "google.com")), ["SHA512withRSA", "RSA-2048"]);
    assert_eq!(labels(&t, &bom, find(&t, &bom, "RSA-2048")), ["RSA-2048"]); // key, then its algorithm
}

#[test]
fn a_very_deep_source_path_costs_no_stack() {
    let location = "d/".repeat(100_000) + "f.java";
    let doc = serde_json::json!({
        "bomFormat": "CycloneDX", "specVersion": "1.6",
        "components": [{"type": "cryptographic-asset", "name": "AES", "bom-ref": "a",
            "cryptoProperties": {"assetType": "algorithm"},
            "evidence": {"occurrences": [{"location": location}]}}]
    });
    let bom = ingest::from_value(&doc, Format::Json, None).unwrap();
    let t = tree::build(&bom, TreeMode::Files);
    check_tree(&t, "deep");
    assert_eq!(t.depth[1], 3); // root, the folded directory chain, the file, the asset
}

#[test]
fn a_large_bom_builds_quickly() {
    // A component tree of 20k nodes, each library providing a few algorithms.
    let mut components = vec![];
    let mut dependencies = vec![];
    for i in 0..4000 {
        let provides: Vec<String> = (0..4).map(|j| format!("alg-{i}-{j}")).collect();
        components.push(serde_json::json!({"type": "library", "name": format!("lib-{i}"), "bom-ref": format!("lib-{i}")}));
        for r in &provides {
            components.push(serde_json::json!({"type": "cryptographic-asset", "name": r, "bom-ref": r,
                "cryptoProperties": {"assetType": "algorithm"}}));
        }
        let parent = if i == 0 { "app".to_string() } else { format!("lib-{}", (i - 1) / 3) };
        dependencies.push(serde_json::json!({"ref": parent, "dependsOn": [format!("lib-{i}")]}));
        dependencies.push(serde_json::json!({"ref": format!("lib-{i}"), "provides": provides}));
    }
    let doc = serde_json::json!({"bomFormat": "CycloneDX", "specVersion": "1.6",
        "metadata": {"component": {"type": "application", "name": "app", "bom-ref": "app"}},
        "components": components, "dependencies": dependencies});
    let started = std::time::Instant::now();
    let bom = ingest::from_value(&doc, Format::Json, None).unwrap();
    let t = tree::build(&bom, tree::modes(&bom)[0]);
    assert_eq!(t.mode, TreeMode::Dependencies);
    assert_eq!(bom.nodes.len(), 20_001);
    assert_eq!(t.len(), 20_001);
    check_tree(&t, "large");
    assert!(started.elapsed().as_secs() < 5, "took {:?}", started.elapsed());
}

