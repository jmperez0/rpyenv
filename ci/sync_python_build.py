"""Keeps crates/pyenv/python-build in step with upstream pyenv (plan M2a, Decision 1).

  python ci/sync_python_build.py --upstream <pyenv checkout> --commit <sha> [--write]

Copies the CPython definitions (file names starting with a digit) and their
`patches/<name>/` directories byte for byte, plus pyenv's LICENSE, and records the commit
and version in UPSTREAM. Without --write it only lists the differences and exits 1 if any.
"""
import argparse
import os
import re
import shutil
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEST = os.path.join(REPO, "crates", "pyenv", "python-build")


def wanted(upstream):
    """{relative path under DEST: bytes} for everything that should be vendored."""
    share = os.path.join(upstream, "plugins", "python-build", "share", "python-build")
    out = {}
    names = sorted(n for n in os.listdir(share) if n[:1].isdigit() and os.path.isfile(os.path.join(share, n)))
    for n in names:
        with open(os.path.join(share, n), "rb") as f:
            out["share/" + n] = f.read()
        pdir = os.path.join(share, "patches", n)
        for dirpath, _, files in os.walk(pdir):
            for fn in files:
                full = os.path.join(dirpath, fn)
                rel = os.path.relpath(full, share).replace(os.sep, "/")
                with open(full, "rb") as f:
                    out["share/" + rel] = f.read()
    with open(os.path.join(upstream, "LICENSE"), "rb") as f:
        out["LICENSE"] = f.read()
    return out


def version_of(upstream):
    with open(os.path.join(upstream, "libexec", "pyenv---version"), encoding="utf-8") as f:
        m = re.search(r'^version="([^"]+)"', f.read(), re.M)
    return m.group(1) if m else "unknown"


def present(dest):
    out = {}
    for dirpath, _, files in os.walk(dest):
        for fn in files:
            full = os.path.join(dirpath, fn)
            rel = os.path.relpath(full, dest).replace(os.sep, "/")
            with open(full, "rb") as f:
                out[rel] = f.read()
    return out


def sync(upstream, dest, commit, write):
    want = wanted(upstream)
    want["UPSTREAM"] = f"pyenv {commit} {version_of(upstream)}\n".encode("utf-8")
    have = present(dest) if os.path.isdir(dest) else {}
    changes = []
    for rel in sorted(set(want) | set(have), key=lambda r: (r == "UPSTREAM", r)):
        if rel not in have:
            changes.append(f"added {rel}")
        elif rel not in want:
            changes.append(f"removed {rel}")
        elif have[rel] != want[rel]:
            changes.append(f"changed {rel}")
    order = {"changed": 0, "added": 1, "removed": 2}
    changes.sort(key=lambda c: (c.endswith("UPSTREAM"), order[c.split()[0]], c))
    if write:
        if os.path.isdir(dest):
            shutil.rmtree(dest)
        for rel, data in want.items():
            p = os.path.join(dest, *rel.split("/"))
            os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, "wb") as f:
                f.write(data)
    return changes


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("--upstream", required=True)
    p.add_argument("--commit", required=True)
    p.add_argument("--write", action="store_true")
    a = p.parse_args(argv)
    changes = sync(a.upstream, DEST, a.commit, a.write)
    for c in changes:
        print(c)
    print(f"{len(changes)} difference(s)")
    return 0 if a.write or not changes else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
