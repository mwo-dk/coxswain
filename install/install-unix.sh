#!/bin/sh
# Install or update Coxswain on NetBSD, OpenBSD or illumos (OmniOS, OpenIndiana) from the latest
# release. (FreeBSD has install-freebsd.sh; Linux and macOS install.sh.)
#
#   NetBSD, OpenBSD:  ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh
#   illumos:          curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh
#
#   sh install-unix.sh [options]
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
# are installed with the system's own tool (pkgin, pkg_add, pkg) only after you have seen the
# command and said yes; root rights come from doas(1), sudo(8) or pfexec(1) when you are not root.
set -eu

REPO=mwo-dk/coxswain
RAW=https://raw.githubusercontent.com/$REPO/master
TERMINAL_ONLY=0
YES=0
PREFIX=
VERSION=
FROM=
UNINSTALL=0

usage() {
  if [ -f "$0" ]; then sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'
  else echo "See https://github.com/$REPO/blob/master/docs/reference/README.md"; fi
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

# Run a command as root: directly, or through doas, sudo or pfexec.
as_root() {
  if [ "$(id -u)" = 0 ]; then "$@"
  elif have doas; then doas "$@"
  elif have sudo; then sudo "$@"
  elif have pfexec; then pfexec "$@"
  else fail "This needs root rights, and none of doas, sudo or pfexec is installed. Run it as root: $*"
  fi
}
can_root() { [ "$(id -u)" = 0 ] || have doas || have sudo || have pfexec; }

# ---------------------------------------------------------------- the system

# What differs: the release's name for the system, its package tool, the man page folder
# below the prefix, the service file and where it goes, the desktop app's packages (empty: no
# desktop build), and the optional packages.
OS=$(uname -s)
case "$OS" in
  NetBSD)
    SYS=netbsd NAME="NetBSD $(uname -r)" MAN=man
    PKG_INSTALL="pkgin -y install" PKG_HINT="pkgin install"
    SERVICE=coxswain_index SERVICE_AT=/etc/rc.d/coxswain_index
    # No desktop build for NetBSD yet: webkit-gtk41's binary packages miss dependencies.
    DESKTOP_PKGS=""
    GOOD_PKGS=""
    OPTIONAL="tesseract poppler-utils libreoffice"
    installed() { pkg_info -q -e "$1" 2>/dev/null; }
    ;;
  OpenBSD)
    SYS=openbsd NAME="OpenBSD $(uname -r)" MAN=man
    PKG_INSTALL="pkg_add" PKG_HINT="pkg_add"
    SERVICE=coxswain_index SERVICE_AT=/etc/rc.d/coxswain_index
    DESKTOP_PKGS="webkitgtk41"
    GOOD_PKGS="xdg-utils gstreamer1-plugins-good"
    OPTIONAL="tesseract poppler-utils libreoffice"
    installed() { pkg_info -q -e "$1-*" 2>/dev/null; }
    ;;
  SunOS)
    SYS=illumos NAME="illumos ($(uname -v))" MAN=share/man
    PKG_INSTALL="pkg install" PKG_HINT="pkg install"
    SERVICE=coxswain-index.xml SERVICE_AT=/var/svc/manifest/site/coxswain-index.xml
    DESKTOP_PKGS=""
    GOOD_PKGS=""
    OPTIONAL=""
    installed() { pkg list -q "$1" 2>/dev/null; }
    ;;
  FreeBSD) fail "On FreeBSD use install-freebsd.sh: fetch -qo - $RAW/install/install-freebsd.sh | sh" ;;
  *) fail "This script is for NetBSD, OpenBSD and illumos. On Linux and macOS see install/INSTALL.md." ;;
esac

# Download a URL to a file with what the system has.
download() {
  if have curl; then curl -fsSL -o "$2" "$1"
  elif have wget; then wget -q -O "$2" "$1"
  elif [ "$OS" = OpenBSD ]; then ftp -V -o "$2" "$1"
  elif have ftp; then ftp -o "$2" "$1" >/dev/null
  else fail "No download tool: install curl, or use --from with archives fetched elsewhere."
  fi
}

# The SHA-256 of a file.
sum256() {
  if have sha256sum; then sha256sum "$1" | awk '{print $1}'
  elif have digest; then digest -a sha256 "$1"
  elif [ "$OS" = OpenBSD ]; then sha256 -q "$1"
  else cksum -a sha256 "$1" | awk '{print $NF}'
  fi
}

# ---------------------------------------------------------------- where

if [ -z "$PREFIX" ]; then
  if [ -x "$HOME/.local/bin/coxswain" ]; then PREFIX="$HOME/.local"
  elif [ -x /usr/local/bin/coxswain ]; then PREFIX=/usr/local
  elif [ "$(id -u)" = 0 ]; then PREFIX=/usr/local
  elif [ "$UNINSTALL" = 0 ] && can_root && ask "Install for every user in /usr/local (asks for root rights)? No puts it in ~/.local for you alone."; then PREFIX=/usr/local
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
    "$PREFIX/$MAN/man1/coxswain.1" "$PREFIX/$MAN/man1/cox.1" \
    "$PREFIX/share/applications/coxswain.desktop" "$PREFIX/share/icons/hicolor/128x128/apps/coxswain.png"
  if [ "$PREFIX" = /usr/local ] && [ -f "$SERVICE_AT" ]; then
    case "$SYS" in
      netbsd) as_root /etc/rc.d/coxswain_index onestop 2>/dev/null || true
        info "If rc.conf turns the helper on, remove its lines there too (coxswain_index, coxswain_index_user)." ;;
      openbsd) as_root rcctl disable coxswain_index 2>/dev/null || true; as_root rcctl stop coxswain_index 2>/dev/null || true ;;
      illumos) as_root svcadm disable -s application/coxswain-index 2>/dev/null || true
        as_root svccfg delete application/coxswain-index 2>/dev/null || true ;;
    esac
    as_root rm -f "$SERVICE_AT"
  fi
  rm -f "$HOME/.config/autostart/coxswain-index.desktop"
  info "Your settings and the search store stay: ~/.config/coxswain and ~/.cache/coxswain."
  info "Remove them too with: rm -rf ~/.config/coxswain ~/.local/share/coxswain ~/.cache/coxswain"
  exit 0
fi

# ---------------------------------------------------------------- which release

case "$(uname -m)" in
  amd64 | x86_64 | i86pc) TARGET=x86_64-unknown-$SYS ;;
  *)
    bold "There is no ready-made build for $(uname -m) yet."
    info "The terminal app builds from source with Rust ($PKG_HINT rust), then"
    info "  cargo install --locked coxswain"
    exit 1
    ;;
esac
# illumos reports i86pc for its 32- and 64-bit kernels alike; the build is 64-bit.
[ "$SYS" = illumos ] && [ "$(isainfo -b 2>/dev/null || echo 64)" != 64 ] && fail "This needs a 64-bit illumos kernel."

TMP=$(mktemp -d "${TMPDIR:-/tmp}/coxswain.XXXXXX")
trap 'rm -rf "$TMP"' EXIT INT TERM

if [ -z "$VERSION" ] && [ -z "$FROM" ]; then
  download "https://api.github.com/repos/$REPO/releases/latest" "$TMP/latest.json" || true
  VERSION=$(sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' "$TMP/latest.json" 2>/dev/null | head -n 1)
  [ -n "$VERSION" ] || fail "Could not ask GitHub for the latest release. Is the network up? Or name one: --version v2.7.0"
fi
if [ -z "$VERSION" ]; then
  # shellcheck disable=SC2012
  VERSION=$(ls "$FROM" | sed -n "s/^coxswain-terminal-\(v[0-9.]*\)-$TARGET\.tar\.gz$/\1/p" | sort -t. -k1,1 -k2,2n -k3,3n | tail -n 1)
  [ -n "$VERSION" ] || fail "No coxswain-terminal-v…-$TARGET.tar.gz in $FROM."
fi
case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac

if [ -x "$PREFIX/bin/coxswain" ]; then
  NOW=$("$PREFIX/bin/coxswain" --version 2>/dev/null | awk '{print $NF}')
  bold "Updating Coxswain ${NOW:+v$NOW }to $VERSION in $PREFIX ($NAME)"
else
  bold "Installing Coxswain $VERSION into $PREFIX ($NAME)"
fi

# Fetch an archive and its sum, check the one against the other, and unpack it. Fails
# quietly (status 1) when the release has no such archive.
get() {
  name="$1.tar.gz"
  if [ -n "$FROM" ]; then
    [ -f "$FROM/$name" ] || return 1
    cp "$FROM/$name" "$FROM/$name.sha256" "$TMP/" || fail "$name.sha256 is not in $FROM."
  else
    url="https://github.com/$REPO/releases/download/$VERSION"
    download "$url/$name.sha256" "$TMP/$name.sha256" 2>/dev/null || return 1
    download "$url/$name" "$TMP/$name" || fail "Could not download $url/$name"
  fi
  want=$(awk '{print $1}' "$TMP/$name.sha256")
  got=$(sum256 "$TMP/$name") || fail "Could not compute the SHA-256 of $name."
  [ "$want" = "$got" ] || fail "$name does not match its SHA-256 sum: it may be damaged. Nothing was installed."
  info "$name: SHA-256 checked"
  # `get` runs inside `||`, where `set -e` does not stop the script: each failure says so.
  (cd "$TMP" && gzip -dc "$name" | tar -xf -) || fail "Could not unpack $name in $TMP: is the disk full?"
}

# ---------------------------------------------------------------- the terminal app

get "coxswain-terminal-$VERSION-$TARGET" || fail "The release $VERSION has no build for $NAME ($TARGET). Releases from 2.7.0 on have one."
T="$TMP/coxswain-terminal-$VERSION-$TARGET"
put mkdir -p "$PREFIX/bin" "$PREFIX/$MAN/man1"
put cp "$T/coxswain" "$PREFIX/bin/coxswain.new"
put chmod 755 "$PREFIX/bin/coxswain.new"
put mv -f "$PREFIX/bin/coxswain.new" "$PREFIX/bin/coxswain"
put ln -sf coxswain "$PREFIX/bin/cox"
if [ -f "$T/coxswain.1" ]; then
  put cp "$T/coxswain.1" "$PREFIX/$MAN/man1/coxswain.1"
  put chmod 644 "$PREFIX/$MAN/man1/coxswain.1"
  put ln -sf coxswain.1 "$PREFIX/$MAN/man1/cox.1"
fi
info "Terminal app: $PREFIX/bin/coxswain (and cox), manual: man coxswain"

# The search helper from boot, for one user, when Coxswain is installed for everyone. It stays
# off until you turn it on (the guide says how).
if [ "$PREFIX" = /usr/local ] && [ -f "$T/$SERVICE" ]; then
  as_root mkdir -p "$(dirname "$SERVICE_AT")"
  as_root cp "$T/$SERVICE" "$SERVICE_AT"
  case "$SYS" in
    illumos)
      as_root chmod 444 "$SERVICE_AT"
      as_root svccfg import "$SERVICE_AT"
      info "SMF service: application/coxswain-index (disabled; see the guide to turn it on)" ;;
    *)
      as_root chmod 555 "$SERVICE_AT"
      info "rc.d script: $SERVICE_AT (off; see the guide to turn it on)" ;;
  esac
fi

# ---------------------------------------------------------------- the desktop app

missing() { for p in "$@"; do installed "$p" || printf '%s\n' "$p"; done | tr '\n' ' ' | sed 's/ $//'; }

if [ "$TERMINAL_ONLY" = 0 ] && [ -n "$DESKTOP_PKGS" ]; then
  echo
  bold "The desktop app (experimental on $OS)"
  info "It draws its window with WebKitGTK. From packages it needs: $DESKTOP_PKGS"
  info "and for opening files and playing video and sound: $GOOD_PKGS"
  # shellcheck disable=SC2086
  LACK=$(missing $DESKTOP_PKGS $GOOD_PKGS)
  if [ -n "$LACK" ]; then
    info "Missing here: $LACK"
    info "I would run: $PKG_INSTALL $LACK"
    if ask "May I install them now (as root)?"; then
      # shellcheck disable=SC2086
      as_root $PKG_INSTALL $LACK || info "Not all of them could be installed."
    fi
  fi
  # shellcheck disable=SC2086
  if [ -n "$(missing $DESKTOP_PKGS)" ]; then
    info "Without WebKitGTK the desktop app cannot start, so it is left out this time."
    info "Install the packages above and run this script again for it."
  elif get "coxswain-desktop-$VERSION-$TARGET"; then
    D="$TMP/coxswain-desktop-$VERSION-$TARGET"
    put mkdir -p "$PREFIX/share/applications" "$PREFIX/share/icons/hicolor/128x128/apps"
    put cp "$D/coxswain-gui" "$PREFIX/bin/coxswain-gui"
    put chmod 755 "$PREFIX/bin/coxswain-gui"
    put cp "$D/coxswain.png" "$PREFIX/share/icons/hicolor/128x128/apps/coxswain.png"
    sed "s|^Exec=.*|Exec=$PREFIX/bin/coxswain-gui %F|" "$D/coxswain.desktop" >"$TMP/coxswain.desktop"
    put cp "$TMP/coxswain.desktop" "$PREFIX/share/applications/coxswain.desktop"
    have update-desktop-database && put update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
    info "Desktop app: $PREFIX/bin/coxswain-gui, and Coxswain in your desktop's menu"
  else
    info "The release $VERSION has no desktop build for $OS."
  fi
fi

# ---------------------------------------------------------------- optional

if [ -n "$OPTIONAL" ]; then
  echo
  bold "Optional packages"
  info "  tesseract       search the words in scans, screenshots and pictures"
  info "  poppler-utils   the same for scanned PDFs (pdftoppm makes pictures of their pages)"
  info "  libreoffice     search older Office files: .doc, .ppt, Visio and others"
  # shellcheck disable=SC2086
  OPT=$(missing $OPTIONAL)
  [ -n "$OPT" ] && info "Not installed here: $OPT. Any of them, whenever you like: $PKG_HINT <name>"
fi

# ---------------------------------------------------------------- done

echo
bold "Done."
case ":$PATH:" in
  *":$PREFIX/bin:"*) ;;
  *)
    info "$PREFIX/bin is not on your PATH yet. For sh and ksh, add to ~/.profile:"
    info "  PATH=\"$PREFIX/bin:\$PATH\"; export PATH"
    [ "$PREFIX" != /usr/local ] && info "and for the manual: MANPATH=\"$PREFIX/$MAN:\$MANPATH\"; export MANPATH"
    ;;
esac
info "Start the terminal app with: coxswain    (or cox)"
[ -x "$PREFIX/bin/coxswain-gui" ] && info "Start the desktop app with: coxswain-gui, or Coxswain in the menu"
info "Update later by running this script again. Guide: https://github.com/$REPO/blob/master/docs/reference/$SYS.md"
