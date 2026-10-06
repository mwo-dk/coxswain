# The desktop app, `coxswain-gui` (Tauri and WebKitGTK), built from this checkout. Linux only.
# The frontend is built first; `cargo build` then embeds gui/dist, as in a local build.
{
  lib,
  rustPlatform,
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

rustPlatform.buildRustPackage {
  pname = "coxswain-gui";
  version = (lib.importTOML ../../Cargo.toml).workspace.package.version;
  src = lib.cleanSource ../..;

  cargoLock = {
    lockFile = ../../Cargo.lock;
    allowBuiltinFetchGit = true;
  };
  cargoBuildFlags = [ "-p" "coxswain-gui" ];

  # npm's packages from gui/package-lock.json, without a hash to keep up to date.
  npmRoot = "gui";
  npmDeps = importNpmLock { npmRoot = ../../gui; };

  nativeBuildInputs = [
    nodejs
    importNpmLock.npmConfigHook
    pkg-config
    wrapGAppsHook3
    copyDesktopItems
  ];
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

  preBuild = ''
    (cd gui && npm run build)
  '';

  # The core's tests run in the terminal app's package.
  doCheck = false;

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

  meta = {
    description = "Two-pane file manager in the Norton Commander tradition, with git status, previews and search (desktop app)";
    homepage = "https://github.com/mwo-dk/coxswain";
    changelog = "https://github.com/mwo-dk/coxswain/blob/master/README.md#changelog";
    license = lib.licenses.mit;
    mainProgram = "coxswain-gui";
    platforms = lib.platforms.linux;
  };
}
