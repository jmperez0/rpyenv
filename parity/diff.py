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
import re
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
    """{v0}/{v1} become the fixture's versions and {sysroot} the harness's SystemRoot (Windows)."""
    text = text.replace("{v0}", VERSIONS[0]).replace("{v1}", VERSIONS[1])
    return text.replace("{sysroot}", os.environ.get("SystemRoot", ""))


def applicable(case, table, os_name=OS_NAME):
    """The rows in `case.allow` that apply on `os_name`."""
    return [r for r in case.allow if table.get(r) in ("both", os_name)]


def verdict(same, rows_here):
    if same:
        return "stale" if rows_here else "same"
    return "allowed" if rows_here else "differs"


GOLDEN = os.path.join(os.path.dirname(os.path.abspath(__file__)), "golden", OS_NAME.lower())
NEXT_FILE = b"\n--- next file ---\n"


def slug(name):
    """The golden file name of a case: lowercased, each run of non-alphanumerics a `-`."""
    return re.sub(r"[^a-z0-9]+", "-", name.lower())


def to_placeholders(data, root, work):
    """Replaces the fixture's paths by {root} and {work}, the longer path first."""
    for path, name in sorted(((root, b"{root}"), (work, b"{work}")), key=lambda t: -len(t[0])):
        data = data.replace(path.encode("utf-8"), name)
    return data


def from_placeholders(data, root, work):
    return data.replace(b"{root}", root.encode("utf-8")).replace(b"{work}", work.encode("utf-8"))


def normalise(result, root, work):
    """A (code, stdout, stderr, files) result with the fixture paths as placeholders."""
    return (result[0],) + tuple(to_placeholders(x, root, work) for x in result[1:])


def judge(same, rows_here, rpyenv_result, golden, root, work):
    """The verdict and the problems behind a `differs`. A difference that a row allows must
    also leave rpyenv's own output exactly equal to its golden (plan decision 4)."""
    v = verdict(same, rows_here)
    if v != "allowed":
        return v, []
    if golden is None:
        return "differs", ["no golden file for rpyenv's output; review it, then run with --update-golden"]
    got = normalise(rpyenv_result, root, work)
    problems = [f"rpyenv's {n} differs from its golden: golden {x!r}, got {y!r}"
                for n, x, y in zip(("exit code", "stdout", "stderr", "compared files"), golden, got) if x != y]
    return ("differs", problems) if problems else ("allowed", [])


def golden_files(case):
    base = os.path.join(GOLDEN, slug(case.name))
    names = ["code", "stdout", "stderr"] + (["files"] if case.compare else [])
    return {n: f"{base}.{n}" for n in names}


def read_golden(case):
    paths = golden_files(case)
    if not all(os.path.isfile(p) for p in paths.values()):
        return None
    data = {}
    for n, p in paths.items():
        with open(p, "rb") as f:
            data[n] = f.read()
    return (int(data["code"].decode("ascii")), data["stdout"], data["stderr"], data.get("files", b""))


def write_golden(case, result, root, work):
    os.makedirs(GOLDEN, exist_ok=True)
    code, out, err, files = normalise(result, root, work)
    parts = {"code": str(code).encode("ascii"), "stdout": out, "stderr": err, "files": files}
    for n, p in golden_files(case).items():
        with open(p, "wb") as f:
            f.write(parts[n])


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
    for rel in case.executable:
        os.chmod(resolve(places, rel), 0o755)
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
    # On Windows each run gets a hidden console of its own, in the OEM code page. pyenv.bat
    # runs `chcp 65001` and never restores it, so in a shared console every later run, rpyenv's
    # included, would write for code page 65001 instead (D-48, D-53).
    flags = 0x08000000 if WINDOWS else 0  # CREATE_NO_WINDOW
    p = subprocess.run(cmd + [expand(a) for a in case.args], cwd=work, env=env,
                       capture_output=True, stdin=subprocess.DEVNULL, timeout=120, creationflags=flags)
    files = NEXT_FILE.join(b"<missing>" if x is None else x
                           for x in map(snapshot, (resolve(places, r) for r in case.compare)))
    return (p.returncode, p.stdout, p.stderr, files)


def describe(a, b):
    out = []
    for label, x, y in (("exit code", a[0], b[0]), ("stdout", a[1], b[1]), ("stderr", a[2], b[2])):
        if x != y:
            out.append(f"    {label}: upstream {x!r}\n    {label}: rpyenv   {y!r}")
    if a[3] != b[3]:
        out.append(f"    compared files: upstream {a[3]!r}\n    compared files: rpyenv   {b[3]!r}")
    return "\n".join(out)


def check_inputs(rpyenv, upstream):
    """A one-line message for a bad --rpyenv or --upstream, or None."""
    exe = os.path.join(rpyenv, "pyenv.exe" if WINDOWS else "pyenv")
    if not os.path.isfile(exe):
        return f"--rpyenv: {exe} not found"
    needed = ["bin/pyenv.bat", ".versions_cache.xml", "../.version"] if WINDOWS else ["bin/pyenv"]
    for rel in needed:
        if not os.path.isfile(os.path.join(upstream, *rel.split("/"))):
            return f"--upstream: {os.path.join(upstream, *rel.split('/'))} not found"
    return None


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("--rpyenv", required=True)
    p.add_argument("--upstream", required=True)
    p.add_argument("--only")
    p.add_argument("--update-golden", action="store_true",
                   help="write rpyenv's output as the golden files of the allowed cases")
    a = p.parse_args(argv)
    rpyenv, upstream = os.path.abspath(a.rpyenv), os.path.abspath(a.upstream)
    problem = check_inputs(rpyenv, upstream)
    if problem:
        print(problem, file=sys.stderr)
        return 1
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
        same = results["upstream"] == results["rpyenv"]
        if a.update_golden and verdict(same, rows_here) == "allowed":
            write_golden(case, results["rpyenv"], places["root"], places["work"])
        v, broken = judge(same, rows_here, results["rpyenv"], read_golden(case), places["root"], places["work"])
        counts[v] = counts.get(v, 0) + 1
        print(f"{v:8} {case.name}" + (f"  ({', '.join(rows_here)})" if rows_here else ""))
        if v in ("stale", "differs"):
            detail = describe(results["upstream"], results["rpyenv"])
            if broken:
                detail = "\n".join("    " + b for b in broken) + "\n" + detail
            failures.append(f"{case.name}: {v}" + (f"\n{detail}" if detail else ""))
            if detail:
                print(detail)
    remove_tree(BASE)
    if not counts and not failures:
        failures.append("no case ran" + (f" (no case named {a.only!r} for {OS_NAME})" if a.only else ""))
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
