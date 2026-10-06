#!/bin/sh
# Pack the terminal app for NetBSD, OpenBSD or illumos as a release does: the program, its
# manual page, the licence and notes, and the system's service file, in
# coxswain-terminal-<tag>-<target>.tar.gz with its .sha256. Used by release.yml and by the CI
# that installs the archive with install/install-unix.sh.
#
#   sh packaging/pack-unix.sh TAG TARGET BIN-DIR OUT-DIR [SERVICE-FILE]
set -eu
tag=$1 target=$2 bin=$3 out=$4 service=${5:-}
name="coxswain-terminal-$tag-$target"
mkdir -p "$out/$name"
cp "$bin/coxswain" crates/coxswain/coxswain.1 LICENSE README.md "$out/$name/"
ln -sf coxswain "$out/$name/cox"
[ -f THIRD-PARTY-NOTICES.md ] && cp THIRD-PARTY-NOTICES.md "$out/$name/"
[ -n "$service" ] && cp "$service" "$out/$name/"
(cd "$out" && tar czf "$name.tar.gz" "$name" && rm -rf "$name" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
ls -l "$out"
