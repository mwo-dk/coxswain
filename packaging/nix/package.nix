# The terminal app, `coxswain`, built from this checkout. The flake at the repository root
# calls this; packaging/nix/SUBMIT.md has the nixpkgs version, which fetches a release instead.
# Built with crane in two steps: the dependencies alone (`cargoArtifacts`, which changes only
# with Cargo.lock, a Cargo.toml or nixpkgs), then the workspace's own crates on top, so CI keeps
# the first between runs (.github/workflows/nix.yml).
{
  lib,
  craneLib,
  installShellFiles,
  pkg-config,
  oniguruma,
  git,
  writableTmpDirAsHomeHook,
}:

let
  # The git crates of `[patch.crates-io]` (tao) are fetched by their pinned revision:
  # no hashes to keep up to date in the flake.
  common = {
    pname = "coxswain";
    version = (lib.importTOML ../../Cargo.toml).workspace.package.version;
    src = lib.cleanSource ../..;
    strictDeps = true;
    cargoExtraArgs = "--locked -p coxswain";
    cargoTestExtraArgs = "-p coxswain-core";
    nativeBuildInputs = [ pkg-config ];
    # The tokenizer for search by meaning links oniguruma: the system's, not a copy built from C.
    buildInputs = [ oniguruma ];
    env.RUSTONIG_SYSTEM_LIBONIG = true;
  };
  # What the build and the tests compile of the dependencies, nothing checked.
  cargoArtifacts = craneLib.buildDepsOnly (common // { cargoCheckCommand = "true"; });
in
craneLib.buildPackage (
  common
  // {
    inherit cargoArtifacts;
    nativeBuildInputs = common.nativeBuildInputs ++ [ installShellFiles ];

    # The history and branch tests make small repositories with git; the config tests want a home.
    nativeCheckInputs = [ git writableTmpDirAsHomeHook ];

    postInstall = ''
      installManPage crates/coxswain/coxswain.1
    '';

    passthru = { inherit cargoArtifacts; };

    meta = {
      description = "Two-panel file manager for the terminal, Norton Commander style, with git status and search";
      homepage = "https://github.com/mwo-dk/coxswain";
      changelog = "https://github.com/mwo-dk/coxswain/blob/master/README.md#changelog";
      license = lib.licenses.mit;
      mainProgram = "coxswain";
      platforms = lib.platforms.unix;
    };
  }
)
