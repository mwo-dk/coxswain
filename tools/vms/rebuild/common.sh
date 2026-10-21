# Shared by the rebuild scripts in tools/vms/rebuild (sourced, POSIX sh).
# Expects: NAME (the VM's folder name) and IDX (0 FreeBSD, 1 OpenBSD, 2 NetBSD, 3 OmniOS: the
# offset of its ports). Settings, from the environment (tools/vms/README.md):
#   COXSWAIN_VMS            the folder that holds the VMs (default: $XDG_DATA_HOME/coxswain-vms)
#   COXSWAIN_VM_PORT_BASE   ssh port of the first VM on 127.0.0.1 (default 2222; the others +1 ...)
#   COXSWAIN_VM_VNC_BASE    VNC display of the first VM (default 1, so port 5901; the others +1 ...)
#   COXSWAIN_VM_HTTP_PORT   port of the short-lived install server on 127.0.0.1 (default 8080)
set -eu
VMS=${COXSWAIN_VMS:-${XDG_DATA_HOME:-$HOME/.local/share}/coxswain-vms}
PORT_BASE=${COXSWAIN_VM_PORT_BASE:-2222}
VNC_BASE=${COXSWAIN_VM_VNC_BASE:-1}
HTTP_PORT=${COXSWAIN_VM_HTTP_PORT:-8080}
PORT=$((PORT_BASE + IDX))
VNC=$((VNC_BASE + IDX))
REBUILD=$(cd "$(dirname "$0")" && pwd)
CACHE=$VMS/cache
DIR=$VMS/$NAME
SERIAL="python3 -I $REBUILD/serial.py"
say() { printf '\n== %s: %s\n' "$NAME" "$*"; }
die() { printf '%s: %s\n' "$NAME" "$*" >&2; exit 1; }

running() { [ -e "$DIR/qemu.pid" ] && kill -0 "$(cat "$DIR/qemu.pid")" 2>/dev/null; }

# UEFI firmware (FreeBSD's image and OmniOS boot with it): OVMF_CODE and OVMF_VARS, or the
# first found of Arch's, Debian's and Ubuntu's, and Fedora's places.
ovmf() {
  for c in "${OVMF_CODE:-}" /usr/share/edk2/x64/OVMF_CODE.4m.fd /usr/share/OVMF/OVMF_CODE_4M.fd \
    /usr/share/edk2/ovmf/OVMF_CODE.fd /usr/share/OVMF/OVMF_CODE.fd; do
    [ -n "$c" ] && [ -f "$c" ] && { CODE=$c; break; }
  done
  for v in "${OVMF_VARS:-}" /usr/share/edk2/x64/OVMF_VARS.4m.fd /usr/share/OVMF/OVMF_VARS_4M.fd \
    /usr/share/edk2/ovmf/OVMF_VARS.fd /usr/share/OVMF/OVMF_VARS.fd; do
    [ -n "$v" ] && [ -f "$v" ] && { VARS=$v; break; }
  done
  [ -n "${CODE:-}" ] && [ -n "${VARS:-}" ] || die "no UEFI firmware (OVMF): install edk2-ovmf (or ovmf), or set OVMF_CODE and OVMF_VARS"
}

# Refuse to run over a VM that is up or over a folder these scripts did not make; make the
# folder, its key and its password (both kept across rebuilds).
prepare() {
  if [ -d "$DIR" ] && [ ! -e "$DIR/.made-by-tools-vms" ] && [ -n "$(ls -A "$DIR")" ]; then
    die "$DIR was not made by these scripts: not touching it (move it away, or set COXSWAIN_VMS elsewhere)"
  fi
  mkdir -p "$DIR"
  running && die "the VM is running: stop it first ($DIR/stop.sh)"
  touch "$DIR/.made-by-tools-vms"
  [ -f "$DIR/id_ed25519" ] || ssh-keygen -q -t ed25519 -N '' -C "test@$NAME" -f "$DIR/id_ed25519"
  if [ ! -s "$DIR/password" ]; then
    (umask 077; openssl rand -base64 18 | tr -d '/+=\n' >"$DIR/password")
  fi
  USER_PW=$(cat "$DIR/password")
  rm -f "$DIR/known_hosts" "$DIR/serial.sock" "$DIR/monitor.sock" "$DIR/qemu.pid"
  cat >"$DIR/rebuild.sh" <<EOF
#!/bin/sh
COXSWAIN_VMS='$VMS' COXSWAIN_VM_PORT_BASE=$PORT_BASE COXSWAIN_VM_VNC_BASE=$VNC_BASE exec '$REBUILD/$NAME.sh' "\$@"
EOF
  chmod +x "$DIR/rebuild.sh"
}

# fetch URL FILE: download into the cache unless it is there already.
fetch() {
  [ -s "$2" ] && return 0
  mkdir -p "$(dirname "$2")"
  curl -fL --retry 3 -o "$2.part" "$1" && mv "$2.part" "$2"
}

# A short-lived HTTP server on 127.0.0.1:$HTTP_PORT (10.0.2.2:$HTTP_PORT from the guest) serving $1.
HTTP_PID=
http_serve() { python3 -I -m http.server --bind 127.0.0.1 --directory "$1" "$HTTP_PORT" >/dev/null 2>&1 & HTTP_PID=$!; }
cleanup() {
  [ -n "$HTTP_PID" ] && kill "$HTTP_PID" 2>/dev/null || true
  if running; then echo quit | socat - "UNIX-CONNECT:$DIR/monitor.sock" >/dev/null 2>&1 || kill "$(cat "$DIR/qemu.pid")"; fi
  return 0
}
trap cleanup EXIT INT TERM

ssh_as() { u=$1; shift; ssh -i "$DIR/id_ed25519" -p "$PORT" -o UserKnownHostsFile="$DIR/known_hosts" \
  -o StrictHostKeyChecking=accept-new -o BatchMode=yes -o ConnectTimeout=5 -o IdentitiesOnly=yes -F /dev/null "$u@127.0.0.1" "$@"; }
wait_ssh() { i=0; until ssh_as "$1" true 2>/dev/null; do i=$((i + 1)); [ $i -gt 120 ] && die "no ssh after 10 minutes"; sleep 5; done; }

# Wait for qemu to exit (the guest powered off), at most $1 seconds.
wait_off() { i=0; while running; do i=$((i + 1)); [ $i -gt "$1" ] && die "the guest did not power off"; sleep 1; done; rm -f "$DIR/qemu.pid"; }

# README.txt: write_readme "<title line>" "<how it was made>" "<notes>"
write_readme() {
  cat >"$DIR/README.txt" <<EOF
$1
Throwaway test machine for Coxswain (BSD / illumos testing). Made by tools/vms/rebuild/$NAME.sh.

Made from
$2

Logins
  user  test   password  in ./password   (passwordless doas/sudo)
  user  root   the same password
  ssh   ./ssh.sh  or  ./ssh.sh command      (key id_ed25519 in this folder, port $PORT on localhost)
        or: ssh -i id_ed25519 -p $PORT -o IdentitiesOnly=yes test@127.0.0.1

Start / stop
  ./start.sh                  headless, qemu runs in the background; wait ~30-60 s for ssh
  VM_DISPLAY=gtk ./start.sh   a window on this desktop instead
  ./stop.sh                   ACPI power button, waits up to 90 s (then pulls the plug)
  4 vCPUs, 4 GB RAM, KVM, virtio disk and network, user-mode networking (IPv4 only).
  VNC: localhost:$((5900 + VNC)) (display :$VNC)
  Monitor: socat - UNIX-CONNECT:monitor.sock     Console output: serial.log

Tests
  $(dirname "$REBUILD")/test-in.sh <system> [checkout] [-- cargo args]
      copies a checkout in and runs cargo there

Throw away / get back
  Throw away: ./stop.sh; rm -rf $DIR     (or only the disk image)
  Get back:   ./rebuild.sh     (unattended, ~10-20 min; the image comes from the cache, so it
              downloads only packages; a new key and password are made if the folder went)

$3
EOF
}

# ssh.sh and stop.sh, the same for every VM.
write_common_scripts() {
  cat >"$DIR/ssh.sh" <<EOF
#!/bin/sh
# ssh into the VM as test (doas/sudo without a password):  ./ssh.sh   or  ./ssh.sh command
cd "\$(dirname "\$0")" && exec ssh -i id_ed25519 -p $PORT -o UserKnownHostsFile=known_hosts -o StrictHostKeyChecking=accept-new -o IdentitiesOnly=yes -F /dev/null test@127.0.0.1 "\$@"
EOF
  cat >"$DIR/stop.sh" <<'EOF'
#!/bin/sh
# Stop the VM: ACPI power button, then wait up to 90 s (then pull the plug).
cd "$(dirname "$0")" || exit 1
pid=$(cat qemu.pid 2>/dev/null); [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null || { echo "Not running."; exit 0; }
echo system_powerdown | socat - UNIX-CONNECT:monitor.sock >/dev/null
i=0; while kill -0 "$pid" 2>/dev/null; do
  i=$((i + 1)); [ $i -ge 90 ] && { echo "No clean shutdown after 90 s: quitting qemu."; echo quit | socat - UNIX-CONNECT:monitor.sock >/dev/null; break; }
  sleep 1
done
rm -f qemu.pid; echo "Stopped."
EOF
  chmod +x "$DIR/ssh.sh" "$DIR/stop.sh"
}

# start.sh: write_start "<title>" "<extra qemu lines, each ending in ' \\' and a newline>"
# The disk is the file named in DISK; a seed.img next to it (cloud-init) goes in while it is
# there, writable: FreeBSD mounts it read-write.
write_start() {
  cat >"$DIR/start.sh" <<EOF
#!/bin/sh
# Start the $1 VM.
#   ./start.sh                 no window, runs in the background (ssh / VNC)
#   VM_DISPLAY=gtk ./start.sh  a window on this desktop (stays in front)
# ssh: ./ssh.sh (test@localhost:$PORT)   VNC: localhost:$((5900 + VNC))   Stop: ./stop.sh
cd "\$(dirname "\$0")" || exit 1
if [ -e qemu.pid ] && kill -0 "\$(cat qemu.pid)" 2>/dev/null; then echo "Already running (pid \$(cat qemu.pid))."; exit 1; fi
D=\${VM_DISPLAY:-none}; BG=; [ "\$D" = none ] && BG=-daemonize
SER=\${VM_SERIAL:-file:serial.log}
SEED=; [ -e seed.img ] && SEED="-drive file=seed.img,format=raw,if=virtio"
exec qemu-system-x86_64 -name $NAME -machine q35,accel=kvm -cpu host -smp 4 -m 4G \\
$2  -drive file=$DISK,format=raw,if=virtio,cache=writeback,discard=unmap \$SEED \\
  -netdev user,id=n0,ipv6=off,hostfwd=tcp:127.0.0.1:$PORT-:22 -device virtio-net-pci,netdev=n0 \\
  -device qemu-xhci -device usb-tablet -vga std -display "\$D" -vnc 127.0.0.1:$VNC \\
  -monitor unix:monitor.sock,server,nowait -serial "\$SER" -pidfile qemu.pid \$BG
EOF
  chmod +x "$DIR/start.sh"
}

# seed.img: a NoCloud seed for cloud-init (OmniOS) and nuageinit (FreeBSD), a FAT disk labelled
# CIDATA holding user-data ($1) and meta-data ($2).
make_seed() {
  S=$DIR/.seed; rm -rf "$S"; mkdir -p "$S"
  printf '%s\n' "$1" >"$S/user-data"; printf '%s\n' "$2" >"$S/meta-data"
  rm -f "$DIR/seed.img"; truncate -s 2M "$DIR/seed.img"
  mformat -i "$DIR/seed.img" -v CIDATA ::
  mcopy -i "$DIR/seed.img" "$S/user-data" "$S/meta-data" ::
  rm -rf "$S"
}
