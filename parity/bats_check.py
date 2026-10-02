"""Checks a bats TAP log from parity/bats_run.sh against parity/expected/bats.txt (plan
decision 3): every failing test must be listed, every listed test must exist and fail, and
every reason must cite a Linux (or both) row, or a milestone.

  python3 parity/bats_check.py <tap-file>
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import allowlist  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
LINE = re.compile(r"^([^\t]+)\t(ok|not ok) \d+ (.*?)(?: # skip.*)?$")


def parse_tap(text):
    """{(file, test name): passed}; a skipped test counts as passed."""
    out = {}
    for line in text.splitlines():
        m = LINE.match(line)
        if m:
            out[(m.group(1), m.group(3))] = m.group(2) == "ok"
    return out


def problems(results, expected, table):
    out = []
    for key, reason in sorted(expected.items()):
        bad = allowlist.check_reason(reason, "Linux", table)
        if bad:
            out.append(f"{key}: {bad}")
            continue
        f, _, name = key.partition(" | ")
        if (f, name) not in results:
            out.append(f"{key}: listed, but there is no such test")
        elif results[(f, name)]:
            out.append(f"{key}: listed, but it passed (remove the entry)")
    for (f, name), passed in sorted(results.items()):
        if not passed and f"{f} | {name}" not in expected:
            out.append(f"{f} | {name}: failed, and it is not listed")
    return out


def main(argv):
    with open(argv[0], encoding="utf-8") as f:
        results = parse_tap(f.read())
    expected = allowlist.read_expected(os.path.join(HERE, "expected", "bats.txt"), 2)
    found = problems(results, expected, allowlist.rows())
    failed = sum(1 for ok in results.values() if not ok)
    line = f"bats: {len(results)} tests, {failed} failing, {len(expected)} expected to fail, {len(found)} problems"
    print(line)
    for p in found:
        print("FAIL " + p)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(f"### Upstream pyenv bats suite\n\n{line}\n\n" + "".join(f"- {p}\n" for p in found) + "\n")
    return 1 if found or not results else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
