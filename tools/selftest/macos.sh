#!/bin/bash
# Coxswain self-test for macOS: downloads a release into a temporary folder, checks it, drives the
# terminal app in a pseudo-terminal, starts the desktop app, and writes a Markdown report.
#
#   curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/tools/selftest/macos.sh | bash
#   bash tools/selftest/macos.sh --help
#
# Everything runs with a fake HOME (and XDG_*, CFFIXED_USER_HOME) in a folder of its own under
# /tmp, removed at the end. Never sudo, never installs anything (except with --brew, after asking).
# Nothing leaves the machine but the downloads from GitHub. tools/selftest/README.md.
#
# Written for the bash 3.2 that macOS ships: no associative arrays, no ${x,,}, no mapfile.

unset CDPATH
REPO=${COXSWAIN_SELFTEST_REPO:-mwo-dk/coxswain}
VERSION=""
BREW=0 KEEP=0 NO_ASK=0 DRY=0 CI_MODE=0 DND=0 OUT="" TERM_BIN=""

usage() {
  cat <<'EOF'
Coxswain self-test for macOS

  macos.sh [options]

  --version X.Y.Z    test that release (default: the latest)
  --brew             test the Homebrew formula and cask instead of the downloads: installs them
                     (asks first, y/N) and offers to remove what it installed at the end
  --keep             keep the temporary folder (fake home, downloads, logs) at the end
  --no-ask           ask nothing: no screenshots, no by-hand checklist, report to the Desktop
  --out DIR          write the report (and screenshots) to DIR instead of the Desktop
  --ci               unattended, for GitHub's macOS runners: --no-ask, screenshots on, report
                     and screenshots in --out (default ./selftest-out), a summary in
                     $GITHUB_STEP_SUMMARY, exit 1 on any FAIL
  --terminal-bin P   use the terminal app at P instead of the downloaded one (a local build)
  --dnd              only drag and drop in the desktop app: you drag, the test checks the files
  --dry-run          on a system other than macOS: run the parts that are not macOS-specific
  --help             this text

What it does, each item marked PASS, FAIL, SKIP or NOTE:
  1. downloads the terminal app and the desktop .dmg, checks them against their .sha256 and
     SHA256SUMS; signature, Gatekeeper and quarantine status of the app
  2. the terminal app: --version, --help (against the docs), --paths; then its main features
     driven by keys in a pseudo-terminal (expect), each timed, a hang caught by a timeout
  3. the search helper: an index in the fake home, Find finds a planted word
  4. the desktop app: starts and stays up, --settings=search; optional screenshots; a short
     checklist to try by hand, answers recorded
  5. clean-up: stops only what it started, removes the temporary folder
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION=${2:-}; shift ;;
    --version=*) VERSION=${1#*=} ;;
    --brew) BREW=1 ;;
    --keep) KEEP=1 ;;
    --no-ask) NO_ASK=1 ;;
    --out) OUT=${2:-}; shift ;;
    --out=*) OUT=${1#*=} ;;
    --ci) CI_MODE=1; NO_ASK=1 ;;
    --terminal-bin) TERM_BIN=${2:-}; shift ;;
    --terminal-bin=*) TERM_BIN=${1#*=} ;;
    --dnd) DND=1 ;;
    --dry-run) DRY=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

VERSION=${VERSION#v}
if [ -n "$VERSION" ] && ! echo "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "Not a version: $VERSION (want X.Y.Z)" >&2; exit 2
fi
if [ "$CI_MODE" = 1 ] && [ "$BREW" = 1 ]; then echo "--ci and --brew do not go together." >&2; exit 2; fi
if [ "$DND" = 1 ] && [ "$NO_ASK" = 1 ]; then echo "--dnd needs you at the Mac: not with --no-ask or --ci." >&2; exit 2; fi
MAC=0; [ "$(uname -s)" = Darwin ] && MAC=1
if [ "$MAC" = 0 ] && [ "$DRY" = 0 ]; then
  echo "This self-test is for macOS. On another system, --dry-run runs the parts that are not macOS-specific." >&2
  exit 2
fi
if [ "$(id -u)" = 0 ]; then echo "Please run it as yourself, not as root." >&2; exit 2; fi
if [ -n "$TERM_BIN" ]; then
  [ -x "$TERM_BIN" ] || { echo "Not a program: $TERM_BIN" >&2; exit 2; }
  TERM_BIN=$(cd "$(dirname "$TERM_BIN")" && pwd -P)/$(basename "$TERM_BIN")
fi
[ "$CI_MODE" = 1 ] && [ -z "$OUT" ] && OUT="$PWD/selftest-out"
if [ -n "$OUT" ]; then
  if mkdir -p "$OUT"; then OUT=$(cd "$OUT" && pwd -P); else echo "Cannot write to $OUT" >&2; exit 2; fi
fi

# ---------------------------------------------------------------------------------------------
# Output and results

if [ -t 1 ] && [ "$CI_MODE" = 0 ]; then
  C_PASS=$'\033[32m' C_FAIL=$'\033[31m' C_SKIP=$'\033[33m' C_NOTE=$'\033[36m' C_B=$'\033[1m' C_0=$'\033[0m'
else
  C_PASS="" C_FAIL="" C_SKIP="" C_NOTE="" C_B="" C_0=""
fi
NPASS=0 NFAIL=0 NSKIP=0 NNOTE=0 FAILED_NAMES="" SECTION=""
REAL_HOME=$HOME

say() { printf '%s\n' "$*"; }
section() { SECTION=$1; printf '\n%s== %s ==%s\n' "$C_B" "$1" "$C_0"; }
md() { printf '%s' "$1" | tr '\n' ' ' | sed 's/|/\\|/g'; }
# result STATUS NAME [DETAIL] [MILLISECONDS]
result() {
  local st=$1 name=$2 detail=${3:-} ms=${4:-} c t=""
  case "$st" in
    PASS) NPASS=$((NPASS + 1)); c=$C_PASS ;;
    FAIL) NFAIL=$((NFAIL + 1)); c=$C_FAIL; FAILED_NAMES="$FAILED_NAMES
- $SECTION: $name" ;;
    SKIP) NSKIP=$((NSKIP + 1)); c=$C_SKIP ;;
    *) st=NOTE; NNOTE=$((NNOTE + 1)); c=$C_NOTE ;;
  esac
  case "$ms" in ''|*[!0-9]*) ;; *) t="$((ms / 1000)).$(((ms % 1000) / 100)) s" ;; esac
  printf '  %s%-4s%s  %s%s%s\n' "$c" "$st" "$C_0" "$name" "${t:+  ($t)}" "${detail:+  -- $detail}"
  printf '| %s | %s | %s | %s | %s |\n' "$st" "$(md "$SECTION")" "$(md "$name")" "$t" "$(md "$detail")" >>"$ROWS"
  if [ "$CI_MODE" = 1 ] && [ "$st" = FAIL ]; then
    echo "::error title=Mac self-test: $SECTION: $name::$(md "$detail")"
  fi
}
# ask QUESTION y|n: yes or no from the keyboard (the script may come through a pipe: /dev/tty)
ask() {
  local q=$1 def=$2 a=""
  if [ "$NO_ASK" = 1 ]; then [ "$def" = y ]; return; fi
  if [ "$def" = y ]; then printf '%s [Y/n] ' "$q"; else printf '%s [y/N] ' "$q"; fi
  read -r a </dev/tty 2>/dev/null || { echo; return 1; }  # no keyboard: no
  case "$a" in [yY]*) return 0 ;; [nN]*) return 1 ;; *) [ "$def" = y ] ;; esac
}
sum_of() { if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1"; else sha256sum "$1"; fi | awk '{print $1}'; }

# ---------------------------------------------------------------------------------------------
# The temporary folder and the fake home. Under /tmp, not $TMPDIR: Coxswain's walks leave out
# /private/var/folders, where $TMPDIR is on a Mac.

# COXSWAIN_SELFTEST_TMP: another base than /tmp (the script's own checks use it on Linux).
WORK=$(mktemp -d "${COXSWAIN_SELFTEST_TMP:-/tmp}/coxswain-selftest.XXXXXX") || { echo "mktemp failed" >&2; exit 1; }
WORK=$(cd "$WORK" && pwd -P) || exit 1
FHOME=$WORK/home DEMO=$WORK/home/demo RIGHT=$WORK/home/right
DL=$WORK/downloads LOGS=$WORK/logs ROWS=$WORK/rows.md SHOTS=$WORK/shots
mkdir -p "$FHOME" "$DEMO" "$RIGHT" "$DL" "$LOGS" "$SHOTS" "$WORK/config" "$WORK/cache" "$WORK/data"
: >"$ROWS"
PIDS=""

# The fake environment, for every Coxswain program this test starts.
fake() {
  env HOME="$FHOME" CFFIXED_USER_HOME="$FHOME" XDG_CONFIG_HOME="$WORK/config" XDG_CACHE_HOME="$WORK/cache" \
    XDG_DATA_HOME="$WORK/data" LANG=en_GB.UTF-8 LC_ALL=en_GB.UTF-8 GIT_CONFIG_NOSYSTEM=1 "$@"
}

# What macOS itself may write for an app by its bundle id, whatever HOME says (SYS_PATHS), and
# Coxswain's own places in the real home (OWN_PATHS), looked at before and after. What appeared
# is only reported, with the command that removes it: this script removes nothing outside $WORK.
SYS_PATHS="Library/WebKit/dk.mwo.coxswain
Library/Caches/dk.mwo.coxswain
Library/HTTPStorages/dk.mwo.coxswain
Library/Saved Application State/dk.mwo.coxswain.savedState
Library/Preferences/dk.mwo.coxswain.plist"
OWN_PATHS="Library/Application Support/coxswain
Library/Application Support/dk.mwo.coxswain
Library/Caches/coxswain
Library/LaunchAgents/dk.mwo.coxswain.index.plist
.config/coxswain
.cache/coxswain
.local/share/coxswain"
mtime() { if [ "$MAC" = 1 ]; then stat -f %m "$1" 2>/dev/null; else stat -c %Y "$1" 2>/dev/null; fi; }
# BEFORE_LIST: "path<TAB>mtime" per line, of what was there at the start; kept in memory.
BEFORE_LIST=""
while IFS= read -r p; do
  [ -n "$p" ] && [ -e "$REAL_HOME/$p" ] && BEFORE_LIST="$BEFORE_LIST$p	$(mtime "$REAL_HOME/$p")
"
done <<EOF
$SYS_PATHS
$OWN_PATHS
EOF
was_there() { printf '%s' "$BEFORE_LIST" | cut -f1 | grep -Fxq "$1"; }

# The PIDs of every process with this run's fake home: what this test started, the search
# helper it detached too. Nothing else is ever stopped.
ours() {
  local p
  if [ "$MAC" = 1 ]; then
    # shellcheck disable=SC2009 # pgrep cannot match on the environment
    ps -E -ww -x -o pid=,command= -U "$(id -u)" 2>/dev/null | grep -E " HOME=$FHOME( |\$)" | awk '{print $1}'
  else
    for p in /proc/[0-9]*; do
      tr '\0' '\n' 2>/dev/null <"$p/environ" | grep -Fxq "HOME=$FHOME" && echo "${p#/proc/}"
    done
  fi
}

cleanup() {
  local p
  trap - EXIT INT TERM HUP
  for p in $PIDS $(ours | tr '\n' ' '); do [ "$p" != "$$" ] && kill "$p" 2>/dev/null; done
  sleep 1
  for p in $(ours | tr '\n' ' '); do [ "$p" != "$$" ] && kill -9 "$p" 2>/dev/null; done
  [ -d "$WORK/mnt" ] && [ "$MAC" = 1 ] && hdiutil detach -force "$WORK/mnt" >/dev/null 2>&1
  [ "$LAUNCHD_SAID" = 1 ] || launchd_check say
  if [ "$KEEP" = 1 ]; then
    say "Kept: $WORK"
  else
    unregister "$WORK/app/Coxswain.app"
    rm -rf "$WORK" && say "Removed the temporary folder ($WORK)."
  fi
}
# Take a copied app out of LaunchServices' list before its folder goes (macOS only).
unregister() {
  local ls=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
  [ "$MAC" = 1 ] && [ -d "$1" ] && [ -x "$ls" ] && "$ls" -u "$1" >/dev/null 2>&1
  return 0
}
# The test's app registered itself with launchd (Start with my session turned on in it): its
# LaunchAgent is in the fake home, but launchd took it into your session. This script never runs
# launchctl itself; it says what happened and how to undo it. launchd_check row|say
LAUNCHD_SAID=0
launchd_check() {
  local uid own="" how
  [ "$MAC" = 1 ] && [ -e "$FHOME/Library/LaunchAgents/dk.mwo.coxswain.index.plist" ] || return 0
  uid=$(id -u)
  was_there "Library/LaunchAgents/dk.mwo.coxswain.index.plist" &&
    own="; launchctl bootstrap gui/$uid ~/Library/LaunchAgents/dk.mwo.coxswain.index.plist"
  how="launchctl bootout gui/$uid/dk.mwo.coxswain.index$own"
  if [ "$1" = row ]; then
    result FAIL "the test app registered its search helper with launchd" "Start with my session was turned on in the test copy; launchd now knows a job for it. Undo: $how"
  else
    say "The test app registered its search helper with launchd (Start with my session was turned on)."
    say "This script does not touch launchd. To undo it, run:"
    say "  launchctl bootout gui/$uid/dk.mwo.coxswain.index"
    [ -n "$own" ] && say "  launchctl bootstrap gui/$uid ~/Library/LaunchAgents/dk.mwo.coxswain.index.plist   # your own, again"
  fi
  LAUNCHD_SAID=1
}
trap cleanup EXIT
trap 'echo; echo "Stopped."; exit 130' INT TERM HUP

# ---------------------------------------------------------------------------------------------

main() {
  say "${C_B}Coxswain self-test for macOS${C_0}"
  say "Everything happens in $WORK, with a fake home folder there."
  say "Your own Coxswain settings, state and cache are not touched; nothing is installed."
  [ "$DRY" = 1 ] && say "Dry run: the macOS-specific parts are skipped."

  if [ -z "$VERSION" ]; then
    VERSION=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" | sed -n 's#.*/tag/v##p')
    [ -n "$VERSION" ] || { echo "Could not find the latest release of $REPO." >&2; exit 1; }
  fi
  say "Testing Coxswain $VERSION."

  ARCH=$(uname -m)
  [ "$MAC" = 1 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null)" = 1 ] && ARCH=arm64
  case "$ARCH" in
    arm64|aarch64) TARGET=aarch64-apple-darwin DMG_ARCH=aarch64 ;;
    *) TARGET=x86_64-apple-darwin DMG_ARCH=x64 ;;
  esac
  TERM_TGZ="coxswain-terminal-v$VERSION-$TARGET.tar.gz"
  DMG="Coxswain_${VERSION}_${DMG_ARCH}.dmg"

  SHOT_OK=0
  if [ "$MAC" = 1 ] && [ "$CI_MODE" = 1 ]; then SHOT_OK=1; fi
  if [ "$MAC" = 1 ] && [ "$NO_ASK" = 0 ]; then
    say ""
    say "Screenshots of the desktop app need macOS's Screen Recording permission for this terminal"
    say "(macOS asks once). They show the whole screen: close anything private first."
    ask "Take screenshots of the desktop app?" n && SHOT_OK=1
  fi

  write_config
  make_demo
  if [ "$BREW" = 1 ]; then brew_install; else get_release; fi
  check_signatures
  if [ "$DND" = 1 ]; then
    test_dnd
  else
    test_terminal
    test_desktop
  fi
  finish
}

write_config() {
  local c
  # Where both apps read it on a Mac and elsewhere; --config-path says which, checked later.
  for c in "$WORK/config/coxswain" "$FHOME/Library/Application Support/coxswain"; do
    mkdir -p "$c"
    cat >"$c/config.toml" <<EOF
# Written by the Coxswain self-test: English, no update check, search only in the fake home.
language = "en-GB"
check_updates = false

[search]
name_roots = ["$FHOME"]
text_roots = ["$FHOME"]
name_exclude = ["$FHOME/Library/CloudStorage", "$FHOME/Library/Mobile Documents"]
text_exclude = ["CloudStorage", "Mobile Documents"]
EOF
  done
}

make_demo() {
  section "Demo folder"
  local i
  ( cd "$DEMO" || exit 1
    {
      echo "# Rowing on the harbour"
      echo
      echo "A *demo* file for the Coxswain self-test, with **Markdown**."
      echo
      i=5
      while [ $i -le 200 ]; do
        if [ $i = 150 ]; then echo "selftestzebra marks this line: OARLOCK"; else echo "line $i of the notes"; fi
        i=$((i + 1))
      done
    } >notes.md
    echo "copy me" >copyme.txt
    echo "move me" >moveme.txt
    echo "delete me" >deletemeforever.txt
    echo "The rare quillwort grows by the boathouse." >plants.txt
    printf 'fn main() {\n    // the stroke rate\n    let rate = 32;\n    println!("{rate} strokes a minute");\n}\n' >main.rs
    printf 'def stroke(n):\n    """Count the strokes."""\n    return n * 2\n' >script.py
    printf '{"boat": "eight", "crew": 9, "cox": true}\n' >data.json
    printf 'seat,name,weight\n1,Bow,72\n2,Two,80\n8,Stroke,85\n' >crew.csv
    printf '[Event "Selftest"]\n[Site "Harbour"]\n[Date "2026.10.20"]\n[White "Selftest White"]\n[Black "Selftest Black"]\n[Result "1-0"]\n\n1. e4 e5 2. Qh5 Nc6 3. Bc4 Nf6 4. Qxf7# 1-0\n' >game.pgn
    mkdir -p bundle && echo "inside the archive" >bundle/inside-archive.txt
    tar czf bundle.tar.gz bundle
    if command -v ditto >/dev/null 2>&1; then ditto -c -k --keepParent bundle bundle.zip
    elif command -v zip >/dev/null 2>&1; then zip -qr bundle.zip bundle
    fi
    rm -rf "$DEMO/bundle"
  ) || { result FAIL "demo files" "could not make them in $DEMO"; return; }
  result PASS "text, Markdown, code, JSON, CSV, PGN, tar.gz files" "in demo/"
  if [ -f "$DEMO/bundle.zip" ]; then result PASS "zip archive"; else result SKIP "zip archive" "no ditto or zip"; fi

  local icon=/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericDocumentIcon.icns
  if command -v sips >/dev/null 2>&1 && [ -f "$icon" ] &&
    sips -s format png "$icon" --out "$DEMO/picture.png" >/dev/null 2>&1 &&
    sips -s format jpeg "$DEMO/picture.png" --out "$DEMO/photo.jpg" >/dev/null 2>&1; then
    result PASS "pictures (PNG, JPEG)" "made with sips"
  else
    result SKIP "pictures (PNG, JPEG)" "no sips (macOS only)"
  fi
  if command -v cupsfilter >/dev/null 2>&1 && cupsfilter "$DEMO/notes.md" >"$DEMO/report.pdf" 2>/dev/null && [ -s "$DEMO/report.pdf" ]; then
    result PASS "PDF" "made with cupsfilter"
  else
    rm -f "$DEMO/report.pdf"; result SKIP "PDF" "cupsfilter could not make one"
  fi
  if [ "$MAC" = 1 ]; then
    mkdir -p "$WORK/isosrc" && echo "inside the disk image" >"$WORK/isosrc/inside-iso.txt"
    if hdiutil makehybrid -quiet -iso -joliet -default-volume-name SELFTEST -o "$DEMO/selftest.iso" "$WORK/isosrc" 2>/dev/null; then
      result PASS "ISO disk image" "made with hdiutil makehybrid"
    else
      result SKIP "ISO disk image" "hdiutil makehybrid failed"
    fi
  else
    result SKIP "ISO disk image" "hdiutil is macOS only"
  fi
  # git on a Mac without the Command Line Tools is a stub that opens an install dialog: not run then.
  GIT_OK=0
  if { [ "$MAC" = 0 ] || xcode-select -p >/dev/null 2>&1; } && command -v git >/dev/null 2>&1; then
    if ( mkdir -p "$DEMO/repo" && cd "$DEMO/repo" &&
      g() { fake git -c user.name="Selftest Rower" -c user.email=selftest@example.invalid "$@" >/dev/null 2>&1; } &&
      g init -q && g checkout -q -b main && g config user.name "Selftest Rower" &&
      g config user.email selftest@example.invalid && echo "# The boat" >README.md && g add . &&
      g commit -q -m "First stroke" && g checkout -q -b feature/oars && echo oars >oars.txt &&
      g add . && g commit -q -m "Add the oars" && g checkout -q main ); then
      GIT_OK=1; result PASS "git repository" "two commits, branches main and feature/oars"
    else
      result FAIL "git repository" "git could not make it"
    fi
  else
    result SKIP "git repository" "no git (Command Line Tools not installed)"
  fi
}

get_release() {
  section "Downloads and checksums"
  local base="https://github.com/$REPO/releases/download/v$VERSION" f t0 want got
  for f in SHA256SUMS "$TERM_TGZ" "$TERM_TGZ.sha256" "$DMG"; do
    t0=$SECONDS
    if curl -fsSL --retry 3 --retry-delay 3 --connect-timeout 30 -o "$DL/$f" "$base/$f"; then
      result PASS "download $f" "$(du -h "$DL/$f" | awk '{print $1}')" "$(((SECONDS - t0) * 1000))"
    else
      result FAIL "download $f" "$base/$f"
    fi
  done
  [ -s "$DL/$TERM_TGZ.sha256" ] && [ -s "$DL/$TERM_TGZ" ] && {
    want=$(awk '{print $1; exit}' "$DL/$TERM_TGZ.sha256"); got=$(sum_of "$DL/$TERM_TGZ")
    if [ "$want" = "$got" ]; then result PASS "$TERM_TGZ matches its .sha256"; else result FAIL "$TERM_TGZ matches its .sha256" "want $want, got $got"; fi
  }
  for f in "$TERM_TGZ" "$DMG"; do
    [ -s "$DL/$f" ] && [ -s "$DL/SHA256SUMS" ] || continue
    want=$(awk -v f="$f" '{n = $2; sub(/^\*/, "", n)} n == f {print $1}' "$DL/SHA256SUMS")
    got=$(sum_of "$DL/$f")
    if [ -z "$want" ]; then result FAIL "$f in SHA256SUMS" "not listed"
    elif [ "$want" = "$got" ]; then result PASS "$f matches SHA256SUMS"
    else result FAIL "$f matches SHA256SUMS" "want $want, got $got"; fi
  done

  BIN=""
  if [ -n "$TERM_BIN" ]; then
    BIN=$TERM_BIN; result NOTE "terminal app" "using $TERM_BIN, not the download"
  elif [ -s "$DL/$TERM_TGZ" ]; then
    mkdir -p "$WORK/term" && tar xzf "$DL/$TERM_TGZ" -C "$WORK/term" </dev/null
    BIN="$WORK/term/${TERM_TGZ%.tar.gz}/coxswain"
    if [ -x "$BIN" ] && [ -L "$WORK/term/${TERM_TGZ%.tar.gz}/cox" ]; then
      result PASS "unpack the terminal app" "coxswain and cox"
    else
      result FAIL "unpack the terminal app" "coxswain or cox missing"; [ -x "$BIN" ] || BIN=""
    fi
  fi

  APP=""
  if [ "$MAC" = 1 ] && [ -s "$DL/$DMG" ]; then
    mkdir -p "$WORK/mnt" "$WORK/app"
    if hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$WORK/mnt" "$DL/$DMG" >"$LOGS/hdiutil.log" 2>&1 </dev/null; then
      if [ -d "$WORK/mnt/Coxswain.app" ] && ditto "$WORK/mnt/Coxswain.app" "$WORK/app/Coxswain.app"; then
        APP="$WORK/app/Coxswain.app"; result PASS "open the .dmg read-only, copy Coxswain.app out"
      else
        result FAIL "copy Coxswain.app out of the .dmg" "$(cd "$WORK/mnt" && echo *)"
      fi
      hdiutil detach "$WORK/mnt" >/dev/null 2>&1 || hdiutil detach -force "$WORK/mnt" >/dev/null 2>&1
      rmdir "$WORK/mnt" 2>/dev/null
    else
      result FAIL "open the .dmg (hdiutil attach)" "$(tail -1 "$LOGS/hdiutil.log")"
    fi
  elif [ "$MAC" = 0 ]; then
    result SKIP "open the .dmg" "hdiutil is macOS only"
  fi
}

BREW_NEW=""
brew_install() {
  section "Homebrew"
  BIN="" APP=""
  if ! command -v brew >/dev/null 2>&1; then result SKIP "Homebrew" "brew is not installed"; return; fi
  export HOMEBREW_NO_ANALYTICS=1 HOMEBREW_NO_ENV_HINTS=1 HOMEBREW_NO_AUTO_UPDATE=1
  local t0 have_f=0 have_c=0 tapped=0
  brew list --formula coxswain >/dev/null 2>&1 && have_f=1
  brew list --cask coxswain-gui >/dev/null 2>&1 && have_c=1
  brew tap 2>/dev/null | grep -qx mwo-dk/coxswain && tapped=1
  # What is installed already is tested as it is: never upgraded or reinstalled here.
  [ "$have_f" = 1 ] && result NOTE "coxswain already installed with Homebrew" "tested as it is, not upgraded"
  [ "$have_c" = 1 ] && result NOTE "coxswain-gui already installed with Homebrew" "tested as it is, not upgraded"
  if [ "$have_f" = 0 ] || [ "$have_c" = 0 ]; then
    say "With --brew this test installs, with your own Homebrew (outside the fake home):"
    [ "$have_f" = 0 ] && say "  brew install mwo-dk/coxswain/coxswain                 (the terminal app)"
    [ "$have_c" = 0 ] && say "  brew install --cask mwo-dk/coxswain/coxswain-gui      (the desktop app, into Applications)"
    [ "$tapped" = 0 ] && say "This also adds the tap mwo-dk/coxswain to Homebrew (offered to remove at the end)."
    say "Homebrew may install or upgrade what these need. Its analytics and auto-update are off here."
    if ! ask "Install them now?" n; then
      result SKIP "Homebrew install" "not installed: you said no"
    else
      [ "$tapped" = 0 ] && BREW_NEW="$BREW_NEW tap"
      if [ "$have_f" = 0 ]; then
        t0=$SECONDS
        if brew install mwo-dk/coxswain/coxswain </dev/null >"$LOGS/brew-formula.log" 2>&1; then
          BREW_NEW="$BREW_NEW formula"; have_f=1
          result PASS "brew install coxswain" "" "$(((SECONDS - t0) * 1000))"
        else
          result FAIL "brew install coxswain" "$(tail -1 "$LOGS/brew-formula.log")"
        fi
      fi
      if [ "$have_c" = 0 ]; then
        t0=$SECONDS
        if brew install --cask mwo-dk/coxswain/coxswain-gui </dev/null >"$LOGS/brew-cask.log" 2>&1; then
          BREW_NEW="$BREW_NEW cask"; have_c=1
          result PASS "brew install --cask coxswain-gui" "" "$(((SECONDS - t0) * 1000))"
        else
          result FAIL "brew install --cask coxswain-gui" "$(tail -1 "$LOGS/brew-cask.log")"
        fi
      fi
    fi
  fi
  [ "$have_f" = 1 ] && BIN="$(brew --prefix)/bin/coxswain"
  if [ "$have_c" = 1 ]; then
    for APP in /Applications/Coxswain.app "$REAL_HOME/Applications/Coxswain.app" ""; do [ -d "$APP" ] && break; done
    [ -n "$APP" ] || result FAIL "Coxswain.app installed" "not in /Applications or ~/Applications"
  fi
}

check_signatures() {
  section "Signature and Gatekeeper"
  if [ "$MAC" = 0 ]; then result SKIP "codesign, spctl, quarantine" "macOS only"; return; fi
  local out
  if [ -n "$BIN" ]; then
    if codesign --verify --strict "$BIN" 2>"$LOGS/codesign-term.log"; then
      result PASS "terminal app: signature valid" "$(codesign -dv "$BIN" 2>&1 | grep -E '^Signature=|^Authority=' | head -1)"
    elif [ "$(uname -m)" = x86_64 ] && grep -q "not signed at all" "$LOGS/codesign-term.log"; then
      # The linker signs arm64 binaries (ad hoc) by itself, not Intel ones; those run unsigned.
      result NOTE "terminal app: signature" "none on Intel, which needs none to run"
    else
      result FAIL "terminal app: signature valid" "$(tail -1 "$LOGS/codesign-term.log")"
    fi
    if xattr -p com.apple.quarantine "$BIN" >/dev/null 2>&1; then
      result NOTE "terminal app: quarantine" "set: macOS asks before the first start"
    else
      result NOTE "terminal app: quarantine" "none (curl and Homebrew set none)"
    fi
  fi
  [ -n "$APP" ] || { result SKIP "desktop app: signature" "no Coxswain.app"; return; }
  if codesign --verify --deep --strict "$APP" 2>"$LOGS/codesign-app.log"; then
    result PASS "desktop app: signature valid (codesign --verify --deep --strict)"
  else
    result FAIL "desktop app: signature valid" "$(tail -1 "$LOGS/codesign-app.log")"
  fi
  out=$(codesign -dv --verbose=2 "$APP" 2>&1)
  printf '%s\n' "$out" >"$LOGS/codesign-dv.log"
  if printf '%s' "$out" | grep -q 'Authority=Developer ID Application'; then
    result PASS "desktop app: signed with a Developer ID" "$(printf '%s' "$out" | grep -m1 '^Authority=')"
  else
    result NOTE "desktop app: ad-hoc signed" "$(printf '%s' "$out" | grep -m1 -E '^Signature=|^TeamIdentifier=') (docs/systems/macos.md: builds are ad-hoc signed)"
  fi
  out=$(spctl -a -vv -t exec "$APP" 2>&1)
  printf '%s\n' "$out" >"$LOGS/spctl.log"
  if printf '%s' "$out" | grep -q 'accepted'; then
    result PASS "Gatekeeper (spctl -a -vv)" "$(printf '%s' "$out" | grep -m1 'source=')"
  else
    result NOTE "Gatekeeper (spctl -a -vv)" "rejected: not notarised, so a copy downloaded with a browser needs xattr -cr"
  fi
  if [ "$BREW" = 0 ]; then
    # What a browser download gets: a quarantined copy. Checked, never started.
    ditto "$APP" "$WORK/app-q/Coxswain.app" 2>/dev/null
    xattr -w com.apple.quarantine "0081;$(printf '%x' "$(date +%s)");Safari;" "$WORK/app-q/Coxswain.app"
    if spctl -a -t exec "$WORK/app-q/Coxswain.app" >/dev/null 2>&1; then
      result PASS "quarantined copy: Gatekeeper lets it start"
    else
      result NOTE "quarantined copy: Gatekeeper would stop it" "as the docs say (\"is damaged\")"
    fi
    xattr -cr "$WORK/app-q/Coxswain.app"
    if xattr -p com.apple.quarantine "$WORK/app-q/Coxswain.app" >/dev/null 2>&1; then
      result FAIL "xattr -cr removes the quarantine"
    else
      result PASS "xattr -cr removes the quarantine" "the docs' fix works"
    fi
    unregister "$WORK/app-q/Coxswain.app"
    rm -rf "$WORK/app-q"
  fi
}

# ---------------------------------------------------------------------------------------------
# 2. and 3. The terminal app

test_terminal() {
  section "Terminal app"
  if [ -z "$BIN" ]; then result SKIP "terminal app" "no terminal app to test (on Linux: --terminal-bin)"; return; fi
  if [ "$MAC" = 0 ] && [ -z "$TERM_BIN" ]; then result SKIP "terminal app" "the Mac binary does not run here"; return; fi
  local out want docs
  out=$(fake "$BIN" --version 2>&1 </dev/null)
  if [ "$out" = "coxswain $VERSION" ]; then result PASS "coxswain --version" "$out"
  elif [ -n "$TERM_BIN" ]; then result NOTE "coxswain --version" "$out (a local build)"
  else result FAIL "coxswain --version" "said \"$out\", want \"coxswain $VERSION\""; fi
  if [ -z "$TERM_BIN" ] && [ "$BREW" = 0 ]; then
    out=$(fake "$(dirname "$BIN")/cox" --version 2>&1 </dev/null)
    if [ "$out" = "coxswain $VERSION" ]; then result PASS "cox --version"; else result FAIL "cox --version" "$out"; fi
  fi

  # --help, word for word as docs/reference/command-line-flags.md shows it at that tag
  # (from a clone, with a local build: the clone's page).
  docs="$WORK/command-line-flags.md"
  local here; here=$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)
  if [ -n "$TERM_BIN" ] && [ -f "$here/../../docs/reference/command-line-flags.md" ]; then
    cp "$here/../../docs/reference/command-line-flags.md" "$docs"
  else
    curl -fsSL -o "$docs" "https://raw.githubusercontent.com/$REPO/v$VERSION/docs/reference/command-line-flags.md" || rm -f "$WORK/command-line-flags.md"
  fi
  if [ -s "$docs" ]; then
    awk '/`coxswain --help` prints:/ {f = 1; next} f && /^```/ {if (b) exit; b = 1; next} b {print}' "$docs" >"$WORK/help-docs.txt"
    fake "$BIN" --help >"$WORK/help-app.txt" 2>&1 </dev/null
    if [ -s "$WORK/help-docs.txt" ] && diff "$WORK/help-docs.txt" "$WORK/help-app.txt" >"$LOGS/help.diff"; then
      result PASS "coxswain --help matches the docs" "$(wc -l <"$WORK/help-app.txt" | tr -d ' ') lines"
    else
      result FAIL "coxswain --help matches the docs" "$(grep -c '^[<>]' "$LOGS/help.diff") lines differ: $(grep -m2 '^[<>]' "$LOGS/help.diff" | tr '\n' ' ')"
    fi
  else
    result SKIP "coxswain --help matches the docs" "could not fetch the docs page"
  fi

  # --paths: everything under the fake home.
  fake "$BIN" --paths >"$WORK/paths.txt" 2>&1 </dev/null
  out=$(grep -v -F "$WORK" "$WORK/paths.txt" | grep -v '^ *$')
  if [ -s "$WORK/paths.txt" ] && [ -z "$out" ]; then
    result PASS "coxswain --paths: all in the fake home" "$(wc -l <"$WORK/paths.txt" | tr -d ' ') places"
  else
    result FAIL "coxswain --paths: all in the fake home" "outside: $(md "$out")"
  fi
  want=$(fake "$BIN" --config-path 2>/dev/null </dev/null)
  if [ -f "$want" ] && grep -q 'Written by the Coxswain self-test' "$want"; then
    result PASS "config.toml read from the fake home" "${want#"$WORK"/}"
  else
    result FAIL "config.toml read from the fake home" "it reads $want"
  fi

  section "Terminal app, driven by keys"
  EXPECT=${EXPECT:-/usr/bin/expect}
  if ! [ -x "$EXPECT" ] && ! EXPECT=$(command -v expect 2>/dev/null); then
    result SKIP "driving the app by keys" "no expect"; search_files noexpect; return
  fi
  write_driver
  local scale=1; [ "$CI_MODE" = 1 ] && scale=2
  say "  (a pseudo-terminal of 120 x 40; each step has a time limit, so a hang shows as FAIL)"
  fake TERM=xterm-256color "$EXPECT" -f "$WORK/drive.exp" "$BIN" "$DEMO" "$RIGHT" "$WORK/tui.txt" "$LOGS" "$scale" \
    "$( [ -f "$DEMO/bundle.zip" ] && echo 1 || echo 0)" "$( [ -f "$DEMO/selftest.iso" ] && echo 1 || echo 0)" "$GIT_OK" \
    </dev/null >"$LOGS/expect.log" 2>&1 &
  local epid=$!
  PIDS="$PIDS $epid"
  # Show each result as it comes; N counts the lines shown.
  N=0
  while kill -0 "$epid" 2>/dev/null; do show_new; sleep 1; done
  wait "$epid" 2>/dev/null
  show_new
  if ! grep -q '^DONE' "$LOGS/expect.log"; then
    result FAIL "the key driver ran to the end" "$(tail -3 "$LOGS/expect.log" | tr '\n' ' ')"
  fi
  search_files
}

show_new() {
  local st name ms detail
  [ -f "$WORK/tui.txt" ] || return 0
  while IFS=$'\037' read -r st name ms detail; do
    N=$((N + 1)); result "$st" "$name" "$detail" "$ms"
  done < <(tail -n +$((N + 1)) "$WORK/tui.txt")
}

search_files() {
  section "Search helper"
  local idx db
  if [ "${1:-}" = noexpect ]; then
    result SKIP "name index and search store" "no expect: the terminal app was not started, so nothing built them"
    result SKIP "search by meaning" "the fake home has no model, and this test never downloads one"
    return
  fi
  idx=$(awk '/^name index/ {print $NF}' "$WORK/paths.txt" 2>/dev/null)
  db=$(awk '/^search store/ {print $NF}' "$WORK/paths.txt" 2>/dev/null)
  if [ -n "$idx" ] && [ -s "$idx" ]; then result PASS "name index built in the fake home" "$(du -h "$idx" | awk '{print $1}')"
  else result FAIL "name index built in the fake home" "no ${idx:-index.bin}"; fi
  if [ -n "$db" ] && [ -s "$db" ]; then result PASS "search store (text) built in the fake home" "$(du -h "$db" | awk '{print $1}')"
  else result FAIL "search store (text) built in the fake home" "no ${db:-search.db}"; fi
  result SKIP "search by meaning" "the fake home has no model, and this test never downloads one"
}

# The expect script: one spawn of the app per feature, the cursor put on its file by starting
# with that file (coxswain FILE RIGHT). The screen is the app's output with the escape sequences
# taken out; when a pattern does not show, the terminal is resized by one column and back, so
# the app draws the whole screen again.
write_driver() {
  cat >"$WORK/drive.exp" <<'EXP'
log_user 0
match_max 100000
lassign $argv bin demo right resfile logs scale havezip haveiso havegit
set res [open $resfile w]
set rows 40; set cols 120
set stty_init "rows $rows cols $cols"
array set key [list F1 "\033OP" F3 "\033OR" F5 "\033\[15~" F6 "\033\[17~" F7 "\033\[18~" \
  F10 "\033\[21~" F12 "\033\[24~" S-F8 "\033\[19;2~" M-F7 "\033\[18;3~" M-Enter "\033\r" \
  Enter "\r" Esc "\033" Tab "\t" Down "\033\[B" Space " " C-z "\032" C-g "\007" \
  M-c "\033c" M-b "\033b" M-k "\033k"]
set raw ""; set screen ""; set pid 0; set tname "first start"

proc strip {s} {
  regsub -all {\u001b\[[0-9;]*[HfABCDGd]} $s " " s
  regsub -all {\u001b\[[0-9;?<>=!]*[ -/]*[@-~]} $s "" s
  regsub -all {\u001b\][^\u0007\u001b]*(\u0007|\u001b\\)?} $s "" s
  regsub -all {\u001b[()*+][0-9A-Za-z]} $s "" s
  regsub -all {\u001b.} $s "" s
  regsub -all {[\u0000-\u0008\u000b-\u001f]} $s "" s
  return $s
}
proc nudge {} {
  global spawn_out rows cols
  catch {exec stty rows $rows cols [expr {$cols - 1}] < $spawn_out(slave,name)}
  after 300
  catch {exec stty rows $rows cols $cols < $spawn_out(slave,name)}
}
# ok, timeout or eof
proc wait_for {re secs} {
  global spawn_id raw screen scale
  set t0 [clock milliseconds]
  set deadline [expr {$t0 + int($secs * 1000 * $scale)}]
  set next [expr {$t0 + 1500}]
  while 1 {
    set screen [strip $raw]
    if {[regexp -nocase -- $re $screen]} { return ok }
    set now [clock milliseconds]
    if {$now >= $deadline} { return timeout }
    if {$now >= $next} { nudge; set next [expr {$now + 4000}] }
    set timeout 1
    expect {
      -re {.+} {
        append raw $expect_out(buffer)
        if {[string length $raw] > 400000} { set raw [string range $raw end-200000 end] }
      }
      timeout {}
      eof {
        catch {append raw $expect_out(buffer)}
        set screen [strip $raw]
        if {[regexp -nocase -- $re $screen]} { return ok }
        return eof
      }
    }
  }
}
proc save {name} {
  global logs screen
  regsub -all {[^A-Za-z0-9]+} $name - f
  set h [open "$logs/tui-$f.txt" w]
  regsub -all {[ \t]+} $screen " " s
  puts $h [string range $s end-3000 end]
  close $h
}
# one step: send keys, wait for what must show
proc s {label keys re secs} {
  global raw screen parts
  set raw ""
  set t [clock milliseconds]
  if {$keys ne ""} { send -- $keys }
  if {$re eq ""} { return }
  set r [wait_for $re $secs]
  set ms [expr {[clock milliseconds] - $t}]
  lappend parts [format "%s %.1f s" $label [expr {$ms / 1000.0}]]
  if {$r ne "ok"} {
    if {$r eq "eof"} {
      set why "the app ended"
    } elseif {[string trim $screen] eq ""} {
      set why "nothing drawn for [expr {int($secs * $::scale)}] s: hung?"
    } else {
      set why "the screen did not show /$re/ within [expr {int($secs * $::scale)}] s"
    }
    error "$label: $why"
  }
}
# a file system check, polled
proc fs {label cond secs} {
  global parts scale
  set t [clock milliseconds]
  set end [expr {$t + int($secs * 1000 * $scale)}]
  while {![uplevel #0 [list expr $cond]]} {
    if {[clock milliseconds] > $end} { error "$label: not so after $secs s" }
    after 200
  }
  lappend parts $label
}
proc launch {args} {
  global spawn_id spawn_out bin pid raw
  set raw ""
  set pid [eval [list spawn -noecho $bin] $args]
}
proc stop {} {
  global spawn_id pid
  if {$pid > 0} { catch {exec kill -9 $pid} }
  set pid 0
  catch {close}
  catch {wait}
}
proc quit_app {} {
  global spawn_id key parts
  send -- $key(Esc)
  after 400
  set t [clock milliseconds]
  send -- $key(F10)
  set timeout [expr {int(3 * $::scale)}]
  set again 0
  expect {
    eof { catch {wait}; set ::pid 0; lappend parts [format "F10 quits %.1f s%s" [expr {([clock milliseconds] - $t) / 1000.0}] [expr {$again ? " (the second F10)" : ""}]] }
    timeout {
      # An app still busy when Esc came reads Esc and F10 in one go, and an Esc right before
      # an escape sequence takes its first byte (crossterm), so F10 arrives as the text "[21~".
      # One more F10 on its own tells that from a hang.
      if {!$again} { set again 1; send -- $key(F10); set timeout [expr {int(10 * $::scale)}]; exp_continue }
      quit_logs; error "F10 did not end the app, twice (3 s, then 10 s): hung? (who holds its terminal: logs/quit-*.txt)"
    }
  }
}
# Why a quit did not end: whether the app still runs (and where it waits), and what else holds its
# terminal open, since expect sees the end only when the last holder lets go.
proc quit_logs {} {
  global spawn_out logs pid tname
  regsub -all {[^A-Za-z0-9]+} $tname - f
  set h [open "$logs/quit-$f.txt" w]
  catch {puts $h "app (pid $pid):\n[exec ps -o pid,ppid,pgid,stat,etime,command -p $pid]"} e1
  catch {puts $h "\nholding the terminal ($spawn_out(slave,name)):\n[exec lsof -w $spawn_out(slave,name)]"} e2
  catch {puts $h "\nterminal mode (icanon or echo here means raw mode was lost):\n[exec stty -a < $spawn_out(slave,name)]"} e4
  catch {puts $h "\nits children:\n[exec sh -c "ps -axo pid,ppid,pgid,stat,etime,command | awk -v p=$pid 'NR==1 || \$2==p || \$3==p'"]"} e3
  close $h
  # The screen as it is now, after Esc and F10 (save keeps the last screen a step waited for).
  wait_for {\u0000never} 3
  save "$tname after F10"
  if {$pid > 0} { catch {exec sample $pid 3 -file "$logs/quit-$f-sample.txt" 2>/dev/null} }
}
proc result {st name ms detail} {
  global res
  puts $res "$st\037$name\037$ms\037$detail"
  flush $res
}
# The panel learns of a git repository a moment after it starts; a git key pressed before says
# "Not in a git repository", so the git tests wait for the branch in the panel's frame first.
# a test: start on FILE, run BODY, quit with F10
proc t {name file body} {
  global parts right tname
  set tname $name
  set parts {}
  set t0 [clock milliseconds]
  if {[catch {
    launch $file $right
    s "start" "" {10\s*Quit} 20
    uplevel #0 $body
    quit_app
  } err]} {
    save $name
    stop
    result FAIL $name [expr {[clock milliseconds] - $t0}] "$err (screen: logs/tui-*.txt)"
  } else {
    result PASS $name [expr {[clock milliseconds] - $t0}] [join $parts ", "]
  }
}

# First start: the guide, skipped with Esc.
set parts {}
set t0 [clock milliseconds]
if {[catch {
  launch "$demo/notes.md" $right
  s "the first-run guide shows" "" {Two panels} 30
  s "Esc skips it" $key(Esc) {10\s*Quit} 10
  quit_app
} err]} {
  save "first start"; stop
  result FAIL "first start: the guide, then the panels" [expr {[clock milliseconds] - $t0}] $err
} else {
  result PASS "first start: the guide, then the panels" [expr {[clock milliseconds] - $t0}] [join $parts ", "]
}

t "F3 built-in viewer, / search in it" "$demo/notes.md" {
  s "F3 opens notes.md" $key(F3) {Line 1\D} 10
  send -- "/"; after 500
  s "/selftestzebra finds line 150" "selftestzebra\r" {OARLOCK} 10
  s "Esc closes the viewer" $key(Esc) {10\s*Quit} 10
}
t "F5 copy to the other panel" "$demo/copyme.txt" {
  s "F5 asks" $key(F5) {Copy "copyme\.txt"} 10
  s "Enter copies" $key(Enter) {Copied} 15
  fs "the copy is there" {[file exists $right/copyme.txt]} 10
}
t "F6 move, then Ctrl+Z undo" "$demo/moveme.txt" {
  s "F6 asks" $key(F6) {Move or rename "moveme\.txt"} 10
  s "Enter moves" $key(Enter) {Moved} 15
  fs "moved" {[file exists $right/moveme.txt] && ![file exists $demo/moveme.txt]} 10
  s "Ctrl+Z undoes" $key(C-z) {Undone} 15
  fs "back in place" {[file exists $demo/moveme.txt] && ![file exists $right/moveme.txt]} 10
}
t "F7 new folder" "$demo/notes.md" {
  s "F7 asks" $key(F7) {New folder} 10
  s "a name and Enter" "selftest-folder\r" {selftest-folder} 10
  fs "the folder is there" {[file isdirectory $demo/selftest-folder]} 10
}
# Not F8: on a Mac the Trash is your own, and undo cannot take things back out of it there.
t "Shift+F8 delete permanently" "$demo/deletemeforever.txt" {
  s "Shift+F8 asks" $key(S-F8) {Permanently delete "deletemeforever\.txt"} 10
  s "Enter deletes" $key(Enter) "" 0
  fs "the file is gone" {![file exists $demo/deletemeforever.txt]} 10
}
t "archive opened like a folder (tar.gz)" "$demo/bundle.tar.gz" {
  s "Enter opens it" $key(Enter) {\[archive\]} 15
  s "Down, Enter goes into its folder" "$key(Down)\r" {inside-archive\.txt} 10
}
if {$havezip} {
  t "archive opened like a folder (zip)" "$demo/bundle.zip" {
    s "Enter opens it" $key(Enter) {\[archive\]} 15
    s "Down, Enter goes into its folder" "$key(Down)\r" {inside-archive\.txt} 10
  }
} else { result SKIP "archive opened like a folder (zip)" "" "no zip made" }
if {$haveiso} {
  t "disk image: Alt+Enter Properties" "$demo/selftest.iso" {
    s "Alt+Enter names the format" $key(M-Enter) {ISO 9660} 15
  }
} else { result SKIP "disk image: Alt+Enter Properties" "" "no ISO made" }
if {$havegit} {
  t "Alt+C git config" "$demo/repo/README.md" {
    s "the panel shows the branch" "" {\smain\s} 10
    s "Alt+C shows the repository's config" $key(M-c) {Selftest Rower} 15
  }
  t "Alt+B git branches" "$demo/repo/README.md" {
    s "the panel shows the branch" "" {\smain\s} 10
    s "Alt+B lists the branches" $key(M-b) {Add the oars} 15
  }
  t "Ctrl+G git history" "$demo/repo/README.md" {
    s "the panel shows the branch" "" {\smain\s} 10
    s "Ctrl+G lists the file's commits" $key(C-g) {First} 15
  }
} else {
  foreach n {"Alt+C git config" "Alt+B git branches" "Ctrl+G git history"} { result SKIP $n "" "no git repository" }
}
t "Alt+K chess games in a PGN" "$demo/game.pgn" {
  s "Alt+K lists the games" $key(M-k) {Selftest Whi} 15
}
t "Space: preview" "$demo/notes.md" {
  s "Space answers (in the terminal app: use the desktop app)" $key(Space) {desktop app} 10
}
t "Alt+F7 Find a planted word" "$demo/notes.md" {
  s "Alt+F7 opens Find" $key(M-F7) {Type a name} 10
  s "quillwort is found in plants.txt" "quillwort" {boathouse} 120
}
t "F12 Settings" "$demo/notes.md" {
  s "F12 opens Settings" $key(F12) {Privacy and updates} 10
  s "Esc closes it" $key(Esc) {10\s*Quit} 10
}
t "F1 help, Tab Features" "$demo/notes.md" {
  s "F1 opens Help" $key(F1) {Keys \(from your config\)} 10
  s "Tab: Features and questions" $key(Tab) {Features and questions} 10
  s "Esc" $key(Esc) "" 0
  after 400
}
close $res
puts DONE
EXP
}

# ---------------------------------------------------------------------------------------------
# 4. The desktop app

SHOT_N=0 SHOT_LIST=""
shot() {
  [ "$SHOT_OK" = 1 ] || return 0
  SHOT_N=$((SHOT_N + 1))
  local f="$SHOTS/desktop-$SHOT_N.png"
  if screencapture -x "$f" 2>/dev/null && [ -s "$f" ]; then
    SHOT_LIST="$SHOT_LIST desktop-$SHOT_N.png"; result PASS "screenshot: $1" "desktop-$SHOT_N.png"
  else
    result SKIP "screenshot: $1" "screencapture failed (Screen Recording permission?)"
  fi
}
# start_gui LOG ARGS…: the desktop app in the background, its PID in GUI_PID
start_gui() {
  local log=$1; shift
  fake "$GUI_EXE" "$@" </dev/null >"$LOGS/$log" 2>&1 &
  GUI_PID=$!
  PIDS="$PIDS $GUI_PID"
}
# stop_gui NAME: SIGTERM, then wait up to 10 s
stop_gui() {
  local i=0
  kill "$GUI_PID" 2>/dev/null
  while kill -0 "$GUI_PID" 2>/dev/null && [ $i -lt 20 ]; do sleep 0.5; i=$((i + 1)); done
  if kill -0 "$GUI_PID" 2>/dev/null; then
    kill -9 "$GUI_PID" 2>/dev/null; result FAIL "$1: stops when asked (SIGTERM)" "still running after 10 s"
  else
    result PASS "$1: stops when asked (SIGTERM)" "" "$((i * 500))"
  fi
  wait "$GUI_PID" 2>/dev/null
}
# stays_up SECONDS: 0 when the app is still running then
stays_up() {
  local i=0
  while [ $i -lt "$1" ]; do kill -0 "$GUI_PID" 2>/dev/null || return 1; sleep 1; i=$((i + 1)); done
}

test_desktop() {
  section "Desktop app"
  if [ "$MAC" = 0 ]; then result SKIP "desktop app" "macOS only"; return; fi
  [ -n "$APP" ] || { result SKIP "desktop app" "no Coxswain.app"; return; }
  local exe out
  exe=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist" 2>/dev/null)
  GUI_EXE="$APP/Contents/MacOS/$exe"
  [ -x "$GUI_EXE" ] || { result FAIL "desktop app program" "no $GUI_EXE"; return; }
  out=$(fake "$GUI_EXE" --version 2>&1 </dev/null)
  if [ "$out" = "coxswain-gui $VERSION" ]; then result PASS "coxswain-gui --version" "$out"
  else result FAIL "coxswain-gui --version" "said \"$out\", want \"coxswain-gui $VERSION\""; fi

  start_gui gui.log "$DEMO" "$RIGHT"
  if stays_up 20; then
    result PASS "starts and stays up 20 s" "with the demo folder and right/"
    shot "the desktop app on the demo folder"
    stop_gui "desktop app"
  else
    result FAIL "starts and stays up 20 s" "ended: $(tail -2 "$LOGS/gui.log" | tr '\n' ' ')"
  fi

  start_gui gui-settings.log --settings=search "$DEMO"
  if stays_up 10; then
    result PASS "--settings=search starts with Settings open" "stays up 10 s"
    shot "Settings at Finding files"
    stop_gui "desktop app with Settings"
  else
    result FAIL "--settings=search starts with Settings open" "ended: $(tail -2 "$LOGS/gui-settings.log" | tr '\n' ' ')"
  fi

  [ "$NO_ASK" = 1 ] && { result SKIP "by-hand checklist" "--no-ask"; return; }
  by_hand
}

# ---------------------------------------------------------------------------------------------
# Drag and drop (--dnd): you drag, the test looks at the files. docs/files/drag-and-drop.md.

# dnd_step N TEXT CHECK...: say what to do, wait for Enter (s skips), then CHECK must hold
# within 5 s (a drop copies on a thread).
dnd_step() {
  local name=$1 text=$2 a i=0; shift 2
  printf '\n  %s\n      Enter when done, s to skip: ' "$text"
  read -r a </dev/tty || a=s
  case "$a" in [sS]*) result SKIP "$name" "skipped"; return ;; esac
  while ! "$@" && [ $i -lt 10 ]; do sleep 0.5; i=$((i + 1)); done
  if "$@"; then result PASS "$name" "the files say so"
  else result FAIL "$name" "not as expected: $(cd "$FHOME" && find demo right finder-in finder-out -maxdepth 2 2>/dev/null | sort | tr '\n' ' ')"; fi
}
# dnd_ask NAME TEXT: what only you can see, y/n/s
dnd_ask() {
  local a note
  printf '\n  %s\n      y/n/s? ' "$2"
  read -r a </dev/tty || a=s
  case "$a" in
    [yY]*) result PASS "$1" "by hand" ;;
    [nN]*) printf '      What happened? (Enter to leave it out) '; read -r note </dev/tty || note=""
      result FAIL "$1" "by hand${note:+: $note}" ;;
    *) result SKIP "$1" "skipped" ;;
  esac
}
# shellcheck disable=SC2329 # called through dnd_step
both() { [ -f "$1" ] && [ -f "$2" ]; }
# shellcheck disable=SC2329
moved() { [ ! -e "$1" ] && [ -f "$2" ]; }

test_dnd() {
  section "Drag and drop"
  if [ "$MAC" = 0 ]; then result SKIP "drag and drop" "macOS only"; return; fi
  [ -n "$APP" ] || { result SKIP "drag and drop" "no Coxswain.app"; return; }
  local exe
  exe=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist" 2>/dev/null)
  GUI_EXE="$APP/Contents/MacOS/$exe"
  [ -x "$GUI_EXE" ] || { result FAIL "desktop app program" "no $GUI_EXE"; return; }
  local IN=$FHOME/finder-in OUTF=$FHOME/finder-out
  mkdir -p "$IN" "$OUTF" "$DEMO/into"
  for f in dnd-option dnd-command dnd-menu dnd-folder dnd-esc; do echo "$f" >"$DEMO/$f.txt"; done
  echo "from Finder" >"$IN/from-finder.txt"
  echo "to Finder" >"$RIGHT/to-finder.txt"
  say "The desktop app opens with demo/ on the left and right/ on the right, and two Finder"
  say "windows: finder-in and finder-out (both in the test's temporary folder)."
  say "Skip the first-run guide (Esc) if it shows. Do each drag with the mouse or trackpad, then"
  say "press Enter here; the test checks the files."
  start_gui gui-dnd.log "$DEMO" "$RIGHT"
  sleep 3
  open "$IN" "$OUTF" 2>/dev/null || result NOTE "Finder windows" "could not open them: open $IN by hand"
  dnd_step "Option at the drop copies" "Drag dnd-option.txt from the left pane onto the right pane, holding Option as you let go." both "$DEMO/dnd-option.txt" "$RIGHT/dnd-option.txt"
  dnd_step "Command at the drop moves" "Drag dnd-command.txt from the left pane onto the right pane, holding Command as you let go." moved "$DEMO/dnd-command.txt" "$RIGHT/dnd-command.txt"
  dnd_step "the drop menu: c copies" "Drag dnd-menu.txt onto the right pane with no key held: a menu \"Drop ... in right\" opens; press c." both "$DEMO/dnd-menu.txt" "$RIGHT/dnd-menu.txt"
  dnd_step "onto a folder's row" "Drag dnd-folder.txt onto the row of the folder into/ in the left pane, holding Option." both "$DEMO/dnd-folder.txt" "$DEMO/into/dnd-folder.txt"
  dnd_step "Esc cancels a drag" "Start dragging dnd-esc.txt, press Esc while dragging, then let go over the right pane." moved "$RIGHT/dnd-esc.txt" "$DEMO/dnd-esc.txt"
  dnd_step "in from Finder" "Drag from-finder.txt from the Finder window finder-in onto the right pane, holding Option." both "$IN/from-finder.txt" "$RIGHT/from-finder.txt"
  dnd_step "out to Finder" "Drag to-finder.txt from the right pane into the Finder window finder-out." both "$RIGHT/to-finder.txt" "$OUTF/to-finder.txt"
  if [ -f "$DEMO/bundle.zip" ]; then
    dnd_step "out of an archive to Finder" "In the left pane open bundle.zip (Enter), then the folder bundle; drag inside-archive.txt into finder-out." test -f "$OUTF/inside-archive.txt"
  else
    result SKIP "out of an archive to Finder" "no zip archive"
  fi
  dnd_ask "the picture under the pointer" "While dragging, did a small picture under the pointer show the file's icon and name (or how many files)?"
  dnd_ask "the dashed outline" "While dragging over a pane or a folder's row, did it get a dashed outline?"
  say ""
  say "  Last: close the two Finder windows, and press Cmd+Q in the desktop app."
  local i=0
  while kill -0 "$GUI_PID" 2>/dev/null && [ $i -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
  if kill -0 "$GUI_PID" 2>/dev/null; then
    result NOTE "the app is still open" "stopped by the test"; stop_gui "desktop app, drag and drop"
  else
    result PASS "the app has ended" "nothing left running"
  fi
}

CHECKS="The window shows the demo folder (left) and right/ (right); the first-run guide can be skipped
notes.md: Space shows the preview pane with the Markdown formatted (heading, bold, italics)
main.rs and script.py: the preview shows the code in syntax colours
picture.png and photo.jpg: the preview shows the picture
report.pdf: the preview shows its first page
crew.csv: the preview shows a table; data.json a tree
bundle.tar.gz and bundle.zip: Enter opens each like a folder, inside-archive.txt previews
selftest.iso: the preview names ISO 9660 and its size
game.pgn: Alt+K (or the preview) shows the chess board, and the moves step forward and back
repo: Alt+B lists main and feature/oars; Ctrl+G on README.md lists its commits
Find (Alt+F7 or Ctrl+F), quillwort: plants.txt under In files
Settings (Ctrl+,) opens; Finding files shows the index of this fake home (do NOT turn on Start with my session)
F1, then Features: the list of features with their questions
Clouds in the sidebar: your iCloud Drive / OneDrive / Dropbox, if you linked them (look, do not open files)
Cmd+Q quits the app"

by_hand() {
  section "Desktop app, by hand"
  say "Now the desktop app opens again with the fake home, for you to try a few things."
  say "${C_B}Do NOT turn on \"Start with my session\" (Settings -> Finding files) in this test copy:${C_0}"
  say "it would register the test's search helper with your session's launchd."
  say "Answer each with y (works), n (does not), or s (skip); after an n you can add a word."
  CLOUDS=0
  if [ -d "$REAL_HOME/Library/CloudStorage" ] || [ -d "$REAL_HOME/Library/Mobile Documents" ]; then
    say ""
    say "To check Clouds in the sidebar, the test can link your cloud folders (~/Library/CloudStorage,"
    say "iCloud Drive) into the fake home. Coxswain only reads their names; the links are removed at the"
    say "end, your files are not touched. macOS may ask Terminal for access to them."
    if ask "Link your cloud folders for this test?" n; then
      mkdir -p "$FHOME/Library"
      [ -d "$REAL_HOME/Library/CloudStorage" ] && ln -s "$REAL_HOME/Library/CloudStorage" "$FHOME/Library/CloudStorage"
      [ -d "$REAL_HOME/Library/Mobile Documents" ] && ln -s "$REAL_HOME/Library/Mobile Documents" "$FHOME/Library/Mobile Documents"
      CLOUDS=1
    fi
  fi
  start_gui gui-hand.log "$DEMO" "$RIGHT"
  sleep 3
  local line a note n=0
  while IFS= read -r line; do
    n=$((n + 1))
    if [ $n = 14 ] && [ "$CLOUDS" = 0 ]; then result SKIP "$line" "cloud folders not linked"; continue; fi
    if [ $n = 15 ]; then say "  Last: press Cmd+Q in the desktop app."; fi
    printf '  %2d. %s\n      y/n/s? ' "$n" "$line"
    read -r a </dev/tty || a=s
    case "$a" in
      [yY]*) result PASS "$line" "by hand" ;;
      [nN]*) printf '      What happened? (Enter to leave it out) '; read -r note </dev/tty || note=""
        result FAIL "$line" "by hand${note:+: $note}" ;;
      *) result SKIP "$line" "skipped" ;;
    esac
    if [ $n = 1 ] && [ "$SHOT_OK" = 1 ]; then shot "by hand, first look"; fi
  done <<EOF
$CHECKS
EOF
  local i=0
  while kill -0 "$GUI_PID" 2>/dev/null && [ $i -lt 20 ]; do sleep 0.5; i=$((i + 1)); done
  if kill -0 "$GUI_PID" 2>/dev/null; then
    result NOTE "the app is still open" "stopped by the test"; stop_gui "desktop app, by hand"
  else
    result PASS "the app has ended" "nothing left running"
  fi
  rm -f "$FHOME/Library/CloudStorage" "$FHOME/Library/Mobile Documents"
}

# ---------------------------------------------------------------------------------------------
# 5. Report and clean-up

finish() {
  section "Clean-up"
  local p m touched="" appeared=""
  while IFS='	' read -r p m; do
    [ -n "$p" ] && [ "$(mtime "$REAL_HOME/$p")" != "$m" ] && touched="$touched ~/$p"
  done <<EOF
$BEFORE_LIST
EOF
  # Only reported, never removed: the line to run is in the report and on screen.
  while IFS= read -r p; do
    [ -n "$p" ] && [ -e "$REAL_HOME/$p" ] && ! was_there "$p" && appeared="$appeared
$p"
  done <<EOF
$SYS_PATHS
$OWN_PATHS
EOF
  if [ -n "$appeared" ]; then
    while IFS= read -r p; do
      [ -n "$p" ] || continue
      if printf '%s\n' "$SYS_PATHS" | grep -Fxq "$p"; then
        result NOTE "macOS made ~/$p for the test app" "left as it is; to remove it: rm -rf ~/'$p'"
      else
        result FAIL "Coxswain made ~/$p in your real home" "it should have used the fake home; left as it is; to remove it: rm -rf ~/'$p'"
      fi
    done <<EOF
$appeared
EOF
  else
    result PASS "nothing of Coxswain's appeared in your real home"
  fi
  launchd_check row
  if [ -n "$touched" ]; then result NOTE "your own Coxswain files changed meanwhile" "$touched (your running Coxswain, or this test)"
  else result PASS "your own Coxswain files untouched" "${BEFORE_LIST:+$(printf '%s' "$BEFORE_LIST" | grep -c .) places checked}"; fi
  if [ -n "$BREW_NEW" ]; then
    if ask "Remove what this test installed with Homebrew ($BREW_NEW )?" y; then
      case "$BREW_NEW" in *cask*) brew uninstall --cask coxswain-gui >/dev/null 2>&1 ;; esac
      case "$BREW_NEW" in *formula*) brew uninstall coxswain >/dev/null 2>&1 ;; esac
      case "$BREW_NEW" in *tap*) brew untap mwo-dk/coxswain >/dev/null 2>&1 ;; esac
      result PASS "Homebrew: removed again" "$BREW_NEW"
    else
      result NOTE "Homebrew: left installed" "$BREW_NEW"
    fi
  fi

  local stamp report dir mac_os model mem
  stamp=$(date +%Y-%m-%d-%H%M)
  if [ "$MAC" = 1 ]; then
    mac_os="macOS $(sw_vers -productVersion 2>/dev/null) ($(sw_vers -buildVersion 2>/dev/null))"
    model=$(sysctl -n hw.model 2>/dev/null); mem="$(($(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1073741824)) GB"
  else
    mac_os="$(uname -sr) (dry run)"; model=$(uname -m); mem=""
  fi
  report="$WORK/report.md"
  {
    echo "# Coxswain $VERSION self-test on macOS"
    echo
    echo "| | |"
    echo "|---|---|"
    echo "| Date | $(date '+%Y-%m-%d %H:%M %Z') |"
    echo "| System | $mac_os, $(uname -m), $model${mem:+, $mem} |"
    echo "| Route | $([ "$BREW" = 1 ] && echo Homebrew || echo "GitHub release downloads")$([ -n "$TERM_BIN" ] && echo ", terminal app $TERM_BIN")$([ "$CI_MODE" = 1 ] && echo ", CI") |"
    echo "| Result | $NPASS passed, $NFAIL failed, $NSKIP skipped, $NNOTE notes |"
    echo
    if [ -n "$FAILED_NAMES" ]; then echo "## Failed"; printf '%s\n\n' "$FAILED_NAMES"; fi
    echo "## Every item"
    echo
    echo "| Result | Part | Item | Time | Detail |"
    echo "|---|---|---|---|---|"
    cat "$ROWS"
    for p in "$LOGS"/tui-*.txt "$LOGS/help.diff"; do
      [ -s "$p" ] || continue
      echo; echo "### $(basename "$p")"; echo; echo '```'; head -c 3000 "$p"; echo; echo '```'
    done
    if [ -n "$SHOT_LIST" ]; then echo; echo "## Screenshots"; for p in $SHOT_LIST; do echo; echo "![$p]($p)"; done; fi
  } >"$report"
  # Nothing personal in it: the real home is written as ~.
  LC_ALL=C sed "s#$REAL_HOME#~#g" "$report" >"$report.tmp" && mv "$report.tmp" "$report"

  dir=""
  if [ -n "$OUT" ]; then dir=$OUT
  elif [ "$MAC" = 1 ] && [ -d "$REAL_HOME/Desktop" ] && ask "Save the report to your Desktop (coxswain-selftest-$stamp.md)?" y; then
    dir="$REAL_HOME/Desktop"
  fi
  # The pictures' links follow the names they get there.
  LC_ALL=C sed "s#(desktop-#(coxswain-selftest-$stamp-desktop-#" "$report" >"$WORK/report-out.md"
  if [ -n "$dir" ] && cp "$WORK/report-out.md" "$dir/coxswain-selftest-$stamp.md" 2>/dev/null; then
    for p in $SHOT_LIST; do cp "$SHOTS/$p" "$dir/coxswain-selftest-$stamp-$p" 2>/dev/null; done
    REPORT="$dir/coxswain-selftest-$stamp.md"
  else
    KEEP=1; REPORT=$report
  fi
  if [ "$CI_MODE" = 1 ] && [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then cat "$REPORT" >>"$GITHUB_STEP_SUMMARY"; fi
  [ "$CI_MODE" = 1 ] && cp -R "$LOGS" "$OUT/logs" 2>/dev/null

  printf '\n%s== Summary ==%s\n' "$C_B" "$C_0"
  printf '  %s%d passed%s, %s%d failed%s, %s%d skipped%s, %s%d notes%s\n' \
    "$C_PASS" "$NPASS" "$C_0" "$C_FAIL" "$NFAIL" "$C_0" "$C_SKIP" "$NSKIP" "$C_0" "$C_NOTE" "$NNOTE" "$C_0"
  [ -n "$FAILED_NAMES" ] && printf 'Failed:%s\n' "$FAILED_NAMES"
  say "Report: ${REPORT/#$REAL_HOME/~}"
  say "It stays on this Mac: nothing was sent anywhere."
}

main
cleanup
trap - EXIT
[ "$CI_MODE" = 1 ] && [ "$NFAIL" -gt 0 ] && exit 1
exit 0
