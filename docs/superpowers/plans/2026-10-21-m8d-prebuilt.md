# M8d: the prebuilt route (python-build-standalone) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `pyenv install --deps=prebuilt <v>`, and `[s]` at the prompt, install a python-build-standalone build of a built-in CPython definition on Linux instead of compiling it, with the hash checked against a table whose rows passed GitHub attestation in the weekly sync.

**Architecture:** A checked-in table `crates/pyenv/prebuilt/linux.tsv` (one row per definition name and arch: release, SHA-256, bundled OpenSSL and Tk versions) is written by `ci/prebuilt_table.py`. The script lists python-build-standalone's releases, keeps attested ones (20250311 and later) and, for each new row, downloads the asset, checks the SHA-256, runs `gh attestation verify` and reads the versions from the bytes. At install time, `install/prebuilt.rs` turns a row into a one-line definition (`install_package "<pkg>" "<url>#<sha>" prebuilt <verify step> ensurepip`), as M7a does for PyPy, and the builder's new `prebuilt` step copies the tree, rewrites its compiler settings (D18) and places it. §4.4's check then runs without `dbm.gnu` and `nis` (D17) and with Tcl in a venv. `versions/<v>/.rpyenv-deps` records the release, so a later rpyenv with a newer build says so.

**Tech Stack:** Rust (crates/pyenv), Python 3 for the CI table script (stdlib only, plus the `gh` CLI), GitHub Actions, podman/docker for the container check.

**Spec:** `docs/superpowers/specs/2026-10-17-m8-linux-build-deps-design.md` §7, D8, D15–D19 (and §4.4, §5, §9).

## Global Constraints

- Linux only; the prebuilt module is `#[cfg(unix)]`, as `deps`, `record` and `store` are. Windows `cargo clippy --workspace --all-targets -- -D warnings` must stay clean.
- The route is **never the default** (D8): only `--deps=prebuilt`, `RPYENV_BUILD_DEPS=prebuilt` or `[s]` at the prompt choose it.
- Only rows from releases **20250311 and later** (D16). Only built-in CPython definitions (`Origin::Builtin`) whose name has a row for the host arch (`x86_64` or `aarch64`).
- Release folder base URL, exactly: `https://github.com/astral-sh/python-build-standalone/releases/download`. Attestation identity, exactly: `--repo astral-sh/python-build-standalone --signer-workflow astral-sh/python-build-standalone/.github/workflows/release.yml`.
- Asset name, exactly: `cpython-<X.Y.Z[aN|bN|rcN]>+<release>-<arch>-unknown-linux-gnu-[freethreaded-]install_only.tar.gz`; in the URL, `+` is written `%2B`.
- §4.4 for prebuilt skips exactly the imports `dbm.gnu` and `nis` (D17); everything else in `stdlib::modules(minor)`, the CA check and Tcl (also in a venv) is required. Measured during planning (Ubuntu 24.04 and Fedora 44 containers, builds 3.9.25/20251031, 3.13.16 and 3.13.16t/20261020, 3.14.8/20261020): `dbm.ndbm`, `curses.panel` and `readline` import; `dbm.gnu` never does; 3.9.25's `crypt` fails on Fedora 44 for lack of `libcrypt.so.1` (package `libxcrypt-compat`); CA certificates are found at `/etc/ssl/cert.pem` or `/etc/ssl/certs` (Ubuntu needs `ca-certificates` installed).
- D18's rewrite: in `lib/python3.*/_sysconfigdata_*.py`, `bin/python3.*-config` and `lib/python3.*/config-3.*/Makefile`: the whole word `clang++` → `c++`, the whole word `clang` → `cc`, and the path `/install` → the version's folder. No file may keep `/install`; a build without a `_sysconfigdata_*.py` fails the install.
- The prebuilt route needs a version folder that `store::plain_path` accepts (no space, colon or shell character), because the rewritten settings hold that path unquoted in shell and make files.
- Messages name the build: `python-build-standalone <YYYY-MM-DD>` (the release tag as a date), "third-party", the bundled OpenSSL version, the Tk version, and "no dbm.gnu or nis". A Tk 9 build adds the Tk 9 line.
- Tests never reach the network: debug-only `RPYENV_TEST_PREBUILT_TABLE` (a file) and `RPYENV_TEST_PREBUILT_BASE` (a base URL) point at fixtures and the local test server; with `RPYENV_TEST_PYTHON_ORG` set and the base unset, the base is a closed local port, as `pypy::urls` does. Release builds ignore both.
- Every user-visible difference from upstream gets an allowlist row (D12) with a citing test, and `python parity/coverage.py` must stay fully covered.
- Python scripts: `encoding='utf-8'` on every `open()`; downloaded files are only read as data (tar members, bytes), never executed.
- Commit messages: one line, no attribution lines.

## Review Focus

1. **A PYENV_ROOT with a space:** `--deps=prebuilt` refuses before downloading, naming the path. A reasonable person expects a clear refusal, not a build whose `sysconfig` breaks the compiler line. Test in Task 4.
2. **A row whose tarball's layout changed** (no `_sysconfigdata_*.py`): the install fails with a message naming the missing file, and nothing is left in `versions/`. Test in Task 3.
3. **A Ctrl+C during the download:** exit 130, nothing left in `versions/`, no record. Test in Task 4.
4. **An alias** (`--deps=prebuilt 3.13.16:mine`): it installs under `versions/mine`, the record's `definition` is `3.13.16`, and the stale line's reinstall hint is `3.13.16:mine`. Tests in Tasks 4 and 5.
5. **A machine that is neither x86_64 nor aarch64:** no `[s]`, and `--deps=prebuilt` names the arch. Unit test in Task 2 (`host_arch` mapping) and the refusal text in Task 4.

---

## File Structure

| File | Responsibility |
|---|---|
| `ci/prebuilt_table.py` (new) | Lists releases, chooses rows, downloads, checks SHA-256 and attestation, reads OpenSSL/Tk versions, writes the table. |
| `ci/test_prebuilt_table.py` (new) | Offline unit tests of the script's pure parts and its tar inspection. |
| `crates/pyenv/prebuilt/linux.tsv` (new) | The table (generated). |
| `crates/pyenv/src/install/prebuilt.rs` (new) | Table parsing and lookup, URLs, the one-line definition, the summary text, D18's rewrite. |
| `crates/pyenv/src/install/mod.rs` | `#[cfg(unix)] pub mod prebuilt;` |
| `crates/pyenv/src/install/builder.rs` | The `prebuilt` step; §4.4 for prebuilt (skips, venv Tcl, wording). |
| `crates/pyenv/src/install/record.rs` | `prebuilt <release> <arch>` in the record; the stale line for prebuilt. |
| `crates/pyenv/src/install/deps.rs` | `[s]` in `prompt_lines`, `choice`, `answer_hint`. |
| `crates/pyenv/src/commands/install.rs` | Choosing the route, refusals, swapping in the one-line definition, messages, writing the record. |
| `crates/pyenv/tests/common/fakepbs.rs` (new), `crates/pyenv/tests/cli_install_prebuilt.rs` (new) | Fake python-build-standalone tarball; CLI tests. |
| `.github/workflows/sync-python-build.yml` | Rebuilds the table weekly. |
| `ci/check_prebuilt.sh` (new), `.github/workflows/install-linux.yml` | Real prebuilt install on x86_64 and aarch64 runners, and the must-fail run without CA certificates. |
| `docs/parity/allowlist.md`, `README.md`, `docs/distributions.md`, the spec | Documentation. |

---

### Task 1: The table script and the generated table

**Files:**
- Create: `ci/prebuilt_table.py`, `ci/test_prebuilt_table.py`, `crates/pyenv/prebuilt/linux.tsv`

**Interfaces:**
- Produces: the table format Task 2 parses. `#` comment lines, then tab-separated `name arch release sha256 openssl tk`, for example
  `3.13.16	x86_64	20261020	0ad293ecda8ea93a28e4a6b5e9cc20ae74be15aafed75be04418b8f82bd0d0da	3.5.9	9.0`.
  `name` is the definition name (`3.13.16`, `3.13.16t`, `3.15.0rc3`); `arch` is `x86_64` or `aarch64`; `release` is 8 digits; `openssl` matches `\d+\.\d+\.\d+[a-z]?`; `tk` is `8.6` or `9.0`. Rows are sorted by (version key, `t` last, arch).

- [ ] **Step 1: Write the failing tests** in `ci/test_prebuilt_table.py` (unittest, like `ci/test_pypy_tables.py`):

```python
"""Offline tests of ci/prebuilt_table.py (M8 design §7, D15, D16)."""
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


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run them to see them fail**

Run: `python ci/test_prebuilt_table.py`
Expected: `ModuleNotFoundError: No module named 'prebuilt_table'`.

- [ ] **Step 3: Write `ci/prebuilt_table.py`**

```python
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
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `python ci/test_prebuilt_table.py`
Expected: `OK` (7 tests).

- [ ] **Step 5: Generate the real table** (network; about 200 downloads of ~30 MB each, one at a time, each deleted after it is read; plan 20–40 minutes)

Run: `python ci/prebuilt_table.py --share crates/pyenv/python-build/share --write --workdir C:\tmp\prebuilt_work` (make that folder first; it must be empty)
Expected: one `new row:` line per row and a final `prebuilt table: N rows, N new or changed`. For the record, N was about 200 when this plan was written (202 attested rows over x86_64/aarch64 and both variants before filtering by definition). Then `crates/pyenv/prebuilt/linux.tsv` holds a `3.13.16 x86_64 20261020 0ad293ec…d0da 3.5.9 9.0` row (the hash measured during planning).

- [ ] **Step 6: Check it no longer changes**

Run the same command again without `--write`.
Expected: `prebuilt table: no change (N rows)` with no downloads.

- [ ] **Step 7: Commit**

```bash
git add ci/prebuilt_table.py ci/test_prebuilt_table.py crates/pyenv/prebuilt/linux.tsv
git commit -m "M8d: the prebuilt table, built from attested python-build-standalone releases (D15, D16)"
```

---

### Task 2: `install/prebuilt.rs`

**Files:**
- Create: `crates/pyenv/src/install/prebuilt.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `#[cfg(unix)] pub mod prebuilt;` in alphabetical place, after `preflight`)

**Interfaces:**
- Consumes: Task 1's table format.
- Produces (Tasks 3–5 rely on these exact names):
  - `pub struct Row { pub name: String, pub arch: String, pub release: String, pub sha256: String, pub openssl: String, pub tk: String }`
  - `pub fn parse(text: &str) -> Result<Vec<Row>, String>`
  - `pub fn table() -> Result<Vec<Row>, String>`
  - `pub fn host_arch() -> Option<&'static str>`
  - `pub fn find<'a>(rows: &'a [Row], name: &str, arch: &str) -> Option<&'a Row>`
  - `pub fn base() -> String`, `pub fn url(row: &Row) -> String`, `pub fn package(row: &Row) -> String`
  - `pub fn definition_text(row: &Row, verify_step: &str) -> String`
  - `pub fn date(release: &str) -> String` (`20261020` → `2026-10-20`)
  - `pub fn summary(row: &Row) -> String`
  - `pub fn tk9_line(row: &Row) -> Option<&'static str>`
  - `pub const SKIPPED_IMPORTS: [&str; 2] = ["dbm.gnu", "nis"];`
  - `pub fn rewrite_text(text: &str, prefix: &str) -> String`
  - `pub fn rewrite_build_settings(tree: &Path, prefix: &Path) -> Result<usize, String>`

- [ ] **Step 1: Write the failing tests** at the end of the new file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0ad293ecda8ea93a28e4a6b5e9cc20ae74be15aafed75be04418b8f82bd0d0da";

    fn row(name: &str, arch: &str, tk: &str) -> Row {
        Row {
            name: name.into(),
            arch: arch.into(),
            release: "20261020".into(),
            sha256: SHA.into(),
            openssl: "3.5.9".into(),
            tk: tk.into(),
        }
    }

    #[test]
    fn the_table_parses_and_rejects_bad_rows() {
        let good = format!("# c\n3.13.16\tx86_64\t20261020\t{SHA}\t3.5.9\t9.0\n");
        assert_eq!(parse(&good).unwrap(), vec![row("3.13.16", "x86_64", "9.0")]);
        for bad in [
            format!("3.13.16\tx86_64\t20261020\t{SHA}\t3.5.9\n"),
            format!("3.13.16\tarm\t20261020\t{SHA}\t3.5.9\t9.0\n"),
            format!("3.13.16\tx86_64\t2026102\t{SHA}\t3.5.9\t9.0\n"),
            format!("3.13.16\tx86_64\t20261020\t{}\t3.5.9\t9.0\n", &SHA[1..]),
            format!("3.13.16\tx86_64\t20250205\t{SHA}\t3.5.9\t9.0\n"),
            format!("../x\tx86_64\t20261020\t{SHA}\t3.5.9\t9.0\n"),
            format!("3.13.16\tx86_64\t20261020\t{SHA}\t3.5.9\t9\n"),
        ] {
            assert!(parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_embedded_table_parses_and_has_rows_for_both_arches() {
        let rows = parse(EMBEDDED).unwrap();
        assert!(rows.iter().any(|r| r.arch == "x86_64"));
        assert!(rows.iter().any(|r| r.arch == "aarch64"));
    }

    #[test]
    fn urls_packages_and_definitions() {
        let r = row("3.13.16t", "aarch64", "9.0");
        assert_eq!(
            url(&r),
            "https://github.com/astral-sh/python-build-standalone/releases/download/20261020/cpython-3.13.16%2B20261020-aarch64-unknown-linux-gnu-freethreaded-install_only.tar.gz"
        );
        assert_eq!(package(&r), "cpython-3.13.16t-20261020-aarch64");
        assert!(crate::install::is_plain_name(&package(&r)));
        assert_eq!(
            definition_text(&r, "verify_py313"),
            format!("install_package \"cpython-3.13.16t-20261020-aarch64\" \"{}#{SHA}\" prebuilt verify_py313 ensurepip\n", url(&r))
        );
    }

    #[test]
    fn find_matches_name_and_arch() {
        let rows = vec![row("3.13.16", "x86_64", "9.0"), row("3.13.16", "aarch64", "9.0")];
        assert_eq!(find(&rows, "3.13.16", "aarch64").unwrap().arch, "aarch64");
        assert!(find(&rows, "3.13.15", "x86_64").is_none());
        assert!(find(&rows, "3.13.16t", "x86_64").is_none());
    }

    #[test]
    fn host_arch_maps_only_the_two_built_arches() {
        assert_eq!(arch_of("x86_64"), Some("x86_64"));
        assert_eq!(arch_of("aarch64"), Some("aarch64"));
        assert_eq!(arch_of("riscv64"), None);
        assert_eq!(arch_of("x86"), None);
    }

    #[test]
    fn the_summary_names_date_openssl_tk_and_what_is_missing() {
        let r = row("3.13.16", "x86_64", "9.0");
        assert_eq!(
            summary(&r),
            "python-build-standalone 2026-10-20 (third-party build; OpenSSL 3.5.9, Tk 9.0; no dbm.gnu or nis)"
        );
        assert!(tk9_line(&r).is_some());
        assert!(tk9_line(&row("3.9.25", "x86_64", "8.6")).is_none());
    }

    #[test]
    fn rewrite_replaces_clang_and_the_install_prefix_only_as_whole_words() {
        let t = "build_time_vars = {'CC': 'clang -pthread',\n 'CXX': 'clang++ -pthread',\n 'LDSHARED': 'clang -pthread -shared -L/install/lib',\n 'INCLUDEPY': '/install/include/python3.13',\n 'prefix': '/install',\n 'OTHER': '/usr/install/x clangd -DX=clang_x /installer',\n}\n";
        let out = rewrite_text(t, "/r/versions/3.13.16");
        assert!(out.contains("'CC': 'cc -pthread'"), "{out}");
        assert!(out.contains("'CXX': 'c++ -pthread'"), "{out}");
        assert!(out.contains("-L/r/versions/3.13.16/lib"), "{out}");
        assert!(out.contains("'INCLUDEPY': '/r/versions/3.13.16/include/python3.13'"), "{out}");
        assert!(out.contains("'prefix': '/r/versions/3.13.16'"), "{out}");
        assert!(out.contains("'/usr/install/x clangd -DX=clang_x /installer'"), "{out}");
        let mk = "CC=\t\tclang -pthread\nprefix=\t\t/install\nLIBPL=$(prefix)/lib\n";
        let out = rewrite_text(mk, "/r/v");
        assert_eq!(out, "CC=\t\tcc -pthread\nprefix=\t\t/r/v\nLIBPL=$(prefix)/lib\n");
    }

    #[test]
    fn rewrite_build_settings_edits_the_three_files_and_needs_sysconfigdata() {
        let d = tempfile::tempdir().unwrap();
        let t = d.path();
        std::fs::create_dir_all(t.join("lib/python3.13t/config-3.13t-x86_64-linux-gnu")).unwrap();
        std::fs::create_dir_all(t.join("bin")).unwrap();
        std::fs::write(t.join("lib/python3.13t/_sysconfigdata_t_linux_x86_64-linux-gnu.py"), "{'CC': 'clang', 'p': '/install'}\n").unwrap();
        std::fs::write(t.join("lib/python3.13t/config-3.13t-x86_64-linux-gnu/Makefile"), "CC= clang\nprefix= /install\n").unwrap();
        std::fs::write(t.join("bin/python3.13t-config"), "prefix_build=\"/install\"\n").unwrap();
        std::fs::write(t.join("bin/python3.13t"), "ELF").unwrap();
        assert_eq!(rewrite_build_settings(t, Path::new("/r/v")).unwrap(), 3);
        for f in [
            "lib/python3.13t/_sysconfigdata_t_linux_x86_64-linux-gnu.py",
            "lib/python3.13t/config-3.13t-x86_64-linux-gnu/Makefile",
            "bin/python3.13t-config",
        ] {
            let s = std::fs::read_to_string(t.join(f)).unwrap();
            assert!(!s.contains("/install") && !s.contains("clang"), "{f}: {s}");
        }
        let e = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(e.path().join("lib/python3.13")).unwrap();
        let err = rewrite_build_settings(e.path(), Path::new("/r/v")).unwrap_err();
        assert!(err.contains("_sysconfigdata"), "{err}");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run (WSL, scratch env as always): `cargo test -q -p pyenv --lib prebuilt`
Expected: compile error, module `prebuilt` not found (after adding the `mod` line: unresolved names).

- [ ] **Step 3: Write the module**

```rust
//! The prebuilt route (M8 design §7, D15–D19): the table of python-build-standalone builds that
//! ci/prebuilt_table.py writes (`crates/pyenv/prebuilt/linux.tsv`), the one-line definition a row
//! becomes, the text that names a build, and the rewrite of its compiler settings (D18).

use std::path::Path;

static EMBEDDED: &str = include_str!("../../prebuilt/linux.tsv");
pub const BASE: &str = "https://github.com/astral-sh/python-build-standalone/releases/download";
/// Releases before this one have no GitHub attestation (D16); the table never holds them.
const FIRST_ATTESTED: &str = "20250311";
/// §4.4's imports a prebuilt build never has (D17): gdbm is GPL, nis is not built.
pub const SKIPPED_IMPORTS: [&str; 2] = ["dbm.gnu", "nis"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub arch: String,
    pub release: String,
    pub sha256: String,
    pub openssl: String,
    pub tk: String,
}

fn is_version_name(s: &str) -> bool {
    let core = s.strip_suffix('t').unwrap_or(s);
    let mut parts = core.splitn(3, '.');
    let (Some(a), Some(b), Some(c)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let patch_end = c.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(c.len());
    let (num, pre) = c.split_at(patch_end);
    let pre_ok = pre.is_empty()
        || ["a", "b", "rc"]
            .iter()
            .any(|p| pre.strip_prefix(p).is_some_and(digits));
    digits(a) && digits(b) && digits(num) && pre_ok
}

pub fn parse(text: &str) -> Result<Vec<Row>, String> {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let bad = || format!("unreadable prebuilt table line: {l}");
            let [name, arch, release, sha, openssl, tk] = f.as_slice() else {
                return Err(bad());
            };
            let ok = is_version_name(name)
                && matches!(*arch, "x86_64" | "aarch64")
                && release.len() == 8
                && release.bytes().all(|b| b.is_ascii_digit())
                && *release >= FIRST_ATTESTED
                && sha.len() == 64
                && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                && !openssl.is_empty()
                && openssl.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
                && matches!(*tk, "8.6" | "9.0");
            if !ok {
                return Err(bad());
            }
            Ok(Row {
                name: name.to_string(),
                arch: arch.to_string(),
                release: release.to_string(),
                sha256: sha.to_string(),
                openssl: openssl.to_string(),
                tk: tk.to_string(),
            })
        })
        .collect()
}

/// Debug builds read `RPYENV_TEST_PREBUILT_TABLE` (a file) when it is set; release builds always
/// use the embedded table.
pub fn table() -> Result<Vec<Row>, String> {
    #[cfg(debug_assertions)]
    if let Some(p) = std::env::var_os("RPYENV_TEST_PREBUILT_TABLE").filter(|v| !v.is_empty()) {
        let text = std::fs::read_to_string(&p)
            .map_err(|e| format!("{}: {e}", Path::new(&p).display()))?;
        return parse(&text);
    }
    parse(EMBEDDED)
}

fn arch_of(rust_arch: &str) -> Option<&'static str> {
    match rust_arch {
        "x86_64" => Some("x86_64"),
        "aarch64" => Some("aarch64"),
        _ => None,
    }
}

/// The table's arch for the machine rpyenv was built for.
pub fn host_arch() -> Option<&'static str> {
    arch_of(std::env::consts::ARCH)
}

pub fn find<'a>(rows: &'a [Row], name: &str, arch: &str) -> Option<&'a Row> {
    rows.iter().find(|r| r.name == name && r.arch == arch)
}

/// The release folder base. Debug builds honor `RPYENV_TEST_PREBUILT_BASE`; with
/// `RPYENV_TEST_PYTHON_ORG` set and it unset, a closed local port, so a test never reaches the
/// network (as `pypy::urls`). Release builds ignore both.
pub fn base() -> String {
    #[cfg(debug_assertions)]
    {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        if let Some(b) = var("RPYENV_TEST_PREBUILT_BASE") {
            return b.trim_end_matches('/').to_string();
        }
        if var("RPYENV_TEST_PYTHON_ORG").is_some() {
            return "http://127.0.0.1:9/pbs".to_string();
        }
    }
    BASE.to_string()
}

fn asset(row: &Row) -> String {
    let (ver, ft) = match row.name.strip_suffix('t') {
        Some(v) => (v, "freethreaded-"),
        None => (row.name.as_str(), ""),
    };
    format!(
        "cpython-{ver}%2B{}-{}-unknown-linux-gnu-{ft}install_only.tar.gz",
        row.release, row.arch
    )
}

pub fn url(row: &Row) -> String {
    format!("{}/{}/{}", base(), row.release, asset(row))
}

/// The package name: the download's file name in the cache and the extracted folder's name.
pub fn package(row: &Row) -> String {
    format!("cpython-{}-{}-{}", row.name, row.release, row.arch)
}

/// The one-line definition the interpreter runs; `verify_step` is the built-in definition's own.
pub fn definition_text(row: &Row, verify_step: &str) -> String {
    format!(
        "install_package \"{}\" \"{}#{}\" prebuilt {verify_step} ensurepip\n",
        package(row),
        url(row),
        row.sha256
    )
}

pub fn date(release: &str) -> String {
    if release.len() == 8 {
        format!("{}-{}-{}", &release[..4], &release[4..6], &release[6..])
    } else {
        release.to_string()
    }
}

pub fn summary(row: &Row) -> String {
    format!(
        "python-build-standalone {} (third-party build; OpenSSL {}, Tk {}; no dbm.gnu or nis)",
        date(&row.release),
        row.openssl,
        row.tk
    )
}

pub fn tk9_line(row: &Row) -> Option<&'static str> {
    row.tk.starts_with('9').then_some(
        "pyenv: this build has Tk 9, under which some older Tk programs behave differently",
    )
}

/// `/install` at the start of a path: after a quote, a blank, `=`, `:`, or `-I`/`-L`, and
/// followed by `/`, a quote, a blank or the end.
fn install_at(b: &[u8], i: usize) -> bool {
    const P: &[u8] = b"/install";
    if !b[i..].starts_with(P) {
        return false;
    }
    let before_ok = i == 0
        || matches!(b[i - 1], b'\'' | b'"' | b' ' | b'\t' | b'\n' | b'=' | b':')
        || (i >= 2 && b[i - 2] == b'-' && matches!(b[i - 1], b'I' | b'L'));
    let after = b.get(i + P.len()).copied();
    let after_ok = matches!(
        after,
        None | Some(b'/' | b'\'' | b'"' | b' ' | b'\t' | b'\n' | b':')
    );
    before_ok && after_ok
}

/// The whole word `w` at `i`: not preceded or followed by a letter, digit, `_`, `-`, `.` or `/`.
fn word_at(b: &[u8], i: usize, w: &[u8]) -> bool {
    let part = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b'/' | b'+');
    b[i..].starts_with(w)
        && (i == 0 || !part(b[i - 1]))
        && b.get(i + w.len()).is_none_or(|c| !part(*c))
}

/// D18: `clang++` → `c++`, `clang` → `cc` (whole words) and the path `/install` → `prefix`.
pub fn rewrite_text(text: &str, prefix: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut start = 0;
    while i < b.len() {
        let (len, with): (usize, &str) = if install_at(b, i) {
            (8, prefix)
        } else if word_at(b, i, b"clang++") {
            (7, "c++")
        } else if word_at(b, i, b"clang") {
            (5, "cc")
        } else {
            i += 1;
            continue;
        };
        out.push_str(&text[start..i]);
        out.push_str(with);
        i += len;
        start = i;
    }
    out.push_str(&text[start..]);
    out
}

/// D18 over `tree` (the build before it is placed at `prefix`): every
/// `lib/python3.*/_sysconfigdata_*.py`, `lib/python3.*/config-3.*/Makefile` and
/// `bin/python3.*-config`. Returns how many files changed; a tree without a sysconfig data file
/// is an error (the table promises python-build-standalone's layout).
pub fn rewrite_build_settings(tree: &Path, prefix: &Path) -> Result<usize, String> {
    let prefix = prefix.to_string_lossy();
    let mut files = Vec::new();
    let mut sysconfig = 0;
    let lib = tree.join("lib");
    for e in std::fs::read_dir(&lib).map_err(|e| format!("{}: {e}", lib.display()))? {
        let d = e.map_err(|e| e.to_string())?.path();
        let n = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !n.starts_with("python3.") || !d.is_dir() {
            continue;
        }
        for f in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))? {
            let p = f.map_err(|e| e.to_string())?.path();
            let fname = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if fname.starts_with("_sysconfigdata_") && fname.ends_with(".py") {
                sysconfig += 1;
                files.push(p);
            } else if fname.starts_with("config-3.") && p.is_dir() && p.join("Makefile").is_file() {
                files.push(p.join("Makefile"));
            }
        }
    }
    if sysconfig == 0 {
        return Err(format!(
            "no _sysconfigdata_*.py under {}/python3.*: not a python-build-standalone layout",
            lib.display()
        ));
    }
    if let Ok(rd) = std::fs::read_dir(tree.join("bin")) {
        for e in rd.filter_map(Result::ok) {
            let n = e.file_name().to_string_lossy().into_owned();
            let is_file = e.file_type().is_ok_and(|t| t.is_file());
            if is_file && n.starts_with("python3.") && n.ends_with("-config") {
                files.push(e.path());
            }
        }
    }
    let mut changed = 0;
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let new = rewrite_text(&text, &prefix);
        if new != text {
            std::fs::write(&f, new).map_err(|e| format!("{}: {e}", f.display()))?;
            changed += 1;
        }
    }
    Ok(changed)
}
```

Note: `rewrite_build_settings`'s test expects 3, so the fixture's three files must each change; the `_sysconfigdata_` count check is separate from the change count.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -q -p pyenv --lib prebuilt`
Expected: 8 passed.

- [ ] **Step 5: fmt, clippy (WSL and Windows), commit**

```bash
cargo fmt --all
cargo clippy -q --workspace --all-targets -- -D warnings
git add crates/pyenv/src/install/prebuilt.rs crates/pyenv/src/install/mod.rs
git commit -m "M8d: install/prebuilt.rs reads the table, names the build and rewrites its compiler settings (D18)"
```

---

### Task 3: The builder's `prebuilt` step and §4.4 for prebuilt builds

**Files:**
- Modify: `crates/pyenv/src/install/builder.rs` (`run_step`, `verify`, `StoreTcl`, `stdlib_failures`, `stdlib_check`, and their unit tests)
- Create: `crates/pyenv/tests/common/fakepbs.rs` (used here and in Task 4)
- Create: `crates/pyenv/tests/cli_install_prebuilt.rs` (this task's builder-level CLI tests; Task 4 adds more)
- Modify: `crates/pyenv/tests/common/mod.rs` (`pub mod fakepbs;`)

**Interfaces:**
- Consumes: `prebuilt::rewrite_build_settings`, `prebuilt::SKIPPED_IMPORTS`.
- Produces: the builder runs a definition whose package steps are `prebuilt <verify step> ensurepip`. `StoreTcl.cache` becomes `Option<&Path>` (`None`: only the venv check). `stdlib_failures(run, minor, store_tcl, skip: &[&str])`.

- [ ] **Step 1: The fake tarball.** `crates/pyenv/tests/common/fakepbs.rs` builds `<top>.tar.gz` bytes for a python-build-standalone-like tree under `python/`. Model it on `common/fakepypy.rs` (same `file`/`link` helpers) and reuse fakebuild's interpreter script if `common/fakebuild.rs` exposes one; otherwise write this stub:

```rust
//! A fake python-build-standalone `install_only` tarball for tier-1 tests (M8d): top folder
//! `python/`, `bin/python3.13` answers `-c` (exit 0, or 1 for `import <missing>`), `-m venv
//! [--without-pip] <dir>` (copies itself to `<dir>/bin/python`) and `-I -m ensurepip` (exit 0),
//! links `bin/python` and `bin/python3`, and the three files D18 rewrites, with `clang` and
//! `/install`. Unix only.

use std::io::Write;

pub fn python_script(missing: &[&str]) -> String {
    let mut fail = String::new();
    for m in missing {
        fail.push_str(&format!("[ \"$2\" = \"import {m}\" ] && exit 1; "));
    }
    format!(
        "#!/bin/sh\ncase \"$1\" in\n  -c) {fail}exit 0;;\n  -m) if [ \"$2\" = venv ]; then d=\"$3\"; [ \"$d\" = --without-pip ] && d=\"$4\"; mkdir -p \"$d/bin\" && cp \"$0\" \"$d/bin/python\"; exit 0; fi;;\nesac\nexit 0\n"
    )
}

/// `with_sysconfig` false leaves out `_sysconfigdata_*.py` (a changed layout).
pub fn tarball_gz(missing: &[&str], with_sysconfig: bool) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    let mut file = |path: &str, body: &[u8], mode: u32| {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Regular);
        h.set_size(body.len() as u64);
        h.set_mode(mode);
        b.append_data(&mut h, format!("python/{path}"), body).unwrap();
    };
    file("bin/python3.13", python_script(missing).as_bytes(), 0o755);
    file("bin/python3.13-config", b"#!/bin/sh\nprefix_build=\"/install\"\n", 0o755);
    file("lib/python3.13/config-3.13-x86_64-linux-gnu/Makefile", b"CC=\t\tclang -pthread\nprefix=\t\t/install\n", 0o644);
    if with_sysconfig {
        file(
            "lib/python3.13/_sysconfigdata__linux_x86_64-linux-gnu.py",
            b"build_time_vars = {'CC': 'clang -pthread', 'CXX': 'clang++ -pthread', 'prefix': '/install', 'INCLUDEPY': '/install/include/python3.13'}\n",
            0o644,
        );
    }
    file("lib/python3.13/os.py", b"# os\n", 0o644);
    let mut link = |path: &str, target: &str| {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_size(0);
        h.set_mode(0o777);
        b.append_link(&mut h, format!("python/{path}"), target).unwrap();
    };
    link("bin/python", "python3.13");
    link("bin/python3", "python3.13");
    let tar_bytes = b.into_inner().unwrap();
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(&tar_bytes).unwrap();
    enc.finish().unwrap()
}
```

(The two closures both borrow `b` mutably; write them as two small `fn`s taking `&mut tar::Builder<Vec<u8>>`, as fakepypy does, if the borrow checker objects.)

- [ ] **Step 2: Write the failing CLI tests** in `crates/pyenv/tests/cli_install_prebuilt.rs`. These run a one-line definition file directly (like `cli_install_pypy.rs`'s `def_file`), so they test the builder alone; Task 4 adds the route. Use `cli_install_pypy.rs`'s `env`, `run` and `left_in_versions` helpers (copy them).

```rust
//! `pyenv install` of a python-build-standalone build on Linux (M8 design §7) against a fake
//! tarball on a local server.
#![cfg(unix)]

mod common;

use common::fakepbs::tarball_gz;
use common::fakepypy::sha256;
use common::server::{start, Reply, Server};
use common::Fixture;

const PKG: &str = "cpython-3.13.99-20261020-x86_64";

// env, run, left_in_versions: as in cli_install_pypy.rs.

fn serve(body: Vec<u8>) -> (Server, String) {
    let sha = sha256(&body);
    (start(vec![("/pbs.tar.gz", vec![Reply::Body(body)])]), sha)
}

/// `<base>/defs/3.13.99`: the one-line definition the route makes.
fn def_file(f: &Fixture, s: &Server, sha: &str) -> String {
    let p = f.base.join("defs").join("3.13.99");
    f.file(
        &p,
        &format!(
            "install_package \"{PKG}\" \"{}#{sha}\" prebuilt verify_py313 ensurepip\n",
            s.url("/pbs.tar.gz")
        ),
    );
    p.display().to_string()
}

#[test]
fn the_prebuilt_step_places_the_tree_and_rewrites_its_compiler_settings() {
    let f = Fixture::new();
    let (s, sha) = serve(tarball_gz(&[], true));
    let def = def_file(&f, &s, &sha);
    let r = run(&f, &["install", &def], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = f.root.join("versions/3.13.99");
    assert!(v.join("bin/python3.13").is_file());
    let sc = std::fs::read_to_string(v.join("lib/python3.13/_sysconfigdata__linux_x86_64-linux-gnu.py")).unwrap();
    assert!(sc.contains("'CC': 'cc -pthread'") && sc.contains("'CXX': 'c++ -pthread'"), "{sc}");
    assert!(sc.contains(&format!("'prefix': '{}'", v.display())), "{sc}");
    for p in ["lib/python3.13/config-3.13-x86_64-linux-gnu/Makefile", "bin/python3.13-config"] {
        let t = std::fs::read_to_string(v.join(p)).unwrap();
        assert!(!t.contains("/install") && !t.contains("clang"), "{p}: {t}");
    }
    assert!(!v.join(".rpyenv-incomplete").exists());
}

/// D17: a prebuilt build has no dbm.gnu or nis; §4.4 doesn't ask for them.
#[test]
fn the_check_after_install_skips_dbm_gnu_and_nis_for_a_prebuilt_build() {
    let f = Fixture::new();
    let (s, sha) = serve(tarball_gz(&["dbm.gnu", "nis"], true));
    let def = def_file(&f, &s, &sha);
    let r = run(&f, &["install", &def], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
}

#[test]
fn a_prebuilt_build_without_sqlite3_fails_the_check_and_leaves_nothing() {
    let f = Fixture::new();
    let (s, sha) = serve(tarball_gz(&["sqlite3"], true));
    let def = def_file(&f, &s, &sha);
    let r = run(&f, &["install", &def], &[]);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("doesn't work here without: sqlite3"), "{}", r.stderr);
    assert!(r.stderr.contains("--deps=build"), "{}", r.stderr);
    assert!(left_in_versions(&f).is_empty(), "{:?}", left_in_versions(&f));
}

/// Review Focus 2: a changed layout fails clearly and leaves nothing.
#[test]
fn a_prebuilt_tarball_without_sysconfig_data_fails_and_leaves_nothing() {
    let f = Fixture::new();
    let (s, sha) = serve(tarball_gz(&[], false));
    let def = def_file(&f, &s, &sha);
    let r = run(&f, &["install", &def], &[]);
    assert_ne!(r.code, 0);
    assert!(r.stderr.contains("_sysconfigdata"), "{}", r.stderr);
    assert!(left_in_versions(&f).is_empty(), "{:?}", left_in_versions(&f));
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -q -p pyenv --test cli_install_prebuilt`
Expected: `rpyenv cannot run build step `prebuilt'` (or similar) in the first test's stderr; the others fail likewise.

- [ ] **Step 4: Add the step** in `run_step`, after the `"pypy"` arm:

```rust
        // M8 design §7: a python-build-standalone build is copied as it is, its compiler
        // settings are rewritten for this system and this folder (D18), then the whole is placed.
        "prebuilt" => {
            let staged = txn.stage_dir().join("prebuilt");
            copy_tree(src, &staged).map_err(|e| {
                InstallError::Message(format!(
                    "pyenv: cannot copy {} into {}: {e}",
                    src.display(),
                    staged.display()
                ))
            })?;
            super::prebuilt::rewrite_build_settings(&staged, &plan.prefix)
                .map_err(|m| InstallError::Message(format!("pyenv: {m}")))?;
            if interrupted() {
                return Err(InstallError::Interrupted);
            }
            txn.place(&staged).map_err(|e| {
                InstallError::Message(format!(
                    "pyenv: cannot move the build into {}: {e}",
                    plan.prefix.display()
                ))
            })
        }
```

- [ ] **Step 5: §4.4 for prebuilt.**
  1. In `verify`, replace the `from_source` gate:

```rust
    // M8 design §4.4 is for CPython built from source (ruling R14) and for a prebuilt build
    // (§7): PyPy's and conda's `verify_py*` steps keep upstream's checks only.
    let from_source = pkg.steps.iter().any(|s| s == "standard");
    let prebuilt = pkg.steps.iter().any(|s| s == "prebuilt");
    if let Some(minor) = super::stdlib::minor_of_step(step).filter(|_| from_source || prebuilt) {
        stdlib_check(job, plan, minor, python, prebuilt, say)?;
    }
```

  2. `StoreTcl.cache: Option<&'a Path>`; in `stdlib_failures`, the two library-location checks run only when `cache` is `Some`, the venv check always. Add the `skip: &[&str]` parameter: an import in `skip` is not run. Update the existing unit tests' calls (`None` → unchanged, existing `StoreTcl { cache: x, .. }` → `cache: Some(x)`, `skip: &[]`).
  3. In `stdlib_check(…, prebuilt: bool, …)`: when `prebuilt`, `store_tcl` is `Some(StoreTcl { cache: None, exists: &exists, in_venv: &in_venv })` (Tcl must start in a venv: measured broken in 2024 builds), and `skip` is `&super::prebuilt::SKIPPED_IMPORTS`. Otherwise as today with `skip: &[]`.
  4. The strict failure for prebuilt (not the `continue` summary, which `[c]` never reaches here) prints instead of the two current lines:

```rust
    if prebuilt && !skip {
        say(&format!("pyenv: {name} (a prebuilt build) doesn't work here without: {}", fails.join(", ")));
        say(&format!(
            "Fix the system (certificates: the ca-certificates package; crypt on Fedora: libxcrypt-compat), install it from source instead (pyenv install --deps=build {name}), or set RPYENV_SKIP_PREFLIGHT=1 to keep it."
        ));
        return Err(InstallError::Failed);
    }
```

  (`skip` here is the existing RPYENV_SKIP_PREFLIGHT flag; with it, the existing warning line applies.)

- [ ] **Step 6: Add a unit test** next to the existing `stdlib_failures` tests:

```rust
    #[test]
    fn stdlib_failures_skips_the_given_imports_and_checks_tcl_in_a_venv_without_a_cache() {
        let run = |args: &[&str]| -> (bool, String) {
            let a = args[1];
            (!(a == "import dbm.gnu" || a == "import nis"), "/v/lib/tcl9.0\n".into())
        };
        let exists = |_: &Path| true;
        let venv_ok = || Ok(false);
        let t = StoreTcl { cache: None, exists: &exists, in_venv: &venv_ok };
        let f = stdlib_failures(&run, 12, Some(&t), &crate::install::prebuilt::SKIPPED_IMPORTS);
        assert_eq!(f, vec!["tkinter (Tcl doesn't start in a virtual environment; needs tk)".to_string()]);
        let f = stdlib_failures(&run, 12, None, &[]);
        assert!(f.iter().any(|l| l.starts_with("dbm ")) && f.iter().any(|l| l.starts_with("nis ")), "{f:?}");
    }
```

- [ ] **Step 7: Run everything touched**

Run: `cargo test -q -p pyenv --lib builder` and `cargo test -q -p pyenv --test cli_install_prebuilt`, then the full `cargo test -q --workspace` (WSL).
Expected: all pass, including the 4 new CLI tests.

- [ ] **Step 8: fmt, clippy (WSL and Windows), commit**

```bash
git add crates/pyenv/src/install/builder.rs crates/pyenv/tests/common/fakepbs.rs crates/pyenv/tests/common/mod.rs crates/pyenv/tests/cli_install_prebuilt.rs
git commit -m "M8d: the builder's prebuilt step; the check after install skips dbm.gnu and nis and starts Tcl in a venv (D17)"
```

---

### Task 4: Choosing the route: `--deps=prebuilt`, `[s]`, refusals and messages

**Files:**
- Modify: `crates/pyenv/src/install/deps.rs` (`prompt_lines`, `choice`, `answer_hint` and their tests)
- Modify: `crates/pyenv/src/commands/install.rs`
- Modify: `crates/pyenv/src/install/record.rs` (`prebuilt_text` only; the stale line is Task 5)
- Modify: `crates/pyenv/tests/cli_install_prebuilt.rs`, `crates/pyenv/tests/cli_install_deps.rs` (prompt text where it lists letters)

**Interfaces:**
- Consumes: `prebuilt::{table, host_arch, find, definition_text, summary, tk9_line, Row}`, `store::plain_path`.
- Produces:
  - `deps::prompt_lines(version, deps, m, install, build, crb, continue_offered, prebuilt: Option<&str>)`: when `Some(summary)`, the line `  [s] use a prebuilt CPython instead: <summary>` comes after `[b]` and before `[c]`; when `None`, no `[s]` line.
  - `deps::choice(line, install_offered, build_offered, continue_offered, prebuilt_offered) -> Option<Route>`: `s` → `Route::Prebuilt` when offered.
  - `deps::answer_hint(install, build, continue, prebuilt)`: letters in the order `p, i, b, s, c`.
  - `Resolution` gains `prebuilt: bool`.
  - `record::prebuilt_text(definition: &str, row: &prebuilt::Row) -> String`: `# The python-build-standalone build rpyenv installed (M8 design §7).\ndefinition <definition>\nprebuilt <release> <arch>\n`.

- [ ] **Step 1: deps.rs tests first.** Add to `deps.rs`'s tests:

```rust
    #[test]
    fn the_prompt_offers_s_with_the_builds_summary_only_when_given() {
        let deps = vec![/* reuse the existing tests' one-module Dep, e.g. sqlite missing */];
        let s = "python-build-standalone 2026-10-20 (third-party build; OpenSSL 3.5.9, Tk 9.0; no dbm.gnu or nis)";
        let with = prompt_lines("3.13.16", &deps, Some(Manager::Apt), Err("x"), None, None, true, Some(s));
        let i_s = with.iter().position(|l| l.starts_with("  [s] use a prebuilt CPython instead: python-build-standalone 2026-10-20")).unwrap();
        let i_c = with.iter().position(|l| l.starts_with("  [c]")).unwrap();
        assert!(i_s < i_c, "{with:?}");
        let without = prompt_lines("3.13.16", &deps, Some(Manager::Apt), Err("x"), None, None, true, None);
        assert!(!without.iter().any(|l| l.contains("[s]")), "{without:?}");
    }

    #[test]
    fn s_chooses_prebuilt_only_when_offered_and_the_hint_lists_it() {
        assert_eq!(choice("s", false, false, false, true), Some(Route::Prebuilt));
        assert_eq!(choice("S", false, false, false, true), Some(Route::Prebuilt));
        assert_eq!(choice("s", true, true, true, false), None);
        assert_eq!(answer_hint(true, true, true, true), "pyenv: answer p, i, b, s or c");
        assert_eq!(answer_hint(false, false, false, true), "pyenv: answer p or s");
    }
```

Fill `deps` from an existing test in that module that builds a `Dep` list (copy it). Update every existing call of the three functions (tests included) with the new trailing argument (`None` / `false`).

- [ ] **Step 2: Run them to see them fail**, then implement:
  - `prompt_lines`: after the `[b]` block, `if let Some(s) = prebuilt { out.push(format!("  [s] use a prebuilt CPython instead: {s}")); }`.
  - `choice`: `"s" if prebuilt_offered => Some(Route::Prebuilt),`.
  - `answer_hint`: insert `(prebuilt_offered, "s")` between `b` and `c`.
  Run `cargo test -q -p pyenv --lib deps` until green.

- [ ] **Step 3: The route in `commands/install.rs`.**
  1. A helper, above `resolve_deps`:

```rust
/// M8 design §7: the prebuilt build `found` may use here, or why it can't (for `--deps=prebuilt`).
/// `None` reason text is never shown at the prompt: there `[s]` is simply left out.
fn prebuilt_row(
    found: &Found,
    prefix: &Path,
    debug: bool,
    patch: bool,
) -> Result<crate::install::prebuilt::Row, String> {
    use crate::install::prebuilt;
    let name = &found.name;
    let Some(arch) = prebuilt::host_arch() else {
        return Err(format!(
            "pyenv: no prebuilt builds for {} machines",
            std::env::consts::ARCH
        ));
    };
    if found.origin != Origin::Builtin {
        return Err(format!(
            "pyenv: --deps=prebuilt installs only rpyenv's built-in definitions, not {name}"
        ));
    }
    if debug {
        return Err("pyenv: --deps=prebuilt can't make a debug build (-g)".into());
    }
    if patch {
        return Err("pyenv: --deps=prebuilt can't apply a patch (-p)".into());
    }
    if !store::plain_path(prefix) {
        return Err(format!(
            "pyenv: --deps=prebuilt needs a version folder without spaces, colons or shell characters ({})",
            prefix.display()
        ));
    }
    let rows = prebuilt::table()?;
    prebuilt::find(&rows, name, arch).cloned().ok_or_else(|| {
        format!(
            "pyenv: this rpyenv has no prebuilt build of {name} for {arch} (python-build-standalone releases from 2025-03-11 on); install it from source with --deps=system, install or build"
        )
    })
}
```

  2. `resolve_deps` gains `prebuilt: Option<&str>` (the row's summary, when `[s]` can be offered). Remove the "comes in a later rpyenv release" refusal. Pass `prebuilt` to `prompt_lines`, `prebuilt.is_some()` to `choice` and `answer_hint`. A new match arm before `(Route::Build, _)`:

```rust
            // `[s]` (M8 design §7): install the prebuilt build instead; nothing to resolve.
            (Route::Prebuilt, _) => {
                return Ok(Resolution {
                    cont: false,
                    store: Vec::new(),
                    prebuilt: true,
                })
            }
```

  Every other `Resolution { … }` gets `prebuilt: false`. (`chosen == Some(Route::Prebuilt)` never reaches `resolve_deps`: step 3 decides it first.)
  3. In `install()`, after `parsed` is known and before `let compiles = …`:

```rust
        // `--deps=prebuilt` / RPYENV_BUILD_DEPS=prebuilt (M8 design §7): always the prebuilt
        // build, whatever is missing, or a refusal. The flag wins over the variable.
        let given = f
            .deps
            .clone()
            .or_else(|| env("RPYENV_BUILD_DEPS").filter(|v| !v.is_empty()));
        let wants_prebuilt = given.as_deref() == Some("prebuilt");
        let row = prebuilt_row(&found, &prefix, f.debug, f.patch);
        if wants_prebuilt {
            if let Err(m) = &row {
                say(m);
                status = status.max(1);
                break;
            }
        }
```

  and pass `row.as_ref().ok().map(prebuilt::summary).as_deref()` (bind the `String` first) to `resolve_deps`; skip the whole `compiles` block when `wants_prebuilt`. After that block, when `wants_prebuilt || resolution.prebuilt` (keep the `Resolution` you got), swap the definition:

```rust
        let use_prebuilt = wants_prebuilt || chose_prebuilt;
        let (found, parsed, row) = if use_prebuilt {
            let row = row.expect("checked above");
            let verify = parsed
                .packages
                .iter()
                .flat_map(|p| p.steps.iter())
                .find(|s| s.starts_with("verify_py"))
                .cloned()
                .unwrap_or_else(|| "verify_py3_latest".into());
            let f2 = Found { text: prebuilt::definition_text(&row, &verify), ..found };
            match defs::parse(&f2, &env, &sibling) {
                Ok(d) => {
                    say(&format!("pyenv: installing the {} of {}", prebuilt::summary(&row), f2.name));
                    if let Some(l) = prebuilt::tk9_line(&row) {
                        say(l);
                    }
                    (f2, d, Some(row))
                }
                Err(e) => {
                    say(&e);
                    status = status.max(1);
                    break;
                }
            }
        } else {
            (found, parsed, None)
        };
```

  (Restructure the surrounding `let` bindings as needed so `found`, `parsed` and `job_env` keep their later uses; `sibling` borrows `found`'s origin directory, so compute the swap before `sibling` is needed again or clone what it needs. The existing PyPy/conda swap above shows the pattern.)
  4. After `builder::run` returns `Ok(())` and before `txn.commit()`, when `row` is `Some(r)`, write the record and warn on failure, as the builder does for the store's:

```rust
                if let Some(r) = &row {
                    let file = prefix.join(crate::install::record::FILE);
                    if let Err(e) = std::fs::write(&file, crate::install::record::prebuilt_text(&found.name, r)) {
                        say(&format!("pyenv: warning: cannot write {}: {e}", file.display()));
                    }
                }
```

  Here `found.name` is the definition name (`3.13.16`), also under an alias.
  5. `record::prebuilt_text` as specified in Interfaces, plus `parse` must not take the `prebuilt` line as a library (Task 5 reads it; for now a unit test checks `parse(&prebuilt_text(…))` has `libs` empty).

- [ ] **Step 4: Failing CLI tests**, added to `cli_install_prebuilt.rs`. A test table file (`RPYENV_TEST_PREBUILT_TABLE`) with one row for the built-in definition the tests install, pointed at the server (`RPYENV_TEST_PREBUILT_BASE` = the server's base URL; serve the asset at `/<release>/<asset with %2B>`; check how `server.rs` matches paths and whether it decodes `%2B`, and serve the path as requested). Use a definition name that exists in the vendored share (pick the newest `3.13.*` there and read its `verify_py*` step; the fake interpreter must be `bin/python3.<minor>`, so make `fakepbs::tarball_gz` take the minor if needed). Tests:

```rust
#[test]
fn deps_prebuilt_installs_the_tables_build_without_a_compiler() { /* exit 0; stderr has "installing the python-build-standalone 2026-10-20 (third-party build; OpenSSL 3.5.9, Tk 9.0; no dbm.gnu or nis)"; versions/<v>/.rpyenv-deps has "prebuilt 20261020 x86_64" (host arch); PATH without cc (the fixture's syspath has no compiler stub) */ }

#[test]
fn rpyenv_build_deps_prebuilt_works_without_a_terminal() { /* same via the variable, RPYENV_TEST_TTY unset */ }

#[test]
fn a_prebuilt_download_with_the_wrong_hash_leaves_nothing() { /* table sha of other bytes: exit != 0, "checksum" in stderr, versions/ empty */ }

#[test]
fn deps_prebuilt_without_a_row_refuses_before_downloading() { /* empty test table: exit 1, "has no prebuilt build of", the server saw no request */ }

#[test]
fn deps_prebuilt_refuses_debug_and_patch() { /* -g and -p: exit 1 with the two messages */ }

/// Review Focus 1.
#[test]
fn deps_prebuilt_refuses_a_root_with_a_space() { /* PYENV_ROOT under a folder with a space: exit 1, "without spaces", nothing downloaded */ }

/// Review Focus 3.
#[test]
fn an_interrupt_during_the_prebuilt_download_leaves_nothing() { /* Reply::StallThenFinish, SIGINT the group (common::sigint_group) once the server got the request: exit 130, versions/ empty, no record */ }

/// Review Focus 4.
#[test]
fn an_alias_installs_under_its_name_and_records_the_definition() { /* --deps=prebuilt <v>:mine → versions/mine, record "definition <v>" */ }

#[test]
fn the_prompt_offers_s_and_s_installs_the_prebuilt_build() { /* RPYENV_TEST_TTY=1, RPYENV_TEST_PREFLIGHT=sqlite (as cli_install_deps does), stdin "s\n": stderr has "[s] use a prebuilt CPython instead: python-build-standalone 2026-10-20", exit 0, record has "prebuilt" */ }

#[test]
fn the_prompt_leaves_s_out_without_a_row() { /* empty table: no "[s]" in stderr; answering "s" prints "pyenv: answer p" and the hint has no s */ }

#[test]
fn a_tk_9_build_says_so() { /* tk 9.0 row → the Tk 9 line; a tk 8.6 row → no such line */ }
```

  Write each body fully, in the style of `cli_install_pypy.rs` and `cli_install_deps.rs` (copy their helpers; `cli_install_deps.rs` has the prompt and interrupt patterns, including the ETXTBSY retry for stubs).

- [ ] **Step 5: Run them to see them fail, implement until they pass**, then the full suite (WSL), fmt, clippy (WSL and Windows).

Run: `cargo test -q -p pyenv --test cli_install_prebuilt` then `cargo test -q --workspace`.

- [ ] **Step 6: Commit**

```bash
git add crates/pyenv/src/install/deps.rs crates/pyenv/src/commands/install.rs crates/pyenv/src/install/record.rs crates/pyenv/tests/cli_install_prebuilt.rs crates/pyenv/tests/cli_install_deps.rs
git commit -m "M8d: --deps=prebuilt and [s] install a python-build-standalone build of a built-in definition, naming its date, OpenSSL and Tk"
```

---

### Task 5: The record's stale line for prebuilt builds

**Files:**
- Modify: `crates/pyenv/src/install/record.rs`, `crates/pyenv/src/commands/install.rs` (the `stale_lines` call), `crates/pyenv/tests/cli_install_prebuilt.rs`

**Interfaces:**
- `Record` gains `pub prebuilt: Option<(String, String)>` (release, arch); `parse` reads `prebuilt <release> <arch>` (before the library arm).
- `stale_lines(versions, now, prebuilt_now: &dyn Fn(&str, &str) -> Option<(String, String)>)`: `prebuilt_now(definition, arch)` gives the table's (release, OpenSSL) for that row.

- [ ] **Step 1: Failing unit test** in `record.rs`:

```rust
    #[test]
    fn a_prebuilt_record_names_a_newer_build_in_the_table() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path();
        std::fs::create_dir_all(v.join("mine")).unwrap();
        std::fs::write(v.join("mine").join(FILE), "# c\ndefinition 3.13.16\nprebuilt 20261020 x86_64\n").unwrap();
        let none = |_: &str, _: &str, _: Origin| None;
        let newer = |d: &str, a: &str| (d == "3.13.16" && a == "x86_64").then(|| ("20261105".to_string(), "3.5.10".to_string()));
        assert_eq!(
            stale_lines(v, &none, &newer),
            vec!["pyenv: mine is the python-build-standalone build of 2026-10-20; rpyenv now offers the one of 2026-11-05 (OpenSSL 3.5.10). To reinstall it: pyenv install -f --deps=prebuilt 3.13.16:mine".to_string()]
        );
        let same = |_: &str, _: &str| Some(("20261020".to_string(), "3.5.9".to_string()));
        assert!(stale_lines(v, &none, &same).is_empty());
        let gone = |_: &str, _: &str| None;
        assert!(stale_lines(v, &none, &gone).is_empty());
        let r = parse("definition 3.13.16\nprebuilt 20261020 x86_64\n").unwrap();
        assert_eq!(r.prebuilt, Some(("20261020".into(), "x86_64".into())));
        assert!(r.libs.is_empty());
    }
```

  Use `prebuilt::date` for the dates. A later release is "newer" when it compares greater as a string (8 digits).

- [ ] **Step 2: Implement**, update existing `stale_lines` calls and tests (`&|_, _| None`), and in `install()` pass:

```rust
        let prebuilt_rows = crate::install::prebuilt::table().unwrap_or_default();
        let prebuilt_now = |d: &str, a: &str| {
            crate::install::prebuilt::find(&prebuilt_rows, d, a).map(|r| (r.release.clone(), r.openssl.clone()))
        };
```

- [ ] **Step 3: A CLI test**: a version folder with a prebuilt record of an older release, the test table with a newer one; any `pyenv install` (for example of a missing definition, which then fails) prints the line. Then fmt, clippy, full suite, commit:

```bash
git commit -m "M8d: pyenv install says when rpyenv offers a newer build of an installed prebuilt version"
```

---

### Task 6: CI: the weekly table and the real install

**Files:**
- Modify: `.github/workflows/sync-python-build.yml`
- Create: `ci/check_prebuilt.sh`
- Modify: `.github/workflows/install-linux.yml`

- [ ] **Step 1: The sync step.** After "Rebuild the conda table", add "Rebuild the prebuilt table", modeled on the PyPy and conda steps (same failure handling: a table step's failure lets the CPython sync still open its PR, and "Fail when a table couldn't be rebuilt" fails the run). It runs `python3 ci/prebuilt_table.py --share crates/pyenv/python-build/share --write --workdir "$RUNNER_TEMP/prebuilt"` with `GH_TOKEN: ${{ github.token }}` (gh needs a token for `api` and `attestation verify`). It runs after the vendored copy is updated, so new definitions get rows in the same PR. Add `ci/test_prebuilt_table.py` to where the workflow (or ci.yml) runs the other `ci/test_*.py` files.

- [ ] **Step 2: `ci/check_prebuilt.sh <pyenv binary dir>`** (bash, `set -euo pipefail`), run on the runner itself:
  1. Picks the newest `3.13.*` row for `uname -m` from `crates/pyenv/prebuilt/linux.tsv` (not `t`).
  2. `PYENV_ROOT=$(mktemp -d)`; `RPYENV_BUILD_DEPS=prebuilt pyenv install <v>`; expects exit 0 and the "third-party build" line.
  3. Checks: `python -c "import ssl, sqlite3, ctypes, lzma, bz2, readline, curses, uuid, tkinter; tkinter.Tcl()"`; `sysconfig.get_config_var('CC')` starts with `cc`; no `/install` in the sysconfig data; `.rpyenv-deps` has `prebuilt <release> <arch>`.
  4. Builds a C extension with the runner's gcc: a venv from the installed Python, `pip install setuptools`, a one-file extension (as in the planning spike, `hello.c` + `setup.py`), `pip install --no-build-isolation .`, `python -c "import hello"`.
  5. **The check must be able to fail** (spec §9): `docker run --rm -v <bin>:/in:ro ubuntu:24.04` (no `ca-certificates` in that image) runs `RPYENV_BUILD_DEPS=prebuilt pyenv install <v>` with the table's URL reachable (the image has no CA certificates, so download with... see note) and expects exit 1, `ssl certificates` in the output, and an empty `versions/`.
     Note: rpyenv's own download needs CA certificates too (D-128: it falls back to its pinned bundle when the system has none). Measure in this step whether the download succeeds in that image; if rpyenv's download itself fails first, make the must-fail case a different missing piece the check catches instead: install the prebuilt build normally on the runner, then copy the installed tree into the container and run the check by `pyenv install -f` from a pre-filled download cache (`PYTHON_BUILD_CACHE_PATH`), so only the check differs. Record what was measured in the script's comment.
- [ ] **Step 3: The workflow job** `prebuilt (${{ matrix.runner }})` in `install-linux.yml`, matrix `runner: [ubuntu-latest, ubuntu-24.04-arm]`, every PR and push (and add `ci/check_prebuilt.sh` and `crates/pyenv/prebuilt/**` to the workflow's path filters). It builds rpyenv as the `pypy` job does and runs `bash ci/check_prebuilt.sh <dir>`. The docker must-fail part runs on `ubuntu-latest` only.
- [ ] **Step 4: Run the script locally in WSL** (scratch `PYENV_ROOT`, real network; the container part in podman with `--cgroup-manager=cgroupfs`, never on the host's package manager) and paste its output summary into the report. Commit:

```bash
git commit -m "CI: the weekly sync rebuilds the prebuilt table; a real prebuilt install on x86_64 and aarch64 builds a C extension, and fails without CA certificates"
```

---

### Task 7: Documentation

**Files:**
- Modify: `docs/parity/allowlist.md`, `README.md`, `docs/distributions.md`, `docs/superpowers/specs/2026-10-17-m8-linux-build-deps-design.md`, `docs/specs/2026-09-27-rpyenv-design.md` (only if it lists M8 parts' status)

- [ ] **Step 1: Allowlist rows** (next free numbers, D-130 on), each cited by a test above:
  - `install --deps=prebuilt`, `RPYENV_BUILD_DEPS=prebuilt` (CPython 3, Linux): upstream has no such option; rpyenv installs the python-build-standalone build from its table (date, OpenSSL, Tk named; no dbm.gnu or nis), refusing `-g`, `-p`, a user's own definition, a root with spaces, and versions without a row. Cite `deps_prebuilt_installs_the_tables_build_without_a_compiler` and the refusal tests.
  - The prompt's `[s]`. Cite `the_prompt_offers_s_and_s_installs_the_prebuilt_build`.
  - The stale-build line. Cite Task 5's CLI test.
  - Update D-124's text if it lists the prompt's letters.
- [ ] **Step 2: README**: the `--deps` row gains `prebuilt` (a third-party python-build-standalone build; never the default). **distributions.md**: a "Prebuilt builds" section: what it is, the trust model (D15) in two sentences, what the build lacks (dbm.gnu, nis), Tk 9 on 2026 builds, the compiler-settings rewrite, how to reinstall when rpyenv offers a newer build.
- [ ] **Step 3: Spec**: mark M8d done in §3's table (as M8c), and add the measured numbers from Task 1 (row count) and Task 6 (CI times) to §7.
- [ ] **Step 4: Run** `python parity/coverage.py` (expect every row covered), then commit:

```bash
git commit -m "Docs: the prebuilt route (allowlist, README, distributions.md, spec §7)"
```

---

## Self-review notes (planner)

- Spec coverage: §7 checks (Task 1 sync, Task 4 hash), same §4.4 check (Task 3), never default (Task 4: only flag, variable or `[s]`), third-party + date + OpenSSL in the prompt (Task 4), Tk 9 (Task 4), coverage (Task 1 filter + Task 4 `[s]` left out), D17 (Task 3), D18 (Tasks 2–3), D19 (Task 4), record (Tasks 4–5), §9's CI row (Task 6), must-fail (Task 6).
- Types: `Row`, `summary`, `definition_text(row, verify_step)`, `SKIPPED_IMPORTS`, `rewrite_build_settings(tree, prefix)`, `StoreTcl.cache: Option<&Path>`, `stdlib_failures(…, skip)`, `Resolution.prebuilt`, `prompt_lines(…, prebuilt)`, `choice(…, prebuilt_offered)`, `answer_hint(…, prebuilt)`, `record::prebuilt_text(definition, row)`, `Record.prebuilt`, `stale_lines(…, prebuilt_now)` are used with the same names throughout.
- Known measurement left to execution: whether rpyenv's own download works in an image without CA certificates (Task 6 Step 2.5 says what to do either way).
