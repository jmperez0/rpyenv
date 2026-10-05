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


class Malformed(unittest.TestCase):
    def test_a_malformed_row_is_an_error(self):
        for bad in ("| D-100 | Linux | x |", "| D-02 | linux | x |", "| D-03 | Both | x |"):
            with self.assertRaises(ValueError, msg=bad):
                allowlist.rows(write(SAMPLE + bad + "\n"))


class Reasons(unittest.TestCase):
    """Review focus 3."""

    table = {"D-01": "both", "D-12": "Windows", "D-35": "Linux"}

    def test_a_row_for_this_os_or_both_is_accepted(self):
        self.assertIsNone(allowlist.check_reason("D-35 the lock", "Linux", self.table))
        self.assertIsNone(allowlist.check_reason("D-01 fixed line", "Windows", self.table))

    def test_a_delivered_milestone_is_rejected(self):
        self.assertIsNone(allowlist.check_reason("M2 not yet", "Linux", self.table))
        old = allowlist.DELIVERED
        allowlist.DELIVERED = old + ("M2",)
        try:
            msg = allowlist.check_reason("M2 not yet", "Linux", self.table)
        finally:
            allowlist.DELIVERED = old
        self.assertIn("M2 is delivered", msg)
        self.assertIn("pyenv-<cmd> wrapper", msg)

    def test_m2a_and_m2b_are_delivered_m2_is_not(self):
        msg = allowlist.check_reason("M2a installer", "Linux", self.table)
        self.assertIn("M2a is delivered", msg)
        msg = allowlist.check_reason("M2b Windows installer", "Windows", self.table)
        self.assertIn("M2b is delivered", msg)
        self.assertIsNone(allowlist.check_reason("M2 x", "Linux", self.table))

    def test_m1_is_delivered_and_rejected(self):
        self.assertIn("M1 is delivered", allowlist.check_reason("M1 x", "Linux", self.table))

    def test_a_milestone_is_accepted(self):
        self.assertIsNone(allowlist.check_reason("M4 virtualenvs and plugins", "Linux", self.table))

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


class M4Milestones(unittest.TestCase):
    def test_m4b_is_a_reason_tag_and_m4a_is_delivered(self):
        table = allowlist.rows()
        self.assertIsNone(allowlist.check_reason("M4b not built yet", "Linux", table))
        self.assertIn("M4a", allowlist.DELIVERED)
