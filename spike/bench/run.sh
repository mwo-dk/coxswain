#!/bin/sh
# The spike's measurements on this machine: bench/run.sh <data folder> [quick]
# Each line is JSON from spike-host. "quick": the full file natively and with Cranelift only.
set -eu
D=$1
cd "$(dirname "$0")/.."
# SPIKE_BUILD: cargo's feature flags (NetBSD, where wasmtime does not build: --no-default-features --features wasmi).
# shellcheck disable=SC2086
cargo build -q --release -p spike-host -p spike-proc ${SPIKE_BUILD:---features spike-host/pulley,spike-host/wasmi}
H=${CARGO_TARGET_DIR:-target}/release/spike-host
export SPIKE_WASM=plugin.wasm SPIKE_CORE=plugin-core.wasm
N=$(getconf _NPROCESSORS_ONLN 2>/dev/null || getconf NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpuonline 2>/dev/null || nproc)
[ "$N" -gt 8 ] && N=8
echo "{\"system\": \"$(uname -srm)\", \"cpus\": $N, \"threads\": $N}"
pgn=$D/full.pgn
scid="$D/scid-big.si4 $D/scid-big.sn4 $D/scid-big.sg4"
for job in pgn-scan pgn-parse scid-table scid-decode; do
  case $job in pgn-*) f=$pgn small=$D/pgn-20k.pgn ;; *) f=$scid small=$scid ;; esac
  for rt in ${SPIKE_RT:-native cranelift}; do
    # shellcheck disable=SC2086
    $H bench $rt $job 1 3 $f
    # shellcheck disable=SC2086
    $H bench $rt $job "$N" 3 $f
  done
  [ "${2:-}" = quick ] && continue
  for rt in ${SPIKE_SMALL:-native cranelift pulley wasmi}; do
    # shellcheck disable=SC2086
    $H bench $rt $job 1 1 $small | sed 's/^{/{"set": "small", /'
  done
done
# shellcheck disable=SC2086
$H batch "$pgn" ${SPIKE_BATCH:-}
