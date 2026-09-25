#!/usr/bin/env bash
# Run a command as user "demo" with a throwaway home, for screenshots that show nothing
# personal. The real /home, /tmp, /mnt and /run/media are hidden, the host name is "demo",
# and the environment is cleared. Needs bubblewrap (bwrap) and a release build.
#
#   docs/screenshots/sandbox.sh coxswain-gui
#   docs/screenshots/sandbox.sh alacritty -o font.normal.family="MesloLGM Nerd Font Mono" -e coxswain
set -euo pipefail
repo="$(cd "$(dirname "$0")/../.." && pwd)"
demo="${DEMO_HOME:-${XDG_RUNTIME_DIR:-/tmp}/coxswain-demo-home}"
[ -d "$demo" ] || "$repo/docs/screenshots/demo-home.sh" "$demo"

exec bwrap \
  --ro-bind / / --dev /dev --proc /proc \
  --tmpfs /tmp --tmpfs /home --tmpfs /mnt --tmpfs /run/media --tmpfs /opt \
  --bind "$demo" /home/demo \
  --ro-bind "$repo/target/release" /opt/coxswain \
  --bind "$XDG_RUNTIME_DIR" "$XDG_RUNTIME_DIR" \
  --unshare-uts --hostname demo \
  --chdir /home/demo \
  --clearenv \
  --setenv HOME /home/demo --setenv USER demo --setenv LOGNAME demo \
  --setenv PATH "/opt/coxswain:/usr/local/bin:/usr/bin:/bin" \
  --setenv LANG "${LANG:-C.UTF-8}" --setenv TERM xterm-256color \
  --setenv XDG_RUNTIME_DIR "$XDG_RUNTIME_DIR" \
  --setenv WAYLAND_DISPLAY "${WAYLAND_DISPLAY:-}" --setenv DISPLAY "${DISPLAY:-}" \
  "$@"
