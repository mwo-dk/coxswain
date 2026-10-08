//! Finds the changelog `notices::changes` reads: the full one in `docs/changelog.md` in the
//! repository, or, in a published crate (which has only the README next to Cargo.toml), the
//! README's last ten versions. Its links are relative to the folder it is in.
fn main() {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let (file, base) = [(dir.join("../../docs/changelog.md"), "docs/"), (dir.join("README.md"), ""), (dir.join("../../README.md"), "")]
        .into_iter()
        .find(|(p, _)| p.exists())
        .expect("a changelog: docs/changelog.md or README.md");
    println!("cargo:rerun-if-changed={}", file.display());
    println!("cargo:rustc-env=COXSWAIN_CHANGELOG={}", file.display());
    println!("cargo:rustc-env=COXSWAIN_CHANGELOG_BASE={base}");
}
