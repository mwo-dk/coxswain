#!/usr/bin/env bash
# Run a command as user "demo" with a throwaway home, for screenshots that show nothing
# personal. The real /home, /tmp, /mnt and /run/media are hidden, the host name is "demo",
# and the environment is cleared; podman and docker cannot run in it. Needs bubblewrap (bwrap)
# and a release build.
#
#   docs/screenshots/sandbox.sh coxswain-gui
#   docs/screenshots/sandbox.sh alacritty -o font.normal.family="MesloLGM Nerd Font Mono" -e coxswain
set -euo pipefail
repo="$(cd "$(dirname "$0")/../.." && pwd)"
demo="${DEMO_HOME:-${XDG_RUNTIME_DIR:-/tmp}/coxswain-demo-home}"
[ -d "$demo" ] || "$repo/docs/screenshots/demo-home.sh" "$demo"

# No containers in here: a podman run in the sandbox (a preview's tool, Settings listing the
# images) starts its pause process in the sandbox's namespaces and points the user's rootless
# podman at it. Every podman and docker on the PATH is covered with an empty file.
no_containers=()
for b in $(type -ap podman docker 2>/dev/null) /usr/bin/podman /usr/bin/docker; do
  [ -e "$b" ] && no_containers+=(--ro-bind /dev/null "$(readlink -f "$b")")
done

exec bwrap \
  --ro-bind / / --dev /dev --proc /proc \
  "${no_containers[@]}" \
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
