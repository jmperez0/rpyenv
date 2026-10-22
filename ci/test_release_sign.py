"""Offline tests of ci/release_sign.py (Phase 3 spec §6) against a fake gh and signtool."""
import contextlib
import io
import json
import os
import sys
import tempfile
import unittest
import zipfile
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import release_sign as rs  # noqa: E402
import signing  # noqa: E402

ARCHES = ["x64", "arm64"]
COMMIT = "c" * 40
EXES = ["pyenv-shim.exe", "pyenv-shimw.exe", "pyenv.exe"]


def step1_assets(version="0.2.0"):
    return {f"rpyenv-{version}-linux-x64.tar.gz", f"rpyenv-{version}-linux-arm64.tar.gz", "install.sh",
            "tmp-unsigned-exes-x64.zip", "tmp-unsigned-exes-arm64.zip"}


class FakeGitHub:
    """A draft release and workflow runs, as `gh` would report them."""

    def __init__(self, tag, assets, outcomes=None, register_runs=True):
        self.tag, self.version = tag, signing.parse_tag(tag)[0]
        self.assets = set(assets) if assets is not None else None
        self.is_draft = True
        self.runs = []
        self.outcomes = outcomes or {}  # workflow stem -> list of "success"/"failure"
        self.register_runs = register_runs
        self.dispatched, self.uploaded = [], []
        self.extra_in_zip = None

    def zip_names(self, asset):
        kind = "exes" if "-exes-" in asset else "msi"
        arch = asset.rsplit("-", 1)[1].removesuffix(".zip")
        names = list(EXES) if kind == "exes" else [f"rpyenv-{self.version}-{arch}.msi"]
        return names + ([self.extra_in_zip] if self.extra_in_zip else [])

    def gh(self, *args):
        a = list(args)
        if a[:2] == ["release", "view"]:
            if self.assets is None:
                raise rs.Stop("gh release view: release not found")
            return json.dumps({"isDraft": self.is_draft, "targetCommitish": COMMIT, "url": "https://draft",
                               "assets": [{"name": n} for n in sorted(self.assets)]})
        if a[:2] == ["release", "download"]:
            dest = a[a.index("-D") + 1]
            os.makedirs(dest, exist_ok=True)
            for i, x in enumerate(a):
                if x == "-p" and a[i + 1] in self.assets:
                    with zipfile.ZipFile(os.path.join(dest, a[i + 1]), "w") as z:
                        for n in self.zip_names(a[i + 1]):
                            z.writestr(n, b"MZ unsigned")
            return ""
        if a[:2] == ["release", "upload"]:
            files = [x for x in a[3:] if x.endswith(".zip")]
            self.uploaded.append(sorted(os.path.basename(f) for f in files))
            self.assets |= {os.path.basename(f) for f in files}
            return ""
        if a[0] == "api":
            return COMMIT + "\n"
        if a[:2] == ["workflow", "run"]:
            stem = a[2].removesuffix(".yml")
            ref = a[a.index("--ref") + 1]
            request = [x for x in a if x.startswith("request=")][0].split("=", 1)[1]
            self.dispatched.append((stem, ref))
            if self.register_runs:
                self.runs.insert(0, {"databaseId": len(self.runs) + 1, "displayTitle": f"{stem} {self.tag} {request}",
                                     "status": "in_progress", "conclusion": "", "url": f"https://run/{stem}",
                                     "stem": stem})
            return ""
        if a[:2] == ["run", "list"]:
            stem = a[a.index("--workflow") + 1].removesuffix(".yml")
            return json.dumps([r for r in self.runs if r["stem"] == stem])
        raise AssertionError(f"unexpected gh {a}")

    def gh_live(self, *args):
        assert args[:2] == ("run", "watch"), args
        run = next(r for r in self.runs if r["databaseId"] == int(args[2]))
        outcome = (self.outcomes.get(run["stem"]) or ["success"]).pop(0)
        run.update(status="completed", conclusion=outcome)
        if outcome == "success" and run["stem"] == "release-msi":
            self.assets |= {"tmp-unsigned-msi-x64.zip", "tmp-unsigned-msi-arm64.zip"}
        if outcome == "success" and run["stem"] == "release-finish":
            self.assets = {n for n in self.assets if not n.startswith("tmp-")}
            self.assets |= {f"rpyenv-{self.version}-x64.msi", f"rpyenv-{self.version}-arm64.msi", "SHA256SUMS"}
        return 0 if outcome == "success" else 1


class Flow(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.signed_calls, self.signtool_results = [], []
        self.already_signed, self.attest_fail = set(), set()
        self.card = "ok"
        self.workdirs = []

    def tearDown(self):
        self.tmp.cleanup()

    def mkdtemp(self, prefix=""):
        d = os.path.join(self.tmp.name, f"work{len(self.workdirs)}")
        os.makedirs(d)
        self.workdirs.append(d)
        return d

    def fake_signtool(self, signtool, cfg, timestamp, files):
        self.signed_calls.append((timestamp, sorted(os.path.relpath(f, self.workdirs[-1]) for f in files)))
        for f in files:
            with open(f, "rb") as fh:
                assert fh.read() == b"MZ unsigned", "signed twice"
            with open(f, "wb") as fh:
                fh.write(b"MZ signed")
        return self.signtool_results.pop(0) if self.signtool_results else (0, "")

    def fake_attest(self, path, repo, workflow, commit):
        if os.path.basename(path) in self.attest_fail:
            raise signing.SigningError(f"{os.path.basename(path)}: attestation: no match")

    def fake_info(self, path):
        if os.path.basename(path) in self.already_signed:
            return signing.SigInfo("Valid", "X", True)
        return signing.SigInfo("NotSigned", "", False)

    def run_main(self, fake, *argv):
        buf = io.StringIO()
        patches = [
            mock.patch.object(rs, "gh", fake.gh),
            mock.patch.object(rs, "gh_live", fake.gh_live),
            mock.patch.object(rs, "card_state", lambda t: self.card),
            mock.patch.object(rs, "run_signtool", self.fake_signtool),
            mock.patch.object(rs.time, "sleep", lambda s: None),
            mock.patch.object(rs.tempfile, "mkdtemp", self.mkdtemp),
            mock.patch.object(signing, "find_signtool", lambda override=None: "signtool.exe"),
            mock.patch.object(signing, "verify_attestation", self.fake_attest),
            mock.patch.object(signing, "signature_info", self.fake_info),
            mock.patch.object(signing, "signature_problems", lambda p, t, s: []),
        ]
        with contextlib.ExitStack() as stack:
            for p in patches:
                stack.enter_context(p)
            stack.enter_context(contextlib.redirect_stdout(buf))
            stack.enter_context(contextlib.redirect_stderr(buf))
            code = rs.main(list(argv))
        return code, buf.getvalue()

    def test_a_dry_run_goes_from_step_1_to_done(self):
        fake = FakeGitHub("v0.2.0-dryrun.7", step1_assets())
        code, text = self.run_main(fake, "v0.2.0-dryrun.7")
        self.assertEqual(code, 0, text)
        self.assertEqual(fake.dispatched, [("release-msi", "main"), ("release-finish", "main")])
        self.assertEqual(fake.uploaded, [["tmp-signed-exes-arm64.zip", "tmp-signed-exes-x64.zip"],
                                         ["tmp-signed-msi-arm64.zip", "tmp-signed-msi-x64.zip"]])
        self.assertEqual([len(files) for _, files in self.signed_calls], [6, 2])
        self.assertEqual({t for t, _ in self.signed_calls}, {"http://time.certum.pl"})
        self.assertIn("https://draft", text)
        self.assertTrue(all(not os.path.exists(d) for d in self.workdirs))

    def test_a_release_dispatches_on_its_tag(self):
        fake = FakeGitHub("v0.2.0", step1_assets())
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 0, text)
        self.assertEqual(fake.dispatched, [("release-msi", "v0.2.0"), ("release-finish", "v0.2.0")])

    def test_a_dry_run_on_a_branch(self):
        fake = FakeGitHub("v0.2.0-dryrun.8", step1_assets())
        code, text = self.run_main(fake, "v0.2.0-dryrun.8", "--ref", "phase3-release-signing")
        self.assertEqual(code, 0, text)
        self.assertEqual(fake.dispatched, [("release-msi", "phase3-release-signing"),
                                           ("release-finish", "phase3-release-signing")])

    def test_ref_is_refused_for_a_release(self):
        fake = FakeGitHub("v0.2.0", step1_assets())
        code, text = self.run_main(fake, "v0.2.0", "--ref", "main")
        self.assertEqual(code, 1)
        self.assertIn("--ref is only for dry runs", text)
        self.assertEqual((fake.dispatched, self.signed_calls), ([], []))

    def test_check_only_reads(self):
        fake = FakeGitHub("v0.2.0", step1_assets())
        code, text = self.run_main(fake, "v0.2.0", "--check")
        self.assertEqual(code, 0)
        self.assertIn("sign-exes", text)
        self.assertEqual((fake.dispatched, fake.uploaded, self.signed_calls), ([], [], []))

    def test_card_missing_stops_before_downloading(self):
        self.card = "missing"
        fake = FakeGitHub("v0.2.0", step1_assets())
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("Insert the Certum card", text)
        self.assertEqual((self.workdirs, self.signed_calls), ([], []))

    def test_extra_file_stops_before_signing(self):
        fake = FakeGitHub("v0.2.0", step1_assets())
        fake.extra_in_zip = "evil.exe"
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("evil.exe", text)
        self.assertEqual(self.signed_calls, [])
        self.assertTrue(all(not os.path.exists(d) for d in self.workdirs))

    def test_already_signed_stops_before_signing(self):
        self.already_signed.add("pyenv.exe")
        code, text = self.run_main(FakeGitHub("v0.2.0", step1_assets()), "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("already signed", text)
        self.assertEqual(self.signed_calls, [])

    def test_attestation_failure_stops_before_signing(self):
        self.attest_fail.add("pyenv-shimw.exe")
        code, text = self.run_main(FakeGitHub("v0.2.0", step1_assets()), "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("pyenv-shimw.exe: attestation", text)
        self.assertEqual(self.signed_calls, [])

    def test_timestamp_fallback_uses_fresh_copies(self):
        self.signtool_results = [(1, "SignTool Error: The specified timestamp server either could not be reached"), (0, "")]
        fake = FakeGitHub("v0.2.0", step1_assets())
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 0, text)
        self.assertEqual([t for t, _ in self.signed_calls][:2], ["http://time.certum.pl", "http://timestamp.digicert.com"])

    def test_other_signtool_errors_stop(self):
        self.signtool_results = [(1, "SignTool Error: The smart card was removed")]
        fake = FakeGitHub("v0.2.0", step1_assets())
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("smart card was removed", text)
        self.assertEqual(len(self.signed_calls), 1)
        self.assertEqual(fake.uploaded, [])

    def test_a_failed_ci_step_needs_retry(self):
        fake = FakeGitHub("v0.2.0", step1_assets(), outcomes={"release-msi": ["failure"]})
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("https://run/release-msi", text)
        self.assertIn("--retry", text)
        self.assertEqual(fake.dispatched, [("release-msi", "v0.2.0")])
        code, text = self.run_main(fake, "v0.2.0", "--retry")
        self.assertEqual(code, 0, text)
        self.assertEqual(fake.dispatched[1:], [("release-msi", "v0.2.0"), ("release-finish", "v0.2.0")])
        self.assertEqual(len(self.signed_calls), 2)

    def test_a_run_that_never_appears_stops(self):
        fake = FakeGitHub("v0.2.0", step1_assets(), register_runs=False)
        clock = iter(range(0, 10000, 10))
        with mock.patch.object(rs.time, "monotonic", lambda: next(clock)):
            code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("found no run", text)

    def test_no_draft(self):
        code, text = self.run_main(FakeGitHub("v0.2.0", None), "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("no draft for v0.2.0", text)

    def test_published_release(self):
        fake = FakeGitHub("v0.2.0", step1_assets())
        fake.is_draft = False
        code, text = self.run_main(fake, "v0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("already published", text)

    def test_bad_tag(self):
        code, text = self.run_main(FakeGitHub("v0.2.0", step1_assets()), "0.2.0")
        self.assertEqual(code, 1)
        self.assertIn("not a release tag", text)


class State(unittest.TestCase):
    def s(self, *names):
        return rs.state(set(names), ARCHES)

    def test_state_each_step(self):
        ue = ("tmp-unsigned-exes-x64.zip", "tmp-unsigned-exes-arm64.zip")
        se = ("tmp-signed-exes-x64.zip", "tmp-signed-exes-arm64.zip")
        um = ("tmp-unsigned-msi-x64.zip", "tmp-unsigned-msi-arm64.zip")
        sm = ("tmp-signed-msi-x64.zip", "tmp-signed-msi-arm64.zip")
        self.assertEqual(self.s(*ue), "sign-exes")
        self.assertEqual(self.s(*ue, *se), "await-msi")
        self.assertEqual(self.s(*ue, *se, *um), "sign-msi")
        self.assertEqual(self.s(*ue, *se, *um, *sm), "await-finish")
        self.assertEqual(self.s("SHA256SUMS", "install.sh"), "done")

    def test_state_half_uploaded_arch_repeats_the_step(self):
        self.assertEqual(self.s("tmp-unsigned-exes-x64.zip", "tmp-unsigned-exes-arm64.zip",
                                "tmp-signed-exes-x64.zip"), "sign-exes")
        self.assertEqual(self.s("tmp-unsigned-exes-x64.zip", "tmp-unsigned-exes-arm64.zip",
                                "tmp-signed-exes-x64.zip", "tmp-signed-exes-arm64.zip",
                                "tmp-unsigned-msi-x64.zip"), "await-msi")

    def test_state_finish_half_done_never_goes_back(self):
        self.assertEqual(self.s("SHA256SUMS", "tmp-unsigned-exes-x64.zip", "tmp-unsigned-exes-arm64.zip"),
                         "await-finish")

    def test_state_unknown(self):
        self.assertEqual(self.s("install.sh"), "unknown")


if __name__ == "__main__":
    unittest.main()
