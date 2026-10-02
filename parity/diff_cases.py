"""Differential cases (spec §12.4), run by parity/diff.py against rpyenv and the real upstream
on identical fixture roots. `allow` names the allowlist rows that explain an expected
difference; a row whose OS doesn't match the run is ignored on that OS. A case with no
applicable row must match byte for byte; a case with one must differ."""
from dataclasses import dataclass


@dataclass(frozen=True)
class Case:
    name: str
    args: tuple
    os: str = "both"  # "both", "Linux" or "Windows"
    env: tuple = ()  # (name, value) pairs; value None removes the variable; {v0}/{v1} expand
    files: tuple = ()  # (path, content) pairs; path "root/…" or "work/…"; {v0}/{v1} expand
    remove: tuple = ()  # paths removed after the fixture is built
    readonly: tuple = ()  # paths made read-only after the fixture is built
    compare: tuple = ()  # paths whose bytes are compared after the run
    allow: tuple = ()  # allowlist rows explaining a difference


CASES = (
    Case("--version", ("--version",), allow=("D-01",)),
    Case("root", ("root",), allow=("D-12",)),
    Case("versions", ("versions",)),
    Case("versions --bare", ("versions", "--bare")),
    Case("version", ("version",)),
    Case("version-name", ("version-name",)),
    Case("version-origin", ("version-origin",), allow=("D-12",)),
    Case("version-file", ("version-file",), allow=("D-12",)),
    Case("global", ("global",)),
    Case("local with no local file", ("local",)),
    Case("which python", ("which", "python")),
    Case("whence python", ("whence", "python")),
    Case("prefix", ("prefix",), allow=("D-12",)),
    Case("shims", ("shims",)),
    Case("commands", ("commands",), allow=("D-16", "D-49")),
    Case("help", ("help",), allow=("D-08", "D-49")),
    Case("version with PYENV_VERSION not installed", ("version",), env=(("PYENV_VERSION", "9.9"),)),
    Case("version-name with PYENV_VERSION not installed", ("version-name",), env=(("PYENV_VERSION", "9.9"),)),
    Case("which for a missing command", ("which", "nosuch")),
    Case("exec for a missing command", ("exec", "nosuch")),
    Case("local with a version not installed", ("local", "9.9"), compare=("work/.python-version",)),
    Case("global with a version not installed", ("global", "9.9"), compare=("root/version",)),
    Case("version-file-read of a missing file", ("version-file-read", "missing"), allow=("D-12",)),
    Case("no arguments", (), allow=("D-01", "D-20", "D-49")),
    Case("an unknown command", ("frobnicate",)),
    Case("local writes the version file", ("local", "{v1}"), compare=("work/.python-version",)),
    Case("global writes the version file", ("global", "{v1}"), compare=("root/version",)),
    # The command-level allowlist rows, each with the trigger from its Upstream cell. Fixture
    # paths are literal: `files` expands {v0}/{v1} in contents, not in paths.
    # D-02: --version warns when PYENV, PYENV_ROOT or PYENV_HOME is unset.
    Case("--version without PYENV, PYENV_ROOT and PYENV_HOME", ("--version",), os="Windows",
         env=(("PYENV", None), ("PYENV_ROOT", None), ("PYENV_HOME", None)), allow=("D-01", "D-02")),
    # D-03: a leading UTF-8 BOM becomes part of the first version name.
    Case("version-name with a BOM in the local file", ("version-name",),
         files=(("work/.python-version", "\ufeff{v1}\n"),), allow=("D-03",)),
    # D-04: a word longer than 1024 characters is split in two (`read -n 1024`).
    Case("version-name with a word longer than 1024 characters", ("version-name",), os="Linux",
         files=(("work/.python-version", "a" * 1500 + "\n"),), allow=("D-04",)),
    # D-05: two installed versions equal in major, minor and patch raise a VBScript runtime error.
    Case("which python with 3.8 and 3.8.0 installed", ("which", "python"), os="Windows",
         files=(("root/versions/3.8/python.exe", ""), ("root/versions/3.8.0/python.exe", ""),
                ("work/.python-version", "3.8\n")), allow=("D-05",)),
    # D-06: prefix ties are broken by `sort`'s locale collation (en_US puts `a` before `B`).
    # The en_US.UTF-8 cases need that locale; without it they measure `stale` and fail.
    Case("version-name with a prefix tie in en_US.UTF-8", ("version-name",), os="Linux",
         env=(("LANG", "en_US.UTF-8"),),
         files=(("root/versions/3.13.0-a/bin/python", ""), ("root/versions/3.13.0-B/bin/python", ""),
                ("work/.python-version", "3.13\n")), allow=("D-06",)),
    # D-07: envs are listed in bash glob order, which depends on the locale.
    Case("versions with envs in en_US.UTF-8", ("versions",), os="Linux", env=(("LANG", "en_US.UTF-8"),),
         files=(("root/versions/3.12.10/envs/B/bin/python", ""), ("root/versions/3.12.10/envs/a/bin/python", "")),
         allow=("D-07",)),
    # D-17: a missing `versions` folder is created as a side effect.
    Case("versions without a versions folder", ("versions",), os="Windows",
         remove=("root/versions",), compare=("root/versions",), allow=("D-17",)),
    # D-09: `pyenv --help` prints `no such command '--help'` and exits 1.
    Case("--help", ("--help",), os="Windows", allow=("D-09",)),
    # D-10: `pyenv VERSION` prints nothing (routing is half case-insensitive).
    Case("VERSION in capitals", ("VERSION",), os="Windows", allow=("D-10",)),
    # D-19: --debug / PYENV_DEBUG give a bash `set -x` trace on stderr.
    Case("version with PYENV_DEBUG", ("version",), os="Linux", env=(("PYENV_DEBUG", "1"),), allow=("D-19",)),
    # D-18: a missing relative <dir> prints bash's `cd` error.
    Case("version-file for a missing directory", ("version-file", "missing"), os="Linux", allow=("D-18",)),
    # D-21: an unwritable file prints bash's redirection error (needs a non-root user).
    Case("local into a read-only file, bash", ("local", "{v1}"), os="Linux",
         files=(("work/.python-version", "{v0}\n"),), readonly=("work/.python-version",),
         compare=("work/.python-version",), allow=("D-21",)),
    # D-22: with an extra argument the PATH check is skipped (pyenv.bat routes on %1%2). The
    # shims folder is off PATH (C:\Windows is where GitHub's runners have it) and both tools'
    # python shims exist, so the check would warn.
    Case("version extra with the shims off PATH", ("version", "extra"), os="Windows",
         env=(("PATH", r"C:\Windows\System32;C:\Windows"),),
         files=(("root/shims/python.bat", ""), ("root/shims/python.exe", "")), allow=("D-22",)),
    # D-11: `--unset` with a missing file prints a VBScript runtime error (exit 0).
    Case("local --unset without a local file", ("local", "--unset"), os="Windows", allow=("D-11",)),
    Case("global --unset without a global file", ("global", "--unset"), os="Windows",
         remove=("root/version",), allow=("D-11",)),
    # D-14: `global -f` writes `-f` into the version file as a version.
    Case("global -f", ("global", "-f", "{v1}"), os="Linux", compare=("root/version",), allow=("D-14",)),
    # D-13: pyenv-win lacks `prefix` (D-12); rpyenv joins with `;`, not `:`.
    Case("prefix with two versions selected", ("prefix",), os="Windows",
         files=(("work/.python-version", "{v0}\n{v1}\n"),), allow=("D-12", "D-13")),
    # D-23: a failed `--unset` must not report success. Here `.python-version` is a folder,
    # which pyenv-win's DeleteFile leaves in place with no message and exit 0.
    Case("local --unset when .python-version is a folder", ("local", "--unset"), os="Windows",
         files=(("work/.python-version/x", ""),), allow=("D-23",)),
    # D-24: `rm -f`'s error text (here `.python-version` is a folder).
    Case("local --unset when .python-version is a directory", ("local", "--unset"), os="Linux",
         files=(("work/.python-version/x", ""),), allow=("D-24",)),
    # D-26: an unwritable file raises a VBScript runtime error on stderr (exit 0).
    Case("local into a read-only file, VBScript", ("local", "{v1}"), os="Windows",
         files=(("work/.python-version", "{v0}\n"),), readonly=("work/.python-version",),
         compare=("work/.python-version",), allow=("D-26",)),
    Case("global into a read-only file, VBScript", ("global", "{v1}"), os="Windows",
         readonly=("root/version",), compare=("root/version",), allow=("D-26",)),
    # D-28: one argument containing `:` is split, each part validated, then written as one word.
    Case("local with a colon-joined argument", ("local", "{v1}:{v0}"), os="Linux",
         compare=("work/.python-version",), allow=("D-28",)),
    # D-29: a directory named like the command in a version's `bin` counts as found (`-x`).
    Case("which for a directory named like the command", ("which", "tool"), os="Linux",
         files=(("root/versions/3.12.10/bin/tool/x", ""),), allow=("D-29",)),
    # D-31: every path component is printed in on-disk letter case.
    Case("which python in a version folder named in another case", ("which", "python"), os="Windows",
         files=(("root/versions/PyPy3.9/python.exe", ""), ("work/.python-version", "pypy3.9\n")),
         allow=("D-31",)),
    # D-32: a version with the command in both `Scripts` and `bin` is listed twice.
    Case("whence for a command in Scripts and bin", ("whence", "tool"), os="Windows",
         files=(("root/versions/3.9.1/Scripts/tool.exe", ""), ("root/versions/3.9.1/bin/tool.exe", "")),
         allow=("D-32",)),
    # D-33: every entry in `versions/*/bin`, including non-executables and dotfiles.
    Case("versions --executables with a dotfile and a plain file", ("versions", "--executables"), os="Linux",
         files=(("root/versions/3.12.10/bin/.dot", ""), ("root/versions/3.12.10/bin/plain", "")),
         allow=("D-33",)),
    # D-37: Linux sorts by the locale; Windows' `dir` drops only hidden and system files.
    Case("shims in en_US.UTF-8", ("shims",), os="Linux", env=(("LANG", "en_US.UTF-8"),),
         files=(("root/shims/B", ""), ("root/shims/a", "")), allow=("D-37",)),
    Case("shims with rpyenv's dot files", ("shims",), os="Windows",
         files=(("root/shims/python.exe", ""), ("root/shims/.rehash-state", ""),
                ("root/shims/.template/pyenv-shim.exe", "")), allow=("D-37",)),
    # D-38: the child's PATH starts with libexec and the plugin `bin` folders, and
    # PYENV_HOOK_PATH and _PYENV_INSTALL_PREFIX are exported. The fixture's `python` (mode
    # 755) is rewritten to print them.
    Case("exec environment", ("exec", "python"), os="Linux",
         files=(("root/versions/3.12.10/bin/python",
                 "#!/bin/sh\necho \"PATH=$PATH\"\necho \"HOOK=${PYENV_HOOK_PATH-unset}\"\n"
                 "echo \"PREFIX=${_PYENV_INSTALL_PREFIX-unset}\"\n"),), allow=("D-38",)),
    # D-39: `… -m pip` runs through a wrapper that exports PYENV_REHASH_REAL_COMMAND; rpyenv
    # spawns the child and returns its exit code.
    Case("exec python -m pip", ("exec", "python", "-m", "pip", "install", "x"), os="Linux",
         files=(("root/versions/3.12.10/bin/python",
                 "#!/bin/sh\necho \"REAL=${PYENV_REHASH_REAL_COMMAND-unset}\"\nexit 3\n"),), allow=("D-39",)),
    # D-41: `exec` without a command prints `|| was unexpected at this time.`, exit 255.
    Case("exec without a command", ("exec",), os="Windows", allow=("D-41",)),
    # D-48: a character the console's code page lacks. Set through PYENV_VERSION: pyenv-win
    # reads a version file in the ANSI code page, which would garble the name before it is
    # printed.
    Case("version naming a character outside the code page", ("version",), os="Windows",
         env=(("PYENV_VERSION", "3.9.1-\u6f22"),), allow=("D-48",)),
    # D-53: version files are read in the ANSI code page, so a UTF-8 `\u00e9` arrives as `\u00c3\u00a9`. `\u00e9`
    # is in the console's code page (850 here, 437 on GitHub's runners: 0x82 in both), so
    # D-48 isn't involved.
    Case("version-name with a non-ASCII name in the local file", ("version-name",), os="Windows",
         files=(("work/.python-version", "3.9.1-\u00e9\n"),), allow=("D-53",)),
)
