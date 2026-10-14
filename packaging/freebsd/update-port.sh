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

# The quarterly ports tree: with main, USES=cargo wants a newer lang/rust than the packages
# have for days after each Rust bump (the quarterly and the latest package sets both lag it)
# and builds Rust from source, for hours. The quarterly branch asks for the Rust of the
# quarter, which both package sets have. The port itself is submitted against main: the diff
# is made against it.
if [ ! -d /usr/ports/Mk ]; then
  branch="$(date +%Y)Q$(( ($(date +%m | sed 's/^0//') - 1) / 3 + 1 ))"
  echo "ports tree: $branch"
  git clone -q --depth 1 -b "$branch" https://git.FreeBSD.org/ports.git /usr/ports ||
    { echo "no $branch branch yet: main"; git clone -q --depth 1 https://git.FreeBSD.org/ports.git /usr/ports; }
fi
git -C /usr/ports fetch -q --depth 1 origin main
port=/usr/ports/sysutils/coxswain
in_tree=no
git -C /usr/ports cat-file -e origin/main:sysutils/coxswain/Makefile 2>/dev/null && in_tree=yes
echo "$in_tree" >"$out/in-tree"

rm -rf "$port"
cp -R "$here/sysutils/coxswain" "$port"
cd "$port"
sed -i '' "s/^DISTVERSION=.*/DISTVERSION=	$v/" Makefile
rm -f Makefile.crates distinfo
make makesum BATCH=yes
make cargo-crates BATCH=yes | grep -v '^===>' >Makefile.crates
make makesum BATCH=yes
# Fail at once when the tree wants a Rust the packages do not have, instead of building it.
want=$(make -V BUILD_DEPENDS BATCH=yes | tr ' ' '\n' | sed -n 's/^rust>=\([0-9.]*\):lang\/rust$/\1/p')
have=$(pkg query %v rust)
if [ -n "$want" ] && [ "$(pkg version -t "$have" "$want")" = "<" ]; then
  echo "::error::The ports tree wants rust $want, the packages have $have: start the run again once the package is built"
  exit 1
fi
# cargo-crates extracted the sources before the git crates were listed: start clean.
make clean BATCH=yes
make stage check-plist package BATCH=yes
make clean BATCH=yes
git -C /usr/ports add sysutils/coxswain
portlint -AC

cp Makefile Makefile.crates distinfo "$here/sysutils/coxswain/"
git -C /usr/ports diff --staged origin/main -- sysutils/coxswain >"$out/port.diff"
echo "sysutils/coxswain $v: built and checked; in the ports tree: $in_tree"
