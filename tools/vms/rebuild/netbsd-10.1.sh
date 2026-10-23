#!/bin/sh
# Rebuild the NetBSD 10.1 test VM from scratch, unattended (about 10 minutes).
# Uses the official live image (NetBSD-10.1-amd64-live.img.gz, SHA-512 checked, cached),
# grown to 24 GB (the image grows its root itself on first boot), then pkgin packages and
# Coxswain; leaves it stopped. The disk is wiped; the folder's ssh key and password are kept.
NAME=netbsd-10.1 IDX=2
. "$(dirname "$0")/common.sh"
prepare
C=$CACHE/netbsd-10.1
# 10.1 has moved to the archive since 10.2 and 11.0 came out.
U=https://archive.netbsd.org/pub/NetBSD-archive/images/10.1

say "image"
fetch $U/NetBSD-10.1-amd64-live.img.gz "$C/NetBSD-10.1-amd64-live.img.gz"
fetch $U/SHA512 "$C/SHA512"
want=$(sed -n 's/^SHA512 (NetBSD-10.1-amd64-live.img.gz) = //p' "$C/SHA512")
[ -n "$want" ] && [ "$(sha512sum <"$C/NetBSD-10.1-amd64-live.img.gz" | cut -d' ' -f1)" = "$want" ] || die "live image does not match SHA512"
echo "NetBSD-10.1-amd64-live.img.gz: SHA-512 OK"
rm -f "$DIR/netbsd.raw"
gzip -dc "$C/NetBSD-10.1-amd64-live.img.gz" >"$DIR/netbsd.raw"
truncate -s 24G "$DIR/netbsd.raw"

DISK=netbsd.raw
write_start "NetBSD 10.1" ""
write_common_scripts

say "first boot: root key in single user, then the image grows its root and reboots"
cd "$DIR"
VM_SERIAL=unix:serial.sock,server,nowait ./start.sh
KEY=$(cat id_ed25519.pub)
# Boot menu (keyboard): 3 = boot prompt; move the console to the serial line; boot single user.
MONITOR=monitor.sock $SERIAL serial.sock install.log sleep=1.5 'keys=3' sleep=1 'keys=consdev com0\r' timeout=30 'wait=> ' \
  'send=boot -s\r' timeout=180 'wait=RETURN for /bin/sh:' 'send=\r' 'wait=# ' \
  "send=mount -uw / && mkdir -p -m 700 /root/.ssh && echo '$KEY' >/root/.ssh/authorized_keys && sync && mount -ur / && echo KEY-DONE\r" \
  'wait=KEY-DONE\s' 'wait=# ' 'send=exit\r' timeout=600 'wait=rebooting'
echo quit | socat - UNIX-CONNECT:monitor.sock >/dev/null; sleep 1; rm -f qemu.pid serial.sock

say "second boot, packages"
./start.sh
wait_ssh root
ssh_as root sh -s <<EOF
PATH=/sbin:/usr/sbin:/bin:/usr/bin:/usr/pkg/sbin:/usr/pkg/bin; export PATH
set -e
df -h /
sed -i -e 's/^hostname=.*/hostname=netbsd101/' -e '/^no_swap=/d' /etc/rc.conf
# dhcpcd -w: wait for the lease at boot, so ntpdate can resolve its servers.
printf 'dhcpcd_flags="-w"\nntpdate=YES\nntpd=YES\n' >>/etc/rc.conf
hostname netbsd101
dd if=/dev/zero of=/swap bs=1m count=2048 2>/dev/null; chmod 600 /swap; echo '/swap none swap sw 0 0' >>/etc/fstab; swapctl -A
usermod -p "\$(pwhash '$USER_PW')" root
useradd -m -G wheel -s /bin/sh -p "\$(pwhash '$USER_PW')" test
mkdir -p -m 700 /home/test/.ssh; cp /root/.ssh/authorized_keys /home/test/.ssh/; chown -R test:users /home/test/.ssh
export PKG_PATH=https://cdn.netbsd.org/pub/pkgsrc/packages/NetBSD/x86_64/10.1/All
pkg_add pkgin
pkgin -y update
pkgin -y install rust pkgconf git tmux 7-zip rsync sudo nodejs
echo 'test ALL=(ALL) NOPASSWD: ALL' >/usr/pkg/etc/sudoers.d/test
/etc/rc.d/ntpd start || true
EOF

say "Coxswain"
ssh_as test 'ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh -s -- --yes --prefix /usr/local'

say "check"
ssh_as test 'uname -a; cargo --version; rustc --version; node --version; git --version; tmux -V; 7zz i | head -2 || 7z | head -2; coxswain --version' | tee "$DIR/verify.txt"
ssh_as root '/sbin/shutdown -p now' || true
wait_off 120
write_readme "NetBSD 10.1 (amd64) virtual machine, terminal app of Coxswain" \
"  The official NetBSD-10.1-amd64-live.img.gz (archive.netbsd.org, SHA512 checked, cached in
  $CACHE/netbsd-10.1), a ready installed system with X; the disk grown to 24 GB and the
  image's own resize_root grew / on first boot. One root file system, 2 GB swap file /swap.
  Disk netbsd.raw: sparse 24 GB raw. (NetBSD 10.2 and 11.0 are out; 10.1 is archived.)" \
"Installed
  pkgin: rust, pkgconf, git, tmux, 7-zip, rsync, sudo, nodejs. Coxswain's terminal app in
  /usr/local by ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh
  (no desktop build for NetBSD). sudo: test ALL=(ALL) NOPASSWD: ALL. ntpd on.
  root takes the key over ssh too:  ssh -i id_ed25519 -p $PORT root@127.0.0.1
  The console is the VGA screen (VNC :$VNC); X is installed (startx) but not started.

Last check: verify.txt"
say "done: stopped. Start with $DIR/start.sh"
