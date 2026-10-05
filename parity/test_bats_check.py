import io
import contextlib
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


class Skips(unittest.TestCase):
    def test_lists_the_skipped_tests_and_only_those(self):
        self.assertEqual(bats_check.skipped(TAP), [("a.bats", "skipped one")])

    def test_a_log_without_skips_has_none(self):
        self.assertEqual(bats_check.skipped("a.bats\tok 1 x\na.bats\tnot ok 2 y\n"), [])

    def test_a_name_that_merely_contains_skip_is_not_a_skip(self):
        self.assertEqual(bats_check.skipped("a.bats\tok 1 does not # skipx name\n"), [])


class StaleEntries(unittest.TestCase):
    """Review focus 1."""

    results = bats_check.parse_tap(TAP)
    good = {"a.bats | fails here": "D-35 x", "b.bats | fails here": "M4 y"}

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


class Structure(unittest.TestCase):
    """The log itself must be complete: every listed file, each with a full plan."""

    ok = "a.bats\t1..2\na.bats\tok 1 x\na.bats\tnot ok 2 y\n"

    def test_a_complete_log_passes(self):
        self.assertEqual(bats_check.structure_problems(self.ok, ["a.bats"]), [])

    def test_a_missing_file_fails(self):
        (p,) = bats_check.structure_problems(self.ok, ["a.bats", "b.bats"])
        self.assertIn("missing", p)
        self.assertIn("b.bats", p)

    def test_an_extra_file_fails(self):
        (p,) = bats_check.structure_problems(self.ok, [])
        self.assertIn("not listed", p)
        self.assertIn("a.bats", p)

    def test_a_missing_plan_fails(self):
        (p,) = bats_check.structure_problems("a.bats\tok 1 x\n", ["a.bats"])
        self.assertIn("no `1..N` plan", p)

    def test_a_short_count_fails(self):
        (p,) = bats_check.structure_problems("a.bats\t1..3\na.bats\tok 1 x\n", ["a.bats"])
        self.assertIn("plan says 3", p)

    def test_a_load_error_is_an_unlisted_failure(self):
        log = "a.bats\t1..1\na.bats\tnot ok 1 bats-gather-tests\n"
        self.assertEqual(bats_check.structure_problems(log, ["a.bats"]), [])
        (p,) = bats_check.problems(bats_check.parse_tap(log), {}, TABLE)
        self.assertIn("not listed", p)


if __name__ == "__main__":
    unittest.main()


class Suites(unittest.TestCase):
    def test_virtualenv_suite_has_its_own_lists(self):
        exp, files, heading = bats_check.SUITES["virtualenv"]
        self.assertEqual((exp, files), ("bats-virtualenv.txt", "bats-virtualenv-files.txt"))
        self.assertIn("pyenv-virtualenv", heading)
        self.assertEqual(bats_check.SUITES["pyenv"][:2], ("bats.txt", "bats-files.txt"))


class UnknownSuite(unittest.TestCase):
    def test_an_unknown_suite_is_a_usage_error(self):
        with contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(bats_check.main(["--suite", "nope", "x.tap"]), 2)
        self.assertIn("unknown suite", err.getvalue())
