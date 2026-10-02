"""Spec §12.6: the shim's overhead, `python -c pass` through the shim vs directly, timed with
hyperfine and posted in the CI summary. Reports only; it never fails on a number.

  python ci/bench.py --rpyenv target/release --python <python executable> [--runs N]

The python's installation becomes version `bench` of a fresh root (a symlink on Linux, a
junction on Windows), `pyenv rehash` makes its shims, and hyperfine times both commands.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile


def summary(results):
    by = {r["command"]: r for r in results}
    shim, direct = by["through the shim"], by["directly"]
    rows = "".join(
        f"| {r['command']} | {r['mean'] * 1000:.1f} ms | ± {r['stddev'] * 1000:.1f} ms |\n" for r in results
    )
    over = shim["mean"] - direct["mean"]
    return (
        "| Command | Mean | Std dev |\n|---|---|---|\n" + rows
        + f"\nShim overhead: {over * 1000:.1f} ms ({over / direct['mean'] * 100:.1f}%)\n"
    )


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("--rpyenv", required=True)
    p.add_argument("--python", required=True)
    p.add_argument("--runs", type=int, default=40)
    a = p.parse_args(argv)
    windows = os.name == "nt"
    exe = ".exe" if windows else ""
    python = os.path.abspath(a.python)
    prefix = os.path.dirname(python) if windows else os.path.dirname(os.path.dirname(python))
    base = tempfile.mkdtemp(prefix="rpyenv-bench-")
    try:
        root = os.path.join(base, "root")
        os.makedirs(os.path.join(root, "versions"))
        link = os.path.join(root, "versions", "bench")
        if windows:
            subprocess.run(["cmd", "/d", "/c", "mklink", "/J", link, prefix], check=True, capture_output=True)
        else:
            os.symlink(prefix, link)
        with open(os.path.join(root, "version"), "w", encoding="utf-8") as f:
            f.write("bench\n")
        env = dict(os.environ, PYENV_ROOT=root)
        subprocess.run([os.path.join(os.path.abspath(a.rpyenv), "pyenv" + exe), "rehash"], env=env, check=True)
        shim = os.path.join(root, "shims", os.path.basename(python))  # python3 on a distro with no `python`
        out = os.path.join(base, "bench.json")
        subprocess.run(
            ["hyperfine", "-N", "--warmup", "5", "--runs", str(a.runs), "--export-json", out,
             "-n", "through the shim", f'"{shim}" -c pass', "-n", "directly", f'"{python}" -c pass'],
            env=env, check=True,
        )
        with open(out, encoding="utf-8") as f:
            text = summary(json.load(f)["results"])
        print(text)
        target = os.environ.get("GITHUB_STEP_SUMMARY")
        if target:
            with open(target, "a", encoding="utf-8") as f:
                f.write(f"### Shim overhead ({sys.platform})\n\n{text}\n")
    finally:
        shutil.rmtree(base, ignore_errors=True)  # removes the Windows junction without following it
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
