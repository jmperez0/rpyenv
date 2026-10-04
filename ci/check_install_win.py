"""Checks one real `pyenv install` on Windows (spec §12.5 tier 2; plan M2b Task 11).

Usage: python ci/check_install_win.py <PYENV_ROOT> <version code>

Fails (exit 1) unless:
- the version is complete: no marker, no staging leftovers;
- its executables and their copies exist;
- its standard modules import;
- pip runs where the version has ensurepip;
- the `python` shim reaches it;
- a venv works (3.3+).
"""
import os
import re
import subprocess
import sys
import tempfile


def parse(code):
    """(major, minor, free-threaded) of a pyenv-win version code."""
    m = re.match(r"^(\d+)\.(\d+)(?:\.\d+)?(?:(?:a|b|c|rc)\d+)?(t)?(?:-win32|-arm)?$", code)
    if not m:
        raise SystemExit(f"not a version code: {code}")
    return int(m.group(1)), int(m.group(2)), bool(m.group(3))


# Must stay valid on 2.4-2.6 and 3.0+: no `with`, no `except X as e`, no f-strings, one-argument print.
# Pythons without sysconfig (2.6 and older, 3.0-3.1) read as None, which means not free-threaded.
GIL_PROBE = (
    "try:" + chr(10) +
    " import sysconfig; v = sysconfig.get_config_var('Py_GIL_DISABLED')" + chr(10) +
    "except ImportError:" + chr(10) +
    " v = None" + chr(10) +
    "print(v)"
)


def expected_version(code):
    """'X.Y.Z' for a three-part code, 'X.Y' for a two-part one (compare major.minor only)."""
    parse(code)
    m = re.match(r"^(\d+\.\d+(?:\.\d+)?)", code)
    return m.group(1)


def version_matches(code, actual):
    want = expected_version(code)
    return actual == want or (want.count(".") == 1 and actual.startswith(want + "."))


def modules(x, y):
    if x == 2 and y < 6:
        return ["zlib", "Tkinter"]
    if x == 2:
        return ["zlib", "bz2", "ctypes", "ssl", "sqlite3", "Tkinter"]
    mods = ["zlib", "bz2", "ctypes", "ssl", "sqlite3", "tkinter"]
    if (x, y) >= (3, 3):
        mods += ["lzma", "venv"]
    return mods


def copies(x, y):
    return ["python.exe", f"python{x}.exe", f"python{x}{y}.exe", f"python{x}.{y}.exe"]


def run(cmd, env=None):
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, env=env)
    except OSError as e:
        return 127, str(e)
    return p.returncode, (p.stdout + p.stderr).strip()


def main(root, code):
    x, y, ft = parse(code)
    versions = os.path.join(root, "versions")
    prefix = os.path.join(versions, code)
    fails = []
    if not os.path.isdir(prefix):
        raise SystemExit(f"FAIL: {prefix} does not exist")
    if os.path.exists(os.path.join(prefix, ".rpyenv-incomplete")):
        fails.append("the incomplete marker is still there")
    stray = [n for n in os.listdir(versions) if n.startswith((".tmp-", ".old-"))]
    if stray:
        fails.append(f"staging leftovers: {stray}")
    for name in copies(x, y):
        if not os.path.isfile(os.path.join(prefix, name)):
            fails.append(f"missing {name}")
    py = os.path.join(prefix, "python.exe")
    env = dict(os.environ, PYTHONNOUSERSITE="1")
    env.pop("PYTHONHOME", None)
    env.pop("PYTHONPATH", None)
    rc, out = run([py, "-E", "-s", "-c", "import " + ", ".join(modules(x, y))], env)
    if rc:
        fails.append(f"imports failed: {out}")
    rc, out = run([py, "-E", "-s", "-c", "import sys; print('%d.%d.%d' % sys.version_info[:3])"], env)
    if rc or not version_matches(code, out):
        fails.append(f"running version is {out!r} (rc {rc}), expected {expected_version(code)}")
    rc, out = run([py, "-E", "-s", "-c", GIL_PROBE], env)
    if rc or (out == "1") != ft:
        fails.append(f"free-threaded mismatch: Py_GIL_DISABLED={out!r} (rc {rc}), expected {'1' if ft else 'not 1'}")
    outs = {}
    for name in copies(x, y):
        rc, out = run([os.path.join(prefix, name), "-E", "-s", "-c", "import sys; print(sys.version)"], env)
        if rc:
            fails.append(f"{name} did not run (rc {rc}): {out}")
        else:
            outs[name] = out
    if len(set(outs.values())) > 1:
        fails.append(f"copies disagree: {outs}")
    has_pip = (x, y) >= (3, 4) or (x, y) == (2, 7)
    if has_pip:
        rc, out = run([os.path.join(prefix, "Scripts", "pip.exe"), "--version"], env)
        if rc:
            fails.append(f"Scripts\\pip.exe --version failed: {out}")
    shim = os.path.join(root, "shims", "python.exe")
    rc, out = run([shim, "-c", "import sys; print(sys.prefix)"], dict(env, PYENV_VERSION=code, PYENV_ROOT=root, PYENV=root, PYENV_HOME=root))
    if rc or os.path.normcase(os.path.normpath(out)) != os.path.normcase(os.path.normpath(prefix)):
        fails.append(f"the python shim gave rc {rc}: {out}")
    if (x, y) >= (3, 3):
        with tempfile.TemporaryDirectory() as d:
            venv = os.path.join(d, "venv")
            rc, out = run([py, "-E", "-s", "-m", "venv", venv], env)
            if rc:
                fails.append(f"venv failed: {out}")
            else:
                rc, out = run([os.path.join(venv, "Scripts", "python.exe"), "-c", "pass"], env)
                if rc:
                    fails.append(f"the venv's python failed: {out}")
    for f in fails:
        print(f"FAIL: {f}")
    print(f"{code}: {'OK' if not fails else f'{len(fails)} failure(s)'}")
    return 1 if fails else 0


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    sys.exit(main(sys.argv[1], sys.argv[2]))
