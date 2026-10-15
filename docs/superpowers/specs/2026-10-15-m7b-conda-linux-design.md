# M7b: Anaconda, Miniconda and Miniforge on Linux — design

Date: 2026-10-15. Status: agreed in brainstorming, awaiting review of this file.
Parent spec: `docs/specs/2026-09-27-rpyenv-design.md` (§8, §10, §14 M7, §15.1).
Pattern: M7a's spec, `docs/superpowers/specs/2026-10-14-m7a-pypy-design.md` (merged, PR #25).

## 1. Goal

On Linux (x86_64, aarch64), `pyenv install` installs Anaconda, Miniconda and Miniforge
from their official installers, with upstream pyenv's names, each download checked
against a published SHA-256, and conda's bundled system tools don't get shims, as
upstream's rehash hook arranges.

Success means:

- `pyenv install miniforge3-<release>`, `miniconda3-<release>`, `anaconda3-<release>`
  (and the `-latest` names conda's publishers still serve) work as upstream's do.
- `install --list` shows upstream's conda lines.
- Once any version has `bin/conda`, the names in upstream's `conda.d/default.list`
  (`curl`, `sed`, `openssl`, `clear`, …) get no shim (allowlist D-44 closes).
- `pyenv virtualenv` on a real conda install runs `conda create` (M4b's code, first real
  check).

Windows (installer, version and hash source, launch-plan `PATH`, ownership-based shims,
conda envs) is **M7c**, with its own spec.

## 2. Decisions (user, 2026-10-14)

| # | Question | Decision |
|---|---|---|
| D1 | Splitting M7b | By OS: M7b Linux (this spec), M7c Windows. |
| D2 | The 6 definitions upstream installs with no hash | Checked at install time against the publisher's own hash: `miniconda*-latest` against the SHA-256 column of `repo.anaconda.com/miniconda/`, `miniforge3-latest` against the release's versioned `.sha256` file. A name with no such source, or a hash that can't be read, is refused. |
| D3 | `CONDA_PLUGINS_AUTO_ACCEPT_TOS=true` in 75 `miniconda3` definitions | As upstream (the variable is set for the installer and the `conda install` step), plus a one-line notice before the install (§4.3). |
| D4 | Which conda executables get shims | Upstream's `conda.d/default.list`, exactly (§5). The parent spec's package-ownership rule stays a Windows design (M7c). |
| D5 | How rpyenv reads the definitions | M7a's pattern: CI evaluates the vendored definitions with stubs in a no-network container into an embedded table; a user's own definition is evaluated with `bash` at install time. Chosen over a Rust reader for the `case … install_script … esac` shape and over evaluating every definition with `bash` on the user's machine. |
| D6 | How safe `-latest` is (after a side-agent note) | Said in this spec and the README, without a runtime warning: a `-latest` hash comes from the download's own publisher, so it catches a broken download, not a compromised publisher; a numbered version carries upstream's reviewed hash. Windows CPython's hashes already come from python.org itself. |
| D7 | What `install -f` carries over into a reinstalled conda version (after a side-agent note, 2026-10-15) | Only `envs/`. A conda package spans `lib/`, `site-packages` and `conda-meta/`, so the CPython rule (extra `bin/` and `site-packages` entries, D-58) would carry half-packages. Before the old copy is deleted, rpyenv prints the base packages it didn't carry (conda ones by `conda-meta/` record, pip ones by `INSTALLER`), with ready `conda install` / `pip install` commands. Chosen over the CPython rule, upstream's install over the old folder (no rollback), and carrying whole conda packages (breaks across releases). |

## 3. Measurements this design rests on (2026-10-14)

| Fact | Method | Result |
|---|---|---|
| Shapes of upstream's conda definitions | `C:\tmp\m7b_defs.py` over the pinned upstream (pyenv 2.8.9, `8000f98`) | 445 files: anaconda 17, anaconda2 20, anaconda3 48, mambaforge 55, miniconda 13, miniconda2 19, miniconda3 191, miniforge 1, miniforge3 81. 441 are one `case "$(anaconda_architecture …)"` with `install_script "<name>" "<url>[#sha256]" anaconda\|miniconda verify_<x>` per arch; 4 (`anaconda-2.4.0`, `anaconda-4.0.0`, `miniconda-3.18.3`, `miniconda-latest`) are a one-line `source` of their `*2` twin. 75 also `export CONDA_PLUGINS_AUTO_ACCEPT_TOS=true` (all `miniconda3`: every 25.x and `-latest`). |
| Definitions with no hash | same script | 6: `miniconda2-latest`, `miniconda3-latest`, `miniforge3-latest`, `mambaforge`, `mambaforge-pypy3`, `miniforge-pypy3`. Every other definition hashes every URL. |
| Download hosts | `C:\tmp\m7b_hosts.py`, same tree | Every URL (1862) starts with one of `https://repo.anaconda.com/miniconda/` (782), `https://github.com/conda-forge/miniforge/` (668), `https://repo.continuum.io/archive/` (184), `https://repo.continuum.io/miniconda/` (128), `https://repo.anaconda.com/archive/` (100). |
| Publisher hashes for `-latest` | `curl` and `gh api` against the live sources | `repo.anaconda.com/miniconda/` lists `Miniconda3-latest-Linux-x86_64.sh` with a SHA-256 column. Miniforge's latest release (26.7.2-0) has no `.sha256` for the unversioned `Miniforge3-Linux-x86_64.sh`, but has `Miniforge3-26.7.2-0-Linux-x86_64.sh.sha256`; GitHub reports the same digest for both installers. `mambaforge`, `mambaforge-pypy3` and `miniforge-pypy3` (unversioned) answer 404; a numbered `Mambaforge-24.3.0-0` installer answers 200. |
| Upstream's install steps | `bin/python-build` lines 1297-1331 | `build_package_anaconda`: `bash <name>.sh -f -b -p <prefix>`. `build_package_miniconda`: the same, then `<prefix>/bin/conda install --yes pip conda=<installed conda version>` (pyenv issue #2070). `anaconda_architecture` maps `x86_64` → `Linux-x86_64`, `aarch64` → `Linux-aarch64`. `verify_py3_latest` is `verify_py311 "$1" "3"` (checks `bin/python3`). |
| Upstream's shim filter | `pyenv.d/rehash/conda.bash`, `conda.d/default.list` | When any `versions/*/bin/conda` or `versions/*/envs/*/bin/conda` exists (existence, not execute bit), every registered shim whose name is on the list (177 names after removing comments and blank lines) is dropped, whichever version provides it. |
| What shimming those names does today | `C:\tmp\m7b_shadow.sh` in WSL: scratch root, a fake `miniconda3-x` with `bin/conda` and `bin/curl`, a `3.12.0`, rpyenv's debug build | `curl` gets a shim. Global `3.12.0` or `3.12.0 system`: the shim runs `/usr/bin/curl`. Global `3.12.0 miniconda3-x system`: the shim runs conda's `curl`. Upstream (no shim) runs `/usr/bin/curl` in all three. |
| No conda in pyenv-win | grep of `C:\Users\JM\.pyenv\pyenv-win` (versions excluded) for `conda`, `miniforge` | 0 matches: Windows has no drop-in names to follow (M7c decides them). |

Plan Task 1 repeats the evaluation on the pinned copy and reports, per family and arch,
how many rows are `ok`, `ok-latest`, `no-binary` and `unsupported`.

## 4. Linux

### 4.1 The table, built by the sync job

1. **Vendoring.** `ci/sync_python_build.py` copies every upstream definition whose name
   starts with `anaconda`, `miniconda`, `miniforge` or `mambaforge` into
   `crates/pyenv/python-build/share/python-build`, unpatched, as it does for `pypy*`. It
   also copies `pyenv.d/rehash/conda.d/default.list` to
   `crates/pyenv/conda/default.list`, with an attribution header naming upstream's MIT
   license.
2. **Evaluation.** A stub evaluator (`crates/pyenv/src/install/conda_eval.sh`, the shape
   of `pypy_eval.sh`) sources each definition in a subshell, for `Linux-x86_64` and
   `Linux-aarch64`, with stub `anaconda_architecture`, `install_script` and `colorize`.
   `ci/conda_tables.py` (the shape of `ci/pypy_tables.py`) runs it in the same
   no-network container image, pinned by digest, and writes
   `crates/pyenv/conda/linux.tsv`: name, arch, verdict, package, URL, SHA-256, step
   (`anaconda` or `miniconda`), verify step, exported variables.
3. **Verdicts:**

   | Verdict | Rule |
   |---|---|
   | `ok` | The URL has a SHA-256 and starts with one of the five prefixes of §3. |
   | `ok-latest` | No SHA-256, the name is `miniconda2-latest`, `miniconda3-latest` or `miniforge3-latest` (the names D2 gives a hash source), and the URL starts with one of the five prefixes. |
   | `no-binary` | The definition took its own "not available for" branch. |
   | `unsupported` | Anything else, with its reason: `no published hash` (an unhashed name without a hash source: `mambaforge`, `mambaforge-pypy3`, `miniforge-pypy3`), `unknown host`, or the stub gap that stopped it. |

4. **Checks that fail the job:** the arch named in an `ok` row's file name matches the
   row's arch (`Linux-aarch64` / `Linux-x86_64`, as M7a's file check); every vendored
   conda definition has a row per arch; a minimum number of `ok` rows (`MIN_OK_ROWS`,
   set in the plan just under Task 1's measured count, as M7a's `MIN_PAGE_HASHES`).
5. **In the weekly sync,** the step runs beside M7a's PyPy step under the same rule
   (M7a D9): a failure leaves `crates/pyenv/conda/` as it was, the pull request reports
   it, and the run fails; the CPython sync still opens its pull request.

### 4.2 How rpyenv uses it

- `crates/pyenv/src/install/conda.rs` parses the embedded table (debug override:
  `RPYENV_TEST_CONDA_TABLES`). Its trust rule refuses any `ok` or `ok-latest` row whose
  URL is outside the five prefixes, as `pypy::parse_linux` does.
- `commands/install.rs` routes a definition whose name starts with `anaconda`,
  `miniconda`, `miniforge` or `mambaforge` as it routes `pypy*`: a vendored one through
  the table, a user's own (a `PYTHON_BUILD_DEFINITIONS` folder or a path) that the
  definition reader can't read through `conda_eval.sh` with `bash`.
- The row becomes a one-line definition text the existing interpreter reads, as
  `pypy::definition_text` does.

### 4.3 The install

1. **Pre-flight:** no compiler check (a binary install), as for PyPy.
2. **Terms notice:** when the row exports `CONDA_PLUGINS_AUTO_ACCEPT_TOS`, rpyenv prints,
   before downloading:
   `rpyenv: Miniconda's defaults channel is covered by Anaconda's terms of service, which require a paid license for organizations of 200 or more people; this install accepts them, as upstream pyenv does. Miniforge (conda-forge) isn't covered.`
   The variable is then set for the installer and the `conda install` step.
3. **Hash for `ok-latest`:** read before downloading.
   - `miniconda*-latest`: `https://repo.anaconda.com/miniconda/`, the row whose file is
     the definition's file name, its SHA-256 column.
   - `miniforge3-latest`: the tag `https://github.com/conda-forge/miniforge/releases/latest`
     redirects to, then `…/releases/download/<tag>/Miniforge3-<tag>-Linux-<arch>.sh.sha256`
     (the first 64 hex characters of the file).
   - Neither source readable, no matching row, or not 64 hex characters: refused (§6).
     Debug override for both: `RPYENV_TEST_CONDA` (a local server), as
     `RPYENV_TEST_PYPY`.
4. **Download and check** through the existing fetcher, then the step.
5. **In place:** conda can't be moved after installing (parent §15.1, choice 4). A new
   `Txn` method sets any existing version aside as `.old-<name>` (as `place` does),
   creates the empty `versions/<name>` and writes the `.rpyenv-incomplete` marker into
   it, before the installer runs. Upstream's `-f` lets the installer use the existing
   folder.
6. **The steps** (builder):
   - `anaconda`: `bash <package>.sh -f -b -p <prefix>`, its output in the build log.
   - `miniconda`: the same, then `<prefix>/bin/conda install --yes pip conda=<v>`, `<v>`
     read from `<prefix>/bin/conda --version`.
7. **Verify** through the existing verify code: `verify_pyXY` checks `bin/pythonX.Y`,
   `verify_py3_latest` checks `bin/python3`.
8. **Commit:** the marker goes, and an old version's `envs/` is carried over, and nothing
   else (D7). A conda env's files name its own final path (`versions/<name>/envs/<env>`),
   which is the same after the carry-over. When the old base had packages the new one
   lacks, rpyenv prints, on stderr, before deleting the old copy:
   `rpyenv: the previous {name}'s base packages weren't carried over (only envs/ was); to reinstall them: conda install {conda names}; pip install {pip names}`
   (sorted names; a part with no names is left out, with its `; `). Conda names: records in
   the old `conda-meta/` (`<name>-<version>-<build>.json`) with no record of that name in
   the new one. Pip names: old `site-packages/*.dist-info` whose `INSTALLER` file reads
   `pip`, with no `.dist-info` of that project in the new base.
9. **Failure or Ctrl+C at any step:** the transaction's rollback deletes the
   half-installed folder and renames `.old-<name>` back. A folder that still has the
   marker is already not a version to `versions`, shims or `install`'s "already
   installed" test.

## 5. Shims (closes D-44)

- `rpyenv-core`'s `shimset.rs`, Linux flavor: when any `versions/*/bin/conda` or
  `versions/*/envs/*/bin/conda` exists, every name on the embedded `default.list` loses
  its shim, whichever version provides it. Names are compared whole, after upstream's own
  parsing (`#` to end of line removed, blank lines skipped).
- Being in `shimset.rs`, the rule covers `pyenv rehash`, the rehash after `pip`, and the
  M5b live watcher alike.
- `which`, `whence` and `versions --executables` are unchanged, as upstream: with conda
  selected, `pyenv which curl` still finds conda's copy.
- Upstream's quirk is kept: one conda install hides a listed name's shim for every
  version.

## 6. Errors

| Case | Outcome |
|---|---|
| Not x86_64 or aarch64 | upstream's `ERROR: The binary distribution of <X> is not available for <arch>.` lines |
| `unsupported` row (vendored) | `python-build: <name>: rpyenv cannot install this conda definition (<reason>)` |
| A user's definition `bash` can't evaluate | `python-build: <name>: rpyenv cannot read this conda definition (<reason>)` |
| `-latest` hash unreadable | `python-build: <name>: the publisher's SHA-256 for <file> couldn't be read from <source>, so rpyenv won't install it` |
| Hash mismatch | the fetcher's existing checksum failure; rollback |
| Installer or `conda install` exits non-zero, or Ctrl+C | python-build's failure block (log tail), rollback |
| Verify fails | upstream's verify error, rollback |

## 7. Parity and allowlist

- **D-44** is removed (its test inverted: `curl` and `clear` lose their shims once
  `bin/conda` exists).
- **New rows,** each cited by a test: the terms notice (an extra stdout line); the
  `unsupported` refusals (upstream would try the download and fail on a 404 or install
  unchecked); `-latest` checked against the publisher (upstream installs it unchecked);
  the user-definition refusal.
- **Differential cases:** `latest -k anaconda3`, `latest -k miniconda3`,
  `latest -k miniforge3` (Linux); `install --list` goldens gain the conda lines.

## 8. Testing

- **`ci/` (Python, `ci/test_conda_tables.py`):** evaluator cases (plain, `source` alias,
  exported variable, unhashed `-latest`, unhashed without a source, unknown host, a
  `no-binary` arch); the table checks of §4.1.4.
- **Rust unit tests:** table parsing and the trust rule; reading the hash from a saved
  Miniconda page and a saved `.sha256` (fixtures in `crates/pyenv/tests/fixtures/conda/`);
  the `default.list` parser.
- **Rust integration tests** (`tests/cli_install_conda.rs`, fake installer in
  `tests/common/fakeconda.rs`, served locally; it writes `bin/python3.X`, `bin/conda` and
  `conda-meta/`, and records its arguments and environment): install in place, the marker
  present while the installer runs, rollback on each §6 failure, reinstall carrying over
  `envs/`, the terms notice and variable, both `-latest` sources through
  `RPYENV_TEST_CONDA`, refusals; `tests/cli_rehash.rs`: the listed names lose their shims
  once `bin/conda` exists, and keep them when none does.
- **Real installs in CI** (`install-linux.yml`, a `conda` job shaped like M7a's `pypy`
  job; Anaconda's ~1 GB installer is left to the fake, its step being Miniconda's minus
  `conda install`):

  | Install | Runner | Proves |
  |---|---|---|
  | a numbered `miniforge3` | ubuntu-latest, ubuntu-24.04-arm | the `miniconda` step and verify on both arches, conda-forge |
  | a numbered `miniconda3` 25.x | ubuntu-latest | the terms variable and notice, `conda install pip` against `defaults` |
  | `miniforge3-latest` | ubuntu-latest | the publisher-hash path |

  After each: `python -c` through the shim; no `curl` shim; `pyenv virtualenv` on the
  conda base creates a conda env.

## 9. Parent-spec and docs updates (with this milestone)

- Parent §15.1, resolution 3: Linux uses upstream's list only (D4); the ownership rule
  and its list belong to M7c. Choice 4 (in place, marker) is implemented as §4.3.
- Parent §14: M7b is Linux conda, M7c Windows conda.
- README: a conda section: the families and arches, the terms notice, the `-latest`
  caveat (D6), and the advice to pick a numbered version for a reviewed hash.

## 10. Risks

| Risk | Effect | Mitigation |
|---|---|---|
| A publisher renames its `-latest` hash source | `-latest` installs refused | the CI `miniforge3-latest` install fails first; numbered names unaffected |
| Upstream adds a conda definition shape the stubs lack | that definition `unsupported` | the sync PR shows the table diff; the floor check catches a mass failure |
| Anaconda's terms change | the notice's wording goes stale | the notice names the terms, not their details beyond the 200-person threshold the parent spec cites |
| `conda install pip` needs the network after the download | an offline install fails at that step | as upstream |
