#!/bin/sh
# Bring sysutils/coxswain to version $1, in a FreeBSD VM: regenerate distinfo and
# Makefile.crates, build and check the package, copy the port back into packaging/freebsd, and
# write port.diff against the port as it is in the ports tree (empty when it is not there yet).
# release.yml runs it for each release; it works by hand the same way.
set -eu
v="${1#v}"
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
mkdir -p "$out"

[ -d /usr/ports/Mk ] || git clone -q --depth 1 https://git.FreeBSD.org/ports.git /usr/ports
port=/usr/ports/sysutils/coxswain
in_tree=no
[ -f "$port/Makefile" ] && in_tree=yes
echo "$in_tree" >"$out/in-tree"

rm -rf "$port"
cp -R "$here/sysutils/coxswain" "$port"
cd "$port"
sed -i '' "s/^DISTVERSION=.*/DISTVERSION=	$v/" Makefile
rm -f Makefile.crates distinfo
make makesum BATCH=yes
make cargo-crates BATCH=yes | grep -v '^===>' >Makefile.crates
make makesum BATCH=yes
# cargo-crates extracted the sources before the git crates were listed: start clean.
make clean BATCH=yes
make stage check-plist package BATCH=yes
make clean BATCH=yes
git -C /usr/ports add sysutils/coxswain
portlint -AC

cp Makefile Makefile.crates distinfo "$here/sysutils/coxswain/"
git -C /usr/ports diff --staged >"$out/port.diff"
echo "sysutils/coxswain $v: built and checked; in the ports tree: $in_tree"
