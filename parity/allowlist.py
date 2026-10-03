"""The allowlist of intentional differences from upstream (docs/parity/allowlist.md, spec §4),
and the expected-failure files of the upstream suites, whose reasons cite it."""
import os
import re

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ALLOWLIST = os.path.join(REPO, "docs", "parity", "allowlist.md")
ROW = re.compile(r"^\|\s*(D-\d{2})\s*\|\s*(both|Linux|Windows)\s*\|")
MALFORMED = re.compile(r"^\|\s*D-")
# A test of a command rpyenv doesn't implement yet fails for that reason alone; its reason
# cites the milestone that brings the command (spec §14) instead of a row.
MILESTONES = ("M2a", "M2b", "M2", "M3", "M4", "M5", "M6", "M7", "M8", "M9")
# (the tag is the first whole word of a reason, so `M2a` and `M2b` never match `M2`)
# milestones that have shipped; add yours when your plan lands
DELIVERED = ("M1", "M2a")


def rows(path=ALLOWLIST):
    """{"D-01": "both", ...}: each row's ID and the OS it applies to, in file order."""
    out = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            m = ROW.match(line)
            if not m and MALFORMED.match(line):
                raise ValueError(f"{path}: malformed allowlist row: {line.strip()[:60]}")
            if m:
                if m.group(1) in out:
                    raise ValueError(f"{path}: row {m.group(1)} appears twice")
                out[m.group(1)] = m.group(2)
    return out


def tag(reason):
    """The row ID or milestone a reason starts with: `D-35 the lock is …` gives `D-35`."""
    words = reason.split()
    return words[0] if words else ""


def check_reason(reason, os_name, table):
    """None when `reason` starts with a milestone, or with a row of `table` that applies to
    `os_name` ("Linux" or "Windows"); otherwise what is wrong with it."""
    t = tag(reason)
    if t in DELIVERED:
        return (
            f"{t} is delivered: the test should pass now (add its pyenv-<cmd> wrapper in "
            "bats_run.sh / update expected lists), or cite an allowlist row"
        )
    if t in MILESTONES:
        return None
    if t not in table:
        return f"{t!r} is neither an allowlist row nor a milestone"
    if table[t] not in ("both", os_name):
        return f"{t} is a {table[t]} row, not a {os_name} one"
    return None


def read_expected(path, key_fields):
    """An expected-failure file: `<key fields> | <reason>` per line; `#` comments and blank
    lines are skipped. Returns {key: reason}, the key being the first `key_fields` fields
    joined by ` | `."""
    out = {}
    with open(path, encoding="utf-8") as f:
        for n, line in enumerate(f, 1):
            line = line.rstrip("\n")
            if not line.strip() or line.startswith("#"):
                continue
            parts = line.split(" | ")
            if len(parts) != key_fields + 1:
                raise ValueError(f"{path}:{n}: expected {key_fields + 1} fields separated by ' | '")
            key = " | ".join(parts[:key_fields])
            if key in out:
                raise ValueError(f"{path}:{n}: {key!r} is listed twice")
            out[key] = parts[-1]
    return out
