#!/bin/sh
# Run Coxswain's tests in one of the BSD / illumos VMs, before CI does.
#
#   tools/vms/test-in.sh <freebsd|openbsd|netbsd|omnios> [checkout] [-- cargo args]
#
#   checkout     the Coxswain checkout to test (default: the current folder)
#   cargo args   what to run instead of the default (the CI's package set for that system):
#                  freebsd, openbsd: test -p coxswain-core -p coxswain -p coxswain-gui --locked
#                  netbsd, omnios:   test -p coxswain-core -p coxswain --locked -j2
#                                    (-j2: with 4 jobs in 4 GB, rustc runs out of memory on
#                                    OmniOS and NetBSD stalls in its swap file)
#                e.g.  tools/vms/test-in.sh openbsd . -- test --workspace --locked
#                      tools/vms/test-in.sh netbsd -- clippy -p coxswain --locked
#
# Starts the VM headless if it is not running (and stops it afterwards only then), copies the
# worktree to ~/coxswain-src in the VM (rsync when the VM has it, else tar; without target/,
# node_modules/ and .git; gui/dist goes along when built, else an empty one is made), runs
# cargo there with a target folder that stays in the VM (~/coxswain-target, so later runs are
# incremental), streams the output and exits with cargo's status.
# The VMs are in $COXSWAIN_VMS (default $XDG_DATA_HOME/coxswain-vms), made by tools/vms/rebuild/;
# a VM is used only through its folder's start.sh, ssh.sh and stop.sh, so a folder made by hand
# works too when it has them.
set -eu
VMS=${COXSWAIN_VMS:-${XDG_DATA_HOME:-$HOME/.local/share}/coxswain-vms}
TOOLS=$(cd "$(dirname "$0")" && pwd)
case "${1:-}" in
  freebsd) DIR=$VMS/freebsd-15.1 DEF="test -p coxswain-core -p coxswain -p coxswain-gui --locked" ;;
  openbsd) DIR=$VMS/openbsd-7.9 DEF="test -p coxswain-core -p coxswain -p coxswain-gui --locked" ;;
  netbsd) DIR=$VMS/netbsd-10.1 DEF="test -p coxswain-core -p coxswain --locked -j2" ;;
  omnios) DIR=$VMS/omnios DEF="test -p coxswain-core -p coxswain --locked -j2" ;;
  *) sed -n '2,/^set -eu/p' "$0" | sed -e '$d' -e 's/^# \{0,1\}//'; exit 2 ;;
esac
shift
SRC=.
if [ $# -gt 0 ] && [ "$1" != -- ]; then SRC=$1; shift; fi
[ "${1:-}" = -- ] && shift
[ $# -gt 0 ] && ARGS="$*" || ARGS=$DEF
SRC=$(cd "$SRC" && pwd)
[ -f "$SRC/Cargo.toml" ] || { echo "$SRC has no Cargo.toml: not a Coxswain checkout?" >&2; exit 2; }
[ -x "$DIR/ssh.sh" ] || { echo "$DIR/ssh.sh is missing: make the VM first: $TOOLS/rebuild/$(basename "$DIR").sh." >&2; exit 2; }

# A guest that stops answering for 2 minutes ends the run (instead of hanging here).
vm() { "$DIR/ssh.sh" -o BatchMode=yes -o ConnectTimeout=5 -o ServerAliveInterval=15 -o ServerAliveCountMax=8 "$@"; }
up() { [ -e "$DIR/qemu.pid" ] && kill -0 "$(cat "$DIR/qemu.pid" 2>/dev/null)" 2>/dev/null; }

STARTED=0
if ! up; then
  echo "== starting $(basename "$DIR") (headless)"
  # A start.sh may stay in the foreground (one made by hand) or put qemu in the background.
  VM_DISPLAY=none nohup "$DIR/start.sh" >/dev/null 2>&1 9>&- &
  STARTED=1
fi
finish() {
  if [ "$STARTED" = 1 ]; then echo "== stopping $(basename "$DIR")"; "$DIR/stop.sh" >/dev/null 2>&1 || true; fi
}
trap finish EXIT
trap 'exit 130' INT TERM
i=0; until vm true 2>/dev/null; do
  i=$((i + 1)); [ $i -gt 60 ] && { echo "No ssh after 5 minutes." >&2; exit 1; }; sleep 5
done

echo "== copying $SRC"
if vm 'command -v rsync' >/dev/null 2>&1; then
  # rsync calls "<-e> <host> rsync --server ...": the wrapper drops the host and goes through ssh.sh.
  rsync -az --timeout=120 --delete --exclude /target/ --exclude node_modules/ --exclude /.git/ \
    --exclude /.worktrees/ -e "sh -c 'shift; exec \"$DIR/ssh.sh\" -o BatchMode=yes \"\$@\"' ssh" \
    "$SRC/" vm:coxswain-src/
else
  vm 'rm -rf coxswain-src && mkdir coxswain-src'
  (cd "$SRC" && tar -cf - --exclude ./target --exclude node_modules --exclude ./.git \
    --exclude ./.worktrees .) | vm 'cd coxswain-src && tar -xf -'
fi

# The desktop app embeds gui/dist (cd gui && npm run build); without it an empty one lets it compile.
if [ ! -d "$SRC/gui/dist" ]; then
  echo "== no gui/dist in $SRC: an empty one in the VM (build the frontend first for a real desktop app)"
  vm 'mkdir -p coxswain-src/gui/dist'
fi
echo "== cargo $ARGS"
set +e
vm "PATH=\$PATH:/usr/local/bin:/usr/pkg/bin:/opt/ooce/bin; export PATH; cd coxswain-src && CARGO_TARGET_DIR=\$HOME/coxswain-target cargo $ARGS"
rc=$?
echo "== exit status $rc"
exit $rc
