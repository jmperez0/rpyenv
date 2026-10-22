"""Offline tests of ci/signing.py (Phase 3 spec §6, §7)."""
import json
import os
import struct
import sys
import tempfile
import unittest
import zipfile
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import signing  # noqa: E402


def pe(magic=0x20B, body=b"\x90" * 1001):
    """A minimal PE image: DOS header, PE signature, COFF header, optional header, body."""
    e_lfanew = 0x80
    dirs = 96 if magic == 0x10B else 112
    opt_size = dirs + 16 * 8
    head = bytearray(e_lfanew + 24 + opt_size)
    head[0:2] = b"MZ"
    struct.pack_into("<I", head, 0x3C, e_lfanew)
    head[e_lfanew:e_lfanew + 4] = b"PE\0\0"
    struct.pack_into("<H", head, e_lfanew + 4 + 16, opt_size)
    opt = e_lfanew + 24
    struct.pack_into("<H", head, opt, magic)
    struct.pack_into("<I", head, opt + dirs - 4, 16)
    struct.pack_into("<I", head, opt + 64, 0x1234)
    return bytes(head) + body


def fake_sign(data, blob=b"\x30" * 24):
    """What signtool does to a PE: pad to 8 bytes, append the certificate table, point the
    security directory at it, rewrite the checksum."""
    n = len(data)
    off = (n + 7) // 8 * 8
    out = bytearray(data + bytes(off - n) + blob)
    chk, sec = signing._pe_offsets(data)
    struct.pack_into("<II", out, sec, off, len(blob))
    struct.pack_into("<I", out, chk, 0x9999)
    return bytes(out)


class Config(unittest.TestCase):
    def test_the_repo_config_pins_the_card(self):
        cfg = signing.load_config()
        self.assertEqual(cfg["thumbprint"], "58E0373B2F4DF17A924C8B1EAB467FC7619A4403")
        self.assertEqual(cfg["timestamp"][0], "http://time.certum.pl")
        self.assertEqual(cfg["arches"], ["x64", "arm64"])

    def test_a_bad_thumbprint_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "s.json")
            with open(p, "w", encoding="utf-8") as f:
                json.dump({"thumbprint": "58e0373b"}, f)
            with self.assertRaises(signing.SigningError):
                signing.load_config(p)


class Tags(unittest.TestCase):
    def test_release_and_dry_run(self):
        self.assertEqual(signing.parse_tag("v0.2.0"), ("0.2.0", False))
        self.assertEqual(signing.parse_tag("v0.2.0-dryrun.17"), ("0.2.0", True))

    def test_other_tags_are_refused(self):
        for t in ("0.2.0", "v0.2", "v0.2.0-rc1", "v0.2.0-dryrun.", "v0.2.0 "):
            with self.assertRaises(signing.SigningError, msg=t):
                signing.parse_tag(t)


class Names(unittest.TestCase):
    def test_file_names_per_kind(self):
        cfg = signing.load_config()
        self.assertEqual(signing.file_names(cfg, "exes", "0.2.0")["arm64"],
                         ["pyenv-shim.exe", "pyenv-shimw.exe", "pyenv.exe"])
        self.assertEqual(signing.file_names(cfg, "msi", "0.2.0"),
                         {"x64": ["rpyenv-0.2.0-x64.msi"], "arm64": ["rpyenv-0.2.0-arm64.msi"]})
        self.assertEqual(signing.zip_name("signed", "msi", "x64"), "tmp-signed-msi-x64.zip")


class Zips(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.d = self.tmp.name

    def tearDown(self):
        self.tmp.cleanup()

    def zip_of(self, names):
        p = os.path.join(self.d, "z.zip")
        with zipfile.ZipFile(p, "w") as z:
            for n in names:
                z.writestr(n, n.encode())
        return p

    def test_unzip_exact_names(self):
        paths = signing.unzip_exact(self.zip_of(["b.exe", "a.exe"]), os.path.join(self.d, "o"), ["a.exe", "b.exe"])
        self.assertEqual([os.path.basename(p) for p in paths], ["a.exe", "b.exe"])
        with open(paths[1], "rb") as f:
            self.assertEqual(f.read(), b"b.exe")

    def test_unzip_refuses_an_extra_file(self):
        with self.assertRaisesRegex(signing.SigningError, "expected"):
            signing.unzip_exact(self.zip_of(["a.exe", "b.exe", "c.exe"]), os.path.join(self.d, "o"), ["a.exe", "b.exe"])

    def test_unzip_refuses_a_missing_file(self):
        with self.assertRaises(signing.SigningError):
            signing.unzip_exact(self.zip_of(["a.exe"]), os.path.join(self.d, "o"), ["a.exe", "b.exe"])

    def test_unzip_refuses_a_path(self):
        with self.assertRaises(signing.SigningError):
            signing.unzip_exact(self.zip_of(["../a.exe"]), os.path.join(self.d, "o"), ["a.exe"])
        self.assertFalse(os.path.exists(os.path.join(self.d, "a.exe")))

    def test_make_zip_is_flat(self):
        sub = os.path.join(self.d, "x64")
        os.makedirs(sub)
        f = os.path.join(sub, "pyenv.exe")
        with open(f, "wb") as fh:
            fh.write(b"MZ")
        out = os.path.join(self.d, "out.zip")
        signing.make_zip(out, [f])
        with zipfile.ZipFile(out) as z:
            self.assertEqual(z.namelist(), ["pyenv.exe"])


class Pe(unittest.TestCase):
    def test_pe_signed_copy_matches(self):
        for magic in (0x10B, 0x20B):
            for body in (b"\x90" * 1001, b"\x90" * 1000):
                u = pe(magic, body)
                self.assertIsNone(signing.pe_difference(u, fake_sign(u)), (magic, len(body)))

    def test_pe_byte_changed(self):
        u = pe()
        s = bytearray(fake_sign(u))
        s[500] ^= 1
        self.assertEqual(signing.pe_difference(u, bytes(s)), "differs from the unsigned file")

    def test_pe_no_signature(self):
        u = pe()
        self.assertEqual(signing.pe_difference(u, u), "the signed file has no signature")

    def test_pe_unsigned_already_signed(self):
        s = fake_sign(pe())
        self.assertEqual(signing.pe_difference(s, s), "the unsigned file already has a signature")

    def test_pe_data_after_the_signature(self):
        u = pe()
        self.assertEqual(signing.pe_difference(u, fake_sign(u) + b"x"), "the signed file has data after its signature")

    def test_pe_nonzero_padding(self):
        u = pe(body=b"\x90" * 1001)
        s = bytearray(fake_sign(u))
        s[len(u)] = 1
        self.assertEqual(signing.pe_difference(u, bytes(s)), "the padding before the signature is not zero")

    def test_pe_signature_moved(self):
        u = pe(body=b"\x90" * 1000)
        s = bytearray(fake_sign(u + b"\0" * 8))
        self.assertRegex(signing.pe_difference(u, bytes(s)), "^the signature starts at")

    def test_not_a_pe(self):
        with self.assertRaises(signing.SigningError):
            signing.pe_difference(b"hello", b"hello")


class Msi(unittest.TestCase):
    BASE = {"": ("root", "000C1084-0000-0000-C000-000000000046", b""),
            "\x05SummaryInformation": ("stream", "", b"sum"), "x": ("stream", "", b"data")}

    def test_signed_copy_matches(self):
        signed = dict(self.BASE, **{"\x05DigitalSignature": ("stream", "", b"sig")})
        self.assertIsNone(signing.msi_difference(self.BASE, signed))

    def test_a_changed_stream_differs(self):
        signed = dict(self.BASE, **{"\x05DigitalSignature": ("stream", "", b"sig"), "x": ("stream", "", b"datb")})
        self.assertEqual(signing.msi_difference(self.BASE, signed), "differs from the unsigned file")

    def test_a_changed_root_clsid_differs(self):
        signed = dict(self.BASE, **{"\x05DigitalSignature": ("stream", "", b"sig"), "": ("root", "0", b"")})
        self.assertEqual(signing.msi_difference(self.BASE, signed), "differs from the unsigned file")

    def test_no_signature(self):
        self.assertEqual(signing.msi_difference(self.BASE, self.BASE), "the signed file has no signature")

    def test_unsigned_already_signed(self):
        s = dict(self.BASE, **{"\x05DigitalSignature": ("stream", "", b"sig")})
        self.assertEqual(signing.msi_difference(s, s), "the unsigned file already has a signature")


class Signatures(unittest.TestCase):
    def test_parse_sig_line(self):
        self.assertEqual(signing.parse_sig_line("Valid|58e0373b2f4df17a924c8b1eab467fc7619a4403|True\r\n"),
                         signing.SigInfo("Valid", "58E0373B2F4DF17A924C8B1EAB467FC7619A4403", True))
        self.assertEqual(signing.parse_sig_line("NotSigned||False\n"), signing.SigInfo("NotSigned", "", False))
        with self.assertRaises(signing.SigningError):
            signing.parse_sig_line("garbage")

    def test_find_signtool_picks_the_newest_sdk(self):
        with tempfile.TemporaryDirectory() as d:
            for v in ("10.0.19041.0", "10.0.28000.0", "10.0.26100.0"):
                p = os.path.join(d, "Windows Kits", "10", "bin", v, "x64")
                os.makedirs(p)
                open(os.path.join(p, "signtool.exe"), "wb").close()
            with mock.patch.dict(os.environ, {"ProgramFiles(x86)": d}):
                self.assertIn("10.0.28000.0", signing.find_signtool())
            self.assertEqual(signing.find_signtool("C:/x/signtool.exe"), "C:/x/signtool.exe")

    def test_find_signtool_without_an_sdk(self):
        with tempfile.TemporaryDirectory() as d, mock.patch.dict(os.environ, {"ProgramFiles(x86)": d}):
            with self.assertRaisesRegex(signing.SigningError, "--signtool"):
                signing.find_signtool()


class SignatureProblems(unittest.TestCase):
    """The signer pin and the timestamp check, which every flow test stubs out."""
    PIN = "58E0373B2F4DF17A924C8B1EAB467FC7619A4403"

    def problems(self, info, signtool_code=0, pin=PIN):
        done = mock.Mock(returncode=signtool_code, stdout="SignTool Error: no signature" if signtool_code else "", stderr="")
        with mock.patch.object(signing.subprocess, "run", return_value=done), \
                mock.patch.object(signing, "signature_info", return_value=info):
            return signing.signature_problems("f.exe", pin, "signtool.exe")

    def test_the_pinned_timestamped_signature_passes(self):
        self.assertEqual(self.problems(signing.SigInfo("Valid", self.PIN, True)), [])
        self.assertEqual(self.problems(signing.SigInfo("Valid", self.PIN, True), pin=self.PIN.lower()), [])

    def test_another_signer_fails(self):
        other = "AB" * 20
        self.assertEqual(self.problems(signing.SigInfo("Valid", other, True)),
                         [f"signed by {other}, expected {self.PIN}"])

    def test_no_timestamp_fails(self):
        self.assertEqual(self.problems(signing.SigInfo("Valid", self.PIN, False)), ["no timestamp"])

    def test_a_bad_status_fails(self):
        self.assertEqual(self.problems(signing.SigInfo("HashMismatch", self.PIN, True)),
                         ["signature status HashMismatch"])

    def test_signtool_verify_failing_fails(self):
        got = self.problems(signing.SigInfo("Valid", self.PIN, True), signtool_code=1)
        self.assertEqual(len(got), 1)
        self.assertRegex(got[0], "^signtool verify /pa failed: SignTool Error")


if __name__ == "__main__":
    unittest.main()
