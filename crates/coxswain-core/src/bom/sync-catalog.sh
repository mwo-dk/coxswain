#!/bin/sh
# Copies the policy catalog and its golden cases from cipherscape, which owns them.
# Usage: sync-catalog.sh [path to cipherscape]   (default: ../cipherscape next to this repo)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
cs=${1:-$here/../../../../../cipherscape}
cd "$cs/web"
npm run --silent policy
cp src/core/policy/catalog.gen.json "$here/catalog.json"
# golden.yaml as JSON, so core needs no YAML parser
node --input-type=module -e "
import { readFileSync } from 'node:fs'
import { parse } from 'yaml'
process.stdout.write(JSON.stringify(parse(readFileSync('../policy/golden.yaml', 'utf8')).cases, null, 1) + '\n')
" > "$here/golden.json"
commit=$(git -C "$cs" log -1 --format=%h -- policy)
sed -i "1s/.*/Catalog from cipherscape \`$commit\`./" "$here/README.md"
echo "catalog.json and golden.json from cipherscape $commit"
