#!/bin/sh
# Rebuild the OmniOS (illumos) test VM from scratch, unattended (about 10 minutes).
# Uses the official cloud image omnios-r151058.cloud.raw.zst (current stable; SHA-256 checked,
# cached), grown to 24 GB, set up by its cloud-init from a NoCloud seed disk
# (user test, key, sudo), then pkg packages and Coxswain; leaves it stopped.
# UEFI only: under SeaBIOS this image panics early in boot (startup_modules).
NAME=omnios IDX=3
REL=r151058
. "$(dirname "$0")/common.sh"
prepare
ovmf
C=$CACHE/omnios-$REL U=https://downloads.omnios.org/media/stable

say "image"
fetch $U/omnios-$REL.cloud.raw.zst "$C/omnios-$REL.cloud.raw.zst"
fetch $U/omnios-$REL.cloud.raw.zst.sha256 "$C/omnios-$REL.cloud.raw.zst.sha256"
[ "$(sha256sum <"$C/omnios-$REL.cloud.raw.zst" | cut -d' ' -f1)" = "$(cut -d' ' -f1 "$C/omnios-$REL.cloud.raw.zst.sha256")" ] \
  || die "cloud image does not match its SHA-256"
echo "omnios-$REL.cloud.raw.zst: SHA-256 OK"
rm -f "$DIR/omnios.raw"
zstd -dqf "$C/omnios-$REL.cloud.raw.zst" -o "$DIR/omnios.raw"
truncate -s 24G "$DIR/omnios.raw"
cp "$VARS" "$DIR/efivars.fd"

make_seed "#cloud-config
users:
  - name: test
    gecos: Test
    shell: /bin/bash
    sudo: \"ALL=(ALL) NOPASSWD:ALL\"
    lock_passwd: false
    passwd: '$(printf '%s' "$USER_PW" | openssl passwd -6 -stdin)'
    ssh_authorized_keys:
      - $(cat "$DIR/id_ed25519.pub")
disable_root: false
ssh_pwauth: true" "instance-id: omnios-test-$(date +%s)
local-hostname: omnios"

DISK=omnios.raw
write_start "OmniOS" "  -drive if=pflash,format=raw,readonly=on,file=$CODE \\
  -drive if=pflash,format=raw,file=efivars.fd \\
"
write_common_scripts

say "first boot (cloud-init)"
rm -f "$DIR/serial.log"
"$DIR/start.sh"
# cloud-init makes new host keys: wait for them before the first ssh, or known_hosts holds the old ones.
i=0; until grep -q 'END SSH HOST KEY' "$DIR/serial.log" 2>/dev/null; do i=$((i + 1)); [ $i -gt 120 ] && die 'cloud-init did not finish'; sleep 5; done
wait_ssh test
ssh_as test 'cloud-init status --wait >/dev/null 2>&1; zpool list rpool'

say "packages"
ROOTHASH=$(printf '%s' "$USER_PW" | openssl passwd -6 -stdin)
ssh_as test sh -s <<EOF
set -e
# cloud-init has done its work: off, or it runs again without the seed (new host keys).
sudo svcadm disable system/cloud-init:initlocal system/cloud-init:init system/cloud-init:modules system/cloud-init:final
# The image has no swap; rustc needs more than 4 GB at times.
sudo zfs create -V 4G rpool/swap
sudo swap -a /dev/zvol/dsk/rpool/swap
echo '/dev/zvol/dsk/rpool/swap - - swap - no -' | sudo tee -a /etc/vfstab >/dev/null
sudo /usr/gnu/bin/sed -i -e 's|^PATH=.*|PATH=/usr/local/bin:/usr/bin:/usr/sbin:/sbin:/opt/ooce/bin:/usr/gnu/bin|' \
  -e 's|^SUPATH=.*|SUPATH=/usr/sbin:/sbin:/opt/ooce/sbin:/usr/local/bin:/usr/bin:/opt/ooce/bin|' /etc/default/login
sudo /usr/gnu/bin/sed -i 's|^root:[^:]*:|root:$ROOTHASH:|' /etc/shadow
sudo svcadm enable network/chrony
sudo pkg refresh --full
sudo pkg install -q ooce/developer/rust developer/versioning/git terminal/tmux developer/gcc14 system/header \
  developer/build/gnu-make developer/pkg-config ooce/runtime/node-24 build-essential || [ \$? = 4 ]   # 4: nothing to do
EOF

say "Coxswain"
ssh_as test 'curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh -s -- --yes --prefix /usr/local'

say "check"
ssh_as test 'uname -a; cargo --version; rustc --version; node --version; git --version; tmux -V; 7z i | sed -n 2p; coxswain --version' | tee "$DIR/verify.txt"
ssh_as test 'sudo shutdown -y -i5 -g0' || true
wait_off 180
rm -f "$DIR/seed.img"
write_readme "OmniOS $REL (illumos, current stable) virtual machine, terminal app of Coxswain" \
"  The official omnios-$REL.cloud.raw.zst (downloads.omnios.org, SHA-256 checked, cached in
  $CACHE/omnios-$REL), grown to 24 GB (rpool grows itself), set up by its cloud-init from
  a NoCloud seed disk that is removed afterwards (the cloud-init services are then disabled).
  ZFS root pool rpool, 4 GB swap on rpool/swap. UEFI (OVMF, variables in efivars.fd):
  under SeaBIOS this image panics early in boot. Disk omnios.raw: sparse 24 GB raw." \
"Installed
  pkg: ooce/developer/rust (/opt/ooce/bin), git, tmux, gcc14, build-essential, gnu-make,
  pkg-config, ooce/runtime/node-24; 7zip, rsync, sudo come with the image. Coxswain's terminal
  app in /usr/local by curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh
  (no desktop build for illumos); /usr/local/bin is on PATH through /etc/default/login.
  sudo: test ALL=(ALL) NOPASSWD:ALL. chrony on. No X.
  The console is the serial line (serial.log); the VNC screen shows the boot loader only.

Last check: verify.txt"
say "done: stopped. Start with $DIR/start.sh"
