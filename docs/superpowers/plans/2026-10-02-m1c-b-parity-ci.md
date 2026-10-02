# M1c-b: Parity CI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** CI proves rpyenv matches upstream:
- Upstream pyenv's bats suite and pyenv-win's pytest suite run against rpyenv. Every expected failure is listed with the allowlist row or milestone that explains it.
- Differential tests run the same commands against real pyenv and real pyenv-win and compare the bytes.
- A coverage check makes sure every allowlist row is pinned by some test.
- The shim's overhead is timed and posted in the CI summary.

**Architecture:**
- **Shared tooling:** a flat `parity/` folder of standard-library Python modules.
  - `allowlist.py` parses `docs/parity/allowlist.md` and the expected-failure files.
  - `coverage.py` derives which rows are pinned, from the differential cases, the expected-failure files and comments above Rust `#[test]` functions.
- **The suites:**
  - `diff.py` runs `diff_cases.py`.
  - `bats_run.sh` with `bats_check.py` runs and checks upstream's bats suite, inside a Debian container.
  - `pyenv_win_run.py` appends `pyenv_win_overlay.py` to pyenv-win's own `conftest.py`.
- **CI:** a new `.github/workflows/parity.yml` runs all of it. `ci/bench.py` times the shim with hyperfine.

**Tech Stack:**
- Python 3.10+, standard library only. The one exception is pyenv-win's own test dependencies (pytest 9.1.1, tempenv 2.1.0, packaging 26.3), installed only in the Windows parity job.
- bats 1.11.1, Docker (`debian:trixie-slim`), hyperfine, GitHub Actions.

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md`:
- §4 "Parity policy" (read it first)
- §12 Testing, items 3 (upstream suites), 4 (differential testing) and 6 (performance)
- §14: "Differential testing starts in M1 for the commands M1 delivers"

Allowlist: `docs/parity/allowlist.md`. References: `docs/parity/pyenv-m1-reference.md`, `docs/parity/pyenv-win-m1-reference.md`.

**Where this fits:** M1a, M1b, M1b-win and M1c-a are on `main` (da6a5fb). This plan completes M1: "parity suites wired into CI". M2 (installer) comes next.

## Decisions this plan makes

1. **Upstream versions are pinned.** All four are the versions the M1 references were written against:
   - pyenv `ab74141ab4bcd34aa9b11f363dd8ad4f6fc01cd2` (2.8.6), fetched as a GitHub tarball. With no `.git`, `pyenv --version` prints `pyenv 2.8.6`, as in the reference.
   - pyenv-win `856ed5a8c107879d53374f782a4b40cc794f19e0` (3.1.1 + 232 commits), fetched with `git clone`. The Windows runners' git converts line endings to CRLF on checkout, as in the reference's checkout, and pyenv-win's batch files rely on that.
   - bats `v1.11.1`, from a tarball.
   - the container image `debian:trixie-slim`.
2. **The bats suite runs in a `debian:trixie-slim` container, as an unprivileged user, against root-owned copies of rpyenv's binaries.**
   - The container matches the reference's probe environment (Debian trixie, GNU tools, `gawk`).
   - GitHub's Ubuntu runners have `pwsh`, which would turn four skipped tests into failures; the container doesn't.
   - The suite writes into shims. With M1c-a's templates that is safe, but root-owned binaries make sure that if a regression ever writes through to the binary, the test fails instead of corrupting it.
3. **Expected failures are a ratchet. These all fail the job:**
   - a test that fails without being listed;
   - a test that is listed but passes;
   - a listed test that doesn't exist;
   - a reason that doesn't cite a valid row or milestone.

   Each line's reason starts with the allowlist row (`D-NN`) or the milestone (`M2`…`M9`) that explains it. A milestone means "rpyenv doesn't have this command yet". When a milestone delivers the command, its tests start passing and the list has to shrink.
4. **The differential tests compare bytes: exit code, stdout, stderr, and any files the case names.**
   - A case's `allow` names the rows that explain a difference.
   - A row whose OS doesn't match the run is ignored on that OS, so one case can expect a difference on Windows (`D-12`) and a match on Linux.
   - A case with no applicable row must match. A case with one must differ; otherwise the row is stale for that case.
   - Fixtures live under `<repo>/target/parity-diff`, not the system temp folder. On Windows runners the temp path contains the 8.3 short name `RUNNER~1`, which pyenv-win prints in long form (D-31).
5. **Allowlist coverage is derived, not hand-listed.** A row is covered when one of these cites it:
   - a differential case's `allow`;
   - an expected-failure reason;
   - the comment block directly above a Rust `#[test]` function, with `allowlist D-NN` in the text;
   - `parity/untestable.txt`, with the reason no test can pin it.

   `coverage.py` scans for these. It also fails on a citation of a row that doesn't exist. Planning probe on main: only 2 of 48 rows were cited above a test (method: `C:\tmp\m1cb_cov_proto.py`).
6. **New allowlist rows**, for differences the suites measured that no row explains yet:
   - D-49: the Linux command list.
   - D-50: `sort` without `-V`.
   - D-51: bash hooks.
   - D-52: internal commands resolved in-process.
7. **CI runs in a new workflow, `parity.yml`.** `ci.yml` stays the fast build/lint/test gate, and parity jobs can fail on their own without hiding a build failure. Its first run is the PR that delivers this plan. Nothing here can run on CI before that, so each task's controller check reproduces its job's commands locally: WSL for Linux, this host for Windows.
8. **Not included:**
   - The shim and `pyenv exec` disagree when `PATHEXT` is unset. This follows pyenv-win, where `GetExtensions` versus cmd's defaults behave the same way, and real users always have `PATHEXT`.
   - The live watcher and installer tests (M2, M5).

## Measured baseline (planning probes on main da6a5fb, 2026-10-02)

- **bats.** Every upstream `.bats` file at ab74141, run with bats 1.11.1 in WSL Debian (forky/sid, bash 5.3) against root-owned copies of rpyenv's binaries:
  - 156 ok (8 of them skipped because fish/pwsh are absent), 110 not ok.
  - The 110 are classified below, and 0 were left unclassified. The method is a rule table in `C:\tmp\m1cb_expected.py`. The classification is by test name and file; Task 3 checks each against the test body.
  - M1c-a moved 5 `rehash.bats` repair tests from failing to passing, and broke none.
- **pyenv-win pytest.** Suite at 856ed5a, with the overlay from Task 4, against rpyenv's debug build:
  - AMD64: 45 failed. X86: 42 failed, all of them also in the AMD64 set; the 3 `test_patched_venv_module` cases are skipped on X86.
  - Classified below.
- **Differential prototype.** 25 commands run against each upstream (`C:\tmp\m1cb_diff_proto.py`):
  - Windows (pyenv-win): 16 the same. The 9 that differ are all explained by D-01, D-08, D-12, D-16 and D-20.
  - Linux (pyenv ab74141, run from a git clone, which is why `--version` showed a `-18-gab74141a` suffix): 21 the same. The 4 that differ are `--version` (D-01), and `commands`, `help` and no-arguments, all three of which list commands rpyenv lacks (new row D-49).

## Global Constraints

- **Python:**
  - `parity/` and `ci/` scripts use Python 3.10+ and the standard library only.
  - Every `open()` passes `encoding="utf-8"`, and text files written by tests pass `newline=""` when the bytes matter.
  - Unit tests use `unittest`, live in `parity/test_*.py`, and run with `python3 -m unittest discover -s parity -p "test_*.py"`.
- **Pins:** the exact SHAs and versions in Decision 1. Actions are pinned by SHA (resolved 2026-10-02 with `gh api repos/<owner>/<repo>/commits/<ref> --jq .sha`):
  - `actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5`
  - `dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02`, with `toolchain: "1.96"`
  - `actions/setup-python@ece7cb06caefa5fff74198d8649806c4678c61a1 # v6`
  - `taiki-e/install-action@83ac0ad63c0167e6f06796fab0fce28db1bf3db0 # v2`
- **Parity policy** (spec §4): every intentional difference gets a row in `docs/parity/allowlist.md`. Rows follow the table's existing format: `| D-NN | OS | Command | Upstream | rpyenv | Reason |`. OS is `both`, `Linux` or `Windows`.
- **Rust toolchain:** Rust `1.96`, edition 2021. The existing `ci.yml` gate is unchanged. For any Rust change, run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo build --workspace` and `cargo test --workspace`.
- **Commits:** one-line `git commit -m "..."` messages, with no attribution or trailer lines.
- **WSL:** never pass `$VARS` inside `wsl … -- bash -c "…"`, because the default shell expands them first. Run WSL work from a script file with `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/<script>.sh`, from PowerShell.
- **Build first:** `cargo test -p rpyenv-e2e` alone does not rebuild the shims. Run `cargo build --workspace` before any run that uses the binaries.

## Review Focus

These are the five failure modes most likely to make the parity CI lie to a maintainer. Each has a test in the task named in brackets.

1. **An upstream test renamed or removed, or an rpyenv change that makes a listed test pass.** The job must fail ("listed but passed" / "listed but not found") rather than silently shrink coverage. [Task 3: `test_bats_check.StaleEntries`; Task 4: `test_pyenv_win_overlay`]
2. **A differential case whose allowed difference disappears.** It must fail as stale, so the allowlist can't accumulate dead rows. [Task 2: `test_diff.Verdicts`]
3. **A reason citing a Linux row for a Windows test, or a row that doesn't exist.** It must be rejected. [Task 1: `test_allowlist.Reasons`]
4. **A Rust test's citation comment separated from its `#[test]`,** for example by a blank line or another function. It must not count as coverage. [Task 1: `test_coverage.RustCitations`]
5. **A runner where pyenv-win can't run** (VBScript removed). The differential job must stop with a clear message, not report every case as a difference. [Task 5: the "VBScript is available" step]

---

### Task 1: The allowlist module, the coverage check, and four new rows

**Files:**
- Create: `parity/allowlist.py`, `parity/coverage.py`, `parity/untestable.txt`, `parity/test_allowlist.py`, `parity/test_coverage.py`
- Create (empty starting versions that Tasks 2–4 fill): `parity/diff_cases.py`, `parity/expected/bats.txt`, `parity/expected/pyenv-win.txt`
- Modify: `docs/parity/allowlist.md` (add D-49 to D-52)

**Interfaces:**
- Produces:
  - `allowlist.REPO: str`
  - `allowlist.rows(path=ALLOWLIST) -> dict[str, str]`
  - `allowlist.MILESTONES: tuple`
  - `allowlist.tag(reason) -> str`
  - `allowlist.check_reason(reason, os_name, table) -> str | None`
  - `allowlist.read_expected(path, key_fields) -> dict[str, str]`
  - `coverage.rust_test_citations(crates_dir) -> dict[str, list[str]]`
  - `coverage.citations() -> dict[str, list[str]]`
  - `coverage.main(argv) -> int`
  - `diff_cases.Case` (a frozen dataclass) and `diff_cases.CASES: tuple`
  - File formats:
    - `parity/expected/bats.txt`: `<file> | <test name> | <reason>`
    - `parity/expected/pyenv-win.txt`: `<test id> | <reason>`
    - `parity/untestable.txt`: `<row> | <reason>`

- [ ] **Step 1: Write the failing unit tests**

`parity/test_allowlist.py`:

```python
import os
import tempfile
import unittest

import allowlist

SAMPLE = """# Intentional differences

| ID | OS | Command | Upstream | rpyenv | Reason |
|---|---|---|---|---|---|
| D-01 | both | `--version` | a | b | c |
| D-12 | Windows | `root` | a | b | c |
| D-35 | Linux | `rehash` | a | b | c |
"""


def write(text):
    f = tempfile.NamedTemporaryFile("w", encoding="utf-8", suffix=".md", delete=False)
    f.write(text)
    f.close()
    return f.name


class Rows(unittest.TestCase):
    def test_reads_ids_and_os(self):
        self.assertEqual(
            allowlist.rows(write(SAMPLE)),
            {"D-01": "both", "D-12": "Windows", "D-35": "Linux"},
        )

    def test_a_duplicate_row_is_an_error(self):
        with self.assertRaises(ValueError):
            allowlist.rows(write(SAMPLE + "| D-12 | Linux | x | a | b | c |\n"))

    def test_the_real_allowlist_parses(self):
        table = allowlist.rows()
        self.assertIn("D-01", table)
        self.assertTrue(all(v in ("both", "Linux", "Windows") for v in table.values()))


class Reasons(unittest.TestCase):
    """Review focus 3."""

    table = {"D-01": "both", "D-12": "Windows", "D-35": "Linux"}

    def test_a_row_for_this_os_or_both_is_accepted(self):
        self.assertIsNone(allowlist.check_reason("D-35 the lock", "Linux", self.table))
        self.assertIsNone(allowlist.check_reason("D-01 fixed line", "Windows", self.table))

    def test_a_milestone_is_accepted(self):
        self.assertIsNone(allowlist.check_reason("M3 shell integration", "Linux", self.table))

    def test_a_row_for_the_other_os_is_rejected(self):
        self.assertIn("Linux row", allowlist.check_reason("D-35 x", "Windows", self.table))

    def test_an_unknown_row_or_word_is_rejected(self):
        self.assertIsNotNone(allowlist.check_reason("D-99 x", "Linux", self.table))
        self.assertIsNotNone(allowlist.check_reason("because", "Linux", self.table))
        self.assertIsNotNone(allowlist.check_reason("", "Linux", self.table))


class Expected(unittest.TestCase):
    def test_reads_keys_and_reasons_and_skips_comments(self):
        path = write("# c\n\na.bats | t 1 | D-01 x\nb.bats | t 2 | M3 y\n")
        self.assertEqual(
            allowlist.read_expected(path, 2),
            {"a.bats | t 1": "D-01 x", "b.bats | t 2": "M3 y"},
        )

    def test_a_wrong_field_count_or_a_duplicate_is_an_error(self):
        with self.assertRaises(ValueError):
            allowlist.read_expected(write("a.bats | D-01 x\n"), 2)
        with self.assertRaises(ValueError):
            allowlist.read_expected(write("k | D-01 x\nk | D-01 y\n"), 1)


if __name__ == "__main__":
    unittest.main()
```

`parity/test_coverage.py`:

```python
import os
import tempfile
import textwrap
import unittest

import coverage


def crate(files):
    d = tempfile.mkdtemp()
    for rel, text in files.items():
        p = os.path.join(d, rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w", encoding="utf-8") as f:
            f.write(textwrap.dedent(text))
    return d


class RustCitations(unittest.TestCase):
    """Review focus 4: only the comment block directly above a `#[test]` fn counts."""

    def test_doc_block_above_a_test_counts_with_attributes_between(self):
        d = crate({"a/tests/t.rs": """
            /// Something (allowlist D-07).
            #[cfg(windows)]
            #[test]
            fn sorted() {}
        """})
        self.assertEqual(list(coverage.rust_test_citations(d)), ["D-07"])

    def test_a_blank_line_or_a_non_test_fn_breaks_the_link(self):
        d = crate({"a/src/x.rs": """
            // allowlist D-07
            fn helper() {}

            /// allowlist D-08

            #[test]
            fn t() {}
        """})
        self.assertEqual(coverage.rust_test_citations(d), {})

    def test_the_test_is_named_in_the_result(self):
        d = crate({"a/tests/t.rs": "// allowlist D-09\n#[test]\nfn pins_it() {}\n"})
        (where,) = coverage.rust_test_citations(d)["D-09"]
        self.assertTrue(where.endswith("::pins_it"), where)


class Main(unittest.TestCase):
    def test_report_mode_never_fails(self):
        self.assertEqual(coverage.main(["--report"]), 0)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run them to see them fail**

Run: `python -m unittest discover -s parity -p "test_*.py"` (on Windows; use `python3` in WSL).
Expected: errors, `No module named 'allowlist'` and `No module named 'coverage'`.

- [ ] **Step 3: Implement**

`parity/allowlist.py`:

```python
"""The allowlist of intentional differences from upstream (docs/parity/allowlist.md, spec §4),
and the expected-failure files of the upstream suites, whose reasons cite it."""
import os
import re

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ALLOWLIST = os.path.join(REPO, "docs", "parity", "allowlist.md")
ROW = re.compile(r"^\|\s*(D-\d{2})\s*\|\s*(both|Linux|Windows)\s*\|")
# A test of a command rpyenv doesn't implement yet fails for that reason alone; its reason
# cites the milestone that brings the command (spec §14) instead of a row.
MILESTONES = ("M2", "M3", "M4", "M5", "M6", "M7", "M8", "M9")


def rows(path=ALLOWLIST):
    """{"D-01": "both", ...}: each row's ID and the OS it applies to, in file order."""
    out = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            m = ROW.match(line)
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
```

`parity/diff_cases.py`, starting version (Task 2 adds the cases):

```python
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
```

`parity/expected/bats.txt` and `parity/expected/pyenv-win.txt`, starting versions:

```
# Expected failures of the upstream suite against rpyenv (plan decision 3).
```

The bats file's first line names "upstream pyenv's bats suite", and the pyenv-win file's names "pyenv-win's pytest suite". Formats are in Task 3 and Task 4.

`parity/untestable.txt`:

```
# Allowlist rows no automated test can pin, each with the reason: `D-NN | <reason>`.
```

`parity/coverage.py`:

```python
"""Every allowlist row must be pinned by a test (spec §4, §12). A row counts as covered when
it is cited by a differential case's `allow` (parity/diff_cases.py), by a reason in an upstream
suite's expected-failure file (parity/expected/), in the comment block directly above a Rust
`#[test]` function (`allowlist D-NN`), or in parity/untestable.txt with the reason no test can
pin it. A citation of a row that doesn't exist is an error too.

  python parity/coverage.py [--report]      (--report lists gaps but exits 0)
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import allowlist  # noqa: E402
import diff_cases  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
ID = re.compile(r"\bD-\d{2}\b")
FN = re.compile(r"(pub(\([^)]*\))?\s+)?(async\s+)?fn\s+(\w+)")


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
            for line in lines:
                s = line.strip()
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
    """{row: [where it is cited, …]} from every source."""
    out = {}

    def add(row, where):
        out.setdefault(row, []).append(where)

    for case in diff_cases.CASES:
        for row in case.allow:
            add(row, f"diff case {case.name!r}")
    for fname, fields in (("bats.txt", 2), ("pyenv-win.txt", 1)):
        path = os.path.join(HERE, "expected", fname)
        for key, reason in allowlist.read_expected(path, fields).items():
            if allowlist.tag(reason).startswith("D-"):
                add(allowlist.tag(reason), f"expected/{fname}: {key}")
    for row, reason in allowlist.read_expected(os.path.join(HERE, "untestable.txt"), 1).items():
        add(row, f"untestable: {reason}")
    for row, wheres in rust_test_citations(os.path.join(allowlist.REPO, "crates")).items():
        for where in wheres:
            add(row, f"test {where}")
    return out


def main(argv):
    report = "--report" in argv
    table = allowlist.rows()
    cited = citations()
    dangling = sorted(set(cited) - set(table))
    uncovered = [row for row in table if row not in cited]
    print(f"allowlist rows: {len(table)}; covered: {len(table) - len(uncovered)}")
    for row in uncovered:
        print(f"uncovered: {row}")
    for row in dangling:
        print(f"cited but not in the allowlist: {row} ({'; '.join(cited[row])})")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(f"### Allowlist coverage\n\n{len(table) - len(uncovered)} of {len(table)} rows "
                    f"covered. Uncovered: {', '.join(uncovered) or 'none'}.\n\n")
    if report:
        return 0
    return 1 if uncovered or dangling else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

Add the four rows to `docs/parity/allowlist.md`, after D-48:

```markdown
| D-49 | Linux | `commands`, `help`, `pyenv` with no arguments | Lists every `pyenv-*` command in `libexec` and the plugin folders, including ones rpyenv doesn't implement yet | Lists the commands rpyenv implements; the list grows with each milestone (spec §14) | A different command set (the Linux counterpart of D-16). |
| D-50 | Linux | `versions` | When `sort` lacks `-V`, versions are listed in plain lexical order (`1.218.0` before `1.9.0`) | Always version order | rpyenv doesn't use `sort`; the fallback order is an artifact of the shell implementation. |
| D-51 | Linux | `exec`, `rehash`, `version-name`, `version-origin`, `which`; `hooks` | Runs bash hook scripts from `PYENV_HOOK_PATH` and the `pyenv.d` folders, and has a `pyenv hooks` command that lists them | No bash hooks, and no `pyenv hooks` command | Spec D4: "No bash hooks". |
| D-52 | Linux | `prefix`, `which`, shims | Commands call one another through `PATH` (`pyenv-which`, `pyenv-version-name`), so a plugin or a test can replace one; a shim passes `_PYENV_SHIM_PATH` to `pyenv exec` | Commands are resolved in-process, so replacing `pyenv-<cmd>` on `PATH` has no effect on other commands; shims don't use `_PYENV_SHIM_PATH` (D-30 covers a shim linked from elsewhere) | One binary with no internal subprocesses (spec D5). |
```

- [ ] **Step 4: Run the tests and the report**

Run: `python -m unittest discover -s parity -p "test_*.py"`, then `python parity/coverage.py --report`.

Expected:
- All unit tests pass.
- The report prints `allowlist rows: 52; covered: 2` (the two rows already cited above a test), lists 50 uncovered rows, and exits 0.
- Copy the uncovered list into the report. Task 6 starts from it.

- [ ] **Step 5: Commit**

```bash
git add parity docs/parity/allowlist.md
git commit -m "Add the parity allowlist module, the coverage check and rows D-49 to D-52"
```

---

### Task 2: The differential harness and its measured cases

**Files:**
- Create: `parity/diff.py`, `parity/test_diff.py`
- Modify: `parity/diff_cases.py` (the cases)

**Interfaces:**
- Consumes:
  - `allowlist.rows`
  - `diff_cases.Case`, `diff_cases.CASES` (Task 1)
- Produces:
  - `diff.verdict(same: bool, rows_here: list) -> str`, one of `"same"`, `"allowed"`, `"stale"`, `"differs"`
  - `diff.applicable(case, table, os_name) -> list[str]`
  - `diff.main(argv) -> int`
  - Command line: `python parity/diff.py --rpyenv <dir> --upstream <dir> [--only NAME]`

- [ ] **Step 1: Write the failing unit tests**

`parity/test_diff.py`:

```python
import unittest

import allowlist
import diff
import diff_cases


class Verdicts(unittest.TestCase):
    """Review focus 2."""

    def test_the_four_outcomes(self):
        self.assertEqual(diff.verdict(True, []), "same")
        self.assertEqual(diff.verdict(False, ["D-12"]), "allowed")
        self.assertEqual(diff.verdict(True, ["D-12"]), "stale")
        self.assertEqual(diff.verdict(False, []), "differs")


class Applicable(unittest.TestCase):
    def test_rows_for_the_other_os_are_ignored(self):
        table = {"D-01": "both", "D-12": "Windows", "D-49": "Linux"}
        case = diff_cases.Case("x", ("commands",), allow=("D-12", "D-49", "D-01"))
        self.assertEqual(diff.applicable(case, table, "Linux"), ["D-49", "D-01"])
        self.assertEqual(diff.applicable(case, table, "Windows"), ["D-12", "D-01"])


class Cases(unittest.TestCase):
    def test_names_are_unique_and_rows_exist(self):
        names = [c.name for c in diff_cases.CASES]
        self.assertEqual(len(names), len(set(names)))
        table = allowlist.rows()
        for c in diff_cases.CASES:
            self.assertIn(c.os, ("both", "Linux", "Windows"), c.name)
            for row in c.allow:
                self.assertIn(row, table, f"{c.name}: {row}")
            for rel in c.files + tuple((p, "") for p in c.remove + c.readonly + c.compare):
                self.assertTrue(rel[0].split("/")[0] in ("root", "work"), f"{c.name}: {rel[0]}")


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run them to see them fail**

Run: `python -m unittest discover -s parity -p "test_diff.py"`
Expected: error, `No module named 'diff'`.

- [ ] **Step 3: Implement `parity/diff.py`**

```python
"""Differential tests (spec §12.4). Each case in diff_cases.py runs against rpyenv and against
the real upstream (pyenv on Linux, pyenv-win on Windows), each on a freshly built fixture root
at the same path, and the exit codes, stdout, stderr and the case's compared files are checked
byte for byte (plan decision 4).

  python parity/diff.py --rpyenv <dir with pyenv[.exe]> --upstream <pyenv source | pyenv-win dir> [--only NAME]

On Linux, --upstream is pyenv's source tree (with bin/pyenv). On Windows it is the `pyenv-win`
folder of a pyenv-win checkout (with bin/, libexec/ and .versions_cache.xml; .version is in
its parent).
"""
import argparse
import os
import shutil
import stat
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import allowlist  # noqa: E402
import diff_cases  # noqa: E402

WINDOWS = os.name == "nt"
OS_NAME = "Windows" if WINDOWS else "Linux"
VERSIONS = ("3.9.1", "3.10.11") if WINDOWS else ("3.12.10", "3.11.9")
BASE = os.path.join(allowlist.REPO, "target", "parity-diff")


def expand(text):
    return text.replace("{v0}", VERSIONS[0]).replace("{v1}", VERSIONS[1])


def applicable(case, table, os_name=OS_NAME):
    """The rows in `case.allow` that apply on `os_name`."""
    return [r for r in case.allow if table.get(r) in ("both", os_name)]


def verdict(same, rows_here):
    if same:
        return "stale" if rows_here else "same"
    return "allowed" if rows_here else "differs"


def remove_tree(path):
    """Removes `path`, clearing read-only flags that block a delete on Windows."""
    def again(func, p, _):
        os.chmod(p, stat.S_IWRITE)
        func(p)

    if os.path.exists(path):
        # `onerror` is deprecated from Python 3.12, whose `onexc` takes the same arguments.
        hook = {"onexc": again} if sys.version_info >= (3, 12) else {"onerror": again}
        shutil.rmtree(path, **hook)


def resolve(places, rel):
    head, _, tail = rel.partition("/")
    return os.path.join(places[head], *tail.split("/")) if tail else places[head]


def build(upstream, case):
    """A fresh fixture at BASE: two versions with a `python`, the first one global."""
    remove_tree(BASE)
    root = os.path.join(BASE, "pyenv-win" if WINDOWS else "root")
    work = os.path.join(BASE, "work")
    os.makedirs(work)
    for v in VERSIONS:
        d = os.path.join(root, "versions", v, *([] if WINDOWS else ["bin"]))
        os.makedirs(d)
        exe = os.path.join(d, "python.exe" if WINDOWS else "python")
        with open(exe, "w", encoding="utf-8") as f:
            f.write("" if WINDOWS else "#!/bin/sh\n")
        if not WINDOWS:
            os.chmod(exe, 0o755)
    os.makedirs(os.path.join(root, "shims"))
    with open(os.path.join(root, "version"), "w", encoding="utf-8", newline="") as f:
        f.write(VERSIONS[0] + "\n")
    if WINDOWS:
        # pyenv-win finds its root from its own location, so its scripts live in the fixture.
        for d in ("bin", "libexec"):
            shutil.copytree(os.path.join(upstream, d), os.path.join(root, d))
        shutil.copy(os.path.join(upstream, ".versions_cache.xml"), root)
        shutil.copy(os.path.join(upstream, "..", ".version"), BASE)
    places = {"root": root, "work": work}
    for rel, content in case.files:
        p = resolve(places, rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w", encoding="utf-8", newline="") as f:
            f.write(expand(content))
    for rel in case.remove:
        p = resolve(places, rel)
        if os.path.isdir(p):
            remove_tree(p)
        elif os.path.exists(p):
            os.remove(p)
    for rel in case.readonly:
        os.chmod(resolve(places, rel), stat.S_IREAD)
    return places


def snapshot(path):
    if os.path.isdir(path):
        return b"<directory>"
    if not os.path.exists(path):
        return None
    with open(path, "rb") as f:
        return f.read()


def run(tool, rpyenv, upstream, places, case):
    root, work = places["root"], places["work"]
    env = {"HOME": BASE, "USERPROFILE": BASE, "PYENV_ROOT": root}
    if WINDOWS:
        sysroot = os.environ["SystemRoot"]
        system32 = os.path.join(sysroot, "System32")
        env.update(SystemRoot=sysroot, ComSpec=os.path.join(system32, "cmd.exe"),
                   PATHEXT=".COM;.EXE;.BAT;.CMD;.VBS", PYENV=root, PYENV_HOME=root,
                   PATH=os.pathsep.join([os.path.join(root, "shims"), system32, sysroot]))
        if tool == "upstream":
            cmd = [os.path.join(system32, "cmd.exe"), "/d", "/c", "call",
                   os.path.join(root, "bin", "pyenv.bat")]
        else:
            cmd = [os.path.join(rpyenv, "pyenv.exe")]
    else:
        env.update(LANG="C.UTF-8", PATH=os.pathsep.join([os.path.join(root, "shims"), "/usr/bin", "/bin"]))
        cmd = [os.path.join(upstream, "bin", "pyenv")] if tool == "upstream" else [os.path.join(rpyenv, "pyenv")]
    for k, v in case.env:
        if v is None:
            env.pop(k, None)
        else:
            env[k] = expand(v)
    p = subprocess.run(cmd + [expand(a) for a in case.args], cwd=work, env=env,
                       capture_output=True, stdin=subprocess.DEVNULL, timeout=120)
    return (p.returncode, p.stdout, p.stderr, tuple(snapshot(resolve(places, r)) for r in case.compare))


def describe(a, b):
    out = []
    for label, x, y in (("exit code", a[0], b[0]), ("stdout", a[1], b[1]), ("stderr", a[2], b[2])):
        if x != y:
            out.append(f"    {label}: upstream {x!r}\n    {label}: rpyenv   {y!r}")
    for i, (x, y) in enumerate(zip(a[3], b[3])):
        if x != y:
            out.append(f"    compared file {i}: upstream {x!r}\n    compared file {i}: rpyenv   {y!r}")
    return "\n".join(out)


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("--rpyenv", required=True)
    p.add_argument("--upstream", required=True)
    p.add_argument("--only")
    a = p.parse_args(argv)
    rpyenv, upstream = os.path.abspath(a.rpyenv), os.path.abspath(a.upstream)
    table = allowlist.rows()
    counts, failures = {}, []
    for case in diff_cases.CASES:
        if case.os not in ("both", OS_NAME) or (a.only and case.name != a.only):
            continue
        unknown = [r for r in case.allow if r not in table]
        if unknown:
            failures.append(f"{case.name}: unknown allowlist rows {unknown}")
            continue
        results = {}
        for tool in ("upstream", "rpyenv"):
            places = build(upstream, case)
            results[tool] = run(tool, rpyenv, upstream, places, case)
        rows_here = applicable(case, table)
        v = verdict(results["upstream"] == results["rpyenv"], rows_here)
        counts[v] = counts.get(v, 0) + 1
        print(f"{v:8} {case.name}" + (f"  ({', '.join(rows_here)})" if rows_here else ""))
        if v in ("stale", "differs"):
            detail = describe(results["upstream"], results["rpyenv"])
            failures.append(f"{case.name}: {v}" + (f"\n{detail}" if detail else ""))
            if detail:
                print(detail)
    remove_tree(BASE)
    print(f"{OS_NAME}: " + ", ".join(f"{n} {k}" for k, n in sorted(counts.items())))
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(f"### Differential tests ({OS_NAME})\n\n" + ", ".join(f"{n} {k}" for k, n in sorted(counts.items()))
                    + "\n\n" + "".join(f"- {x.splitlines()[0]}\n" for x in failures) + "\n")
    for x in failures:
        print("FAIL " + x, file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

- [ ] **Step 4: Add the measured cases**

Replace `CASES = ()` in `parity/diff_cases.py` with the cases below. The `allow` rows come from the planning probe:
- `help` and no arguments on Windows are the two whose detailed difference the probe output cut short. D-08 (the missing `)`) and D-20 (the first line) are the expected explanation. Step 5 confirms or corrects that by measuring.
- The last two cases (`local` and `global` writing a file) were not in the probe. They compare the version file's bytes, which also settles the CRLF question carried from M1c-a.

```python
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
```

- [ ] **Step 5: Measure on both OSes, and make every verdict honest**

1. **Upstream copies.**
   - Windows: `git clone` pyenv-win into `C:\tmp\pyenv-win-856ed5a` and check out 856ed5a. Git on this host converts line endings to CRLF on checkout.
   - Linux: in WSL, `curl -fsSL https://github.com/pyenv/pyenv/archive/ab74141ab4bcd34aa9b11f363dd8ad4f6fc01cd2.tar.gz | tar -xz` into `~/m1cb-upstream`. Do this in a script file run with `--exec`.
2. **Run** the harness:
   - Windows: `python parity/diff.py --rpyenv target/debug --upstream C:/tmp/pyenv-win-856ed5a/pyenv-win`.
   - WSL: `python3 parity/diff.py --rpyenv target/debug --upstream ~/m1cb-upstream/pyenv-ab74141…`, from `~/rpyenv-linux` after fetching the branch.
3. **Rules for each case**, applied per OS:
   - **`same` or `allowed`:** done.
   - **`stale`** (it matches although `allow` lists a row for this OS): remove that row from the case's `allow`.
   - **`differs`:** read the difference, then do one of these:
     1. If an existing row explains it, add that row to `allow`. **Cite only rows whose described difference actually appears.**
     2. If no row explains it, stop and report the case, the difference and your analysis as DONE_WITH_CONCERNS. The controller rules: a new row, or an rpyenv bug to fix in this task.
4. **Finish** with both OSes passing (exit 0). Copy both runs' output into the report.

- [ ] **Step 6: Run the unit tests, then commit**

Run: `python -m unittest discover -s parity -p "test_*.py"` (Windows) and the same with `python3` in WSL. Both should pass.

```bash
git add parity/diff.py parity/diff_cases.py parity/test_diff.py
git commit -m "Add the differential harness and its measured cases"
```

---

### Task 3: Upstream pyenv's bats suite, run in a container, with its expected failures

**Files:**
- Create: `parity/bats_run.sh`, `parity/bats_check.py`, `parity/test_bats_check.py`
- Modify: `parity/expected/bats.txt` (the list below)

**Interfaces:**
- Consumes: `allowlist.rows`, `allowlist.read_expected`, `allowlist.check_reason` (Task 1)
- Produces:
  - `bats_check.parse_tap(text) -> dict[tuple[str, str], bool]` (`(file, name)` → passed)
  - `bats_check.problems(results, expected, table) -> list[str]`
  - `bats_check.main(argv) -> int`
  - `bash parity/bats_run.sh <rpyenv-bin> <pyenv-src> <bats-dir> <tap-out>`, run as root, writing `<file>\t<TAP line>` lines

- [ ] **Step 1: Write the failing unit tests**

`parity/test_bats_check.py`:

```python
import unittest

import bats_check

TAP = "\n".join([
    "a.bats\t1..3",
    "a.bats\tok 1 passes",
    "a.bats\tnot ok 2 fails here",
    "a.bats\t# (in test file a.bats, line 9)",
    "a.bats\tok 3 skipped one # skip -- fish not installed",
    "b.bats\tnot ok 1 fails here",
])
TABLE = {"D-35": "Linux", "D-12": "Windows"}


class Parse(unittest.TestCase):
    def test_reads_results_per_file_and_treats_skips_as_passing(self):
        self.assertEqual(bats_check.parse_tap(TAP), {
            ("a.bats", "passes"): True,
            ("a.bats", "fails here"): False,
            ("a.bats", "skipped one"): True,
            ("b.bats", "fails here"): False,
        })


class StaleEntries(unittest.TestCase):
    """Review focus 1."""

    results = bats_check.parse_tap(TAP)
    good = {"a.bats | fails here": "D-35 x", "b.bats | fails here": "M3 y"}

    def test_the_exact_set_passes(self):
        self.assertEqual(bats_check.problems(self.results, self.good, TABLE), [])

    def test_an_unlisted_failure_fails(self):
        expected = {"a.bats | fails here": "D-35 x"}
        (p,) = bats_check.problems(self.results, expected, TABLE)
        self.assertIn("not listed", p)

    def test_a_listed_test_that_passes_fails(self):
        expected = dict(self.good, **{"a.bats | passes": "D-35 x"})
        (p,) = bats_check.problems(self.results, expected, TABLE)
        self.assertIn("passed", p)

    def test_a_listed_test_that_does_not_exist_fails(self):
        expected = dict(self.good, **{"a.bats | renamed": "D-35 x"})
        (p,) = bats_check.problems(self.results, expected, TABLE)
        self.assertIn("no such test", p)

    def test_a_windows_row_is_rejected(self):
        expected = dict(self.good, **{"a.bats | fails here": "D-12 x"})
        (p,) = bats_check.problems(self.results, expected, TABLE)
        self.assertIn("Windows row", p)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run them to see them fail**

Run: `python -m unittest discover -s parity -p "test_bats_check.py"`
Expected: error, `No module named 'bats_check'`.

- [ ] **Step 3: Implement**

`parity/bats_check.py`:

```python
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
```

`parity/bats_run.sh`:

```bash
#!/usr/bin/env bash
# Runs upstream pyenv's bats suite against rpyenv (spec §12.3, plan decision 2). Run it as
# root, in a Debian container on CI or on a Debian host:
#   bash parity/bats_run.sh <rpyenv-bin-dir> <pyenv-src-dir> <bats-dir> <tap-out>
# Each test file runs as the unprivileged user `tester`, against root-owned copies of rpyenv's
# binaries that no test can change. <tap-out> gets one `<file>\t<TAP line>` line per TAP line.
set -euo pipefail
bin=$1 up=$2 bats=$3 out=$4
work=$(mktemp -d /tmp/rpyenv-bats.XXXXXX)
chmod 755 "$work"
install -d -m 755 "$work/bin"
install -m 755 "$bin/pyenv" "$bin/pyenv-shim" "$work/bin/"
# The suite puts `<test>/../libexec` on PATH: there, `pyenv` is rpyenv and each `pyenv-<cmd>`
# the suite calls directly is a wrapper for `pyenv <cmd>`.
mkdir -p "$work/run/libexec"
cp -r "$up/test" "$work/run/test"
ln -s "$work/bin/pyenv" "$work/run/libexec/pyenv"
for c in root prefix version version-name version-origin version-file version-file-read \
         version-file-write versions which whence exec rehash shims commands help global local; do
  printf '#!/bin/sh\nexec pyenv %s "$@"\n' "$c" > "$work/run/libexec/pyenv-$c"
done
printf '#!/bin/sh\nexec pyenv --version "$@"\n' > "$work/run/libexec/pyenv---version"
chmod 755 "$work/run/libexec/"pyenv-*
id tester >/dev/null 2>&1 || useradd --create-home tester
chown -R tester "$work/run"
: > "$out"
cd "$work/run/test"
for f in $(ls -- *.bats); do
  runuser -u tester -- "$bats/bin/bats" --tap "./$f" 2>&1 | sed "s|^|$f\t|" >> "$out" || true
done
echo "bats: $(grep -c -P '\tok ' "$out") ok, $(grep -c -P '\tnot ok ' "$out") not ok"
```

`parity/expected/bats.txt`: keep the Task 1 header line, then add these lines. Format: `<file> | <test name> | <reason>`; the reason starts with a row or a milestone.

```
commands.bats | commands | D-49 rpyenv lists the commands it implements
commands.bats | commands --sh | D-49 rpyenv lists the commands it implements
commands.bats | commands in path with spaces | D-49 rpyenv lists the commands it implements
commands.bats | commands --no-sh | D-49 rpyenv lists the commands it implements
completions.bats | command with no completion support | M3 shell completions
completions.bats | command with completion support | M3 shell completions
completions.bats | forwards extra arguments | M3 shell completions
exec.bats | completes with names of executables | M3 shell completions
exec.bats | carries original IFS within hooks | D-51 no bash hooks
help.bats | shows help for a specific command | M4 `help` for a plugin command (plugin dispatch)
help.bats | replaces missing extended help with summary text | M4 `help` for a plugin command (plugin dispatch)
help.bats | extracts only usage | M4 `help` for a plugin command (plugin dispatch)
help.bats | multiline usage section | M4 `help` for a plugin command (plugin dispatch)
help.bats | multiline extended help section | M4 `help` for a plugin command (plugin dispatch)
hooks.bats | prints usage help given no argument | D-51 no bash hooks, no `pyenv hooks` command
hooks.bats | prints list of hooks | D-51 no bash hooks, no `pyenv hooks` command
hooks.bats | supports hook paths with spaces | D-51 no bash hooks, no `pyenv hooks` command
hooks.bats | resolves relative paths | D-51 no bash hooks, no `pyenv hooks` command
hooks.bats | resolves symlinks | D-51 no bash hooks, no `pyenv hooks` command
init.bats | creates shims and versions directories | M3 shell integration (`pyenv init`)
init.bats | auto rehash | M3 shell integration (`pyenv init`)
init.bats | auto rehash for --path | M3 shell integration (`pyenv init`)
init.bats | setup shell completions | M3 shell integration (`pyenv init`)
init.bats | detect parent shell | M3 shell integration (`pyenv init`)
init.bats | detect parent shell from script | M3 shell integration (`pyenv init`)
init.bats | setup shell completions (fish) | M3 shell integration (`pyenv init`)
init.bats | fish instructions | M3 shell integration (`pyenv init`)
init.bats | setup shell completions (pwsh) | M3 shell integration (`pyenv init`)
init.bats | pwsh instructions | M3 shell integration (`pyenv init`)
init.bats | shell detection for installer | M3 shell integration (`pyenv init`)
init.bats | shell detection for fish startup file | M3 shell integration (`pyenv init`)
init.bats | completion includes install option | M3 shell integration (`pyenv init`)
init.bats | install setup for detected shell startup files | M3 shell integration (`pyenv init`)
init.bats | install setup for bash uses existing bash_profile | M3 shell integration (`pyenv init`)
init.bats | install setup for zsh startup files | M3 shell integration (`pyenv init`)
init.bats | install setup for fish startup file | M3 shell integration (`pyenv init`)
init.bats | install setup for pwsh startup file | M3 shell integration (`pyenv init`)
init.bats | install refuses to modify files with pyenv-related code | M3 shell integration (`pyenv init`)
init.bats | install treats PYENV_ROOT as pyenv-related code | M3 shell integration (`pyenv init`)
init.bats | install refuses unreadable startup file without partial writes | M3 shell integration (`pyenv init`)
init.bats | install setup keeps fish block intact when generic lines already exist | M3 shell integration (`pyenv init`)
init.bats | install setup fails gracefully for unsupported shell | M3 shell integration (`pyenv init`)
init.bats | option to skip rehash | M3 shell integration (`pyenv init`)
init.bats | adds shims to PATH | M3 shell integration (`pyenv init`)
init.bats | adds shims to PATH (fish) | M3 shell integration (`pyenv init`)
init.bats | adds shims to PATH (pwsh) | M3 shell integration (`pyenv init`)
init.bats | removes existing shims from PATH | M3 shell integration (`pyenv init`)
init.bats | adds shims to PATH with --no-push-path if they're not on PATH | M3 shell integration (`pyenv init`)
init.bats | doesn't change PATH with --no-push-path if shims are already on PATH | M3 shell integration (`pyenv init`)
init.bats | outputs sh-compatible syntax | M3 shell integration (`pyenv init`)
init.bats | outputs sh-compatible case syntax | M3 shell integration (`pyenv init`)
init.bats | outputs fish-specific syntax (fish) | M3 shell integration (`pyenv init`)
init.bats | outputs pwsh-specific syntax (pwsh) | M3 shell integration (`pyenv init`)
latest.bats | read from installed | M2 `pyenv latest` arrives with the installer
latest.bats | read from known | M2 `pyenv latest` arrives with the installer
latest.bats | installed version not found | M2 `pyenv latest` arrives with the installer
latest.bats | known version not found | M2 `pyenv latest` arrives with the installer
latest.bats | complete name resolves to itself | M2 `pyenv latest` arrives with the installer
latest.bats | sort CPython | M2 `pyenv latest` arrives with the installer
latest.bats | ignores rolling releases, branch tips, alternative srcs, prereleases, virtualenvs; 't' versions if prefix without 't' | M2 `pyenv latest` arrives with the installer
latest.bats | resolves to a 't' version if prefix has 't' | M2 `pyenv latest` arrives with the installer
latest.bats | falls back to argument with -b | M2 `pyenv latest` arrives with the installer
latest.bats | falls back to argument and succeeds with -f | M2 `pyenv latest` arrives with the installer
pip-rehash.bats | pip-rehash triggered when using 'pip' | D-39 no pip wrapper; the shim's exit check rehashes
pip-rehash.bats | pip-rehash triggered when using 'pip3' | D-39 no pip wrapper; the shim's exit check rehashes
pip-rehash.bats | pip-rehash triggered when using 'pip3.x' | D-39 no pip wrapper; the shim's exit check rehashes
pip-rehash.bats | pip-rehash triggered when using 'python -m pip install' | D-39 no pip wrapper; the shim's exit check rehashes
prefix.bats | prefix for system in / | D-52 replaces `pyenv-which` on PATH; rpyenv resolves in-process
pyenv.bats | default PYENV_DIR | M4 uses the test plugin `pyenv echo` (plugin dispatch)
pyenv.bats | inherited PYENV_DIR | M4 uses the test plugin `pyenv echo` (plugin dispatch)
pyenv.bats | adds its own libexec and plugin bin dirs to PATH | D-38 the child's PATH has no libexec or plugin dirs
pyenv.bats | PYENV_HOOK_PATH preserves value from environment | D-51 no bash hooks
pyenv.bats | PYENV_HOOK_PATH includes pyenv built-in plugins | D-51 no bash hooks
rehash.bats | empty rehash | D-35 rehash keeps its state and template in `shims`
rehash.bats | rehash in progress | D-35 the lock is `shims/.rehash.lock`
rehash.bats | removes stale lockfile | D-35 the lock is `shims/.rehash.lock`
rehash.bats | leaves valid shims unchanged when repairing another shim | D-35 every shim shares the template, so a repair relinks all
rehash.bats | repairs explicitly registered hidden shims and preserves other dotfiles | D-51 shims registered by hooks
rehash.bats | preserves sourceable shims from the built-in rehash hook | D-34 no source shims for activation scripts
rehash.bats | does not overwrite hook customizations when a shim is registered again | D-51 no bash hooks
rehash.bats | repairs existing shims before rehash hooks invoke them | D-51 no bash hooks
rehash.bats | carries original IFS within hooks | D-51 no bash hooks
rehash.bats | sh-rehash in bash | M3 shell integration (`pyenv sh-rehash`)
rehash.bats | sh-rehash in bash (integration) | M3 shell integration (`pyenv sh-rehash`)
rehash.bats | sh-rehash in fish | M3 shell integration (`pyenv sh-rehash`)
rehash.bats | sh-rehash in pwsh | M3 shell integration (`pyenv sh-rehash`)
rehash.bats | shim sets _PYENV_SHIM_PATH when linked from elsewhere | D-52 shims don't pass `_PYENV_SHIM_PATH`
shell.bats | shell integration enabled | M3 shell integration (`pyenv shell`)
shell.bats | no shell version | M3 shell integration (`pyenv shell`)
shell.bats | shell version | M3 shell integration (`pyenv shell`)
shell.bats | shell version (fish) | M3 shell integration (`pyenv shell`)
shell.bats | shell version (pwsh) | M3 shell integration (`pyenv shell`)
shell.bats | shell revert | M3 shell integration (`pyenv shell`)
shell.bats | shell revert (fish) | M3 shell integration (`pyenv shell`)
shell.bats | shell revert (pwsh) | M3 shell integration (`pyenv shell`)
shell.bats | shell unset | M3 shell integration (`pyenv shell`)
shell.bats | shell unset (fish) | M3 shell integration (`pyenv shell`)
shell.bats | shell unset (pwsh) | M3 shell integration (`pyenv shell`)
shell.bats | shell change invalid version | M3 shell integration (`pyenv shell`)
shell.bats | shell change version | M3 shell integration (`pyenv shell`)
shell.bats | shell change version (fish) | M3 shell integration (`pyenv shell`)
shell.bats | shell change version (pwsh) | M3 shell integration (`pyenv shell`)
--version.bats | reads version from git repo | D-01 the version line is fixed, not from git
version-name.bats | PYENV_VERSION can be overridden by hook | D-51 no bash hooks
version-name.bats | carries original IFS within hooks | D-51 no bash hooks
version-origin.bats | reports from hook | D-51 no bash hooks
version-origin.bats | carries original IFS within hooks | D-51 no bash hooks
versions.bats | sort doesn't support version sorting | D-50 versions are always in version order
which.bats | carries original IFS within hooks | D-51 no bash hooks
which.bats | hooks get resolved version name | D-51 no bash hooks
```

- [ ] **Step 4: Check each classification against the test body**

The list was classified by test name (Measured baseline). For each entry, read the test in pyenv ab74141's `test/<file>` and confirm that the reason is the cause. If it isn't, cite the row that is, or report the entry. Report every entry you changed.

- [ ] **Step 5: Run the suite and the check on Linux**

In WSL, from script files run with `--exec`:
1. **Upstream copies.** Use `~/m1cb-upstream/` from Task 2, plus bats: `curl -fsSL https://github.com/bats-core/bats-core/archive/refs/tags/v1.11.1.tar.gz | tar -xz`.
2. **Build.** In `~/rpyenv-linux`, after fetching this branch, run `cargo build --workspace`.
3. **Run the suite as root:** `wsl -d Debian -u root --exec /usr/bin/bash /mnt/c/tmp/<script>.sh`. The script runs `bash parity/bats_run.sh /home/jm/rpyenv-linux/target/debug <pyenv-src> <bats-dir> /home/jm/m1cb-bats-tap.txt`, with every path written literally in the script.
4. **Check the log:** `python3 parity/bats_check.py /home/jm/m1cb-bats-tap.txt`.

Expected:
- `bats_run.sh` reports 156 ok and 110 not ok, matching the baseline.
- The check exits 0.
- WSL runs Debian forky/sid, not trixie. If the counts differ from the baseline, report the difference: the CI container (Task 5) is the authority.

**Check it can fail:**
1. Delete one line from `bats.txt` and run the check. It must fail with "not listed".
2. Restore the line, then add a line for a passing test. The check must fail with "passed".
3. Restore the file.

- [ ] **Step 6: Run the unit tests, then commit**

```bash
git add parity/bats_run.sh parity/bats_check.py parity/test_bats_check.py parity/expected/bats.txt
git commit -m "Run upstream pyenv's bats suite with a checked list of expected failures"
```

---

### Task 4: pyenv-win's pytest suite, run through a conftest overlay, with its expected failures

**Files:**
- Create: `parity/pyenv_win_overlay.py`, `parity/pyenv_win_run.py`, `parity/requirements-pyenv-win.txt`, `parity/test_pyenv_win_overlay.py`
- Modify: `parity/expected/pyenv-win.txt` (the list below)

**Interfaces:**
- Consumes: `allowlist.rows`, `allowlist.read_expected`, `allowlist.check_reason` (Task 1)
- Produces:
  - `pyenv_win_overlay.test_key(nodeid) -> str`: the nodeid without the `PYENV_FORCE_ARCH` parameter
  - `python parity/pyenv_win_run.py --pyenv-win <checkout> --rpyenv <bin dir> [pytest args]`

- [ ] **Step 1: Write the failing unit test**

`parity/test_pyenv_win_overlay.py`:

```python
import ast
import os
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))


def load_test_key():
    """`test_key` from the overlay, without importing the rest of it: the overlay is appended
    to pyenv-win's conftest.py, where `pytest` and its fixtures are in scope."""
    with open(os.path.join(HERE, "pyenv_win_overlay.py"), encoding="utf-8") as f:
        tree = ast.parse(f.read())
    fn = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == "test_key")
    scope = {"_re": __import__("re")}
    exec(compile(ast.Module(body=[fn], type_ignores=[]), "overlay", "exec"), scope)
    return scope["test_key"]


class Keys(unittest.TestCase):
    """Review focus 1: one expected-failure line matches both architectures."""

    def test_the_arch_parameter_is_dropped(self):
        key = load_test_key()
        self.assertEqual(
            key("test_pyenv_feature_exec.py::test_exec_arg[PYENV_FORCE_ARCH=AMD64-Two Words-python shim]"),
            "test_pyenv_feature_exec.py::test_exec_arg[Two Words-python shim]",
        )
        self.assertEqual(key("t.py::test_latest_quiet[PYENV_FORCE_ARCH=X86]"), "t.py::test_latest_quiet")
        self.assertEqual(key("t.py::test_bad_path[PYENV_FORCE_ARCH=X86-<lambda>]"), "t.py::test_bad_path[<lambda>]")


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run it to see it fail**

Run: `python -m unittest discover -s parity -p "test_pyenv_win_overlay.py"`
Expected: error, `No such file or directory: …pyenv_win_overlay.py`.

- [ ] **Step 3: Implement**

`parity/pyenv_win_overlay.py`:

```python

# --- rpyenv overlay: appended to pyenv-win's tests/conftest.py by parity/pyenv_win_run.py ---
# Runs pyenv-win's own suite against rpyenv (spec §12.3). pyenv-win's setup builds each test's
# fake root; rpyenv's binaries then go into its bin folder, and the suite calls bin\pyenv.exe.
# Tests listed in parity/expected/pyenv-win.txt are strict xfails (plan decision 3).
import os as _os
import re as _re
import shutil as _shutil
import sys as _sys

_RPYENV_BIN = _os.environ["RPYENV_BIN"]
_PARITY = _os.environ["RPYENV_PARITY"]
_sys.path.insert(0, _PARITY)
import allowlist as _allowlist  # noqa: E402


@pytest.fixture()
def pyenv_file(bin_path):
    return str(Path(bin_path, "pyenv.exe"))


@pytest.fixture(autouse=True)
def tmp_pyenv(tmp_path, pyenv_path, local_path, bin_path, shims_path, settings, arch):
    settings = settings()
    touch(tmp_path / '.python-version')
    settings['pyenv_path'] = pyenv_path
    settings['local_path'] = local_path
    os.mkdir(pyenv_path)
    os.mkdir(local_path)
    pyenv_setup(settings)
    for f in ("pyenv.exe", "pyenv-shim.exe", "pyenv-shimw.exe"):
        _shutil.copy(_os.path.join(_RPYENV_BIN, f), bin_path)
    prev_cwd = os.getcwd()
    os.chdir(local_path)
    yield
    os.chdir(prev_cwd)


def test_key(nodeid):
    """The nodeid without the session's `PYENV_FORCE_ARCH=…` parameter, so one line matches
    both architectures."""
    key = _re.sub(r"PYENV_FORCE_ARCH=\w+-?", "", nodeid)
    return _re.sub(r"\[\]$", "", key)


def pytest_collection_modifyitems(config, items):
    expected = _allowlist.read_expected(_os.path.join(_PARITY, "expected", "pyenv-win.txt"), 1)
    table = _allowlist.rows()
    problems = []
    for key, reason in expected.items():
        bad = _allowlist.check_reason(reason, "Windows", table)
        if bad:
            problems.append(f"{key}: {bad}")
    seen = set()
    for item in items:
        key = test_key(item.nodeid)
        if key in expected:
            seen.add(key)
            item.add_marker(pytest.mark.xfail(strict=True, reason=expected[key]))
    problems += [f"{k}: listed, but there is no such test" for k in sorted(set(expected) - seen)]
    if problems:
        raise pytest.UsageError("parity/expected/pyenv-win.txt:\n" + "\n".join(problems))
```

The name `test_key` doesn't start a test that pytest collects: conftest.py is not a test module.

`parity/pyenv_win_run.py`:

```python
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
    cmd = [sys.executable, "-m", "pytest", "-p", "no:cacheprovider", "-q", "-rfEX", tests, *rest]
    return subprocess.run(cmd, env=env).returncode


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

`parity/requirements-pyenv-win.txt`:

```
pytest==9.1.1
tempenv==2.1.0
packaging==26.3
```

`parity/expected/pyenv-win.txt`: keep the Task 1 header line, then add these lines. Format: `<test id without the architecture> | <reason>`.

```
test_pyenv_feature_exec.py::test_exec_arg[Double Quote-python shim] | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_exec.py::test_exec_arg[Exclamation Mark-python shim] | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_exec.py::test_exec_arg[Imbalance Double Quote-python shim] | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_exec.py::test_exec_arg[Imbalance Single Quote-python shim] | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_exec.py::test_exec_arg[One Double Quote-python shim] | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_exec.py::test_exec_arg[One Single Quote-python shim] | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_exec.py::test_exec_arg[One Word-python shim] | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_exec.py::test_exec_arg[Percentage-python shim] | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_exec.py::test_exec_arg[Pound-python shim] | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_exec.py::test_exec_arg[Single Quote-python shim] | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_exec.py::test_exec_arg[Two Words-python shim] | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_exec.py::test_path_not_updated | D-36 shims are `.exe`, not `python.bat`
test_pyenv_feature_install.py::test_check_pyenv_install_list | M2 installer
test_pyenv_feature_install.py::test_patched_venv_module[3.10.11-python310] | M2 installer
test_pyenv_feature_install.py::test_patched_venv_module[3.11.3-python311] | M2 installer
test_pyenv_feature_install.py::test_patched_venv_module[3.9.13-python39] | M2 installer
test_pyenv_feature_latest.py::test_latest_arch_cases[<lambda>] | M2 `pyenv latest` arrives with the installer
test_pyenv_feature_latest.py::test_latest_edge_cases[<lambda>] | M2 `pyenv latest` arrives with the installer
test_pyenv_feature_latest.py::test_latest_help | M2 `pyenv latest` arrives with the installer
test_pyenv_feature_latest.py::test_latest_quiet | M2 `pyenv latest` arrives with the installer
test_pyenv_feature_latest.py::test_latest_sort[<lambda>] | M2 `pyenv latest` arrives with the installer
test_pyenv_feature_rehash.py::test_rehash_global_version[<lambda>] | D-36 one `.exe` shim per program; no extensionless sh shims
test_pyenv_feature_rehash.py::test_rehash_local_version[<lambda>] | D-36 one `.exe` shim per program; no extensionless sh shims
test_pyenv_feature_shell.py::test_no_shell_version[cmd] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_no_shell_version[powershell] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_no_shell_version[pwsh] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_help[cmd] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_help[powershell] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_help[pwsh] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_many_versions_defined[cmd] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_many_versions_defined[powershell] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_many_versions_defined[pwsh] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_set_installed_version[cmd-<lambda>] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_set_many_versions[cmd-<lambda>] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_set_many_versions_one_not_installed[cmd-<lambda>] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_set_many_versions_one_not_installed[powershell-<lambda>] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_set_many_versions_one_not_installed[pwsh-<lambda>] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_set_unknown_version[cmd-<lambda>] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_set_unknown_version[powershell-<lambda>] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_set_unknown_version[pwsh-<lambda>] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_unset_unaffected[cmd-<lambda>] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_version_defined[cmd] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_version_defined[powershell] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_shell.py::test_shell_version_defined[pwsh] | M3 shell integration (`pyenv shell`)
test_pyenv_feature_version.py::test_bad_path[<lambda>] | D-15 the PATH check looks for `shims\python.exe`
```

- [ ] **Step 4: Run the suite on Windows**

1. Use the pyenv-win checkout from Task 2 (`C:\tmp\pyenv-win-856ed5a`), or make a fresh `git clone` and check out 856ed5a.
2. Create a venv in `C:\tmp`, install `parity/requirements-pyenv-win.txt`, and run `cargo build --workspace`.
3. Run `python parity/pyenv_win_run.py --pyenv-win C:/tmp/pyenv-win-856ed5a --rpyenv target/debug`, using the venv's python.

Expected: exit 0. Both architectures run, because xfails don't count as failures and the suite skips X86 only after a real failure. You should see 45 xfailed for AMD64 and 42 for X86, plus 3 skipped.

**Check it can fail:**
1. Remove one line from the list. The run must fail on that test.
2. Restore the line, then add a line for a passing test, for example `test_pyenv_feature_versions.py::test_list_no_versions`; check the exact id with `--collect-only`. Strict xfail must report XPASS(strict), and the run must fail.
3. Restore the file.

- [ ] **Step 5: Run the unit tests, then commit**

```bash
git add parity/pyenv_win_overlay.py parity/pyenv_win_run.py parity/requirements-pyenv-win.txt parity/test_pyenv_win_overlay.py parity/expected/pyenv-win.txt
git commit -m "Run pyenv-win's pytest suite through a conftest overlay with strict expected failures"
```

---

### Task 5: The parity workflow

**Files:**
- Create: `.github/workflows/parity.yml`

**Interfaces:**
- Consumes the command lines of Tasks 1–4.

- [ ] **Step 1: Write the workflow**

```yaml
name: Parity

on:
  push:
    branches: [main]
  pull_request:

env:
  PYENV_SHA: ab74141ab4bcd34aa9b11f363dd8ad4f6fc01cd2
  PYENV_WIN_SHA: 856ed5a8c107879d53374f782a4b40cc794f19e0
  BATS_VERSION: "1.11.1"

jobs:
  coverage:
    name: allowlist coverage
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - run: python3 -m unittest discover -s parity -p "test_*.py" -v
      - run: python3 parity/coverage.py --report

  linux:
    name: upstream pyenv (differential + bats)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02
        with:
          toolchain: "1.96"
      - run: cargo build --workspace
      - name: Fetch upstream pyenv and bats
        run: |
          mkdir -p upstream
          curl -fsSL "https://github.com/pyenv/pyenv/archive/${PYENV_SHA}.tar.gz" | tar -xz -C upstream
          mv "upstream/pyenv-${PYENV_SHA}" upstream/pyenv
          curl -fsSL "https://github.com/bats-core/bats-core/archive/refs/tags/v${BATS_VERSION}.tar.gz" | tar -xz -C upstream
          mv "upstream/bats-core-${BATS_VERSION}" upstream/bats
      - name: Differential tests (spec §12.4)
        run: python3 parity/diff.py --rpyenv target/debug --upstream upstream/pyenv
      - name: Upstream bats suite in debian:trixie-slim (spec §12.3)
        run: |
          docker run --rm -v "$PWD:/w" -w /w debian:trixie-slim bash -c \
            'apt-get update -qq && apt-get install -y -qq --no-install-recommends gawk >/dev/null && bash parity/bats_run.sh /w/target/debug /w/upstream/pyenv /w/upstream/bats /w/bats-tap.txt'
          python3 parity/bats_check.py bats-tap.txt

  windows:
    name: upstream pyenv-win (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [windows-2025, windows-2022]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02
        with:
          toolchain: "1.96"
      - run: cargo build --workspace
      - name: Fetch pyenv-win
        shell: bash
        run: |
          git clone --quiet https://github.com/pyenv-win/pyenv-win.git upstream/pyenv-win
          git -C upstream/pyenv-win checkout --quiet "$PYENV_WIN_SHA"
      - name: VBScript is available (pyenv-win runs on cscript)
        shell: cmd
        run: |
          echo WScript.Echo "vbscript ok" > "%RUNNER_TEMP%\probe.vbs"
          cscript //nologo "%RUNNER_TEMP%\probe.vbs" || (echo ::error::VBScript is not available on this runner, so pyenv-win cannot run & exit /b 1)
      - name: Differential tests (spec §12.4)
        run: python parity/diff.py --rpyenv target/debug --upstream upstream/pyenv-win/pyenv-win
      - run: python -m pip install -q -r parity/requirements-pyenv-win.txt
      - name: pyenv-win's pytest suite (spec §12.3)
        run: python parity/pyenv_win_run.py --pyenv-win upstream/pyenv-win --rpyenv target/debug
```

- [ ] **Step 2: Reproduce every job locally**

Nothing here can run on CI before the PR (plan decision 7), so run each job's commands exactly as written, with only the paths changed:
- **coverage:** on Windows (`python`) and in WSL (`python3`).
- **linux:** in WSL, from a script file. Use the tarball URLs as written, into a fresh folder, and run both parity commands. For the bats step, run `bats_run.sh` as root directly, without docker: WSL has no docker (Task 3's method).
- **windows:** on this host. Use a fresh `git clone` and checkout, the cscript probe as written (in `cmd`), then the diff and pytest commands.

Paste each command and its last lines of output into the report. Then check that the YAML parses: install PyYAML into the Task 4 venv (`python -m pip install pyyaml`) and run `python -c "import yaml; yaml.safe_load(open('.github/workflows/parity.yml', encoding='utf-8'))"`. The PR's run is the real check.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/parity.yml
git commit -m "Run the parity suites, the differential tests and the coverage report in CI"
```

---

### Task 6: Cover every allowlist row, then enforce it

**Files:**
- Modify: `parity/diff_cases.py` (new cases), `parity/untestable.txt`, `.github/workflows/parity.yml` (drop `--report`)
- Modify: Rust test files whose tests already pin a row: add `(allowlist D-NN)` to the test's doc comment
- Modify: `docs/parity/allowlist.md` (D-48: replace "most likely" with the measured upstream behaviour)

**Interfaces:**
- Consumes:
  - `coverage.py --report` (Task 1)
  - `diff.py` and the `Case` fields (Task 2)

- [ ] **Step 1: List the gaps**

Run: `python parity/coverage.py --report`. Copy the uncovered rows into the report.

- [ ] **Step 2: Cover each row, choosing in this order**

1. **A differential case**, when both tools can run the command on a fixture. The case's `allow` cites the row, and the measured run must show the row's difference: verdict `allowed`. This is the preferred method, because it pins both sides. Cases to add, at least:
   - **D-04** (Linux): `version-name` with `work/.python-version` holding a word longer than 1024 characters. Use `files=(("work/.python-version", "a" * 1500 + "\n"),)` and `allow=("D-04",)`.
   - **D-19** (Linux): `version` with `env=(("PYENV_DEBUG", "1"),)` and `allow=("D-19",)`.
   - **D-21** (Linux): `local {v1}` with `work/.python-version` present and listed in `readonly`, `allow=("D-21",)`. The CI runner's user isn't root, so the write fails.
   - **D-22** (Windows): `version extra`, with `allow=("D-22",)`.
   - **D-17** (Windows): `versions` with `remove=("root/versions",)`, `compare=("root/versions",)` and `allow=("D-17",)`.
   - **D-48** (Windows): a local file naming a version with a character outside the console's code page, so that the `version` message prints the name. For example `files=(("work/.python-version", "3.9.1-漢\n"),)`, `allow=("D-48",)`. Then rewrite D-48's Upstream cell with what pyenv-win actually printed (the measured bytes), replacing "most likely … (not measured …)".
   - Also add every other row that describes a command-level difference: D-02, D-05, D-06, D-07, D-09, D-10, D-11, D-13, D-14, D-18, D-23 to D-28, D-31, D-32, D-37, D-41, unless measuring shows the case can't be built. Each case's comment names the row's trigger from the row's own Upstream text.
2. **A Rust test that already pins rpyenv's side of the row.** Add `(allowlist D-NN)` to the doc comment directly above its `#[test]`. Choose a test that would fail if rpyenv's behaviour for the row changed, and name in the report the assertion that pins it. This fits rows about shims and launching: D-29, D-30, D-33 to D-36, D-38 to D-40, D-42, D-43, D-45 to D-47. If no such test exists, write one, in the existing test file for that area, in that file's style.
3. **`parity/untestable.txt`**, only for a row that neither method can pin. Give a reason that names what the test would need. Expect this to be rare: a case that can't be built can usually be a Rust test.

**Rule for a case that measures `differs`** (it shows an unexplained difference beyond the cited row): stop and report it, as in Task 2.

- [ ] **Step 3: Enforce coverage**

In `.github/workflows/parity.yml`, change `python3 parity/coverage.py --report` to `python3 parity/coverage.py`.

Run on Windows and in WSL:
- `python parity/coverage.py`: exit 0 and `covered: 52` (or the current row count).
- both differential runs: exit 0.
- `cargo test --workspace`: it still passes, because comment edits change nothing.

**Check it can fail:**
1. Comment out one case's `allow` row whose only citation is that case. Coverage must fail with "uncovered".
2. Add a citation of `D-99` in a test comment. Coverage must fail with "cited but not in the allowlist".
3. Restore both.

- [ ] **Step 4: Commit**

Use one commit per kind:
- the differential cases;
- the Rust citations and any new tests;
- the untestable entries plus the D-48 measurement;
- the enforcement.

```bash
git commit -m "Add differential cases for the remaining command-level allowlist rows"
git commit -m "Cite allowlist rows above the Rust tests that pin them"
git commit -m "Record untestable rows and the measured pyenv-win output for D-48"
git commit -m "Enforce allowlist coverage in CI"
```

---

### Task 7: The shim overhead benchmark

**Files:**
- Create: `ci/bench.py`, `ci/test_bench.py`
- Modify: `.github/workflows/parity.yml` (a `bench` job)

**Interfaces:**
- Produces:
  - `bench.summary(results: list[dict]) -> str`: a Markdown table from hyperfine's JSON `results`, with the overhead line
  - `python ci/bench.py --rpyenv <release dir> --python <python executable> [--runs N]`

- [ ] **Step 1: Write the failing unit test**

`ci/test_bench.py`:

```python
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bench  # noqa: E402


class Summary(unittest.TestCase):
    def test_reports_means_and_the_overhead(self):
        text = bench.summary([
            {"command": "through the shim", "mean": 0.0300, "stddev": 0.0010},
            {"command": "directly", "mean": 0.0250, "stddev": 0.0008},
        ])
        self.assertIn("| through the shim | 30.0 ms | ± 1.0 ms |", text)
        self.assertIn("| directly | 25.0 ms | ± 0.8 ms |", text)
        self.assertIn("Shim overhead: 5.0 ms (20.0%)", text)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run it to see it fail**

Run: `python -m unittest discover -s ci -p "test_bench.py"`
Expected: error, `No module named 'bench'`.

- [ ] **Step 3: Implement `ci/bench.py`**

```python
"""Spec §12.6: the shim's overhead, `python -c pass` through the shim vs directly, timed with
hyperfine and posted in the CI summary. Reports only; it never fails on a number.

  python ci/bench.py --rpyenv target/release --python <python executable> [--runs N]

The python's installation becomes version `bench` of a fresh root (a symlink on Linux, a
junction on Windows), `pyenv rehash` makes its shims, and hyperfine times both commands.
"""
import argparse
import json
import os
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
    shim = os.path.join(root, "shims", "python" + exe)
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
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

- [ ] **Step 4: Add the job**

In `.github/workflows/parity.yml`, add:

```yaml
  bench:
    name: shim overhead (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-2025]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02
        with:
          toolchain: "1.96"
      - uses: actions/setup-python@ece7cb06caefa5fff74198d8649806c4678c61a1 # v6
        id: py
        with:
          python-version: "3.12"
      - uses: taiki-e/install-action@83ac0ad63c0167e6f06796fab0fce28db1bf3db0 # v2
        with:
          tool: hyperfine
      - run: cargo build --release --workspace
      - run: python ci/bench.py --rpyenv target/release --python "${{ steps.py.outputs.python-path }}"
```

Also add `python3 -m unittest discover -s ci -p "test_*.py" -v` to the `coverage` job, after its parity unit tests.

- [ ] **Step 5: Run it locally**

1. Install hyperfine if it's missing: `cargo install hyperfine --locked`, or a release binary. Say which in the report.
2. Run `cargo build --release --workspace`.
3. On Windows, run `python ci/bench.py --rpyenv target/release --python <a real python.exe>`. Use the venv's base python (`python -c "import sys; print(sys._base_executable)"`), not a pyenv-win shim.
4. Do the same in WSL with `/usr/bin/python3`.

Expected: a table and an overhead line on both OSes. Paste both into the report. The numbers are informational only.

- [ ] **Step 6: Commit**

```bash
git add ci/bench.py ci/test_bench.py .github/workflows/parity.yml
git commit -m "Time the shim's overhead with hyperfine and post it in the CI summary"
```

---

### Task 8: Items carried from M1c-a

**Files:**
- Modify:
  - `ci/shim_deps.py` (show cargo's stderr)
  - `crates/pyenv/src/commands/which.rs`, `crates/rpyenv-core/src/shimset.rs` (wrap comment lines over 100 columns)
  - `docs/specs/2026-09-27-rpyenv-design.md` (§8 and §11 wording)
  - `docs/parity/allowlist.md` (D-48: move the debug-log note into the rpyenv column)
  - `crates/pyenv/tests/cli_exec.rs` (`exec_debug_log_lines_name_pyenv_exec`: exact exit code per OS)

- [ ] **Step 1: `ci/shim_deps.py`**

Replace the `subprocess.run(…, check=True, capture_output=True, …)` call with one that keeps stderr. On a non-zero exit, print `cargo tree failed for <shim>:` followed by cargo's stderr, then return 1. Extend `self_test` only if the change adds a pure function. Run `python ci/shim_deps.py --self-test` and `python ci/shim_deps.py`.

- [ ] **Step 2: Wording**

1. **Spec §11, Encoding bullet:** reword the opening as "rpyenv's stdout and stderr, when they go to a pipe, a file or NUL, are in the console's output code page …". Keep the rest.
2. **Spec §8, Linux bullet:** replace "the copy" with "the template" wherever it means `shims/.template/pyenv-shim`, so it can't be read as the per-shim copy fallback.
3. **D-48:** move the sentence "`RPYENV_DEBUG_LOG` is always UTF-8" from the Reason column to the rpyenv column.
4. **Long comments:** wrap the comment lines over 100 columns in `which.rs` and `shimset.rs`. Find them with `python -c` over the two files, printing the line numbers of lines longer than 100 characters.

- [ ] **Step 3: The `cli_exec` exit code**

In `exec_debug_log_lines_name_pyenv_exec`, replace the `assert_ne!(r.code, 0)`-style assertion with `assert_eq!(r.code, if cfg!(windows) { 1 } else { 127 });`. Those are the exit codes measured by the M1c-a final review: on Linux, `pyenv exec nosuchcmd` exits 127, and on Windows the not-found report exits 1. Run `cargo test -p pyenv --test cli_exec` on Windows and in WSL.

- [ ] **Step 4: Commit**

```bash
git add ci/shim_deps.py crates docs
git commit -m "Show cargo's error in the shim guard and tidy wording carried from M1c-a"
```

---

## After this plan

- **M1 is complete.** Every M1 command has parity tests: the upstream suites, differential cases and the allowlist coverage check. The shim's overhead is reported on every PR.
- **M2 (installer):**
  - `install`, `uninstall`, `update`, `latest` and `install --list`;
  - the three installer test tiers (spec §12.5);
  - the `M2` entries in both expected-failure lists, which must shrink as those commands land.
- **Carried, low priority:**
  - the stdin→NUL test and the `window_station_visible` false-branch test (M1c-a Decision 6);
  - the shim and `pyenv exec` disagreeing when `PATHEXT` is unset (Decision 8).
