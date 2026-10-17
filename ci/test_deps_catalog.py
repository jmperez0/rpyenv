"""ci/deps_catalog.py without the network: version parsing, the per-library filters, and
rewriting a row."""
import os
import sys
import unittest
import urllib.error
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import deps_catalog as dc  # noqa: E402

SHA = "a" * 64


class Versions(unittest.TestCase):
    def test_versions_sort_numerically(self):
        self.assertEqual(dc.newest(["1.3.1", "1.3.10", "1.3.2"]), "1.3.10")

    def test_listing_matches_take_the_newest(self):
        page = '<a href="ncurses-6.5.tar.gz">x</a> <a href="ncurses-6.6.tar.gz">y</a> ncurses-6.4.tar.gz'
        self.assertEqual(dc.newest(dc.matches(page, r"ncurses-(\d+\.\d+)\.tar\.gz")), "6.6")

    def test_tcl_and_tk_stay_on_8_6_and_openssl_on_3(self):
        self.assertEqual(dc.allowed("tcl", ["8.6.18", "9.0.3", "8.6.19"]), ["8.6.18", "8.6.19"])
        self.assertEqual(dc.allowed("openssl", ["3.6.5", "4.0.3", "3.5.9"]), ["3.6.5", "3.5.9"])
        self.assertEqual(dc.allowed("zlib", ["1.3.2"]), ["1.3.2"])

    def test_github_tags_become_versions(self):
        tags = ["v5.8.4", "v5.8.3", "openssl-3.6.5", "v6.0.0beta1"]
        self.assertEqual(dc.tag_versions(tags), ["5.8.4", "5.8.3", "3.6.5"])

    def test_sqlite_product_line(self):
        page = "PRODUCT,3.54.1,2026/sqlite-autoconf-3540100.tar.gz,3332984,b6ced585\n"
        self.assertEqual(dc.sqlite_latest(page), ("3.54.1", "https://sqlite.org/2026/sqlite-autoconf-3540100.tar.gz"))

    def test_cacert_names_sort_by_date(self):
        page = "cacert-2026-07-16.pem cacert-2026-09-25.pem cacert-2026-08-13.pem"
        self.assertEqual(dc.cacert_latest(page), "2026-09-25")


class Rows(unittest.TestCase):
    TSV = "# comment\nzlib\t1.3.1\thttps://x/zlib-1.3.1.tar.gz\t" + SHA + "\nxz\t5.8.4\thttps://x/xz-5.8.4.tar.gz\t" + SHA + "\n"

    def test_rows_are_read_and_comments_kept_on_write(self):
        rows = dc.read_rows(self.TSV)
        self.assertEqual([r[0] for r in rows], ["zlib", "xz"])
        out = dc.replace_row(self.TSV, "zlib", "1.3.2", "https://x/zlib-1.3.2.tar.gz", "b" * 64)
        self.assertIn("# comment\n", out)
        self.assertIn("zlib\t1.3.2\thttps://x/zlib-1.3.2.tar.gz\t" + "b" * 64 + "\n", out)
        self.assertIn("xz\t5.8.4\t", out)

    def test_every_catalog_key_has_a_source(self):
        here = os.path.dirname(os.path.abspath(__file__))
        path = os.path.join(here, "..", "crates", "pyenv", "deps", "catalog.tsv")
        with open(path, encoding="utf-8") as f:
            keys = [r[0] for r in dc.read_rows(f.read())]
        self.assertEqual(sorted(keys), sorted(dc.SOURCES))


class Retries(unittest.TestCase):
    def setUp(self):
        self.sleeps = []
        p = mock.patch.object(dc, "SLEEP", self.sleeps.append)
        p.start()
        self.addCleanup(p.stop)

    @staticmethod
    def err(code):
        return urllib.error.HTTPError("https://x/", code, "boom", {}, None)

    @staticmethod
    def ok(body=b"hello"):
        r = mock.MagicMock()
        r.__enter__.return_value.read.return_value = body
        return r

    def test_two_522s_then_success_returns_the_body(self):
        with mock.patch("urllib.request.urlopen", side_effect=[self.err(522), self.err(522), self.ok()]) as u:
            self.assertEqual(dc.get("https://x/"), "hello")
        self.assertEqual(u.call_count, 3)
        self.assertEqual(self.sleeps, [2, 4])

    def test_three_522s_raise_the_last_error(self):
        with mock.patch("urllib.request.urlopen", side_effect=[self.err(522)] * 3) as u:
            with self.assertRaises(urllib.error.HTTPError):
                dc.get("https://x/")
        self.assertEqual(u.call_count, 3)

    def test_a_404_fails_at_once(self):
        with mock.patch("urllib.request.urlopen", side_effect=[self.err(404), self.ok()]) as u:
            with self.assertRaises(urllib.error.HTTPError):
                dc.get("https://x/")
        self.assertEqual(u.call_count, 1)
        self.assertEqual(self.sleeps, [])


if __name__ == "__main__":
    unittest.main()
