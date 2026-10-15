"""Tests for ci/conda_tables.py and crates/pyenv/src/install/conda_eval.sh (M7b design §4.1)."""
import os
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import conda_tables as ct  # noqa: E402

SHA = "a" * 64
CASE = '''case "$(anaconda_architecture 2>/dev/null || true)" in
"Linux-x86_64" )
  install_script "Miniforge3-24.1.2-0-Linux-x86_64" "https://github.com/conda-forge/miniforge/releases/download/24.1.2-0/Miniforge3-24.1.2-0-Linux-x86_64.sh#{sha}" "miniconda" verify_py312
  ;;
* )
  {{ echo
    colorize 1 "ERROR"
    echo ": The binary distribution of Miniforge3 is not available for $(anaconda_architecture 2>/dev/null || true)."
    echo
  }} >&2
  exit 1
  ;;
esac
'''


def bash_works():
    try:
        return subprocess.run(["bash", "-c", "true"], capture_output=True).returncode == 0 and os.name != "nt"
    except OSError:
        return False


@unittest.skipUnless(bash_works(), "needs bash (run in WSL)")
class Evaluator(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()

    def write(self, name, text):
        with open(os.path.join(self.dir, name), "w", encoding="utf-8", newline="\n") as f:
            f.write(text)

    def eval(self, name, arch):
        out = subprocess.run(["bash", ct.EVAL, os.path.join(self.dir, name), arch],
                             capture_output=True, text=True, check=True).stdout
        return out.strip()

    def test_hashed_definition(self):
        self.write("miniforge3-24.1.2-0", CASE.format(sha=SHA))
        self.assertEqual(self.eval("miniforge3-24.1.2-0", "Linux-x86_64"),
                         "ok Miniforge3-24.1.2-0-Linux-x86_64 https://github.com/conda-forge/miniforge/releases/download/24.1.2-0/"
                         f"Miniforge3-24.1.2-0-Linux-x86_64.sh#{SHA} miniconda verify_py312")

    def test_no_binary_names_what(self):
        self.write("miniforge3-24.1.2-0", CASE.format(sha=SHA))
        self.assertEqual(self.eval("miniforge3-24.1.2-0", "Linux-aarch64"), "no-binary Miniforge3")

    def test_source_alias(self):
        self.write("miniforge3-24.1.2-0", CASE.format(sha=SHA))
        self.write("miniforge-24.1.2-0", 'source "${BASH_SOURCE%/*}/miniforge3-24.1.2-0"\n')
        self.assertTrue(self.eval("miniforge-24.1.2-0", "Linux-x86_64").startswith("ok Miniforge3-24.1.2-0-Linux-x86_64 "))

    def test_exported_variable_is_recorded(self):
        self.write("miniconda3-25.1.1-0", "export CONDA_PLUGINS_AUTO_ACCEPT_TOS=true\n" + CASE.format(sha=SHA))
        self.assertTrue(self.eval("miniconda3-25.1.1-0", "Linux-x86_64").endswith(" verify_py312 CONDA_PLUGINS_AUTO_ACCEPT_TOS=true"))

    def test_unhashed_url_is_reported_as_ok_without_hash(self):
        self.write("miniconda3-latest", CASE.format(sha=SHA).replace(f"#{SHA}", ""))
        v = self.eval("miniconda3-latest", "Linux-x86_64")
        self.assertTrue(v.startswith("ok ") and "#" not in v, v)

    def test_unknown_function_is_unsupported(self):
        self.write("anaconda3-x", "install_something_else foo\n")
        self.assertEqual(self.eval("anaconda3-x", "Linux-x86_64"), "unsupported calls install_something_else")

    def test_other_step_is_unsupported(self):
        self.write("anaconda3-y", CASE.format(sha=SHA).replace('"miniconda"', '"pypy"'))
        self.assertEqual(self.eval("anaconda3-y", "Linux-x86_64"), "unsupported step pypy")


class Classify(unittest.TestCase):
    URL = "https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-x86_64.sh"

    def test_hashed_known_host_stays_ok(self):
        v = f"ok P {self.URL}#{SHA} miniconda verify_py3_latest"
        self.assertEqual(ct.classify("miniconda3-x", v), v)

    def test_unhashed_latest_name_is_ok_latest(self):
        self.assertEqual(ct.classify("miniconda3-latest", f"ok P {self.URL} miniconda verify_py3_latest"),
                         f"ok-latest P {self.URL} miniconda verify_py3_latest")

    def test_unhashed_other_name_has_no_published_hash(self):
        self.assertEqual(ct.classify("mambaforge", f"ok P {self.URL} miniconda verify_py3_latest"),
                         "unsupported no published hash")

    def test_unknown_host(self):
        self.assertEqual(ct.classify("anaconda3-z", f"ok P https://example.com/a.sh#{SHA} anaconda verify_py312"),
                         "unsupported unknown host")

    def test_bad_exported_value(self):
        self.assertEqual(ct.classify("miniconda3-x", f"ok P {self.URL}#{SHA} miniconda verify_py312 A=b;c"),
                         "unsupported exported value rpyenv can't carry: A=b;c")

    def test_other_verdicts_pass_through(self):
        self.assertEqual(ct.classify("x", "no-binary Anaconda"), "no-binary Anaconda")


class Check(unittest.TestCase):
    def ok(self, arch, file):
        return f"ok P https://repo.anaconda.com/archive/{file}#{SHA} anaconda verify_py312"

    def test_arch_must_match_the_file(self):
        rows = [("a", "Linux-x86_64", self.ok("Linux-x86_64", "Anaconda3-1-Linux-aarch64.sh")),
                ("a", "Linux-aarch64", self.ok("Linux-aarch64", "Anaconda3-1-Linux-x86_64.sh"))]
        errors = ct.check(rows, minimum=0)
        self.assertEqual(len(errors), 2, errors)

    def test_minimum_ok_rows(self):
        rows = [("a", "Linux-x86_64", self.ok("Linux-x86_64", "Anaconda3-1-Linux-x86_64.sh"))]
        self.assertEqual(ct.check(rows, minimum=1), [])
        self.assertTrue(ct.check(rows, minimum=2)[0].startswith("1 ok rows, fewer than 2"))


if __name__ == "__main__":
    unittest.main()
