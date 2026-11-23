#!/bin/sh
# The size each runtime adds to a release binary (stripped, thin LTO, as Coxswain's own).
set -eu
T=${CARGO_TARGET_DIR:-target}
build() { cargo build -q --release -p spike-host "$@" && cp "$(ls $T/release/spike-host.exe 2>/dev/null || echo $T/release/spike-host)" "$T/size-$name"; }
size() { wc -c < "$T/size-$1" | tr -d ' '; }
name=none build --no-default-features
name=cranelift build
name=cranelift+pulley build --features pulley
name=wasmi build --no-default-features --features wasmi
echo "target $(rustc -vV | sed -n 's/^host: //p')  rustc $(rustc -V | cut -d' ' -f2)  wasmtime $(sed -n '/^name = "wasmtime"$/{n;s/version = //p;}' Cargo.lock | tr -d '"')  wasmi $(sed -n '/^name = "wasmi"$/{n;s/version = //p;}' Cargo.lock | tr -d '"')"
base=$(size none)
for v in none cranelift cranelift+pulley wasmi; do
  s=$(size $v); echo "$v: $s bytes (+$(( (s - base) / 1024 )) KiB)"
done
if [ "${SPIKE_MUSL:-}" = 1 ] || { [ "$(uname -s)" = Linux ] && [ "$(uname -m)" = x86_64 ] && rustup target list --installed 2>/dev/null | grep -q musl; }; then
  for f in "--no-default-features" ""; do
    # shellcheck disable=SC2086
    cargo build -q --release -p spike-host --target x86_64-unknown-linux-musl $f
    echo "musl ${f:-cranelift}: $(wc -c < $T/x86_64-unknown-linux-musl/release/spike-host | tr -d ' ') bytes"
  done
fi
