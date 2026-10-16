"""Tests for ci/check_conda_win.py's pure parts (plan M7c Task 10)."""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_conda_win as c  # noqa: E402

EXP = r"C:\Users\runner\root\versions\miniforge3-24.1\envs\ci-env"


class PrefixMatches(unittest.TestCase):
    def test_same(self):
        self.assertTrue(c.prefix_matches(EXP, EXP))

    def test_case(self):
        self.assertTrue(c.prefix_matches(EXP, EXP.upper()))

    def test_slashes(self):
        self.assertTrue(c.prefix_matches(EXP, EXP.replace("\\", "/")))

    def test_trailing_separator_and_newline(self):
        self.assertTrue(c.prefix_matches(EXP, EXP + "\\" + "\r\n"))
        self.assertTrue(c.prefix_matches(EXP + "/", EXP))

    def test_different_folder(self):
        self.assertFalse(c.prefix_matches(EXP, EXP.replace("ci-env", "other")))

    def test_parent_or_child(self):
        self.assertFalse(c.prefix_matches(EXP, os.path.dirname(EXP)))
        self.assertFalse(c.prefix_matches(EXP, EXP + "\sub"))

    def test_empty(self):
        self.assertFalse(c.prefix_matches("", ""))


class Tail(unittest.TestCase):
    def test_short_and_long(self):
        self.assertEqual(c.tail("abc", 10), "abc")
        self.assertEqual(c.tail("x" * 20 + "end", 5), "...xxend")


if __name__ == "__main__":
    unittest.main()
