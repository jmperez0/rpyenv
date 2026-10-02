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
