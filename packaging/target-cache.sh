#!/bin/sh
# The compiled dependencies of a build in a CI virtual machine or container, kept between runs:
# .github/actions/target-cache restores target/ on the runner before the machine starts and saves
# it after (the machine gets the checkout, target/ included, and gives it back). This runs in
# the machine, around the build. POSIX sh, for each system's own shell.
#
#   sh packaging/target-cache.sh check          before the build
#   sh packaging/target-cache.sh retry cargo …  a build that, failing on a restored target/,
#                                               is tried once more from scratch
#   sh packaging/target-cache.sh prune debug    after the build: only the dependencies stay
#                                               (in a VM, moved to target.vm/: see below)
#
# A target/ another rustc made is dropped before the build, so an old compiler's files are never
# kept or mixed in; target/rustc-v names this one, and the saved cache's key holds it. The
# workspace's own crates, test programs and incremental files are pruned: they change every run.
# CARGO_TARGET_DIR, when set, is used instead of target/.
set -eu
t=${CARGO_TARGET_DIR:-target}
case "$1" in
check)
  v=$(rustc -vV | tr '\n' ' ')
  if [ -d "$t" ] && [ "$(cat "$t/rustc-v" 2>/dev/null)" = "$v" ]; then
    echo "$t: the compiled dependencies of an earlier run"
    touch "$t/restored"
  else
    [ -e "$t/rustc-v" ] && echo "$t: made by another rustc; built from scratch"
    # Its contents, not the folder: in a container it is where the runner's folder is mounted.
    mkdir -p "$t"
    find "$t" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
  fi
  printf '%s\n' "$v" > "$t/rustc-v"
  ;;
retry)
  shift
  "$@" && exit 0
  [ -e "$t/restored" ] || exit 1
  echo "Failed on the compiled dependencies of an earlier run: once more, from scratch"
  find "$t" -mindepth 1 -maxdepth 1 ! -name rustc-v -exec rm -rf {} +
  "$@"
  ;;
prune)
  rm -f "$t/restored"
  for d in "$t/$2" "$t"/*/"$2"; do
    [ -d "$d/deps" ] || continue
    rm -rf "$d/incremental" "$d/examples" "$d"/deps/*coxswain* "$d"/.fingerprint/coxswain* "$d"/build/coxswain*
    # The programs: the workspace's own (beside deps/) and every test program (in deps/,
    # without a dot). The dependencies are libraries, all with an extension.
    find "$d" -maxdepth 1 -type f -exec rm -f {} +
    find "$d/deps" -type f ! -name '*.*' -exec rm -f {} +
  done
  # A virtual machine gives the checkout back by rsync, which adds and never deletes: target/
  # goes back as target.vm/, which the save step puts in place of the runner's old target/.
  [ -n "${CARGO_TARGET_DIR:-}" ] || mv target target.vm
  ;;
*)
  echo "usage: $0 check | retry <command> | prune <profile>" >&2
  exit 2
  ;;
esac
