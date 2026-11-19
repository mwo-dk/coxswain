"""The milestone gate of packaging/release-pr.py. Run: python3 packaging/test_release_pr.py (not -m unittest: pip's `packaging` module shadows the folder)"""

import importlib.util
import json
import os
import unittest

_spec = importlib.util.spec_from_file_location("release_pr", os.path.join(os.path.dirname(__file__), "release-pr.py"))
rp = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(rp)


def stone(n, title, state="open"):
    return {"number": n, "title": title, "state": state}


class Pages(unittest.TestCase):
    def test_brackets_inside_strings(self):
        # A task-list line that starts with a link: `][` inside a string, which split the pages.
        a = [{"title": "After the release", "body": "- [ ] [crates.io](https://crates.io)][x]"}]
        b = [{"title": "[a][b]", "body": "]["}]
        text = json.dumps(a) + "\n" + json.dumps(b) + "\n"
        self.assertEqual(rp.pages(text), a + b)
        self.assertEqual(rp.pages(json.dumps(a) + json.dumps(b)), a + b)

    def test_empty(self):
        self.assertEqual(rp.pages(""), [])
        self.assertEqual(rp.pages("[]\n"), [])


class Holding(unittest.TestCase):
    stones = [stone(1, "2.37.0", "closed"), stone(2, "2.38.0"), stone(3, "2.39.0"),
              stone(4, "2.37.2"), stone(5, "Later"), stone(6, "2.36.5")]

    def numbers(self, new, was):
        return [m["number"] for m in rp.holding(self.stones, new, was)]

    def test_own_milestone(self):
        self.assertEqual(self.numbers("2.38.0", "2.37.0"), [2])
        self.assertEqual(self.numbers("2.37.2", "2.37.1"), [4])

    def test_patch_without_milestone_is_held_by_the_next_minor(self):
        self.assertEqual(self.numbers("2.37.1", "2.37.0"), [2, 4])

    def test_closed_and_out_of_range_do_not_hold(self):
        self.assertEqual(self.numbers("2.39.1", "2.39.0"), [])
        self.assertEqual(self.numbers("2.40.0", "2.39.0"), [])


class Arguments(unittest.TestCase):
    # A word main() does not know once ran the real release (bumped Cargo.toml, deleted changelog.d/).
    def run_main(self, *args):
        import contextlib, io, sys, tempfile
        with tempfile.TemporaryDirectory() as d, contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            old_argv, old_cwd = sys.argv, os.getcwd()
            sys.argv = ["release-pr.py", *args]
            os.chdir(d)  # an empty folder: anything past the guard would fail or touch nothing real
            try:
                return rp.main(), os.listdir(d)
            finally:
                sys.argv = old_argv
                os.chdir(old_cwd)

    def test_help_changes_nothing(self):
        self.assertEqual(self.run_main("--help"), (0, []))
        self.assertEqual(self.run_main("-h", "--unreleased-ok"), (0, []))

    def test_unknown_word_changes_nothing(self):
        self.assertEqual(self.run_main("--bogus"), (64, []))
        self.assertEqual(self.run_main("--next", "--dry-run"), (64, []))


if __name__ == "__main__":
    unittest.main()
