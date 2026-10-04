"""Tests for ci/check_install_win.py's pure parts (plan M2b Task 11)."""
import contextlib
import io
import os
import sys
import tempfile
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

    def test_expected_version(self):
        self.assertEqual(c.expected_version("3.12.10"), "3.12.10")
        self.assertEqual(c.expected_version("3.13.0t"), "3.13.0")
        self.assertEqual(c.expected_version("2.7"), "2.7")
        self.assertEqual(c.expected_version("3.12.0rc1-arm"), "3.12.0")

    def test_version_matches(self):
        self.assertTrue(c.version_matches("3.12.10", "3.12.10"))
        self.assertFalse(c.version_matches("3.12.1", "3.12.10"))
        self.assertFalse(c.version_matches("3.12.10", "3.12.1"))
        self.assertTrue(c.version_matches("2.7", "2.7.18"))
        self.assertFalse(c.version_matches("3.1", "3.10.2"))


class Main(unittest.TestCase):
    def test_zero_byte_exe_fails_without_raising(self):
        with tempfile.TemporaryDirectory() as root:
            d = os.path.join(root, "versions", "3.12.1")
            os.makedirs(d)
            open(os.path.join(d, "python.exe"), "wb").close()
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                rc = c.main(root, "3.12.1")
            self.assertEqual(rc, 1)
            self.assertIn("FAIL:", buf.getvalue())

    def test_run_missing_executable(self):
        rc, out = c.run([os.path.join(tempfile.gettempdir(), "no-such-exe-m2b")])
        self.assertEqual(rc, 127)


if __name__ == "__main__":
    unittest.main()
