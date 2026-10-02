"""Every allowlist row must be pinned by a test (spec §4, §12). A row counts as covered when
it is cited by a differential case's `allow` (parity/diff_cases.py), by a reason in an upstream
suite's expected-failure file (parity/expected/), in the comment block directly above a Rust
`#[test]` function (`allowlist D-NN`), or in parity/untestable.txt with the reason no test can
pin it. A citation of a row that doesn't exist is an error too.

Covered rows are reported by source: "by execution" (a differential case or an expected
failure, which fails if the row stops being true), "by test citation" (a comment the check
can't verify) and "waived" (untestable.txt). A row with an execution source counts as executed.

  python parity/coverage.py [--report]      (--report lists gaps but exits 0)
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import allowlist  # noqa: E402
import diff_cases  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
ID = re.compile(r"\ballowlist\s+(D-\d{2})\b")
FN = re.compile(r"(pub(\([^)]*\))?\s+)?(async\s+)?fn\s+(\w+)")
EXECUTION, CITATION, WAIVED = "by execution", "by test citation", "waived"


def shown(path):
    """`path` relative to the repository when it is inside it (another drive can't be)."""
    try:
        return os.path.relpath(path, allowlist.REPO)
    except ValueError:
        return path


def rust_test_citations(crates_dir):
    """{row: ["<file>::<fn>", …]} for rows cited in a comment block that is followed, with
    only attributes in between, by a `#[test]` function. Any other line breaks the block."""
    found = {}
    for dirpath, _, names in os.walk(crates_dir):
        for name in sorted(names):
            if not name.endswith(".rs"):
                continue
            path = os.path.join(dirpath, name)
            with open(path, encoding="utf-8") as f:
                lines = f.read().splitlines()
            block, is_test = [], False
            pending = list(reversed(lines))
            while pending:
                s = pending.pop().strip()
                if s.startswith("#[test]") and s != "#[test]":
                    pending.append(s[len("#[test]"):])  # `#[test] fn x() {}` on one line
                    s = "#[test]"
                if s.startswith("//") and not s.startswith("//!"):
                    if is_test:  # a comment after the attributes starts over
                        block, is_test = [], False
                    block.append(s)
                    continue
                if s.startswith("#["):
                    is_test = is_test or s.startswith("#[test]")
                    continue
                m = FN.match(s)
                if m and is_test:
                    where = f"{shown(path)}::{m.group(4)}"
                    for row in sorted(set(ID.findall(" ".join(block)))):
                        found.setdefault(row, []).append(where)
                block, is_test = [], False
    return found


def citations():
    """{row: [(kind, where it is cited), …]} from every source; kind is EXECUTION, CITATION
    or WAIVED."""
    out = {}

    def add(row, kind, where):
        out.setdefault(row, []).append((kind, where))

    for case in diff_cases.CASES:
        for row in case.allow:
            add(row, EXECUTION, f"diff case {case.name!r}")
    for fname, fields in (("bats.txt", 2), ("pyenv-win.txt", 1)):
        path = os.path.join(HERE, "expected", fname)
        for key, reason in allowlist.read_expected(path, fields).items():
            if allowlist.tag(reason).startswith("D-"):
                add(allowlist.tag(reason), EXECUTION, f"expected/{fname}: {key}")
    for row, reason in allowlist.read_expected(os.path.join(HERE, "untestable.txt"), 1).items():
        add(row, WAIVED, f"untestable: {reason}")
    for row, wheres in rust_test_citations(os.path.join(allowlist.REPO, "crates")).items():
        for where in wheres:
            add(row, CITATION, f"test {where}")
    return out


def split(table, cited):
    """{kind: [rows]} for the covered rows of `table`, in table order. Each row counts under
    its strongest source: execution, then a test citation, then a waiver."""
    out = {EXECUTION: [], CITATION: [], WAIVED: []}
    for row in table:
        kinds = {kind for kind, _ in cited.get(row, ())}
        for kind in out:
            if kind in kinds:
                out[kind].append(row)
                break
    return out


def verdict(table, cited, report):
    """(exit code, uncovered rows, dangling citations); report mode always exits 0."""
    dangling = sorted(set(cited) - set(table))
    uncovered = [row for row in table if row not in cited]
    return (0 if report or not (uncovered or dangling) else 1), uncovered, dangling


def main(argv):
    report = "--report" in argv
    table = allowlist.rows()
    cited = citations()
    code, uncovered, dangling = verdict(table, cited, report)
    kinds = split(table, cited)
    counts = ", ".join(f"{len(rows)} {kind}" for kind, rows in kinds.items())
    print(f"allowlist rows: {len(table)}; covered: {len(table) - len(uncovered)} ({counts})")
    for kind in (CITATION, WAIVED):
        if kinds[kind]:
            print(f"{kind}: {' '.join(kinds[kind])}")
    for row in uncovered:
        print(f"uncovered: {row}")
    for row in dangling:
        print(f"cited but not in the allowlist: {row} ({'; '.join(w for _, w in cited[row])})")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(f"### Allowlist coverage\n\n{len(table) - len(uncovered)} of {len(table)} rows "
                    f"covered: {counts}. By test citation (a comment the check can't verify): "
                    f"{', '.join(kinds[CITATION]) or 'none'}. Waived: {', '.join(kinds[WAIVED]) or 'none'}. "
                    f"Uncovered: {', '.join(uncovered) or 'none'}.\n\n")
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
