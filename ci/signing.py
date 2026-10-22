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
