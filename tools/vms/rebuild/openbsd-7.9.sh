#!/bin/sh
# Rebuild the OpenBSD 7.9 test VM from scratch, unattended (about 15 minutes).
# Downloads miniroot79.img into the cache once (SHA-256 and signify checked), installs over the
# network from cdn.openbsd.org with autoinstall(8), adds packages and Coxswain, leaves it stopped.
# The disk is wiped; the folder's ssh key and password are kept (made when missing).
NAME=openbsd-7.9 IDX=1
. "$(dirname "$0")/common.sh"
prepare
C=$CACHE/openbsd-7.9 U=https://cdn.openbsd.org/pub/OpenBSD/7.9/amd64

say "image"
fetch $U/miniroot79.img "$C/miniroot79.img"
fetch $U/SHA256.sig "$C/SHA256.sig"
# The release key, from OpenBSD's source tree (mirrored on GitHub): signify -C without signify.
fetch https://raw.githubusercontent.com/openbsd/src/master/etc/signify/openbsd-79-base.pub "$C/openbsd-79-base.pub"
uv run -q --with cryptography python -I "$REBUILD/signify-verify.py" "$C/openbsd-79-base.pub" "$C/SHA256.sig" >"$C/SHA256"
(cd "$C" && sha256sum -c --ignore-missing SHA256) | grep -q 'miniroot79.img: OK' || die "miniroot79.img does not match the signed SHA256"
echo "miniroot79.img: signature and SHA-256 OK"

say "install (autoinstall over serial, sets from cdn.openbsd.org)"
rm -f "$DIR/openbsd.raw"; truncate -s 24G "$DIR/openbsd.raw"
H=$DIR/.http; rm -rf "$H"; mkdir -p "$H"
KEY=$(cat "$DIR/id_ed25519.pub")
cat >"$H/install.conf" <<EOF
System hostname = openbsd79
Which network interface do you wish to configure = vio0
IPv4 address for vio0 = autoconf
IPv6 address for vio0 = none
Password for root account = $USER_PW
Public ssh key for root account = $KEY
Start sshd(8) by default = yes
Do you expect to run the X Window System = yes
Do you want the X Window System to be started by xenodm(1) = yes
Change the default console to com0 = no
Setup a user = test
Full name for user test = Test
Password for user test = $USER_PW
Public ssh key for user test = $KEY
Allow root ssh login = prohibit-password
What timezone are you in = UTC
Which disk is the root disk = sd0
Use (W)hole disk MBR, whole disk (G)PT, (O)penBSD area or (E)dit = whole
Use (A)uto layout, (E)dit auto layout, or create (C)ustom layout = a
URL to autopartitioning template for disklabel = http://10.0.2.2:$HTTP_PORT/disklabel
Location of sets = http
HTTP proxy URL = none
HTTP Server = cdn.openbsd.org
Server directory = pub/OpenBSD/7.9/amd64
Set name(s) = -game79.tgz
Continue without verification = no
EOF
# One big root (cargo's target folders are large), 2 GB swap.
printf '/ 4G-* 100%%\nswap 2G\n' >"$H/disklabel"
http_serve "$H"
cd "$DIR"
qemu-system-x86_64 -name $NAME-install -machine q35,accel=kvm -cpu host -smp 4 -m 4G \
  -drive file=openbsd.raw,format=raw,if=none,id=d0,cache=writeback,discard=unmap -device virtio-blk-pci,drive=d0,bootindex=1 \
  -drive file="$C/miniroot79.img",format=raw,if=none,id=d1,readonly=on -device virtio-blk-pci,drive=d1,bootindex=0 \
  -netdev user,id=n0,ipv6=off -device virtio-net-pci,netdev=n0 \
  -display none -vnc 127.0.0.1:$VNC -serial unix:serial.sock,server,nowait -monitor unix:monitor.sock,server,nowait \
  -pidfile qemu.pid -daemonize
# The boot loader listens to the keyboard: point it at the serial line, then drive the installer there.
MONITOR=monitor.sock $SERIAL serial.sock install.log sleep=2 'keys=set tty com0\r' timeout=30 'wait=boot> ' 'send=boot\r' \
  timeout=120 'wait=\(A\)utoinstall or \(S\)hell\? ' 'send=a\r' 'wait=Response file location\?.*\] ' \
  "send=http://10.0.2.2:$HTTP_PORT/install.conf\r" timeout=2400 'wait=CONGRATULATIONS' timeout=120 'wait=rebooting'
echo quit | socat - UNIX-CONNECT:monitor.sock >/dev/null; wait_off 30; rm -f serial.sock monitor.sock
kill "$HTTP_PID"; HTTP_PID=; rm -rf "$H"

DISK=openbsd.raw
write_start "OpenBSD 7.9" ""
write_common_scripts

say "first boot, packages"
"$DIR/start.sh"
wait_ssh root
ssh_as root sh -s <<'EOF'
set -e
echo 'permit nopass keepenv test' >/etc/doas.conf
rcctl enable ntpd >/dev/null 2>&1 || true
pkg_add -I rust git rsync-- node p7zip xfce xfce4-terminal librsvg symbolsonly-nerd-fonts  # tmux is in base
# X as a user needs the aperture (Xorg is not setuid); xenodm's login starts Xfce for test.
grep -q '^machdep.allowaperture=2' /etc/sysctl.conf 2>/dev/null || echo machdep.allowaperture=2 >>/etc/sysctl.conf
rcctl enable xenodm
echo 'exec startxfce4' >/home/test/.xsession; chown test /home/test/.xsession
EOF

say "Coxswain"
ssh_as test 'ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh -s -- --yes --prefix /usr/local'

say "check"
ssh_as test 'uname -a; cargo --version; rustc --version; node --version; git --version; tmux -V; coxswain --version; ls -l /usr/local/bin/coxswain-gui' | tee "$DIR/verify.txt"
ssh_as root 'shutdown -p now' || true
wait_off 120
write_readme "OpenBSD 7.9 (amd64) virtual machine, with Xfce and Coxswain" \
"  The official miniroot79.img from cdn.openbsd.org (SHA256.sig checked with the release key
  openbsd-79-base.pub, cached in $CACHE/openbsd-7.9), installed by autoinstall(8) over
  the network with all sets but games: X included. One root file system (~22 GB), 2 GB swap.
  Disk openbsd.raw: sparse 24 GB raw." \
"Installed
  pkg_add: rust, git, rsync, node, p7zip, xfce, xfce4-terminal, librsvg, symbolsonly-nerd-fonts;
  tmux is in base. Coxswain (terminal app and desktop app) in /usr/local by the install line
  ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh
  (which also added webkitgtk41). doas: 'permit nopass keepenv test'. ntpd on.
  root takes the key over ssh too:  ssh -i id_ed25519 -p $PORT root@127.0.0.1

The desktop
  The screen (VNC :$VNC, 1920x1200, X with the vesa driver) shows the xenodm login: test and
  its password, and Xfce starts (~/.xsession). Then Applications -> Coxswain, or coxswain-gui.
  From ssh, once test is logged in there:  DISPLAY=:0 XAUTHORITY=~/.Xauthority coxswain-gui &
  shots/ has a screenshot of the desktop app (first run, 2026-10-20) when present.
  Text console: Ctrl+Alt+F1 in the VM window. machdep.allowaperture=2 is set for X.

Last check: verify.txt"
say "done: stopped. Start with $DIR/start.sh"
