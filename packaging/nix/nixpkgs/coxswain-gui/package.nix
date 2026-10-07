{
  lib,
  rustPlatform,
  fetchFromGitHub,
  fetchNpmDeps,
  npmHooks,
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

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "coxswain-gui";
  version = "2.8.1";

  src = fetchFromGitHub {
    owner = "mwo-dk";
    repo = "coxswain";
    tag = "v${finalAttrs.version}";
    hash = "sha256-4lmHzovV/FbjLSEZf2tDJozdkwDhjsZd4rnbXSSbhIw=";
  };

  cargoHash = lib.fakeHash;
  cargoBuildFlags = [ "-p" "coxswain-gui" ];

  npmRoot = "gui";
  # SheetJS is a tarball in gui/vendor (`file:` in the lock): npm adds it to the cache.
  makeCacheWritable = true;
  npmDeps = fetchNpmDeps {
    src = "${finalAttrs.src}/gui";
    hash = lib.fakeHash;
  };

  nativeBuildInputs = [
    nodejs
    npmHooks.npmConfigHook
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

  # The frontend first: `cargo build` embeds gui/dist.
  preBuild = ''
    (cd gui && npm run build)
  '';

  # The core's tests run in the coxswain package.
  doCheck = false;

  desktopItems = [
    (makeDesktopItem {
      name = "coxswain";
      desktopName = "Coxswain";
      comment = "File manager: two panes, git status, instant search";
      exec = "coxswain-gui %F";
      icon = "coxswain";
      categories = [
        "System"
        "FileTools"
        "FileManager"
      ];
    })
  ];

  postInstall = ''
    install -Dm644 gui/src-tauri/icons/128x128.png $out/share/icons/hicolor/128x128/apps/coxswain.png
  '';

  meta = {
    description = "Two-pane file manager in the Norton Commander tradition, with git status, previews and search (desktop app)";
    homepage = "https://github.com/mwo-dk/coxswain";
    changelog = "https://github.com/mwo-dk/coxswain/releases/tag/v${finalAttrs.version}";
    license = lib.licenses.mit;
    maintainers = with lib.maintainers; [ ]; # [ <NIXPKGS-HANDLE> ]
    mainProgram = "coxswain-gui";
    platforms = lib.platforms.linux;
  };
})
