# The terminal app, `coxswain`, built from this checkout. The flake at the repository root
# calls this; packaging/nix/SUBMIT.md has the nixpkgs version, which fetches a release instead.
{
  lib,
  rustPlatform,
  installShellFiles,
  pkg-config,
  oniguruma,
  git,
  writableTmpDirAsHomeHook,
}:

rustPlatform.buildRustPackage {
  pname = "coxswain";
  version = (lib.importTOML ../../Cargo.toml).workspace.package.version;
  src = lib.cleanSource ../..;

  # The git crates of `[patch.crates-io]` (tao, drag) are fetched by their pinned revision:
  # no hashes to keep up to date in the flake.
  cargoLock = {
    lockFile = ../../Cargo.lock;
    allowBuiltinFetchGit = true;
  };

  cargoBuildFlags = [ "-p" "coxswain" ];
  cargoTestFlags = [ "-p" "coxswain" "-p" "coxswain-core" ];

  nativeBuildInputs = [ installShellFiles pkg-config ];
  # The tokenizer for search by meaning links oniguruma: the system's, not a copy built from C.
  buildInputs = [ oniguruma ];
  env.RUSTONIG_SYSTEM_LIBONIG = true;

  # The history and branch tests make small repositories with git; the config tests want a home.
  nativeCheckInputs = [ git writableTmpDirAsHomeHook ];

  postInstall = ''
    ln -s coxswain $out/bin/cox
    installManPage crates/coxswain/coxswain.1
  '';

  meta = {
    description = "Two-panel file manager for the terminal, Norton Commander style, with git status and search";
    homepage = "https://github.com/mwo-dk/coxswain";
    changelog = "https://github.com/mwo-dk/coxswain/blob/master/README.md#changelog";
    license = lib.licenses.mit;
    mainProgram = "coxswain";
    platforms = lib.platforms.unix;
  };
}
