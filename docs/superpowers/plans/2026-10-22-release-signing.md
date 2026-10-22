# Phase 3 release signing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sign rpyenv's Windows executables and MSIs with the maintainer's Certum card between CI steps, so a release draft ends with signed MSIs, `SHA256SUMS` and attestations for every final file.

**Architecture:** CI builds and attests the unsigned files and puts them on a draft release as `tmp-*` zips. A local script (`ci/release_sign.py`) reads the draft and verifies the attestations. It signs with `signtool` (the card software asks for the PIN), uploads the result and starts the next workflow. CI (`ci/check_signed.py`) proves each signed file is the attested file plus a signature before building on it. Shared logic lives in `ci/signing.py`.

**Tech Stack:** Python 3 stdlib (`unittest`, `zipfile`, `struct`), `olefile==0.47` (CI only, pinned by hash), `gh` 2.87, Windows SDK `signtool.exe`, PowerShell 7 (`Get-AuthenticodeSignature`), GitHub Actions (`actions/attest-build-provenance`).

**Spec:** `docs/superpowers/specs/2026-10-22-release-signing-design.md`

## Global Constraints

- Pinned certificate thumbprint: `58E0373B2F4DF17A924C8B1EAB467FC7619A4403`.
- Timestamp servers, in order: `http://time.certum.pl`, then `http://timestamp.digicert.com`. Every signature uses `/fd sha256 /tr <url> /td sha256`.
- Files signed: `pyenv.exe`, `pyenv-shim.exe` and `pyenv-shimw.exe`, for `x64` and `arm64`, plus `rpyenv-{version}-{arch}.msi`. Nothing else.
- Temporary draft assets: `tmp-unsigned-exes-<arch>.zip`, `tmp-signed-exes-<arch>.zip`, `tmp-unsigned-msi-<arch>.zip`, `tmp-signed-msi-<arch>.zip`.
- Release tags: `v<X.Y.Z>`, or `v<X.Y.Z>-dryrun.<N>` for a dry run. Dry-run steps 3 and 5 are dispatched on `main`; real ones on the tag.
- `gh attestation verify` always gets `--repo jmperez0/rpyenv --signer-workflow jmperez0/rpyenv/.github/workflows/<file> --source-digest <40-hex commit> --deny-self-hosted-runners`. Measured 2026-10-22 on v0.1.0's `install.sh`: the right commit gives exit 0, an all-zero digest gives exit 1.
- Attest steps run in the top-level workflow's own job (never inside `build.yml` or `msi.yml`), so the signer workflow is `release.yml`, `release-msi.yml` or `release-finish.yml`.
- No script reads, stores or passes the card PIN.
- Python: `encoding="utf-8"` on every text `open()`. The ci tests are stdlib `unittest`; they must pass on Ubuntu (`python3 -m unittest discover -s ci`) and on Windows (`python -m unittest discover -s ci`).
- Never install an MSI or run `signtool sign` on the development host during implementation. Real signing happens only in Task 10, by the maintainer.
- Commit messages: one line, no attribution lines. Ask the user before any push, PR, tag, workflow dispatch or release change.
- Actions are pinned by commit SHA, using the same SHAs `build.yml` uses today.

## Review Focus

1. **A draft left half-way by an interrupted upload or delete:** for example `SHA256SUMS` present with some `tmp-*` assets still there, or one arch's zip uploaded without the other. The script must continue from the right step and never sign the executables again after the MSIs exist. Pinned in Task 6 (`test_state_*`).
2. **A signed file that differs from the attested one only in a byte outside the signature:** it must fail in CI, not pass because the signature itself is valid. Pinned in Task 4 (self-test cases 2 and 5) and Task 1 (`test_pe_byte_changed`).
3. **A zip with an extra file, a missing file or a path like `../x`:** it must be refused before anything is signed or extracted. Pinned in Task 1 (`test_unzip_*`) and Task 6 (`test_extra_file_stops_before_signing`).
4. **The timestamp server down:** the second server is tried on fresh copies, so files are never signed twice. Pinned in Task 6 (`test_timestamp_fallback_uses_fresh_copies`).
5. **`main` moving during a dry run, or a tag/ref mismatch:** steps 3 and 5 must refuse before downloading anything. Pinned in Task 7 (`ci/release_guard.sh`, run in both workflows' first step) and checked by hand in Task 10.

---

## File Structure

| File | Responsibility |
|---|---|
| `installer/signing.json` (new) | Pinned settings: repo, thumbprint, timestamp URLs, description, URL, file names, arches |
| `ci/signing.py` (new) | Shared logic: config, tag parsing, expected names, exact unzip and zip, PE and MSI comparison, signature reading, signtool discovery, attestation check |
| `ci/test_signing.py` (new) | Offline tests of `signing.py` |
| `ci/check_signed.py` (new) | CI command: checks the unsigned/signed zip pairs and writes the signed files out |
| `ci/test_check_signed.py` (new) | Offline tests of `check_signed.py` |
| `ci/requirements-signing.txt` (new) | `olefile==0.47` with its hash |
| `ci/signing_selftest.ps1` (new) | CI self-test: a throwaway certificate, pass cases and fail cases |
| `ci/release_guard.sh` (new) | Steps 3 and 5: the run matches the draft's tag, ref and commit |
| `ci/release_sign.py` (new) | The maintainer's script (steps 2 and 4) |
| `ci/test_release_sign.py` (new) | Offline tests of `release_sign.py` against a fake `gh` |
| `.github/workflows/build.yml` (changed) | `exes` job + `msi.yml` call (input `msi`) + tarballs |
| `.github/workflows/msi.yml` (new) | Reusable: MSI build and tests from the `exes-<arch>` artifact |
| `.github/workflows/packaging.yml` (changed) | New paths; `signing-selftest` job |
| `.github/workflows/release.yml` (changed) | Step 1 |
| `.github/workflows/release-msi.yml` (new) | Step 3 |
| `.github/workflows/release-finish.yml` (new) | Step 5 |
| `installer/sign.ps1` (deleted) | Replaced |
| `docs/releasing.md` (new), `README.md`, `docs/windows-install.md`, M6b spec | Docs |

---

### Task 1: Shared signing logic (`ci/signing.py`) and pinned settings

**Files:**
- Create: `installer/signing.json`
- Create: `ci/signing.py`
- Test: `ci/test_signing.py`

**Interfaces:**
- Produces (used by Tasks 2, 6):
  - `signing.CONFIG: str`
  - `signing.BUILT_BY: dict`, equal to `{"exes": "release.yml", "msi": "release-msi.yml"}`
  - `signing.SigningError(Exception)`
  - `signing.SigInfo`, a namedtuple of `status: str`, `thumbprint: str` (upper case), `timestamped: bool`
  - `load_config(path=CONFIG) -> dict`
  - `parse_tag(tag) -> (version: str, dry_run: bool)`
  - `file_names(cfg, kind, version) -> {arch: [names]}`
  - `zip_name(stage, kind, arch) -> str`
  - `unzip_exact(zip_path, dest, names) -> [paths]`
  - `make_zip(zip_path, files) -> None`
  - `pe_difference(unsigned: bytes, signed: bytes) -> str | None`
  - `msi_entries(path) -> dict`
  - `msi_difference(unsigned_entries, signed_entries) -> str | None`
  - `file_difference(unsigned_path, signed_path) -> str | None`
  - `parse_sig_line(text) -> SigInfo`
  - `signature_info(path) -> SigInfo`
  - `signature_problems(path, thumbprint, signtool) -> [str]`
  - `find_signtool(override=None) -> str`
  - `verify_attestation(path, repo, workflow, commit) -> None` (raises `SigningError`)

- [ ] **Step 1: Write `installer/signing.json`**

```json
{
  "repo": "jmperez0/rpyenv",
  "thumbprint": "58E0373B2F4DF17A924C8B1EAB467FC7619A4403",
  "timestamp": ["http://time.certum.pl", "http://timestamp.digicert.com"],
  "description": "rpyenv",
  "url": "https://github.com/jmperez0/rpyenv",
  "exes": ["pyenv.exe", "pyenv-shim.exe", "pyenv-shimw.exe"],
  "msi": "rpyenv-{version}-{arch}.msi",
  "arches": ["x64", "arm64"]
}
```

- [ ] **Step 2: Write the failing tests in `ci/test_signing.py`**

```python
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


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 3: Run the tests and check that they fail**

Run: `python -m unittest ci.test_signing -v` from the repo root, or `python -m unittest discover -s ci -p "test_signing.py" -v`.
Expected: `ModuleNotFoundError: No module named 'signing'`.

- [ ] **Step 4: Write `ci/signing.py`**

```python
"""Shared by the release signing steps (Phase 3, docs/superpowers/specs/2026-10-22-release-signing-design.md).

ci/release_sign.py (the maintainer's PC) and ci/check_signed.py (CI) both use it: the pinned
settings in installer/signing.json, the expected file names, exact unzipping, signature
reading and the check that a signed file is its unsigned file plus a signature (spec §7).
"""
import glob
import json
import os
import re
import shutil
import struct
import subprocess
import zipfile
from collections import namedtuple

REPO_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CONFIG = os.path.join(REPO_DIR, "installer", "signing.json")
TAG = re.compile(r"^v(\d+\.\d+\.\d+)(-dryrun\.\d+)?$")
# The workflow whose attestation an unsigned file of each kind must carry (spec §4.2).
BUILT_BY = {"exes": "release.yml", "msi": "release-msi.yml"}
# Authenticode's streams in a signed MSI; everything else must equal the unsigned MSI.
SIGNATURE_STREAMS = {"\x05DigitalSignature", "\x05MsiDigitalSignatureEx"}
SigInfo = namedtuple("SigInfo", "status thumbprint timestamped")
PS_SIGNATURE = (
    "$s = Get-AuthenticodeSignature -LiteralPath $env:RPYENV_SIG_PATH; "
    '"{0}|{1}|{2}" -f $s.Status, $s.SignerCertificate.Thumbprint, [bool]$s.TimeStamperCertificate'
)


class SigningError(Exception):
    pass


def load_config(path=CONFIG):
    with open(path, encoding="utf-8") as f:
        cfg = json.load(f)
    if not re.fullmatch(r"[0-9A-F]{40}", cfg.get("thumbprint", "")):
        raise SigningError(f"{path}: thumbprint must be 40 upper-case hex digits")
    return cfg


def parse_tag(tag):
    """(version, dry run?) for v1.2.3 or v1.2.3-dryrun.N."""
    m = TAG.match(tag)
    if not m:
        raise SigningError(f"not a release tag: {tag!r} (v1.2.3 or v1.2.3-dryrun.N)")
    return m.group(1), bool(m.group(2))


def file_names(cfg, kind, version):
    """{arch: sorted file names} one arch's zip of `kind` ("exes" or "msi") must hold."""
    if kind == "exes":
        return {a: sorted(cfg["exes"]) for a in cfg["arches"]}
    if kind == "msi":
        return {a: [cfg["msi"].format(version=version, arch=a)] for a in cfg["arches"]}
    raise SigningError(f"unknown kind {kind!r}")


def zip_name(stage, kind, arch):
    """The draft asset of one hand-off: stage is "unsigned" or "signed"."""
    return f"tmp-{stage}-{kind}-{arch}.zip"


def unzip_exact(zip_path, dest, names):
    """Extracts a zip that holds exactly `names` (plain file names) into dest; their paths."""
    with zipfile.ZipFile(zip_path) as z:
        got = sorted(z.namelist())
        if got != sorted(names):
            raise SigningError(f"{os.path.basename(zip_path)} holds {got}, expected {sorted(names)}")
        os.makedirs(dest, exist_ok=True)
        out = []
        for n in sorted(names):
            p = os.path.join(dest, n)
            with z.open(n) as src, open(p, "wb") as dst:
                shutil.copyfileobj(src, dst)
            out.append(p)
    return out


def make_zip(zip_path, files):
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as z:
        for p in files:
            z.write(p, os.path.basename(p))


def _pe_offsets(data):
    """(checksum offset, security-directory entry offset) of a PE image."""
    try:
        if data[:2] != b"MZ":
            raise SigningError("not a PE file")
        pe = struct.unpack_from("<I", data, 0x3C)[0]
        if data[pe:pe + 4] != b"PE\0\0":
            raise SigningError("not a PE file")
        opt = pe + 24
        magic = struct.unpack_from("<H", data, opt)[0]
        dirs = {0x10B: 96, 0x20B: 112}.get(magic)
        if dirs is None:
            raise SigningError(f"unknown optional-header magic {magic:#x}")
        if struct.unpack_from("<I", data, opt + dirs - 4)[0] < 5:
            raise SigningError("the PE file has no security directory")
        struct.unpack_from("<II", data, opt + dirs + 32)
    except struct.error:
        raise SigningError("not a PE file") from None
    return opt + 64, opt + dirs + 32


def _pe_cert_table(data):
    _, sec = _pe_offsets(data)
    return struct.unpack_from("<II", data, sec)


def _pe_blank(data, n):
    """The first n bytes with the checksum and the security-directory entry zeroed."""
    chk, sec = _pe_offsets(data)
    b = bytearray(data[:n])
    b[chk:chk + 4] = bytes(4)
    b[sec:sec + 8] = bytes(8)
    return bytes(b)


def pe_difference(unsigned, signed):
    """None when `signed` is `unsigned` plus an appended Authenticode signature, else why not."""
    if _pe_cert_table(unsigned) != (0, 0):
        return "the unsigned file already has a signature"
    off, size = _pe_cert_table(signed)
    if (off, size) == (0, 0):
        return "the signed file has no signature"
    n = len(unsigned)
    want = (n + 7) // 8 * 8
    if off != want:
        return f"the signature starts at {off}, expected {want}"
    if off + size != len(signed):
        return "the signed file has data after its signature"
    if any(signed[n:off]):
        return "the padding before the signature is not zero"
    if _pe_blank(unsigned, n) != _pe_blank(signed, n):
        return "differs from the unsigned file"
    return None


def msi_entries(path):
    """{entry path: (kind, clsid, data)} for an MSI's root, storages and streams."""
    import olefile  # ci/requirements-signing.txt; only CI's MSI checks need it

    ole = olefile.OleFileIO(path)
    try:
        out = {"": ("root", ole.root.clsid, b"")}
        for entry in ole.listdir(streams=True, storages=True):
            name = "/".join(entry)
            if ole.get_type(entry) == olefile.STGTY_STREAM:
                out[name] = ("stream", "", ole.openstream(entry).read())
            else:
                out[name] = ("storage", ole.getclsid(entry), b"")
        return out
    finally:
        ole.close()


def msi_difference(unsigned, signed):
    """None when the signed MSI's entries are the unsigned MSI's plus the signature streams."""
    if SIGNATURE_STREAMS & unsigned.keys():
        return "the unsigned file already has a signature"
    if "\x05DigitalSignature" not in signed:
        return "the signed file has no signature"
    rest = {k: v for k, v in signed.items() if k not in SIGNATURE_STREAMS}
    if rest != unsigned:
        return "differs from the unsigned file"
    return None


def file_difference(unsigned_path, signed_path):
    if signed_path.lower().endswith(".msi"):
        return msi_difference(msi_entries(unsigned_path), msi_entries(signed_path))
    with open(unsigned_path, "rb") as f:
        u = f.read()
    with open(signed_path, "rb") as f:
        s = f.read()
    return pe_difference(u, s)


def parse_sig_line(text):
    parts = text.strip().split("|")
    if len(parts) != 3:
        raise SigningError(f"unreadable Get-AuthenticodeSignature output {text!r}")
    return SigInfo(parts[0], parts[1].upper(), parts[2] == "True")


def signature_info(path):
    """The Authenticode status, signer thumbprint and timestamp presence of a file (Windows)."""
    r = subprocess.run(
        ["pwsh", "-NoProfile", "-NonInteractive", "-Command", PS_SIGNATURE],
        env={**os.environ, "RPYENV_SIG_PATH": os.path.abspath(path)},
        capture_output=True, text=True, encoding="utf-8")
    if r.returncode:
        raise SigningError(f"Get-AuthenticodeSignature {path}: {r.stderr.strip()[-300:]}")
    return parse_sig_line(r.stdout)


def signature_problems(path, thumbprint, signtool):
    """[] when `signtool verify /pa` passes and the signer and timestamp are as pinned."""
    out = []
    r = subprocess.run([signtool, "verify", "/pa", path], capture_output=True, text=True, errors="replace")
    if r.returncode:
        out.append(f"signtool verify /pa failed: {(r.stdout + r.stderr).strip()[-300:]}")
    info = signature_info(path)
    if info.status != "Valid":
        out.append(f"signature status {info.status}")
    if info.thumbprint != thumbprint.upper():
        out.append(f"signed by {info.thumbprint or 'nobody'}, expected {thumbprint.upper()}")
    if not info.timestamped:
        out.append("no timestamp")
    return out


def find_signtool(override=None):
    """The newest x64 signtool.exe of the Windows 10/11 SDK, unless one is given."""
    if override:
        return override
    kits = os.path.join(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"), "Windows Kits", "10", "bin")
    found = glob.glob(os.path.join(kits, "10.*", "x64", "signtool.exe"))
    if not found:
        raise SigningError(f"no signtool.exe under {kits}; install the Windows SDK or pass --signtool")

    def version(p):
        return tuple(int(x) for x in os.path.basename(os.path.dirname(os.path.dirname(p))).split("."))

    return max(found, key=version)


def verify_attestation(path, repo, workflow, commit):
    r = subprocess.run(
        ["gh", "attestation", "verify", path, "--repo", repo,
         "--signer-workflow", f"{repo}/.github/workflows/{workflow}",
         "--source-digest", commit, "--deny-self-hosted-runners"],
        capture_output=True, text=True, encoding="utf-8")
    if r.returncode:
        raise SigningError(f"{os.path.basename(path)}: attestation: {r.stderr.strip()[-300:]}")
```

- [ ] **Step 5: Run the tests and check that they pass**

Run: `python -m unittest discover -s ci -p "test_signing.py" -v`
Expected: all tests pass. Then run `python -m unittest discover -s ci -p "test_*.py"`; expected: no regressions.

- [ ] **Step 6: Commit**

```bash
git add installer/signing.json ci/signing.py ci/test_signing.py
git commit -m "Signing: shared checks (pinned settings, exact unzip, signed = unsigned + signature for PE and MSI)"
```

---

### Task 2: The CI check command (`ci/check_signed.py`)

**Files:**
- Create: `ci/check_signed.py`
- Create: `ci/requirements-signing.txt`
- Test: `ci/test_check_signed.py`

**Interfaces:**
- Consumes: from Task 1, `signing.load_config`, `file_names`, `zip_name`, `unzip_exact`, `verify_attestation`, `signature_problems`, `file_difference`, `find_signtool`, `BUILT_BY` and `SigningError`.
- Produces (used by Tasks 4, 7): `python ci/check_signed.py {exes,msi} --zips DIR --out DIR (--commit SHA | --no-attestation) [--version V] [--thumbprint HEX] [--arches A ...]`.
  - Prints `ok <arch>/<name>` or `FAIL <arch>/<name>: <reason>` per file.
  - Exit codes: 0 when every file passes, 1 on any failure, 2 on a usage error.
  - The signed files end up in `--out/<arch>/<name>`.

- [ ] **Step 1: Write `ci/requirements-signing.txt`**

```
# ci/check_signed.py reads MSIs with olefile (Phase 3 spec §7). Hash from PyPI's JSON API, 2026-10-22.
olefile==0.47 --hash=sha256:543c7da2a7adadf21214938bb79c83ea12b473a4b6ee4ad4bf854e7715e13d1f
```

- [ ] **Step 2: Write the failing tests in `ci/test_check_signed.py`**

```python
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
```

- [ ] **Step 3: Run the tests and check that they fail**

Run: `python -m unittest discover -s ci -p "test_check_signed.py" -v`
Expected: `ModuleNotFoundError: No module named 'check_signed'`.

- [ ] **Step 4: Write `ci/check_signed.py`**

```python
"""Checks a release's signed Windows files in CI (Phase 3 spec §5, §7).

  python ci/check_signed.py {exes,msi} --zips DIR --out DIR (--commit SHA | --no-attestation)
                            [--version V] [--thumbprint HEX] [--arches A ...]

--zips holds tmp-unsigned-<kind>-<arch>.zip and tmp-signed-<kind>-<arch>.zip for each arch, and
each zip must hold exactly the names in installer/signing.json. Every unsigned file must carry
the attestation of the workflow that built it at --commit. Every signed file must pass
`signtool verify /pa`, be signed by the pinned thumbprint with a timestamp, and be its unsigned
file plus a signature. The signed files are left in --out/<arch>/. Every file is checked before
the exit status (1 on any failure) is decided.
"""
import argparse
import os
import shutil
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import signing  # noqa: E402


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("kind", choices=["exes", "msi"])
    ap.add_argument("--zips", required=True)
    ap.add_argument("--out", required=True)
    who = ap.add_mutually_exclusive_group(required=True)
    who.add_argument("--commit")
    who.add_argument("--no-attestation", action="store_true")
    ap.add_argument("--version")
    ap.add_argument("--thumbprint")
    ap.add_argument("--arches", nargs="+")
    a = ap.parse_args(argv)
    if a.kind == "msi" and not a.version:
        ap.error("msi needs --version")
    cfg = signing.load_config()
    thumbprint = (a.thumbprint or cfg["thumbprint"]).upper()
    names = signing.file_names(cfg, a.kind, a.version)
    work = tempfile.mkdtemp(prefix="check-signed.")
    failed = 0
    try:
        signtool = signing.find_signtool()
        for arch in a.arches or cfg["arches"]:
            unsigned = signing.unzip_exact(os.path.join(a.zips, signing.zip_name("unsigned", a.kind, arch)),
                                           os.path.join(work, arch), names[arch])
            signed = signing.unzip_exact(os.path.join(a.zips, signing.zip_name("signed", a.kind, arch)),
                                         os.path.join(a.out, arch), names[arch])
            for u, s in zip(unsigned, signed):
                label = f"{arch}/{os.path.basename(s)}"
                problems = []
                try:
                    if a.commit:
                        signing.verify_attestation(u, cfg["repo"], signing.BUILT_BY[a.kind], a.commit)
                    problems += signing.signature_problems(s, thumbprint, signtool)
                    why = signing.file_difference(u, s)
                    if why:
                        problems.append(why)
                except signing.SigningError as e:
                    problems.append(str(e))
                for p in problems:
                    print(f"FAIL {label}: {p}", flush=True)
                if not problems:
                    print(f"ok {label}", flush=True)
                failed += bool(problems)
    except (signing.SigningError, OSError) as e:
        print(f"check_signed: {e}", file=sys.stderr)
        return 1
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
```

Note: a missing zip raises `FileNotFoundError`, an `OSError`. Its message names the path, which is what `test_a_missing_zip_fails` checks.

- [ ] **Step 5: Run the tests and check that they pass**

Run: `python -m unittest discover -s ci -p "test_*.py"`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add ci/check_signed.py ci/test_check_signed.py ci/requirements-signing.txt
git commit -m "Signing: ci/check_signed.py checks each unsigned/signed pair (attestation, pinned signer, timestamp, only a signature added)"
```

---

### Task 3: Split `build.yml` into an `exes` job and a reusable `msi.yml`; remove `sign.ps1`

**Files:**
- Modify: `.github/workflows/build.yml`
- Create: `.github/workflows/msi.yml`
- Delete: `installer/sign.ps1`
- Modify: `.github/workflows/packaging.yml` (paths)

**Interfaces:**
- Produces:
  - `build.yml` takes the input `msi` (boolean, default `true`) and uploads the run artifacts `exes-<arch>` (3 exes, flat), `msi-<arch>` (when `msi`) and `tar-<arch>`.
  - `msi.yml` (on `workflow_call`, no inputs) downloads `exes-<arch>` from the same run and uploads `msi-<arch>` (`dist/*.msi`).

- [ ] **Step 1: Write `.github/workflows/msi.yml`**

```yaml
name: Build MSIs
# Builds and tests the MSI from the run's exes-<arch> artifact (M6b design §7, Phase 3 spec §5).
# Called by build.yml (pull requests, unsigned) and release-msi.yml (signed executables).
on:
  workflow_call:

permissions:
  contents: read

jobs:
  msi:
    name: msi (${{ matrix.arch }})
    timeout-minutes: 60
    strategy:
      fail-fast: false
      matrix:
        include:
          - arch: x64
            runner: windows-2025
          - arch: arm64
            runner: windows-11-arm
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: actions/setup-dotnet@a98b56852c35b8e3190ac28c8c2271da59106c68 # v6.0.0
        with:
          dotnet-version: "8.0.x"
      - uses: actions/setup-python@5fda3b95a4ea91299a34e894583c3862153e4b97 # v7.0.0
        with:
          python-version: "3.12"
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          name: exes-${{ matrix.arch }}
          path: bin
      - name: Build the MSI
        shell: pwsh
        run: ./installer/build.ps1 -Arch ${{ matrix.arch }} -BinDir bin -OutDir dist
      - name: Build a higher-version MSI for the upgrade test
        shell: pwsh
        run: ./installer/build.ps1 -Arch ${{ matrix.arch }} -BinDir bin -OutDir dist-next -Version Next
      - name: Check the MSI's tables
        shell: pwsh
        run: ./installer/test_msi_tables.ps1 (Get-ChildItem dist/*.msi).FullName
      - name: Install, upgrade and uninstall
        run: python installer/test_msi.py dist dist-next
      - name: Keep the msiexec logs of a failed run
        if: failure()
        uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: msi-logs-${{ matrix.arch }}
          path: msi-logs/
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: msi-${{ matrix.arch }}
          path: dist/*.msi
          if-no-files-found: error
```

- [ ] **Step 2: Replace the `windows` job of `.github/workflows/build.yml`**

Change the header and `on:` to:

```yaml
name: Build packages
# Builds and verifies the release files (M6b design §7, Phase 3 spec §5). Called by
# packaging.yml (pull requests: everything, unsigned) and release.yml (msi: false; the
# MSIs are built by release-msi.yml from the signed executables).
on:
  workflow_call:
    inputs:
      msi:
        type: boolean
        default: true
```

Replace the whole `windows:` job with:

```yaml
  exes:
    name: exes (${{ matrix.arch }})
    timeout-minutes: 60
    strategy:
      fail-fast: false
      matrix:
        include:
          - arch: x64
            runner: windows-2025
          - arch: arm64
            runner: windows-11-arm
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02
        with:
          toolchain: "1.96"
      - uses: actions/setup-python@5fda3b95a4ea91299a34e894583c3862153e4b97 # v7.0.0
        with:
          python-version: "3.12"
      - run: cargo build --release --locked -p pyenv -p pyenv-shim -p pyenv-shimw
      - name: Static C runtime
        run: python ci/pe_imports.py --check target/release/pyenv.exe target/release/pyenv-shim.exe target/release/pyenv-shimw.exe
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: exes-${{ matrix.arch }}
          path: |
            target/release/pyenv.exe
            target/release/pyenv-shim.exe
            target/release/pyenv-shimw.exe
          if-no-files-found: error

  msi:
    needs: exes
    if: inputs.msi
    uses: ./.github/workflows/msi.yml
```

The `linux:` job stays as it is.

- [ ] **Step 3: Delete `installer/sign.ps1`, then update `packaging.yml`**

Run: `git rm installer/sign.ps1`

In `.github/workflows/packaging.yml`, add these paths after `"ci/pe_imports.py"`:

```yaml
      - "ci/signing.py"
      - "ci/check_signed.py"
      - "ci/requirements-signing.txt"
      - "ci/signing_selftest.ps1"
```

Then add these after `".github/workflows/release.yml"`:

```yaml
      - ".github/workflows/msi.yml"
      - ".github/workflows/release-msi.yml"
      - ".github/workflows/release-finish.yml"
```

Remove `secrets: inherit` from the `build` job; the workflows use no secrets now. Make the same change in `release.yml` in Task 5.

- [ ] **Step 4: Check that the YAML parses and that nothing still refers to the removed script**

Run: `python -c "import sys, yaml; [yaml.safe_load(open(p, encoding='utf-8')) for p in sys.argv[1:]]; print('ok')" .github/workflows/build.yml .github/workflows/msi.yml .github/workflows/packaging.yml`
Expected: `ok`

Use the Grep tool: pattern `sign\.ps1|RPYENV_SIGN_TOOL`, glob `!crates/**`. Expected: matches only in `docs/superpowers/` (historical specs and plans) and `README.md`/`docs/windows-install.md`, if any; there must be none in `.github/` or `installer/`.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/build.yml .github/workflows/msi.yml .github/workflows/packaging.yml
git commit -m "CI: build.yml builds the executables in their own job; the MSI steps move to the reusable msi.yml; sign.ps1 removed"
```

---

### Task 4: The signing self-test in pull-request packaging

**Files:**
- Create: `ci/signing_selftest.ps1`
- Modify: `.github/workflows/packaging.yml` (new job)

**Interfaces:**
- Consumes: Task 2's `check_signed.py` CLI and output lines; Task 3's `exes-x64` and `msi-x64` artifacts.
- Produces: the `signing-selftest` job. It passes only when check_signed accepts files whose only change is a signature, and rejects files that differ in one byte (spec §8.1, §8.2).

- [ ] **Step 1: Write `ci/signing_selftest.ps1`**

```powershell
# Phase 3 spec §8: proves ci/check_signed.py accepts a file that only gained a signature and
# rejects one that differs by one byte. Signs copies of this run's x64 build with a throwaway
# self-signed certificate, trusted in LocalMachine\Root on the GitHub runner only.
param([Parameter(Mandatory)] [string] $Exes, [Parameter(Mandatory)] [string] $Msi)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$version = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value
$signtool = (Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\signtool.exe" |
    Sort-Object { [version]$_.Directory.Parent.Name } | Select-Object -Last 1).FullName
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=rpyenv CI self-test' `
    -CertStoreLocation Cert:\CurrentUser\My -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
    -NotAfter (Get-Date).AddDays(1)
$cer = Join-Path $env:RUNNER_TEMP 'selftest.cer'
Export-Certificate -Cert $cert -FilePath $cer | Out-Null
Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
$work = Join-Path $env:RUNNER_TEMP 'selftest'

function Copy-To([string] $Dir, [string[]] $Files) {
    New-Item -ItemType Directory -Force $Dir | Out-Null
    foreach ($f in $Files) { Copy-Item $f $Dir }
    (Get-ChildItem $Dir -File).FullName
}
function Sign([string[]] $Files) {
    & $signtool sign /sha1 $cert.Thumbprint /fd sha256 /tr http://timestamp.digicert.com /td sha256 /d rpyenv $Files
    if ($LASTEXITCODE) { throw "signtool sign failed ($LASTEXITCODE)" }
}
function Flip-Byte([string] $File, [int] $Offset) {
    $b = [IO.File]::ReadAllBytes($File); $b[$Offset] = $b[$Offset] -bxor 1; [IO.File]::WriteAllBytes($File, $b)
}
function Check([string] $Kind, [string[]] $Unsigned, [string[]] $Signed) {
    $zips = Join-Path $work "zips-$Kind-$([guid]::NewGuid())"
    New-Item -ItemType Directory $zips | Out-Null
    Compress-Archive -Path $Unsigned -DestinationPath (Join-Path $zips "tmp-unsigned-$Kind-x64.zip")
    Compress-Archive -Path $Signed -DestinationPath (Join-Path $zips "tmp-signed-$Kind-x64.zip")
    $out = & python ci/check_signed.py $Kind --version $version --zips $zips --out (Join-Path $zips out) `
        --no-attestation --thumbprint $cert.Thumbprint --arches x64 2>&1 | Out-String
    $code = $LASTEXITCODE
    Write-Host $out
    @{ Code = $code; Text = $out }
}
function Expect([string] $Case, $Result, [int] $Code, [string] $Pattern) {
    if ($Result.Code -ne $Code -or $Result.Text -notmatch $Pattern) {
        throw "self-test '$Case': expected exit $Code and /$Pattern/, got exit $($Result.Code)"
    }
    Write-Host "self-test '$Case': as expected"
}

Push-Location $repo
try {
    $u = Copy-To (Join-Path $work 'u-exes') (Get-ChildItem $Exes -Filter *.exe).FullName
    $s = Copy-To (Join-Path $work 's-exes') $u
    Sign $s
    # 1. Only a signature added: every executable passes. If this fails, the comparison
    #    method in ci/signing.py is wrong for what signtool writes (spec §8.1).
    Expect 'exes signed' (Check exes $u $s) 0 'ok x64/pyenv\.exe'
    # 2. The unsigned reference differs by one byte in the code: the comparison alone must fail.
    $m = Copy-To (Join-Path $work 'm-exes') $u
    Flip-Byte (Join-Path $work 'm-exes\pyenv.exe') 0x400
    Expect 'exes one byte' (Check exes $m $s) 1 'FAIL x64/pyenv\.exe: differs from the unsigned file'
    # 3. The signed file changed after signing: the whole check must fail.
    $t = Copy-To (Join-Path $work 't-exes') $s
    Flip-Byte (Join-Path $work 't-exes\pyenv.exe') 0x400
    Expect 'exes tampered' (Check exes $u $t) 1 'FAIL x64/pyenv\.exe'

    $um = Copy-To (Join-Path $work 'u-msi') (Get-ChildItem $Msi -Filter *.msi).FullName
    $sm = Copy-To (Join-Path $work 's-msi') $um
    Sign $sm
    # 4. Only a signature added to the MSI.
    Expect 'msi signed' (Check msi $um $sm) 0 "ok x64/rpyenv-$([regex]::Escape($version))-x64\.msi"
    # 5. The unsigned MSI's largest stream differs by one byte: the comparison must fail.
    $mm = Copy-To (Join-Path $work 'm-msi') $um
    python -c "import sys, olefile; o = olefile.OleFileIO(sys.argv[1], write_mode=True); e = max(o.listdir(), key=o.get_size); d = bytearray(o.openstream(e).read()); d[0] ^= 1; o.write_stream(e, bytes(d)); o.close()" $mm[0]
    if ($LASTEXITCODE) { throw "could not change the MSI stream" }
    Expect 'msi one byte' (Check msi $mm $sm) 1 'differs from the unsigned file'
}
finally { Pop-Location }
```

- [ ] **Step 2: Add the job to `.github/workflows/packaging.yml`**

```yaml
  signing-selftest:
    name: signing check self-test
    needs: build
    runs-on: windows-2025
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: actions/setup-python@5fda3b95a4ea91299a34e894583c3862153e4b97 # v7.0.0
        with:
          python-version: "3.12"
      - run: python -m pip install --require-hashes --only-binary ":all:" -r ci/requirements-signing.txt
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          name: exes-x64
          path: art/exes
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          name: msi-x64
          path: art/msi
      - name: Sign with a throwaway certificate; check what must pass and what must fail
        shell: pwsh
        run: ./ci/signing_selftest.ps1 -Exes art/exes -Msi art/msi
```

- [ ] **Step 3: Check that the YAML parses**

Run: `python -c "import yaml; yaml.safe_load(open('.github/workflows/packaging.yml', encoding='utf-8')); print('ok')"`
Expected: `ok`. The real test is the PR's CI run in Task 9. This script never runs on the development host: it imports a certificate into `LocalMachine\Root`.

- [ ] **Step 4: Commit**

```bash
git add ci/signing_selftest.ps1 .github/workflows/packaging.yml
git commit -m "CI: pull-request packaging signs a build with a throwaway certificate; check_signed must pass it and fail one-byte changes"
```

---

### Task 5: Step 1 in `release.yml`

**Files:**
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: Task 3's `build.yml` input `msi` and the artifacts `exes-<arch>` and `tar-<arch>`.
- Produces a draft with `rpyenv-<v>-linux-<arch>.tar.gz`, `install.sh` and `tmp-unsigned-exes-<arch>.zip`, with `targetCommitish` = `$GITHUB_SHA`. Every unsigned exe, both tarballs and `install.sh` are attested by `release.yml`.

- [ ] **Step 1: Rewrite `.github/workflows/release.yml`**

```yaml
name: Release
# Step 1 of a release (Phase 3 spec §4): a v* tag builds the executables and tarballs,
# attests them and drafts the release with the unsigned executables as tmp-* zips. Then
# `python ci/release_sign.py <tag>` signs on the maintainer's PC (docs/releasing.md). Run
# by hand, it does the same into a pre-release draft v<version>-dryrun.<run>, which creates
# no tag until someone publishes it: delete that draft afterwards.
on:
  push:
    tags: ["v*"]
  workflow_dispatch:

permissions:
  contents: read

jobs:
  version:
    runs-on: ubuntu-latest
    outputs:
      version: ${{ steps.v.outputs.version }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - id: v
        run: |
          version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
          echo "version=$version" >>"$GITHUB_OUTPUT"
          if [ "$GITHUB_REF_TYPE" = tag ] && [ "$GITHUB_REF_NAME" != "v$version" ]; then
            echo "the tag $GITHUB_REF_NAME doesn't match the workspace version $version" >&2
            exit 1
          fi

  build:
    needs: version
    uses: ./.github/workflows/build.yml
    with:
      msi: false

  draft:
    needs: [version, build]
    runs-on: ubuntu-latest
    permissions:
      contents: write
      id-token: write
      attestations: write
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          path: art
      - name: Arrange the draft's files
        run: |
          mkdir assets
          cp install.sh art/tar-*/*.tar.gz assets/
          for arch in x64 arm64; do
            (cd "art/exes-$arch" && zip -X "../../assets/tmp-unsigned-exes-$arch.zip" pyenv.exe pyenv-shim.exe pyenv-shimw.exe)
          done
          ls -l assets
      - uses: actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8 # v4.2.2
        with:
          subject-path: |
            art/exes-*/*.exe
            assets/*.tar.gz
            assets/install.sh
      - name: Draft the release
        env:
          GH_TOKEN: ${{ github.token }}
          VERSION: ${{ needs.version.outputs.version }}
        run: |
          if [ "$GITHUB_REF_TYPE" = tag ]; then
            gh release create "$GITHUB_REF_NAME" assets/* --draft --target "$GITHUB_SHA" \
              --generate-notes --title "rpyenv $VERSION"
          else
            tag="v$VERSION-dryrun.$GITHUB_RUN_NUMBER"
            gh release create "$tag" assets/* --draft --prerelease --target "$GITHUB_SHA" \
              --generate-notes --title "rpyenv $VERSION (dry run $GITHUB_RUN_NUMBER)"
          fi
          echo "Next: python ci/release_sign.py <tag> on the PC with the card (docs/releasing.md)"
```

- [ ] **Step 2: Check that the YAML parses**

Run: `python -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml', encoding='utf-8')); print('ok')"`
Expected: `ok`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "Release step 1: attest the unsigned executables, tarballs and install.sh; draft them with the executables as tmp-unsigned zips"
```

---

### Task 6: The maintainer's script (`ci/release_sign.py`)

**Files:**
- Create: `ci/release_sign.py`
- Test: `ci/test_release_sign.py`

**Interfaces:**
- Consumes: all of Task 1's `signing` API.
- Produces: `python ci/release_sign.py <tag> [--check] [--retry] [--signtool PATH]`, plus these module functions used by the tests:
  - `state(names: set, arches: list) -> str`, one of `done`, `await-finish`, `sign-msi`, `await-msi`, `sign-exes`, `unknown`
  - `gh(*args) -> str` (raises `Stop`)
  - `gh_live(*args) -> int`
  - `card_state(thumbprint) -> str`
  - `run_signtool(signtool, cfg, timestamp, files) -> (code, output)`
  - `main(argv) -> int`
- Workflow run names that Task 7 must match: `release-msi <tag> <request>` and `release-finish <tag> <request>`.

- [ ] **Step 1: Write the failing tests in `ci/test_release_sign.py`**

```python
"""Offline tests of ci/release_sign.py (Phase 3 spec §6) against a fake gh and signtool."""
import contextlib
import io
import json
import os
import sys
import tempfile
import unittest
import zipfile
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import release_sign as rs  # noqa: E402
import signing  # noqa: E402

ARCHES = ["x64", "arm64"]
COMMIT = "c" * 40
EXES = ["pyenv-shim.exe", "pyenv-shimw.exe", "pyenv.exe"]


def step1_assets(version="0.2.0"):
    return {f"rpyenv-{version}-linux-x64.tar.gz", f"rpyenv-{version}-linux-arm64.tar.gz", "install.sh",
            "tmp-unsigned-exes-x64.zip", "tmp-unsigned-exes-arm64.zip"}


class FakeGitHub:
    """A draft release and workflow runs, as `gh` would report them."""

    def __init__(self, tag, assets, outcomes=None, register_runs=True):
        self.tag, self.version = tag, signing.parse_tag(tag)[0]
        self.assets = set(assets) if assets is not None else None
        self.is_draft = True
        self.runs = []
        self.outcomes = outcomes or {}  # workflow stem -> list of "success"/"failure"
        self.register_runs = register_runs
        self.dispatched, self.uploaded = [], []
        self.extra_in_zip = None

    def zip_names(self, asset):
        kind = "exes" if "-exes-" in asset else "msi"
        arch = asset.rsplit("-", 1)[1].removesuffix(".zip")
        names = list(EXES) if kind == "exes" else [f"rpyenv-{self.version}-{arch}.msi"]
        return names + ([self.extra_in_zip] if self.extra_in_zip else [])

    def gh(self, *args):
        a = list(args)
        if a[:2] == ["release", "view"]:
            if self.assets is None:
                raise rs.Stop("gh release view: release not found")
            return json.dumps({"isDraft": self.is_draft, "targetCommitish": COMMIT, "url": "https://draft",
                               "assets": [{"name": n} for n in sorted(self.assets)]})
        if a[:2] == ["release", "download"]:
            dest = a[a.index("-D") + 1]
            os.makedirs(dest, exist_ok=True)
            for i, x in enumerate(a):
                if x == "-p" and a[i + 1] in self.assets:
                    with zipfile.ZipFile(os.path.join(dest, a[i + 1]), "w") as z:
                        for n in self.zip_names(a[i + 1]):
                            z.writestr(n, b"MZ unsigned")
            return ""
        if a[:2] == ["release", "upload"]:
            files = [x for x in a[3:] if x.endswith(".zip")]
            self.uploaded.append(sorted(os.path.basename(f) for f in files))
            self.assets |= {os.path.basename(f) for f in files}
            return ""
        if a[0] == "api":
            return COMMIT + "\n"
        if a[:2] == ["workflow", "run"]:
            stem = a[2].removesuffix(".yml")
            ref = a[a.index("--ref") + 1]
            request = [x for x in a if x.startswith("request=")][0].split("=", 1)[1]
            self.dispatched.append((stem, ref))
            if self.register_runs:
                self.runs.insert(0, {"databaseId": len(self.runs) + 1, "displayTitle": f"{stem} {self.tag} {request}",
                                     "status": "in_progress", "conclusion": "", "url": f"https://run/{stem}",
                                     "stem": stem})
            return ""
        if a[:2] == ["run", "list"]:
            stem = a[a.index("--workflow") + 1].removesuffix(".yml")
            return json.dumps([r for r in self.runs if r["stem"] == stem])
        raise AssertionError(f"unexpected gh {a}")

    def gh_live(self, *args):
        assert args[:2] == ("run", "watch"), args
        run = next(r for r in self.runs if r["databaseId"] == int(args[2]))
        outcome = (self.outcomes.get(run["stem"]) or ["success"]).pop(0)
        run.update(status="completed", conclusion=outcome)
        if outcome == "success" and run["stem"] == "release-msi":
            self.assets |= {"tmp-unsigned-msi-x64.zip", "tmp-unsigned-msi-arm64.zip"}
        if outcome == "success" and run["stem"] == "release-finish":
            self.assets = {n for n in self.assets if not n.startswith("tmp-")}
            self.assets |= {f"rpyenv-{self.version}-x64.msi", f"rpyenv-{self.version}-arm64.msi", "SHA256SUMS"}
        return 0 if outcome == "success" else 1


class Flow(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.signed_calls, self.signtool_results = [], []
        self.already_signed, self.attest_fail = set(), set()
        self.card = "ok"
        self.workdirs = []

    def tearDown(self):
        self.tmp.cleanup()

    def mkdtemp(self, prefix=""):
        d = os.path.join(self.tmp.name, f"work{len(self.workdirs)}")
        os.makedirs(d)
        self.workdirs.append(d)
        return d

    def fake_signtool(self, signtool, cfg, timestamp, files):
        self.signed_calls.append((timestamp, sorted(os.path.relpath(f, self.workdirs[-1]) for f in files)))
        for f in files:
            with open(f, "rb") as fh:
                assert fh.read() == b"MZ unsigned", "signed twice"
            with open(f, "wb") as fh:
                fh.write(b"MZ signed")
        return self.signtool_results.pop(0) if self.signtool_results else (0, "")

    def fake_attest(self, path, repo, workflow, commit):
        if os.path.basename(path) in self.attest_fail:
            raise signing.SigningError(f"{os.path.basename(path)}: attestation: no match")

    def fake_info(self, path):
        if os.path.basename(path) in self.already_signed:
            return signing.SigInfo("Valid", "X", True)
        return signing.SigInfo("NotSigned", "", False)

    def run_main(self, fake, *argv):
        buf = io.StringIO()
        patches = [
            mock.patch.object(rs, "gh", fake.gh),
            mock.patch.object(rs, "gh_live", fake.gh_live),
            mock.patch.object(rs, "card_state", lambda t: self.card),
            mock.patch.object(rs, "run_signtool", self.fake_signtool),
            mock.patch.object(rs.time, "sleep", lambda s: None),
            mock.patch.object(rs.tempfile, "mkdtemp", self.mkdtemp),
            mock.patch.object(signing, "find_signtool", lambda override=None: "signtool.exe"),
            mock.patch.object(signing, "verify_attestation", self.fake_attest),
            mock.patch.object(signing, "signature_info", self.fake_info),
            mock.patch.object(signing, "signature_problems", lambda p, t, s: []),
        ]
        with contextlib.ExitStack() as stack:
            for p in patches:
                stack.enter_context(p)
            stack.enter_context(contextlib.redirect_stdout(buf))
            stack.enter_context(contextlib.redirect_stderr(buf))
            code = rs.main(list(argv))
        return code, buf.getvalue()

    def test_a_dry_run_goes_from_step_1_to_done(self):
        fake = FakeGitHub("v0.2.0-dryrun.7", step1_assets())
        code, text = self.run_main(fake, "v0.2.0-dryrun.7")
        self.assertEqual(code, 0, text)
        self.assertEqual(fake.dispatched, [("release-msi", "main"), ("release-finish", "main")])
        self.assertEqual(fake.uploaded, [["tmp-signed-exes-arm64.zip", "tmp-signed-exes-x64.zip"],
                                         ["tmp-signed-msi-arm64.zip", "tmp-signed-msi-x64.zip"]])
        self.assertEqual([len(files) for _, files in self.signed_calls], [6, 2])
        self.assertEqual({t for t, _ in self.signed_calls}, {"http://time.certum.pl"})
        self.assertIn("https://draft", text)
        self.assertTrue(all(not os.path.exists(d) for d in self.workdirs))

    def test_a_release_dispatches_on_its_tag(self):
        fake = FakeGitHub("v0.2.0", step1_assets())
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 0, text)
        self.assertEqual(fake.dispatched, [("release-msi", "v0.2.0"), ("release-finish", "v0.2.0")])

    def test_check_only_reads(self):
        fake = FakeGitHub("v0.2.0", step1_assets())
        code, text = self.run_main(fake, "v0.2.0", "--check")
        self.assertEqual(code, 0)
        self.assertIn("sign-exes", text)
        self.assertEqual((fake.dispatched, fake.uploaded, self.signed_calls), ([], [], []))

    def test_card_missing_stops_before_downloading(self):
        self.card = "missing"
        fake = FakeGitHub("v0.2.0", step1_assets())
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("Insert the Certum card", text)
        self.assertEqual((self.workdirs, self.signed_calls), ([], []))

    def test_extra_file_stops_before_signing(self):
        fake = FakeGitHub("v0.2.0", step1_assets())
        fake.extra_in_zip = "evil.exe"
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("evil.exe", text)
        self.assertEqual(self.signed_calls, [])
        self.assertTrue(all(not os.path.exists(d) for d in self.workdirs))

    def test_already_signed_stops_before_signing(self):
        self.already_signed.add("pyenv.exe")
        code, text = self.run_main(FakeGitHub("v0.2.0", step1_assets()), "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("already signed", text)
        self.assertEqual(self.signed_calls, [])

    def test_attestation_failure_stops_before_signing(self):
        self.attest_fail.add("pyenv-shimw.exe")
        code, text = self.run_main(FakeGitHub("v0.2.0", step1_assets()), "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("pyenv-shimw.exe: attestation", text)
        self.assertEqual(self.signed_calls, [])

    def test_timestamp_fallback_uses_fresh_copies(self):
        self.signtool_results = [(1, "SignTool Error: The specified timestamp server either could not be reached"), (0, "")]
        fake = FakeGitHub("v0.2.0", step1_assets())
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 0, text)
        self.assertEqual([t for t, _ in self.signed_calls][:2], ["http://time.certum.pl", "http://timestamp.digicert.com"])

    def test_other_signtool_errors_stop(self):
        self.signtool_results = [(1, "SignTool Error: The smart card was removed")]
        fake = FakeGitHub("v0.2.0", step1_assets())
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("smart card was removed", text)
        self.assertEqual(len(self.signed_calls), 1)
        self.assertEqual(fake.uploaded, [])

    def test_a_failed_ci_step_needs_retry(self):
        fake = FakeGitHub("v0.2.0", step1_assets(), outcomes={"release-msi": ["failure"]})
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("https://run/release-msi", text)
        self.assertIn("--retry", text)
        self.assertEqual(fake.dispatched, [("release-msi", "v0.2.0")])
        code, text = self.run_main(fake, "v0.2.0", "--retry")
        self.assertEqual(code, 0, text)
        self.assertEqual(fake.dispatched[1:], [("release-msi", "v0.2.0"), ("release-finish", "v0.2.0")])
        self.assertEqual(len(self.signed_calls), 2)

    def test_a_run_that_never_appears_stops(self):
        fake = FakeGitHub("v0.2.0", step1_assets(), register_runs=False)
        clock = iter(range(0, 10000, 10))
        with mock.patch.object(rs.time, "monotonic", lambda: next(clock)):
            code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("found no run", text)

    def test_no_draft(self):
        code, text = self.run_main(FakeGitHub("v0.2.0", None), "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("no draft for v0.2.0", text)

    def test_published_release(self):
        fake = FakeGitHub("v0.2.0", step1_assets())
        fake.is_draft = False
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("already published", text)

    def test_bad_tag(self):
        code, text = self.run_main(FakeGitHub("v0.2.0", step1_assets()), "0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("not a release tag", text)


class State(unittest.TestCase):
    def s(self, *names):
        return rs.state(set(names), ARCHES)

    def test_state_each_step(self):
        ue = ("tmp-unsigned-exes-x64.zip", "tmp-unsigned-exes-arm64.zip")
        se = ("tmp-signed-exes-x64.zip", "tmp-signed-exes-arm64.zip")
        um = ("tmp-unsigned-msi-x64.zip", "tmp-unsigned-msi-arm64.zip")
        sm = ("tmp-signed-msi-x64.zip", "tmp-signed-msi-arm64.zip")
        self.assertEqual(self.s(*ue), "sign-exes")
        self.assertEqual(self.s(*ue, *se), "await-msi")
        self.assertEqual(self.s(*ue, *se, *um), "sign-msi")
        self.assertEqual(self.s(*ue, *se, *um, *sm), "await-finish")
        self.assertEqual(self.s("SHA256SUMS", "install.sh"), "done")

    def test_state_half_uploaded_arch_repeats_the_step(self):
        self.assertEqual(self.s("tmp-unsigned-exes-x64.zip", "tmp-unsigned-exes-arm64.zip",
                                "tmp-signed-exes-x64.zip"), "sign-exes")
        self.assertEqual(self.s("tmp-unsigned-exes-x64.zip", "tmp-unsigned-exes-arm64.zip",
                                "tmp-signed-exes-x64.zip", "tmp-signed-exes-arm64.zip",
                                "tmp-unsigned-msi-x64.zip"), "await-msi")

    def test_state_finish_half_done_never_goes_back(self):
        self.assertEqual(self.s("SHA256SUMS", "tmp-unsigned-exes-x64.zip", "tmp-unsigned-exes-arm64.zip"),
                         "await-finish")

    def test_state_unknown(self):
        self.assertEqual(self.s("install.sh"), "unknown")


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run the tests and check that they fail**

Run: `python -m unittest discover -s ci -p "test_release_sign.py" -v`
Expected: `ModuleNotFoundError: No module named 'release_sign'`.

- [ ] **Step 3: Write `ci/release_sign.py`**

```python
"""Signs a release's Windows files with the maintainer's Certum card (Phase 3 spec §6).

  python ci/release_sign.py <tag> [--check] [--retry] [--signtool PATH]

<tag> is v1.2.3 (a pushed tag) or v1.2.3-dryrun.N (a dry run's draft). The draft's assets are
the only state: the script does the next step, waits for CI and goes on until the draft is
finished or something needs the maintainer. Before signing it checks the card, the
attestations and the exact file names; the card software asks for the PIN, which this script
never sees. Downloads go to a temporary folder that is removed at the end. docs/releasing.md
has the whole procedure.
"""
import argparse
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import signing  # noqa: E402

NEXT = {"exes": "release-msi.yml", "msi": "release-finish.yml"}
FIND_RUN_SECONDS = 120
CARD = {
    "missing": "Insert the Certum card: its certificate is not in CurrentUser\\My",
    "nokey": "the certificate has no private key here; is the card software running?",
    "expired": "the certificate is not valid today",
}
PS_CARD = (
    '$c = Get-Item -LiteralPath ("Cert:\\CurrentUser\\My\\" + $env:RPYENV_THUMBPRINT) -ErrorAction SilentlyContinue; '
    'if (-not $c) { "missing" } elseif (-not $c.HasPrivateKey) { "nokey" } '
    'elseif ($c.NotBefore -gt (Get-Date) -or $c.NotAfter -lt (Get-Date)) { "expired" } else { "ok" }'
)


class Stop(Exception):
    """Ends the run with a message for the maintainer (exit 1)."""


def gh(*args):
    r = subprocess.run(["gh", *args], capture_output=True, text=True, encoding="utf-8")
    if r.returncode:
        raise Stop(f"gh {' '.join(args[:3])}: {r.stderr.strip()[-300:]}")
    return r.stdout


def gh_live(*args):
    """gh with its output on the terminal (gh run watch); its exit code."""
    return subprocess.run(["gh", *args]).returncode


def card_state(thumbprint):
    r = subprocess.run(["pwsh", "-NoProfile", "-NonInteractive", "-Command", PS_CARD],
                       env={**os.environ, "RPYENV_THUMBPRINT": thumbprint},
                       capture_output=True, text=True, encoding="utf-8")
    return r.stdout.strip() if r.returncode == 0 else f"error: {r.stderr.strip()[-300:]}"


def run_signtool(signtool, cfg, timestamp, files):
    r = subprocess.run([signtool, "sign", "/sha1", cfg["thumbprint"], "/fd", "sha256", "/tr", timestamp,
                        "/td", "sha256", "/d", cfg["description"], "/du", cfg["url"], *files],
                       capture_output=True, text=True, errors="replace")
    return r.returncode, r.stdout + r.stderr


def state(names, arches):
    """The draft's step, from its asset names alone (spec §6.1)."""
    def every(stage, kind):
        return all(signing.zip_name(stage, kind, a) in names for a in arches)

    if "SHA256SUMS" in names:
        return "await-finish" if any(n.startswith("tmp-") for n in names) else "done"
    if every("signed", "msi"):
        return "await-finish"
    if every("unsigned", "msi"):
        return "sign-msi"
    if every("signed", "exes"):
        return "await-msi"
    if every("unsigned", "exes"):
        return "sign-exes"
    return "unknown"


def draft(repo, tag):
    try:
        return json.loads(gh("release", "view", tag, "-R", repo, "--json", "isDraft,targetCommitish,url,assets"))
    except Stop as e:
        if "not found" in str(e).lower():
            return None
        raise


def commit_of(repo, tag, dry_run, rel):
    c = rel["targetCommitish"] if dry_run else gh("api", f"repos/{repo}/commits/{tag}", "--jq", ".sha").strip()
    if not re.fullmatch(r"[0-9a-f]{40}", c):
        raise Stop(f"{tag}: the draft's commit {c!r} is not a commit hash")
    return c


def sign_copies(signtool, cfg, originals, dest):
    """Copies the files to dest/<arch>/ and signs them in one call, once per timestamp server."""
    last = ""
    for url in cfg["timestamp"]:
        shutil.rmtree(dest, ignore_errors=True)
        copies = []
        for arch, paths in originals.items():
            os.makedirs(os.path.join(dest, arch))
            copies += [shutil.copy2(p, os.path.join(dest, arch)) for p in paths]
        code, output = run_signtool(signtool, cfg, url, copies)
        if code == 0:
            return copies
        last = output.strip()[-500:]
        if "timestamp" not in output.lower():
            break
        print(f"The timestamp server {url} failed; trying the next one (the PIN dialog may show again).", flush=True)
    raise Stop(f"signtool sign failed:\n{last}")


def sign_step(kind, cfg, repo, tag, version, commit, signtool_override):
    card = card_state(cfg["thumbprint"])
    if card != "ok":
        raise Stop(CARD.get(card, card))
    signtool = signing.find_signtool(signtool_override)
    names = signing.file_names(cfg, kind, version)
    work = tempfile.mkdtemp(prefix="rpyenv-sign.")
    try:
        zips = os.path.join(work, "zips")
        patterns = [x for arch in names for x in ("-p", signing.zip_name("unsigned", kind, arch))]
        gh("release", "download", tag, "-R", repo, "-D", zips, *patterns)
        originals = {}
        for arch, files in names.items():
            paths = signing.unzip_exact(os.path.join(zips, signing.zip_name("unsigned", kind, arch)),
                                        os.path.join(work, "unsigned", arch), files)
            for p in paths:
                signing.verify_attestation(p, repo, signing.BUILT_BY[kind], commit)
                info = signing.signature_info(p)
                if info.status != "NotSigned":
                    raise Stop(f"{arch}/{os.path.basename(p)} is already signed ({info.status})")
            originals[arch] = paths
        count = sum(len(p) for p in originals.values())
        print(f"Signing {count} files: enter the card PIN in the Certum dialog.", flush=True)
        signed = sign_copies(signtool, cfg, originals, os.path.join(work, "signed"))
        for p in signed:
            problems = signing.signature_problems(p, cfg["thumbprint"], signtool)
            if problems:
                raise Stop(f"{p}: " + "; ".join(problems))
        out = []
        for arch in names:
            z = os.path.join(work, signing.zip_name("signed", kind, arch))
            signing.make_zip(z, sorted(p for p in signed if os.path.basename(os.path.dirname(p)) == arch))
            out.append(z)
        gh("release", "upload", tag, "-R", repo, "--clobber", *out)
        print(f"Uploaded {', '.join(os.path.basename(z) for z in out)}.", flush=True)
    finally:
        shutil.rmtree(work, ignore_errors=True)


def runs(repo, workflow, tag):
    """This tag's runs of a workflow, newest first, found by their run name."""
    prefix = f"{workflow.removesuffix('.yml')} {tag} "
    out = gh("run", "list", "-R", repo, "--workflow", workflow, "--limit", "30",
             "--json", "databaseId,displayTitle,status,conclusion,url")
    return [r for r in json.loads(out) if r["displayTitle"].startswith(prefix)]


def dispatch(repo, workflow, tag, ref):
    request = secrets.token_hex(8)
    gh("workflow", "run", workflow, "-R", repo, "--ref", ref, "-f", f"tag={tag}", "-f", f"request={request}")
    deadline = time.monotonic() + FIND_RUN_SECONDS
    while time.monotonic() < deadline:
        for r in runs(repo, workflow, tag):
            if r["displayTitle"].endswith(" " + request):
                return r
        time.sleep(5)
    raise Stop(f"started {workflow} for {tag}, but found no run named with request {request} "
               f"after {FIND_RUN_SECONDS} s")


def follow(repo, run):
    print(f"Waiting for {run['url']}", flush=True)
    gh_live("run", "watch", str(run["databaseId"]), "-R", repo, "--exit-status", "--interval", "30")


def main(argv=None):
    ap = argparse.ArgumentParser(description="Sign a release's Windows files (docs/releasing.md).")
    ap.add_argument("tag")
    ap.add_argument("--check", action="store_true", help="print the draft's step; change nothing")
    ap.add_argument("--retry", action="store_true", help="start a failed CI step again (once)")
    ap.add_argument("--signtool", help="signtool.exe to use (default: the newest Windows SDK's)")
    a = ap.parse_args(argv)
    try:
        cfg = signing.load_config()
        repo = cfg["repo"]
        version, dry_run = signing.parse_tag(a.tag)
        ref = "main" if dry_run else a.tag
        retried = False
        while True:
            rel = draft(repo, a.tag)
            if rel is None:
                raise Stop(f"no draft for {a.tag}; push the tag or start a dry run (docs/releasing.md)")
            if not rel["isDraft"]:
                raise Stop(f"{a.tag} is already published")
            names = {x["name"] for x in rel["assets"]}
            st = state(names, cfg["arches"])
            print(f"{a.tag}: {st}", flush=True)
            if st == "done":
                print(f"Done. Review and publish the draft: {rel['url']}")
                return 0
            if a.check:
                return 0
            if st == "unknown":
                raise Stop(f"the draft's assets match no step: {sorted(names)}")
            commit = commit_of(repo, a.tag, dry_run, rel)
            if st.startswith("sign-"):
                kind = st.removeprefix("sign-")
                sign_step(kind, cfg, repo, a.tag, version, commit, a.signtool)
                follow(repo, dispatch(repo, NEXT[kind], a.tag, ref))
                continue
            workflow = NEXT["exes" if st == "await-msi" else "msi"]
            found = runs(repo, workflow, a.tag)
            last = found[0] if found else None
            if last is None:
                follow(repo, dispatch(repo, workflow, a.tag, ref))
            elif last["status"] != "completed":
                follow(repo, last)
            elif last["conclusion"] == "success":
                raise Stop(f"{last['url']} succeeded, but the draft is still at {st}; look at the draft")
            elif a.retry and not retried:
                retried = True
                follow(repo, dispatch(repo, workflow, a.tag, ref))
            else:
                raise Stop(f"{workflow} failed: {last['url']}\n"
                           f"Fix the cause, then run: python ci/release_sign.py {a.tag} --retry")
    except (Stop, signing.SigningError, OSError) as e:
        print(f"release_sign: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `python -m unittest discover -s ci -p "test_*.py"`
Expected: all pass. In `test_a_failed_ci_step_needs_retry`, the first run stops after step 3 fails. The second run, with `--retry`, starts step 3 again and goes on to the end. It signs the MSIs, so `signed_calls` ends with 2 entries, exes and MSIs.

- [ ] **Step 5: Commit**

```bash
git add ci/release_sign.py ci/test_release_sign.py
git commit -m "Signing: ci/release_sign.py signs a release's executables and MSIs from the draft's state (card, attestations and names checked first)"
```

---

### Task 7: Steps 3 and 5 (`release-msi.yml`, `release-finish.yml`, `ci/release_guard.sh`)

**Files:**
- Create: `ci/release_guard.sh`
- Create: `.github/workflows/release-msi.yml`
- Create: `.github/workflows/release-finish.yml`

**Interfaces:**
- Consumes:
  - Task 2's `check_signed.py`.
  - Task 3's `msi.yml`.
  - Task 6's run names (`release-msi <tag> <request>`, `release-finish <tag> <request>`) and the inputs `tag` and `request`.
- Produces:
  - After step 3: `tmp-unsigned-msi-<arch>.zip` on the draft, with the MSIs attested by `release-msi.yml`.
  - After step 5: the final MSIs and `SHA256SUMS`, every final file attested by `release-finish.yml`, and no `tmp-*` assets.

- [ ] **Step 1: Write `ci/release_guard.sh`**

```bash
#!/usr/bin/env bash
# Steps 3 and 5 of a release (Phase 3 spec §5): refuse unless this dispatched run belongs to
# the draft $TAG. A release runs on its tag; a dry run (v1.2.3-dryrun.N) runs on main, and
# main must still be the commit step 1 drafted.
set -euo pipefail
case "$TAG" in
  v[0-9]*.[0-9]*.[0-9]*-dryrun.[0-9]*) want=branch ;;
  v[0-9]*.[0-9]*.[0-9]*) want=tag ;;
  *) echo "not a release tag: $TAG" >&2; exit 1 ;;
esac
if [ "$GITHUB_REF_TYPE" != "$want" ]; then
  echo "$TAG must run on $([ "$want" = tag ] && echo "its tag" || echo main), not $GITHUB_REF" >&2
  exit 1
fi
if [ "$want" = tag ] && [ "$GITHUB_REF_NAME" != "$TAG" ]; then
  echo "this run is on $GITHUB_REF_NAME, not $TAG" >&2
  exit 1
fi
if [ "$want" = branch ] && [ "$GITHUB_REF_NAME" != main ]; then
  echo "a dry run's steps run on main, not $GITHUB_REF_NAME" >&2
  exit 1
fi
info=$(gh release view "$TAG" --json isDraft,targetCommitish --jq '"\(.isDraft) \(.targetCommitish)"')
if [ "${info%% *}" != true ]; then
  echo "$TAG is not a draft" >&2
  exit 1
fi
if [ "$want" = branch ] && [ "${info#* }" != "$GITHUB_SHA" ]; then
  echo "main moved since step 1 (${info#* } drafted, now $GITHUB_SHA); start a new dry run" >&2
  exit 1
fi
echo "release guard: $TAG on $GITHUB_REF at $GITHUB_SHA"
```

- [ ] **Step 2: Write `.github/workflows/release-msi.yml`**

```yaml
name: Release MSIs
# Step 3 of a release (Phase 3 spec §5.1), started by ci/release_sign.py: checks the signed
# executables against the attested unsigned ones, builds and tests the MSIs from them,
# attests the unsigned MSIs and puts them on the draft for signing.
run-name: release-msi ${{ inputs.tag }} ${{ inputs.request }}
on:
  workflow_dispatch:
    inputs:
      tag:
        required: true
        type: string
      request:
        required: true
        type: string

permissions:
  contents: read

jobs:
  exes:
    name: check the signed executables
    runs-on: windows-2025
    timeout-minutes: 20
    # Draft releases are visible only to tokens with push access.
    permissions:
      contents: write
    env:
      GH_TOKEN: ${{ github.token }}
      GH_REPO: ${{ github.repository }}
      TAG: ${{ inputs.tag }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: actions/setup-python@5fda3b95a4ea91299a34e894583c3862153e4b97 # v7.0.0
        with:
          python-version: "3.12"
      - name: This run belongs to the draft
        shell: bash
        run: bash ci/release_guard.sh
      - name: Download the hand-off
        shell: bash
        run: gh release download "$TAG" -D zips -p "tmp-unsigned-exes-*.zip" -p "tmp-signed-exes-*.zip"
      - name: Signed = attested + signature, pinned signer, timestamped
        run: python ci/check_signed.py exes --zips zips --out signed --commit ${{ github.sha }}
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: exes-x64
          path: signed/x64/*.exe
          if-no-files-found: error
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: exes-arm64
          path: signed/arm64/*.exe
          if-no-files-found: error

  msi:
    needs: exes
    uses: ./.github/workflows/msi.yml

  upload:
    needs: msi
    runs-on: ubuntu-latest
    permissions:
      contents: write
      id-token: write
      attestations: write
    env:
      GH_TOKEN: ${{ github.token }}
      GH_REPO: ${{ github.repository }}
      TAG: ${{ inputs.tag }}
    steps:
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          name: msi-x64
          path: art/x64
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          name: msi-arm64
          path: art/arm64
      - uses: actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8 # v4.2.2
        with:
          subject-path: art/*/*.msi
      - name: Put the unsigned MSIs on the draft
        run: |
          for arch in x64 arm64; do
            (cd "art/$arch" && zip -X "../../tmp-unsigned-msi-$arch.zip" -- *.msi)
          done
          gh release upload "$TAG" tmp-unsigned-msi-x64.zip tmp-unsigned-msi-arm64.zip --clobber
```

The zip holds plain names (`rpyenv-<v>-<arch>.msi`); `check_signed` refuses any other name, so the first dry run (Task 10) would show a mistake here.

- [ ] **Step 3: Write `.github/workflows/release-finish.yml`**

```yaml
name: Release finish
# Step 5 of a release (Phase 3 spec §5.2), started by ci/release_sign.py: checks the signed
# MSIs against the attested unsigned ones, writes SHA256SUMS, attests the final files, puts
# them on the draft and removes the tmp-* hand-off files. The draft stays a draft.
run-name: release-finish ${{ inputs.tag }} ${{ inputs.request }}
on:
  workflow_dispatch:
    inputs:
      tag:
        required: true
        type: string
      request:
        required: true
        type: string

permissions:
  contents: read

jobs:
  msi:
    name: check the signed MSIs
    runs-on: windows-2025
    timeout-minutes: 20
    # Draft releases are visible only to tokens with push access.
    permissions:
      contents: write
    env:
      GH_TOKEN: ${{ github.token }}
      GH_REPO: ${{ github.repository }}
      TAG: ${{ inputs.tag }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: actions/setup-python@5fda3b95a4ea91299a34e894583c3862153e4b97 # v7.0.0
        with:
          python-version: "3.12"
      - run: python -m pip install --require-hashes --only-binary ":all:" -r ci/requirements-signing.txt
      - name: This run belongs to the draft
        shell: bash
        run: bash ci/release_guard.sh
      - name: Download the hand-off
        shell: bash
        run: gh release download "$TAG" -D zips -p "tmp-unsigned-msi-*.zip" -p "tmp-signed-msi-*.zip"
      - name: Signed = attested + signature, pinned signer, timestamped
        shell: bash
        run: |
          version=${TAG#v}
          version=${version%%-dryrun.*}
          python ci/check_signed.py msi --version "$version" --zips zips --out signed --commit ${{ github.sha }}
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: final-msi
          path: signed/*/*.msi
          if-no-files-found: error

  publish:
    needs: msi
    runs-on: ubuntu-latest
    permissions:
      contents: write
      id-token: write
      attestations: write
    env:
      GH_TOKEN: ${{ github.token }}
      GH_REPO: ${{ github.repository }}
      TAG: ${{ inputs.tag }}
    steps:
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          name: final-msi
          path: art
      - name: Gather the final files
        run: |
          mkdir assets
          cp art/*/*.msi assets/
          gh release download "$TAG" -D assets -p "rpyenv-*.tar.gz" -p install.sh
          for f in assets/*.tar.gz assets/install.sh; do
            gh attestation verify "$f" --repo "$GH_REPO" \
              --signer-workflow "$GH_REPO/.github/workflows/release.yml" \
              --source-digest "$GITHUB_SHA" --deny-self-hosted-runners
          done
          cd assets
          sha256sum rpyenv-* install.sh >SHA256SUMS
          cat SHA256SUMS
      - uses: actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8 # v4.2.2
        with:
          subject-path: "assets/*"
      - name: Final files on the draft; hand-off files removed
        run: |
          gh release upload "$TAG" assets/*.msi assets/SHA256SUMS --clobber
          gh release view "$TAG" --json assets --jq '.assets[].name' | grep '^tmp-' | while read -r name; do
            gh release delete-asset "$TAG" "$name" --yes
          done
          echo "Done: review and publish the draft $TAG"
```

- [ ] **Step 4: Check that the YAML parses and the guard is valid shell**

Run: `python -c "import sys, yaml; [yaml.safe_load(open(p, encoding='utf-8')) for p in sys.argv[1:]]; print('ok')" .github/workflows/release-msi.yml .github/workflows/release-finish.yml`
Expected: `ok`.

Run (WSL, via a script file per the WSL rules): `shellcheck ci/release_guard.sh`, if shellcheck is installed in WSL; otherwise `bash -n ci/release_guard.sh` in Git Bash.
Expected: no findings, or no output.

Test the guard's refusals in Git Bash with a stub `gh` on `PATH`. Write `C:\tmp\guard_test.sh`:

```bash
#!/usr/bin/env bash
# Runs ci/release_guard.sh against a stub gh; prints each case's exit code and message.
repo=/c/Users/JM/repos/OWN/rpyenv
stub=$(mktemp -d)
cat >"$stub/gh" <<'EOF'
#!/usr/bin/env bash
echo "$STUB_INFO"
EOF
chmod +x "$stub/gh"
run() { echo "--- $1"; shift; env PATH="$stub:$PATH" "$@" bash "$repo/ci/release_guard.sh"; echo "exit $?"; }
s=1111111111111111111111111111111111111111
run "release on its tag" TAG=v0.2.0 GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v0.2.0 GITHUB_REF=refs/tags/v0.2.0 GITHUB_SHA=$s STUB_INFO="true $s"
run "release on main" TAG=v0.2.0 GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main GITHUB_REF=refs/heads/main GITHUB_SHA=$s STUB_INFO="true $s"
run "other tag" TAG=v0.2.0 GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v0.1.0 GITHUB_REF=refs/tags/v0.1.0 GITHUB_SHA=$s STUB_INFO="true $s"
run "dry run ok" TAG=v0.2.0-dryrun.3 GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main GITHUB_REF=refs/heads/main GITHUB_SHA=$s STUB_INFO="true $s"
run "main moved" TAG=v0.2.0-dryrun.3 GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main GITHUB_REF=refs/heads/main GITHUB_SHA=$s STUB_INFO="true 2222222222222222222222222222222222222222"
run "not a draft" TAG=v0.2.0 GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v0.2.0 GITHUB_REF=refs/tags/v0.2.0 GITHUB_SHA=$s STUB_INFO="false $s"
run "bad tag" TAG=0.2.0 GITHUB_REF_TYPE=tag GITHUB_REF_NAME=0.2.0 GITHUB_REF=refs/tags/0.2.0 GITHUB_SHA=$s STUB_INFO="true $s"
rm -rf "$stub"
```

Run: `bash /c/tmp/guard_test.sh`
Expected: exit 0 for "release on its tag" and "dry run ok"; exit 1 with the matching message for the other five.

- [ ] **Step 5: Commit**

```bash
git add ci/release_guard.sh .github/workflows/release-msi.yml .github/workflows/release-finish.yml
git commit -m "Release steps 3 and 5: check the signed files, build and test the MSIs, then SHA256SUMS and attestations; a guard ties each run to its draft"
```

---

### Task 8: Docs

**Files:**
- Create: `docs/releasing.md`
- Modify: `README.md` (the "Installing" paragraph near line 250)
- Modify: `docs/windows-install.md` (lines 6–8)
- Modify: `docs/superpowers/specs/2026-10-10-m6b-packaging-design.md` (D3 row, §7.2 heading)

- [ ] **Step 1: Write `docs/releasing.md`**

````markdown
# Releasing rpyenv

A release needs the maintainer's PC with the Certum card. CI builds, tests and attests;
the PC only signs. The whole run takes about 25–30 minutes (an estimate until the first
release measures it), most of it CI.

## Steps

1. **Bump the version** in `Cargo.toml` (`version = "X.Y.Z"`) on `main`, then push the tag:
   `git tag vX.Y.Z && git push origin vX.Y.Z`. `release.yml` builds the executables and
   tarballs, attests them and creates a draft release.
2. **Insert the card**, then run `python ci/release_sign.py vX.Y.Z` from the repo root. It:
   1. signs the six executables (the Certum dialog asks for the PIN);
   2. starts `release-msi.yml` and waits for it;
   3. signs the two MSIs (PIN again);
   4. starts `release-finish.yml` and waits for it;
   5. prints the draft's link.
3. **Review the draft** (6 assets: 2 MSIs, 2 tarballs, `install.sh`, `SHA256SUMS`) and
   publish it.

`python ci/release_sign.py vX.Y.Z --check` prints where a draft is, without changing
anything. Running the script again continues from the draft's state.

## Dry run

Run `release.yml` by hand on `main` (Actions → Release → Run workflow). It creates the draft
pre-release `vX.Y.Z-dryrun.N`. Then run `python ci/release_sign.py vX.Y.Z-dryrun.N`.
Delete the draft afterwards. Don't push to `main` until the dry run is finished: steps 3 and 5
refuse to run if `main` has moved since step 1.

## When something fails

| What fails | What happens | What to do |
|---|---|---|
| Card removed, wrong PIN, signtool error | Nothing is uploaded; the temporary folder is removed | Run the script again |
| First timestamp server down | The script tries the second one (the PIN dialog may show again) | Nothing, unless both are down: try later |
| A check in `release-msi.yml` or `release-finish.yml` fails (attestation, signer, timestamp, "differs from the unsigned file") | That run uploads nothing | Read the run log. Don't retry until you know why. |
| A build or test in step 3 or 5 fails | The draft does not change | Fix the cause, then `python ci/release_sign.py <tag> --retry` |
| `main` moved during a dry run | Step 3 or 5 stops with that message | Start a new dry run |
| You want to drop a release | — | Delete the draft by hand. The script never deletes drafts or tags. |

## What is pinned

`installer/signing.json` holds the certificate's thumbprint, the timestamp servers and the
names of the files that may be signed. A new certificate is a one-line change there.
The design is `docs/superpowers/specs/2026-10-22-release-signing-design.md`.
````

- [ ] **Step 2: Update `README.md`**

Replace:

```
Releases: https://github.com/jmperez0/rpyenv/releases. v0.1.0 is **unsigned**, so Windows
SmartScreen warns before running the MSI ("More info", then "Run anyway").
```

with:

```
Releases: https://github.com/jmperez0/rpyenv/releases. From v0.2.0 the MSIs and the programs
in them are signed with a Certum code-signing certificate. SmartScreen may still warn while
the certificate is new ("More info", then "Run anyway"); v0.1.0 is unsigned and always warns.
```

- [ ] **Step 3: Update `docs/windows-install.md`**

Replace:

```
v0.1.0 is **unsigned**. Windows SmartScreen warns before running the MSI: choose
"More info", then "Run anyway". On a PC with Smart App Control turned on, Windows blocks
unsigned programs, rpyenv's shims included.
```

with:

```
From v0.2.0 the MSIs and the programs in them are signed with a Certum code-signing
certificate. SmartScreen may still warn while the certificate has no download reputation:
choose "More info", then "Run anyway". v0.1.0 is **unsigned**: SmartScreen always warns, and
on a PC with Smart App Control turned on, Windows blocks its programs, rpyenv's shims included.
```

- [ ] **Step 4: Point the M6b spec to this one**

In `docs/superpowers/specs/2026-10-10-m6b-packaging-design.md`, append this to the D3 row's text: ` Superseded by docs/superpowers/specs/2026-10-22-release-signing-design.md (Phase 3).` Then add this line under the `### 7.2 \`release.yml\`` heading:

```
> Since Phase 3, `release.yml` is only step 1 of a release; see
> `docs/superpowers/specs/2026-10-22-release-signing-design.md` §4–§5.
```

- [ ] **Step 5: Commit**

```bash
git add docs/releasing.md README.md docs/windows-install.md docs/superpowers/specs/2026-10-10-m6b-packaging-design.md
git commit -m "Docs: releasing.md (signing with the card, dry runs, failures); README and windows-install say v0.2.0 on is signed"
```

---

### Task 9: Full local check, then the PR (with the user's approval)

**Files:** none new.

- [ ] **Step 1: Run every ci test**

Run: `python -m unittest discover -s ci -p "test_*.py"`
Expected: all pass, including the existing ones (`test_install_sh` may skip parts on Windows, as it did before this branch).

Also run the same in WSL (Ubuntu-like): write `C:\tmp\ci_tests.sh` containing `cd /mnt/c/Users/JM/repos/OWN/rpyenv && python3 -m unittest discover -s ci -p "test_*.py"`. Then run `wsl -d Debian --exec /usr/bin/bash -l /mnt/c/tmp/ci_tests.sh` from PowerShell.
Expected: all pass, matching what the `coverage` job will see.

- [ ] **Step 2: Ask the user before pushing**

Ask: "Push `phase3-release-signing` and open a PR?" On yes:
- `git push -u origin phase3-release-signing`
- `gh pr create` with a body file `C:\tmp\pr_phase3_body.md`. The body says what changed and how it was tested, with no attribution lines.

- [ ] **Step 3: Read the PR's CI results**

Expected, all green:
- `Packaging / build / exes (x64|arm64)`
- `msi (x64|arm64)`
- `tarball (x64|arm64)`
- `signing check self-test`, whose log must show "as expected" for all five cases
- `Parity / coverage`

If self-test case 1 or 4 fails, the comparison method is wrong for what signtool writes (spec §8.1). Read the `FAIL` reason, fix `pe_difference` or `msi_difference` with a unit test that reproduces it, and push again.

---

### Task 10: The dry run with the card (after the user merges)

`workflow_dispatch` workflows can only be started once they are on the default branch, so this task runs after the merge. Every step that starts or changes something on GitHub, or uses the card, is the user's to approve or do.

- [ ] **Step 1: Ask the user to start the dry run**

Ask for approval, then run `gh workflow run release.yml --ref main -R jmperez0/rpyenv`. Wait for it to finish. Read the draft's name (`v0.1.0-dryrun.N` while `Cargo.toml` says 0.1.0) with `gh release list -R jmperez0/rpyenv --limit 3`.

- [ ] **Step 2: Check the state without changing anything**

Run: `python ci/release_sign.py v0.1.0-dryrun.N --check`
Expected: `v0.1.0-dryrun.N: sign-exes`.

- [ ] **Step 3: The user runs the script with the card**

The user inserts the card and types `! python ci/release_sign.py v0.1.0-dryrun.N` into the prompt. The PIN goes into the Certum dialog only. Measure and record:
- how many PIN dialogs each signtool call showed (the user reports this);
- the wall-clock time of the whole run;
- whether every step reached `done`.

- [ ] **Step 4: Check the result from outside**

1. `gh release download v0.1.0-dryrun.N -R jmperez0/rpyenv -D C:\tmp\dryrun_<N>` into a new, empty folder.
2. `sha256sum -c SHA256SUMS` in that folder.
3. `gh attestation verify <file> --repo jmperez0/rpyenv --signer-workflow jmperez0/rpyenv/.github/workflows/release-finish.yml` for each of the 6 files.
4. `signtool verify /pa /v` on both MSIs. Run it read-only on the host; signtool verify does not run the file.
5. Install the x64 MSI on the rpyenv-s6 VM from its `clean` checkpoint, never on the host. Then check that `pyenv.exe`'s Digital Signatures tab, or `Get-AuthenticodeSignature`, shows the Certum signature with a timestamp.

- [ ] **Step 5: Record the measurements and finish**

- Put the measured PIN-prompt count and run time into `docs/releasing.md`, replacing "an estimate until the first release measures it", and into spec §2.
- Commit on a new branch, then ask before pushing.
- Ask the user to delete the dry-run draft. The script never deletes drafts.
