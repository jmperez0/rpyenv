"""Runs pyenv-win's own pytest suite against rpyenv (spec §12.3, plan decision 3).

  python parity/pyenv_win_run.py --pyenv-win <checkout> --rpyenv <bin dir> [pytest args]

Appends parity/pyenv_win_overlay.py to the checkout's tests/conftest.py (once), then runs
pytest on that tests folder. The checkout is a throwaway copy: it is changed in place.
"""
import argparse
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MARK = "# --- rpyenv overlay"
KEYS = ("PYENV", "PYENV_ROOT", "PYENV_HOME")


def installed_env(base, home):
    """`base` as upstream's CI sets it up (.github/scripts/build.sh): PYENV, PYENV_ROOT and
    PYENV_HOME name an install at `home`, whose bin and shims lead PATH. The suite's `run`
    fixture rewrites those names to each test's root; without them its commands run with no
    pyenv on PATH. A host's own install is dropped from PATH, so it can't stand in."""
    old = {os.path.normcase(os.path.normpath(base[k])) for k in KEYS if base.get(k)}
    keep = [
        p for p in base.get("PATH", "").split(os.pathsep)
        if p and os.path.normcase(os.path.dirname(os.path.normpath(p))) not in old
    ]
    env = dict(base, **{k: home for k in KEYS})
    env["PATH"] = os.pathsep.join([os.path.join(home, "bin"), os.path.join(home, "shims"), *keep])
    return env


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("--pyenv-win", required=True)
    p.add_argument("--rpyenv", required=True)
    a, rest = p.parse_known_args(argv)
    tests = os.path.join(os.path.abspath(a.pyenv_win), "tests")
    conftest = os.path.join(tests, "conftest.py")
    with open(conftest, encoding="utf-8") as f:
        text = f.read()
    if MARK not in text:
        with open(os.path.join(HERE, "pyenv_win_overlay.py"), encoding="utf-8") as f:
            overlay = f.read()
        with open(conftest, "a", encoding="utf-8") as f:
            f.write("\n" + overlay)
    home = os.path.join(os.path.abspath(a.pyenv_win), "pyenv-win")
    env = dict(installed_env(os.environ, home), RPYENV_BIN=os.path.abspath(a.rpyenv), RPYENV_PARITY=HERE)
    cmd = [sys.executable, "-m", "pytest", "-p", "no:cacheprovider", "-q", "-rfEX", "--rootdir", tests, tests, *rest]
    return subprocess.run(cmd, env=env).returncode


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
