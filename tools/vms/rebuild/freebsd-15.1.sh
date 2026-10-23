#!/bin/sh
# Rebuild the FreeBSD 15.1 test VM from scratch, unattended (about 10 minutes).
# Uses the official FreeBSD-15.1-RELEASE-amd64-BASIC-CLOUDINIT-ufs.raw.xz (download.freebsd.org,
# SHA-256 checked, cached), grown to 32 GB (it grows its root itself on first boot), set up by
# nuageinit from a NoCloud seed disk (user test, key), then pkg packages and Coxswain; leaves it
# stopped. The disk is wiped; the folder's ssh key and password are kept.
NAME=freebsd-15.1 IDX=0
. "$(dirname "$0")/common.sh"
prepare
REL=15.1-RELEASE IMG=FreeBSD-15.1-RELEASE-amd64-BASIC-CLOUDINIT-ufs.raw.xz
C=$CACHE/freebsd-15.1 U=https://download.freebsd.org/releases/VM-IMAGES/$REL/amd64/Latest

say "image"
fetch $U/$IMG "$C/$IMG"
fetch $U/CHECKSUM.SHA256 "$C/CHECKSUM.SHA256"
want=$(sed -n "s/^SHA256 ($IMG) = //p" "$C/CHECKSUM.SHA256")
[ -n "$want" ] && [ "$(sha256sum <"$C/$IMG" | cut -d' ' -f1)" = "$want" ] || die "$IMG does not match CHECKSUM.SHA256"
echo "$IMG: SHA-256 OK"
DISK=freebsd.raw
rm -f "$DIR/$DISK"
xz -dc "$C/$IMG" >"$DIR/$DISK"
truncate -s 32G "$DIR/$DISK"

KEY=$(cat "$DIR/id_ed25519.pub")
HASH=$(printf '%s' "$USER_PW" | openssl passwd -6 -stdin)
make_seed "#cloud-config
users:
  - name: test
    gecos: Test
    groups: wheel
    shell: /bin/sh
    passwd: '$HASH'
    ssh_authorized_keys:
      - $KEY
runcmd:
  - mkdir -p -m 700 /root/.ssh
  - echo '$KEY' >/root/.ssh/authorized_keys
  - echo 'PermitRootLogin prohibit-password' >>/etc/ssh/sshd_config
  - sysrc sshd_enable=YES
  - service sshd restart || service sshd start" "instance-id: freebsd-test-$(date +%s)
local-hostname: freebsd151"

write_start "FreeBSD 15.1" ""
write_common_scripts

say "first boot (nuageinit)"
rm -f "$DIR/serial.log"
"$DIR/start.sh"
wait_ssh root

say "packages"
ssh_as root sh -s <<SCRIPT
set -ex
sysrc hostname=freebsd151 sshd_enable=YES ntpd_enable=YES ntpd_sync_on_start=YES >/dev/null
echo '$HASH' | pw usermod root -H 0
# rustc needs more than 4 GB at times.
if [ ! -e /usr/swap0 ]; then
  dd if=/dev/zero of=/usr/swap0 bs=1m count=4096 2>/dev/null; chmod 600 /usr/swap0
  echo 'md99 none swap sw,file=/usr/swap0,late 0 0' >>/etc/fstab
  swapon -aL || true   # it adds the file, yet fails on the image's other entries
fi
# The first boot may still be updating the base system with pkg: wait for it.
while pgrep -q pkg; do sleep 5; done
env ASSUME_ALWAYS_YES=yes pkg bootstrap -f
pkg install -y rust pkgconf git rsync tmux node22 npm-node22 sudo 7-zip \
  webkit2-gtk_41 gtk3 libsoup3 librsvg2-rust
echo 'test ALL=(ALL) NOPASSWD: ALL' >/usr/local/etc/sudoers.d/test
service ntpd start || true
SCRIPT

say "Coxswain"
ssh_as test 'fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh -s -- --yes --prefix /usr/local'

say "check"
ssh_as test 'uname -a; cargo --version; rustc --version; node --version; git --version; tmux -V; coxswain --version' | tee "$DIR/verify.txt"
ssh_as root 'shutdown -p now' || true
wait_off 120
# nuageinit runs once per instance-id; without the seed it has nothing to do on later boots.
rm -f "$DIR/seed.img"
write_readme "FreeBSD 15.1 (amd64) virtual machine, with Coxswain" \
"  The official $IMG (download.freebsd.org, SHA-256 checked
  against CHECKSUM.SHA256, cached in $C), grown to 32 GB (growfs on first boot), set up
  by nuageinit from a NoCloud seed disk that is removed afterwards. UFS root, 4 GB swap file
  /usr/swap0. Disk $DISK: sparse 32 GB raw." \
"Installed
  pkg: rust, pkgconf, git, rsync, tmux, node22, npm-node22, sudo, 7-zip and the desktop app's
  build libraries (webkit2-gtk_41, gtk3, libsoup3, librsvg2-rust). Coxswain (terminal app and
  desktop app) in /usr/local by
  fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh
  sudo: test ALL=(ALL) NOPASSWD: ALL. ntpd on. No X: the console is the VGA screen (VNC :$VNC).
  root takes the key over ssh too:  ssh -i id_ed25519 -p $PORT root@127.0.0.1

Last check: verify.txt"
say "done: stopped. Start with $DIR/start.sh"
