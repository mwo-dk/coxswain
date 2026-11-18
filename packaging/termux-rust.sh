#!/bin/sh
# In Termux (termux.yml and release-check.yml): make rustc as new as the locked crates need.
# A stale mirror can serve an old rustc (1.92 once, when a locked crate needed 1.93). The newest
# rust-version among the locked crates for Android is what it must reach; short of it, rust
# comes from the primary repository, up to three times. Run in a checkout; needs jq and rust.
set -e
need=$(cargo metadata --locked --format-version 1 --filter-platform aarch64-linux-android |
  jq -r ".packages[].rust_version // empty" | sort -V | tail -1)
for i in 1 2 3 4; do
  have=$(rustc --version | cut -d" " -f2)
  [ "$(printf "%s\n%s\n" "$need" "$have" | sort -V | head -1)" = "$need" ] && break
  if [ "$i" = 4 ]; then
    echo "::error title=Termux rust::rustc $have from the Termux repository; the locked crates need $need"; exit 1
  fi
  echo "::warning::rustc $have is older than $need (try $i of 3): rust from packages.termux.dev"
  echo "deb https://packages.termux.dev/apt/termux-main stable main" > $PREFIX/etc/apt/sources.list
  rm -rf $PREFIX/etc/termux/chosen_mirrors
  sleep $((20 * i))
  apt-get update -q || true
  apt-get install -y --only-upgrade rust || true
done
echo "rustc $have, the locked crates need ${need:-nothing in particular}"
