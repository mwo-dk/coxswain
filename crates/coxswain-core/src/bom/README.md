Catalog from cipherscape `32fc829`.

# BOM ratings data

`catalog.json` rates algorithms: which are broken, deprecated or fine, under which profile and
from which year, with sources. It is cipherscape's compiled `policy/algorithms.yaml`, and
cipherscape owns it: change it there, then run `./sync-catalog.sh`, which copies it here with
`golden.json`, the expected ratings the tests check. The first line of this file names the
cipherscape commit they came from.

The catalog is an unreviewed seed (`lastReviewed` is null). A rating is a hint, not an audit.
