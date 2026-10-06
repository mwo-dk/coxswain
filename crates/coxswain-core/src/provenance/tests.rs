use super::*;
use std::path::PathBuf;

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/provenance/testdata").join(path)
}

fn open(path: &str) -> Attestations {
    load(&fixture(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn provenance(e: &Entry) -> &Provenance {
    match &e.statement.predicate {
        Predicate::Provenance(p) => p,
        other => panic!("not provenance: {other:?}"),
    }
}

const REAL: [&str; 4] = [
    "slsa-verifier/envelope-v0.2-multi-subject.intoto.jsonl",
    "slsa-verifier/bundle-v0.3-generic-v0.2.intoto.jsonl",
    "slsa-verifier/bundle-v0.2-delegator-v1.build.slsa",
    "slsa-verifier/bundle-v0.3-github-v1.intoto.jsonl",
];

#[test]
fn every_real_fixture_loads_without_issues() {
    for f in REAL {
        let a = open(f);
        assert_eq!(a.entries.len(), 1, "{f}");
        assert!(a.issues.is_empty(), "{f}: {:?}", a.issues);
        assert!(!a.entries[0].statement.subjects.is_empty(), "{f}");
    }
}

#[test]
fn a_bare_envelope_with_v0_2_provenance() {
    let a = open("slsa-verifier/envelope-v0.2-multi-subject.intoto.jsonl");
    let e = &a.entries[0];
    assert_eq!(e.wrapping, Wrapping::Envelope);
    assert_eq!(e.signatures.len(), 1);
    assert_eq!(e.signatures[0].keyid, None, "an empty keyid is none");
    assert!(e.signatures[0].length > 60, "an ECDSA signature, decoded");
    assert!(e.signer.is_none() && e.log.is_empty());
    let names: Vec<_> = e.statement.subjects.iter().map(Resource::label).collect();
    assert_eq!(names, ["artifact1", "artifact2", "artifact3"]);
    let p = provenance(e);
    assert_eq!(p.version, SlsaVersion::V0_2);
    assert_eq!(p.build_type, "https://github.com/slsa-framework/slsa-github-generator/generic@v1");
    assert!(p.builder.id.ends_with("generator_generic_slsa3.yml@refs/heads/main"));
    assert_eq!(p.invocation.as_deref(), Some("4060917406-1"), "buildInvocationID, as the generator spells it");
    assert_eq!(p.completeness, Some(Completeness { parameters: true, environment: false, materials: false }));
    assert_eq!(p.reproducible, Some(false));
    // configSource comes first, as a dependency, with the workflow as its entry point.
    let cs = &p.dependencies[0];
    assert!(cs.config_source);
    assert_eq!(cs.uri.as_deref(), Some("git+https://github.com/slsa-framework/example-package@refs/heads/main"));
    assert_eq!(cs.digest.get("sha1").map(String::as_str), Some("60a179bd9181657528c7b14243f07511b4f63cf5"));
    assert_eq!(cs.entry_point.as_deref(), Some(".github/workflows/e2e.generic.schedule.main.multi-subjects.slsa3.yml"));
    assert_eq!(p.dependencies.len(), 2, "configSource and one material");
    assert!(p.internal.get("environment").is_some());
}

#[test]
fn bundles_with_a_certificate_or_a_chain_and_their_log_entries() {
    let a = open("slsa-verifier/bundle-v0.3-github-v1.intoto.jsonl");
    let e = &a.entries[0];
    assert_eq!(e.wrapping, Wrapping::Bundle { media_type: "application/vnd.dev.sigstore.bundle.v0.3+json".into() });

    assert_eq!(
        e.log,
        [LogEntry { index: Some(188622862), integrated_time: Some(1743032850), kind: Some("dsse".into()), has_proof: true, has_promise: true }]
    );
    let p = provenance(e);
    assert_eq!(p.version, SlsaVersion::V1);
    assert_eq!(p.build_type, "https://actions.github.io/buildtypes/workflow/v1");
    assert_eq!(p.external.pointer("/workflow/path").and_then(|v| v.as_str()), Some(".github/workflows/release.yml"));
    assert_eq!(p.dependencies[0].digest.get("gitCommit").map(String::as_str), Some("8f70009fde0c94ade6ce2a054b94718c819126ec"));
    assert_eq!(p.invocation.as_deref(), Some("https://github.com/aspect-build/rules_lint/actions/runs/14095611671/attempts/1"));

    let a = open("slsa-verifier/bundle-v0.2-delegator-v1.build.slsa");
    let e = &a.entries[0];
    assert_eq!(e.wrapping, Wrapping::Bundle { media_type: "application/vnd.dev.sigstore.bundle+json;version=0.2".into() });
    assert!(e.signer.is_some(), "from the first of x509CertificateChain");
    assert_eq!(e.log[0].kind.as_deref(), Some("intoto"));
    let p = provenance(e);
    assert_eq!(p.build_type, "https://github.com/slsa-framework/slsa-github-generator/delegator-generic@v0");
    assert_eq!(p.internal.get("GITHUB_SHA").and_then(|v| v.as_str()), Some("4d329c75e7ec1725f7c9ce917a8799d408d06be3"));

    let p = provenance(&open("slsa-verifier/bundle-v0.3-generic-v0.2.intoto.jsonl").entries[0]).clone();
    assert_eq!(p.version, SlsaVersion::V0_2, "a bundle carries v0.2 too");
}

#[test]
fn a_bare_v1_statement_with_versions_and_byproducts() {
    let a = open("made/statement-v1.json");
    let e = &a.entries[0];
    assert_eq!((e.line, &e.wrapping), (None, &Wrapping::Statement));
    assert_eq!(e.statement.subjects.len(), 3);
    let p = provenance(e);
    assert_eq!(p.builder.version.get("runner").map(String::as_str), Some("2.328.0"));
    assert_eq!(p.started.as_deref(), Some("2026-10-01T10:00:00Z"));
    assert_eq!(p.finished.as_deref(), Some("2026-10-01T10:17:00Z"));
    assert_eq!(p.byproducts[0].label(), "build.log");
    assert_eq!(p.dependencies[1].label(), "rust", "the name over the URI");
}

#[test]
fn other_predicates() {
    let e = &open("made/vsa.intoto.json").entries[0];
    match &e.statement.predicate {
        Predicate::Vsa(v) => {
            assert_eq!(v.result.as_deref(), Some("PASSED"));
            assert_eq!(v.levels, ["SLSA_BUILD_LEVEL_3"]);
            assert_eq!(v.verifier, "https://example.com/verifier");
            assert_eq!(v.policy.as_deref(), Some("https://example.com/policy/rocket"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(e.statement.subjects[0].label(), "https://example.com/rocket-1.4.0.tar.gz", "the URI when there is no name");
    match &open("made/sbom.provenance.json").entries[0].statement.predicate {
        Predicate::Bom { bom } => assert_eq!(bom["bomFormat"], "CycloneDX"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn json_lines_are_lenient() {
    let a = open("made/lenient.intoto.jsonl");
    let lines: Vec<_> = a.entries.iter().map(|e| e.line).collect();
    assert_eq!(lines, [Some(1), Some(3)]);
    assert!(matches!(a.entries[1].statement.predicate, Predicate::Other { .. }));
    let issues: Vec<_> = a.issues.iter().map(|i| (i.code, i.line)).collect();
    assert_eq!(
        issues,
        [
            (IssueCode::BadBase64, Some(2)),
            (IssueCode::SubjectWithoutDigest, Some(3)),
            (IssueCode::InvalidJson, Some(5)),
            (IssueCode::NotInToto, Some(6)),
        ]
    );
}

#[test]
fn what_is_refused() {
    assert!(matches!(parse("{not json"), Err(Error::InvalidJson { line: 1, .. })));
    assert!(matches!(parse("x\ny\n"), Err(Error::InvalidJson { .. })));
    assert_eq!(parse(r#"{"bomFormat": "CycloneDX"}"#), Err(Error::NotAttestation));
    assert_eq!(parse("{\"a\":1}\n{\"b\":2}\n"), Err(Error::NotAttestation));
    assert_eq!(parse(""), Err(Error::NotAttestation));
    let blob = r#"{"mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json", "messageSignature": {}}"#;
    assert_eq!(parse(blob), Err(Error::NotAttestation), "a signed blob, not an attestation");
}

#[test]
fn base64_standard_or_url_safe_with_or_without_padding() {
    for (s, bytes) in [("Pz8/", &b"???"[..]), ("Pz8_", b"???"), ("Pz8/\n", b"???"), ("Pz8=", b"??"), ("Pz8", b"??")] {
        assert_eq!(ingest::decode(s).as_deref(), Some(bytes), "{s}");
    }
    assert_eq!(ingest::decode("not base64!!"), None);
}

#[test]
fn v0_2_and_v1_map_to_the_same_fields() {
    let v02 = r#"{"_type": "https://in-toto.io/Statement/v0.1", "subject": [{"name": "a", "digest": {"sha256": "aa"}}],
      "predicateType": "https://slsa.dev/provenance/v0.2",
      "predicate": {"builder": {"id": "B"}, "buildType": "T",
        "invocation": {"configSource": {"uri": "git+https://h/r@refs/tags/v1", "digest": {"sha1": "c"}, "entryPoint": "w.yml"},
                       "parameters": {"p": 1}},
        "materials": [{"uri": "pkg:x", "digest": {"sha256": "d"}}],
        "metadata": {"buildStartedOn": "s", "buildFinishedOn": "f"}}}"#;
    let v1 = r#"{"_type": "https://in-toto.io/Statement/v1", "subject": [{"name": "a", "digest": {"sha256": "aa"}}],
      "predicateType": "https://slsa.dev/provenance/v1",
      "predicate": {"buildDefinition": {"buildType": "T", "externalParameters": {"p": 1},
          "resolvedDependencies": [{"uri": "git+https://h/r@refs/tags/v1", "digest": {"sha1": "c"}}, {"uri": "pkg:x", "digest": {"sha256": "d"}}]},
        "runDetails": {"builder": {"id": "B"}, "metadata": {"startedOn": "s", "finishedOn": "f"}}}}"#;
    let (a, b) = (parse(v02).unwrap(), parse(v1).unwrap());
    let (a, b) = (provenance(&a.entries[0]), provenance(&b.entries[0]));
    assert_eq!((&a.builder, &a.build_type, &a.external, &a.started, &a.finished), (&b.builder, &b.build_type, &b.external, &b.started, &b.finished));
    let uris = |p: &Provenance| p.dependencies.iter().map(|d| (d.uri.clone(), d.digest.clone())).collect::<Vec<_>>();
    assert_eq!(uris(a), uris(b));
}

#[test]
fn sniffing_by_name_and_by_content() {
    for n in ["x.intoto.jsonl", "X.INTOTO.JSON", "a.sigstore.json", "a.sigstore", "a.dsse.json", "rocket.provenance.json", "b.build.slsa", "provenance.json"] {
        assert!(sniff_name(n), "{n}");
    }
    for n in ["bom.json", "a.json", "intoto.txt", "provenance.json.bak"] {
        assert!(!sniff_name(n), "{n}");
    }
    assert!(sniff_head(br#"{"payloadType":"application/vnd.in-toto+json","payload":"#));
    assert!(sniff_head(br#"{"_type": "https://in-toto.io/Statement/v1", "subject""#));
    assert!(sniff_head(br#"{"mediaType":"application/vnd.dev.sigstore.bundle.v0.3+json""#));
    assert!(!sniff_head(br#"{"bomFormat": "CycloneDX", "specVersion": "1.6"}"#));
    assert!(!sniff_head(br#"{"payloadType": "text/plain"}"#));

    // By content, under a name that says nothing: gh attestation download's sha256:<hex>.jsonl.
    let dir = std::env::temp_dir().join(format!("coxswain-provenance-sniff-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("sha256-2c26b4.jsonl");
    std::fs::copy(fixture("slsa-verifier/bundle-v0.3-github-v1.intoto.jsonl"), &f).unwrap();
    assert!(sniff(&f));
    let bom = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/bom/testdata/cbomkit/flick.cdx.json");
    let plain = dir.join("flick.json");
    std::fs::copy(bom, &plain).unwrap();
    assert!(!sniff(&plain), "a BOM is not provenance");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_signer_from_a_fulcio_certificate() {
    let a = open("slsa-verifier/bundle-v0.3-github-v1.intoto.jsonl");
    let s = a.entries[0].signer.as_ref().expect("a signer");
    assert_eq!(s.identity, "https://github.com/bazel-contrib/publish-to-bcr/.github/workflows/publish.yaml@refs/tags/v0.0.1");
    assert_eq!(s.ca, "O=sigstore.dev, CN=sigstore-intermediate");
    assert_eq!((s.not_before, s.not_after), (1743032850, 1743032850 + 600), "ten minutes, from the log entry's time");
    // DER UTF8Strings (8 and on) read as text, and they win over the raw legacy ones (1–6).
    assert_eq!(s.get(Claim::Issuer), Some("https://token.actions.githubusercontent.com"));
    assert_eq!(s.get(Claim::SourceRepositoryUri), Some("https://github.com/aspect-build/rules_lint"));
    assert_eq!(s.get(Claim::SourceRepositoryDigest), Some("8f70009fde0c94ade6ce2a054b94718c819126ec"));
    assert_eq!(s.get(Claim::SourceRepositoryRef), Some("refs/heads/publish-to-bcr"));
    assert_eq!(s.get(Claim::BuildConfigUri), Some("https://github.com/aspect-build/rules_lint/.github/workflows/release.yml@refs/heads/publish-to-bcr"));
    assert_eq!(s.get(Claim::BuildTrigger), Some("workflow_dispatch"));
    assert_eq!(s.get(Claim::RunnerEnvironment), Some("github-hosted"));
    assert_eq!(s.get(Claim::SourceRepositoryVisibility), Some("public"));
    // Legacy only: nothing newer replaces them.
    assert_eq!(s.get(Claim::WorkflowName), Some("Release"));
    assert_eq!(s.get(Claim::Repository), Some("aspect-build/rules_lint"));
    assert_eq!(s.claims.len(), 17, "one per claim, sorted");
    assert!(s.claims.windows(2).all(|w| w[0].0 < w[1].0));
}

#[test]
fn a_bad_certificate_is_an_issue() {
    let good = std::fs::read_to_string(fixture("slsa-verifier/bundle-v0.3-github-v1.intoto.jsonl")).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&good).unwrap();
    v["verificationMaterial"]["certificate"]["rawBytes"] = "AAAA".into();
    let a = parse(&v.to_string()).unwrap();
    assert!(a.entries[0].signer.is_none());
    assert_eq!(a.issues.iter().map(|i| i.code).collect::<Vec<_>>(), [IssueCode::BadCertificate]);
}

mod checks {
    use super::super::check::{self, CannotCheck, GitSource, Source, Subject};
    use super::*;
    use std::sync::atomic::AtomicBool;

    const FOO_SHA256: &str = "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae";

    fn res(name: &str, digest: &[(&str, &str)]) -> Resource {
        Resource { name: Some(name.into()), digest: digest.iter().map(|(a, d)| (a.to_string(), d.to_string())).collect(), ..Resource::default() }
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-provenance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn subjects_against_the_files_here() {
        let d = dir("subjects");
        let (here, other) = (d.join("rel"), d.join("other"));
        std::fs::create_dir_all(here.join("dist")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(here.join("dist/rocket.tar.gz"), "foo").unwrap();
        std::fs::write(here.join("rocket.zip"), "bar").unwrap();
        std::fs::write(other.join("elsewhere.bin"), "foo").unwrap();
        std::fs::write(d.join("outside"), "foo").unwrap();
        let prov = here.join("rocket.intoto.jsonl");
        let no = AtomicBool::new(false);
        let at = |s: &Resource| check::subject(&prov, Some(&other), s, Some(check::AUTO_LIMIT), &no);

        // By its path below the provenance's folder.
        assert_eq!(at(&res("dist/rocket.tar.gz", &[("sha256", FOO_SHA256)])), Subject::Matches { path: here.join("dist/rocket.tar.gz"), algorithm: "sha256".into() });
        // By its last part, in upper case hex.
        assert!(matches!(at(&res("build/out/rocket.zip", &[("sha256", &FOO_SHA256.to_uppercase())])), Subject::Differs { ref actual, .. } if actual.starts_with("fcde2b2e")));
        // sha512 when there is no sha256.
        let foo512 = "f7fbba6e0636f890e56fbbf3283e524c6fa3204ae298382d624741d0dc6638326e282c41be5e4254d8820772c5518a2c5a8c0c7f7eda19594a7eb539453e1ed7";
        assert!(matches!(at(&res("dist/rocket.tar.gz", &[("sha512", foo512)])), Subject::Matches { .. }));
        // In the other pane's folder.
        assert!(matches!(at(&res("elsewhere.bin", &[("sha256", FOO_SHA256)])), Subject::Matches { ref path, .. } if *path == other.join("elsewhere.bin")));
        // By a URL's last part.
        let by_uri = Resource { uri: Some("https://example.com/dl/elsewhere.bin?x=1".into()), ..res("", &[("sha256", FOO_SHA256)]) };
        assert!(matches!(at(&Resource { name: None, ..by_uri }), Subject::Matches { .. }));
        // Never out of the folders.
        assert_eq!(at(&res("../outside", &[("sha256", FOO_SHA256)])), Subject::Missing);
        assert_eq!(at(&res("gone.tar.gz", &[("sha256", FOO_SHA256)])), Subject::Missing);
        // What cannot be checked.
        assert_eq!(at(&res("ghcr.io/demo/rocket", &[("sha256", "00")])), Subject::CannotCheck { why: CannotCheck::Image });
        assert_eq!(at(&res("gone.tar.gz", &[])), Subject::CannotCheck { why: CannotCheck::NoDigest });
        assert_eq!(at(&res("rocket.zip", &[("sha1", "aa"), ("md5", "bb")])), Subject::CannotCheck { why: CannotCheck::Algorithms(vec!["md5".into(), "sha1".into()]) });
        // Large ones wait, and a cancelled hash says so.
        assert_eq!(check::subject(&prov, None, &res("rocket.zip", &[("sha256", "aa")]), Some(2), &no), Subject::Large { path: here.join("rocket.zip"), size: 3 });
        assert!(matches!(check::subject(&prov, None, &res("rocket.zip", &[("sha256", "aa")]), None, &AtomicBool::new(true)), Subject::Unreadable { .. }));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn images_are_told_from_files() {
        for n in ["ghcr.io/demo/rocket", "docker.io/library/rust", "localhost/app", "registry:5000/app", "pkg:docker/rust@1", "oci://x/y"] {
            assert!(check::is_image(&res(n, &[])), "{n}");
        }
        for n in ["dist/rocket.tar.gz", "rocket.tar.gz", "https://example.com/a.tar.gz", "../x/y", "./dist/a"] {
            assert!(!check::is_image(&res(n, &[])), "{n}");
        }
    }

    #[test]
    fn git_sources_and_remotes() {
        let r = Resource { uri: Some("git+https://github.com/Demo/Rocket.git@refs/tags/v1.4.0".into()), ..res("", &[("gitCommit", "ABC1234")]) };
        assert_eq!(
            check::git_source(&Resource { name: None, ..r }),
            Some(GitSource { repo: "github.com/demo/rocket".into(), reference: Some("refs/tags/v1.4.0".into()), commit: "abc1234".into() })
        );
        let v02 = Resource { uri: Some("git+ssh://git@gitlab.com/g/p@main".into()), ..res("", &[("sha1", "abc1234")]) };
        assert_eq!(check::git_source(&v02).map(|s| s.repo), Some("gitlab.com/g/p".into()));
        assert_eq!(check::git_source(&Resource { uri: Some("pkg:docker/rust".into()), ..res("", &[("sha256", "a")]) }), None);
        for url in ["https://github.com/demo/rocket.git", "git@github.com:demo/rocket.git", "ssh://git@github.com/demo/rocket", "https://user@GitHub.com/demo/rocket/"] {
            assert_eq!(check::remote_key(url).as_deref(), Some("github.com/demo/rocket"), "{url}");
        }
    }

    #[test]
    fn the_source_commit_in_a_checkout_here() {
        use crate::history::tests::{add, commit_as, repo};
        let Some(d) = repo("provenance-source") else { return };
        let g = |args: &[&str]| assert!(crate::tools::command("git").arg("-C").arg(&d).args(args).status().unwrap().success());
        let head = || String::from_utf8(crate::tools::command("git").arg("-C").arg(&d).args(["rev-parse", "HEAD"]).output().unwrap().stdout).unwrap().trim().to_string();
        g(&["remote", "add", "origin", "git@github.com:demo/rocket.git"]);
        std::fs::create_dir_all(d.join("dist")).unwrap();
        let mut commits = vec![];
        for i in 0..3 {
            std::fs::write(d.join("f"), i.to_string()).unwrap();
            add(&d);
            commit_as(&d, "t", 1_700_000_000 + i, "c");
            commits.push(head());
        }
        g(&["checkout", "-q", "-b", "side", &commits[0]]);
        std::fs::write(d.join("f"), "side").unwrap();
        commit_as(&d, "t", 1_700_000_100, "side");
        let side = head();
        g(&["checkout", "-q", "main"]);

        let prov = d.join("dist/rocket.intoto.jsonl");
        let src = |c: &str| GitSource { repo: "github.com/demo/rocket".into(), reference: None, commit: c.into() };
        let at = |c: &str| check::source(&prov, None, &src(c));
        let top = PathBuf::from(String::from_utf8(crate::tools::command("git").arg("-C").arg(&d).args(["rev-parse", "--show-toplevel"]).output().unwrap().stdout).unwrap().trim());
        assert_eq!(at(&commits[2]), Source::OnBranch { checkout: top.clone(), behind: 0 });
        assert_eq!(at(&commits[0]), Source::OnBranch { checkout: top.clone(), behind: 2 });
        assert_eq!(at(&side), Source::Elsewhere { checkout: top.clone() });
        assert_eq!(at("0123456789abcdef0123456789abcdef01234567"), Source::Missing { checkout: top.clone() });
        assert_eq!(at("--output=x"), Source::Missing { checkout: top.clone() }, "never an option");
        g(&["checkout", "-q", &commits[0]]);
        assert_eq!(at(&commits[2]), Source::Ahead { checkout: top.clone(), ahead: 2 });
        let other = GitSource { repo: "github.com/someone/else".into(), ..src(&commits[0]) };
        assert_eq!(check::source(&prov, None, &other), Source::NoCheckout);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
