#!/usr/bin/env bash
# Build and install Bosum from source on Linux or macOS.
#
#   ./install/install.sh             terminal app and desktop app
#   ./install/install.sh --tui-only  terminal app only (needs only Rust)
#   ./install/install.sh --yes       answer yes to every question
#
# Installs into $PREFIX (default ~/.local): bin/bosum, bin/bosum-gui, plus a desktop entry
# on Linux. On macOS the desktop app goes to ~/Applications/Bosum.app.
set -euo pipefail

cd "$(dirname "$0")/.."
PREFIX="${PREFIX:-$HOME/.local}"
TUI_ONLY=0
YES=0
for arg in "$@"; do
  case "$arg" in
    --tui-only) TUI_ONLY=1 ;;
    --yes | -y) YES=1 ;;
    -h | --help) sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $arg (try --help)" >&2; exit 2 ;;
  esac
done

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '  %s\n' "$*"; }
fail() { printf '\033[31mError:\033[0m %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# Ask politely; Enter means yes.
ask() {
  [ "$YES" = 1 ] && return 0
  local reply
  read -r -p "  $1 [Y/n] " reply </dev/tty || return 1
  [[ -z "$reply" || "$reply" =~ ^[Yy] ]]
}

sudo_cmd() { if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi; }

OS="$(uname -s)"
bold "Installing Bosum ($OS)"

# ---------------------------------------------------------------- Rust

if ! have cargo && [ -x "$HOME/.cargo/bin/cargo" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi
if ! have cargo; then
  bold "Rust is needed to build Bosum, and it isn't installed yet."
  info "The official installer (rustup, https://rustup.rs) puts it in ~/.cargo and ~/.rustup,"
  info "and touches nothing else. You can remove it later with 'rustup self uninstall'."
  if ask "Would you like me to install Rust now?"; then
    have curl || fail "curl is needed to download Rust. Please install curl and run this again."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    export PATH="$HOME/.cargo/bin:$PATH"
  else
    fail "No problem. Install Rust from https://rustup.rs when you're ready, then run this again."
  fi
fi
# Bosum uses the 2024 edition, which needs Rust 1.85 or newer.
RUST_MINOR="$(rustc --version | awk '{split($2, v, "."); print v[2]}')"
if [ "${RUST_MINOR:-0}" -lt 85 ]; then
  bold "Your Rust ($(rustc --version)) is older than Bosum needs (1.85)."
  if have rustup && ask "Would you like me to update it with 'rustup update stable'?"; then
    rustup update stable
  else
    fail "Please update Rust (rustup update) and run this again."
  fi
fi
info "Rust: $(rustc --version)"

# ---------------------------------------------------------------- desktop app prerequisites

if [ "$TUI_ONLY" = 0 ]; then
  if [ "$OS" = Linux ] && ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
    bold "The desktop app needs a few system libraries (WebKitGTK and friends)."
    if have apt-get; then
      PKGS=(apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config)
      PRE=(apt-get update)
    elif have dnf; then
      PKGS=(dnf install -y webkit2gtk4.1-devel openssl-devel curl wget file libappindicator-gtk3-devel librsvg2-devel gcc gcc-c++ make pkgconf-pkg-config)
      PRE=(true)
    elif have pacman; then
      PKGS=(pacman -S --needed --noconfirm webkit2gtk-4.1 base-devel curl wget file openssl libappindicator-gtk3 librsvg pkgconf)
      PRE=(true)
    elif have zypper; then
      PKGS=(zypper install -y webkit2gtk3-soup2-devel libopenssl-devel curl wget file libappindicator3-1 librsvg-devel gcc gcc-c++ make pkg-config)
      PRE=(true)
    else
      PKGS=()
    fi
    if [ ${#PKGS[@]} -gt 0 ]; then
      info "I would run: sudo ${PKGS[*]}"
      if ask "May I install them (this asks for your password)?"; then
        sudo_cmd "${PRE[@]}"
        sudo_cmd "${PKGS[@]}"
      else
        info "Skipping the desktop app. You can still use the terminal app."
        TUI_ONLY=1
      fi
    else
      info "I don't know this distribution's package manager. See https://v2.tauri.app/start/prerequisites/"
      info "Skipping the desktop app for now."
      TUI_ONLY=1
    fi
  fi
  if [ "$OS" = Darwin ] && ! xcode-select -p >/dev/null 2>&1; then
    bold "The desktop app needs Apple's command line tools."
    if ask "Would you like me to start their installer?"; then
      xcode-select --install || true
      fail "Finish the Command Line Tools installation, then run this script again."
    fi
    TUI_ONLY=1
  fi
fi

if [ "$TUI_ONLY" = 0 ] && ! have npm; then
  bold "The desktop app's interface is built with Node.js, which isn't installed."
  if [ "$OS" = Darwin ] && have brew && ask "Would you like me to install it with Homebrew?"; then
    brew install node
  elif have apt-get && ask "Would you like me to install it with apt (asks for your password)?"; then
    sudo_cmd apt-get install -y nodejs npm
  elif have dnf && ask "Would you like me to install it with dnf (asks for your password)?"; then
    sudo_cmd dnf install -y nodejs npm
  elif have pacman && ask "Would you like me to install it with pacman (asks for your password)?"; then
    sudo_cmd pacman -S --needed --noconfirm nodejs npm
  fi
  if ! have npm; then
    info "No worries: install Node.js 20 or newer from https://nodejs.org and run this again"
    info "for the desktop app. Continuing with the terminal app."
    TUI_ONLY=1
  fi
fi

# ---------------------------------------------------------------- build

bold "Building the terminal app (a few minutes the first time)…"
cargo build --release --locked -p bosum
mkdir -p "$PREFIX/bin"
install -m 755 target/release/bosum "$PREFIX/bin/bosum"
info "Installed $PREFIX/bin/bosum"

if [ "$TUI_ONLY" = 0 ]; then
  bold "Building the desktop app…"
  (cd gui && npm ci --no-audit --no-fund)
  if [ "$OS" = Darwin ]; then
    (cd gui && npx tauri build --bundles app)
    mkdir -p "$HOME/Applications"
    rm -rf "$HOME/Applications/Bosum.app"
    cp -R target/release/bundle/macos/Bosum.app "$HOME/Applications/"
    ln -sf "$HOME/Applications/Bosum.app/Contents/MacOS/bosum-gui" "$PREFIX/bin/bosum-gui"
    info "Installed ~/Applications/Bosum.app"
  else
    (cd gui && npx tauri build --no-bundle)
    install -m 755 target/release/bosum-gui "$PREFIX/bin/bosum-gui"
    ICONS="$PREFIX/share/icons/hicolor/128x128/apps"
    APPS="$PREFIX/share/applications"
    mkdir -p "$ICONS" "$APPS"
    install -m 644 gui/src-tauri/icons/128x128.png "$ICONS/bosum.png"
    cat >"$APPS/bosum.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Bosum
Comment=File manager: two panes, git status, instant search
Exec=$PREFIX/bin/bosum-gui %F
Icon=bosum
Terminal=false
Categories=System;FileTools;FileManager;
EOF
    info "Installed $PREFIX/bin/bosum-gui and a menu entry"
  fi
fi

echo
bold "Done."
case ":$PATH:" in
  *":$PREFIX/bin:"*) ;;
  *) info "Add $PREFIX/bin to your PATH to run 'bosum' from anywhere, e.g.:"
     info "  echo 'export PATH=\"$PREFIX/bin:\$PATH\"' >> ~/.bashrc" ;;
esac
info "Terminal app: bosum        Desktop app: bosum-gui (or Bosum in your app menu)"
info "Git glyphs look best with a Nerd Font: https://www.nerdfonts.com"
