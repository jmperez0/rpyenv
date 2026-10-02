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
