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


class Bounds(unittest.TestCase):
    up = (1, b"no such command\r\n", b"", b"")

    def test_a_well_bounded_difference(self):
        rp = (0, b"C:\\root\r\n", b"", b"")
        self.assertEqual(diff.bound_failures(("code", "stdout"), b"C:\\root", self.up, rp), [])

    def test_an_out_of_bounds_field(self):
        rp = (0, b"C:\\root\r\n", b"", b"")
        failures = diff.bound_failures(("stdout",), b"C:\\root", self.up, rp)
        self.assertEqual(len(failures), 1)
        self.assertIn("code", failures[0])
        rp = (1, b"x\r\n", b"oops", b"")
        self.assertIn("stderr", " ".join(diff.bound_failures(("stdout",), b"", self.up, rp)))
        rp = (1, b"x\r\n", b"", b"changed")
        self.assertIn("files", " ".join(diff.bound_failures(("stdout",), b"", self.up, rp)))

    def test_a_missing_substring(self):
        rp = (0, b"something else\r\n", b"", b"")
        failures = diff.bound_failures(("code", "stdout"), b"C:\\root", self.up, rp)
        self.assertEqual(len(failures), 1)
        self.assertIn("lacks", failures[0])

    def test_the_substring_may_be_in_any_allowed_stream(self):
        rp = (1, b"", b"(rpyenv 0.1)", b"")
        self.assertEqual(diff.bound_failures(("stdout", "stderr"), b"(rpyenv ", (1, b"a", b"", b""), rp), [])


class Cases(unittest.TestCase):
    def test_names_are_unique_and_rows_exist(self):
        self.assertTrue(diff_cases.CASES)
        names = [c.name for c in diff_cases.CASES]
        self.assertEqual(len(names), len(set(names)))
        table = allowlist.rows()
        for c in diff_cases.CASES:
            self.assertIn(c.os, ("both", "Linux", "Windows"), c.name)
            for row in c.allow:
                self.assertIn(row, table, f"{c.name}: {row}")
            if c.allow:
                self.assertTrue(c.may_differ and set(c.may_differ) <= set(diff.FIELDS), c.name)
            for rel in c.files + tuple((p, "") for p in c.remove + c.readonly + c.compare):
                self.assertTrue(rel[0].split("/")[0] in ("root", "work"), f"{c.name}: {rel[0]}")


if __name__ == "__main__":
    unittest.main()
