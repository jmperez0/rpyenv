"""Checks a release's signed Windows files in CI (Phase 3 spec §5, §7).

  python ci/check_signed.py {exes,msi} --zips DIR --out DIR (--commit SHA | --no-attestation)
                            [--version V] [--thumbprint HEX] [--arches A ...]

--zips holds tmp-unsigned-<kind>-<arch>.zip and tmp-signed-<kind>-<arch>.zip for each arch, and
each zip must hold exactly the names in installer/signing.json. Every unsigned file must carry
the attestation of the workflow that built it at --commit. Every signed file must pass
`signtool verify /pa`, be signed by the pinned thumbprint with a timestamp, and be its unsigned
file plus a signature. The signed files are left in --out/<arch>/. Every file is checked before
the exit status (1 on any failure) is decided.
"""
import argparse
import os
import shutil
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import signing  # noqa: E402


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("kind", choices=["exes", "msi"])
    ap.add_argument("--zips", required=True)
    ap.add_argument("--out", required=True)
    who = ap.add_mutually_exclusive_group(required=True)
    who.add_argument("--commit")
    who.add_argument("--no-attestation", action="store_true")
    ap.add_argument("--version")
    ap.add_argument("--thumbprint")
    ap.add_argument("--arches", nargs="+")
    a = ap.parse_args(argv)
    if a.kind == "msi" and not a.version:
        ap.error("msi needs --version")
    cfg = signing.load_config()
    thumbprint = (a.thumbprint or cfg["thumbprint"]).upper()
    names = signing.file_names(cfg, a.kind, a.version)
    work = tempfile.mkdtemp(prefix="check-signed.")
    failed = 0
    try:
        signtool = signing.find_signtool()
        for arch in a.arches or cfg["arches"]:
            unsigned = signing.unzip_exact(os.path.join(a.zips, signing.zip_name("unsigned", a.kind, arch)),
                                           os.path.join(work, arch), names[arch])
            signed = signing.unzip_exact(os.path.join(a.zips, signing.zip_name("signed", a.kind, arch)),
                                         os.path.join(a.out, arch), names[arch])
            for u, s in zip(unsigned, signed):
                label = f"{arch}/{os.path.basename(s)}"
                problems = []
                try:
                    if a.commit:
                        signing.verify_attestation(u, cfg["repo"], signing.BUILT_BY[a.kind], a.commit)
                    problems += signing.signature_problems(s, thumbprint, signtool)
                    why = signing.file_difference(u, s)
                    if why:
                        problems.append(why)
                except signing.SigningError as e:
                    problems.append(str(e))
                for p in problems:
                    print(f"FAIL {label}: {p}", flush=True)
                if not problems:
                    print(f"ok {label}", flush=True)
                failed += bool(problems)
    except (signing.SigningError, OSError) as e:
        print(f"check_signed: {e}", file=sys.stderr)
        return 1
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
