//! Finds the README whose changelog `notices::changes` reads: the workspace's in the
//! repository, the copy cargo puts next to Cargo.toml in a published crate.
fn main() {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let readme = [dir.join("README.md"), dir.join("../../README.md")].into_iter().find(|p| p.exists()).expect("README.md with the changelog");
    println!("cargo:rerun-if-changed={}", readme.display());
    println!("cargo:rustc-env=COXSWAIN_README={}", readme.display());
}
