# M7a: PyPy — design

Date: 2026-10-14. Status: agreed in brainstorming, awaiting review of this file.
Parent spec: `docs/specs/2026-09-27-rpyenv-design.md` (§14 M7, §15.1, §16).
M6 (setup, migrate, packaging, release workflow) is merged (PR #22, 8a94645).

## 1. Goal

`pyenv install` installs official PyPy binaries on Linux (x64, aarch64) and Windows
(x64), checked against hashes PyPy published, with the names and listings each OS's
drop-in target uses.

Success means:

- Linux: `pyenv install pypy3.11-7.3.20` (and `pypy3.11:latest`) works as upstream's
  does, and `install --list` shows upstream's PyPy lines.
- Windows: `pyenv update` lists PyPy as pyenv-win does, and
  `pyenv install pypy3.11-v7.3.20-win64` installs it, with pip.
- No install ever depends on the layout of a pypy.org web page for a version this
  rpyenv already knows.

## 2. Decisions (user, 2026-10-14)

| # | Question | Decision |
|---|---|---|
| D1 | Splitting M7 | M7a PyPy (this spec), M7b Anaconda, Miniconda and Miniforge (later), each with its own spec, plan and PR. |
| D2 | Version names | Each OS's own: Linux uses upstream pyenv's names (`pypy3.11-7.3.20`), Windows uses pyenv-win's codes (`pypy3.11-v7.3.20-win64`). Amends parent spec §15.1 choice 1, written before pyenv-win's naming was checked. |
| D3 | GraalPy | Stays a non-goal (parent §16). Windows keeps refusing GraalPy codes. |
| D4 | Linux source of truth | Upstream's PyPy definitions, **evaluated in CI** with stub functions into a table that rpyenv embeds; hashes cross-checked against pypy.org in CI; a user's own PyPy definition is evaluated with `bash` at run time (§4). Chosen over parsing bash in Rust, reading pypy.org at install time, and running upstream's `python-build`. |
| D5 | Windows hashes | A table built in CI from pypy.org's `versions.json` and `checksums.html`, embedded in rpyenv; reading `checksums.html` at install time only for a release newer than the table (§5). Chosen after a side-agent note: reading the page on every install would let a pypy.org redesign break every Windows PyPy install. |
| D6 | Windows file without a published hash | Not installable (no table row; the fallback refuses), never a warning. |
| D7 | Windows ARM64 hosts | As pyenv-win: x64 PyPy rows are listed and installable (they run under emulation). |

## 3. Measurements this design rests on (2026-10-14)

| Fact | Method | Result |
|---|---|---|
| Shapes of upstream's PyPy definitions | `C:\tmp\m7_pypy_defs.py` over upstream pyenv 2.6.6's `share/python-build` (WSL) | 241 `pypy*` files: 116 `-src`, 78 inline `case "$(pypy_architecture)"`, 39 template (`VERSION`, `PYVER`, `*_hash`, fixed tail), 8 other (`pypy-5.x` that `source` another file, `pypy-dev`) |
| Upstream's binary definitions can be evaluated with stubs | `C:\tmp\m7_eval_pypy.sh`: each non-`-src` file sourced in a subshell with stub `pypy_architecture`, `install_package`, `install_zip`, `colorize`, `require_osx_version`, for `linux64` and `linux-aarch64` | 179 pairs printed one name, URL and SHA-256 (all shapes, including `source`); 65 stopped on upstream's own "no binary" error; 6 (nightlies, `pypy-dev`) called functions the stubs lack: refused, as intended |
| Upstream's hashes agree with pypy.org | `C:\tmp\m7_crosscheck.py`: the 179 rows against `checksums.html` | 116 match, **0 differ**, 63 absent from the page, all PyPy 7.2.0 or older (newest absent: `pypy3.6-7.2.0`) |
| pypy.org's machine-readable list has no hashes | `C:\tmp\m7_pypy_sources.py` over `versions.json` | 104 releases; file keys `arch`, `download_url`, `filename`, `platform`; no hash key on any file |
| Windows files with a published hash | same script, `checksums.html` | 97 of 98 Windows files; the one without is the nightly `pypy-c-jit-latest-win64.zip` |
| Windows zip layout | `C:\tmp\m7_zip_listing.py` (HTTP range reads of the central directory) on `pypy3.11-v7.3.20-win64.zip` | One top-level folder; its root has `python.exe`, `pythonw.exe`, `python3.exe`, `python3.11.exe`, `pypy.exe`, `pypy3.exe`, `pypy3.11.exe`, `pypyw.exe`, `pypy3.11w.exe`; `Lib\ensurepip` present; no `Scripts\` |

The 2.6.6 checkout is older than the vendored pin (2.8.9); the plan's first task
repeats the evaluation and cross-check on the pinned copy.

## 4. Linux

### 4.1 The table, built by the sync job

`.github/workflows/sync-python-build.yml` and `ci/sync_python_build.py` are extended:

1. **Vendoring.** Every upstream `pypy*` definition is copied into
   `crates/pyenv/python-build/share/`, `-src` ones included, so `install --list`
   matches upstream. PyPy patches are not copied (they serve source builds only).
2. **Evaluation.** `crates/pyenv/src/install/pypy_eval.sh` (run by `ci/pypy_tables.py`)
   sources each binary definition with stub
   python-build functions, once per architecture (`linux64`, `linux-aarch64`), and
   prints what it would install. It runs in a container started with
   `--network none` and the definitions mounted read-only (podman locally, docker on
   the runner); the stubs download nothing.
3. **Output.** `crates/pyenv/pypy/linux.tsv`, one row per vendored `pypy*`
   name and architecture, with a status:
   - `ok`: package directory, URL, SHA-256, build steps (`pypy verify_pyXY ensurepip`);
   - `no-binary`: the definition stopped on upstream's "binary not available" error;
   - `unsupported`: `-src`, or a definition that called a function the stubs lack
     (nightlies, `pypy-dev`).
   The §6 refusal for a name comes from its status. `unsupported` is allowed only for
   `-src` names and the names listed in `crates/pyenv/pypy/linux-unsupported.txt` (from the first
   run: the nightlies and `pypy-dev` of §3); any other stub gap fails the job.
4. **Cross-check.** The job fetches pypy.org's `checksums.html`. A row whose file is on
   the page must carry the same SHA-256, or the job fails. A row whose file is not on
   the page must be listed in `crates/pyenv/pypy/linux-unpublished.txt`
   (created from the first run: the 63 rows of §3), or the job fails.
5. Any failure stops the job before a pull request is opened, so a changed definition
   shape, stub gap or page layout fails in CI, never on a user's machine.

### 4.2 How rpyenv uses it

- `build.rs` embeds the table. Definitions are found in the existing order; when the
  one found is the vendored (`Origin::Builtin`) copy of a `pypy*` name, `defs.rs`
  builds the definition from this host's row: `install_package <dir>
  <url>#<sha256> pypy verify_pyXY ensurepip`. The existing interpreter and builder
  run it. A vendored `pypy*` name with no row for this host gets the matching §6
  refusal.
- **A user's own PyPy definition** (a `pypy*` name found in `PYTHON_BUILD_DEFINITIONS`,
  a plugin, or given as a path) that the interpreter does not accept is evaluated at
  run time: rpyenv runs
  `bash` with the same stub script (`pypy_eval.sh`, embedded in the binary) and the
  host's architecture, and takes the one row it prints. Upstream sources definitions
  with bash too, so this adds no new trust. Without `bash`, the error names the file.

### 4.3 The install

The existing pipeline runs: download, hash check, extract into the temporary prefix,
steps, rename into place, default-packages, rehash. The pre-flight check for build
dependencies is skipped for a PyPy definition (nothing is compiled).

New step `pypy` (upstream's `build_package_pypy`):

1. Copy the extracted tree into the temporary prefix.
2. Create `bin/` and `lib/`.
3. For each regular, executable, non-link `bin/pypy*`: link `bin/python` to it.
4. For each regular, executable, non-link `bin/libpypy*`: link `lib/<name>` →
   `../bin/<name>`.

`verify_pyXY` and `ensurepip` already exist (`builder.rs`, `verify.rs`).

## 5. Windows

### 5.1 The table and `update`

- The same sync job reads `versions.json` (files, versions) and `checksums.html`
  (hashes) and writes `crates/pyenv/pypy/windows.tsv`: pyenv-win code,
  URL, SHA-256. A file without a hash on the page gets no row. The code is pyenv-win's:
  the zip's file name without `.zip`, for files pyenv-win's own pattern accepts
  (`pypy<X.Y>-v<version>-win64`).
- The job runs weekly even when upstream pyenv has not changed, so a new PyPy release
  reaches the table within a week, through a pull request the maintainer merges.
- `pyenv update` adds PyPy rows from `versions.json` to `.versions_cache.xml`, in
  pyenv-win's format and order (CPython rows first, then the others in pyenv-win's
  order, as `wincatalog.rs` already sorts them). The cache holds names only, as for
  CPython (parent §9.1). With `PYTHON_BUILD_MIRROR_URL` set, PyPy is not read (pyenv-win's
  banner then names only the mirror).
- **A failed PyPy read never blocks the python.org list** (user's decision, 2026-10-14):
  `update` saves the fresh CPython rows, keeps the PyPy rows already in the cache, prints
  the HTTP error and a warning, and exits 0.

### 5.2 The install

`winpkg.rs` gets a PyPy-zip kind:

1. **Hash.** The table row for the code. A code missing from the table (a release newer
   than this rpyenv) falls back to fetching `checksums.html` and finding that file's
   SHA-256. If the page can't be fetched or parsed, or has no hash for the file:
   refuse (D6).
2. **Download and check** against that hash.
3. **Extract** with `zipx`: the zip's single top-level folder becomes
   `versions\.tmp-<code>`.
4. **Finish:** `python -m ensurepip -U --default-pip` (the zip has `Lib\ensurepip`).
   No `pythonX.Y.exe` copies: the zip already has `python3.exe`, `python3.11.exe` and
   `pypy3.11.exe`.
5. Rename into place, then rehash. Shims come from the folder root and `Scripts\`, the
   rule CPython already uses. No registry writes (pyenv-win registers no PyPy either).

## 6. Errors

Each names the version, exits 1, and leaves nothing in `versions/`.

| Case | Message |
|---|---|
| `-src`, a nightly (`*-c-jit-latest`), `pypy-dev`, a distribution-specific definition (`require_distro`: 15 of PyPy 7.2.0 and older, which pick PyPy's build on an old Ubuntu and a third-party portable build elsewhere) | `PyPy source, nightly and distribution-specific builds are not supported` |
| No binary for this architecture (Linux) | upstream's text: `The binary distribution of PyPy is not available for <arch>.` |
| Hash mismatch | the existing checksum error |
| Windows code not in the table, fallback fails | `<code> is newer than this rpyenv's PyPy table and pypy.org's checksum page couldn't be read: update rpyenv` |
| A user's own PyPy definition, no `bash` | `<file> needs bash to be read` |
| GraalPy (Windows) | the existing refusal, reworded to name GraalPy |

## 7. Parity and allowlist

- **D-55** (Linux `install --list`): narrowed to conda and the other families; a golden
  covers the PyPy lines.
- **D-79** (Windows `update`): the PyPy half closes; GraalPy stays.
- **D-82** (Windows `install`): narrowed to GraalPy.
- **New rows:** Windows: rpyenv makes no `python<pypy…>.exe` copies (pyenv-win splits
  the code on dots and writes names like `pythonpypy3.exe`); it checks the SHA-256,
  where pyenv-win checks none. (Both run `ensurepip`: the zip has `Lib\ensurepip`, which
  is pyenv-win's condition, so pip is not a difference.) Linux: `-src`, nightlies and
  `pypy-dev` are refused where upstream builds or installs them.
- `parity/diff_cases.py` gains PyPy `install --list` and `latest` cases against
  upstream.

## 8. Testing

**Unit (Rust):** table parsing and lookup (wrong architecture, missing or malformed
row refused); the definition built from a row runs through the interpreter; the `pypy`
step's links on a fake extracted tree (`bin/python` → `pypy3.11`,
`lib/libpypy3.11-c.so` → `../bin/…`, links skipped); Windows code ↔ table row; the
fallback `checksums.html` parser on a saved copy of the page.

**CLI** (fake-download harness `tests/common/fakebuild.rs`, Windows fixtures):

- Linux: install from the table (hash checked, links, `verify` and `ensurepip` run
  against a fake `pypy`); `-src`, nightly and no-binary refusals leave nothing; a
  custom-directory definition goes through `bash`, and without `bash` the error shows.
- Windows: `update` writes PyPy rows in pyenv-win's format and order (against the
  pyenv-win fixture's PyPy lines); install from the table; install through the
  fallback; the fallback failure says to update rpyenv; a hash mismatch is refused;
  GraalPy is still refused.

**Sync job** (`ci/test_sync_python_build.py`): the evaluator on fixture definitions of
every shape, `source` included; the cross-check (a mismatch fails, an unlisted absent
row fails, a listed one passes); table and vendored files in step; a test that the
evaluator's container really has no network.

**Real installs on CI** (nightly schedule plus path triggers):

- `install-linux.yml`: the newest `pypy3.11` on x64 and arm64; `ci/check_install.sh`
  runs `python -c 'import ssl, sqlite3, ctypes'`, `pip --version` and a shim.
- `install-windows.yml`: the newest `pypy3.11-…-win64`; `ci/check_install_win.py`
  checks the same and that `Scripts\pip.exe` exists.

## 9. Parent-spec updates (with this milestone)

- §15.1 choice 1: names follow each OS's drop-in target (D2).
- §15.1 resolution 1: for PyPy, superseded by D4 and D5 (tables built in CI from
  upstream's definitions and from pypy.org, cross-checked; no catalog published by
  rpyenv; `RPYENV_CATALOG_URL` stays unused).
- §14: M7 split into M7a (PyPy) and M7b (conda).
- README: PyPy in the install section.

## 10. Risks

1. **Stub coverage.** A future upstream definition may call a python-build function
   the stubs lack. Effect: the sync job fails; the new version is missing until the
   stubs grow. No user-facing failure.
2. **pypy.org layout.** A redesign of `checksums.html` fails the sync job (both tables)
   and the Windows fallback; versions already in the table keep installing.
3. **Windows freshness.** A PyPy release is installable on Windows without the
   fallback only after the weekly job's pull request is merged and rpyenv is released.
4. **Container on the runner.** The evaluation needs docker on `ubuntu-latest`
   (present today); without it the job fails rather than evaluating with network.
