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
# --suite NAME: (expected-failure list, files list, summary heading), all under parity/expected/.
SUITES = {
    "pyenv": ("bats.txt", "bats-files.txt", "Upstream pyenv bats suite"),
    "virtualenv": ("bats-virtualenv.txt", "bats-virtualenv-files.txt",
                   "Upstream pyenv-virtualenv bats suite"),
}
PLAN = re.compile(r"^([^\t]+)\t1\.\.(\d+)$")
LINE = re.compile(r"^([^\t]+)\t(ok|not ok) \d+ (.*?)(?: # skip.*)?$")
SKIP = re.compile(r"^([^\t]+)\tok \d+ (.*?) # skip\b")


def parse_tap(text):
    """{(file, test name): passed}; a skipped test counts as passed."""
    out = {}
    for line in text.splitlines():
        m = LINE.match(line)
        if m:
            out[(m.group(1), m.group(3))] = m.group(2) == "ok"
    return out


def skipped(text):
    """[(file, test name)] of the tests that reported `ok N name # skip ...`, in log order.
    They count as passes, but how many there are must be visible: a skip for a missing
    locale or tool means the test did not run."""
    out = []
    for line in text.splitlines():
        m = SKIP.match(line)
        if m:
            out.append((m.group(1), m.group(2)))
    return out


def read_files(path):
    """The expected file names: one per line, `#` comments and blank lines skipped."""
    with open(path, encoding="utf-8") as f:
        return [l.strip() for l in f if l.strip() and not l.startswith("#")]


def structure_problems(text, files, files_name="bats-files.txt"):
    """A file that vanished from the log, one nobody listed, a file with no plan, or a plan
    that doesn't match the results counted (a crashed or truncated run) are all problems."""
    plans, counts = {}, {}
    for line in text.splitlines():
        m = PLAN.match(line)
        if m:
            plans[m.group(1)] = int(m.group(2))
        m = LINE.match(line)
        if m:
            counts[m.group(1)] = counts.get(m.group(1), 0) + 1
    seen = set(plans) | set(counts)
    out = []
    missing = sorted(set(files) - seen)
    extra = sorted(seen - set(files))
    if missing:
        out.append("files missing from the log: " + ", ".join(missing))
    if extra:
        out.append(f"files in the log that are not listed in {files_name}: " + ", ".join(extra))
    for f in sorted(seen):
        if f not in plans:
            out.append(f"{f}: no `1..N` plan line")
        elif plans[f] != counts.get(f, 0):
            out.append(f"{f}: plan says {plans[f]} tests, the log has {counts.get(f, 0)}")
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
    suite = "pyenv"
    if argv[:1] == ["--suite"]:
        suite, argv = argv[1], argv[2:]
    if suite not in SUITES:
        print(f"bats_check.py: unknown suite {suite!r}; use one of: {', '.join(SUITES)}", file=sys.stderr)
        return 2
    exp_name, files_name, heading = SUITES[suite]
    with open(argv[0], encoding="utf-8") as f:
        text = f.read()
    results = parse_tap(text)
    expected = allowlist.read_expected(os.path.join(HERE, "expected", exp_name), 2)
    found = structure_problems(text, read_files(os.path.join(HERE, "expected", files_name)), files_name)
    found += problems(results, expected, allowlist.rows())
    failed = sum(1 for ok in results.values() if not ok)
    skips = skipped(text)
    label = "bats" if suite == "pyenv" else f"bats ({suite})"
    line = (
        f"{label}: {len(results)} tests, {failed} failing, {len(expected)} expected to fail, "
        f"{len(found)} problems, {len(skips)} skipped"
    )
    print(line)
    for f, name in skips:
        print(f"  skipped: {f} | {name}")
    for p in found:
        print("FAIL " + p)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(
                f"### {heading}\n\n{line}\n\n"
                + "".join(f"- {p}\n" for p in found)
                + "".join(f"- skipped: {sf} | {name}\n" for sf, name in skips)
                + "\n"
            )
    return 1 if found or not results else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
