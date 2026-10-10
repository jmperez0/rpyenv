# M6b: packaging and release — design

Date: 2026-10-10. Status: agreed in brainstorming, awaiting review of this file.
Parent spec: `docs/specs/2026-09-27-rpyenv-design.md` (§9.4, §14 M6, §17 question 2).
M6a (setup, migrate, `init --install pwsh`, first run, native arch) is merged.

## 1. Goal

A public **v0.1.0** that people can install from a GitHub release without building
rpyenv, on Windows and Linux, and remove cleanly.

Success means:

- Windows users install one MSI, either just for themselves (no admin) or for all
  users, interactively or silently, and uninstall it from Settings.
- Linux users run one `curl … | sh` line, on x64 or arm64, on any distribution,
  including over an existing upstream pyenv checkout.
- A maintainer pushes a `v*` tag and gets a draft release with every asset, checksums
  and build attestations, already tested.

## 2. Decisions (user, 2026-10-10)

| # | Question | Decision |
|---|---|---|
| D1 | Audience | Public v0.1.0 for others. |
| D2 | Builds | Windows x64 and ARM64; Linux x64 and arm64 (static musl). |
| D3 | Code signing | Unsigned for v0.1.0. The release workflow has a signing step that runs only when its secrets exist. The provider (SignPath Foundation or Azure Artifact Signing, see §9) is chosen later. |
| D4 | Linux install location | `$PYENV_ROOT/bin` (and `$PYENV_ROOT/completions`), so existing pyenv startup lines work unchanged. |
| D5 | Linux shell setup | Ask, default yes; non-interactive runs print the lines; `--init`/`--no-init` force it. |
| D6 | Windows uninstall | For the uninstalling user: `pyenv migrate --restore` (if a backup exists), then `pyenv setup --undo`. Other users of an all-users install run `pyenv setup --undo` themselves (documented). `PYENV_ROOT` and Python versions are always kept. |
| D7 | Channels | GitHub Releases only for v0.1.0 (no winget or Scoop yet). |
| D8 | Upstream pyenv checkout on Linux | `install.sh` offers to take it over (`--take-over` non-interactively) and can give it back (`--restore-upstream`). |
| D9 | MSI tooling | WiX **5.0.2**, pinned. It is MS-RL licensed. WiX 6 packages carry the Open Source Maintenance Fee EULA; 5.0.2's package page shows no such notice. |

## 3. What M6b delivers

| Piece | Purpose |
|---|---|
| `installer/rpyenv.wxs` (+ WiX 5.0.2) | One dual-mode MSI per Windows architecture (§4). |
| `pyenv setup --undo` | Removes what `pyenv setup` added for this user (§5). |
| `install.sh` | Linux installer, upgrader and uninstaller (§6). |
| `.cargo/config.toml` | Static C runtime for Windows MSVC targets (§7). |
| `.github/workflows/release.yml` | Tag-triggered build, verify, publish (§7). |
| `.github/workflows/packaging.yml` | The same build and verification on pull requests that touch packaging (§7). |
| README and parent-spec updates | §8. |

## 4. The Windows MSI

### 4.1 Package

- WiX 5.0.2, `Package Scope="perUserOrMachine"`, one MSI per architecture:
  `rpyenv-<version>-x64.msi`, `rpyenv-<version>-arm64.msi`.
- Product version = the workspace version. A fixed `UpgradeCode`.
- `MajorUpgrade`: a newer MSI replaces an older one in place; the same version again
  does nothing; an older version is refused with a message.

### 4.2 Layout (parent spec §9.4 table)

| | Just for me | For all users |
|---|---|---|
| Install folder | `%LOCALAPPDATA%\Programs\rpyenv` | `%ProgramFiles%\rpyenv` |
| Contents | `bin\pyenv.exe`, `bin\pyenv-shim.exe`, `bin\pyenv-shimw.exe`, `completions\pyenv.pwsh` (and the other completion scripts), `LICENSE`, `README.md` | same |
| `bin` on `PATH` | user `PATH` (appended) | machine `PATH` (appended) |
| `RPYENV_LIVE_REHASH=1` (if chosen) | user environment | machine environment |
| Active Setup key | — | `HKLM\SOFTWARE\Microsoft\Active Setup\Installed Components\{rpyenv}` |

WiX fixes an `Environment` element's user/machine choice at build time, so each
environment entry is a pair of components conditioned on `ALLUSERS`.

### 4.3 Dialogs

- WiX's standard scope choice (`WixUI_Advanced`): **Just for me** / **For all users**
  (the all-users button carries the UAC shield; Windows Installer raises the UAC
  prompt when installation starts).
- One **Options** page:
  - **Enable live rehash** — unchecked by default (parent spec D8: opt-in). Property
    `LIVEREHASH`.
  - **Move pyenv-win aside (`pyenv migrate`)** — shown only when pyenv-win is found for
    this user (`%USERPROFILE%\.pyenv\pyenv-win\bin\pyenv.ps1` exists); checked by
    default when shown. Property `MIGRATE`. Per-user installs only (an all-users
    install relies on `pyenv setup`'s hint, parent spec §9.4).

### 4.4 Custom actions

All run `pyenv.exe` from the installed `bin`, hidden (`WixQuietExec`), as the
installing user (deferred, impersonated). None can fail the install or uninstall:
exit codes and output go to the MSI log.

| When | Runs | Notes |
|---|---|---|
| Per-user install | `pyenv migrate` if `MIGRATE=1`, then `pyenv setup` | If setup fails, the final page tells the user to run `pyenv setup`. |
| All-users install | — | The Active Setup key runs `"<bin>\pyenv.exe" setup` once per user at logon; the M6a first-run rule covers a user who runs `pyenv` sooner. |
| Uninstall, not during an upgrade | `pyenv migrate --restore` if `PYENV_ROOT\.rpyenv-migrate\manifest.txt` exists, then `pyenv setup --undo` | Runs before the files are removed. |

### 4.5 One install per machine

A per-user and an all-users install of the same `UpgradeCode` don't see each other, and
both `bin` folders would end up on `PATH`. Each install writes a scope marker
(`HKCU\Software\rpyenv\Installed` or `HKLM\Software\rpyenv\Installed`); a launch
condition refuses to install in one scope while the other's marker exists, and names
the install to remove first.

### 4.6 Silent installs

- Per-user: `msiexec /i rpyenv-<version>-x64.msi /qn`.
- All users: `msiexec /i rpyenv-<version>-x64.msi /qn ALLUSERS=1 MSIINSTALLPERUSER=""`
  from an elevated prompt (for example `sudo msiexec …`). From a non-elevated prompt it
  is expected to fail without a UAC prompt; M6b verifies this and documents the result.
- Optional properties: `LIVEREHASH=1`, `MIGRATE=1`. A silent install never migrates
  unless `MIGRATE=1` is passed.

## 5. `pyenv setup --undo`

Windows-only, like `pyenv setup`. For the current user:

- removes the shims entry from the user `Path` (any spelling `pathlist::same` matches,
  keeping the value's `REG_EXPAND_SZ`/`REG_SZ` type), then broadcasts `WM_SETTINGCHANGE`;
- removes the exact PowerShell line (``iex ((pyenv init - pwsh) -join "`n")``) from
  both profiles, keeping each file's encoding (`pwsh_profile::remove`);
- deletes `PYENV_ROOT\.rpyenv-setup` and `.rpyenv-setup-failed`.

It never touches `PYENV_ROOT`'s versions, the shims folder's files, or a `Path` it
cannot read (same rule as `setup`, M6a I5). It is idempotent and exits 0 when there is
nothing to undo; it exits 1 when a step fails.

## 6. Linux: tarball and `install.sh`

### 6.1 Tarball

`rpyenv-<version>-linux-x64.tar.gz` and `-linux-arm64.tar.gz`, containing
`bin/pyenv`, `bin/pyenv-shim`, `completions/pyenv.{bash,zsh,fish}`, `LICENSE`,
`README.md`. It unpacks into `$PYENV_ROOT`, upstream's layout, so `pyenv init` finds
the completions (`install_prefix()` is the folder above `bin`).

### 6.2 `install.sh`

POSIX `sh` (runs under dash). Usage:

```sh
curl -fsSL https://github.com/jmperez0/rpyenv/releases/latest/download/install.sh | sh
# options: sh -s -- [--version vX.Y.Z] [--init|--no-init] [--take-over]
#                   [--restore-upstream] [--uninstall]
```

Steps:

1. Linux only; `uname -m` must be x86_64/amd64 or aarch64/arm64. Otherwise it stops
   with a message.
2. Downloads the tarball and `SHA256SUMS` (curl or wget; base URL overridable by
   `RPYENV_INSTALL_BASE_URL` for tests) and verifies with `sha256sum` or
   `shasum -a 256`. A mismatch or a missing tool stops it before anything is written.
3. `PYENV_ROOT` defaults to `~/.pyenv`.
4. **Upstream checkout** (`$PYENV_ROOT/libexec/pyenv` or `$PYENV_ROOT/.git` exists):
   it asks to take over (non-interactively it stops unless `--take-over` is given).
   Taking over moves every top-level entry of `$PYENV_ROOT` into
   `$PYENV_ROOT/.upstream-pyenv/` except a keep-list of the user's data: `versions/`,
   `version`, `shims/`, `cache/`, `plugins/` (whose `python-build` alone is moved,
   since rpyenv embeds its own), and `.upstream-pyenv/` itself. It records what it
   moved in `.upstream-pyenv/moved.txt`. `--restore-upstream` removes rpyenv's `bin`
   and `completions`, then moves back exactly what `moved.txt` lists.
5. Unpacks into a temporary folder, then moves `bin/` and `completions/` into place, so
   a failed run leaves the previous install working. Running it again upgrades.
6. Shell setup: when `/dev/tty` is usable (which works under `curl | sh`), asks
   "Add pyenv to <startup file>? [Y/n]" and runs `pyenv init --install <shell>`
   (`<shell>` from `$SHELL`); otherwise prints the lines to add.
7. `--uninstall` removes only the files it installed (`bin/pyenv`, `bin/pyenv-shim`,
   `completions/pyenv.*`) and prints the startup lines to delete. Python versions stay.

## 7. Builds and the release workflow

### 7.1 Builds

- `.cargo/config.toml`: `-C target-feature=+crt-static` for
  `cfg(all(windows, target_env = "msvc"))`, so no binary needs `vcruntime140.dll`
  (not present on every machine, especially ARM64). CI then tests the same kind of
  binary that ships.
- Linux: `*-unknown-linux-musl` targets on native runners, with `musl-tools` (for
  `ring`, the only C-backed dependency). The job checks that the binaries are static.
- The existing release profile stays (`lto`, `codegen-units = 1`, `panic = "abort"`,
  `strip`). Builds use `--locked` and the toolchain from `rust-toolchain.toml` (1.96).

### 7.2 `release.yml`

Trigger: a `v*` tag; also `workflow_dispatch` as a dry run that builds and verifies but
publishes nothing.

1. **Version check:** the tag equals `v<workspace version>`, or the run stops.
2. **Windows** (x64 runner and Windows 11 ARM64 runner): build; sign the `.exe` files
   (skipped without secrets); build the MSI with WiX 5.0.2; sign the MSI (skipped
   without secrets).
3. **Linux** (x64 and arm64 runners): musl build, static check, tarball.
4. **Verify** the exact artifacts (§7.4).
5. **Publish** (the only job with `contents: write`, `id-token: write`,
   `attestations: write`): `SHA256SUMS`; build-provenance attestations for every asset
   (`actions/attest-build-provenance`); a **draft** release with the 2 MSIs, 2
   tarballs, `install.sh` and `SHA256SUMS`, and generated notes. A maintainer publishes
   it.

Conventions as in the current CI: actions pinned by commit SHA, minimal permissions per
job.

### 7.3 `packaging.yml`

Runs steps 2–4 on pull requests that change `installer/`, `install.sh`, `.cargo/`,
the workflows or the release profile, so packaging breaks are found before a tag.

### 7.4 Verification

- **MSI (CI, Windows runners, both architectures):** silent per-user install →
  files, user `PATH` (bin, and shims first), marker, profile line; silent uninstall →
  files gone, `PATH` entries gone, profile line gone, `PYENV_ROOT` kept. Silent
  all-users install → `Program Files`, machine `PATH`, Active Setup key; uninstall.
  An upgrade from a test build with a lower version. The scope-marker refusal.
- **MSI (interactive, Windows Sandbox on the development host):** the dialogs, the UAC
  prompt, `MIGRATE` with a pyenv-win fixture, the Active Setup run at logon (and
  whether a console window flashes), a silent all-users install from a non-elevated
  prompt. The development host's own rpyenv, pyenv-win and `PATH` are never touched.
- **`install.sh` (CI, both Linux architectures, throwaway `HOME`, a locally served
  release):** fresh install; upgrade; a tampered tarball (refused); an upstream
  checkout non-interactively (refused), with `--take-over`, then
  `--restore-upstream`; non-interactive shell setup (prints the lines); `--init`;
  `--uninstall`. `shellcheck` on the script.

## 8. Documentation

- README: an "Installing" section replacing "Installing (planned)" — the MSI (both
  modes, silent options), the Linux one-liner and its options, taking over an upstream
  checkout, uninstalling, and verifying downloads (`SHA256SUMS`,
  `gh attestation verify`). That v0.1.0 is unsigned and what users see because of it
  (SmartScreen; Smart App Control blocks unsigned executables).
- Parent spec: §9.4 as built; a new §9.5 "Installing rpyenv itself on Linux"; §17
  question 2 answered (§9 below).

## 9. Code signing: research (2026-10-10) and the decision

Official sources: SignPath Foundation's terms (https://signpath.org/terms) and
Microsoft's Artifact Signing quickstart
(https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart, updated
2026-10-08). Other figures come from third-party reports and are marked.

| Option | Cost | Publisher shown | CI | Conditions |
|---|---|---|---|---|
| SignPath Foundation | free | SignPath Foundation | GitHub Actions step | OSI license; the project must already be released; manual approval per release; MFA; a "Code signing policy" page with the attribution "Free code signing provided by SignPath.io, certificate by SignPath Foundation". |
| Azure Artifact Signing | $9.99/month (Basic) | the developer or organization | official action | Individuals must be in the US or Canada; organizations in the EU, UK and others qualify; validation takes 1–20 business days. |
| Certum Open Source | about €50/year (third-party, unverified) | the developer | hard to automate (phone 2FA) | Authenticode support and cloud availability unconfirmed. |
| Commercial OV (DigiCert, Sectigo, …) | several hundred dollars a year (from memory) | the developer | via a cloud HSM | No instant reputation. |
| Unsigned | free | Unknown publisher | — | SmartScreen warning; Smart App Control blocks the executables (reports say new signed software can also be blocked until it builds reputation). |

Decision (D3): v0.1.0 ships unsigned; the workflow's signing step is ready. SignPath
Foundation fits once v0.1.0 is public (its terms require a released project); Azure
fits if the maintainer has a qualifying business. Either plugs into the same step.

## 10. Risks the plan settles first

1. WiX 5: dual-mode package + `WixUI_Advanced` + the Options page, prototyped before
   the rest of the MSI.
2. The Windows 11 ARM64 runner: label, availability for this public repository, MSI
   build and install on it.
3. A static musl build of `ring` on arm64.
4. Static CRT: nothing in CI may expect the dynamic one (for example
   `ci/shim_deps.py`, which checks the shims' DLL imports).
5. Silent all-users install from a non-elevated prompt; the Active Setup console flash
   (both in Windows Sandbox).

## 11. Not in M6b

- Code signing itself (D3), winget and Scoop (D7).
- Shared, admin-managed Python versions (parent spec §15.3).
- macOS.
