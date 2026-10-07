use super::*;
use std::collections::HashMap;
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
    for want in ["core/src/main/java/org/keycloak/jose", "services/src/main/java/org/keycloak/keys", "crypto"] {
        assert!(top.iter().any(|l| l == want), "{want} not in {top:?}");
    }
    // cipherscape has 12: it also makes groups for files where assets only occur again
    assert_eq!(top.len(), 9);
    assert!(t.groups.iter().enumerate().all(|(g, _)| !t.children[bom.nodes.len() + g].is_empty()), "an empty group");

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
    assert!(started.elapsed() < crate::test_limit(std::time::Duration::from_secs(5)), "took {:?}", started.elapsed());
}


// Ratings

use assess::{Context, Reason};
use policy::{policy, AssetParams, Param, Resolution};

#[test]
fn status_order_and_roll_up() {
    assert_eq!(Status::NotRated.worst(Status::Safe), Status::Safe);
    assert_eq!(Status::Broken.worst(Status::NotRated), Status::Broken);
    assert_eq!(Status::NotRated.worst(Status::NotRated), Status::NotRated);
    // an unknown asset never lets its component read as green
    assert_eq!(Status::Safe.worst(Status::Unknown), Status::Unknown);
    assert_eq!(Status::Unknown.worst(Status::Deprecated), Status::Deprecated);
    let colors: Vec<_> = Status::ALL.iter().map(|s| s.color()).collect();
    use status::Color::*;
    assert_eq!(colors, [Green, Green, Grey, Yellow, Red, Red, Muted]);
}

#[test]
fn golden_ratings_match_cipherscape() {
    #[derive(serde::Deserialize)]
    struct Case {
        algorithm: String,
        profile: String,
        year: i32,
        expect: Status,
        key_bits: Option<u64>,
        security_bits: Option<u64>,
        param_set: Option<String>,
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!("golden.json")).unwrap();
    assert!(cases.len() > 50);
    for c in cases {
        let asset = AssetParams {
            algorithm_id: c.algorithm.clone(),
            key_bits: c.key_bits,
            security_bits: c.security_bits,
            param_set: c.param_set.clone(),
        };
        let got = policy().evaluate(&asset, &c.profile, c.year).status;
        assert_eq!(got, c.expect, "{} {:?} {:?} {:?} @{} {}", c.algorithm, c.key_bits, c.security_bits, c.param_set, c.profile, c.year);
    }
}

#[test]
fn an_evaluation_says_what_is_missing_and_where_the_rule_comes_from() {
    let e = policy().evaluate(&AssetParams::new("rsa"), "nist", 2026);
    assert_eq!((e.status, e.missing), (Status::Unknown, vec![Param::KeyBits]));
    let e = policy().evaluate(&AssetParams { key_bits: Some(2048), ..AssetParams::new("rsa") }, "nist", 2031);
    let source = e.source.unwrap();
    assert_eq!(source.reference.as_deref(), Some("ir8547"));
    assert!(source.text.contains("IR 8547"));
    assert_eq!(policy().milestones("nist"), [2030, 2035]);
}

#[test]
fn remediation_fits_the_profile_and_parameters() {
    let aes = |bits| AssetParams { key_bits: Some(bits), ..AssetParams::new("aes") };
    let rsa1024 = policy().remediation(&AssetParams { key_bits: Some(1024), ..AssetParams::new("rsa") }, "nist");
    assert_eq!(rsa1024.len(), 2);
    assert!(rsa1024[0].summary.contains("broken"));
    assert!(rsa1024[1].summary.contains("post-quantum"));
    assert_eq!(policy().remediation(&aes(128), "cnsa2").len(), 1);
    assert_eq!(policy().remediation(&aes(256), "cnsa2").len(), 0);
    assert_eq!(policy().remediation(&aes(128), "nist").len(), 0);
}

/// The parts an algorithm resolves to, or why it has none.
type Resolved = Result<Vec<AssetParams>, &'static str>;

fn resolved(name: &str, info: AlgorithmInfo) -> Resolved {
    match policy().resolve(name, &info) {
        Resolution::Rated { parts, .. } => Ok(parts),
        Resolution::NotRated { .. } => Err("not-rated"),
        Resolution::Unresolved { .. } => Err("unresolved"),
    }
}

fn params(id: &str, key_bits: Option<u64>, security_bits: Option<u64>, param_set: Option<&str>) -> AssetParams {
    AssetParams { algorithm_id: id.into(), key_bits, security_bits, param_set: param_set.map(str::to_string) }
}

#[test]
fn names_found_in_real_files_resolve() {
    let none = AlgorithmInfo::default;
    let one = |id, k, s, p| Ok(vec![params(id, k, s, p)]);
    let info = |family: Option<&str>, param_set: Option<&str>, oid: &str| AlgorithmInfo {
        family: family.map(str::to_string),
        param_set: param_set.map(str::to_string),
        oid: Some(oid.to_string()),
        ..Default::default()
    };
    let cases: Vec<(&str, AlgorithmInfo, Resolved)> = vec![
        // CBOMkit: Keycloak and Kafka
        ("AES", none(), one("aes", None, None, None)),
        ("AES128", none(), one("aes", Some(128), None, None)),
        ("AES128-CBC-PKCS5", none(), one("aes", Some(128), None, None)),
        ("AES128-GCM", none(), one("aes", Some(128), None, None)),
        ("ConcatenationKDF", none(), Err("not-rated")),
        ("DSA", none(), one("dsa", None, None, None)),
        ("EC", none(), one("ecc", None, None, None)),
        ("EC-secp256r1", none(), one("ecc", None, Some(128), None)),
        ("EC-secp384r1", none(), one("ecc", None, Some(192), None)),
        ("EC-secp521r1", none(), one("ecc", None, Some(256), None)),
        ("ECDH", none(), one("ecc", None, None, None)),
        ("Ed25519", none(), one("eddsa", None, None, None)),
        ("EdDSA", none(), one("eddsa", None, None, None)),
        ("HMAC-SHA256", none(), one("hmac", None, None, Some("256"))),
        ("HMACSHA2", none(), one("hmac", None, None, None)),
        ("MGF1", none(), Err("not-rated")),
        ("RAW", none(), Err("unresolved")),
        ("PRIVATE KEY", none(), Err("unresolved")),
        ("RSA-2048", none(), one("rsa", Some(2048), None, None)),
        ("RSASSA-PSS", none(), one("rsa", None, None, None)),
        ("SHA1", none(), one("sha1", None, None, None)),
        ("SHA256", none(), one("sha2", None, None, Some("256"))),
        ("SHA512", none(), one("sha2", None, None, Some("512"))),
        // the official examples: composites, and what parameterSetIdentifier means
        (
            "RSA-PKCS1-1.5-SHA512-2048",
            info(Some("RSASSA-PKCS1"), Some("512"), "1.2.840.113549.1.1.13"),
            Ok(vec![params("rsa", Some(2048), None, None), params("sha2", None, None, Some("512"))]),
        ),
        ("SHA384", info(None, Some("384"), "2.16.840.1.101.3.4.2.9"), one("sha2", None, None, Some("384"))),
        (
            "SHA512withRSA",
            info(None, Some("512"), "1.2.840.113549.1.1.13"),
            Ok(vec![params("sha2", None, None, Some("512")), params("rsa", None, None, None)]),
        ),
        ("RSA-2048", info(None, Some("2048"), "1.2.840.113549.1.1.1"), one("rsa", Some(2048), None, None)),
        ("AES-128-GCM-128-12", info(Some("AES"), Some("128"), "2.16.840.1.101.3.4.1.6"), one("aes", Some(128), None, None)),
        (
            "AES-256-GCM",
            AlgorithmInfo { classical_security_level: Some(256), ..info(Some("AES"), None, "2.16.840.1.101.3.4.1.46") },
            one("aes", Some(256), None, None),
        ),
        (
            "ECDH-secp521r1",
            AlgorithmInfo { curve: Some("secp521r1".into()), ..info(Some("ECDH"), None, "1.3.132.0.35") },
            one("ecc", None, Some(256), None),
        ),
        (
            "draft-ietf-tls-hybrid-design-13",
            AlgorithmInfo { primitive: Some("combiner".into()), ..info(None, None, "1.3.101.110") },
            Err("not-rated"),
        ),
        ("ML-DSA-65", none(), one("ml-dsa", None, None, Some("65"))),
        ("Kyber768", none(), one("ml-kem", None, None, Some("768"))),
        ("sha256WithRSAEncryption", none(), Ok(vec![params("sha2", None, None, Some("256")), params("rsa", None, None, None)])),
    ];
    for (name, info, want) in cases {
        assert_eq!(resolved(name, info), want, "{name}");
    }
}

fn outvoted(name: &str, info: AlgorithmInfo) -> Vec<policy::SignalSource> {
    match policy().resolve(name, &info) {
        Resolution::Rated { signals, .. } => signals.iter().filter(|s| !s.agreed).map(|s| s.source).collect(),
        other => panic!("{name}: {other:?}"),
    }
}

#[test]
fn wrong_oids_are_outvoted_not_trusted() {
    use policy::SignalSource::*;
    let with = |family: Option<&str>, param_set: Option<&str>, oid: &str| AlgorithmInfo {
        family: family.map(str::to_string),
        param_set: param_set.map(str::to_string),
        oid: Some(oid.to_string()),
        ..Default::default()
    };

    // ML-KEM-1024 with an AES OID
    let info = AlgorithmInfo { primitive: Some("kem".into()), ..with(Some("ML-KEM"), None, "2.16.840.1.101.3.4.1.48") };
    assert_eq!(resolved("ML-KEM-1024", info.clone()), Ok(vec![params("ml-kem", None, None, Some("1024"))]));
    assert_eq!(outvoted("ML-KEM-1024", info), [Oid]);

    // SHA-384 with the SHA3-384 OID
    let info = with(Some("SHA-2"), Some("384"), "2.16.840.1.101.3.4.2.9");
    assert_eq!(resolved("SHA-384", info.clone()), Ok(vec![params("sha2", None, None, Some("384"))]));
    assert_eq!(outvoted("SHA-384", info), [Oid]);

    // X25519 with the id-ecDH OID: ECC by majority, with the right strength
    let info = AlgorithmInfo { curve: Some("Curve25519".into()), ..with(Some("ECDH"), None, "1.3.132.1.12") };
    assert_eq!(resolved("X25519", info.clone()), Ok(vec![params("ecc", None, Some(128), None)]));
    assert_eq!(outvoted("X25519", info), [Name]);

    // a tie between name and OID goes to the name
    let info = with(None, None, "2.16.840.1.101.3.4.2.9");
    assert_eq!(resolved("SHA-384", info.clone()), Ok(vec![params("sha2", None, None, Some("384"))]));
    assert_eq!(outvoted("SHA-384", info), [Oid]);

    // OID and family disagreeing with no name to settle it: no answer rather than a guess
    assert_eq!(resolved("?", with(Some("AES"), None, "2.16.840.1.101.3.4.2.9")), Err("unresolved"));
}

#[test]
fn every_algorithm_in_the_fixtures_is_resolved_or_skipped_on_purpose() {
    let mut unresolved = std::collections::BTreeSet::new();
    for path in REAL {
        let bom = open(path);
        for n in &bom.nodes {
            if let Some(Crypto::Algorithm(info)) = &n.crypto
                && matches!(policy().resolve(&n.label, info), Resolution::Unresolved { .. })
            {
                unresolved.insert(n.label.clone());
            }
        }
    }
    // Only junk may stay unresolved. Teach the resolver or the catalog before adding to this.
    assert_eq!(unresolved.into_iter().collect::<Vec<_>>(), ["PRIVATE KEY", "RAW"]);
}

/// The fixed moment the assessment tests rate at: 2026-09-24.
fn context(year: i32) -> Context {
    let now = assess::parse_time("2026-09-24T00:00:00Z").unwrap();
    Context { profile: "nist".into(), year, now, expiry_warning_days: 90 }
}

struct Rated {
    bom: Bom,
    tree: Tree,
    resolutions: Vec<Option<Resolution>>,
    issues: Vec<Issue>,
}

impl Rated {
    fn new(path: &str) -> Rated {
        let bom = open(path);
        let tree = tree::build(&bom, tree::modes(&bom)[0]);
        let (resolutions, issues) = assess::resolve(&bom, policy());
        Rated { bom, tree, resolutions, issues }
    }

    fn at(&self, year: i32) -> assess::Assessed {
        assess::assess(&self.bom, &self.tree, policy(), &self.resolutions, &context(year))
    }

    fn find(&self, label: &str) -> u32 {
        find(&self.tree, &self.bom, label)
    }

    fn status(&self, a: &assess::Assessed, label: &str) -> Status {
        a.status[self.find(label) as usize]
    }
}

#[test]
fn keycloak_is_rated_by_the_catalog() {
    let r = Rated::new("cbomkit/keycloak.cdx.json");
    let now = r.at(2026);
    for (label, want) in [
        ("RSA-2048", Status::Acceptable),
        ("SHA1", Status::Deprecated),
        ("DSA", Status::Disallowed),
        ("AES128-GCM", Status::Acceptable),
        ("EC-secp256r1", Status::Acceptable),
        ("MGF1", Status::NotRated),
        // honest where parameters are missing: EC without a curve could be P-192
        ("EC", Status::Unknown),
        ("RAW", Status::Unknown),
    ] {
        assert_eq!(r.status(&now, label), want, "{label}");
    }
    let ec = now.explain(r.find("EC"));
    assert!(matches!(&ec.reasons[0], Reason::Rule { evaluation, .. } if evaluation.missing == [Param::SecurityBits]));

    // a key without parameters takes its algorithm's status
    let key = now.explain(r.find("secret-key@ad2ff456-2f18-4c34-938b-54964e020aeb"));
    assert_eq!(key.reasons, [Reason::Inherited { node: r.find("HMAC-SHA256"), status: Status::Acceptable }]);
    assert_eq!(key.status, Status::Acceptable);

    // the worst rolls up to the root, and the explanation names the asset, not a group or a key
    assert_eq!(now.status[0], Status::Disallowed);
    let Reason::Rollup { node, status } = now.explain(0).reasons[0].clone() else { panic!("not a roll-up") };
    assert_eq!(status, Status::Disallowed);
    assert_eq!(r.tree.node(&r.bom, node).label, "DSA");

    // later years
    assert_eq!(r.status(&r.at(2031), "RSA-2048"), Status::Deprecated);
    assert_eq!(r.status(&r.at(2036), "RSA-2048"), Status::Disallowed);
    assert_eq!(r.status(&r.at(2036), "AES128-GCM"), Status::Acceptable);

    // one name nothing knows, and nothing else
    assert_eq!(codes_of(&r.issues), [IssueCode::UnresolvedAlgorithm]);
    assert!(r.issues[0].message.contains("\"RAW\""));
}

fn codes_of(issues: &[Issue]) -> Vec<IssueCode> {
    issues.iter().map(|i| i.code).collect()
}

#[test]
fn an_expired_certificate_is_red_whatever_its_algorithms() {
    let r = Rated::new("bom-examples/certificate.cdx.json");
    let now = r.at(2026);
    assert_eq!(r.status(&now, "google.com"), Status::Disallowed);
    let reasons = now.explain(r.find("google.com")).reasons;
    assert!(reasons.iter().any(|x| matches!(x, Reason::Lifecycle { not_valid_after, status: Status::Disallowed, days_left, .. }
        if not_valid_after == "2017-11-22T07:59:59Z" && *days_left < -3000)));

    // a key is rated by its own size, through its algorithm (the key comes before the algorithm of the same name)
    let key = now.explain(r.find("RSA-2048"));
    assert!(matches!(&key.reasons[0], Reason::Rule { params, .. } if *params == params_rsa_2048()));

    // SHA512withRSA names no key size, and none is guessed
    assert_eq!(r.status(&now, "SHA512withRSA"), Status::Unknown);
}

fn params_rsa_2048() -> AssetParams {
    AssetParams { key_bits: Some(2048), ..AssetParams::new("rsa") }
}

#[test]
fn a_certificate_close_to_expiry_is_deprecated() {
    let doc = |after: &str| {
        format!(
            r#"{{"bomFormat": "CycloneDX", "specVersion": "1.6", "components": [{{"type": "cryptographic-asset",
            "name": "c", "cryptoProperties": {{"assetType": "certificate", "certificateProperties": {{"notValidAfter": "{after}"}}}}}}]}}"#
        )
    };
    let status = |after| {
        let bom = parse_json(&doc(after), None).unwrap();
        let tree = tree::build(&bom, TreeMode::Flat);
        let (res, _) = assess::resolve(&bom, policy());
        assess::assess(&bom, &tree, policy(), &res, &context(2026)).status[1]
    };
    assert_eq!(status("2026-10-24T00:00:00Z"), Status::Deprecated);
    assert_eq!(status("2027-09-24"), Status::Acceptable);
    assert_eq!(status("2026-09-23T23:00:00+02:00"), Status::Disallowed);
    assert_eq!(status("next tuesday"), Status::Unknown); // no date, no references
}

#[test]
fn outvoted_oids_are_warnings() {
    let r = Rated::new("bom-examples/algorithm.cdx.json");
    let m: Vec<_> = r.issues.iter().filter(|i| i.code == IssueCode::OidMismatch).collect();
    assert_eq!(m.len(), 1);
    assert!(m[0].message.contains("ML-KEM-1024"), "{}", m[0].message);
}

#[test]
fn assets_that_refer_to_each_other_in_a_circle_are_unknown() {
    let bom = parse_json(
        r#"{"bomFormat": "CycloneDX", "specVersion": "1.6", "components": [
          {"type": "cryptographic-asset", "name": "a", "bom-ref": "a", "cryptoProperties": {"assetType": "protocol",
            "protocolProperties": {"cipherSuites": [{"algorithms": ["b"]}]}}},
          {"type": "cryptographic-asset", "name": "b", "bom-ref": "b", "cryptoProperties": {"assetType": "protocol",
            "protocolProperties": {"cipherSuites": [{"algorithms": ["a"]}]}}}]}"#,
        None,
    )
    .unwrap();
    let tree = tree::build(&bom, TreeMode::Flat);
    let (res, _) = assess::resolve(&bom, policy());
    let a = assess::assess(&bom, &tree, policy(), &res, &context(2026));
    assert_eq!((a.status[1], a.status[2]), (Status::Unknown, Status::Unknown));
}

#[test]
fn dates_without_a_date_crate() {
    use assess::{parse_time, start_of_year, year_of};
    assert_eq!(parse_time("1970-01-01"), Some(0));
    assert_eq!(parse_time("2000-03-01T00:00:00Z"), Some(951_868_800));
    assert_eq!(parse_time("2000-03-01T01:00:00.123+01:00"), Some(951_868_800));
    assert_eq!(parse_time("2000-13-01"), None);
    assert_eq!(year_of(951_868_800), 2000);
    assert_eq!(year_of(start_of_year(2031)), 2031);
    assert_eq!(year_of(start_of_year(2031) - 1), 2030);
}

#[test]
fn sniffing_by_name_and_by_content() {
    assert!(sniff_name("app.cdx.json") && sniff_name("App.CDX.XML") && sniff_name("x.cbom.json") && sniff_name("bom.json"));
    assert!(!sniff_name("package.json") && !sniff_name("sbom.txt"));
    assert!(sniff_head(br#"{"$schema": "x", "bomFormat": "CycloneDX", "specVersion": "1.6"}"#));
    assert!(sniff_head(br#"<?xml version="1.0"?><bom xmlns="http://cyclonedx.org/schema/bom/1.6">"#));
    assert!(!sniff_head(br#"{"name": "CycloneDX", "version": "1"}"#));

    assert!(sniff(&fixture("spec/cryptography-full-1.7.json")));
    assert!(sniff(&fixture("spec/cryptography-full-1.7.xml")));
    assert!(!sniff(&fixture("README.md")));
    assert!(!sniff(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")));
}

// Diff

use diff::Change;
use serde_json::Value;

struct Version {
    bom: Bom,
    tree: Tree,
    resolutions: Vec<Option<Resolution>>,
    status: Vec<Status>,
}

impl Version {
    fn new(doc: &Value) -> Version {
        let bom = ingest::from_value(doc, Format::Json, None).unwrap();
        let tree = tree::build(&bom, tree::modes(&bom)[0]);
        let (resolutions, _) = assess::resolve(&bom, policy());
        let status = assess::assess(&bom, &tree, policy(), &resolutions, &context(2026)).status;
        Version { bom, tree, resolutions, status }
    }

    fn side(&self) -> diff::Side<'_> {
        diff::Side { bom: &self.bom, tree: &self.tree, resolutions: &self.resolutions, status: &self.status }
    }

    fn labelled(&self, change: &[Change], which: Change) -> Vec<String> {
        let mut out: Vec<String> = (0..self.tree.len())
            .filter(|&i| change[i] == which)
            .map(|i| self.tree.node(&self.bom, i as u32).label.clone())
            .collect();
        out.sort();
        out
    }
}

fn json(path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixture(path)).unwrap()).unwrap()
}

fn components(doc: &mut Value) -> &mut Vec<Value> {
    doc["components"].as_array_mut().unwrap()
}

#[test]
fn new_bom_refs_on_every_scan_change_nothing() {
    let before = json("cbomkit/keycloak.cdx.json");
    let mut after = before.clone();
    let mut rekey = HashMap::new();
    for (i, c) in components(&mut after).iter_mut().enumerate() {
        let new = format!("new-uuid-{i}");
        rekey.insert(c["bom-ref"].as_str().unwrap().to_string(), new.clone());
        c["bom-ref"] = new.clone().into();
        let name = c["name"].as_str().unwrap().to_string();
        if let Some((prefix, _)) = name.split_once('@') {
            c["name"] = format!("{prefix}@{new}").into();
        }
    }
    for d in after["dependencies"].as_array_mut().unwrap() {
        d["ref"] = rekey[d["ref"].as_str().unwrap()].clone().into();
        for r in d["dependsOn"].as_array_mut().into_iter().flatten() {
            *r = rekey[r.as_str().unwrap()].clone().into();
        }
    }
    let d = diff::diff(&Version::new(&before).side(), &Version::new(&after).side());
    assert_eq!(d.counts, diff::Counts::default());
}

#[test]
fn a_crypto_asset_is_known_by_what_it_is_and_where_it_sits() {
    let v = Version::new(&json("cbomkit/keycloak.cdx.json"));
    let ids = diff::identities(&v.side());
    assert_eq!(
        ids[find(&v.tree, &v.bom, "DSA") as usize],
        "alg:dsa:2048:: @ group:file:saml-core-api/src/main/java/org/keycloak/dom/xmlsec/w3/xmldsig/DSAKeyValueType.java"
    );
}

#[test]
fn keycloak_before_and_after_a_clean_up() {
    let before = json("cbomkit/keycloak.cdx.json");
    let mut after = before.clone();
    // fixed: SHA-1 becomes SHA-256 in the same place; DSA and its two keys are gone
    let sha1 = components(&mut after).iter_mut().find(|c| c["name"] == "SHA1").unwrap();
    sha1["name"] = "SHA256".into();
    sha1["cryptoProperties"]["oid"] = "2.16.840.1.101.3.4.2.1".into();
    sha1["cryptoProperties"]["algorithmProperties"]["parameterSetIdentifier"] = "256".into();
    let gone: Vec<Value> = components(&mut after)
        .iter()
        .filter(|c| {
            c["name"] == "DSA"
                || c["evidence"]["occurrences"][0]["location"].as_str().is_some_and(|l| l.ends_with("DSAKeyValueType.java"))
        })
        .map(|c| c["bom-ref"].clone())
        .collect();
    components(&mut after).retain(|c| !gone.contains(&c["bom-ref"]));
    after["dependencies"].as_array_mut().unwrap().retain(|d| !gone.contains(&d["ref"]));
    // a new risk: someone added MD5
    components(&mut after).push(serde_json::json!({
        "type": "cryptographic-asset", "bom-ref": "md5-new", "name": "MD5",
        "cryptoProperties": {"assetType": "algorithm", "algorithmProperties": {"primitive": "hash"}, "oid": "1.2.840.113549.2.5"},
        "evidence": {"occurrences": [{"location": "services/src/main/java/org/keycloak/NewChecksum.java", "line": 12}]}
    }));

    let (a, b) = (Version::new(&before), Version::new(&after));
    let d = diff::diff(&a.side(), &b.side());
    assert_eq!((d.counts.added, d.counts.removed, d.counts.new_risks), (2, 4, 1));
    assert_eq!(d.counts.fixed, 4); // SHA-1 (deprecated), and DSA with its two keys (disallowed)
    let removed: Vec<&str> = d.removed.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(removed, ["DSA", "key@b627000e-ed4e-449c-acb9-4e9547d6ee93", "key@58af0705-bf7c-4abc-9f49-5b3ed8dd31ca", "SHA1"]);
    assert_eq!(b.labelled(&d.change, Change::Added), ["MD5", "SHA256"]);
}

#[test]
fn a_renewed_certificate_is_an_improvement_not_a_swap() {
    let before = json("bom-examples/certificate.cdx.json");
    let mut after = before.clone();
    let cert = components(&mut after).iter_mut().find(|c| c["cryptoProperties"]["assetType"] == "certificate").unwrap();
    cert["cryptoProperties"]["certificateProperties"]["notValidAfter"] = "2027-11-22T07:59:59Z".into();
    let (a, b) = (Version::new(&before), Version::new(&after));
    let d = diff::diff(&a.side(), &b.side());
    assert_eq!(b.labelled(&d.change, Change::Improved), ["google.com"]);
    assert_eq!((d.counts.added, d.counts.removed, d.counts.improved, d.counts.fixed), (0, 0, 1, 1));
}

// View helpers

use view::{Filter, Loaded};

#[test]
fn view_names_filters_and_sunburst() {
    let l = Loaded::open(&fixture("cbomkit/keycloak.cdx.json"), None).unwrap();
    assert_eq!(l.mode, TreeMode::Files);
    let key = (0..l.tree.len() as u32).find(|&i| l.node(i).label == "secret-key@ad2ff456-2f18-4c34-938b-54964e020aeb").unwrap();
    assert_eq!(l.key_of(key), Some(("secret-key".into(), "HMAC-SHA256".into())));
    assert_eq!(l.families(key), [policy::Family::Mac]);
    let flat = Loaded::open(&fixture("cbomkit/keycloak.cdx.json"), Some(TreeMode::Flat)).unwrap();
    assert!((0..flat.tree.len() as u32).any(|i| flat.group_kind(i) == Some(NodeKind::Algorithm)));

    let text: Vec<String> = (0..l.tree.len() as u32).map(|i| l.search_text(i)).collect();
    let f = Filter { status: [Status::Disallowed].into(), ..Default::default() };
    let (matches, keep) = view::mask(&l, &f, &text);
    assert_eq!(matches.iter().filter(|&&m| m).count(), 3);
    assert!(keep[0] && !matches[0]);
    let f = Filter { query: "jwkparser".into(), ..Default::default() };
    let (matches, _) = view::mask(&l, &f, &text);
    assert!(matches.iter().filter(|&&m| m).count() >= 2); // the file group and what is found in it

    let leaves = view::leaf_counts(&l.tree, None);
    let arcs = view::sunburst(&l, &leaves, 0, view::RINGS, view::MIN_ANGLE);
    let ring1: f64 = arcs.iter().filter(|a| a.depth == 1).map(|a| a.a1 - a.a0).sum();
    assert!((ring1 - 360.0).abs() < 1e-9);
    assert!(arcs.iter().all(|a| a.node.is_none() || a.a1 - a.a0 >= view::MIN_ANGLE));
    let (_, keep) = view::mask(&l, &Filter { status: [Status::Disallowed].into(), ..Default::default() }, &text);
    let leaves = view::leaf_counts(&l.tree, Some(&keep));
    assert_eq!(leaves[0], 3.0);
}

#[test]
fn a_file_named_in_a_bom_gets_the_systems_separators() {
    let dir = std::env::temp_dir().join(format!("coxswain-test-bom-disk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("src").join("main").join("Keys.java");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "").unwrap();
    let bom = dir.join("cbom.json");
    // the same string as joining part by part: on Windows, backslashes only
    assert_eq!(view::on_disk(&bom, "src/main/Keys.java").unwrap().to_string_lossy(), file.to_string_lossy());
    assert_eq!(view::on_disk(&bom, "/src/./main/Keys.java").unwrap().to_string_lossy(), file.to_string_lossy());
    assert_eq!(view::on_disk(&bom, "src/../../etc/passwd"), None);
    assert_eq!(view::on_disk(&bom, "src/main/Gone.java"), None);
    std::fs::remove_dir_all(&dir).unwrap();
}
