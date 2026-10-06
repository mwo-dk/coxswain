{
  lib,
  rustPlatform,
  fetchFromGitHub,
  installShellFiles,
  pkg-config,
  oniguruma,
  git,
  writableTmpDirAsHomeHook,
  versionCheckHook,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "coxswain";
  version = "2.1.0";

  src = fetchFromGitHub {
    owner = "mwo-dk";
    repo = "coxswain";
    tag = "v${finalAttrs.version}";
    hash = lib.fakeHash;
  };

  cargoHash = lib.fakeHash;

  cargoBuildFlags = [ "-p" "coxswain" ];
  cargoTestFlags = [ "-p" "coxswain" "-p" "coxswain-core" ];

  nativeBuildInputs = [
    installShellFiles
    pkg-config
  ];
  buildInputs = [ oniguruma ];
  env.RUSTONIG_SYSTEM_LIBONIG = true;

  nativeCheckInputs = [
    git
    writableTmpDirAsHomeHook
  ];

  # Reads the time of a source file, which Nix sets to 1970, before the first time a zip can
  # hold; fixed upstream after 2.1.1 (the test uses the clock), drop this then.
  checkFlags = [ "--skip=archive::tests::archive_zip_times_are_local" ];

  postInstall = ''
    ln -s coxswain $out/bin/cox
    installManPage crates/coxswain/coxswain.1
  '';

  nativeInstallCheckInputs = [ versionCheckHook ];
  doInstallCheck = true;

  meta = {
    description = "Two-panel file manager for the terminal, Norton Commander style, with git status and search";
    homepage = "https://github.com/mwo-dk/coxswain";
    changelog = "https://github.com/mwo-dk/coxswain/releases/tag/v${finalAttrs.version}";
    license = lib.licenses.mit;
    maintainers = with lib.maintainers; [ ]; # [ <NIXPKGS-HANDLE> ]
    mainProgram = "coxswain";
    platforms = lib.platforms.unix;
  };
})
