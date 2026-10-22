# Phase 3: signing the Windows release files — design

Status: design approved in conversation (user, 2026-10-22, four parts). Supersedes M6b D3
and the signing steps in M6b §7.2 (`docs/superpowers/specs/2026-10-10-m6b-packaging-design.md`).

## 1. Goal

From v0.2.0 on, the Windows release files are Authenticode-signed with the maintainer's
Certum code-signing certificate:

- the three executables in each MSI (`pyenv.exe`, `pyenv-shim.exe`, `pyenv-shimw.exe`), for
  x64 and ARM64;
- the two MSIs (`rpyenv-<version>-x64.msi`, `rpyenv-<version>-arm64.msi`).

The Linux tarballs and `install.sh` stay unsigned; `SHA256SUMS` and the attestations
cover them, as today.

Success means:

1. A release draft holds the same six assets as v0.1.0 (2 MSIs, 2 tarballs, `install.sh`,
   `SHA256SUMS`), with signed MSIs that contain signed executables.
2. Every final asset has a build-provenance attestation, and the README's command
   (`gh attestation verify … --repo jmperez0/rpyenv --source-ref refs/tags/v<version>`)
   still passes.
3. CI proves that each signed file is the attested unsigned file plus a signature, and
   nothing else.
4. Pull-request packaging runs keep their current coverage and stay unsigned.

## 2. Facts this design rests on

Measured on 2026-10-22, read-only, without using the key:

- The certificate is in `Cert:\CurrentUser\My` only while the card is inserted.
  Thumbprint `58E0373B2F4DF17A924C8B1EAB467FC7619A4403`, issuer Certum Code Signing 2021
  CA, EKU Code Signing only, RSA 4096, valid 2026-10-19 to 2028-01-21. The chain to
  Certum Trusted Network CA 2 builds for the code-signing policy with online revocation
  checks.
- The private key is on a physical Certum card. It cannot reach a GitHub-hosted runner, so
  signing happens on the maintainer's PC.
- This PC has `signtool.exe` (Windows SDK 10.0.26100 and 10.0.28000) and WiX
  (`C:\Users\JM\.dotnet\tools\wix.exe`).
- `parity.yml`'s `coverage` job runs `python3 -m unittest discover -s ci` on Ubuntu, so a
  test in `ci/` runs on every push and pull request.

Not measured yet (the plan's first tasks measure them, see §8):

- how many PIN prompts one `signtool sign` call over several files shows;
- which method proves "signed file = unsigned file + signature" for a PE and for an MSI.

## 3. Decisions (user, 2026-10-22)

| # | Decision |
|---|---|
| S1 | Approach A: two hand-offs between CI and the PC. The executables are signed before they go into the MSI, then the MSI is signed. |
| S2 | Only the maintainer types the PIN, into the card software's own dialog. No script reads, stores or passes it. |
| S3 | Every signature has an RFC 3161 timestamp (SHA-256), so it stays valid after the certificate expires. |
| S4 | Only expected file names are signed; the list lives in `installer/signing.json`. |
| S5 | The state of a release lives only in its draft's assets. Running the script again continues from that state. |
| S6 | Pull-request packaging stays unsigned. `installer/sign.ps1` and the `RPYENV_SIGN_TOOL` steps are removed. |
| S7 | The local script is Python, in `ci/` so its tests run in the existing `coverage` job. |

## 4. The release flow

```
tag v0.2.0 ─► [1 CI: release.yml]      build exes + tarballs, attest, draft
           ─► [2 PC: release_sign.py]  verify, sign 6 exes (PIN), upload, start 3
           ─► [3 CI: release-msi.yml]  check exes, build + test MSIs, attest, upload
           ─► [4 PC: release_sign.py]  verify, sign 2 MSIs (PIN), upload, start 5
           ─► [5 CI: release-finish.yml] check MSIs, SHA256SUMS, attest, tidy draft
           ─► maintainer reviews and publishes the draft
```

Estimated at 25–30 minutes per release, mostly CI time (inference from today's
`build.yml` run times; the dry run measures it).

### 4.1 Draft assets at each point

Temporary assets have a fixed prefix so they can never be mistaken for release files.

| After step | Draft holds |
|---|---|
| 1 | `rpyenv-<v>-linux-x64.tar.gz`, `rpyenv-<v>-linux-arm64.tar.gz`, `install.sh`, `tmp-unsigned-exes-x64.zip`, `tmp-unsigned-exes-arm64.zip` |
| 2 | + `tmp-signed-exes-x64.zip`, `tmp-signed-exes-arm64.zip` |
| 3 | + `tmp-unsigned-msi-x64.zip`, `tmp-unsigned-msi-arm64.zip` |
| 4 | + `tmp-signed-msi-x64.zip`, `tmp-signed-msi-arm64.zip` |
| 5 | `rpyenv-<v>-x64.msi`, `rpyenv-<v>-arm64.msi`, the two tarballs, `install.sh`, `SHA256SUMS`; every `tmp-*` asset removed |

One change from the conversation's part 2: step 1 uploads the tarballs and `install.sh`
to the draft straight away, because they are never signed. Step 5 then needs nothing
from step 1's workflow run, so the 90-day artifact limit no longer applies.

### 4.2 Attestations

- Step 1 attests each unsigned executable (before zipping) and the tarballs and `install.sh`.
- Step 3 attests each unsigned MSI.
- Step 5 attests the final files: both signed MSIs, both tarballs, `install.sh` and
  `SHA256SUMS`.
- The zips are only containers; every check verifies the files inside them.
- Steps 3 and 5 run on the tag (`gh workflow run … --ref v<version>`), so their
  attestations carry `refs/tags/v<version>`, and the README's verify command works
  unchanged.

## 5. Workflows

| File | Started by | Does |
|---|---|---|
| `build.yml`, new `exes` job (split from today's `windows` job) | inside `build.yml` | Builds the 3 executables per architecture (x64 on `windows-2025`, ARM64 on `windows-11-arm`), runs `ci/pe_imports.py --check`, uploads the run artifact `exes-<arch>` |
| `msi.yml` (new, reusable, takes today's MSI steps) | `build.yml`, `release-msi.yml` | Downloads `exes-<arch>` from its own run, builds the MSI and the higher-version MSI, runs `test_msi_tables.ps1` and `test_msi.py`, uploads `msi-<arch>` |
| `build.yml` | `packaging.yml` (pull requests), `release.yml` | Runs `exes`, then `msi.yml` (only when asked for: input `msi: true`, the default), then the tarballs. Unsigned. |
| `release.yml` (changed) | a `v*` tag, or `workflow_dispatch` for a dry run | Step 1: version check, `build.yml` with `msi: false`, attestations (§4.2), draft creation with the step-1 assets (§4.1) |
| `release-msi.yml` (new) | the script, `workflow_dispatch`; inputs `tag`, `request` | Step 3 |
| `release-finish.yml` (new) | the script, `workflow_dispatch`; inputs `tag`, `request` | Step 5 |

Rules shared by `release-msi.yml` and `release-finish.yml`:

- `run-name` includes the `request` input, so the script can find the run it started.
- The first job checks that the run belongs to the draft. A release runs on its own tag.
  A dry run runs on a branch (`main` by default; any branch with `--ref`), and `github.sha`
  must equal the commit the draft points at, else "the branch moved since step 1; start a
  new dry run".
- GitHub only lets you start a workflow by hand once a file of that name is on the default
  branch; the run then uses the file from the `--ref` branch. Placeholder
  `release-msi.yml` and `release-finish.yml` therefore go to `main` in a small PR first. The
  card dry run (§8.2) then runs on the feature branch, before the real PR is merged.
- Only the job that writes to the draft has `contents: write`, `id-token: write` and
  `attestations: write`. Actions stay pinned by commit SHA.
- `workflow_dispatch` can only be started by accounts with write access to the repo.

### 5.1 Step 3: `release-msi.yml`

1. Download `tmp-unsigned-exes-<arch>.zip` and `tmp-signed-exes-<arch>.zip` from the draft.
2. Verify the unsigned executables with `gh attestation verify` (repo, signer workflow
   `release.yml`, source digest = the draft's commit).
3. For each signed executable: `ci/check_signed.py` (§7) proves that it is the unsigned
   file plus a signature, that the signer's thumbprint is the pinned one and that a
   timestamp is present. The names must match `signing.json` exactly.
4. Upload them as `exes-<arch>`, then call `msi.yml` (build and all MSI tests).
5. Attest the unsigned MSIs and upload `tmp-unsigned-msi-<arch>.zip` to the draft.

### 5.2 Step 5: `release-finish.yml`

1. Download `tmp-unsigned-msi-*` and `tmp-signed-msi-*`, verify the unsigned MSIs'
   attestations (signer workflow `release-msi.yml`), then run `ci/check_signed.py` on the
   signed MSIs as in step 3.
2. Take the signed `rpyenv-<v>-<arch>.msi` files (the zips already use the final names), download the tarballs and `install.sh` from the
   draft, write `SHA256SUMS` (`sha256sum rpyenv-* install.sh`, as today), attest the
   final files.
3. Upload the MSIs and `SHA256SUMS`, then delete every `tmp-*` asset.
4. The draft stays a draft; the maintainer publishes it.

## 6. The local script: `ci/release_sign.py`

| Command | Does |
|---|---|
| `python ci/release_sign.py v0.2.0` | Reads the draft and does the next step (§6.1) |
| `python ci/release_sign.py v0.2.0 --check` | Prints the state only; signs nothing, starts nothing |

A dry run's draft tag (`v0.2.0-dryrun.17`) is accepted the same way. For a dry run,
`--ref <branch>` (default `main`) names the branch steps 3 and 5 run on; a release refuses
`--ref`, since its steps always run on its tag.

### 6.1 State, from the draft's assets only

| Draft has | Script does |
|---|---|
| no draft for the tag | Stops: "no draft for v0.2.0; push the tag or start a dry run" |
| `tmp-unsigned-exes-*` only | Step 2: sign the executables, upload, start step 3, wait |
| `tmp-signed-exes-*`, no `tmp-unsigned-msi-*` | Looks for a `release-msi.yml` run for the tag. Running: waits. Failed: prints its link and starts step 3 again only if asked (`--retry`). None: starts step 3. |
| `tmp-unsigned-msi-*`, no `tmp-signed-msi-*` | Step 4: sign the MSIs, upload, start step 5, wait |
| `tmp-signed-msi-*`, no `SHA256SUMS` | Same as the row for step 3, for `release-finish.yml` |
| `SHA256SUMS`, every final file and some `tmp-*` | Step 5 stopped while tidying: check the final files (attestation, `SHA256SUMS`), then delete the `tmp-*` |
| `SHA256SUMS` and no `tmp-*` | Prints "done" and the draft's link |

### 6.2 Checks before signing (any failure stops before signtool runs)

1. The certificate with the pinned thumbprint is in `CurrentUser\My` and valid today;
   otherwise "Insert the Certum card".
2. Every file in the downloaded zip passes `gh attestation verify` against the repo, the
   workflow that should have built it (`release.yml` for executables, `release-msi.yml`
   for MSIs) and the draft's commit.
3. The zip's file names equal the list in `signing.json`, no more and no fewer.
4. No file has a signature yet.

### 6.3 Signing

One call per hand-off, over every file of both architectures:

```
signtool sign /sha1 <thumbprint> /fd sha256 /tr <timestamp URL> /td sha256
              /d rpyenv /du https://github.com/jmperez0/rpyenv <files>
```

- signtool is the newest x64 `signtool.exe` under `Windows Kits\10\bin`, or `--signtool`.
- If the call fails and its output names the timestamp server, the script runs it once
  more with the second timestamp URL from `signing.json`. That costs a second PIN prompt.
- Afterwards `signtool verify /pa /v` runs on every file; the script checks the signer
  thumbprint and that a timestamp is present. Then it zips, uploads (`--clobber`) and
  starts the next workflow.

### 6.4 Starting and following CI

- For a dry run, the branch's head must be the draft's commit before anything is signed.
- `gh workflow run <file> --ref <tag, or the dry run's branch> -f tag=<tag> -f request=<random hex>`.
- The run is found by the `request` value in its name (`gh run list --json`), then followed
  with `gh run watch --exit-status`. Not found within 2 minutes: the script stops and
  says so.

### 6.5 Files on the PC

Downloads go into a new folder from `tempfile.mkdtemp`, deleted at the end, also on
failure. Files are hashed and signed; nothing downloaded is run.

### 6.6 `installer/signing.json`

```json
{
  "thumbprint": "58E0373B2F4DF17A924C8B1EAB467FC7619A4403",
  "timestamp": ["http://time.certum.pl", "http://timestamp.digicert.com"],
  "description": "rpyenv",
  "url": "https://github.com/jmperez0/rpyenv",
  "exes": ["pyenv.exe", "pyenv-shim.exe", "pyenv-shimw.exe"],
  "msi": "rpyenv-{version}-{arch}.msi",
  "arches": ["x64", "arm64"]
}
```

The script and both CI workflows read it; a certificate change is a one-line PR.

## 7. `ci/check_signed.py` (CI, Windows runners)

`check_signed.py <unsigned> <signed> --thumbprint <hex>` exits 0 only when all of these hold:

1. `signtool verify /pa /v <signed>` passes, the signer's thumbprint is the given one and
   a timestamp is present.
2. The signed file is the unsigned file plus a signature, by the method chosen in §8.1.

The method candidates are:

- PE: `signtool remove /s` on a copy, then compare SHA-256 with the unsigned file; or
  compute the Authenticode digest of both files (the PE hash that skips the checksum
  field, the certificate-table entry and the certificate table) and compare.
- MSI: compare every OLE stream except `\x05DigitalSignature` and
  `\x05MsiDigitalSignatureEx`; or a `signtool remove` equivalent if one works on MSIs.

## 8. Testing

### 8.1 Measured first, before building on it

1. On a Windows runner, with a throwaway self-signed code-signing certificate
   (`New-SelfSignedCertificate -Type CodeSigningCert`): sign a built `pyenv.exe` and an
   MSI, run each comparison candidate, then change one byte of the signed file outside
   the signature and run it again. A method is accepted only if it passes the first case
   and fails the second.
2. On the dry run with the card: count the PIN prompts per signtool call.

### 8.2 Kept as tests

- **`ci/test_release_sign.py`** (runs in `coverage`, Ubuntu, `gh` and `signtool`
  stubbed): every row of §6.1; the card missing; an extra and a missing file name; a
  file already signed; the timestamp fallback; a run not found; the temporary folder
  removed after a failure.
- **`packaging.yml` on pull requests:** after `msi.yml`, a job signs the executables and
  an MSI with a throwaway self-signed certificate and runs `check_signed.py` (must pass),
  then flips one byte and runs it again (must fail).
- **Dry run with the card** before the v0.2.0 tag: the whole flow into a
  `-dryrun.N` draft; then `signtool verify /pa` and an MSI install on the rpyenv-s6 VM
  from its `clean` checkpoint, never on the development host. It also measures the time
  per release.

## 9. Failure handling

| What fails | What happens | What the maintainer does |
|---|---|---|
| Card removed, wrong PIN, signtool error | Nothing uploaded; temporary folder removed | Run the script again; it starts the step from scratch |
| First timestamp server down | Script retries with the second (second PIN prompt) | Nothing, unless both are down: try later |
| A `check_signed.py` or attestation check fails in CI | That run uploads nothing | Read the run log; this is a stop, not a retry |
| A build or test in step 3 or 5 fails | The draft does not change | A runner or network failure: `release_sign.py <tag> --retry`. A code fix needs a new tag or a new dry run (steps 3 and 5 run the drafted commit). |
| Step 5 stopped while removing `tmp-*` assets | Every final file is on the draft | Run the script again: it checks the final files' attestations (`release-finish.yml`) and `SHA256SUMS`, then removes the `tmp-*` assets itself |
| The dry run's branch moved | Step 3 or 5 fails with that message | Start a new dry run |
| Dropping a release | — | Delete the draft by hand; the script never deletes drafts or tags |

## 10. Documentation

- New `docs/releasing.md`: the five steps, the two commands, §9's table, and how to start
  a dry run.
- `README.md` ("Installing") and `docs/windows-install.md` (top): v0.1.0 is unsigned;
  from v0.2.0 the MSIs and executables are signed with a Certum code-signing
  certificate. SmartScreen may still warn while a new certificate has no download
  reputation (Microsoft's documented behaviour; not measured here).
- M6b spec: a note at D3 and §7.2 pointing to this spec.

## 11. Not in this phase

- Signing the Linux tarballs or `install.sh` (attestations and `SHA256SUMS` cover them).
- winget or Scoop manifests.
- Signing in CI with a cloud key (SignPath, Azure Artifact Signing); the card cannot be
  used there.
