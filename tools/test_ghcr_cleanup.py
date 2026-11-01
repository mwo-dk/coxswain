"""Which GHCR versions tools/ghcr-cleanup.py keeps and deletes. Run: python3 -m unittest tools/test_ghcr_cleanup.py"""

import importlib.util
import os
import unittest

_spec = importlib.util.spec_from_file_location("ghcr_cleanup", os.path.join(os.path.dirname(__file__), "ghcr-cleanup.py"))
cleanup = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cleanup)


def build(n, tags=()):
    """Image n: an index holding an amd64 and an arm64 manifest, pushed on day n."""
    day = f"2026-01-{n:02d}T00:00:00Z"
    index, amd, arm = f"sha256:i{n}", f"sha256:a{n}", f"sha256:r{n}"
    versions = [
        {"id": f"{n}i", "name": index, "created_at": day, "tags": list(tags)},
        {"id": f"{n}a", "name": amd, "created_at": day, "tags": []},
        {"id": f"{n}r", "name": arm, "created_at": day, "tags": []},
    ]
    manifests = {index: {"manifests": [{"digest": amd}, {"digest": arm}]}, amd: {}, arm: {}}
    return versions, manifests


def history(n, latest_tags=("latest",)):
    versions, manifests = [], {}
    for i in range(1, n + 1):
        v, m = build(i, latest_tags if i == n else ())
        versions += v
        manifests |= m
    return versions, manifests


def deleted(versions, manifests, pinned=()):
    return {v["id"] for v in cleanup.select(versions, manifests, set(pinned))[1]}


class Select(unittest.TestCase):
    def test_keeps_newest_five_images_with_their_children(self):
        versions, manifests = history(8)
        self.assertEqual(deleted(versions, manifests), {"1i", "1a", "1r", "2i", "2a", "2r", "3i", "3a", "3r"})

    def test_a_pinned_index_keeps_its_children(self):
        versions, manifests = history(8)
        gone = deleted(versions, manifests, pinned={"sha256:i2"})
        self.assertFalse(gone & {"2i", "2a", "2r"})
        self.assertEqual(gone, {"1i", "1a", "1r", "3i", "3a", "3r"})

    def test_a_tagged_version_is_never_deleted(self):
        versions, manifests = history(8)
        versions[0]["tags"] = ["old"]  # image 1's index
        gone = deleted(versions, manifests)
        self.assertFalse(gone & {"1i", "1a", "1r"})

    def test_children_count_not_toward_the_newest_five(self):
        versions, manifests = history(5)
        self.assertEqual(deleted(versions, manifests), set())

    def test_attestations_held_or_referring_are_kept(self):
        versions, manifests = history(8)
        manifests["sha256:i1"]["manifests"].append({"digest": "sha256:att1"})
        versions.append({"id": "att1", "name": "sha256:att1", "created_at": "2026-01-01T00:00:00Z", "tags": []})
        manifests["sha256:att1"] = {}
        versions.append({"id": "sig1", "name": "sha256:sig1", "created_at": "2026-01-01T00:00:00Z", "tags": []})
        manifests["sha256:sig1"] = {"subject": {"digest": "sha256:i1"}}
        gone = deleted(versions, manifests, pinned={"sha256:i1"})
        self.assertFalse(gone & {"1i", "1a", "1r", "att1", "sig1"})
        # Not pinned: the index goes, and what it holds or what refers to it with it.
        gone = deleted(versions, manifests)
        self.assertTrue({"1i", "1a", "1r", "att1", "sig1"} <= gone)

    def test_a_child_shared_with_a_kept_index_stays(self):
        versions, manifests = history(8)
        manifests["sha256:i1"]["manifests"].append({"digest": "sha256:a8"})
        self.assertNotIn("8a", deleted(versions, manifests))


if __name__ == "__main__":
    unittest.main()
