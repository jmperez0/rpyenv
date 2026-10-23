# Releasing rpyenv

A release needs the maintainer's PC with the Certum card. CI builds, tests and attests;
the PC only signs. The dry run on 2026-10-23 (`v0.1.0-dryrun.4`) took about 12 minutes:
4 min for `release.yml`, then 7 min 13 s for `release_sign.py` from start to `Done` (two
PIN prompts, one per signing step; the rest is CI).

## Steps

1. **Bump the version** in `Cargo.toml` (`version = "X.Y.Z"`) on `main`, then push the tag:
   `git tag vX.Y.Z && git push origin vX.Y.Z`. `release.yml` builds the executables and
   tarballs, attests them and creates a draft release.
2. **Insert the card**, then run `python ci/release_sign.py vX.Y.Z` from the repo root. It:
   1. signs the six executables (the Certum dialog asks for the PIN);
   2. starts `release-msi.yml` and waits for it;
   3. signs the two MSIs (PIN again);
   4. starts `release-finish.yml` and waits for it;
   5. prints the draft's link.
3. **Review the draft** (6 assets: 2 MSIs, 2 tarballs, `install.sh`, `SHA256SUMS`) and
   publish it.

`python ci/release_sign.py vX.Y.Z --check` prints where a draft is, without changing
anything. Running the script again continues from the draft's state.

## Dry run

Run `release.yml` by hand on a branch: `gh workflow run release.yml --ref main`, or another
branch. It creates the draft pre-release `vX.Y.Z-dryrun.N`. Then run
`python ci/release_sign.py vX.Y.Z-dryrun.N`, adding `--ref <branch>` when the branch isn't `main`.
Delete the draft afterwards. Don't push to that branch until the dry run is finished:
steps 3 and 5 refuse to run if the branch has moved since step 1.

## When something fails

| What fails | What happens | What to do |
|---|---|---|
| Card removed, wrong PIN, signtool error | Nothing is uploaded; the temporary folder is removed | Run the script again |
| First timestamp server down | The script tries the second one (the PIN dialog may show again) | Nothing, unless both are down: try later |
| A check in `release-msi.yml` or `release-finish.yml` fails (attestation, signer, timestamp, "differs from the unsigned file") | That run uploads nothing | Read the run log. Don't retry until you know why. |
| A build or test in step 3 or 5 fails | The draft does not change | A runner or network failure: `python ci/release_sign.py <tag> --retry` (add `--ref <branch>` for a dry run). A code fix needs a new tag or a new dry run, because steps 3 and 5 run the drafted commit. |
| Step 5 stopped while removing the `tmp-*` files | Every final file is on the draft | Run the script again: it checks the final files' attestations and `SHA256SUMS`, then removes the `tmp-*` files. |
| The dry run's branch moved | Step 3 or 5 stops with that message | Start a new dry run |
| You want to drop a release | — | Delete the draft by hand. The script never deletes drafts or tags. |

## What is pinned

`installer/signing.json` holds the certificate's thumbprint, the timestamp servers and the
names of the files that may be signed. A new certificate is a one-line change there.
The design is `docs/superpowers/specs/2026-10-22-release-signing-design.md`.
