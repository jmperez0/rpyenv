"""Tests for install.sh (M6b design §6.2). Each test serves a fake release over HTTP from
a temporary folder and runs the script with a throwaway HOME and PYENV_ROOT, with fake
`uname`/`curl`/`pyenv` where it needs them. Stdlib only; runs on Linux.
Run: python3 -m unittest ci/test_install_sh.py -v
"""
import hashlib
import http.server
import io
import os
import shutil
import subprocess
import tarfile
import tempfile
import threading
import unittest
from functools import partial
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SCRIPT = REPO / "install.sh"

FAKE_PYENV = """#!/bin/sh
printf '%s\\n' "$*" >> "$RPYENV_TEST_CALLS"
case "$1" in --version) echo "pyenv 2.8.8 (rpyenv {version})" ;; esac
"""


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def make_tarball(path: Path, version: str) -> None:
    with tarfile.open(path, "w:gz") as tar:
        def add(name: str, text: str, mode: int = 0o644) -> None:
            data = text.encode("utf-8")
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = mode
            tar.addfile(info, io.BytesIO(data))

        add("bin/pyenv", FAKE_PYENV.format(version=version), 0o755)
        add("bin/pyenv-shim", "#!/bin/sh\nexit 0\n", 0o755)
        for sh in ("bash", "zsh", "fish"):
            add(f"completions/pyenv.{sh}", f"# {sh} completions {version}\n")
        add("LICENSE", "MIT\n")
        add("README.md", "rpyenv\n")


class Release:
    """Tarballs for both architectures and SHA256SUMS, served over HTTP."""

    def __init__(self, base: Path, version: str):
        self.dir = base / f"release-{version}"
        self.dir.mkdir()
        sums = []
        for arch in ("x64", "arm64"):
            name = f"rpyenv-{version}-linux-{arch}.tar.gz"
            make_tarball(self.dir / name, version)
            sums.append(f"{hashlib.sha256((self.dir / name).read_bytes()).hexdigest()}  {name}\n")
        (self.dir / "SHA256SUMS").write_text("".join(sums), encoding="utf-8")
        handler = partial(QuietHandler, directory=str(self.dir))
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()


class InstallShTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)
        # A throwaway HOME: the real ~/.pyenv is never reached (global constraints).
        self.home = self.tmp / "home"
        self.home.mkdir()
        self.root = self.home / ".pyenv"
        self.calls = self.tmp / "calls.log"
        self.fakes = self.tmp / "fakes"
        self.fakes.mkdir()
        self.release = Release(self.tmp, "0.9.0")
        self.addCleanup(self.release.close)
        self.fake("uname", 'case "$1" in -s) echo Linux ;; -m) echo x86_64 ;; *) echo Linux ;; esac')

    def fake(self, name: str, body: str) -> None:
        path = self.fakes / name
        path.write_text("#!/bin/sh\n" + body + "\n", encoding="utf-8")
        path.chmod(0o755)

    def run_install(self, *args, root=None, tty=None, url=None):
        env = {
            "HOME": str(self.home),
            "PATH": f"{self.fakes}:{os.environ['PATH']}",
            "SHELL": "/bin/bash",
            "RPYENV_INSTALL_BASE_URL": self.release.url if url is None else url,
            "RPYENV_INSTALL_TTY": str(tty) if tty else "/nonexistent/tty",
            "RPYENV_TEST_CALLS": str(self.calls),
        }
        if root is not None:
            env["PYENV_ROOT"] = str(root)
        return subprocess.run(
            ["sh", str(SCRIPT), *args],
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=60,
        )

    def calls_text(self) -> str:
        return self.calls.read_text(encoding="utf-8") if self.calls.exists() else ""

    def upstream_checkout(self) -> None:
        (self.root / "libexec").mkdir(parents=True)
        (self.root / "libexec" / "pyenv").write_text("#!/usr/bin/env bash\n", encoding="utf-8")
        (self.root / "bin").mkdir()
        (self.root / "bin" / "pyenv").symlink_to("../libexec/pyenv")
        (self.root / ".git").mkdir()
        (self.root / "README.md").write_text("upstream\n", encoding="utf-8")
        (self.root / "versions" / "3.12.1" / "bin").mkdir(parents=True)
        (self.root / "version").write_text("3.12.1\n", encoding="utf-8")
        (self.root / "plugins" / "python-build").mkdir(parents=True)
        (self.root / "plugins" / "pyenv-virtualenv").mkdir()

    def test_fresh_install_puts_binaries_and_completions_in_pyenv_root(self):
        r = self.run_install()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue(os.access(self.root / "bin" / "pyenv", os.X_OK))
        self.assertTrue((self.root / "bin" / "pyenv-shim").is_file())
        for sh in ("bash", "zsh", "fish"):
            self.assertTrue((self.root / "completions" / f"pyenv.{sh}").is_file(), sh)
        self.assertFalse((self.root / "LICENSE").exists(), "only bin and completions are installed")

    def test_rerunning_upgrades_in_place(self):
        self.assertEqual(self.run_install().returncode, 0)
        newer = Release(self.tmp, "0.9.1")
        self.addCleanup(newer.close)
        r = self.run_install(url=newer.url)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("0.9.1", (self.root / "completions" / "pyenv.bash").read_text(encoding="utf-8"))

    def test_a_tampered_tarball_is_refused_and_nothing_is_installed(self):
        tarball = self.release.dir / "rpyenv-0.9.0-linux-x64.tar.gz"
        tarball.write_bytes(tarball.read_bytes() + b"tampered")
        r = self.run_install()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("checksum", r.stderr)
        self.assertFalse((self.root / "bin").exists())

    def test_arm64_machines_get_the_arm64_tarball(self):
        self.fake("uname", 'case "$1" in -s) echo Linux ;; -m) echo aarch64 ;; esac')
        r = self.run_install()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("rpyenv-0.9.0-linux-arm64.tar.gz", r.stdout)

    def test_unsupported_machines_and_systems_are_refused(self):
        self.fake("uname", 'case "$1" in -s) echo Linux ;; -m) echo armv7l ;; esac')
        r = self.run_install()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("armv7l", r.stderr)
        self.fake("uname", 'case "$1" in -s) echo Darwin ;; -m) echo arm64 ;; esac')
        r = self.run_install()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("Linux", r.stderr)

    def test_the_version_flag_names_that_release(self):
        self.fake("curl", 'printf "%s\\n" "$@" >> "$RPYENV_TEST_CALLS"; exit 22')
        r = self.run_install("--version", "v1.2.3", url="")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("https://github.com/jmperez0/rpyenv/releases/download/v1.2.3/SHA256SUMS", self.calls_text())

    def test_an_upstream_checkout_is_refused_without_take_over(self):
        self.upstream_checkout()
        r = self.run_install()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("--take-over", r.stderr)
        self.assertTrue((self.root / "libexec" / "pyenv").is_file(), "nothing was moved")

    def test_take_over_keeps_the_users_data_and_restore_brings_upstream_back(self):
        self.upstream_checkout()
        r = self.run_install("--take-over")
        self.assertEqual(r.returncode, 0, r.stderr)
        saved = self.root / ".upstream-pyenv"
        for kept in ("versions/3.12.1", "version", "plugins/pyenv-virtualenv"):
            self.assertTrue((self.root / kept).exists(), kept)
        for moved in ("libexec/pyenv", ".git", "README.md", "plugins/python-build"):
            self.assertTrue(os.path.lexists(saved / moved), moved)
            self.assertFalse(os.path.lexists(self.root / moved), moved)
        self.assertTrue((self.root / "bin" / "pyenv-shim").is_file())
        r = self.run_install("--restore-upstream")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue((self.root / "bin" / "pyenv").is_symlink())
        self.assertTrue((self.root / "libexec" / "pyenv").is_file())
        self.assertTrue((self.root / "plugins" / "python-build").is_dir())
        self.assertFalse((self.root / "bin" / "pyenv-shim").exists())
        self.assertFalse(saved.exists())
        self.assertTrue((self.root / "versions" / "3.12.1").is_dir())

    def test_a_second_take_over_is_refused(self):
        self.upstream_checkout()
        (self.root / ".upstream-pyenv").mkdir()
        r = self.run_install("--take-over")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn(".upstream-pyenv", r.stderr)
        self.assertTrue((self.root / "libexec" / "pyenv").is_file(), "nothing was moved")

    def test_without_a_terminal_it_prints_the_init_lines(self):
        r = self.run_install()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("init bash", self.calls_text())
        self.assertNotIn("--install", self.calls_text())

    def test_the_init_flag_edits_the_startup_file(self):
        r = self.run_install("--init")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("init --install bash", self.calls_text())

    def test_the_prompt_answer_decides(self):
        answers = self.tmp / "answers"
        answers.write_text("n\n", encoding="utf-8")
        r = self.run_install(tty=answers)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("[Y/n]", r.stderr)
        self.assertNotIn("--install", self.calls_text())
        answers.write_text("\n", encoding="utf-8")
        r = self.run_install(tty=answers)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("init --install bash", self.calls_text())

    def test_uninstall_removes_only_rpyenv_files(self):
        self.assertEqual(self.run_install().returncode, 0)
        (self.root / "versions" / "3.12.1").mkdir(parents=True)
        (self.root / "bin" / "other-tool").write_text("x\n", encoding="utf-8")
        r = self.run_install("--uninstall")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse((self.root / "bin" / "pyenv").exists())
        self.assertFalse((self.root / "bin" / "pyenv-shim").exists())
        self.assertFalse((self.root / "completions").exists())
        self.assertTrue((self.root / "bin" / "other-tool").is_file())
        self.assertTrue((self.root / "versions" / "3.12.1").is_dir())

    def test_uninstall_refuses_an_upstream_checkout(self):
        self.upstream_checkout()
        r = self.run_install("--uninstall")
        self.assertNotEqual(r.returncode, 0)
        self.assertTrue((self.root / "bin" / "pyenv").is_symlink(), "upstream's pyenv stays")

    def test_a_pyenv_root_with_spaces_and_non_ascii(self):
        root = self.tmp / "py env ñ"
        r = self.run_install(root=root)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue((root / "bin" / "pyenv").is_file())
        r = self.run_install("--uninstall", root=root)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse((root / "bin" / "pyenv").exists())

    def test_shellcheck_is_clean(self):
        if shutil.which("shellcheck") is None:
            self.skipTest("shellcheck isn't installed (CI runs it)")
        r = subprocess.run(["shellcheck", "-s", "sh", str(SCRIPT)], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stdout)


if __name__ == "__main__":
    unittest.main()
