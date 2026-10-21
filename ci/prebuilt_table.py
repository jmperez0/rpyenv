"""Builds the python-build-standalone table rpyenv embeds (M8 design §7, D15-D19).

  python3 ci/prebuilt_table.py --share <python-build share dir> [--write] [--workdir DIR]

Lists astral-sh/python-build-standalone's releases with `gh api`, keeps each built-in definition's
newest attested build (releases 20250311 and later) per arch, and keeps a row already in the table
unchanged. For a new or changed row it downloads the asset into --workdir, checks its SHA-256
against the release's (the asset's API digest, else the release's SHA256SUMS), runs
`gh attestation verify` with the repository and signing workflow pinned, reads the bundled OpenSSL
and Tk versions from the tarball's bytes (nothing is run) and deletes the file. Any failure exits 1
before anything is written. Without --write it reports whether the table would change.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEST = os.path.join(REPO, "crates", "pyenv", "prebuilt", "linux.tsv")
GH_REPO = "astral-sh/python-build-standalone"
BASE = f"https://github.com/{GH_REPO}/releases/download"
WORKFLOW = f"{GH_REPO}/.github/workflows/release.yml"
FIRST_ATTESTED = "20250311"
ARCHES = ("x86_64", "aarch64")
ASSET = re.compile(
    r"^cpython-(\d+\.\d+\.\d+(?:(?:a|b|rc)\d+)?)\+(\d{8})-(x86_64|aarch64)-unknown-linux-gnu-"
    r"(freethreaded-)?install_only\.tar\.gz$"
)
OPENSSL = re.compile(rb"OpenSSL (\d+\.\d+\.\d+[a-z]?) +\d{1,2} [A-Z][a-z]{2} \d{4}")
HEADER = (
    "# python-build-standalone builds rpyenv installs with --deps=prebuilt (M8 design §7).\n"
    "# Written by ci/prebuilt_table.py: every row's asset matched its release's SHA-256 and passed\n"
    f"# `gh attestation verify --repo {GH_REPO} --signer-workflow {WORKFLOW}`.\n"
    "# name\tarch\trelease\tsha256\topenssl\ttk\n"
)


class TableError(Exception):
    pass


def parse_asset(name):
    """(definition name, release, arch) for a Linux glibc install_only asset, else None."""
    m = ASSET.match(name)
    if not m:
        return None
    return (m.group(1) + ("t" if m.group(4) else ""), m.group(2), m.group(3))


def asset_name(name, release, arch):
    ft = name.endswith("t")
    ver = name[:-1] if ft else name
    return f"cpython-{ver}+{release}-{arch}-unknown-linux-gnu-{'freethreaded-' if ft else ''}install_only.tar.gz"


def asset_url(name, release, arch):
    return f"{BASE}/{release}/{asset_name(name, release, arch).replace('+', '%2B')}"


def choose(releases, defs):
    """{(name, arch): newest release} over [(tag, [asset names])] for names in `defs`."""
    out = {}
    for tag, assets in releases:
        if tag < FIRST_ATTESTED:
            continue
        for a in assets:
            p = parse_asset(a)
            if not p or p[1] != tag or p[0] not in defs:
                continue
            key = (p[0], p[2])
            if key not in out or out[key] < tag:
                out[key] = tag
    return out


def version_key(name):
    ft = name.endswith("t")
    core = name[:-1] if ft else name
    m = re.match(r"^(\d+)\.(\d+)\.(\d+)(?:(a|b|rc)(\d+))?$", core)
    if not m:
        raise TableError(f"unreadable version {name}")
    pre = {"a": 0, "b": 1, "rc": 2, None: 3}[m.group(4)]
    return (int(m.group(1)), int(m.group(2)), int(m.group(3)), pre, int(m.group(5) or 0), ft)


def render(rows):
    lines = [HEADER]
    for (name, arch) in sorted(rows, key=lambda k: (version_key(k[0]), k[1])):
        release, sha, openssl, tk = rows[(name, arch)]
        lines.append(f"{name}\t{arch}\t{release}\t{sha}\t{openssl}\t{tk}\n")
    return "".join(lines)


def parse_table(text):
    rows = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        f = line.split("\t")
        if len(f) != 6:
            raise TableError(f"unreadable table line: {line}")
        rows[(f[0], f[1])] = (f[2], f[3], f[4], f[5])
    return rows


def inspect(path):
    """(OpenSSL version, Tk version) from the tarball's libpython and its Tcl folder."""
    openssl, tk = None, None
    with tarfile.open(path, "r:gz") as t:
        for m in t:
            n = m.name
            if re.match(r"^python/lib/tcl(\d+\.\d+)/", n):
                tk = re.match(r"^python/lib/tcl(\d+\.\d+)/", n).group(1)
            if m.isfile() and re.match(r"^python/lib/libpython3\.\d+t?\.so", n) and openssl is None:
                found = OPENSSL.search(t.extractfile(m).read())
                if found:
                    openssl = found.group(1).decode("ascii")
    if not openssl or tk not in ("8.6", "9.0"):
        raise TableError(f"{path}: OpenSSL {openssl!r}, Tk {tk!r}")
    return openssl, tk


def gh_json_lines(args):
    out = subprocess.run(["gh", "api", *args], capture_output=True, text=True, encoding="utf-8")
    if out.returncode:
        raise TableError(f"gh api {' '.join(args)}: {out.stderr.strip()}")
    return [json.loads(l) for l in out.stdout.splitlines() if l.strip()]


def list_releases():
    """[(tag, [asset names], {asset: digest})], newest first. Pages of 10: a page of 100 times out."""
    rels = gh_json_lines([
        "--paginate", f"repos/{GH_REPO}/releases?per_page=10",
        "--jq", ".[] | select(.draft | not) | {tag: .tag_name, assets: [.assets[] | {n: .name, d: .digest}]}",
    ])
    return [(r["tag"], [a["n"] for a in r["assets"]], {a["n"]: a["d"] for a in r["assets"] if a["d"]}) for r in rels]


def published_sha(tag, asset, digests, sums_cache):
    d = digests.get(asset)
    if d:
        if not d.startswith("sha256:"):
            raise TableError(f"{asset}: digest {d}")
        return d[len("sha256:"):]
    if tag not in sums_cache:
        with urllib.request.urlopen(f"{BASE}/{tag}/SHA256SUMS", timeout=120) as r:
            text = r.read().decode("utf-8")
        sums_cache[tag] = {l.split()[1].lstrip("*"): l.split()[0] for l in text.splitlines() if l.strip()}
    sha = sums_cache[tag].get(asset)
    if not sha:
        raise TableError(f"{tag}/SHA256SUMS has no {asset}")
    return sha


def download(url, path):
    h = hashlib.sha256()
    with urllib.request.urlopen(url, timeout=600) as r, open(path, "wb") as f:
        while True:
            b = r.read(1 << 20)
            if not b:
                break
            h.update(b)
            f.write(b)
    return h.hexdigest()


def attest(path):
    r = subprocess.run(
        ["gh", "attestation", "verify", path, "--repo", GH_REPO, "--signer-workflow", WORKFLOW],
        capture_output=True, text=True, encoding="utf-8")
    if r.returncode:
        raise TableError(f"{os.path.basename(path)}: attestation: {r.stderr.strip()[-300:]}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--share", required=True)
    ap.add_argument("--write", action="store_true")
    ap.add_argument("--workdir")
    a = ap.parse_args()
    defs = {n for n in os.listdir(a.share) if os.path.isfile(os.path.join(a.share, n))}
    old = {}
    if os.path.exists(DEST):
        with open(DEST, encoding="utf-8") as f:
            old = parse_table(f.read())
    try:
        releases = list_releases()
        chosen = choose([(t, names) for t, names, _ in releases], defs)
        digests = {t: d for t, _, d in releases}
        rows, sums_cache, fresh = {}, {}, 0
        work = a.workdir or tempfile.mkdtemp(prefix="prebuilt-table.")
        os.makedirs(work, exist_ok=True)
        for (name, arch), tag in sorted(chosen.items()):
            prev = old.get((name, arch))
            if prev and prev[0] == tag:
                rows[(name, arch)] = prev
                continue
            asset = asset_name(name, tag, arch)
            want = published_sha(tag, asset, digests[tag], sums_cache)
            path = os.path.join(work, asset)
            got = download(asset_url(name, tag, arch), path)
            try:
                if got != want:
                    raise TableError(f"{asset}: SHA-256 {got}, the release says {want}")
                attest(path)
                openssl, tk = inspect(path)
            finally:
                os.remove(path)
            rows[(name, arch)] = (tag, got, openssl, tk)
            fresh += 1
            print(f"new row: {name} {arch} {tag} OpenSSL {openssl} Tk {tk}", flush=True)
    except (TableError, OSError, ValueError) as e:
        print(f"prebuilt_table: {e}", file=sys.stderr)
        return 1
    text = render(rows)
    old_text = open(DEST, encoding="utf-8").read() if os.path.exists(DEST) else ""
    if text == old_text:
        print(f"prebuilt table: no change ({len(rows)} rows)")
        return 0
    if a.write:
        os.makedirs(os.path.dirname(DEST), exist_ok=True)
        with open(DEST, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
    print(f"prebuilt table: {len(rows)} rows, {fresh} new or changed{'' if a.write else ' (not written)'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
