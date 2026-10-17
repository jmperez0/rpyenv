"""The build catalog's weekly check (M8 design §6.1): for every library in
crates/pyenv/deps/catalog.tsv, the newest upstream release. With --write, each row that is
behind moves to that release: the new tarball is downloaded here and its SHA-256 computed, so
the pull request the sync workflow opens carries a hash a person reviews before it ships.
Tcl and Tk stay on 8.6 (spec D11) and OpenSSL on 3.x (definitions pin their own).

  python3 ci/deps_catalog.py --catalog crates/pyenv/deps/catalog.tsv [--write] [--report out.md]

Exit 0: up to date, or rows written. 1: a source couldn't be read (nothing is written).
2: bad usage.
"""
import argparse
import hashlib
import json
import os
import re
import sys
import tempfile
import urllib.request

GH = "https://github.com/{repo}/releases/download/{tag}/{name}"

# key: (kind, details). Kinds: listing (url, regex, url template), github (repo, tag template,
# file template), sqlite, kernel (util-linux's two-level listing), cacert.
SOURCES = {
    "pkgconf": ("listing", "https://distfiles.ariadne.space/pkgconf/", r"pkgconf-(\d+\.\d+\.\d+)\.tar\.xz", "https://distfiles.ariadne.space/pkgconf/pkgconf-{v}.tar.xz"),
    "zlib": ("github", "madler/zlib", "v{v}", "zlib-{v}.tar.gz"),
    "bzip2": ("listing", "https://sourceware.org/pub/bzip2/", r"bzip2-(\d+\.\d+\.\d+)\.tar\.gz", "https://sourceware.org/pub/bzip2/bzip2-{v}.tar.gz"),
    "xz": ("github", "tukaani-project/xz", "v{v}", "xz-{v}.tar.gz"),
    "ncurses": ("listing", "https://ftp.gnu.org/gnu/ncurses/", r"ncurses-(\d+\.\d+)\.tar\.gz", "https://ftp.gnu.org/gnu/ncurses/ncurses-{v}.tar.gz"),
    "readline": ("listing", "https://ftp.gnu.org/gnu/readline/", r"readline-(\d+\.\d+)\.tar\.gz", "https://ftp.gnu.org/gnu/readline/readline-{v}.tar.gz"),
    "gdbm": ("listing", "https://ftp.gnu.org/gnu/gdbm/", r"gdbm-(\d+\.\d+(?:\.\d+)?)\.tar\.gz", "https://ftp.gnu.org/gnu/gdbm/gdbm-{v}.tar.gz"),
    "sqlite": ("sqlite",),
    "libffi": ("github", "libffi/libffi", "v{v}", "libffi-{v}.tar.gz"),
    "tcl": ("listing", "https://www.tcl-lang.org/software/tcltk/download.html", r"tcl(8\.6\.\d+)-src\.tar\.gz", "https://downloads.sourceforge.net/project/tcl/Tcl/{v}/tcl{v}-src.tar.gz"),
    "tk": ("listing", "https://www.tcl-lang.org/software/tcltk/download.html", r"tk(8\.6\.\d+)-src\.tar\.gz", "https://downloads.sourceforge.net/project/tcl/Tcl/{v}/tk{v}-src.tar.gz"),
    "libuuid": ("kernel",),
    "libxcrypt": ("github", "besser82/libxcrypt", "v{v}", "libxcrypt-{v}.tar.xz"),
    "libtirpc": ("listing", "https://sourceforge.net/projects/libtirpc/files/libtirpc/", r"libtirpc/(\d+\.\d+\.\d+)/", "https://downloads.sourceforge.net/project/libtirpc/libtirpc/{v}/libtirpc-{v}.tar.bz2"),
    "libnsl": ("github", "thkukuk/libnsl", "v{v}", "libnsl-{v}.tar.xz"),
    "zstd": ("github", "facebook/zstd", "v{v}", "zstd-{v}.tar.gz"),
    "openssl": ("github", "openssl/openssl", "openssl-{v}", "openssl-{v}.tar.gz"),
    "cacert": ("cacert",),
}


def key(v):
    return [int(p) if p.isdigit() else -1 for p in re.split(r"[.\-]", v)]


def newest(versions):
    return max(versions, key=key) if versions else None


def matches(text, pattern):
    return sorted(set(re.findall(pattern, text)), key=key)


def allowed(lib, versions):
    """Spec D11 (Tcl/Tk 8.6) and OpenSSL 3.x for the catalog's default."""
    if lib in ("tcl", "tk"):
        return [v for v in versions if v.startswith("8.6.")]
    if lib == "openssl":
        return [v for v in versions if v.startswith("3.")]
    return list(versions)


def tag_versions(tags):
    out = []
    for t in tags:
        m = re.fullmatch(r"(?:v|openssl-)?(\d+(?:\.\d+)+)", t)
        if m:
            out.append(m.group(1))
    return out


def sqlite_latest(page):
    m = re.search(r"PRODUCT,(\d+\.\d+\.\d+),(\d{4}/sqlite-autoconf-\d+\.tar\.gz)", page)
    return (m.group(1), "https://sqlite.org/" + m.group(2)) if m else None


def cacert_latest(page):
    found = re.findall(r"cacert-(\d{4}-\d{2}-\d{2})\.pem", page)
    return max(found) if found else None


def get(url, binary=False):
    req = urllib.request.Request(url, headers={"User-Agent": "rpyenv-deps-catalog"})
    token = os.environ.get("GH_TOKEN")
    if token and url.startswith("https://api.github.com/"):
        req.add_header("Authorization", "Bearer " + token)
    with urllib.request.urlopen(req, timeout=60) as r:
        data = r.read()
    return data if binary else data.decode("utf-8", "replace")


def latest(lib):
    """(version, url) of the newest allowed upstream release of `lib`."""
    s = SOURCES[lib]
    kind = s[0]
    if kind == "listing":
        _, url, pattern, template = s
        v = newest(allowed(lib, matches(get(url), pattern)))
        return v, template.format(v=v)
    if kind == "github":
        _, repo, tag, name = s
        rel = json.loads(get(f"https://api.github.com/repos/{repo}/releases?per_page=50"))
        tags = [r["tag_name"] for r in rel if not r["prerelease"] and not r["draft"]]
        v = newest(allowed(lib, tag_versions(tags)))
        return v, GH.format(repo=repo, tag=tag.format(v=v), name=name.format(v=v))
    if kind == "sqlite":
        return sqlite_latest(get("https://sqlite.org/download.html"))
    if kind == "kernel":
        base = "https://mirrors.edge.kernel.org/pub/linux/utils/util-linux/"
        series = newest(matches(get(base), r"v(\d+\.\d+)/"))
        v = newest(matches(get(f"{base}v{series}/"), r"util-linux-(\d+\.\d+(?:\.\d+)?)\.tar\.xz"))
        return v, f"{base}v{series}/util-linux-{v}.tar.xz"
    if kind == "cacert":
        d = cacert_latest(get("https://curl.se/docs/caextract.html"))
        return d, f"https://curl.se/ca/cacert-{d}.pem"
    raise ValueError(kind)


def read_rows(text):
    return [l.split("\t") for l in text.splitlines() if l.strip() and not l.startswith("#")]


def replace_row(text, lib, version, url, sha):
    out = []
    for l in text.splitlines(keepends=True):
        if l.split("\t")[0] == lib and not l.startswith("#"):
            l = f"{lib}\t{version}\t{url}\t{sha}\n"
        out.append(l)
    return "".join(out)


def sha256_of(url):
    data = get(url, binary=True)
    return hashlib.sha256(data).hexdigest(), data


def main(argv):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--catalog", required=True)
    p.add_argument("--write", action="store_true")
    p.add_argument("--report")
    a = p.parse_args(argv)
    with open(a.catalog, encoding="utf-8") as f:
        text = f.read()
    changes, failed = [], []
    for lib, version, _url, _sha in read_rows(text):
        try:
            v, url = latest(lib)
        except Exception as e:  # noqa: BLE001 - every source failure is reported the same way
            failed.append(f"{lib}: {e}")
            continue
        if v is None:
            failed.append(f"{lib}: no release found")
        elif key(v) > key(version):
            changes.append((lib, version, v, url))
    for line in failed:
        print("cannot read", line, file=sys.stderr)
    if failed:
        return 1
    lines = []
    for lib, old, new, url in changes:
        sha, data = sha256_of(url)
        if lib == "cacert":
            published = get(url + ".sha256").split()[0]
            if published != sha:
                print(f"cacert {new}: download {sha} != published {published}", file=sys.stderr)
                return 1
        text = replace_row(text, lib, new, url, sha)
        lines.append(f"- `{lib}` {old} → {new} (`{sha}`)")
        print(lines[-1])
    if a.report:
        with open(a.report, "w", encoding="utf-8") as f:
            f.write("\n".join(lines) + ("\n" if lines else ""))
    if not changes:
        print("the build catalog is up to date")
    elif a.write:
        with open(a.catalog, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
