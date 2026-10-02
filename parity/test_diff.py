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
