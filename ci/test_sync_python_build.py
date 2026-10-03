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

    @unittest.skipUnless(hasattr(os, "symlink") and os.name != "nt", "needs symlinks (WSL/Linux)")
    def test_refuses_symlinks_in_the_untrusted_upstream_tree(self):
        share = os.path.join("plugins", "python-build", "share", "python-build")
        with tempfile.TemporaryDirectory() as secret:
            with open(os.path.join(secret, "hostname"), "w", encoding="utf-8") as f:
                f.write("secret\n")
            os.makedirs(os.path.join(secret, "dir"))
            cases = {
                "license": lambda up: (os.remove(os.path.join(up, "LICENSE")),
                                       os.symlink(os.path.join(secret, "hostname"), os.path.join(up, "LICENSE"))),
                "definition": lambda up: (os.remove(os.path.join(up, share, "3.12.0")),
                                          os.symlink(os.path.join(secret, "hostname"), os.path.join(up, share, "3.12.0"))),
                "patch dir": lambda up: (os.makedirs(os.path.join(up, share, "patches"), exist_ok=True),
                                         __import__("shutil").rmtree(os.path.join(up, share, "patches", "3.12.0")),
                                         os.symlink(secret, os.path.join(up, share, "patches", "3.12.0"))),
                "patch file": lambda up: (os.remove(os.path.join(up, share, "patches", "3.12.0", "Python-3.12.0", "0001.patch")),
                                          os.symlink(os.path.join(secret, "hostname"),
                                                     os.path.join(up, share, "patches", "3.12.0", "Python-3.12.0", "0001.patch"))),
                "subdir": lambda up: os.symlink(os.path.join(secret, "dir"),
                                                os.path.join(up, share, "patches", "3.12.0", "Python-3.12.0", "sub")),
            }
            for label, mutate in cases.items():
                with self.subTest(label), tempfile.TemporaryDirectory() as up, tempfile.TemporaryDirectory() as dest:
                    self.upstream(up)
                    mutate(up)
                    with self.assertRaises(s.Unsafe):
                        s.sync(up, dest, "abc123", write=True)
                    self.assertEqual(os.listdir(dest), [], "nothing is written")

    @unittest.skipUnless(hasattr(os, "symlink") and os.name != "nt", "needs symlinks (WSL/Linux)")
    def test_a_patch_dir_may_link_to_a_sibling_patch_dir_as_upstream_does(self):
        share = os.path.join("plugins", "python-build", "share", "python-build")
        with tempfile.TemporaryDirectory() as up, tempfile.TemporaryDirectory() as dest:
            self.upstream(up)
            tree(up, {share.replace(os.sep, "/") + "/3.12.0t": "source x\n"})
            os.symlink("3.12.0", os.path.join(up, share, "patches", "3.12.0t"))
            s.sync(up, dest, "abc123", write=True)
            with open(os.path.join(dest, "share", "patches", "3.12.0t", "Python-3.12.0", "0001.patch"), "rb") as f:
                self.assertEqual(f.read(), b"p\r\n")

    @unittest.skipUnless(hasattr(os, "symlink") and os.name != "nt", "needs symlinks (WSL/Linux)")
    def test_main_exits_2_with_a_message_on_a_symlink(self):
        with tempfile.TemporaryDirectory() as up:
            self.upstream(up)
            os.remove(os.path.join(up, "LICENSE"))
            os.symlink("/etc/hostname", os.path.join(up, "LICENSE"))
            self.assertEqual(s.main(["--upstream", up, "--commit", "abc"]), 2)


if __name__ == "__main__":
    unittest.main()
