
# --- rpyenv overlay: appended to pyenv-win's tests/conftest.py by parity/pyenv_win_run.py ---
# Runs pyenv-win's own suite against rpyenv (spec §12.3). pyenv-win's setup builds each test's
# fake root; rpyenv's binaries then go into its bin folder, and the suite calls bin\pyenv.exe.
# Tests listed in parity/expected/pyenv-win.txt are strict xfails (plan decision 3).
import os as _os
import re as _re
import shutil as _shutil
import sys as _sys

_RPYENV_BIN = _os.environ["RPYENV_BIN"]
_PARITY = _os.environ["RPYENV_PARITY"]
_sys.path.insert(0, _PARITY)
import allowlist as _allowlist  # noqa: E402


@pytest.fixture()
def pyenv_file(bin_path):
    return str(Path(bin_path, "pyenv.exe"))


@pytest.fixture(autouse=True)
def tmp_pyenv(tmp_path, pyenv_path, local_path, bin_path, shims_path, settings, arch):
    settings = settings()
    touch(tmp_path / '.python-version')
    settings['pyenv_path'] = pyenv_path
    settings['local_path'] = local_path
    os.mkdir(pyenv_path)
    os.mkdir(local_path)
    pyenv_setup(settings)
    for f in ("pyenv.exe", "pyenv-shim.exe", "pyenv-shimw.exe"):
        _shutil.copy(_os.path.join(_RPYENV_BIN, f), bin_path)
    prev_cwd = os.getcwd()
    os.chdir(local_path)
    yield
    os.chdir(prev_cwd)


def test_key(nodeid):
    """The nodeid without the session's `PYENV_FORCE_ARCH=…` parameter, so one line matches
    both architectures."""
    key = _re.sub(r"PYENV_FORCE_ARCH=\w+-?", "", nodeid)
    return _re.sub(r"\[\]$", "", key)


def pytest_collection_modifyitems(config, items):
    expected = _allowlist.read_expected(_os.path.join(_PARITY, "expected", "pyenv-win.txt"), 1)
    table = _allowlist.rows()
    problems = []
    for key, reason in expected.items():
        bad = _allowlist.check_reason(reason, "Windows", table)
        if bad:
            problems.append(f"{key}: {bad}")
    seen = set()
    for item in items:
        key = test_key(item.nodeid)
        if key in expected:
            seen.add(key)
            item.add_marker(pytest.mark.xfail(strict=True, reason=expected[key]))
    problems += [f"{k}: listed, but there is no such test" for k in sorted(set(expected) - seen)]
    if problems:
        raise pytest.UsageError("parity/expected/pyenv-win.txt:\n" + "\n".join(problems))
