import contextlib
import io
import unittest
from unittest import mock

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


class Slug(unittest.TestCase):
    def test_lowercase_with_runs_of_other_characters_as_one_dash(self):
        self.assertEqual(diff.slug("--version"), "-version")
        self.assertEqual(diff.slug("version-file-read of a missing file"), "version-file-read-of-a-missing-file")
        self.assertEqual(diff.slug("Local  writes: the (file)"), "local-writes-the-file-")


class Placeholders(unittest.TestCase):
    def test_round_trip(self):
        root, work = "C:\\fx\\pyenv-win", "C:\\fx\\work"
        data = b"C:\\fx\\pyenv-win\\versions\r\nin C:\\fx\\work\r\nplain\r\n"
        out = diff.to_placeholders(data, root, work)
        self.assertEqual(out, b"{root}\\versions\r\nin {work}\r\nplain\r\n")
        self.assertEqual(diff.from_placeholders(out, root, work), data)

    def test_a_root_that_is_a_prefix_of_the_work_path(self):
        root, work = "/fx/r", "/fx/r/work"
        data = b"/fx/r/work/x and /fx/r/y"
        out = diff.to_placeholders(data, root, work)
        self.assertEqual(out, b"{work}/x and {root}/y")
        self.assertEqual(diff.from_placeholders(out, root, work), data)


class Expand(unittest.TestCase):
    def test_versions_and_the_system_root(self):
        with mock.patch.dict(diff.os.environ, {"SystemRoot": r"Q:\Win"}):
            self.assertEqual(diff.expand(r"{v0} {v1} {sysroot}\System32"),
                             f"{diff.VERSIONS[0]} {diff.VERSIONS[1]} " + r"Q:\Win\System32")


class Judge(unittest.TestCase):
    root, work = "/fx/root", "/fx/work"
    golden = (0, b"{root}\n", b"", b"")

    def judge(self, rpyenv, golden, same=False):
        return diff.judge(same, ["D-12"], rpyenv, golden, self.root, self.work)

    def test_a_matching_golden_is_allowed(self):
        self.assertEqual(self.judge((0, b"/fx/root\n", b"", b""), self.golden), ("allowed", []))

    def test_a_mismatching_golden_is_a_failure(self):
        verdict, problems = self.judge((1, b"/fx/root/versions\n", b"", b""), self.golden)
        self.assertEqual(verdict, "differs")
        self.assertEqual(len(problems), 2)
        self.assertIn("exit code", problems[0])
        self.assertIn("stdout", problems[1])

    def test_a_missing_golden_is_a_failure_naming_the_flag(self):
        verdict, problems = self.judge((0, b"/fx/root\n", b"", b""), None)
        self.assertEqual(verdict, "differs")
        self.assertIn("--update-golden", problems[0])

    def test_the_golden_is_only_consulted_for_allowed_differences(self):
        self.assertEqual(self.judge((9, b"junk", b"", b""), None, same=True), ("stale", []))
        self.assertEqual(diff.judge(False, [], (9, b"", b"", b""), None, self.root, self.work), ("differs", []))


class Main(unittest.TestCase):
    def run_main(self, argv, cases):
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(diff, "check_inputs", return_value=None), \
                mock.patch.object(diff_cases, "CASES", cases), \
                contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = diff.main(["--rpyenv", "r", "--upstream", "u"] + argv)
        return code, err.getvalue()

    def test_a_run_with_no_cases_fails(self):
        code, err = self.run_main([], ())
        self.assertEqual(code, 1)
        self.assertIn("no case ran", err)

    def test_an_only_that_matches_nothing_fails(self):
        code, err = self.run_main(["--only", "typo"], (diff_cases.Case("real", ("version",)),))
        self.assertEqual(code, 1)
        self.assertIn("no case named 'typo'", err)


class Cases(unittest.TestCase):
    def test_names_are_unique_and_rows_exist(self):
        self.assertTrue(diff_cases.CASES)
        names = [c.name for c in diff_cases.CASES]
        self.assertEqual(len(names), len(set(names)))
        slugs = [diff.slug(n) for n in names]
        self.assertEqual(len(slugs), len(set(slugs)))
        table = allowlist.rows()
        for c in diff_cases.CASES:
            self.assertIn(c.os, ("both", "Linux", "Windows"), c.name)
            for row in c.allow:
                self.assertIn(row, table, f"{c.name}: {row}")
            for rel in c.files + tuple((p, "") for p in c.remove + c.readonly + c.compare):
                self.assertTrue(rel[0].split("/")[0] in ("root", "work"), f"{c.name}: {rel[0]}")


if __name__ == "__main__":
    unittest.main()
