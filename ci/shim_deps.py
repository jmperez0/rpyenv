"""CI guard (spec §3): the shim binaries depend on rpyenv-core and platform crates only.

Reads `cargo tree` for each shim, for every target, and fails on any other crate.
A new platform crate needs a deliberate edit to ALLOWED, which a reviewer sees.
"""
import subprocess
import sys

SHIMS = ("pyenv-shim", "pyenv-shimw")
# rpyenv-core's own dependencies are std and platform crates only (spec §3).
ALLOWED = {"rpyenv-core", "libc", "windows-sys", "windows-link"}


def crates(tree: str) -> set:
    """Crate names in `cargo tree --prefix none` output (`name vX.Y.Z ...` per line)."""
    return {line.split()[0] for line in tree.splitlines() if line.strip()}


def unexpected(shim: str, tree: str) -> list:
    return sorted(crates(tree) - ALLOWED - {shim})


def self_test() -> None:
    ok = "pyenv-shim v0.1.0 (/x)\nrpyenv-core v0.1.0 (/y)\nlibc v0.2.189\nlibc v0.2.189 (*)\n"
    assert unexpected("pyenv-shim", ok) == [], unexpected("pyenv-shim", ok)
    bad = ok + "reqwest v0.12.0\nrustls v0.23.0\n"
    assert unexpected("pyenv-shim", bad) == ["reqwest", "rustls"], unexpected("pyenv-shim", bad)


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        self_test()
        print("self-test ok")
        return 0
    failed = False
    for shim in SHIMS:
        proc = subprocess.run(
            ["cargo", "tree", "-p", shim, "-e", "normal", "--prefix", "none", "--target", "all"],
            capture_output=True,
            encoding="utf-8",
        )
        if proc.returncode != 0:
            print(f"cargo tree failed for {shim}:\n{proc.stderr}")
            return 1
        tree = proc.stdout
        extra = unexpected(shim, tree)
        if extra:
            print(f"{shim} depends on crates outside spec §3's allowance: {', '.join(extra)}")
            failed = True
        else:
            print(f"{shim}: ok ({', '.join(sorted(crates(tree)))})")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
