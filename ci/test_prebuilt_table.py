"""Offline tests of ci/prebuilt_table.py (M8 design §7, D15, D16)."""
import hashlib
import io
import os
import sys
import tarfile
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import prebuilt_table as pt  # noqa: E402


def tgz(members):
    """A .tar.gz in memory: members is [(path, bytes)]."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as t:
        for path, data in members:
            info = tarfile.TarInfo(path)
            info.size = len(data)
            t.addfile(info, io.BytesIO(data))
    return buf.getvalue()


class AssetNames(unittest.TestCase):
    def test_parse_accepts_linux_gnu_install_only(self):
        self.assertEqual(
            pt.parse_asset("cpython-3.13.16+20261020-x86_64-unknown-linux-gnu-install_only.tar.gz"),
            ("3.13.16", "20261020", "x86_64"),
        )
        self.assertEqual(
            pt.parse_asset("cpython-3.13.16+20261020-aarch64-unknown-linux-gnu-freethreaded-install_only.tar.gz"),
            ("3.13.16t", "20261020", "aarch64"),
        )
        self.assertEqual(
            pt.parse_asset("cpython-3.15.0rc3+20261003-x86_64-unknown-linux-gnu-install_only.tar.gz"),
            ("3.15.0rc3", "20261003", "x86_64"),
        )

    def test_parse_rejects_other_builds(self):
        for n in [
            "cpython-3.13.16+20261020-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz",
            "cpython-3.13.16+20261020-x86_64_v3-unknown-linux-gnu-install_only.tar.gz",
            "cpython-3.13.16+20261020-x86_64-unknown-linux-musl-install_only.tar.gz",
            "cpython-3.13.16+20261020-x86_64-apple-darwin-install_only.tar.gz",
            "cpython-3.13.16+20261020-x86_64-unknown-linux-gnu-debug-full.tar.zst",
            "SHA256SUMS",
        ]:
            self.assertIsNone(pt.parse_asset(n), n)

    def test_url_encodes_the_plus(self):
        self.assertEqual(
            pt.asset_url("3.13.16t", "20261020", "x86_64"),
            "https://github.com/astral-sh/python-build-standalone/releases/download/20261020/"
            "cpython-3.13.16%2B20261020-x86_64-unknown-linux-gnu-freethreaded-install_only.tar.gz",
        )


class Choosing(unittest.TestCase):
    def test_newest_attested_release_per_name_and_arch_with_a_definition(self):
        releases = [
            ("20261020", ["cpython-3.13.16+20261020-x86_64-unknown-linux-gnu-install_only.tar.gz"]),
            ("20261003", ["cpython-3.13.16+20261003-x86_64-unknown-linux-gnu-install_only.tar.gz",
                          "cpython-3.12.15+20261003-x86_64-unknown-linux-gnu-install_only.tar.gz"]),
            ("20250205", ["cpython-3.12.9+20250205-x86_64-unknown-linux-gnu-install_only.tar.gz"]),
            ("20250311", ["cpython-3.12.10+20250311-x86_64-unknown-linux-gnu-install_only.tar.gz",
                          "cpython-3.11.99+20250311-x86_64-unknown-linux-gnu-install_only.tar.gz"]),
        ]
        defs = {"3.13.16", "3.12.15", "3.12.9", "3.12.10"}
        self.assertEqual(
            pt.choose(releases, defs),
            {("3.13.16", "x86_64"): "20261020", ("3.12.15", "x86_64"): "20261003",
             ("3.12.10", "x86_64"): "20250311"},
        )


class Inspect(unittest.TestCase):
    def test_reads_openssl_and_tk_from_the_bytes(self):
        lib = b"\x00junk OpenSSL 3.5.9 29 Sep 2026\x00more"
        data = tgz([("python/lib/libpython3.13.so.1.0", lib),
                    ("python/lib/tcl9.0/init.tcl", b"#"),
                    ("python/bin/python3.13", b"ELF")])
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "a.tar.gz")
            with open(p, "wb") as f:
                f.write(data)
            self.assertEqual(pt.inspect(p), ("3.5.9", "9.0"))

    def test_fails_without_openssl_or_tk(self):
        data = tgz([("python/lib/libpython3.13.so.1.0", b"nothing"), ("python/lib/tcl8.6/init.tcl", b"#")])
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "a.tar.gz")
            with open(p, "wb") as f:
                f.write(data)
            with self.assertRaises(pt.TableError):
                pt.inspect(p)


class Format(unittest.TestCase):
    def test_round_trip_and_order(self):
        rows = {
            ("3.13.16t", "x86_64"): ("20261020", "a" * 64, "3.5.9", "9.0"),
            ("3.13.16", "aarch64"): ("20261020", "b" * 64, "3.5.9", "9.0"),
            ("3.9.25", "x86_64"): ("20251031", "c" * 64, "3.5.4", "8.6"),
            ("3.13.16", "x86_64"): ("20261020", "d" * 64, "3.5.9", "9.0"),
        }
        text = pt.render(rows)
        body = [l.split("\t")[:2] for l in text.splitlines() if l and not l.startswith("#")]
        self.assertEqual(body, [["3.9.25", "x86_64"], ["3.13.16", "aarch64"], ["3.13.16", "x86_64"], ["3.13.16t", "x86_64"]])
        self.assertEqual(pt.parse_table(text), rows)


class Main(unittest.TestCase):
    def test_creates_a_missing_workdir(self):
        # The weekly sync passes --workdir "$RUNNER_TEMP/prebuilt", which doesn't exist yet.
        asset = "cpython-3.13.99+20261021-x86_64-unknown-linux-gnu-install_only.tar.gz"
        data = b"tarball bytes"
        sha = hashlib.sha256(data).hexdigest()
        saved = (sys.argv, pt.list_releases, pt.published_sha, pt.attest, pt.inspect, pt.urllib.request.urlopen)
        with tempfile.TemporaryDirectory() as d:
            share = os.path.join(d, "share")
            os.mkdir(share)
            open(os.path.join(share, "3.13.99"), "w", encoding="utf-8").close()
            work = os.path.join(d, "not", "yet")
            sys.argv = ["prebuilt_table.py", "--share", share, "--workdir", work]
            pt.list_releases = lambda: [("20261021", [asset], {})]
            pt.published_sha = lambda *a: sha
            pt.attest = lambda path: None
            pt.inspect = lambda path: ("3.5.9", "9.0")
            pt.urllib.request.urlopen = lambda url, timeout: io.BytesIO(data)
            try:
                self.assertEqual(pt.main(), 0)
            finally:
                (sys.argv, pt.list_releases, pt.published_sha, pt.attest, pt.inspect,
                 pt.urllib.request.urlopen) = saved
            self.assertEqual(os.listdir(work), [])


if __name__ == "__main__":
    unittest.main()
