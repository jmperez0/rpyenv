import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sync_python_build as s  # noqa: E402


def tree(root, files):
    for rel, text in files.items():
        p = os.path.join(root, *rel.split("/"))
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w", encoding="utf-8", newline="") as f:
            f.write(text)


class Sync(unittest.TestCase):
    def upstream(self, d, extra=None):
        files = {
            "LICENSE": "MIT\n",
            "libexec/pyenv---version": 'version="2.8.6"\n',
            "plugins/python-build/share/python-build/3.12.0": "install_package a\n",
            "plugins/python-build/share/python-build/3.12.0t": "source x\n",
            "plugins/python-build/share/python-build/pypy3.10-7.3.12": "pypy\n",
            "plugins/python-build/share/python-build/patches/3.12.0/Python-3.12.0/0001.patch": "p\r\n",
            "plugins/python-build/share/python-build/patches/pypy3.10-7.3.12/x.patch": "no\n",
        }
        files.update(extra or {})
        tree(d, files)

    def test_copies_cpython_definitions_and_their_patches_only(self):
        with tempfile.TemporaryDirectory() as up, tempfile.TemporaryDirectory() as dest:
            self.upstream(up)
            changes = s.sync(up, dest, "abc123", write=True)
            self.assertEqual(sorted(os.listdir(os.path.join(dest, "share"))), ["3.12.0", "3.12.0t", "patches"])
            self.assertEqual(os.listdir(os.path.join(dest, "share", "patches")), ["3.12.0"])
            with open(os.path.join(dest, "share", "patches", "3.12.0", "Python-3.12.0", "0001.patch"), "rb") as f:
                self.assertEqual(f.read(), b"p\r\n", "bytes are copied exactly")
            with open(os.path.join(dest, "UPSTREAM"), encoding="utf-8") as f:
                self.assertEqual(f.read(), "pyenv abc123 2.8.6\n")
            self.assertTrue(changes)
            self.assertEqual(s.sync(up, dest, "abc123", write=False), [], "a second run finds no change")

    def test_reports_added_changed_and_removed_files(self):
        with tempfile.TemporaryDirectory() as up, tempfile.TemporaryDirectory() as dest:
            self.upstream(up)
            s.sync(up, dest, "abc123", write=True)
            tree(up, {"plugins/python-build/share/python-build/3.12.1": "new\n",
                      "plugins/python-build/share/python-build/3.12.0": "changed\n"})
            os.remove(os.path.join(up, "plugins/python-build/share/python-build/3.12.0t"))
            self.assertEqual(
                s.sync(up, dest, "def456", write=False),
                ["changed share/3.12.0", "added share/3.12.1", "removed share/3.12.0t", "changed UPSTREAM"],
            )


if __name__ == "__main__":
    unittest.main()
