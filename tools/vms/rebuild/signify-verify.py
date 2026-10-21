# Verify an OpenBSD signify embedded signature (SHA256.sig) with a public key.
# uv run -q --with cryptography python -I signify-verify.py KEY.pub SHA256.sig  -> writes the signed text to stdout
import base64, sys
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

key = base64.b64decode(open(sys.argv[1]).read().splitlines()[1])
lines = open(sys.argv[2], "rb").read().split(b"\n", 2)
sig, msg = base64.b64decode(lines[1]), lines[2]
assert key[:2] == sig[:2] == b"Ed", "not an Ed25519 signify file"
assert key[2:10] == sig[2:10], "signed with another key"
Ed25519PublicKey.from_public_bytes(key[10:]).verify(sig[10:], msg)  # raises on a bad signature
sys.stdout.buffer.write(msg)
