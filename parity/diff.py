"""Differential tests (spec §12.4). Each case in diff_cases.py runs against rpyenv and against
the real upstream (pyenv on Linux, pyenv-win on Windows), each on a freshly built fixture root
at the same path, and the exit codes, stdout, stderr and the case's compared files are checked
byte for byte (plan decision 4).

  python parity/diff.py --rpyenv <dir with pyenv[.exe]> --upstream <pyenv source | pyenv-win dir> [--only NAME]

On Linux, --upstream is pyenv's source tree (with bin/pyenv). On Windows it is the `pyenv-win`
folder of a pyenv-win checkout (with bin/, libexec/ and .versions_cache.xml; .version is in
its parent).
"""
import argparse
import os
import shutil
import stat
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import allowlist  # noqa: E402
import diff_cases  # noqa: E402

WINDOWS = os.name == "nt"
OS_NAME = "Windows" if WINDOWS else "Linux"
VERSIONS = ("3.9.1", "3.10.11") if WINDOWS else ("3.12.10", "3.11.9")
BASE = os.path.join(allowlist.REPO, "target", "parity-diff")


def expand(text):
    return text.replace("{v0}", VERSIONS[0]).replace("{v1}", VERSIONS[1])


def applicable(case, table, os_name=OS_NAME):
    """The rows in `case.allow` that apply on `os_name`."""
    return [r for r in case.allow if table.get(r) in ("both", os_name)]


def verdict(same, rows_here):
    if same:
        return "stale" if rows_here else "same"
    return "allowed" if rows_here else "differs"


def remove_tree(path):
    """Removes `path`, clearing read-only flags that block a delete on Windows."""
    def again(func, p, _):
        os.chmod(p, stat.S_IWRITE)
        func(p)

    if os.path.exists(path):
        # `onerror` is deprecated from Python 3.12, whose `onexc` takes the same arguments.
        hook = {"onexc": again} if sys.version_info >= (3, 12) else {"onerror": again}
        shutil.rmtree(path, **hook)


def resolve(places, rel):
    head, _, tail = rel.partition("/")
    return os.path.join(places[head], *tail.split("/")) if tail else places[head]


def build(upstream, case):
    """A fresh fixture at BASE: two versions with a `python`, the first one global."""
    remove_tree(BASE)
    root = os.path.join(BASE, "pyenv-win" if WINDOWS else "root")
    work = os.path.join(BASE, "work")
    os.makedirs(work)
    for v in VERSIONS:
        d = os.path.join(root, "versions", v, *([] if WINDOWS else ["bin"]))
        os.makedirs(d)
        exe = os.path.join(d, "python.exe" if WINDOWS else "python")
        with open(exe, "w", encoding="utf-8") as f:
            f.write("" if WINDOWS else "#!/bin/sh\n")
        if not WINDOWS:
            os.chmod(exe, 0o755)
    os.makedirs(os.path.join(root, "shims"))
    with open(os.path.join(root, "version"), "w", encoding="utf-8", newline="") as f:
        f.write(VERSIONS[0] + "\n")
    if WINDOWS:
        # pyenv-win finds its root from its own location, so its scripts live in the fixture.
        for d in ("bin", "libexec"):
            shutil.copytree(os.path.join(upstream, d), os.path.join(root, d))
        shutil.copy(os.path.join(upstream, ".versions_cache.xml"), root)
        shutil.copy(os.path.join(upstream, "..", ".version"), BASE)
    places = {"root": root, "work": work}
    for rel, content in case.files:
        p = resolve(places, rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w", encoding="utf-8", newline="") as f:
            f.write(expand(content))
    for rel in case.remove:
        p = resolve(places, rel)
        if os.path.isdir(p):
            remove_tree(p)
        elif os.path.exists(p):
            os.remove(p)
    for rel in case.readonly:
        os.chmod(resolve(places, rel), stat.S_IREAD)
    return places


def snapshot(path):
    if os.path.isdir(path):
        return b"<directory>"
    if not os.path.exists(path):
        return None
    with open(path, "rb") as f:
        return f.read()


def run(tool, rpyenv, upstream, places, case):
    root, work = places["root"], places["work"]
    env = {"HOME": BASE, "USERPROFILE": BASE, "PYENV_ROOT": root}
    if WINDOWS:
        sysroot = os.environ["SystemRoot"]
        system32 = os.path.join(sysroot, "System32")
        env.update(SystemRoot=sysroot, ComSpec=os.path.join(system32, "cmd.exe"),
                   PATHEXT=".COM;.EXE;.BAT;.CMD;.VBS", PYENV=root, PYENV_HOME=root,
                   PATH=os.pathsep.join([os.path.join(root, "shims"), system32, sysroot]))
        if tool == "upstream":
            cmd = [os.path.join(system32, "cmd.exe"), "/d", "/c", "call",
                   os.path.join(root, "bin", "pyenv.bat")]
        else:
            cmd = [os.path.join(rpyenv, "pyenv.exe")]
    else:
        env.update(LANG="C.UTF-8", PATH=os.pathsep.join([os.path.join(root, "shims"), "/usr/bin", "/bin"]))
        cmd = [os.path.join(upstream, "bin", "pyenv")] if tool == "upstream" else [os.path.join(rpyenv, "pyenv")]
    for k, v in case.env:
        if v is None:
            env.pop(k, None)
        else:
            env[k] = expand(v)
    p = subprocess.run(cmd + [expand(a) for a in case.args], cwd=work, env=env,
                       capture_output=True, stdin=subprocess.DEVNULL, timeout=120)
    return (p.returncode, p.stdout, p.stderr, tuple(snapshot(resolve(places, r)) for r in case.compare))


def describe(a, b):
    out = []
    for label, x, y in (("exit code", a[0], b[0]), ("stdout", a[1], b[1]), ("stderr", a[2], b[2])):
        if x != y:
            out.append(f"    {label}: upstream {x!r}\n    {label}: rpyenv   {y!r}")
    for i, (x, y) in enumerate(zip(a[3], b[3])):
        if x != y:
            out.append(f"    compared file {i}: upstream {x!r}\n    compared file {i}: rpyenv   {y!r}")
    return "\n".join(out)


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("--rpyenv", required=True)
    p.add_argument("--upstream", required=True)
    p.add_argument("--only")
    a = p.parse_args(argv)
    rpyenv, upstream = os.path.abspath(a.rpyenv), os.path.abspath(a.upstream)
    table = allowlist.rows()
    counts, failures = {}, []
    for case in diff_cases.CASES:
        if case.os not in ("both", OS_NAME) or (a.only and case.name != a.only):
            continue
        unknown = [r for r in case.allow if r not in table]
        if unknown:
            failures.append(f"{case.name}: unknown allowlist rows {unknown}")
            continue
        results = {}
        for tool in ("upstream", "rpyenv"):
            places = build(upstream, case)
            results[tool] = run(tool, rpyenv, upstream, places, case)
        rows_here = applicable(case, table)
        v = verdict(results["upstream"] == results["rpyenv"], rows_here)
        counts[v] = counts.get(v, 0) + 1
        print(f"{v:8} {case.name}" + (f"  ({', '.join(rows_here)})" if rows_here else ""))
        if v in ("stale", "differs"):
            detail = describe(results["upstream"], results["rpyenv"])
            failures.append(f"{case.name}: {v}" + (f"\n{detail}" if detail else ""))
            if detail:
                print(detail)
    remove_tree(BASE)
    print(f"{OS_NAME}: " + ", ".join(f"{n} {k}" for k, n in sorted(counts.items())))
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(f"### Differential tests ({OS_NAME})\n\n" + ", ".join(f"{n} {k}" for k, n in sorted(counts.items()))
                    + "\n\n" + "".join(f"- {x.splitlines()[0]}\n" for x in failures) + "\n")
    for x in failures:
        print("FAIL " + x, file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
