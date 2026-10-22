"""Signs a release's Windows files with the maintainer's Certum card (Phase 3 spec §6).

  python ci/release_sign.py <tag> [--check] [--retry] [--signtool PATH]

<tag> is v1.2.3 (a pushed tag) or v1.2.3-dryrun.N (a dry run's draft). The draft's assets are
the only state: the script does the next step, waits for CI and goes on until the draft is
finished or something needs the maintainer. Before signing it checks the card, the
attestations and the exact file names; the card software asks for the PIN, which this script
never sees. Downloads go to a temporary folder that is removed at the end. docs/releasing.md
has the whole procedure.
"""
import argparse
import hashlib
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import signing  # noqa: E402

NEXT = {"exes": "release-msi.yml", "msi": "release-finish.yml"}
FIND_RUN_SECONDS = 120
CARD = {
    "missing": "Insert the Certum card: its certificate is not in CurrentUser\\My",
    "nokey": "the certificate has no private key here; is the card software running?",
    "expired": "the certificate is not valid today",
}
PS_CARD = (
    '$c = Get-Item -LiteralPath ("Cert:\\CurrentUser\\My\\" + $env:RPYENV_THUMBPRINT) -ErrorAction SilentlyContinue; '
    'if (-not $c) { "missing" } elseif (-not $c.HasPrivateKey) { "nokey" } '
    'elseif ($c.NotBefore -gt (Get-Date) -or $c.NotAfter -lt (Get-Date)) { "expired" } else { "ok" }'
)


class Stop(Exception):
    """Ends the run with a message for the maintainer (exit 1)."""


def gh(*args):
    r = subprocess.run(["gh", *args], capture_output=True, text=True, encoding="utf-8")
    if r.returncode:
        raise Stop(f"gh {' '.join(args[:3])}: {r.stderr.strip()[-300:]}")
    return r.stdout


def gh_live(*args):
    """gh with its output on the terminal (gh run watch); its exit code."""
    return subprocess.run(["gh", *args]).returncode


def card_state(thumbprint):
    r = subprocess.run(["pwsh", "-NoProfile", "-NonInteractive", "-Command", PS_CARD],
                       env={**os.environ, "RPYENV_THUMBPRINT": thumbprint},
                       capture_output=True, text=True, encoding="utf-8")
    return r.stdout.strip() if r.returncode == 0 else f"error: {r.stderr.strip()[-300:]}"


def run_signtool(signtool, cfg, timestamp, files):
    r = subprocess.run([signtool, "sign", "/sha1", cfg["thumbprint"], "/fd", "sha256", "/tr", timestamp,
                        "/td", "sha256", "/d", cfg["description"], "/du", cfg["url"], *files],
                       capture_output=True, text=True, errors="replace")
    return r.returncode, r.stdout + r.stderr


def final_names(cfg, version):
    """The assets a finished draft holds (spec §1)."""
    arches = cfg["arches"]
    return ({cfg["msi"].format(version=version, arch=a) for a in arches}
            | {f"rpyenv-{version}-linux-{a}.tar.gz" for a in arches} | {"install.sh", "SHA256SUMS"})


def state(names, arches, finals):
    """The draft's step, from its asset names alone (spec §6.1). "tidy": step 5 uploaded every
    final file but stopped before removing all tmp-* assets, so running it again can't work."""
    def every(stage, kind):
        return all(signing.zip_name(stage, kind, a) in names for a in arches)

    if "SHA256SUMS" in names:
        if not any(n.startswith("tmp-") for n in names):
            return "done"
        return "tidy" if finals <= names else "await-finish"
    if every("signed", "msi"):
        return "await-finish"
    if every("unsigned", "msi"):
        return "sign-msi"
    if every("signed", "exes"):
        return "await-msi"
    if every("unsigned", "exes"):
        return "sign-exes"
    return "unknown"


def draft(repo, tag):
    try:
        return json.loads(gh("release", "view", tag, "-R", repo, "--json", "isDraft,targetCommitish,url,assets"))
    except Stop as e:
        if "not found" in str(e).lower():
            return None
        raise


def commit_of(repo, tag, dry_run, rel):
    c = rel["targetCommitish"] if dry_run else gh("api", f"repos/{repo}/commits/{tag}", "--jq", ".sha").strip()
    if not re.fullmatch(r"[0-9a-f]{40}", c):
        raise Stop(f"{tag}: the draft's commit {c!r} is not a commit hash")
    return c


def sign_copies(signtool, cfg, originals, dest):
    """Copies the files to dest/<arch>/ and signs them in one call, once per timestamp server."""
    last = ""
    for url in cfg["timestamp"]:
        shutil.rmtree(dest, ignore_errors=True)
        copies = []
        for arch, paths in originals.items():
            os.makedirs(os.path.join(dest, arch))
            copies += [shutil.copy2(p, os.path.join(dest, arch)) for p in paths]
        code, output = run_signtool(signtool, cfg, url, copies)
        if code == 0:
            return copies
        last = output.strip()[-500:]
        if "timestamp" not in output.lower():
            break
        print(f"The timestamp server {url} failed; trying the next one (the PIN dialog may show again).", flush=True)
    raise Stop(f"signtool sign failed:\n{last}")


def sign_step(kind, cfg, repo, tag, version, commit, signtool_override):
    card = card_state(cfg["thumbprint"])
    if card != "ok":
        raise Stop(CARD.get(card, card))
    signtool = signing.find_signtool(signtool_override)
    names = signing.file_names(cfg, kind, version)
    work = tempfile.mkdtemp(prefix="rpyenv-sign.")
    try:
        zips = os.path.join(work, "zips")
        patterns = [x for arch in names for x in ("-p", signing.zip_name("unsigned", kind, arch))]
        gh("release", "download", tag, "-R", repo, "-D", zips, *patterns)
        originals = {}
        for arch, files in names.items():
            paths = signing.unzip_exact(os.path.join(zips, signing.zip_name("unsigned", kind, arch)),
                                        os.path.join(work, "unsigned", arch), files)
            for p in paths:
                signing.verify_attestation(p, repo, signing.BUILT_BY[kind], commit)
                info = signing.signature_info(p)
                if info.status != "NotSigned":
                    raise Stop(f"{arch}/{os.path.basename(p)} is already signed ({info.status})")
            originals[arch] = paths
        count = sum(len(p) for p in originals.values())
        print(f"Signing {count} files: enter the card PIN in the Certum dialog.", flush=True)
        signed = sign_copies(signtool, cfg, originals, os.path.join(work, "signed"))
        for p in signed:
            problems = signing.signature_problems(p, cfg["thumbprint"], signtool)
            if problems:
                raise Stop(f"{p}: " + "; ".join(problems))
        out = []
        for arch in names:
            z = os.path.join(work, signing.zip_name("signed", kind, arch))
            signing.make_zip(z, sorted(p for p in signed if os.path.basename(os.path.dirname(p)) == arch))
            out.append(z)
        gh("release", "upload", tag, "-R", repo, "--clobber", *out)
        print(f"Uploaded {', '.join(os.path.basename(z) for z in out)}.", flush=True)
    finally:
        shutil.rmtree(work, ignore_errors=True)


def tidy_step(cfg, repo, tag, version, commit, names):
    """Finishes an interrupted step 5: every final file is on the draft, attested by
    release-finish.yml at the draft's commit and listed in SHA256SUMS; then the tmp-* go."""
    finals = sorted(final_names(cfg, version))
    work = tempfile.mkdtemp(prefix="rpyenv-tidy.")
    try:
        gh("release", "download", tag, "-R", repo, "-D", work, *[x for n in finals for x in ("-p", n)])
        for n in finals:
            signing.verify_attestation(os.path.join(work, n), repo, "release-finish.yml", commit)
        sums = {}
        with open(os.path.join(work, "SHA256SUMS"), encoding="utf-8") as f:
            for line in f:
                parts = line.split()
                if len(parts) != 2:
                    raise Stop(f"unreadable SHA256SUMS line {line!r}")
                sums[parts[1].lstrip("*")] = parts[0]
        want = set(finals) - {"SHA256SUMS"}
        if set(sums) != want:
            raise Stop(f"SHA256SUMS lists {sorted(sums)}, expected {sorted(want)}")
        for n in sorted(want):
            with open(os.path.join(work, n), "rb") as f:
                if hashlib.sha256(f.read()).hexdigest() != sums[n]:
                    raise Stop(f"{n} does not match SHA256SUMS")
        for n in sorted(x for x in names if x.startswith("tmp-")):
            gh("release", "delete-asset", tag, n, "-R", repo, "--yes")
            print(f"Removed {n}.", flush=True)
    finally:
        shutil.rmtree(work, ignore_errors=True)


def runs(repo, workflow, tag):
    """This tag's runs of a workflow, newest first, found by their run name."""
    prefix = f"{workflow.removesuffix('.yml')} {tag} "
    out = gh("run", "list", "-R", repo, "--workflow", workflow, "--limit", "30",
             "--json", "databaseId,displayTitle,status,conclusion,url")
    return [r for r in json.loads(out) if r["displayTitle"].startswith(prefix)]


def dispatch(repo, workflow, tag, ref):
    request = secrets.token_hex(8)
    gh("workflow", "run", workflow, "-R", repo, "--ref", ref, "-f", f"tag={tag}", "-f", f"request={request}")
    deadline = time.monotonic() + FIND_RUN_SECONDS
    while time.monotonic() < deadline:
        for r in runs(repo, workflow, tag):
            if r["displayTitle"].endswith(" " + request):
                return r
        time.sleep(5)
    raise Stop(f"started {workflow} for {tag}, but found no run named with request {request} "
               f"after {FIND_RUN_SECONDS} s")


def follow(repo, run):
    print(f"Waiting for {run['url']}", flush=True)
    gh_live("run", "watch", str(run["databaseId"]), "-R", repo, "--exit-status", "--interval", "30")


def main(argv=None):
    ap = argparse.ArgumentParser(description="Sign a release's Windows files (docs/releasing.md).")
    ap.add_argument("tag")
    ap.add_argument("--check", action="store_true", help="print the draft's step; change nothing")
    ap.add_argument("--retry", action="store_true", help="start a failed CI step again (once)")
    ap.add_argument("--signtool", help="signtool.exe to use (default: the newest Windows SDK's)")
    ap.add_argument("--ref", help="dry runs only: the branch steps 3 and 5 run on (default main)")
    a = ap.parse_args(argv)
    try:
        cfg = signing.load_config()
        repo = cfg["repo"]
        version, dry_run = signing.parse_tag(a.tag)
        if a.ref and not dry_run:
            raise Stop("--ref is only for dry runs; a release's steps run on its tag")
        ref = (a.ref or "main") if dry_run else a.tag
        retried = False
        while True:
            rel = draft(repo, a.tag)
            if rel is None:
                raise Stop(f"no draft for {a.tag}; push the tag or start a dry run (docs/releasing.md)")
            if not rel["isDraft"]:
                raise Stop(f"{a.tag} is already published")
            names = {x["name"] for x in rel["assets"]}
            st = state(names, cfg["arches"], final_names(cfg, version))
            print(f"{a.tag}: {st}", flush=True)
            if st == "done":
                print(f"Done. Review and publish the draft: {rel['url']}")
                return 0
            if a.check:
                return 0
            if st == "unknown":
                raise Stop(f"the draft's assets match no step: {sorted(names)}")
            commit = commit_of(repo, a.tag, dry_run, rel)
            if st == "tidy":
                tidy_step(cfg, repo, a.tag, version, commit, names)
                continue
            if dry_run:
                # Steps 3 and 5 run on `ref`; check it before the card signs anything.
                head = gh("api", f"repos/{repo}/commits/{ref}", "--jq", ".sha").strip()
                if head != commit:
                    raise Stop(f"{ref} is at {head[:12]}, but the draft was built from {commit[:12]}: "
                               f"pass --ref <the branch the dry run started from>, or start a new dry run")
            if st.startswith("sign-"):
                kind = st.removeprefix("sign-")
                sign_step(kind, cfg, repo, a.tag, version, commit, a.signtool)
                follow(repo, dispatch(repo, NEXT[kind], a.tag, ref))
                continue
            workflow = NEXT["exes" if st == "await-msi" else "msi"]
            found = runs(repo, workflow, a.tag)
            last = found[0] if found else None
            if last is None:
                follow(repo, dispatch(repo, workflow, a.tag, ref))
            elif last["status"] != "completed":
                follow(repo, last)
            elif last["conclusion"] == "success":
                raise Stop(f"{last['url']} succeeded, but the draft is still at {st}; look at the draft")
            elif a.retry and not retried:
                retried = True
                follow(repo, dispatch(repo, workflow, a.tag, ref))
            else:
                again = f"python ci/release_sign.py {a.tag} --retry" + (f" --ref {a.ref}" if a.ref else "")
                raise Stop(f"{workflow} failed: {last['url']}\n"
                           f"If the cause was outside the code (a runner or network failure), run: {again}\n"
                           f"A code fix needs a new tag, or a new dry run: steps 3 and 5 run the drafted commit.")
    except (Stop, signing.SigningError, OSError) as e:
        print(f"release_sign: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
