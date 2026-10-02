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


CASES = ()
