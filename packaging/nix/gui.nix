# The desktop app, `coxswain-gui` (Tauri and WebKitGTK), built from this checkout. Linux only.
# The frontend is built first; `cargo build` then embeds gui/dist, as in a local build.
# Built with crane in two steps, like package.nix: the dependencies alone, then the workspace.
{
  lib,
  craneLib,
  importNpmLock,
  nodejs,
  pkg-config,
  wrapGAppsHook3,
  makeDesktopItem,
  copyDesktopItems,
  oniguruma,
  openssl,
  webkitgtk_4_1,
  librsvg,
  glib-networking,
  libayatana-appindicator,
  gst_all_1,
}:

let
  common = {
    pname = "coxswain-gui";
    version = (lib.importTOML ../../Cargo.toml).workspace.package.version;
    src = lib.cleanSource ../..;
    strictDeps = true;
    cargoExtraArgs = "--locked -p coxswain-gui";
    # The core's tests run in the terminal app's package.
    doCheck = false;
    # wrapGAppsHook3 in both steps: it adds GTK's paths to PKG_CONFIG_PATH, and a change there
    # would run the -sys crates' build scripts again, and all built on them.
    nativeBuildInputs = [ pkg-config wrapGAppsHook3 ];
    buildInputs = [
      oniguruma
      openssl
      webkitgtk_4_1
      librsvg
      glib-networking
      libayatana-appindicator
      # Video and sound in the preview pane.
      gst_all_1.gstreamer
      gst_all_1.gst-plugins-base
      gst_all_1.gst-plugins-good
    ];
    env.RUSTONIG_SYSTEM_LIBONIG = true;
  };
  cargoArtifacts = craneLib.buildDepsOnly (
    removeAttrs common [ "src" ]
    // {
      # The workspace's crates stubbed out, but not vendor/candle-core: a dependency of theirs
      # (`[patch.crates-io]` in Cargo.toml), built for real.
      dummySrc = craneLib.mkDummySrc {
        inherit (common) src;
        extraDummyScript = ''
          rm -rf $out/vendor/candle-core
          cp -r --no-preserve=mode ${../../vendor/candle-core} $out/vendor/candle-core
        '';
      };
      cargoCheckCommand = "true";
    }
  );
in
craneLib.buildPackage (
  common
  // {
    inherit cargoArtifacts;

    # npm's packages from gui/package-lock.json, without a hash to keep up to date.
    npmRoot = "gui";
    npmDeps = importNpmLock {
      npmRoot = ../../gui;
      # SheetJS comes from a tarball in gui/vendor (`file:` in the lock), which importNpmLock
      # cannot resolve by itself.
      packageSourceOverrides."node_modules/xlsx" = ../../gui/vendor/xlsx-0.20.3.tgz;
    };

    nativeBuildInputs = common.nativeBuildInputs ++ [
      nodejs
      importNpmLock.npmConfigHook
      copyDesktopItems
    ];

    preBuild = ''
      (cd gui && npm run build)
    '';

    desktopItems = [
      (makeDesktopItem {
        name = "coxswain";
        desktopName = "Coxswain";
        comment = "File manager: two panes, git status, instant search";
        exec = "coxswain-gui %F";
        icon = "coxswain";
        categories = [ "System" "FileTools" "FileManager" ];
      })
    ];

    postInstall = ''
      install -Dm644 gui/src-tauri/icons/128x128.png $out/share/icons/hicolor/128x128/apps/coxswain.png
    '';

    passthru = { inherit cargoArtifacts; };

    meta = {
      description = "Two-pane file manager in the Norton Commander tradition, with git status, previews and search (desktop app)";
      homepage = "https://github.com/mwo-dk/coxswain";
      changelog = "https://github.com/mwo-dk/coxswain/blob/master/README.md#changelog";
      license = lib.licenses.mit;
      mainProgram = "coxswain-gui";
      platforms = lib.platforms.linux;
    };
  }
)
