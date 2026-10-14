"""Tests for ci/pypy_tables.py (M7a design §4.1, §5.1). The evaluator tests need bash, so they
run on Linux (CI's coverage job; locally through WSL)."""
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pypy_tables as t  # noqa: E402

SHA = "a" * 64
OTHER = "b" * 64

# The template shape (39 upstream files, pypy3.9-7.3.8 on): variables, then a fixed tail.
TEMPLATE = """VERSION='7.3.99'
PYVER='3.11'
aarch64_hash=%(sha)s
linux64_hash=%(sha)s
function err_no_binary {
    { echo; colorize 1 "ERROR"; echo ": The binary distribution of PyPy is not available for ${1}."; } >&2
    exit 1
}
function pypy_pkg_data {
    local ARCH="${1}" cmd='install_package' pkg="${1}" ext='tar.bz2' hash=''
    case "${ARCH}" in
    'linux-aarch64' ) hash="${aarch64_hash}"; pkg='aarch64' ;;
    'linux64' ) hash="${linux64_hash}" ;;
    * ) err_no_binary "${ARCH}" ;;
    esac
    local basever="pypy${PYVER}-v${VERSION}"
    echo "${cmd}" "${basever}-${pkg}" "https://downloads.python.org/pypy/${basever}-${pkg}.${ext}#${hash}"
}
declare -a pd="$(pypy_pkg_data "$(pypy_architecture 2>/dev/null || true)")"
${pd[0]} "${pd[1]}" "${pd[2]}" 'pypy' "verify_py${PYVER//./}" 'ensurepip'
""" % {"sha": SHA}

# The inline shape (78 upstream files): one install_package per architecture; aarch64 missing.
INLINE = """case "$(pypy_architecture 2>/dev/null || true)" in
"linux64" )
  install_package "pypy3.8-v7.3.99-linux64" "https://downloads.python.org/pypy/pypy3.8-v7.3.99-linux64.tar.bz2#%s" "pypy" "verify_py38" ensurepip
  ;;
* )
  { echo; colorize 1 "ERROR"; echo ": not available"; } >&2
  exit 1
  ;;
esac
""" % SHA


@unittest.skipUnless(shutil.which("bash"), "needs bash")
class Evaluator(unittest.TestCase):
    def verdict(self, files, name, arch="linux64"):
        with tempfile.TemporaryDirectory() as d:
            for n, text in files.items():
                with open(os.path.join(d, n), "w", encoding="utf-8", newline="\n") as f:
                    f.write(text)
            out = subprocess.run(["bash", t.EVAL, os.path.join(d, name), arch],
                                 capture_output=True, text=True, check=True).stdout
        self.assertEqual(out.count("\n"), 1, out)
        return out.rstrip("\n")

    def test_the_template_shape_gives_its_row_per_architecture(self):
        self.assertEqual(
            self.verdict({"pypy3.11-7.3.99": TEMPLATE}, "pypy3.11-7.3.99"),
            f"ok pypy3.11-v7.3.99-linux64 https://downloads.python.org/pypy/pypy3.11-v7.3.99-linux64.tar.bz2#{SHA} pypy verify_py311 ensurepip")
        self.assertEqual(
            self.verdict({"pypy3.11-7.3.99": TEMPLATE}, "pypy3.11-7.3.99", "linux-aarch64"),
            f"ok pypy3.11-v7.3.99-aarch64 https://downloads.python.org/pypy/pypy3.11-v7.3.99-aarch64.tar.bz2#{SHA} pypy verify_py311 ensurepip")

    def test_the_inline_shape_and_upstreams_own_no_binary_error(self):
        self.assertTrue(self.verdict({"pypy3.8-7.3.99": INLINE}, "pypy3.8-7.3.99").startswith("ok pypy3.8-v7.3.99-linux64 "))
        self.assertEqual(self.verdict({"pypy3.8-7.3.99": INLINE}, "pypy3.8-7.3.99", "linux-aarch64"), "no-binary")

    def test_a_definition_that_sources_a_sibling(self):
        files = {"pypy2-5.7.1": INLINE, "pypy-5.7.1": 'source "${BASH_SOURCE%/*}/pypy2-5.7.1"\n'}
        self.assertTrue(self.verdict(files, "pypy-5.7.1").startswith("ok pypy3.8-v7.3.99-linux64 "))

    def test_a_function_the_stubs_lack_is_unsupported(self):
        text = 'install_nightly_package "pypy-c-jit-latest" "https://x/y.tar.bz2" pypy\n'
        self.assertEqual(self.verdict({"pypy-c-jit-latest": text}, "pypy-c-jit-latest"),
                         "unsupported calls install_nightly_package")

    def test_two_packages_or_no_sha256_are_unsupported(self):
        two = f'install_package a "https://x/a.tar.gz#{SHA}" pypy\ninstall_package b "https://x/b.tar.gz#{SHA}" pypy\n'
        self.assertEqual(self.verdict({"pypy-dev": two}, "pypy-dev"), "unsupported 2 packages, exit 0")
        bare = 'install_package a "https://x/a.tar.bz2" pypy\n'
        self.assertEqual(self.verdict({"pypy-x": bare}, "pypy-x"), "unsupported no sha256 in https://x/a.tar.bz2")


@unittest.skipUnless(shutil.which("bash"), "needs bash")
class LinuxTable(unittest.TestCase):
    def test_rows_cover_every_definition_and_architecture_in_order(self):
        with tempfile.TemporaryDirectory() as d:
            for n, text in {"pypy3.11-7.3.99": TEMPLATE, "pypy3.8-7.3.99": INLINE,
                            "pypy3.11-7.3.99-src": "irrelevant\n", "3.12.0": "not pypy\n"}.items():
                with open(os.path.join(d, n), "w", encoding="utf-8", newline="\n") as f:
                    f.write(text)
            names = t.pypy_names(d)
            self.assertEqual(names, ["pypy3.11-7.3.99", "pypy3.11-7.3.99-src", "pypy3.8-7.3.99"])
            rows = t.linux_rows(names, t.evaluate(d, [n for n in names if not n.endswith("-src")], "local"))
        self.assertEqual([(n, a, v.split()[0]) for n, a, v in rows], [
            ("pypy3.11-7.3.99", "linux64", "ok"), ("pypy3.11-7.3.99", "linux-aarch64", "ok"),
            ("pypy3.11-7.3.99-src", "linux64", "unsupported"), ("pypy3.11-7.3.99-src", "linux-aarch64", "unsupported"),
            ("pypy3.8-7.3.99", "linux64", "ok"), ("pypy3.8-7.3.99", "linux-aarch64", "no-binary")])
        text = t.render_linux(rows)
        self.assertTrue(text.startswith("# "), text)
        self.assertIn("pypy3.11-7.3.99-src\tlinux64\tunsupported source build\n", text)


FIXTURES = os.path.join(t.REPO, "crates", "pyenv", "tests", "fixtures", "pypy")


def read(name):
    with open(os.path.join(FIXTURES, name), encoding="utf-8") as f:
        return f.read()


def page(pairs, filler=t.MIN_PAGE_HASHES):
    """A checksum page: `pairs` plus `filler` unrelated entries, in pypy.org's markup."""
    lines = [f"{h}  {n}" for n, h in pairs] + [f"{i:064x}  pypy0.0-v0.0.{i}-filler.tar.bz2" for i in range(filler)]
    return '<html><head><meta content="x"></head><body><p>pypy sha256:</p><pre>' + "\n".join(lines) + "</pre></body></html>"


class Page(unittest.TestCase):
    def test_the_saved_page_parses_into_its_pairs(self):
        # 1021 on 2026-10-14 (C:\tmp\m7_page_count.py); the Rust parser pins the same number.
        self.assertEqual(len(t.page_hashes(read("checksums.html"))), 1021)

    def test_tags_split_words_and_two_values_for_one_file_fail(self):
        self.assertEqual(t.page_hashes(page([("pypy3.11-v7.3.99-win64.zip", SHA)]))["pypy3.11-v7.3.99-win64.zip"], SHA)
        with self.assertRaisesRegex(ValueError, "two SHA-256 values"):
            t.page_hashes(page([("pypy3.11-v7.3.99-win64.zip", SHA), ("pypy3.11-v7.3.99-win64.zip", OTHER)]))

    def test_a_page_with_few_pairs_means_the_layout_changed(self):
        with self.assertRaisesRegex(ValueError, "fewer than 500"):
            t.page_hashes(page([("pypy3.11-v7.3.99-win64.zip", SHA)], filler=10))


class Windows(unittest.TestCase):
    def test_the_saved_files_give_pyenv_win_codes_in_pyenv_wins_order(self):
        rows = t.windows_rows(read("versions.json"), t.page_hashes(read("checksums.html")))
        self.assertEqual(len(rows), 57)
        codes = [c for c, _, _ in rows]
        self.assertLess(codes.index("pypy3.10-v7.3.19-win64"), codes.index("pypy3.11-v7.3.18-win64"))
        self.assertLess(codes.index("pypy2.7-v7.3.9-win64"), codes.index("pypy2.7-v7.3.10-win64"))
        self.assertTrue(all(u == f"https://downloads.python.org/pypy/{c}.zip" for c, u, _ in rows))

    def test_a_file_without_a_published_hash_gets_no_row(self):
        vj = '[{"pypy_version":"7.3.99","python_version":"3.11.9","files":[' \
             '{"filename":"pypy3.11-v7.3.99-win64.zip","arch":"x64","platform":"win64","download_url":"https://downloads.python.org/pypy/pypy3.11-v7.3.99-win64.zip"},' \
             '{"filename":"pypy-c-jit-latest-win64.zip","arch":"x64","platform":"win64","download_url":"https://downloads.python.org/pypy/pypy-c-jit-latest-win64.zip"}]}]'
        self.assertEqual(t.windows_rows(vj, {}), [])
        self.assertEqual(t.windows_rows(vj, {"pypy3.11-v7.3.99-win64.zip": SHA}),
                         [("pypy3.11-v7.3.99-win64", "https://downloads.python.org/pypy/pypy3.11-v7.3.99-win64.zip", SHA)])

    def test_a_download_url_off_pypys_folder_fails(self):
        vj = '[{"files":[{"filename":"pypy3.11-v7.3.99-win64.zip","download_url":"https://evil.example/pypy3.11-v7.3.99-win64.zip"}]}]'
        with self.assertRaisesRegex(ValueError, "not under https://downloads.python.org/pypy/"):
            t.windows_rows(vj, {"pypy3.11-v7.3.99-win64.zip": SHA})


class CrossCheck(unittest.TestCase):
    URL = "https://downloads.python.org/pypy/pypy3.11-v7.3.99-linux64.tar.bz2"

    def rows(self, sha=SHA, extra=()):
        return [("pypy3.11-7.3.99", "linux64", f"ok pypy3.11-v7.3.99-linux64 {self.URL}#{sha} pypy verify_py311 ensurepip"),
                ("pypy3.11-7.3.99", "linux-aarch64", "no-binary"), *extra]

    def test_a_published_hash_must_match(self):
        sums = {"pypy3.11-v7.3.99-linux64.tar.bz2": SHA}
        self.assertEqual(t.check_linux(self.rows(), sums, set(), set()), [])
        errs = t.check_linux(self.rows(OTHER), sums, set(), set())
        self.assertEqual(len(errs), 1)
        self.assertIn("differs from pypy.org's", errs[0])

    def test_a_file_missing_from_the_page_must_be_listed(self):
        errs = t.check_linux(self.rows(), {}, set(), set())
        self.assertIn("not in linux-unpublished.txt", errs[0])
        self.assertEqual(t.check_linux(self.rows(), {}, {"pypy3.11-v7.3.99-linux64.tar.bz2"}, set()), [])

    def test_an_unexpected_stub_gap_fails_and_stale_list_entries_fail(self):
        gap = [("pypy-dev", "linux64", "unsupported calls install_nightly_package")]
        sums = {"pypy3.11-v7.3.99-linux64.tar.bz2": SHA}
        self.assertIn("add a stub, or list it", t.check_linux(self.rows(extra=gap), sums, set(), set())[0])
        self.assertEqual(t.check_linux(self.rows(extra=gap), sums, set(), {"pypy-dev"}), [])
        stale = t.check_linux(self.rows(), sums, {"gone.tar.bz2"}, {"pypy-gone"})
        self.assertEqual(sorted(e.split(":")[0] for e in stale), ["linux-unpublished.txt", "linux-unsupported.txt"])


def engine():
    for e in ("docker", "podman"):
        if shutil.which(e) and subprocess.run([e, "info"], capture_output=True).returncode == 0:
            return e
    return None


@unittest.skipUnless(engine(), "needs docker or podman")
class Container(unittest.TestCase):
    def test_the_container_has_no_network(self):
        # Reaching any address would make the definition install; with --network none it can't.
        text = ('if (exec 3<>/dev/tcp/1.1.1.1/53) 2>/dev/null; then\n'
                f'  install_package "net" "https://x/net.tar.bz2#{SHA}" pypy\n'
                'else exit 1; fi\n')
        with tempfile.TemporaryDirectory() as d:
            with open(os.path.join(d, "pypy-net"), "w", encoding="utf-8", newline="\n") as f:
                f.write(text)
            verdicts = t.evaluate(d, ["pypy-net"], engine())
        self.assertEqual(verdicts[("pypy-net", "linux64")], "no-binary")


if __name__ == "__main__":
    unittest.main()
