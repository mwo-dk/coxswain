#!/bin/sh
# Every package install in a CI virtual machine or container goes through here (the systems'
# workflows and release.yml): four tries, 30, 60 and 120 s apart, each later one with a fresh
# package index, and on NetBSD and OpenBSD from the other official mirror. One failed install
# fails a release's build job, and with it the whole draft: Release run of v2.33.0, where
# NetBSD's mirror served libgcrypt-1.12.4 without the libgpg-error it needs mid-sync.
# Unlike retry.sh it tries again on any failure, since a mirror half-way through a sync looks
# like a real dependency error; a real one still fails, after the fourth try.
# POSIX sh, for each system's own shell; the checkout is in the VM before `prepare` runs.
#
#   sh packaging/pkg-retry.sh pkgin -y install rust pkgconf
#
# PKG_TRIES and PKG_WAIT (the first wait, in seconds) are for testing it.
set -u
tries=${PKG_TRIES:-4}
wait=${PKG_WAIT:-30}

# Before try $1: a fresh index, and a mirror chosen by the try (even: the second mirror).
refresh() {
  case "$(uname -s)" in
    NetBSD)
      f=/usr/pkg/etc/pkgin/repositories.conf
      if [ $(($1 % 2)) = 0 ]; then host=ftp.NetBSD.org; else host=cdn.NetBSD.org; fi
      if [ -f "$f" ]; then
        sed "s#://[^/]*/pub/pkgsrc/#://$host/pub/pkgsrc/#" "$f" > "$f.new" && mv "$f.new" "$f"
        grep -v '^#' "$f"
      fi
      pkgin -fy update || true ;;
    OpenBSD) # pkg_add reads the index afresh each time.
      if [ $(($1 % 2)) = 0 ]; then host=ftp.openbsd.org; else host=cdn.openbsd.org; fi
      echo "https://$host/pub/OpenBSD" > /etc/installurl
      cat /etc/installurl ;;
    FreeBSD) pkg update -f || true ;; # pkg.FreeBSD.org is already several mirrors
    SunOS) pkg refresh --full || true ;;
    Linux)
      if [ -n "${PREFIX:-}" ] && [ -d "$PREFIX/etc/apt" ]; then # Termux: its primary repository
        echo "deb https://packages.termux.dev/apt/termux-main stable main" > "$PREFIX/etc/apt/sources.list"
        rm -rf "$PREFIX/etc/termux/chosen_mirrors"
        apt-get update -q || true
      fi ;;
  esac
}

i=1
while :; do
  "$@"
  status=$?
  [ "$status" = 0 ] && exit 0
  # illumos pkg(1): 4 is "nothing to do", everything is installed already.
  [ "$(uname -s)" = SunOS ] && [ "$1" = pkg ] && [ "$status" = 4 ] && exit 0
  [ "$i" -ge "$tries" ] && break
  echo "::warning::$1 failed (try $i of $tries): trying again in ${wait}s with a fresh package index"
  sleep "$wait"
  wait=$((wait * 2))
  i=$((i + 1))
  refresh "$i"
done
echo "::error::Could not fetch with '$*' after $tries tries"
exit "$status"
