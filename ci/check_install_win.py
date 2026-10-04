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
    p = subprocess.run(cmd, capture_output=True, text=True, env=env)
    return p.returncode, (p.stdout + p.stderr).strip()


def main(root, code):
    x, y, _ft = parse(code)
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
    has_pip = (x, y) >= (3, 4) or (x == 2 and os.path.isdir(os.path.join(prefix, "Lib", "ensurepip")))
    if has_pip:
        rc, out = run([os.path.join(prefix, "Scripts", "pip.exe"), "--version"], env)
        if rc:
            fails.append(f"Scripts\\pip.exe --version failed: {out}")
    shim = os.path.join(root, "shims", "python.exe")
    rc, out = run([shim, "-c", "import sys; print(sys.prefix)"], dict(env, PYENV_VERSION=code))
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
