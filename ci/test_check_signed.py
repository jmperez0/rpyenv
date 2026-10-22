"""Offline tests of ci/check_signed.py (Phase 3 spec §5, §7); Windows-only calls are stubbed."""
import contextlib
import io
import os
import sys
import tempfile
import unittest
import zipfile
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_signed  # noqa: E402
import signing  # noqa: E402

EXES = ["pyenv-shim.exe", "pyenv-shimw.exe", "pyenv.exe"]


class CheckSigned(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.zips = os.path.join(self.tmp.name, "zips")
        self.out = os.path.join(self.tmp.name, "out")
        os.makedirs(self.zips)
        self.attested, self.differences = [], {}
        patches = [
            mock.patch.object(signing, "find_signtool", lambda override=None: "signtool.exe"),
            mock.patch.object(signing, "verify_attestation", self.fake_attest),
            mock.patch.object(signing, "signature_problems", lambda p, t, s: []),
            mock.patch.object(signing, "file_difference", lambda u, s: self.differences.get(os.path.basename(s))),
        ]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)

    def tearDown(self):
        self.tmp.cleanup()

    def fake_attest(self, path, repo, workflow, commit):
        self.attested.append((os.path.basename(path), repo, workflow, commit))

    def put(self, stage, kind, arch, names):
        with zipfile.ZipFile(os.path.join(self.zips, signing.zip_name(stage, kind, arch)), "w") as z:
            for n in names:
                z.writestr(n, f"{stage} {n}".encode())

    def run_main(self, *argv):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
            code = check_signed.main(list(argv))
        return code, buf.getvalue()

    def test_all_files_pass_and_are_written_out(self):
        for arch in ("x64", "arm64"):
            self.put("unsigned", "exes", arch, EXES)
            self.put("signed", "exes", arch, EXES)
        code, text = self.run_main("exes", "--zips", self.zips, "--out", self.out, "--commit", "a" * 40)
        self.assertEqual(code, 0, text)
        self.assertEqual(text.count("ok "), 6)
        with open(os.path.join(self.out, "arm64", "pyenv.exe"), "rb") as f:
            self.assertEqual(f.read(), b"signed pyenv.exe")
        self.assertIn(("pyenv.exe", "jmperez0/rpyenv", "release.yml", "a" * 40), self.attested)
        self.assertEqual(len(self.attested), 6)

    def test_one_difference_fails_and_every_file_is_still_checked(self):
        self.put("unsigned", "exes", "x64", EXES)
        self.put("signed", "exes", "x64", EXES)
        self.differences["pyenv.exe"] = "differs from the unsigned file"
        code, text = self.run_main("exes", "--zips", self.zips, "--out", self.out,
                                   "--no-attestation", "--arches", "x64")
        self.assertEqual(code, 1)
        self.assertIn("FAIL x64/pyenv.exe: differs from the unsigned file", text)
        self.assertEqual(text.count("ok "), 2)
        self.assertEqual(self.attested, [])

    def test_msi_names_use_the_version(self):
        self.put("unsigned", "msi", "x64", ["rpyenv-0.2.0-x64.msi"])
        self.put("signed", "msi", "x64", ["rpyenv-0.2.0-x64.msi"])
        code, text = self.run_main("msi", "--version", "0.2.0", "--zips", self.zips, "--out", self.out,
                                   "--commit", "b" * 40, "--arches", "x64")
        self.assertEqual(code, 0, text)
        self.assertEqual(self.attested[0][2], "release-msi.yml")

    def test_an_extra_file_fails(self):
        self.put("unsigned", "exes", "x64", EXES)
        self.put("signed", "exes", "x64", EXES + ["evil.exe"])
        code, text = self.run_main("exes", "--zips", self.zips, "--out", self.out,
                                   "--no-attestation", "--arches", "x64")
        self.assertEqual(code, 1)
        self.assertIn("evil.exe", text)

    def test_a_missing_zip_fails(self):
        code, text = self.run_main("exes", "--zips", self.zips, "--out", self.out,
                                   "--no-attestation", "--arches", "x64")
        self.assertEqual(code, 1)
        self.assertIn("tmp-unsigned-exes-x64.zip", text)

    def test_msi_needs_a_version(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as e:
            check_signed.main(["msi", "--zips", self.zips, "--out", self.out, "--no-attestation"])
        self.assertEqual(e.exception.code, 2)

    def test_the_thumbprint_override_reaches_the_signature_check(self):
        seen = []
        self.put("unsigned", "exes", "x64", EXES)
        self.put("signed", "exes", "x64", EXES)
        with mock.patch.object(signing, "signature_problems", lambda p, t, s: seen.append(t) or []):
            self.run_main("exes", "--zips", self.zips, "--out", self.out, "--no-attestation",
                          "--arches", "x64", "--thumbprint", "ab" * 20)
        self.assertEqual(set(seen), {"AB" * 20})


if __name__ == "__main__":
    unittest.main()
