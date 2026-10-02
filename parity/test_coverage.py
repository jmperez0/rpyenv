import contextlib
import io
import os
import tempfile
import textwrap
import unittest

import allowlist
import coverage
import diff_cases


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


class Matching(unittest.TestCase):
    def test_only_the_allowlist_wording_counts(self):
        d = crate({"a/t.rs": "// unlike D-07\n#[test]\nfn a() {}\n"})
        self.assertEqual(coverage.rust_test_citations(d), {})
        d = crate({"a/t.rs": "// (allowlist D-07)\n#[test]\nfn a() {}\n"})
        self.assertEqual(list(coverage.rust_test_citations(d)), ["D-07"])

    def test_a_one_line_test_fn_counts(self):
        d = crate({"a/t.rs": "// allowlist D-07\n#[test] fn a() {}\n"})
        self.assertEqual(list(coverage.rust_test_citations(d)), ["D-07"])


class Split(unittest.TestCase):
    """Covered rows by source: execution can fail if wrong, a test citation can't (yet)."""

    def test_execution_wins_then_a_citation_then_a_waiver(self):
        ex, ci, wa = coverage.EXECUTION, coverage.CITATION, coverage.WAIVED
        table = {f"D-0{n}": "both" for n in range(1, 7)}
        cited = {
            "D-01": [(ci, "test a"), (ex, "diff case 'x'")],
            "D-02": [(ci, "test b")],
            "D-03": [(wa, "untestable: why")],
            "D-04": [(wa, "untestable: why"), (ci, "test c")],
            "D-05": [(ex, "expected/bats.txt: k"), (wa, "untestable: why")],
            "D-99": [(ex, "diff case 'y'")],
        }
        self.assertEqual(coverage.split(table, cited),
                         {ex: ["D-01", "D-05"], ci: ["D-02", "D-04"], wa: ["D-03"]})

    def test_each_source_is_tagged_with_its_kind(self):
        cited = coverage.citations()
        for case in diff_cases.CASES:
            for row in case.allow:
                self.assertIn((coverage.EXECUTION, f"diff case {case.name!r}"), cited[row])
        for row, wheres in coverage.rust_test_citations(os.path.join(allowlist.REPO, "crates")).items():
            for where in wheres:
                self.assertIn((coverage.CITATION, f"test {where}"), cited[row])
        for row, entries in cited.items():
            for kind, where in entries:
                if where.startswith("expected/"):
                    self.assertEqual(kind, coverage.EXECUTION, where)
                if where.startswith("untestable:"):
                    self.assertEqual(kind, coverage.WAIVED, where)


class Main(unittest.TestCase):
    def test_report_mode_never_fails(self):
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(coverage.main(["--report"]), 0)

    def test_the_headline_and_the_summary_give_the_split(self):
        summary = os.path.join(tempfile.mkdtemp(), "summary.md")
        old = os.environ.get("GITHUB_STEP_SUMMARY")
        os.environ["GITHUB_STEP_SUMMARY"] = summary
        try:
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                coverage.main(["--report"])
        finally:
            if old is None:
                del os.environ["GITHUB_STEP_SUMMARY"]
            else:
                os.environ["GITHUB_STEP_SUMMARY"] = old
        split = r"\d+ by execution, \d+ by test citation, \d+ waived"
        self.assertRegex(out.getvalue().splitlines()[0],
                         rf"^allowlist rows: \d+; covered: \d+ \({split}\)$")
        with open(summary, encoding="utf-8") as f:
            self.assertRegex(f.read(), rf"\d+ of \d+ rows covered: {split}\.")

    def test_enforced_mode_fails_on_a_gap_or_a_dangling_citation(self):
        table = {"D-01": "both", "D-02": "both"}
        self.assertEqual(coverage.verdict(table, {"D-01": ["x"], "D-02": ["y"]}, False)[0], 0)
        self.assertEqual(coverage.verdict(table, {"D-01": ["x"]}, False), (1, ["D-02"], []))
        full = {"D-01": ["x"], "D-02": ["y"], "D-99": ["z"]}
        self.assertEqual(coverage.verdict(table, full, False), (1, [], ["D-99"]))
        self.assertEqual(coverage.verdict(table, {}, True)[0], 0)


if __name__ == "__main__":
    unittest.main()
