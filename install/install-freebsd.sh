#!/bin/sh
# Install or update Coxswain on FreeBSD from the latest release.
#
#   fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh
#
#   sh install-freebsd.sh [options]
#     --terminal-only  the terminal app only (it needs nothing beyond the base system)
#     --prefix DIR     install into DIR (default: where Coxswain is already, else asks:
#                      /usr/local for everyone, or ~/.local for you alone)
#     --version vX.Y.Z that release instead of the latest
#     --from DIR       install from release archives already in DIR (no download)
#     --yes            answer yes to every question
#     --uninstall      remove what this script installed
#     --help           this text
#
# The archives are checked against their SHA-256 sums before anything is installed. Packages
# are installed with pkg(8) only after you have seen the command and said yes; root rights
# come from doas(1) or sudo(8) when you are not root.
set -eu

REPO=mwo-dk/coxswain
TERMINAL_ONLY=0
YES=0
PREFIX=
VERSION=
FROM=
UNINSTALL=0

usage() {
  if [ -f "$0" ]; then sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'
  else echo "See https://github.com/mwo-dk/coxswain/blob/master/docs/reference/freebsd.md"; fi
}

while [ $# -gt 0 ]; do
  case "$1" in
    --terminal-only) TERMINAL_ONLY=1 ;;
    --prefix) PREFIX="${2:?--prefix needs a folder}"; shift ;;
    --prefix=*) PREFIX="${1#*=}" ;;
    --version) VERSION="${2:?--version needs a version}"; shift ;;
    --version=*) VERSION="${1#*=}" ;;
    --from) FROM="${2:?--from needs a folder}"; shift ;;
    --from=*) FROM="${1#*=}" ;;
    --yes | -y) YES=1 ;;
    --uninstall) UNINSTALL=1 ;;
    -h | --help) usage; exit 0 ;;
    *) echo "Unknown option: $1 (try --help)" >&2; exit 2 ;;
  esac
  shift
done

if [ -t 1 ]; then B=$(printf '\033[1m'); R=$(printf '\033[31m'); N=$(printf '\033[0m'); else B='' R='' N=''; fi
bold() { printf '%s%s%s\n' "$B" "$*" "$N"; }
info() { printf '  %s\n' "$*"; }
fail() { printf '%sError:%s %s\n' "$R" "$N" "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# A question, answered from the terminal even when this script comes through a pipe. Enter
# means yes; without a terminal to ask, the answer is no.
ask() {
  [ "$YES" = 1 ] && return 0
  [ -r /dev/tty ] || return 1
  printf '  %s [Y/n] ' "$1" >/dev/tty
  read -r reply </dev/tty || return 1
  case "$reply" in '' | [Yy]*) return 0 ;; *) return 1 ;; esac
}

# Run a command as root: directly, or through doas or sudo.
as_root() {
  if [ "$(id -u)" = 0 ]; then "$@"
  elif have doas; then doas "$@"
  elif have sudo; then sudo "$@"
  else fail "This needs root rights, and neither doas nor sudo is installed. Run it as root: $*"
  fi
}

[ "$(uname -s)" = FreeBSD ] || fail "This script is for FreeBSD. On Linux and macOS see install/INSTALL.md."

# ---------------------------------------------------------------- where

if [ -z "$PREFIX" ]; then
  if [ -x "$HOME/.local/bin/coxswain" ]; then PREFIX="$HOME/.local"
  elif [ -x /usr/local/bin/coxswain ]; then PREFIX=/usr/local
  elif [ "$(id -u)" = 0 ]; then PREFIX=/usr/local
  elif [ "$UNINSTALL" = 0 ] && { have doas || have sudo; } && ask "Install for every user in /usr/local (asks for root rights)? No puts it in ~/.local for you alone."; then PREFIX=/usr/local
  else PREFIX="$HOME/.local"
  fi
fi

# Write into the prefix: as root when it is not ours.
put() {
  if [ -w "$PREFIX" ] || { [ ! -e "$PREFIX" ] && [ "$PREFIX" != /usr/local ]; }; then "$@"
  else as_root "$@" || fail "Could not write to $PREFIX as root. Run this as root, or install for yourself alone: --prefix ~/.local"
  fi
}

if [ "$UNINSTALL" = 1 ]; then
  bold "Removing Coxswain from $PREFIX"
  put rm -f "$PREFIX/bin/coxswain" "$PREFIX/bin/cox" "$PREFIX/bin/coxswain-gui" \
    "$PREFIX/share/man/man1/coxswain.1.gz" "$PREFIX/share/man/man1/cox.1.gz" \
    "$PREFIX/share/applications/coxswain.desktop" "$PREFIX/share/icons/hicolor/128x128/apps/coxswain.png"
  if [ "$PREFIX" = /usr/local ] && [ -f /usr/local/etc/rc.d/coxswain_index ]; then
    as_root service coxswain_index onestop 2>/dev/null || true
    as_root rm -f /usr/local/etc/rc.d/coxswain_index
    info "If rc.conf turns the helper on, remove those lines too: sysrc -x coxswain_index_enable coxswain_index_user"
  fi
  rm -f "$HOME/.config/autostart/coxswain-index.desktop"
  info "Your settings and the search store stay: ~/.config/coxswain and ~/.cache/coxswain."
  info "Remove them too with: rm -rf ~/.config/coxswain ~/.cache/coxswain"
  exit 0
fi

# ---------------------------------------------------------------- which release

case "$(uname -m)" in
  amd64 | x86_64) TARGET=x86_64-unknown-freebsd ;;
  *)
    bold "There is no ready-made build for $(uname -m) yet."
    info "The terminal app builds from source with Rust: pkg install rust, then"
    info "  cargo install --locked coxswain"
    exit 1
    ;;
esac

TMP=$(mktemp -d -t coxswain)
trap 'rm -rf "$TMP"' EXIT INT TERM

if [ -z "$VERSION" ] && [ -z "$FROM" ]; then
  VERSION=$(fetch -qo - "https://api.github.com/repos/$REPO/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
  [ -n "$VERSION" ] || fail "Could not ask GitHub for the latest release. Is the network up? Or name one: --version v1.43.0"
fi
if [ -z "$VERSION" ]; then
  # shellcheck disable=SC2012
  VERSION=$(ls "$FROM" | sed -n "s/^coxswain-terminal-\(v[0-9.]*\)-$TARGET\.tar\.gz$/\1/p" | sort -t. -k1,1 -k2,2n -k3,3n | tail -n 1)
  [ -n "$VERSION" ] || fail "No coxswain-terminal-v…-$TARGET.tar.gz in $FROM."
fi
case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac

if [ -x "$PREFIX/bin/coxswain" ]; then
  NOW=$("$PREFIX/bin/coxswain" --version 2>/dev/null | awk '{print $NF}')
  bold "Updating Coxswain ${NOW:+v$NOW }to $VERSION in $PREFIX"
else
  bold "Installing Coxswain $VERSION into $PREFIX"
fi

# Fetch an archive and its sum, and check the one against the other.
get() {
  name="$1.tar.gz"
  if [ -n "$FROM" ]; then
    cp "$FROM/$name" "$FROM/$name.sha256" "$TMP/" || fail "$name or its .sha256 is not in $FROM."
  else
    url="https://github.com/$REPO/releases/download/$VERSION"
    fetch -qo "$TMP/$name" "$url/$name" || fail "Could not download $url/$name"
    fetch -qo "$TMP/$name.sha256" "$url/$name.sha256" || fail "Could not download $url/$name.sha256"
  fi
  want=$(awk '{print $1}' "$TMP/$name.sha256")
  got=$(sha256 -q "$TMP/$name")
  [ "$want" = "$got" ] || fail "$name does not match its SHA-256 sum: it may be damaged. Nothing was installed."
  info "$name: SHA-256 checked"
  tar -xzf "$TMP/$name" -C "$TMP"
}

# ---------------------------------------------------------------- the terminal app

get "coxswain-terminal-$VERSION-$TARGET"
T="$TMP/coxswain-terminal-$VERSION-$TARGET"
put mkdir -p "$PREFIX/bin" "$PREFIX/share/man/man1"
put install -m 755 "$T/coxswain" "$PREFIX/bin/coxswain"
put ln -sf coxswain "$PREFIX/bin/cox"
if [ -f "$T/coxswain.1" ]; then
  gzip -9c "$T/coxswain.1" >"$TMP/coxswain.1.gz"
  put install -m 644 "$TMP/coxswain.1.gz" "$PREFIX/share/man/man1/coxswain.1.gz"
  put ln -sf coxswain.1.gz "$PREFIX/share/man/man1/cox.1.gz"
fi
info "Terminal app: $PREFIX/bin/coxswain (and cox), manual: man coxswain"
# The search helper from boot, for one user, when Coxswain is installed for everyone. It stays
# off until rc.conf turns it on.
if [ "$PREFIX" = /usr/local ] && [ -f "$T/coxswain_index" ]; then
  put mkdir -p /usr/local/etc/rc.d
  put install -m 555 "$T/coxswain_index" /usr/local/etc/rc.d/coxswain_index
  info "rc.d script: /usr/local/etc/rc.d/coxswain_index (off; see the guide to turn it on)"
fi

# ---------------------------------------------------------------- the desktop app

# Packages the desktop app needs, and those that make it better.
NEEDS="webkit2-gtk_41 gtk3 libsoup3"
GOOD="xdg-utils gstreamer1-plugins-good"
missing() { for p in "$@"; do pkg info -e "$p" 2>/dev/null || printf '%s ' "$p"; done; }

if [ "$TERMINAL_ONLY" = 0 ]; then
  echo
  bold "The desktop app (experimental on FreeBSD)"
  info "It draws its window with WebKitGTK. From packages it needs:"
  info "  webkit2-gtk_41, gtk3, libsoup3   the window and what is shown in it"
  info "  xdg-utils                        opening files in their programs (xdg-open)"
  info "  gstreamer1-plugins-good          video and sound in the preview"
  # shellcheck disable=SC2086
  LACK=$(missing $NEEDS $GOOD)
  if [ -n "$LACK" ]; then
    info "Missing here: $LACK"
    info "I would run: pkg install -y $LACK"
    if ask "May I install them now (as root)?"; then
      # shellcheck disable=SC2086
      as_root pkg install -y $LACK
    fi
  fi
  # shellcheck disable=SC2086
  if [ -n "$(missing $NEEDS)" ]; then
    info "Without WebKitGTK the desktop app cannot start, so it is left out this time."
    info "Install the packages above and run this script again for it."
  else
    get "coxswain-desktop-$VERSION-$TARGET"
    D="$TMP/coxswain-desktop-$VERSION-$TARGET"
    put mkdir -p "$PREFIX/share/applications" "$PREFIX/share/icons/hicolor/128x128/apps"
    put install -m 755 "$D/coxswain-gui" "$PREFIX/bin/coxswain-gui"
    put install -m 644 "$D/coxswain.png" "$PREFIX/share/icons/hicolor/128x128/apps/coxswain.png"
    sed "s|^Exec=.*|Exec=$PREFIX/bin/coxswain-gui %F|" "$D/coxswain.desktop" >"$TMP/coxswain.desktop"
    put install -m 644 "$TMP/coxswain.desktop" "$PREFIX/share/applications/coxswain.desktop"
    have update-desktop-database && put update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
    info "Desktop app: $PREFIX/bin/coxswain-gui, and Coxswain in your desktop's menu"
  fi
fi

# ---------------------------------------------------------------- optional

echo
bold "Optional packages"
info "  nerd-fonts      the file icons and git glyphs (else choose plain characters in Settings)"
info "  tesseract       search the words in scans, screenshots and pictures"
info "  poppler-utils   the same for scanned PDFs (pdftoppm makes pictures of their pages)"
info "  libreoffice     search older Office files: .doc, .ppt, Visio and others"
OPT=$(missing nerd-fonts tesseract poppler-utils libreoffice)
if [ -n "$OPT" ]; then
  info "Not installed here: $OPT"
  info "Any of them, whenever you like: pkg install <name>"
fi

# ---------------------------------------------------------------- done

echo
bold "Done."
case ":$PATH:" in
  *":$PREFIX/bin:"*) ;;
  *)
    info "$PREFIX/bin is not on your PATH yet. For sh, add to ~/.profile:"
    info "  PATH=\"$PREFIX/bin:\$PATH\"; export PATH"
    info "For csh or tcsh, add to ~/.cshrc:  set path = ($PREFIX/bin \$path)"
    ;;
esac
info "Start the terminal app with: coxswain    (or cox)"
[ -x "$PREFIX/bin/coxswain-gui" ] && info "Start the desktop app with: coxswain-gui, or Coxswain in the menu"
info "Update later by running this script again. Guide: https://github.com/$REPO/blob/master/docs/reference/freebsd.md"
