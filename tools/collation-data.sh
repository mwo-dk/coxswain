#!/bin/sh
# Makes crates/coxswain-core/src/collate/data: ICU4X's collation tables for the languages Coxswain
# speaks (one per file in crates/coxswain-core/locales), so names sort by the language's alphabet
# without the tables of every language on earth. Run it when icu_collator gets a new version, or
# a language is added; datagen's version must match icu_collator's (Cargo.lock).
# It downloads CLDR and ICU's export from GitHub; the build itself downloads nothing.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
version=$(awk '/^name = "icu_collator"$/ { getline; gsub(/[^0-9.]/, ""); print }' "$root/Cargo.lock")
version=${version%.*}.0
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cargo install --locked --quiet --root "$tmp" icu4x-datagen --version "~$version"
locales=$(ls "$root/crates/coxswain-core/locales" | sed 's/\.json$//')
out="$root/crates/coxswain-core/src/collate/data"
# Only each language's standard order: no phone-book German, traditional Spanish or emoji order.
# shellcheck disable=SC2086
"$tmp/bin/icu4x-datagen" --format baked --use-separate-crates --overwrite --out "$out" \
  --attribute-filter 'collator=/^$/' \
  --markers CollationDiacriticsV1 CollationJamoV1 CollationMetadataV1 CollationReorderingV1 \
    CollationRootV1 CollationSpecialPrimariesV1 CollationTailoringV1 \
  --locales $locales
