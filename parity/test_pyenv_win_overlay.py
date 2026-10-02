import ast
import os
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))


def load_test_key():
    """`test_key` from the overlay, without importing the rest of it: the overlay is appended
    to pyenv-win's conftest.py, where `pytest` and its fixtures are in scope."""
    with open(os.path.join(HERE, "pyenv_win_overlay.py"), encoding="utf-8") as f:
        tree = ast.parse(f.read())
    fn = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == "test_key")
    scope = {"_re": __import__("re")}
    exec(compile(ast.Module(body=[fn], type_ignores=[]), "overlay", "exec"), scope)
    return scope["test_key"]


class Keys(unittest.TestCase):
    """Review focus 1: one expected-failure line matches both architectures."""

    def test_the_arch_parameter_is_dropped(self):
        key = load_test_key()
        self.assertEqual(
            key("test_pyenv_feature_exec.py::test_exec_arg[PYENV_FORCE_ARCH=AMD64-Two Words-python shim]"),
            "test_pyenv_feature_exec.py::test_exec_arg[Two Words-python shim]",
        )
        self.assertEqual(key("t.py::test_latest_quiet[PYENV_FORCE_ARCH=X86]"), "t.py::test_latest_quiet")
        self.assertEqual(key("t.py::test_bad_path[PYENV_FORCE_ARCH=X86-<lambda>]"), "t.py::test_bad_path[<lambda>]")


if __name__ == "__main__":
    unittest.main()
