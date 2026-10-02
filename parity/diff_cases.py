"""Differential cases (spec §12.4), run by parity/diff.py against rpyenv and the real upstream
on identical fixture roots. `allow` names the allowlist rows that explain an expected
difference; a row whose OS doesn't match the run is ignored on that OS. A case with no
applicable row must match byte for byte; a case with one must differ."""
from dataclasses import dataclass


@dataclass(frozen=True)
class Case:
    name: str
    args: tuple
    os: str = "both"  # "both", "Linux" or "Windows"
    env: tuple = ()  # (name, value) pairs; value None removes the variable; {v0}/{v1} expand
    files: tuple = ()  # (path, content) pairs; path "root/…" or "work/…"; {v0}/{v1} expand
    remove: tuple = ()  # paths removed after the fixture is built
    readonly: tuple = ()  # paths made read-only after the fixture is built
    compare: tuple = ()  # paths whose bytes are compared after the run
    allow: tuple = ()  # allowlist rows explaining a difference


CASES = (
    Case("--version", ("--version",), allow=("D-01",)),
    Case("root", ("root",), allow=("D-12",)),
    Case("versions", ("versions",)),
    Case("versions --bare", ("versions", "--bare")),
    Case("version", ("version",)),
    Case("version-name", ("version-name",)),
    Case("version-origin", ("version-origin",), allow=("D-12",)),
    Case("version-file", ("version-file",), allow=("D-12",)),
    Case("global", ("global",)),
    Case("local with no local file", ("local",)),
    Case("which python", ("which", "python")),
    Case("whence python", ("whence", "python")),
    Case("prefix", ("prefix",), allow=("D-12",)),
    Case("shims", ("shims",)),
    Case("commands", ("commands",), allow=("D-16", "D-49")),
    Case("help", ("help",), allow=("D-08", "D-49")),
    Case("version with PYENV_VERSION not installed", ("version",), env=(("PYENV_VERSION", "9.9"),)),
    Case("version-name with PYENV_VERSION not installed", ("version-name",), env=(("PYENV_VERSION", "9.9"),)),
    Case("which for a missing command", ("which", "nosuch")),
    Case("exec for a missing command", ("exec", "nosuch")),
    Case("local with a version not installed", ("local", "9.9"), compare=("work/.python-version",)),
    Case("global with a version not installed", ("global", "9.9"), compare=("root/version",)),
    Case("version-file-read of a missing file", ("version-file-read", "missing"), allow=("D-12",)),
    Case("no arguments", (), allow=("D-01", "D-20", "D-49")),
    Case("an unknown command", ("frobnicate",)),
    Case("local writes the version file", ("local", "{v1}"), compare=("work/.python-version",)),
    Case("global writes the version file", ("global", "{v1}"), compare=("root/version",)),
)
