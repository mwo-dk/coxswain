#!/usr/bin/env bash
# Run a command again while it fails with what looks like a passing fault on the far side (an
# HTTP 5xx or 429, Azure's BlobNotFound, a reset or timed-out connection): five tries in all,
# waiting 15, 30, 60 and 120 seconds between them. Any other failure fails at once. The
# command's output goes to the log, so this is for commands whose output nobody reads.
#
#   packaging/retry.sh gh release upload --clobber v2.11.0 SHA256SUMS
set -uo pipefail
log=$(mktemp)
transient='HTTP 5[0-9][0-9]|HTTP 429|status (code )?5[0-9][0-9]|50[0234] (Internal|Bad|Service|Gateway)|Internal Server Error|Bad Gateway|Service Unavailable|Gateway Time-?out|Too Many Requests|BlobNotFound|connection reset|connection refused|timed out|timeout awaiting|TLS handshake|unexpected EOF|Could not resolve host|temporary failure'
for i in 1 2 3 4 5; do
  "$@" 2>&1 | tee "$log"
  status=${PIPESTATUS[0]}
  [ "$status" = 0 ] && exit 0
  if ! grep -qiE "$transient" "$log"; then exit "$status"; fi
  [ "$i" = 5 ] && break
  wait=$((15 << (i - 1)))
  echo "::warning::A passing fault, it seems (try $i of 5): trying again in ${wait}s: $1"
  sleep "$wait"
done
echo "::error::Failed five times: $1"
exit "$status"
