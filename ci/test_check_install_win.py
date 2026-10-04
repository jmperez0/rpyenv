"""Tests for ci/check_install_win.py's pure parts (plan M2b Task 11)."""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_install_win as c  # noqa: E402


class Parse(unittest.TestCase):
    def test_codes(self):
        self.assertEqual(c.parse("3.10.11"), (3, 10, False))
        self.assertEqual(c.parse("3.13.0t-win32"), (3, 13, True))
        self.assertEqual(c.parse("2.7.18-win32"), (2, 7, False))
        self.assertEqual(c.parse("3.12.0rc1-arm"), (3, 12, False))

    def test_modules_per_version(self):
        self.assertIn("tkinter", c.modules(3, 10))
        self.assertIn("lzma", c.modules(3, 10))
        self.assertIn("Tkinter", c.modules(2, 7))
        self.assertNotIn("lzma", c.modules(2, 7))
        self.assertEqual(c.modules(2, 5), ["zlib", "Tkinter"])

    def test_copies(self):
        self.assertEqual(c.copies(3, 12), ["python.exe", "python3.exe", "python312.exe", "python3.12.exe"])


if __name__ == "__main__":
    unittest.main()
