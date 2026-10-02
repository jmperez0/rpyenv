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


class InstalledEnv(unittest.TestCase):
    """The suite's `run` fixture finds pyenv on PATH only through PYENV/PYENV_ROOT/PYENV_HOME,
    as upstream's CI sets them; a fresh runner has none (first CI run: 42 failures)."""

    def test_names_the_install_and_puts_it_first_on_path(self):
        import sys
        sys.path.insert(0, HERE)
        from pyenv_win_run import installed_env
        home = os.path.join("D:" + os.sep, "w", "pyenv-win")
        host = os.path.join("C:" + os.sep, "Users", "me", ".pyenv", "pyenv-win")
        base = {
            "PYENV": host + os.sep,
            "PATH": os.pathsep.join([os.path.join(host, "bin"), "X", os.path.join(host, "shims"), "Y"]),
        }
        env = installed_env(base, home)
        self.assertEqual({env[k] for k in ("PYENV", "PYENV_ROOT", "PYENV_HOME")}, {home})
        self.assertEqual(
            env["PATH"].split(os.pathsep),
            [os.path.join(home, "bin"), os.path.join(home, "shims"), "X", "Y"],
        )

    def test_a_host_without_an_install_keeps_its_path(self):
        import sys
        sys.path.insert(0, HERE)
        from pyenv_win_run import installed_env
        env = installed_env({"PATH": "X"}, "H")
        self.assertEqual(env["PATH"].split(os.pathsep), [os.path.join("H", "bin"), os.path.join("H", "shims"), "X"])
