r"""Checks one real conda install through rpyenv on Windows (M7c design §4; plan M7c Task 10).

Usage: python ci/check_conda_win.py <conda name>     (PYENV_ROOT names the scratch root; PYENV_EXE the pyenv.exe,
                                                       default target\release\pyenv.exe)

Steps; the first one that fails ends the run with `FAIL step N: ...`:
1. `pyenv install <name>` exits 0; python.exe and conda-meta\ exist.
2. shims: python, pip and conda exist; cygpath and curl (Library\bin tools) don't.
3. conda create -n ci-env through the conda shim; the python shim in that env reports its prefix.
4. `pyenv install -f <name>` keeps envs\ci-env.
5. `pyenv uninstall -f <name>` removes the folder.
"""
import os
import subprocess
import sys

INSTALL_TIMEOUT = 20 * 60
CREATE_TIMEOUT = 15 * 60
TIMEOUT = 2 * 60
TAIL = 4000


def prefix_matches(expected, printed):
    r"""True when `printed` names the folder `expected`: case, `/` vs `\` and a trailing separator don't matter."""
    def norm(p):
        return p.strip().replace("/", "\\").rstrip("\\").lower()
    return norm(expected) == norm(printed) and norm(expected) != ""


def tail(text, n=TAIL):
    return text if len(text) <= n else "..." + text[-n:]


class Failed(Exception):
    pass


def run(cmd, env, timeout=TIMEOUT):
    """(rc, stdout, stderr); a timeout reads as rc 124."""
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, env=env, timeout=timeout)
    except subprocess.TimeoutExpired as e:
        out = e.stdout.decode("utf-8", "replace") if isinstance(e.stdout, bytes) else (e.stdout or "")
        err = e.stderr.decode("utf-8", "replace") if isinstance(e.stderr, bytes) else (e.stderr or "")
        return 124, out, err + f"\n(timed out after {timeout}s)"
    except OSError as e:
        return 127, "", str(e)
    return p.returncode, p.stdout, p.stderr


def must(root, name, step, cmd, env, timeout=TIMEOUT):
    rc, out, err = run(cmd, env, timeout)
    if rc:
        print(f"command: {cmd}\nexit code: {rc}\n--- stdout tail ---\n{tail(out)}\n--- stderr tail ---\n{tail(err)}")
        log = os.path.join(root, "install_cache", name + ".log")
        if os.path.isfile(log):
            with open(log, encoding="utf-8", errors="replace") as f:
                print(f"--- tail of {log} ---\n{tail(f.read())}")
        raise Failed(f"step {step}: {cmd[-2:]} exited {rc}")
    return out


def check(root, name, pyenv):
    shims = os.path.join(root, "shims")
    prefix = os.path.join(root, "versions", name)
    env = dict(os.environ, PYENV_ROOT=root, PYENV=root, PYENV_HOME=root)
    for k in ("PYENV_VERSION", "PYTHONHOME", "PYTHONPATH", "CONDA_PREFIX", "CONDA_DEFAULT_ENV"):
        env.pop(k, None)
    # Miniconda 25+ asks to accept Anaconda's terms of service before the defaults channel works. rpyenv sets
    # this only while installing (as upstream does); a user accepts once themselves, CI does it here (R21).
    # Miniforge uses conda-forge and has no such prompt.
    tos = name.startswith("miniconda")
    if tos:
        env["CONDA_PLUGINS_AUTO_ACCEPT_TOS"] = "true"
    install_env = {k: v for k, v in env.items() if k != "CONDA_PLUGINS_AUTO_ACCEPT_TOS"} if tos else env

    # 1
    must(root, name, 1, [pyenv, "install", name], install_env, INSTALL_TIMEOUT)
    for rel in ("python.exe", "conda-meta"):
        if not os.path.exists(os.path.join(prefix, rel)):
            raise Failed(f"step 1: {rel} missing under {prefix}")
    # 2
    for exe in ("python.exe", "pip.exe", "conda.exe"):
        if not os.path.isfile(os.path.join(shims, exe)):
            raise Failed(rf"step 2: shims\{exe} missing")
    for exe in ("cygpath.exe", "curl.exe"):
        if os.path.exists(os.path.join(shims, exe)):
            raise Failed(rf"step 2: shims\{exe} exists (a Library\bin tool must not get a shim)")
    # 3
    env["PYENV_VERSION"] = name
    must(root, name, 3, [os.path.join(shims, "conda.exe"), "create", "--yes", "-n", "ci-env", "python"], env, CREATE_TIMEOUT)
    env["PYENV_VERSION"] = name + "/envs/ci-env"
    printed = must(root, name, 3, [os.path.join(shims, "python.exe"), "-c", "import sys, ssl; print(sys.prefix)"], env)
    expected = os.path.join(prefix, "envs", "ci-env")
    if not prefix_matches(expected, printed):
        raise Failed(f"step 3: sys.prefix is {printed.strip()!r}, expected {expected}")
    # 4
    del env["PYENV_VERSION"]
    must(root, name, 4, [pyenv, "install", "-f", name], env, INSTALL_TIMEOUT)
    if not os.path.isfile(os.path.join(prefix, "envs", "ci-env", "python.exe")):
        raise Failed(r"step 4: install -f lost envs\ci-env")
    # 5
    must(root, name, 5, [pyenv, "uninstall", "-f", name], env)
    if os.path.exists(prefix):
        raise Failed(f"step 5: {prefix} is still there")


def main(name):
    root = os.environ.get("PYENV_ROOT")
    if not root:
        raise SystemExit("FAIL: PYENV_ROOT is not set")
    pyenv = os.path.abspath(os.environ.get("PYENV_EXE", os.path.join("target", "release", "pyenv.exe")))
    try:
        check(root, name, pyenv)
    except Failed as e:
        print(f"FAIL {e}")
        return 1
    print(f"{name}: OK")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    sys.exit(main(sys.argv[1]))
