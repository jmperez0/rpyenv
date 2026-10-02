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
    env = dict(os.environ, RPYENV_BIN=os.path.abspath(a.rpyenv), RPYENV_PARITY=HERE)
    cmd = [sys.executable, "-m", "pytest", "-p", "no:cacheprovider", "-q", "-rfEX", "--rootdir", tests, tests, *rest]
    return subprocess.run(cmd, env=env).returncode


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
