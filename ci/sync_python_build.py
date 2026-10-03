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


class Unsafe(Exception):
    """The upstream tree (untrusted input) holds a symlink or a path that leaves it."""


def refuse_links(path, root):
    """Raises Unsafe if `path` or any component of it below `root` is a symlink, or if its
    real location is not under the real `root`."""
    rel = os.path.relpath(path, root)
    cur = root
    for part in ([] if rel == "." else rel.split(os.sep)):
        cur = os.path.join(cur, part)
        if os.path.islink(cur):
            raise Unsafe(f"refusing symlink {cur}")
    real_root = os.path.realpath(root)
    real = os.path.realpath(path)
    if real != real_root and not real.startswith(real_root + os.sep):
        raise Unsafe(f"refusing {path}: it resolves to {real}, outside {real_root}")


def wanted(upstream):
    """{relative path under DEST: bytes} for everything that should be vendored."""
    share = os.path.join(upstream, "plugins", "python-build", "share", "python-build")
    out = {}
    refuse_links(share, upstream)
    names = sorted(n for n in os.listdir(share) if n[:1].isdigit() and os.path.isfile(os.path.join(share, n)))
    for n in names:
        refuse_links(os.path.join(share, n), upstream)
        with open(os.path.join(share, n), "rb") as f:
            out["share/" + n] = f.read()
        pdir = os.path.join(share, "patches", n)
        src = pdir
        if os.path.islink(pdir):
            # Upstream's one legitimate link: patches/3.13.8t -> 3.13.8 (a bare sibling name).
            target = os.readlink(pdir)
            sib = os.path.join(share, "patches", target)
            if not target or target in (".", "..") or "/" in target or "\\" in target                     or os.path.islink(sib) or not os.path.isdir(sib):
                raise Unsafe(f"refusing symlink {pdir} -> {target}")
            src = sib
        elif os.path.lexists(pdir):
            refuse_links(pdir, upstream)
        refuse_links(src, upstream)
        for dirpath, dirs, files in os.walk(src):
            for dn in dirs:
                refuse_links(os.path.join(dirpath, dn), upstream)
            for fn in files:
                full = os.path.join(dirpath, fn)
                refuse_links(full, upstream)
                rel = "patches/" + n + "/" + os.path.relpath(full, src).replace(os.sep, "/")
                with open(full, "rb") as f:
                    out["share/" + rel] = f.read()
    refuse_links(os.path.join(upstream, "LICENSE"), upstream)
    with open(os.path.join(upstream, "LICENSE"), "rb") as f:
        out["LICENSE"] = f.read()
    return out


def version_of(upstream):
    with open(os.path.join(upstream, "libexec", "pyenv---version"), encoding="utf-8") as f:
        m = re.search(r'^version="([^"]+)"', f.read(), re.M)
    return m.group(1) if m else "unknown"


def present(dest):
    out = {}
    for dirpath, dirs, files in os.walk(dest):
        for dn in dirs:
            refuse_links(os.path.join(dirpath, dn), dest)
        for fn in files:
            full = os.path.join(dirpath, fn)
            refuse_links(full, dest)
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
    try:
        changes = sync(a.upstream, DEST, a.commit, a.write)
    except Unsafe as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    for c in changes:
        print(c)
    print(f"{len(changes)} difference(s)")
    return 0 if a.write or not changes else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
