# M7b: Anaconda, Miniconda and Miniforge on Linux — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On Linux (x86_64, aarch64), `pyenv install` installs Anaconda, Miniconda and Miniforge from their official installers under upstream's names, each download checked against a published SHA-256, and conda's bundled system tools get no shims (allowlist D-44 closes).

**Architecture:** M7a's pattern. The weekly sync vendors upstream's conda definitions, and CI evaluates them with stub python-build functions in a no-network container into `crates/pyenv/conda/linux.tsv`, which rpyenv embeds. `pyenv install` turns a table row into a one-line `install_script` definition that the existing builder runs, with two new steps (`anaconda`, `miniconda`) that install in place (conda can't be moved). Upstream's `conda.d/default.list` filters the shim set in `rpyenv-core`.

**Tech Stack:** Rust 1.96 (workspace crates `pyenv`, `rpyenv-core`), Python 3 (`ci/`), bash (the evaluator), GitHub Actions, docker (WSL and CI).

**Spec:** `docs/superpowers/specs/2026-10-15-m7b-conda-linux-design.md` (approved 2026-10-15). Pattern reference: `docs/superpowers/specs/2026-10-14-m7a-pypy-design.md`.

## Global Constraints

- Linux only. Every new Rust integration test file starts with `#![cfg(unix)]`; Windows behavior must not change.
- Arches: `x86_64` → `Linux-x86_64`, `aarch64` → `Linux-aarch64` (upstream's `anaconda_architecture`). No other arch gets a table row.
- Conda family = a definition name starting with `anaconda`, `miniconda`, `miniforge` or `mambaforge`.
- The five trusted URL prefixes, verbatim: `https://repo.anaconda.com/miniconda/`, `https://repo.anaconda.com/archive/`, `https://repo.continuum.io/miniconda/`, `https://repo.continuum.io/archive/`, `https://github.com/conda-forge/miniforge/`.
- The `-latest` names with a publisher hash source, verbatim: `miniconda-latest`, `miniconda2-latest`, `miniconda3-latest`, `miniforge3-latest` (`miniconda-latest` is upstream's `source` alias of `miniconda2-latest`; spec D2's `miniconda*-latest` covers it, though spec §4.1.3 lists three names: Task 8 amends §4.1.3).
- Messages, verbatim (`{…}` are substitutions):
  - Terms notice: `rpyenv: Miniconda's defaults channel is covered by Anaconda's terms of service, which require a paid license for organizations of 200 or more people; this install accepts them, as upstream pyenv does. Miniforge (conda-forge) isn't covered.`
  - Vendored unsupported: `python-build: {name}: rpyenv cannot install this conda definition ({reason})`
  - User definition bash can't evaluate: `python-build: {name}: rpyenv cannot read this conda definition ({reason})`
  - `-latest` hash unreadable: `python-build: {name}: the publisher's SHA-256 for {file} couldn't be read from {source}, so rpyenv won't install it`
  - No binary: three stderr lines, as upstream's definition prints them: an empty line, `ERROR: The binary distribution of {what} is not available for {arch}.`, an empty line.
- rpyenv is never the hash authority: every download is checked against a SHA-256 from upstream's definition or, for the three `-latest` names, from the publisher at install time. Nothing installs unchecked.
- Debug-only test overrides (ignored by release builds): `RPYENV_TEST_CONDA_TABLES` (a folder holding `linux.tsv`), `RPYENV_TEST_CONDA` (the fake publisher base URL). With `RPYENV_TEST_PYTHON_ORG` set and `RPYENV_TEST_CONDA` unset, the publisher sources point at the closed port `http://127.0.0.1:9`.
- Repo conventions: one-line commit messages via `-m`, or `-F C:\tmp\cm_<random>.txt`; **no Co-Authored-By, Claude-Session or any attribution line**; never `git push`. Python: `python` on Windows, `python3` in WSL, `encoding='utf-8'` on every `open()`.
- Never run rpyenv or upstream pyenv without `PYENV_ROOT` (and `HOME`) pointing at scratch. Tests use the `Fixture` helper, which does this.
- Linux-only tests run in WSL: `git add` new files first, then `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m7a_wsl.sh test -p pyenv --test <name>` (the script snapshots tracked plus staged files into `~/rpyenv-linux`; its full log is `~/m7a_linux.log`). `… m7a_wsl.sh all` runs fmt, clippy and the whole workspace. `… m7a_wsl.sh py ci/<test file>` runs a ci test. Never put `$VARS` inside `wsl … bash -c "…"`; use `--exec` with a script file.
- On Windows, before each commit: `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`.

## Review Focus

1. **An install killed with SIGKILL mid-installer** leaves `versions/<name>` with `.rpyenv-incomplete`. The next `pyenv install <name>` (without `-f`) must install, not say "already installed", and leave no marker. Test in Task 4 (`a_folder_left_with_the_marker_is_reinstalled_without_f`).
2. **`pyenv install -f` over a conda version with an env** keeps `envs/<env>` (users' conda environments are expensive to recreate). Test in Task 4 (`force_reinstall_keeps_conda_envs`).
3. **A user's own conda definition with no hash** is refused with the "cannot read" message and nothing is downloaded. Test in Task 5 (`a_user_definition_without_a_hash_is_refused`).
4. **Conda present only inside an env** (`versions/x/envs/y/bin/conda`), or in a dot folder (upstream's glob uses `dotglob`), still triggers the shim list. Tests in Task 6 (`conda_in_an_env_or_a_dot_folder_triggers_the_list`).
5. **The Miniconda listing lacks this arch's `-latest` row**: refused before any installer download, naming the listing as the source. Test in Task 5 (`latest_without_a_listing_row_is_refused_before_downloading`).

---

### Task 1: The evaluator and the table builder

**Files:**
- Create: `crates/pyenv/src/install/conda_eval.sh`
- Create: `ci/conda_tables.py`
- Create: `ci/test_conda_tables.py`

**Interfaces:**
- Consumes: nothing new (pattern: `crates/pyenv/src/install/pypy_eval.sh`, `ci/pypy_tables.py`).
- Produces:
  - `conda_eval.sh <definition file> <arch>` prints exactly one line: `ok <package> <url>[#<sha256>] <step> <verify> [<NAME>=<value>...]`, `no-binary <what>`, or `unsupported <reason>`. `<step>` is `anaconda` or `miniconda`; `<verify>` starts with `verify_py`.
  - `ci/conda_tables.py`: `conda_names(share) -> list[str]`, `evaluate(share, names, engine) -> dict[(name, arch), str]`, `classify(name, verdict) -> str`, `linux_rows(names, verdicts) -> list[(name, arch, verdict)]`, `render(rows) -> str`, `check(rows, minimum=MIN_OK_ROWS) -> list[str]`, `main(argv) -> int`. Constants `ARCHES = ("Linux-x86_64", "Linux-aarch64")`, `PREFIXES` (the five), `LATEST` (the four names), `MIN_OK_ROWS = 0` (Task 2 sets it).
  - Table line format (`linux.tsv`): `<name>\t<arch>\t<verdict>`, where a verdict is `ok …` (URL with `#sha256`), `ok-latest …` (URL without), `no-binary <what>` or `unsupported <reason>`.

- [ ] **Step 1: Write the evaluator tests.** `ci/test_conda_tables.py` runs `conda_eval.sh` with bash on small definitions in a temp dir (engine `local`, as `ci/test_pypy_tables.py` does; read that file first and copy its helpers for writing definition files and skipping when bash is missing: `bash_works()`).

```python
"""Tests for ci/conda_tables.py and crates/pyenv/src/install/conda_eval.sh (M7b design §4.1)."""
import os
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import conda_tables as ct  # noqa: E402

SHA = "a" * 64
CASE = '''case "$(anaconda_architecture 2>/dev/null || true)" in
"Linux-x86_64" )
  install_script "Miniforge3-24.1.2-0-Linux-x86_64" "https://github.com/conda-forge/miniforge/releases/download/24.1.2-0/Miniforge3-24.1.2-0-Linux-x86_64.sh#{sha}" "miniconda" verify_py312
  ;;
* )
  {{ echo
    colorize 1 "ERROR"
    echo ": The binary distribution of Miniforge3 is not available for $(anaconda_architecture 2>/dev/null || true)."
    echo
  }} >&2
  exit 1
  ;;
esac
'''


def bash_works():
    try:
        return subprocess.run(["bash", "-c", "true"], capture_output=True).returncode == 0 and os.name != "nt"
    except OSError:
        return False


@unittest.skipUnless(bash_works(), "needs bash (run in WSL)")
class Evaluator(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()

    def write(self, name, text):
        with open(os.path.join(self.dir, name), "w", encoding="utf-8", newline="\n") as f:
            f.write(text)

    def eval(self, name, arch):
        out = subprocess.run(["bash", ct.EVAL, os.path.join(self.dir, name), arch],
                             capture_output=True, text=True, check=True).stdout
        return out.strip()

    def test_hashed_definition(self):
        self.write("miniforge3-24.1.2-0", CASE.format(sha=SHA))
        self.assertEqual(self.eval("miniforge3-24.1.2-0", "Linux-x86_64"),
                         "ok Miniforge3-24.1.2-0-Linux-x86_64 https://github.com/conda-forge/miniforge/releases/download/24.1.2-0/"
                         f"Miniforge3-24.1.2-0-Linux-x86_64.sh#{SHA} miniconda verify_py312")

    def test_no_binary_names_what(self):
        self.write("miniforge3-24.1.2-0", CASE.format(sha=SHA))
        self.assertEqual(self.eval("miniforge3-24.1.2-0", "Linux-aarch64"), "no-binary Miniforge3")

    def test_source_alias(self):
        self.write("miniforge3-24.1.2-0", CASE.format(sha=SHA))
        self.write("miniforge-24.1.2-0", 'source "${BASH_SOURCE%/*}/miniforge3-24.1.2-0"\n')
        self.assertTrue(self.eval("miniforge-24.1.2-0", "Linux-x86_64").startswith("ok Miniforge3-24.1.2-0-Linux-x86_64 "))

    def test_exported_variable_is_recorded(self):
        self.write("miniconda3-25.1.1-0", "export CONDA_PLUGINS_AUTO_ACCEPT_TOS=true\n" + CASE.format(sha=SHA))
        self.assertTrue(self.eval("miniconda3-25.1.1-0", "Linux-x86_64").endswith(" verify_py312 CONDA_PLUGINS_AUTO_ACCEPT_TOS=true"))

    def test_unhashed_url_is_reported_as_ok_without_hash(self):
        self.write("miniconda3-latest", CASE.format(sha=SHA).replace(f"#{SHA}", ""))
        v = self.eval("miniconda3-latest", "Linux-x86_64")
        self.assertTrue(v.startswith("ok ") and "#" not in v, v)

    def test_unknown_function_is_unsupported(self):
        self.write("anaconda3-x", "install_something_else foo\n")
        self.assertEqual(self.eval("anaconda3-x", "Linux-x86_64"), "unsupported calls install_something_else")

    def test_other_step_is_unsupported(self):
        self.write("anaconda3-y", CASE.format(sha=SHA).replace('"miniconda"', '"pypy"'))
        self.assertEqual(self.eval("anaconda3-y", "Linux-x86_64"), "unsupported step pypy")


class Classify(unittest.TestCase):
    URL = "https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-x86_64.sh"

    def test_hashed_known_host_stays_ok(self):
        v = f"ok P {self.URL}#{SHA} miniconda verify_py3_latest"
        self.assertEqual(ct.classify("miniconda3-x", v), v)

    def test_unhashed_latest_name_is_ok_latest(self):
        self.assertEqual(ct.classify("miniconda3-latest", f"ok P {self.URL} miniconda verify_py3_latest"),
                         f"ok-latest P {self.URL} miniconda verify_py3_latest")

    def test_unhashed_other_name_has_no_published_hash(self):
        self.assertEqual(ct.classify("mambaforge", f"ok P {self.URL} miniconda verify_py3_latest"),
                         "unsupported no published hash")

    def test_unknown_host(self):
        self.assertEqual(ct.classify("anaconda3-z", f"ok P https://example.com/a.sh#{SHA} anaconda verify_py312"),
                         "unsupported unknown host")

    def test_bad_exported_value(self):
        self.assertEqual(ct.classify("miniconda3-x", f"ok P {self.URL}#{SHA} miniconda verify_py312 A=b;c"),
                         "unsupported exported value rpyenv can't carry: A=b;c")

    def test_other_verdicts_pass_through(self):
        self.assertEqual(ct.classify("x", "no-binary Anaconda"), "no-binary Anaconda")


class Check(unittest.TestCase):
    def ok(self, arch, file):
        return f"ok P https://repo.anaconda.com/archive/{file}#{SHA} anaconda verify_py312"

    def test_arch_must_match_the_file(self):
        rows = [("a", "Linux-x86_64", self.ok("Linux-x86_64", "Anaconda3-1-Linux-aarch64.sh")),
                ("a", "Linux-aarch64", self.ok("Linux-aarch64", "Anaconda3-1-Linux-x86_64.sh"))]
        errors = ct.check(rows, minimum=0)
        self.assertEqual(len(errors), 2, errors)

    def test_minimum_ok_rows(self):
        rows = [("a", "Linux-x86_64", self.ok("Linux-x86_64", "Anaconda3-1-Linux-x86_64.sh"))]
        self.assertEqual(ct.check(rows, minimum=1), [])
        self.assertTrue(ct.check(rows, minimum=2)[0].startswith("1 ok rows, fewer than 2"))


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run them to see them fail.** Windows: `python -m unittest ci/test_conda_tables.py` → `ModuleNotFoundError: conda_tables`.

- [ ] **Step 3: Write `crates/pyenv/src/install/conda_eval.sh`.**

```bash
#!/usr/bin/env bash
# Evaluates one python-build conda definition (Anaconda, Miniconda, Miniforge) for one
# architecture without installing anything (M7b design §4.1): python-build's functions are
# stubs that record what the definition would install. ci/conda_tables.py runs it over the
# vendored definitions in a container with no network; rpyenv runs it
# (`bash -c "$script" conda_eval <file> <arch>`) for a user's own conda definition.
#
#   conda_eval.sh <definition file> <anaconda architecture>
#
# Prints one line: `ok <package> <url>[#<sha256>] <step> <verify> [<NAME>=<value>...]` (the
# variables the definition exported), `no-binary <what>` (the definition's own "binary
# distribution of <what> is not available" branch), or `unsupported <reason>`.
if [ $# -ne 2 ]; then
  echo "usage: conda_eval.sh <definition file> <arch>" >&2
  exit 2
fi
out=$(
  (
    anaconda_architecture() { echo "$CONDA_EVAL_ARCH"; }
    install_script() {
      local line="PKG" a e
      for a in "$@"; do line="$line $a"; done
      for e in $(compgen -e); do
        case " $CONDA_EVAL_BEFORE " in
          *" $e "*) ;;
          *) line="$line $e=${!e}" ;;
        esac
      done
      echo "$line" >&3
    }
    colorize() { printf '%s' "$2"; }
    command_not_found_handle() { echo "MISSING $1" >&3; return 127; }
    CONDA_EVAL_ARCH=$2
    CONDA_EVAL_BEFORE=$(compgen -e | tr '\n' ' ')
    # shellcheck disable=SC1090
    source "$1" 2>&3
  ) 3>&1 >/dev/null 2>&1
)
rc=$?
missing=$(printf '%s\n' "$out" | sed -n 's/^MISSING //p' | head -n 1)
pkgs=$(printf '%s\n' "$out" | grep -c '^PKG ')
what=$(printf '%s\n' "$out" | sed -n 's/^.*The binary distribution of \(.*\) is not available for .*$/\1/p' | head -n 1)
if [ -n "$missing" ]; then
  echo "unsupported calls $missing"
elif [ "$pkgs" -eq 1 ] && [ "$rc" -eq 0 ]; then
  set -f
  # shellcheck disable=SC2046
  set -- $(printf '%s\n' "$out" | sed -n 's/^PKG //p')
  if [ $# -lt 4 ]; then
    echo "unsupported install_script with $# arguments"
    exit 0
  fi
  pkg=$1 url=$2 step=$3 verify=$4
  shift 4
  hash=${url##*#}
  case "$step" in anaconda|miniconda) ;; *) echo "unsupported step $step"; exit 0 ;; esac
  case "$verify" in verify_py*) ;; *) echo "unsupported verify step $verify"; exit 0 ;; esac
  if [ "$hash" != "$url" ] && { [ ${#hash} -ne 64 ] || [ -n "${hash//[0-9a-f]/}" ]; }; then
    echo "unsupported malformed sha256 in $url"
  else
    echo "ok $pkg $url $step $verify${*:+ $*}"
  fi
elif [ "$pkgs" -eq 0 ] && [ "$rc" -ne 0 ] && [ -n "$what" ]; then
  echo "no-binary $what"
else
  echo "unsupported $pkgs packages, exit $rc"
fi
```

- [ ] **Step 4: Write `ci/conda_tables.py`.**

```python
"""Builds the conda table rpyenv embeds (M7b design §4.1).

  python3 ci/conda_tables.py --share <python-build share dir> [--engine docker|podman|local] [--write]

--share is the vendored crates/pyenv/python-build/share. The upstream definitions are run by
crates/pyenv/src/install/conda_eval.sh with stub python-build functions, in a container with
no network and a read-only file system (`local` runs bash directly: tests only). Any failure
exits 1 before anything is written. Without --write it reports whether the table would change.
"""
import argparse
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEST = os.path.join(REPO, "crates", "pyenv", "conda")
EVAL = os.path.join(REPO, "crates", "pyenv", "src", "install", "conda_eval.sh")
ARCHES = ("Linux-x86_64", "Linux-aarch64")
FAMILIES = ("anaconda", "miniconda", "miniforge", "mambaforge")
PREFIXES = ("https://repo.anaconda.com/miniconda/", "https://repo.anaconda.com/archive/",
            "https://repo.continuum.io/miniconda/", "https://repo.continuum.io/archive/",
            "https://github.com/conda-forge/miniforge/")
# The -latest names whose publisher publishes a hash rpyenv reads at install time (design D2).
LATEST = ("miniconda-latest", "miniconda2-latest", "miniconda3-latest", "miniforge3-latest")
# Fewer `ok` rows than this means the definitions or the stubs changed shape (Task 2 sets it
# just under the count measured on the pinned upstream).
MIN_OK_ROWS = 0
# debian:trixie-slim as pulled on 2026-10-14, the digest ci/pypy_tables.py pins.
IMAGE = "debian:trixie-slim@sha256:a29215f6a35e51e22adffa17f89e9d2ef06214e64a2bad10d765c46aea49f11f"
LOOP = ('for n in "$@"; do for a in Linux-x86_64 Linux-aarch64; do printf "%s\\t%s\\t" "$n" "$a"; '
        'bash "$EVAL" "$DEFS/$n" "$a"; done; done')
ENV_WORD = re.compile(r"[A-Z_][A-Z0-9_]*=[A-Za-z0-9_.:/-]*")


def conda_names(share):
    """Every conda-family definition file in `share`, sorted (C order)."""
    return sorted(n for n in os.listdir(share)
                  if n.startswith(FAMILIES) and os.path.isfile(os.path.join(share, n)))


def evaluate(share, names, engine):
    """{(name, arch): verdict line} for each name and each of ARCHES."""
    if not names:
        return {}
    if engine == "local":
        cmd = ["bash", "-c", LOOP, "_", *names]
        env = dict(os.environ, EVAL=EVAL, DEFS=share)
    else:
        cmd = [engine, "run", "--rm", "--network", "none", "--read-only",
               "-v", f"{os.path.abspath(share)}:/defs:ro", "-v", f"{EVAL}:/eval.sh:ro",
               "-e", "EVAL=/eval.sh", "-e", "DEFS=/defs", IMAGE, "bash", "-c", LOOP, "_", *names]
        env = None
    out = subprocess.run(cmd, capture_output=True, text=True, check=True, env=env).stdout
    verdicts = {}
    for line in out.splitlines():
        name, arch, verdict = line.split("\t", 2)
        verdicts[(name, arch)] = verdict
    want = {(n, a) for n in names for a in ARCHES}
    if set(verdicts) != want:
        raise RuntimeError(f"the evaluator answered for {len(verdicts)} of {len(want)} definition/arch pairs")
    return verdicts


def classify(name, verdict):
    """The table verdict for an evaluator line (design §4.1.3)."""
    w = verdict.split()
    if w[0] != "ok":
        return verdict
    url = w[2]
    for e in w[5:]:
        if not ENV_WORD.fullmatch(e):
            return f"unsupported exported value rpyenv can't carry: {e}"
    if not url.startswith(PREFIXES):
        return "unsupported unknown host"
    if "#" in url:
        return verdict
    if name in LATEST:
        return "ok-latest " + " ".join(w[1:])
    return "unsupported no published hash"


def linux_rows(names, verdicts):
    return [(n, a, classify(n, verdicts[(n, a)])) for n in names for a in ARCHES]


def render(rows):
    head = ("# Generated by ci/conda_tables.py from upstream's conda definitions (M7b design §4.1).\n"
            "# <name>\\t<arch>\\t<verdict>\n")
    return head + "".join(f"{n}\t{a}\t{v}\n" for n, a, v in rows)


def check(rows, minimum=None):
    """Error lines: an `ok`/`ok-latest` row whose file names the other arch, or too few `ok` rows."""
    minimum = MIN_OK_ROWS if minimum is None else minimum
    errors, ok = [], 0
    for name, arch, verdict in rows:
        w = verdict.split()
        if w[0] not in ("ok", "ok-latest"):
            continue
        ok += w[0] == "ok"
        file = w[2].partition("#")[0].rsplit("/", 1)[1]
        want = arch.split("-", 1)[1]
        if want not in file:
            errors.append(f"{name} {arch}: the file {file} isn't a {want} installer")
    if ok < minimum:
        errors.append(f"{ok} ok rows, fewer than {minimum}: have the definitions or the stubs changed shape?")
    return errors


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("--share", required=True)
    p.add_argument("--engine", default="docker", choices=["docker", "podman", "local"])
    p.add_argument("--write", action="store_true")
    a = p.parse_args(argv)
    names = conda_names(a.share)
    rows = linux_rows(names, evaluate(a.share, names, a.engine))
    errors = check(rows)
    for e in errors:
        print(f"error: {e}", file=sys.stderr)
    if errors:
        return 1
    for n, ar, v in rows:
        if v.startswith("unsupported") and v != "unsupported no published hash":
            print(f"warning: {n} {ar}: {v}", file=sys.stderr)
    text = render(rows)
    path = os.path.join(DEST, "linux.tsv")
    old = open(path, encoding="utf-8").read() if os.path.exists(path) else None
    if old != text and a.write:
        os.makedirs(DEST, exist_ok=True)
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
    counts = {}
    for _, ar, v in rows:
        k = (ar, v.split()[0])
        counts[k] = counts.get(k, 0) + 1
    print("linux.tsv: " + "; ".join(f"{ar} {k} {c}" for (ar, k), c in sorted(counts.items()))
          + f"; changed: {'yes' if old != text else 'no'}")
    return 0 if a.write or old == text else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

- [ ] **Step 5: Run the tests.** Windows: `python -m unittest ci/test_conda_tables.py` (Classify and Check pass; Evaluator skips). WSL (after `git add`): `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m7a_wsl.sh py ci/test_conda_tables.py` → all pass, none skipped. Also check the script's mode: `git update-index --chmod=+x crates/pyenv/src/install/conda_eval.sh` (same as `pypy_eval.sh`; check with `git ls-files -s crates/pyenv/src/install/pypy_eval.sh`).

- [ ] **Step 6: Commit.** `git add crates/pyenv/src/install/conda_eval.sh ci/conda_tables.py ci/test_conda_tables.py` then `git commit -m "M7b: conda definition evaluator (stub python-build functions) and ci/conda_tables.py"`.

---

### Task 2: Vendoring, the table, and the weekly sync

**Files:**
- Modify: `ci/sync_python_build.py` (vendor conda-family definitions and `pyenv.d/rehash/conda.d/default.list`)
- Modify: `ci/test_sync_python_build.py`
- Modify: `ci/conda_tables.py` (`MIN_OK_ROWS`)
- Create (generated): 445 files `crates/pyenv/python-build/share/{anaconda,miniconda,miniforge,mambaforge}*`, `crates/pyenv/python-build/rehash/conda.d/default.list`, `crates/pyenv/conda/linux.tsv`
- Create: `C:\tmp\m7b_tables.sh` (scratch helper, not committed)
- Modify: `.github/workflows/sync-python-build.yml`
- Modify: `.github/workflows/install-linux.yml` (paths: add `crates/pyenv/conda/**`)

**Interfaces:**
- Consumes: Task 1's `ci/conda_tables.py` CLI.
- Produces: the vendored definitions; `crates/pyenv/python-build/rehash/conda.d/default.list` (byte-for-byte upstream; its license is the `LICENSE` vendored beside it); `crates/pyenv/conda/linux.tsv`. Ruling to record: the spec's §4.1.1 path `crates/pyenv/conda/default.list` with a header becomes the byte-for-byte path above, because `sync_python_build.py --write` replaces `crates/pyenv/python-build` wholesale and vendored files stay byte-identical; Task 8 amends the spec.

- [ ] **Step 1: Write the failing test** in `ci/test_sync_python_build.py`, following its existing fake-upstream helpers (read the file first): a fake upstream with `share/python-build/miniforge3-1`, `share/python-build/anaconda3-1`, `share/python-build/mambaforge`, `share/python-build/ruby-1` and `pyenv.d/rehash/conda.d/default.list`; `wanted()` must return `share/miniforge3-1`, `share/anaconda3-1`, `share/mambaforge` and `rehash/conda.d/default.list`, byte for byte, and not `share/ruby-1`. Add a second test: a symlinked `default.list` raises `Unsafe`.

- [ ] **Step 2: Run it to see it fail.** `python -m unittest ci/test_sync_python_build.py`.

- [ ] **Step 3: Implement.** In `wanted()`:

```python
FAMILIES = ("anaconda", "miniconda", "miniforge", "mambaforge")
...
    names = sorted(n for n in os.listdir(share)
                   if (n[:1].isdigit() or n.startswith("pypy") or n.startswith(FAMILIES))
                   and os.path.isfile(os.path.join(share, n)))
...
    # Upstream's conda rehash list (M7b design §5): rpyenv-core compiles it in.
    lst = os.path.join(upstream, "pyenv.d", "rehash", "conda.d", "default.list")
    refuse_links(lst, upstream)
    with open(lst, "rb") as f:
        out["rehash/conda.d/default.list"] = f.read()
```

Update the module docstring: it also copies the conda-family definitions (no patches) and `pyenv.d/rehash/conda.d/default.list`.

- [ ] **Step 4: Run the tests** (Windows and WSL `py`) → pass.

- [ ] **Step 5: Vendor and build the table in WSL.** Write `C:\tmp\m7b_tables.sh`:

```bash
#!/usr/bin/env bash
# M7b Task 2: vendors the pinned upstream's definitions and builds crates/pyenv/conda/linux.tsv in
# ~/rpyenv-linux (the working-tree snapshot), then copies the results back to the Windows checkout.
set -euo pipefail
src=/mnt/c/Users/JM/repos/OWN/rpyenv
bash /mnt/c/tmp/m7a_wsl.sh none >/dev/null || true
cd "$HOME/rpyenv-linux"
sha=$(cut -d ' ' -f 2 crates/pyenv/python-build/UPSTREAM)
work="$HOME/m7a-upstream"
[ -d "$work/pyenv-$sha" ] || curl -fsSL "https://github.com/pyenv/pyenv/archive/$sha.tar.gz" | tar -xz -C "$work"
python3 ci/sync_python_build.py --upstream "$work/pyenv-$sha" --commit "$sha" --write
echo "changed or added outside the conda families:"
git status --porcelain crates/pyenv/python-build | grep -vE '^\?\? crates/pyenv/python-build/(share/(anaconda|miniconda|miniforge|mambaforge)|rehash/)' || echo "  none"
python3 ci/conda_tables.py --share crates/pyenv/python-build/share --engine docker --write
cp crates/pyenv/python-build/share/anaconda* crates/pyenv/python-build/share/miniconda* \
   crates/pyenv/python-build/share/miniforge* crates/pyenv/python-build/share/mambaforge* \
   "$src/crates/pyenv/python-build/share/"
mkdir -p "$src/crates/pyenv/python-build/rehash/conda.d" "$src/crates/pyenv/conda"
cp crates/pyenv/python-build/rehash/conda.d/default.list "$src/crates/pyenv/python-build/rehash/conda.d/"
cp crates/pyenv/conda/linux.tsv "$src/crates/pyenv/conda/"
```

Run: `git add` the Task 2 changes so far, then `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m7b_tables.sh`. Expected: "none" outside the conda families; a counts line. Record the counts per arch and verdict in the report. Check by hand that `ok-latest` rows are exactly `miniconda-latest`, `miniconda2-latest`, `miniconda3-latest` and `miniforge3-latest` (for the arches each has), and list every `unsupported` row whose reason isn't `no published hash`.

- [ ] **Step 6: Set `MIN_OK_ROWS`** in `ci/conda_tables.py` to the measured total of `ok` rows rounded down to a multiple of 50, minus 50, with a comment naming the measured count and date (as `MIN_PAGE_HASHES` does). Re-run `python3 ci/conda_tables.py --share crates/pyenv/python-build/share --engine docker` in WSL (no `--write`) → exit 0, "changed: no".

- [ ] **Step 7: The weekly sync.** In `.github/workflows/sync-python-build.yml`, extend the `Rebuild the PyPy tables` step (keep its `id: pypy`, rename it `Rebuild the PyPy and conda tables`) so the brace group also runs `python3 ci/conda_tables.py --share crates/pyenv/python-build/share --engine docker --write`, and the failure branch also restores `crates/pyenv/conda` (`git checkout -- crates/pyenv/pypy crates/pyenv/conda` and `git clean -fdq -- crates/pyenv/pypy crates/pyenv/conda`). In `Anything to propose?`, the porcelain check covers `crates/pyenv/conda` too; `git add` in the PR step adds it. Update the header comment and the results line's wording (`PyPy and conda tables`), the PR title for table-only changes (`Update the PyPy and conda tables`), and the final fail step's message. Then add `- "crates/pyenv/conda/**"` to `install-linux.yml`'s `paths`. Re-run the stub test from M7a's sync change if it still exists (`C:\tmp\m7a_sync_extract.py`, then `C:\tmp\m7a_sync_test.sh` in WSL) after updating the step name it looks up; all checks must pass.

- [ ] **Step 8: Run the whole Rust suite in WSL** (`… m7a_wsl.sh all`): 445 new vendored files must not break existing tests (`crates/pyenv/tests/install_defs.rs` reads only CPython names, ruling R5b). Expected exception: none. Any `install --list` golden failure is the parity job's, fixed in Task 8; Rust tests must pass.

- [ ] **Step 9: Commit.** `git add ci/ .github/workflows/ crates/pyenv/python-build crates/pyenv/conda` then `git commit -m "M7b: vendor upstream's conda definitions and conda.d/default.list, build crates/pyenv/conda/linux.tsv, rebuild it in the weekly sync"`.

---

### Task 3: `conda.rs`: the table, the trust rule and the publisher hashes

**Files:**
- Create: `crates/pyenv/src/install/conda.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (`pub mod conda;`)
- Modify: `crates/pyenv/src/install/fetch.rs` (`Fetcher::redirect_target`)
- Modify: `crates/pyenv/tests/common/server.rs` (`Reply::Redirect`)
- Create: `crates/pyenv/tests/fixtures/conda/miniconda.html` (a trimmed copy of `https://repo.anaconda.com/miniconda/`: the `<table>` header and the rows for `Miniconda3-latest-Linux-x86_64.sh`, `Miniconda3-latest-Linux-aarch64.sh` and one numbered file, fetched once with curl; record the date in the report)
- Test: unit tests in `conda.rs`; `crates/pyenv/tests/fetch_redirect.rs` (`#![cfg(unix)]` not needed: the server runs on Windows too; follow how other fetch tests are written, e.g. grep for `Fetcher::direct()` in `crates/pyenv/tests`)

**Interfaces:**
- Consumes: Task 2's table and `conda_eval.sh`.
- Produces (all `pub` in `pyenv::install::conda`):

```rust
pub struct Install { pub package: String, pub url: String, pub step: String, pub verify: String, pub env: Vec<(String, String)> }
pub enum Verdict { Ok(Install), OkLatest(Install), NoBinary(String), Unsupported(String) }
pub const PREFIXES: [&str; 5];
pub const LATEST: [&str; 4];
pub const MINICONDA_LISTING: &str = "https://repo.anaconda.com/miniconda/";
pub const MINIFORGE: &str = "https://github.com/conda-forge/miniforge";
pub const EVAL_SCRIPT: &str = include_str!("conda_eval.sh");
pub fn is_conda_name(name: &str) -> bool;
pub fn parse_verdict(line: &str) -> Result<Verdict, String>;
pub fn parse_linux(text: &str) -> Result<Vec<LinuxRow>, String>; // trust rule on
pub struct LinuxRow { pub name: String, pub arch: String, pub verdict: Verdict }
pub fn linux_table() -> Result<Vec<LinuxRow>, String>;          // RPYENV_TEST_CONDA_TABLES turns the trust rule off
pub fn linux_verdict<'a>(rows: &'a [LinuxRow], name: &str, arch: &str) -> Option<&'a Verdict>;
pub fn host_arch() -> Option<&'static str>;                      // "Linux-x86_64" | "Linux-aarch64"
pub fn definition_text(i: &Install) -> String;                   // `export K=V` lines, then `install_script "<pkg>" "<url>" <step> <verify>`
pub fn listing_sha(html: &str, file: &str) -> Option<String>;
pub fn sha_file(text: &str) -> Option<String>;                   // first word, 64 lowercase hex
pub fn sources() -> (String, String);                            // (listing URL, miniforge base)
pub fn latest_source(name: &str, i: &Install, arch: &str, f: &Fetcher) -> Result<Install, String>; // url gets #sha
#[cfg(unix)] pub fn evaluate(def: &Path, arch: &str) -> Result<Verdict, String>;
```
- `Fetcher::redirect_target(&self, url: &str) -> Result<String, TextError>`: one GET with redirects off; the `Location` header of a 3xx answer, else an error.
- `common::server::Reply::Redirect(String)`: answers `302 Found` with that `Location`.

- [ ] **Step 1: Write the unit tests** at the bottom of `conda.rs` (module `tests`):

```rust
const SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const URL: &str = "https://repo.anaconda.com/miniconda/Miniconda3-py312_25.1.1-0-Linux-x86_64.sh";

#[test]
fn verdict_lines() {
    let ok = format!("ok Miniconda3-x {URL}#{SHA} miniconda verify_py312 CONDA_PLUGINS_AUTO_ACCEPT_TOS=true");
    let Ok(Verdict::Ok(i)) = parse_verdict(&ok) else { panic!() };
    assert_eq!((i.step.as_str(), i.verify.as_str()), ("miniconda", "verify_py312"));
    assert_eq!(i.env, vec![("CONDA_PLUGINS_AUTO_ACCEPT_TOS".to_string(), "true".to_string())]);
    assert!(matches!(parse_verdict(&format!("ok-latest P {URL} miniconda verify_py3_latest")), Ok(Verdict::OkLatest(_))));
    assert_eq!(parse_verdict("no-binary Anaconda"), Ok(Verdict::NoBinary("Anaconda".into())));
    assert_eq!(parse_verdict("unsupported no published hash"), Ok(Verdict::Unsupported("no published hash".into())));
    // `ok` needs a hash, `ok-latest` must not have one; steps and verify are checked.
    assert!(parse_verdict(&format!("ok P {URL} miniconda verify_py312")).is_err());
    assert!(parse_verdict(&format!("ok-latest P {URL}#{SHA} miniconda verify_py312")).is_err());
    assert!(parse_verdict(&format!("ok P {URL}#{SHA} pypy verify_py312")).is_err());
    assert!(parse_verdict(&format!("ok P {URL}#{SHA} miniconda ensurepip")).is_err());
    assert!(parse_verdict(&format!("ok ../P {URL}#{SHA} miniconda verify_py312")).is_err());
}

#[test]
fn the_table_refuses_a_row_outside_the_five_prefixes() {
    let good = format!("miniconda3-x\tLinux-x86_64\tok P {URL}#{SHA} miniconda verify_py312\n");
    assert_eq!(parse_linux(&good).unwrap().len(), 1);
    let bad = good.replace("https://repo.anaconda.com/miniconda/", "https://example.com/");
    assert!(parse_linux(&bad).unwrap_err().contains("outside"));
}

#[test]
fn definition_text_exports_then_installs() {
    let i = Install { package: "P".into(), url: format!("{URL}#{SHA}"), step: "miniconda".into(), verify: "verify_py312".into(),
                      env: vec![("CONDA_PLUGINS_AUTO_ACCEPT_TOS".into(), "true".into())] };
    assert_eq!(definition_text(&i),
               format!("export CONDA_PLUGINS_AUTO_ACCEPT_TOS=true\ninstall_script \"P\" \"{URL}#{SHA}\" miniconda verify_py312\n"));
}

#[test]
fn the_listing_page_gives_the_hash_of_one_file() {
    let html = include_str!("../../tests/fixtures/conda/miniconda.html");
    let x = listing_sha(html, "Miniconda3-latest-Linux-x86_64.sh").expect("row");
    let a = listing_sha(html, "Miniconda3-latest-Linux-aarch64.sh").expect("row");
    assert_eq!(x.len(), 64);
    assert_ne!(x, a);
    assert_eq!(listing_sha(html, "Miniconda3-latest-Linux-s390x.sh"), None);
}

#[test]
fn a_sha256_file_is_its_first_word() {
    assert_eq!(sha_file(&format!("{SHA}  Miniforge3-26.7.2-0-Linux-x86_64.sh\n")), Some(SHA.to_string()));
    assert_eq!(sha_file("Not Found"), None);
}

#[test]
fn conda_names() {
    for n in ["anaconda3-2024.10-1", "miniconda3-latest", "miniforge3-24.1.2-0", "mambaforge", "anaconda-2.4.0"] {
        assert!(is_conda_name(n), "{n}");
    }
    for n in ["3.12.1", "pypy3.11-7.3.20", "ana"] {
        assert!(!is_conda_name(n), "{n}");
    }
}
```

The `listing_sha` test needs the fixture: fetch it in Step 2.

- [ ] **Step 2: Save the fixture.** `curl.exe -sL https://repo.anaconda.com/miniconda/ -o C:\tmp\miniconda_full.html`, then keep only the `<table>` opening, its header row, and the three rows named above (a Python script in `C:\tmp`, `encoding='utf-8'`), written to `crates/pyenv/tests/fixtures/conda/miniconda.html`.

- [ ] **Step 3: Write the redirect test** (`crates/pyenv/tests/fetch_redirect.rs`):

```rust
mod common;
use common::server::{start, Reply};

#[test]
fn redirect_target_reads_the_location_without_following_it() {
    let s = start(vec![("/releases/latest", vec![Reply::Redirect("https://example.invalid/releases/tag/26.7.2-0".into())])]);
    let f = pyenv::install::fetch::Fetcher::direct();
    assert_eq!(f.redirect_target(&s.url("/releases/latest")).unwrap(), "https://example.invalid/releases/tag/26.7.2-0");
}

#[test]
fn redirect_target_of_a_plain_answer_is_an_error() {
    let s = start(vec![("/x", vec![Reply::Body(b"hi".to_vec())])]);
    assert!(pyenv::install::fetch::Fetcher::direct().redirect_target(&s.url("/x")).is_err());
}
```

- [ ] **Step 4: Run to see the tests fail** (`cargo test -p pyenv --lib conda` and `--test fetch_redirect`): compile errors.

- [ ] **Step 5: Implement.**
  - `Reply::Redirect(String)` in `common/server.rs`: in the reply writer, `HTTP/1.1 302 Found\r\nLocation: {loc}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n` (match the formatting of the existing `Status` arm).
  - `Fetcher::redirect_target`: `self.agent.get(url).config().max_redirects(0).timeout_global(Some(self.text_timeout)).build().call()`; on `Ok(r)` with `r.status().is_redirection()`, return the `location` header as a `String`; any other outcome is a `TextError` (`HTTP <code>` or the ureq error), no retries (one small request; the caller reports the failure).
  - `conda.rs`, following `pypy.rs` (same module-doc style, `is_sha256`, `table_text` with `RPYENV_TEST_CONDA_TABLES`, `linux_table` turning the trust rule off under the override, `host_arch`, `evaluate` with `bash -c EVAL_SCRIPT conda_eval <file> <arch>` whose `ok` without `#` maps to `Unsupported(format!("no sha256 in {url}"))` because a user's definition has no publisher source). `sources()`: debug builds read `RPYENV_TEST_CONDA` (`(format!("{b}/miniconda/"), format!("{b}/miniforge"))`), else with `RPYENV_TEST_PYTHON_ORG` set use base `http://127.0.0.1:9`; release builds return `(MINICONDA_LISTING, MINIFORGE)`.
  - `listing_sha`: tags replaced by spaces (as `pypy::page_hashes`), split on whitespace; for each position where the word equals `file`, the first 64-hex word among the next 8 words. Two different hashes for the file → `None`.
  - `latest_source(name, i, arch, f)`:

```rust
/// The publisher's own SHA-256 for a `-latest` installer (design §4.3 step 3, D2, D6): it
/// catches a broken download, not a compromised publisher.
pub fn latest_source(name: &str, i: &Install, arch: &str, f: &Fetcher) -> Result<Install, String> {
    let (listing, miniforge) = sources();
    let refuse = |file: &str, source: &str| {
        format!("python-build: {name}: the publisher's SHA-256 for {file} couldn't be read from {source}, so rpyenv won't install it")
    };
    let mut out = i.clone();
    if name.starts_with("miniconda") {
        let file = i.url.rsplit('/').next().unwrap_or_default().to_string();
        let html = f.get_text(&listing).map_err(|_| refuse(&file, &listing))?;
        let sha = listing_sha(&html, &file).ok_or_else(|| refuse(&file, &listing))?;
        // A test table points the installer at the fake server; the real URL stays as it is.
        out.url = format!("{}#{sha}", i.url);
        return Ok(out);
    }
    // miniforge3-latest: the release `latest` redirects to, and its versioned installer, whose
    // `.sha256` file conda-forge publishes (the unversioned asset has none).
    let latest = format!("{miniforge}/releases/latest");
    let tag = f
        .redirect_target(&latest)
        .ok()
        .and_then(|loc| loc.rsplit_once("/releases/tag/").map(|(_, t)| t.to_string()))
        .filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
        .ok_or_else(|| refuse(&format!("Miniforge3-{arch}.sh"), &latest))?;
    let file = format!("Miniforge3-{tag}-{arch}.sh");
    let base = format!("{miniforge}/releases/download/{tag}");
    let sums = format!("{base}/{file}.sha256");
    let sha = f.get_text(&sums).ok().and_then(|t| sha_file(&t)).ok_or_else(|| refuse(&file, &sums))?;
    out.url = format!("{base}/{file}#{sha}");
    Ok(out)
}
```

  `Install` derives `Debug, Clone, PartialEq, Eq`; `Verdict` the same.

- [ ] **Step 6: Run** `cargo test -p pyenv --lib conda`, `cargo test -p pyenv --test fetch_redirect` (Windows), and the same in WSL → pass. `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 7: Commit.** `git commit -m "M7b: install::conda (table, trust rule, publisher hashes for -latest) and Fetcher::redirect_target"`.

---

### Task 4: Installing a conda definition: `install_script`, the two steps, in place

**Files:**
- Modify: `crates/pyenv/src/install/defs.rs` (`install_script` → `Fetch::Script`)
- Modify: `crates/pyenv/src/install/builder.rs` (fetch a script; steps `anaconda`, `miniconda`)
- Modify: `crates/pyenv/src/install/txn.rs` (`Txn::place_empty`)
- Create: `crates/pyenv/tests/common/fakeconda.rs`; Modify: `crates/pyenv/tests/common/mod.rs` (`pub mod fakeconda;`)
- Create: `crates/pyenv/tests/cli_install_conda.rs`

**Interfaces:**
- Consumes: nothing from Task 3 (this task installs from a one-line user definition file; Task 5 routes conda names here).
- Produces:
  - `defs::Fetch::Script { url: String, fragment: Option<String> }`: `install_script "<pkg>" "<url>[#sha]" <steps…>`, steps as for `install_package`.
  - `Txn::place_empty(&mut self) -> std::io::Result<()>`: sets any existing version aside as `.old-<name>` and leaves an empty `versions/<name>` holding `.rpyenv-incomplete` (`placed()` is then true; `commit` and rollback work as for `place`).
  - Builder steps `anaconda` (`bash <pkg>.sh -f -b -p <prefix>`) and `miniconda` (the same, then `<prefix>/bin/conda install --yes pip conda=<v>`), each with the definition's variables (`job.definition.vars`) in the child's environment.
  - `common::fakeconda::installer(py: &str) -> String`: the text of a fake `.sh` installer for Python `py` (e.g. `"3.12"`).

- [ ] **Step 1: Write the fake installer** (`tests/common/fakeconda.rs`):

```rust
//! A fake conda installer for tier-1 tests (M7b): run as `bash <it> -f -b -p <prefix>`, it
//! records its arguments, whether `<prefix>/.rpyenv-incomplete` exists, and
//! CONDA_PLUGINS_AUTO_ACCEPT_TOS into `$FAKECONDA_LOG` (when set), then writes `bin/python<X.Y>`
//! (answers `-c …` with exit 0), `bin/python` (a link), `bin/conda` (`--version` prints
//! `conda 25.1.1`; `install …` appends its arguments to the log and exits `$FAKECONDA_INSTALL_EXIT`,
//! default 0) and `conda-meta/history`. It exits `$FAKECONDA_EXIT` (default 0) before writing
//! anything when that is set. Unix only.

pub fn installer(py: &str) -> String {
    format!(
        r#"#!/bin/bash
log=${{FAKECONDA_LOG:-/dev/null}}
prefix=""
while [ $# -gt 0 ]; do
  case "$1" in -p) prefix=$2; shift 2;; *) shift;; esac
done
{{ echo "args: -f -b -p $prefix"; [ -e "$prefix/.rpyenv-incomplete" ] && echo "marker: yes" || echo "marker: no"
   echo "tos: ${{CONDA_PLUGINS_AUTO_ACCEPT_TOS:-unset}}"; }} >> "$log"
[ -n "${{FAKECONDA_EXIT:-}}" ] && exit "$FAKECONDA_EXIT"
mkdir -p "$prefix/bin" "$prefix/conda-meta"
printf '#!/bin/sh\nexit 0\n' > "$prefix/bin/python{py}"
chmod 755 "$prefix/bin/python{py}"
ln -sf "python{py}" "$prefix/bin/python"
cat > "$prefix/bin/conda" <<'EOF'
#!/bin/sh
case "$1" in
  --version) echo "conda 25.1.1";;
  install) echo "conda: $*" >> "${{FAKECONDA_LOG:-/dev/null}}"; exit "${{FAKECONDA_INSTALL_EXIT:-0}}";;
esac
EOF
chmod 755 "$prefix/bin/conda"
echo "+install" > "$prefix/conda-meta/history"
"#
    )
}
```

- [ ] **Step 2: Write the failing tests** (`tests/cli_install_conda.rs`). Copy `env`, `run` and `left_in_versions` from `tests/cli_install_pypy.rs` (the helpers there, lines 24-71).

```rust
//! `pyenv install` of a conda definition on Linux against a fake installer on a local server
//! (M7b design §4.3, §8).
#![cfg(unix)]

mod common;

use common::fakeconda::installer;
use common::fakepypy::sha256;
use common::server::{start, Reply, Server};
use common::Fixture;

const PKG: &str = "Miniconda3-py312_25.1.1-0-Linux-x86_64";

fn serve(body: &str) -> (Server, String) {
    let sha = sha256(body.as_bytes());
    (start(vec![(&format!("/{PKG}.sh"), vec![Reply::Body(body.as_bytes().to_vec())])]), sha)
}

/// `<base>/defs/<name>`: one `install_script` line for the served installer, after `extra`.
fn def_file(f: &Fixture, s: &Server, sha: &str, name: &str, step: &str, extra: &str) -> String {
    let p = f.base.join("defs").join(name);
    f.file(&p, &format!("{extra}install_script \"{PKG}\" \"{}#{sha}\" {step} verify_py312\n", s.url(&format!("/{PKG}.sh"))));
    p.display().to_string()
}

fn log(f: &Fixture) -> String {
    std::fs::read_to_string(f.base.join("fake.log")).unwrap_or_default()
}

#[test]
fn a_miniconda_definition_installs_in_place_and_adds_pip() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "export CONDA_PLUGINS_AUTO_ACCEPT_TOS=true\n");
    let lp = f.base.join("fake.log").display().to_string();
    let r = run(&f, &["install", &def], &[("FAKECONDA_LOG", &lp)]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = f.root.join("versions/miniconda3-t");
    let l = log(&f);
    assert!(l.contains(&format!("args: -f -b -p {}", v.display())), "{l}");
    assert!(l.contains("marker: yes"), "the installer ran in the final, marked folder: {l}");
    assert!(l.contains("tos: true"), "{l}");
    assert!(l.contains("conda: install --yes pip conda=25.1.1"), "{l}");
    assert!(!v.join(".rpyenv-incomplete").exists());
    assert!(v.join("conda-meta/history").is_file());
    assert_eq!(left_in_versions(&f), ["miniconda3-t"]);
    assert!(r.stderr.contains(&format!("Installed {PKG} to {}", v.display())), "{}", r.stderr);
}

#[test]
fn an_anaconda_definition_runs_no_conda_install() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "anaconda3-t", "anaconda", "");
    let lp = f.base.join("fake.log").display().to_string();
    assert_eq!(run(&f, &["install", &def], &[("FAKECONDA_LOG", &lp)]).code, 0);
    assert!(!log(&f).contains("conda: install"), "{}", log(&f));
    assert!(log(&f).contains("tos: unset"));
}

#[test]
fn a_failing_installer_leaves_nothing() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    let r = run(&f, &["install", &def], &[("FAKECONDA_EXIT", "3")]);
    assert_ne!(r.code, 0);
    assert!(r.stderr.contains("BUILD FAILED"), "{}", r.stderr);
    assert!(left_in_versions(&f).is_empty(), "{:?}", left_in_versions(&f));
}

#[test]
fn a_failing_conda_install_step_leaves_nothing() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    assert_ne!(run(&f, &["install", &def], &[("FAKECONDA_INSTALL_EXIT", "1")]).code, 0);
    assert!(left_in_versions(&f).is_empty());
}

#[test]
fn a_hash_mismatch_leaves_nothing_and_runs_nothing() {
    let f = Fixture::new();
    let (s, _) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &"c".repeat(64), "miniconda3-t", "miniconda", "");
    let lp = f.base.join("fake.log").display().to_string();
    assert_ne!(run(&f, &["install", &def], &[("FAKECONDA_LOG", &lp)]).code, 0);
    assert_eq!(log(&f), "", "the installer never ran");
    assert!(left_in_versions(&f).is_empty());
}

#[test]
fn a_failed_verify_leaves_nothing() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12").replace("exit 0\\n' > \"$prefix/bin/python3.12\"", "exit 0\\n' > \"$prefix/bin/nothing\""));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    let r = run(&f, &["install", &def], &[]);
    assert_ne!(r.code, 0);
    assert!(r.stderr.contains("invalid Python executable"), "{}", r.stderr);
    assert!(left_in_versions(&f).is_empty());
}

/// Review focus 2.
#[test]
fn force_reinstall_keeps_conda_envs() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    assert_eq!(run(&f, &["install", &def], &[]).code, 0);
    let env = f.root.join("versions/miniconda3-t/envs/work");
    std::fs::create_dir_all(env.join("conda-meta")).unwrap();
    let r = run(&f, &["install", "-f", &def], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(env.join("conda-meta").is_dir(), "the env was carried over");
    assert_eq!(left_in_versions(&f), ["miniconda3-t"]);
}

/// Review focus 1.
#[test]
fn a_folder_left_with_the_marker_is_reinstalled_without_f() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    let v = f.root.join("versions/miniconda3-t");
    f.file(&v.join("bin/python"), "");
    f.file(&v.join(".rpyenv-incomplete"), "");
    let r = run(&f, &["install", &def], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(!v.join(".rpyenv-incomplete").exists());
    assert!(v.join("conda-meta/history").is_file());
}
```

If `failed_verify`'s `.replace` doesn't match the generated text exactly, give `installer` a second parameter instead (`installer_with(py, python_file)`); the test's point is a tree without `bin/python3.12`.

- [ ] **Step 3: Run them in WSL** (`git add`, then `… m7a_wsl.sh test -p pyenv --test cli_install_conda`) → they fail (`install_script` is an unreadable line).

- [ ] **Step 4: Implement.**
  - `defs.rs`: add `Script { url: String, fragment: Option<String> }` to `Fetch`; in the statement match, `"install_package" | "install_git" | "install_script"`; for `install_script`, build `Fetch::Script` from `w[2]` exactly as `Tarball` is built. Fix every other `match` on `Fetch` the compiler reports (`grep -rn "Fetch::" crates/pyenv/src`).
  - `txn.rs`:

```rust
    /// M7b design §4.3 step 5: conda can't be moved after installing, so it installs into
    /// `versions/<name>` itself. As `place` does, an existing version is set aside; the target
    /// is then an empty folder holding the marker, before the installer runs.
    pub fn place_empty(&mut self) -> std::io::Result<()> {
        let empty = self.stage.join("prefix");
        std::fs::create_dir_all(&empty)?;
        self.place(&empty)
    }
```

  - `builder.rs` `fetch_package`: a `Fetch::Script` arm: `sha256_of_fragment` as for `Tarball`; `FetchRequest { file_name: format!("{}.sh", pkg.name), url, check: Check::Sha256(sha), dest_dir: build_path }`; return the fetched file's path (no extraction).
  - `builder.rs` `run_step`, a new arm before `verify_py*`:

```rust
        // python-build's `build_package_anaconda` and `build_package_miniconda` (M7b design §4.3):
        // the installer writes straight into the final folder, which `place_empty` created with
        // the marker; `-f` lets it use the existing folder.
        "anaconda" | "miniconda" => {
            if !txn.placed() {
                txn.place_empty().map_err(|e| {
                    InstallError::Message(format!("pyenv: cannot create {}: {e}", plan.prefix.display()))
                })?;
            }
            let vars = &job.definition.vars;
            let mut c = command("bash", &plan.env);
            c.arg(src).args(["-f", "-b", "-p"]).arg(&plan.prefix);
            for (k, v) in vars {
                c.env(k, v);
            }
            logged(log, &mut c)?;
            if step == "miniconda" {
                // Upstream's workaround for pyenv issue #2070: pip, without upgrading conda.
                let conda = plan.prefix.join("bin").join("conda");
                let out = command(&conda, &plan.env).arg("--version").stdin(Stdio::null()).output();
                if interrupted() {
                    return Err(InstallError::Interrupted);
                }
                let v = out
                    .ok()
                    .and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().nth(1).map(str::to_string))
                    .ok_or_else(|| InstallError::Message(format!("pyenv: {} --version gave no version", conda.display())))?;
                let mut c = command(&conda, &plan.env);
                c.args(["install", "--yes", "pip", &format!("conda={v}")]);
                for (k, val) in vars {
                    c.env(k, val);
                }
                logged(log, &mut c)?;
            }
            Ok(())
        }
```

  `command(&conda, …)` takes `impl AsRef<OsStr>`; adjust borrowing as the compiler asks. Check that `verify` (unchanged) finds `bin/python` for these names (`plan.xy` is `None`, M7a ruling R6).

- [ ] **Step 5: Run** the WSL tests → pass. Run `… m7a_wsl.sh test -p pyenv --test cli_install_pypy` and `--test cli_install` (CPython) too: unchanged.

- [ ] **Step 6: Commit.** `git commit -m "M7b: install_script definitions, the anaconda and miniconda steps, in-place installs (Txn::place_empty)"`.

---

### Task 5: Routing conda names through the table

**Files:**
- Modify: `crates/pyenv/src/commands/install.rs` (`conda_text`, beside `pypy_text`)
- Modify: `crates/pyenv/src/install/builder.rs` (make `colorize` `pub(crate)` if you use it for `ERROR`)
- Test: `crates/pyenv/tests/cli_install_conda.rs` (more tests)

**Interfaces:**
- Consumes: Task 3's `conda` module, Task 4's steps.
- Produces: `fn conda_text(found: &Found, interpretable: bool, fetcher: &Fetcher, say: &mut dyn FnMut(&str)) -> Option<Result<String, String>>`; `None` for a non-conda name, or a user definition the reader can read itself.

- [ ] **Step 1: Write the failing tests** (append to `cli_install_conda.rs`). A helper writes a table folder for `RPYENV_TEST_CONDA_TABLES`:

```rust
fn table(f: &Fixture, rows: &[(&str, String)]) -> String {
    let arch = pyenv::install::conda::host_arch().expect("tests run on x64 or aarch64");
    let d = f.base.join("tables");
    let text: String = rows.iter().map(|(n, v)| format!("{n}\t{arch}\t{v}\n")).collect();
    f.file(&d.join("linux.tsv"), &text);
    d.display().to_string()
}

#[test]
fn a_vendored_name_installs_from_its_table_row_with_the_terms_notice() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let row = format!("ok {PKG} {}#{sha} miniconda verify_py312 CONDA_PLUGINS_AUTO_ACCEPT_TOS=true", s.url(&format!("/{PKG}.sh")));
    let t = table(&f, &[("miniconda3-25.1.1-0", row)]);
    let lp = f.base.join("fake.log").display().to_string();
    let r = run(&f, &["install", "miniconda3-25.1.1-0"], &[("RPYENV_TEST_CONDA_TABLES", &t), ("FAKECONDA_LOG", &lp)]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("rpyenv: Miniconda's defaults channel is covered by Anaconda's terms of service"), "{}", r.stderr);
    assert!(log(&f).contains("tos: true"));
}

#[test]
fn no_notice_without_the_variable() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let t = table(&f, &[("miniforge3-1", format!("ok {PKG} {}#{sha} miniconda verify_py312", s.url(&format!("/{PKG}.sh"))))]);
    let r = run(&f, &["install", "miniforge3-1"], &[("RPYENV_TEST_CONDA_TABLES", &t)]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(!r.stderr.contains("terms of service"));
}

// allowlist D-109
#[test]
fn an_unsupported_row_is_refused_by_name() {
    let f = Fixture::new();
    let t = table(&f, &[("mambaforge", "unsupported no published hash".into())]);
    let r = run(&f, &["install", "mambaforge"], &[("RPYENV_TEST_CONDA_TABLES", &t)]);
    assert_eq!(r.code, 1);
    assert_eq!(r.stderr.trim_end(), "python-build: mambaforge: rpyenv cannot install this conda definition (no published hash)");
}

#[test]
fn a_no_binary_row_prints_upstreams_lines() {
    let f = Fixture::new();
    let t = table(&f, &[("anaconda3-1", "no-binary Anaconda".into())]);
    let arch = pyenv::install::conda::host_arch().unwrap();
    let r = run(&f, &["install", "anaconda3-1"], &[("RPYENV_TEST_CONDA_TABLES", &t)]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains(&format!("\nERROR: The binary distribution of Anaconda is not available for {arch}.\n")), "{:?}", r.stderr);
}

/// Review focus 3. allowlist D-109
#[test]
fn a_user_definition_without_a_hash_is_refused() {
    let f = Fixture::new();
    let (s, _) = serve(&installer("3.12"));
    let p = f.base.join("defs").join("miniforge3-mine");
    f.file(&p, &format!("case \"$(anaconda_architecture 2>/dev/null || true)\" in\n* )\n  install_script \"{PKG}\" \"{}\" \"miniconda\" verify_py312\n  ;;\nesac\n", s.url(&format!("/{PKG}.sh"))));
    let r = run(&f, &["install", &p.display().to_string()], &[]);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("python-build: miniforge3-mine: rpyenv cannot read this conda definition (no sha256 in "), "{}", r.stderr);
    assert_eq!(s.hits(&format!("/{PKG}.sh")), 0, "nothing downloaded");
}

// allowlist D-108
#[test]
fn miniconda_latest_takes_its_hash_from_the_listing() {
    let f = Fixture::new();
    let arch = pyenv::install::conda::host_arch().unwrap();
    let file = format!("Miniconda3-latest-{arch}.sh");
    let body = installer("3.12");
    let sha = sha256(body.as_bytes());
    let listing = format!("<table><tr><td><a href=\"{file}\">{file}</a></td><td>1M</td><td>2026-08-28</td><td>{sha}</td></tr></table>");
    let s = start(vec![("/miniconda/", vec![Reply::Body(listing.into_bytes())]), (&format!("/miniconda/{file}"), vec![Reply::Body(body.into_bytes())])]);
    let t = table(&f, &[("miniconda3-latest", format!("ok-latest {PKG} {} miniconda verify_py3_latest", s.url(&format!("/miniconda/{file}"))))]);
    let base = s.url("");
    let r = run(&f, &["install", "miniconda3-latest"], &[("RPYENV_TEST_CONDA_TABLES", &t), ("RPYENV_TEST_CONDA", &base)]);
    assert_eq!(r.code, 0, "{}", r.stderr);
}

/// Review focus 5.
#[test]
fn latest_without_a_listing_row_is_refused_before_downloading() {
    let f = Fixture::new();
    let arch = pyenv::install::conda::host_arch().unwrap();
    let file = format!("Miniconda3-latest-{arch}.sh");
    let s = start(vec![("/miniconda/", vec![Reply::Body(b"<table></table>".to_vec())]), (&format!("/miniconda/{file}"), vec![Reply::Body(b"x".to_vec())])]);
    let t = table(&f, &[("miniconda3-latest", format!("ok-latest {PKG} {} miniconda verify_py3_latest", s.url(&format!("/miniconda/{file}"))))]);
    let base = s.url("");
    let r = run(&f, &["install", "miniconda3-latest"], &[("RPYENV_TEST_CONDA_TABLES", &t), ("RPYENV_TEST_CONDA", &base)]);
    assert_eq!(r.code, 1);
    assert_eq!(r.stderr.trim_end(), format!("python-build: miniconda3-latest: the publisher's SHA-256 for {file} couldn't be read from {base}/miniconda/, so rpyenv won't install it"));
    assert_eq!(s.hits(&format!("/miniconda/{file}")), 0);
}

// allowlist D-108
#[test]
fn miniforge_latest_installs_the_versioned_twin_with_its_sha256_file() {
    let f = Fixture::new();
    let arch = pyenv::install::conda::host_arch().unwrap();
    let file = format!("Miniforge3-26.7.2-0-{arch}.sh");
    let body = installer("3.12");
    let sha = sha256(body.as_bytes());
    let s = start(vec![
        ("/miniforge/releases/latest", vec![Reply::Redirect("/miniforge/releases/tag/26.7.2-0".into())]),
        (&format!("/miniforge/releases/download/26.7.2-0/{file}.sha256"), vec![Reply::Body(format!("{sha}  {file}\n").into_bytes())]),
        (&format!("/miniforge/releases/download/26.7.2-0/{file}"), vec![Reply::Body(body.into_bytes())]),
    ]);
    let t = table(&f, &[("miniforge3-latest", format!("ok-latest Miniforge3-{arch} https://github.com/conda-forge/miniforge/releases/latest/download/Miniforge3-{arch}.sh miniconda verify_py3_latest"))]);
    let base = s.url("");
    let r = run(&f, &["install", "miniforge3-latest"], &[("RPYENV_TEST_CONDA_TABLES", &t), ("RPYENV_TEST_CONDA", &base)]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(s.hits(&format!("/miniforge/releases/download/26.7.2-0/{file}")), 1);
}
```

If `Server` has no `hits(path)` accessor (it has a `hits` field, `server.rs:159`), add a small `pub fn hits(&self, path: &str) -> usize` reading it.

- [ ] **Step 2: Run them in WSL** → they fail.

- [ ] **Step 3: Implement `conda_text`** in `commands/install.rs`, next to `pypy_text`, and call it where `pypy_text` is matched (line ~316): `let special = pypy_text(&found, first.is_ok()).or_else(|| conda_text(&found, first.is_ok(), &probe, &mut say))`, where `probe = Fetcher::from_env(&env, None)` is built once before the loop (no cache: it only reads two small pages). Body:

```rust
/// M7b design §4.2-4.3: what a conda-family definition installs here, as a one-line definition
/// the interpreter reads. The vendored copy uses the table's row; a user's own one that the
/// interpreter can't read is evaluated with `bash`. `None` for every other definition.
fn conda_text(found: &Found, interpretable: bool, fetcher: &Fetcher, say: &mut dyn FnMut(&str)) -> Option<Result<String, String>> {
    let builtin = found.origin == Origin::Builtin;
    if !conda::is_conda_name(&found.name) || (!builtin && interpretable) {
        return None;
    }
    let name = &found.name;
    let Some(arch) = conda::host_arch() else {
        return Some(Err(format!("\nERROR: The binary distribution of {name} is not available for Linux-{}.\n", std::env::consts::ARCH)));
    };
    let verdict = match &found.origin {
        Origin::Builtin => conda::linux_table().map(|rows| {
            conda::linux_verdict(&rows, name, arch).cloned().unwrap_or(conda::Verdict::NoBinary(name.clone()))
        }),
        Origin::Dir(d) => conda::evaluate(&d.join(name), arch),
        Origin::Path(p) => conda::evaluate(p, arch),
    };
    let notice = |i: &conda::Install, say: &mut dyn FnMut(&str)| {
        if i.env.iter().any(|(k, _)| k == "CONDA_PLUGINS_AUTO_ACCEPT_TOS") {
            say(TERMS_NOTICE);
        }
    };
    Some(verdict.and_then(|v| match v {
        conda::Verdict::Ok(i) => {
            notice(&i, say);
            Ok(conda::definition_text(&i))
        }
        conda::Verdict::OkLatest(i) => {
            notice(&i, say);
            conda::latest_source(name, &i, arch, fetcher).map(|i| conda::definition_text(&i))
        }
        conda::Verdict::NoBinary(what) => Err(format!("\nERROR: The binary distribution of {what} is not available for {arch}.\n")),
        conda::Verdict::Unsupported(why) if builtin => Err(format!("python-build: {name}: rpyenv cannot install this conda definition ({why})")),
        conda::Verdict::Unsupported(why) => Err(format!("python-build: {name}: rpyenv cannot read this conda definition ({why})")),
    }))
}

const TERMS_NOTICE: &str = "rpyenv: Miniconda's defaults channel is covered by Anaconda's terms of service, which require a paid license for organizations of 200 or more people; this install accepts them, as upstream pyenv does. Miniforge (conda-forge) isn't covered.";
```

  Check how `say` prints a message with embedded newlines (the no-binary case must produce exactly an empty line, the `ERROR:` line, an empty line); adjust the string so the test's expected output holds. Under `RPYENV_TEST_CONDA_TABLES`, an `ok-latest` URL under `https://github.com/conda-forge/miniforge/` is ignored for miniforge (the URL comes from `sources()`), while a miniconda `ok-latest` keeps the table's URL (the test points it at the fake server).

- [ ] **Step 4: Run** the WSL tests → pass; `… m7a_wsl.sh test -p pyenv --test cli_install_pypy` unchanged.

- [ ] **Step 5: Commit.** `git commit -m "M7b: pyenv install routes conda names through the table (terms notice, publisher hash for -latest, refusals)"`.

---

### Task 6: Upstream's conda shim list (closes D-44)

**Files:**
- Create: `crates/rpyenv-core/src/conda_list.rs`; Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod conda_list;`)
- Modify: `crates/rpyenv-core/src/shimset.rs` (`wanted`, Linux flavor)
- Modify: `crates/pyenv/tests/cli_rehash.rs` (invert `rehash_keeps_the_tools_of_a_conda_version`)

**Interfaces:**
- Consumes: Task 2's `crates/pyenv/python-build/rehash/conda.d/default.list`.
- Produces: `conda_list::names() -> &'static BTreeSet<&'static str>` (`std::sync::OnceLock`), `conda_list::conda_exists(versions_dir: &Path) -> bool`. `shimset::wanted` (Linux) drops listed names when `conda_exists`; `executables_pyenv` (used by `versions --executables`) is unchanged.

- [ ] **Step 1: Write the failing tests.** Unit tests in `conda_list.rs`:

```rust
#[test]
fn the_list_is_upstreams_minus_comments() {
    let n = names();
    assert!(n.contains("curl") && n.contains("clear") && n.contains("["));
    assert!(!n.iter().any(|w| w.starts_with('#') || w.is_empty()));
    assert_eq!(n.len(), 177, "pyenv 2.8.9's list; update this when the sync changes it");
}

/// Review focus 4.
#[test]
fn conda_in_an_env_or_a_dot_folder_triggers_the_list() {
    for path in ["v/bin/conda", "v/envs/e/bin/conda", ".old-v/bin/conda"] {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "").unwrap(); // existence, not the execute bit
        assert!(conda_exists(d.path()), "{path}");
    }
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("3.12.0/bin")).unwrap();
    assert!(!conda_exists(d.path()));
}
```

(If `tempfile` isn't a dev-dependency of `rpyenv-core`, use the temp-dir helper its other tests use; grep `fn tmp` in `crates/rpyenv-core/src`.)

In `tests/cli_rehash.rs`, replace `rehash_keeps_the_tools_of_a_conda_version` (and its `allowlist D-44` comment) with:

```rust
/// Upstream's conda.bash hook (M7b design §5): once a version has `bin/conda`, the names in
/// conda.d/default.list get no shim, whichever version provides them (here `clear`, also in
/// 3.12.0); `which` still finds conda's copies.
#[cfg(unix)]
#[test]
fn rehash_drops_condas_system_tools() {
    let f = Fixture::new();
    f.exe("miniconda3-latest/bin/python");
    f.exe("miniconda3-latest/bin/conda");
    let curl = f.exe("miniconda3-latest/bin/curl");
    f.exe("miniconda3-latest/bin/clear");
    f.exe("3.12.0/bin/clear");
    let r = f.pyenv(&["rehash"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 0));
    assert_eq!(f.pyenv(&["shims", "--short"]).stdout, "conda\npython\n");
    let r = f.pyenv_env(&["which", "curl"], &[("PYENV_VERSION", "miniconda3-latest")]);
    assert_eq!(r.stdout, format!("{}\n", curl.display()));
}

#[cfg(unix)]
#[test]
fn without_conda_the_listed_names_keep_their_shims() {
    let f = Fixture::new();
    f.exe("3.12.0/bin/python");
    f.exe("3.12.0/bin/clear");
    assert_eq!(f.pyenv(&["rehash"]).code, 0);
    assert_eq!(f.pyenv(&["shims", "--short"]).stdout, "clear\npython\n");
}
```

- [ ] **Step 2: Run them in WSL** → fail.

- [ ] **Step 3: Implement** `conda_list.rs`:

```rust
//! Upstream's conda rehash hook (pyenv.d/rehash/conda.bash, M7b design §5): when any version
//! or env has `bin/conda`, the names in conda.d/default.list (conda's bundled system tools:
//! `curl`, `sed`, `clear`, …) get no shim, so the system's copies keep working.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

/// Vendored byte for byte by ci/sync_python_build.py; pyenv's MIT license is beside it.
static LIST: &str = include_str!("../../pyenv/python-build/rehash/conda.d/default.list");

/// The list after upstream's `sed 's/#.*$//; /^[[:space:]]*$/d'`, split into words; each is
/// compared whole (upstream's `case` pattern; `[` alone matches itself).
pub fn names() -> &'static BTreeSet<&'static str> {
    static NAMES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        LIST.lines()
            .map(|l| l.split('#').next().unwrap_or(""))
            .flat_map(str::split_whitespace)
            .collect()
    })
}

/// Upstream's `conda_exists`: `versions/*/bin/conda` or `versions/*/envs/*/bin/conda`
/// exists, dot folders included (`dotglob`). Existence only, as the glob tests it.
pub fn conda_exists(versions_dir: &Path) -> bool {
    let has = |d: &Path| d.join("bin").join("conda").symlink_metadata().is_ok();
    let Ok(rd) = std::fs::read_dir(versions_dir) else { return false };
    rd.filter_map(Result::ok).map(|e| e.path()).any(|v| {
        has(&v)
            || std::fs::read_dir(v.join("envs"))
                .map(|envs| envs.filter_map(Result::ok).any(|e| has(&e.path())))
                .unwrap_or(false)
    })
}
```

  In `shimset::wanted`, Linux arm: compute `let drop = crate::conda_list::conda_exists(&ctx.versions_dir());` and filter `name` out when `drop && conda_list::names().contains(name.to_str().unwrap_or(""))`.

- [ ] **Step 4: Run** WSL `… m7a_wsl.sh test -p rpyenv-core` and `… test -p pyenv --test cli_rehash` → pass. Windows `cargo test -p rpyenv-core` → pass (the list compiles in; Windows is unaffected because the filter is in the Linux arm).

- [ ] **Step 5: Commit.** `git commit -m "M7b: rehash drops conda's system tools listed in upstream's conda.d/default.list (closes D-44)"`.

---

### Task 7: Real installs in CI

**Files:**
- Modify: `.github/workflows/install-linux.yml` (a `conda` job)
- Modify: `ci/check_install.sh` (a conda branch)

**Interfaces:**
- Consumes: everything above.
- Produces: CI jobs `conda (ubuntu-latest, miniforge3)`, `conda (ubuntu-24.04-arm, miniforge3)`, `conda (ubuntu-latest, miniconda3)`, `conda (ubuntu-latest, miniforge3-latest)`.

- [ ] **Step 1: `check_install.sh`.** Add a branch before `*)`:

```bash
  anaconda*|miniconda*|miniforge*|mambaforge*)
    # Conda (M7b design §8): python3 is the interpreter, the install is in place (no staging to
    # check), and conda's own scripts carry shebangs this check doesn't model.
    name=python3
    mods="ssl, sqlite3, ctypes, zlib, bz2, lzma"
    conda=1 ;;
```

  Initialize `conda=0` before the `case`, and wrap the shebang loop in `if [ "$conda" = 0 ]; then … fi`. After the shim-prefix check, for conda: `[ ! -e "$root/shims/curl" ] || { echo "::error::conda's curl got a shim"; exit 1; }`, then `PYENV_ROOT="$root" "$PYENV_BIN" virtualenv "$v" ci-env` and check `"$p/envs/ci-env/conda-meta"` is a directory. The script needs the `pyenv` binary: add an optional third argument `<pyenv binary>` (default `pyenv` on `PATH`) and pass `target/release/pyenv` from the workflow.

- [ ] **Step 2: The job.** After the `pypy` job in `install-linux.yml`:

```yaml
  # M7b design §8: real conda installs. A numbered Miniforge on both architectures, a numbered
  # Miniconda 25.x (the terms variable and `conda install pip` against defaults), and
  # miniforge3-latest (the publisher's .sha256 path). Anaconda's ~1 GB installer is left to the
  # tier-1 fake: its step is Miniconda's without `conda install`.
  conda:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { runner: ubuntu-latest, family: miniforge3 }
          - { runner: ubuntu-24.04-arm, family: miniforge3 }
          - { runner: ubuntu-latest, family: miniconda3 }
          - { runner: ubuntu-latest, family: miniforge3-latest }
    runs-on: ${{ matrix.runner }}
    timeout-minutes: 30
    name: conda (${{ matrix.runner }}, ${{ matrix.family }})
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02
        with:
          toolchain: "1.96"
      - run: cargo build --release -p pyenv -p pyenv-shim
      - name: pyenv install
        env:
          FAMILY: ${{ matrix.family }}
        run: |
          case "$FAMILY" in
            *-latest) v=$FAMILY ;;
            *) v=$(target/release/pyenv latest -k "$FAMILY") ;;
          esac
          echo "VERSION=$v" >> "$GITHUB_ENV"
          PYENV_ROOT="$RUNNER_TEMP/root" target/release/pyenv install "$v"
      - name: Check the install
        run: bash ci/check_install.sh "$RUNNER_TEMP/root" "$VERSION" target/release/pyenv
```

  Before relying on `latest -k miniconda3`, check in WSL what it picks (`target/debug/pyenv latest -k miniconda3` against the vendored definitions) and that the pick is a 25.x name with `CONDA_PLUGINS_AUTO_ACCEPT_TOS`; if not, pin the matrix entry to a named 25.x version with a comment saying why.

- [ ] **Step 3: Check the shell.** In WSL: `bash -n ci/check_install.sh`. Run it against a fake root built by hand (a `versions/miniforge3-x` with `bin/python3` printing `sys.prefix` correctly is too much to fake; it's enough to run the real install locally in WSL once, in a scratch `PYENV_ROOT` under `/tmp`, with the debug build: `PYENV_ROOT=/tmp/m7b-real target/debug/pyenv install <numbered miniforge3>` then `bash ci/check_install.sh /tmp/m7b-real <it> target/debug/pyenv`). Put these commands in a script file `C:\tmp\m7b_real.sh` and run it with `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m7b_real.sh`. Record the outcome; delete `/tmp/m7b-real` afterwards.

- [ ] **Step 4: Commit.** `git commit -m "M7b: real conda installs in CI (Miniforge on both arches, Miniconda 25.x, miniforge3-latest) and their check"`.

---

### Task 8: Parity, allowlist and docs

**Files:**
- Modify: `docs/parity/allowlist.md` (remove D-44; amend D-58; add D-107, D-108, D-109)
- Modify: `parity/diff_cases.py` (three `latest -k` cases)
- Modify: `parity/golden/linux/*` (regenerated `install --list`)
- Modify: `docs/specs/2026-09-27-rpyenv-design.md` (§14, §15.1)
- Modify: `docs/superpowers/specs/2026-10-15-m7b-conda-linux-design.md` (corrections below)
- Modify: `README.md`
- Modify: any test comment citing a new row (Task 5's tests already cite D-108 and D-109; add `// allowlist D-107` above `a_vendored_name_installs_from_its_table_row_with_the_terms_notice`)

**Interfaces:**
- Consumes: everything above.
- Produces: `python parity/coverage.py` passes with every row covered.

- [ ] **Step 1: Allowlist.** Delete the D-44 row. Amend D-58's rpyenv cell: add "Conda installs (M7b) run in the final folder, which holds `.rpyenv-incomplete` until they finish; a failure or Ctrl+C deletes it and restores the previous version". Add:

| ID | OS | Command | Upstream | rpyenv | Reason |
|---|---|---|---|---|---|
| D-107 | Linux | `install` (Miniconda 25.x, `miniconda3-latest`) | Sets `CONDA_PLUGINS_AUTO_ACCEPT_TOS` silently | Sets it and prints one line first: Miniconda's defaults channel is under Anaconda's terms (a paid license for organizations of 200 or more), Miniforge isn't | The user is told what the install accepts for them (M7b design D3). |
| D-108 | Linux | `install` (`miniconda-latest`, `miniconda2-latest`, `miniconda3-latest`, `miniforge3-latest`) | Downloads the installer unchecked | Checks it against the publisher's SHA-256 read at install time (Miniconda's listing page; Miniforge's versioned `.sha256`, installing that versioned twin); refuses when it can't be read | Every download is checked against a published hash (spec §9.3; M7b design D2, D6). |
| D-109 | Linux | `install` (conda) | Tries `mambaforge`, `mambaforge-pypy3` and `miniforge-pypy3` (unhashed; the publisher answers 404), and installs a user's unhashed conda definition | Refuses each by name: `rpyenv cannot install this conda definition (<reason>)` / `rpyenv cannot read this conda definition (<reason>)` | No published hash (M7b design §4.1.3, §6). |

- [ ] **Step 2: Differential cases.** In `parity/diff_cases.py`, after the pypy cases: `Case("latest -k anaconda3", ("latest", "-k", "anaconda3"), os="Linux")`, the same for `miniconda3` and `miniforge3`.

- [ ] **Step 3: Goldens and the differential run, in WSL.** `git add` everything, then `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m7a_parity.sh`. If `install --list` goldens differ, regenerate them (`python3 parity/diff.py --rpyenv target/debug --upstream ~/m7a-upstream/pyenv-<sha> --update-golden` in `~/rpyenv-linux`, via a script file), copy `parity/golden/linux/` back to the Windows checkout, and check with `git diff --stat parity/golden` plus a script that only lines starting with `anaconda`, `miniconda`, `miniforge` or `mambaforge` were added. Re-run `m7a_parity.sh`: every case `same` (the three new ones included) and `parity/coverage.py` passing.

- [ ] **Step 4: Parent spec.** §14 item 7: "M7b Anaconda, Miniconda and Miniforge on Linux (spec docs/superpowers/specs/2026-10-15-m7b-conda-linux-design.md); M7c the same on Windows". §15.1 resolution 3: add a line "**Linux (M7b, 2026-10-15):** upstream's `conda.d/default.list` only, as upstream's hook applies it; the ownership rule and its list are M7c's (Windows)." Choice 4: "Implemented on Linux by M7b (§4.3 of its spec)."

- [ ] **Step 5: M7b spec corrections** (each was a plan-time finding): §4.1.1: the list is vendored byte for byte to `crates/pyenv/python-build/rehash/conda.d/default.list` (pyenv's `LICENSE` beside it), not to `crates/pyenv/conda/` with a header; §4.3 step 7: verify runs the existing rule (`bin/python` for names without an X.Y, M7a ruling R6) plus the module checks of the `verify_pyXY` step; §7: the terms notice is an extra **stderr** line; §4.1.3's `ok-latest` names include `miniconda-latest` (D2's `miniconda*-latest`).

- [ ] **Step 6: README.** Next to the PyPy bullet (`README.md:55`), a conda bullet: Anaconda, Miniconda and Miniforge on Linux (x64, aarch64) under upstream's names (`pyenv install miniforge3-<release>`), each installer checked against its published SHA-256; Miniconda 25.x prints a note that its defaults channel is under Anaconda's terms (Miniforge isn't); `-latest` names are checked against a hash read from the same publisher at install time, which catches a broken download but not a compromised publisher, so pick a numbered version for a hash upstream pyenv reviewed; conda's bundled system tools get no shims, as with upstream. Windows: not yet (M7c).

- [ ] **Step 7: Checks.** Windows: `cargo test -p pyenv`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `python -m unittest ci/test_conda_tables.py ci/test_sync_python_build.py ci/test_pypy_tables.py`, `python parity/coverage.py`. WSL: `… m7a_wsl.sh all`, `… m7a_wsl.sh py` with `-m unittest discover -s ci -p "test_*.py"` equivalent (`python3 -m unittest discover -s ci -p "test_*.py"` in `~/rpyenv-linux`, from a script file).

- [ ] **Step 8: Commit.** `git commit -m "M7b: allowlist (D-44 closed, D-58 amended, D-107 to D-109), latest -k conda cases, goldens, parent spec, README"`.
