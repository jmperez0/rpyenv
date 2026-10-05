# M4b Built-in Virtualenvs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** rpyenv gets pyenv-virtualenv's commands built in on both flavors. On Linux they match v1.4.0 byte for byte where v1.4.0 works, and do what it meant where it's broken. On Windows they're new, with junctions in place of symlinks. pyenv-virtualenv's own bats suite becomes the third upstream suite.

**Architecture:**
- **One core module, `rpyenv_core::venv`,** holds the facts about an env that `which`, `virtualenv-prefix`, `virtualenvs`, `virtualenv-delete`, `uninstall` and `sh-activate` share:
  - reading `pyvenv.cfg`;
  - resolving an env's base prefix;
  - the `which` fallbacks.
- **The commands live in two new `pyenv` files:**
  - `commands/venv.rs`: create, list, prefix, delete;
  - `commands/activate.rs`: activate, deactivate, init.
- **Windows** gets a small junction module, plus routing in the PowerShell function.
- **Parity:**
  - `parity/bats_venv_run.sh` runs the pinned plugin's bats against rpyenv's multicall links;
  - `parity/diff.py` installs the pinned plugin on the upstream side, so byte-exact output is checked against upstream's own scripts.

**Tech Stack:** Rust (workspace crates `rpyenv-core`, `pyenv`), `windows-sys` 0.61, bats-core 1.11.1, Python 3 parity scripts, GitHub Actions.

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md`: §2 D4, §6 "Plugin dispatch", §10 "Virtualenvs", §12. Research the plan argues from:
- `docs/parity/pyenv-virtualenv-m4-reference.md` (the "reference" below; v1.4.0 behavior, cited);
- `docs/parity/pyenv-virtualenv-m4-harness-and-windows.md` (the "harness doc").

## Global Constraints

- **Upstream pins:**
  - pyenv-virtualenv **v1.4.0, commit `eda64556af9b2992386deeb75dad2130899fc4c9`** (fetch by commit, never by tag);
  - pyenv 2.8.8 (`07171d013cac53d1cc9248b4c160217f288a2965`);
  - pyenv-win 856ed5a;
  - bats-core 1.11.1.
- **User decisions (2026-10-05):**
  1. Implement the **intended** behavior where v1.4.0 is broken, with one allowlist row per deliberate difference.
  2. A third bats suite, with the **lenient unstub overlay**; `installer.bats` is dropped; it has its own expected list.
  3. **`virtualenv-init` on Linux**, with auto-activation for bash, zsh and fish. On Windows it prints nothing and exits 0.
  4. **Windows `activate`/`deactivate` set variables and the prompt only, through the shell function.** `PATH` is never touched.
- **Spec §10:**
  - layout `versions/<base>/envs/<name>`, with `versions/<name>` linking to it (a symlink on Linux, a **directory junction** on Windows);
  - creation is `python -m venv` from the base;
  - activate/deactivate set `PYENV_VERSION`, `PYENV_VIRTUAL_ENV` and `VIRTUAL_ENV`;
  - the command fallbacks (system site packages via `pyvenv.cfg` `home`, `python*-config` from the base, `conda` from the owning install, and old virtualenvs via `orig-prefix.txt` and `no-global-site-packages.txt`);
  - uninstalling a base deletes its envs, each confirmed unless `-f`;
  - uninstalling by link name deletes the env and the link.
- **D4:** no bash hooks. The plugin's hook points (`before_virtualenv`, `after_activate`, …) have no equivalent (allowlist D-51).
- **Safety, carried from M4a:**
  - Never delete or replace anything rpyenv didn't make.
  - Never follow a link or junction when deleting.
  - Every Windows name segment passes `is_safe_win_segment`.
  - Every target is checked to be a direct child of the folder it belongs to.
- **Messages:** on Linux, byte-identical to v1.4.0 (`pyenv-virtualenv: ` prefix, backtick-apostrophe quoting). On Windows, the same text with CRLF (`Output::emit`).
- **Running tests:**
  - Windows: `cargo test`.
  - Linux: through the WSL runner `C:\tmp\m4b_wsl.sh` (Task 1, Step 1), never with `$VARS` in `wsl … bash -c`.
  - Bats as root: `C:\tmp\sync288_bats_root.sh`.
  - Every test run uses scratch `PYENV`, `PYENV_ROOT`, `PYENV_HOME` and `HOME`.
- **Commits:** a one-line `-m`, or `-F C:\tmp\cm_<random>.txt`. No attribution lines.

## Review Focus

These are the five input classes or failure modes most likely to bite a user, none exercised by a task's main tests. Each has a pinning test in its owning task.

1. **A name that escapes the versions folder:** `pyenv virtualenv 3.12.1 ../x`, `virtualenv-delete ../../x`, `uninstall 3.12.1/envs/..`, `x\..\y` on Windows. Expected: refused with the name message; nothing outside `versions/` is created or removed. Pinned in Tasks 3, 4 and 7.
2. **A `versions/<name>` that is a real version folder**, not a link (an env named like an installed version). Expected: `virtualenv` refuses even with `-f`; `virtualenv-delete <name>` never deletes the version. Pinned in Tasks 3 and 4.
3. **A user's own symlink or junction in `versions/`**, pointing outside `PYENV_ROOT`. Expected: uninstalling the base never follows it; `virtualenv-delete` refuses ("symlink for unknown location"). Pinned in Tasks 4 and 7.
4. **A failed `python -m venv`.** Expected: no half-made env and no dangling link remain, and an env that already existed before `-f` is kept. Pinned in Tasks 3 and 7.
5. **`sh-activate` with odd values:** a `PS1` or path containing `"`, `$` or `ñ`, and an env path containing a space. Expected: a POSIX value is double-quoted exactly as upstream does (upstream doesn't escape, and rpyenv matches it); PowerShell values go through `ps_literal`. Pinned in Tasks 5 and 8.

## Decisions

1. **Intended behavior, as user decision 1 asks, with allowlist rows.** Take the next free D-number at execution time; numbers are given here as D-95 onward.
   - **D-95, Linux, `which`, `exec`, shims:** the fallbacks work. When a selected env lacks a command, rpyenv tries the fallback for that env before the next version and before `system`, and the result is always a file. Upstream at 2.8.8 runs its hooks only after the system step, so they never fire through shims, and the system-site-packages one can return a directory (reference "How `pyenv which` runs them").
   - **D-96, both, `virtualenv`:**
     - **Creation is `<base python> -m venv` only.** rpyenv never installs or prefers the `virtualenv` package, and never runs pip in the base (spec §10).
     - **`-p`/`--python <x>` takes the next word,** then resolves `<x>` as upstream does (`which` in the base, else the last `whence` hit) and runs `<x> -m venv`.
     - **`--help` appends no backend help.**
     - **Conda bases** (Linux) use `conda create`, as upstream does.
   - **D-97, both, `virtualenv`:**
     - `-u`/`--upgrade` implies `-f` before every check.
     - The `versions/<name>` pre-check refuses only when the name is taken by something other than this env's own link. Upstream refuses always (broken guard, reference line 279).
     - A non-link `versions/<name>` is refused even with `-f`. Upstream would put a link inside it.
   - **D-98, Linux, `virtualenv`:** an env made from `system` gets no self-link (upstream: `versions/x/x -> versions/x`).
   - **D-99, both, `virtualenv`:**
     - **pip is ensured for every base, unless declined.** If the new env has no pip and none of `--without-pip`/`--no-pip`/`--no-setuptools` was given, rpyenv runs `<env python> -s -m ensurepip`. If that fails, it runs the `GET_PIP` file when one is set. Upstream skips this for every 3.10–3.19 base (the `"3.1"*` pattern).
     - **rpyenv never downloads `get-pip.py`.** That would be an unverified download; spec §2 makes rpyenv no hash authority. `GET_PIP_URL` is ignored, with a message.
   - **D-100, Linux, `sh-activate`/`sh-deactivate` for `pwsh`:** PowerShell code (upstream prints POSIX `export` lines, which pwsh can't run).
   - **D-101, Windows, every virtualenv command:** these are new; pyenv-win has none. The specifics:
     - junctions;
     - `versions` lists `<base>\envs\<name>` and `<name> --> <target>`;
     - `<base>/envs/<name>` is accepted and printed with `\`;
     - no `-win32` renaming of env names;
     - `activate`/`deactivate` print the set command and exit 1 without integration (as D-89);
     - `virtualenv-init` prints nothing and exits 0;
     - only the system-site-packages fallback applies (conda is M7);
     - multi-version `virtualenv-prefix` joins with `;`.
   - **D-102, Linux, `virtualenv-init`:** the `source activate` helper scripts live in `$PYENV_ROOT/.rpyenv/virtualenv/shims` (or `$PYENV_VIRTUALENV_ROOT/shims` when that is set). rpyenv writes them, under M4a's link rules.
   - **D-103, both, `commands`, `help`, `completions`, `init -`:** the virtualenv commands are built in, so they always appear, as if pyenv-virtualenv were installed. That is a difference only against bare upstream.
   - **Widened instead of added:**
     - D-51 gains the commands `virtualenv`, `activate`, `deactivate` (no hook points);
     - D-52 gains "pyenv-virtualenv's suite stubs core commands rpyenv answers in-process".
2. **Where creation runs.**
   - **The interpreter:**
     - a non-system base uses `lookup::which_pyenv` (Linux) or `which_win_runnable` (Windows) for `python`, with `PYENV_VERSION` set to the base;
     - `system` uses the first of `python3`, `python`, `python2` for which `<py> -m venv --help` succeeds (upstream's order).
   - **The working directory** is `PYENV_VIRTUALENV_CACHE_PATH`, else `PYTHON_BUILD_CACHE_PATH`, else `<root>/cache`, created if missing.
   - **The child's environment:**
     - `PIP_REQUIRE_VENV`, `PIP_REQUIRE_VIRTUALENV` and `VIRTUALENV_PYTHON` are removed (reference "Environment cleared at start");
     - `PYENV_VERSION` is set to the base.
3. **Links (Linux):**
   - `versions/<name>` is an absolute symlink to `versions/<base>/envs/<name>`, made under a unique temp name and renamed into place.
   - An existing symlink is replaced. Anything else is never replaced.
   - The `python*-config` links and `bin/pydoc` are as upstream.
4. **Junctions (Windows):** `rpyenv_core::junction::create(link, target)` uses `FSCTL_SET_REPARSE_POINT` through `windows-sys` (feature `Win32_System_IO`), and is removed with `remove_dir` only.
   - Its message on a volume without reparse points: `pyenv-virtualenv: cannot link <link> to <target>: <os error>`.
5. **Uninstall cascade:**
   - **Linux follows the v1.4.0 hook table** (reference "uninstall/envs.bash"):
     - core's prompt first, then one `pyenv-virtualenv: remove <env>? (y/N) ` per env (any answer starting `y`/`Y` accepts);
     - `-f` skips all of them;
     - a refused env stops the whole uninstall with exit 1, removing nothing.
   - **Windows:** pyenv-win's own `uninstall` never prompts, so only the env prompts appear, and only without `-f` (spec §10). Junctions into the base's `envs` are removed with the base.
6. **Shell code:**
   - **Linux POSIX and fish output** is byte-identical to v1.4.0 (`parity/diff.py` cases with the plugin installed on the upstream side).
   - **pwsh (both OSes):**
     - `$Env:X = <ps_literal>`, and `Remove-Item Env:X -ErrorAction SilentlyContinue`;
     - the prompt is wrapped as `Activate.ps1` does (save `function:prompt` to `function:_pyenv_old_prompt`, then define `prompt` to print `(<venv>) ` and call the saved one);
     - deactivate restores it.
   - **cmd (Windows, without integration):** `set "X=v"` lines, `PROMPT=(<venv>) $P$G`, then the D-89-style message and exit 1.
   - **Git Bash:** POSIX code with `VIRTUAL_ENV` in MSYS form (`/c/…`, `init_win::msys`), as venv's own bash `activate` does.

## File Structure

| File | Responsibility |
|---|---|
| `crates/rpyenv-core/src/venv.rs` (new) | `pyvenv.cfg`, `base_prefix` (virtualenv-prefix semantics), `env_dir`, `fallback` (D-95), name checks shared by both flavors |
| `crates/rpyenv-core/src/junction.rs` (new, Windows) | create and read directory junctions |
| `crates/rpyenv-core/src/lookup.rs` | call `venv::fallback` per selected version (Linux), and the system-site-packages fallback in `which_win_with` |
| `crates/pyenv/src/commands/venv.rs` (new) | `virtualenv` (create), `virtualenvs`, `virtualenv-prefix`, `virtualenv-delete`, `delete_related` (uninstall cascade) |
| `crates/pyenv/src/commands/activate.rs` (new) | `sh-activate`, `sh-deactivate`, `activate`/`deactivate` without integration, `virtualenv-init` |
| `crates/pyenv/src/commands/mod.rs` | table rows for both flavors |
| `crates/pyenv/src/help.rs` | topics (Linux texts verbatim from v1.4.0) |
| `crates/pyenv/src/commands/completions.rs` | entries and two new tails |
| `crates/pyenv/src/commands/uninstall.rs`, `uninstall_win.rs` | the cascade |
| `crates/pyenv/src/commands/versions.rs` | Windows env lines |
| `crates/pyenv/src/commands/init_win.rs` | route `activate`/`deactivate` in `PWSH_FUNCTION` |
| `crates/pyenv/tests/cli_venv.rs` (new, unix), `cli_venv_win.rs` (new, Windows), `common/venv.rs` (new) | CLI tests and the fake-base helpers |
| `parity/bats_venv_run.sh` (new), `parity/bats_check.py`, `parity/expected/bats-virtualenv.txt` (new), `parity/expected/bats-virtualenv-files.txt` (new) | the third suite |
| `parity/diff.py`, `parity/diff_cases.py` | `--pyenv-virtualenv DIR`, Case fields `plugin` and `links` |
| `parity/allowlist.py`, `docs/parity/allowlist.md` | milestones `M4a` and `M4b`; `DELIVERED += M4a, M4b`; rows D-95 to D-103; D-51 and D-52 widened |
| `.github/workflows/parity.yml` | fetch the plugin by commit; the new bats step; `--pyenv-virtualenv` for `diff.py` |
| `docs/specs/2026-09-27-rpyenv-design.md` | §10 (Windows details, intended fallbacks), §12 item 3 (the third suite) |

---
### Task 1: The third bats suite, `diff.py` with the plugin, and the milestone tags

The harness lands first, before any command (harness doc §1.3 point 7). Every test then fails except three that pass by accident, and the expected list cites `M4b` for the rest. Each later task removes the rows its command fixes.

**Files:**
- Create (scratch, not committed): `C:\tmp\m4b_wsl.sh`, `C:\tmp\m4b_venv_bats_root.sh`, `C:\tmp\m4b_venv_bats_check.sh`
- Modify: `parity/allowlist.py:12-15`, `parity/bats_check.py`, `parity/diff.py`, `parity/diff_cases.py:9-19`, `.github/workflows/parity.yml`, `docs/specs/2026-09-27-rpyenv-design.md` §12 item 3
- Create: `parity/bats_venv_run.sh`, `parity/expected/bats-virtualenv.txt`, `parity/expected/bats-virtualenv-files.txt`
- Test: `parity/test_allowlist.py`, `parity/test_bats_check.py`, `parity/test_diff.py`

**Interfaces:**
- Produces:
  - `bats_check.py --suite virtualenv <tap>`;
  - `bats_check.SUITES: dict[str, tuple[str, str, str]]` (expected file, files list, summary heading);
  - `diff_cases.Case` fields `plugin: bool = False` and `links: tuple = ()`, where `links` holds `(path, target)` pairs, both `root/…`/`work/…` places;
  - `diff.py --pyenv-virtualenv DIR`;
  - the `M4b` reason tag.

- [ ] **Step 1: Scratch runners.** Write each with the Write tool, with LF line ends.

`C:\tmp\m4b_wsl.sh`:
```bash
#!/usr/bin/env bash
# Fetch the m4b-virtualenvs branch into ~/rpyenv-linux and run the command given as arguments.
set -u
source /home/jm/.cargo/env
R=/home/jm/rpyenv-linux
git -C "$R" fetch -q /mnt/c/Users/JM/repos/OWN/rpyenv m4b-virtualenvs
git -C "$R" reset -q --hard FETCH_HEAD
cd "$R"
git log --oneline -1
"$@"
```

`C:\tmp\m4b_venv_bats_root.sh` (run as root):
```bash
#!/usr/bin/env bash
# pyenv-virtualenv's bats suite against ~/rpyenv-linux's debug binaries.
set -u
bash /home/jm/rpyenv-linux/parity/bats_venv_run.sh /home/jm/rpyenv-linux/target/debug \
  /home/jm/m4b-upstream/pyenv-virtualenv-1.4.0 \
  /home/jm/m1cb-upstream/bats-core-1.11.1 /home/jm/m4b-venv-tap.txt
echo "bats_venv_run exit=$?"
chown jm:jm /home/jm/m4b-venv-tap.txt
```

`C:\tmp\m4b_venv_bats_check.sh`:
```bash
#!/usr/bin/env bash
cd /home/jm/rpyenv-linux
python3 parity/bats_check.py --suite virtualenv /home/jm/m4b-venv-tap.txt
echo "exit=$?"
```

Run each as `wsl -d Debian [-u root] --exec /usr/bin/bash /mnt/c/tmp/<name>.sh [args]`.

- [ ] **Step 2: The failing parity tests.** Append to `parity/test_allowlist.py`:

```python
class M4Milestones(unittest.TestCase):
    def test_m4b_is_a_reason_tag_and_m4a_is_delivered(self):
        table = allowlist.rows()
        self.assertIsNone(allowlist.check_reason("M4b not built yet", "Linux", table))
        self.assertIn("M4a", allowlist.DELIVERED)
```

Append to `parity/test_bats_check.py`:

```python
class Suites(unittest.TestCase):
    def test_virtualenv_suite_has_its_own_lists(self):
        exp, files, heading = bats_check.SUITES["virtualenv"]
        self.assertEqual((exp, files), ("bats-virtualenv.txt", "bats-virtualenv-files.txt"))
        self.assertIn("pyenv-virtualenv", heading)
        self.assertEqual(bats_check.SUITES["pyenv"][:2], ("bats.txt", "bats-files.txt"))
```

Append to `parity/test_diff.py`:

```python
class PluginAndLinks(unittest.TestCase):
    @unittest.skipIf(os.name == "nt", "Linux cases")
    def test_links_and_the_plugin_go_into_the_fixture(self):
        with tempfile.TemporaryDirectory() as t:
            plugin = os.path.join(t, "plugin")
            os.makedirs(plugin)
            old = diff.BASE
            diff.BASE = os.path.join(t, "base")
            try:
                case = diff_cases.Case(
                    "x", (), os="Linux", plugin=True,
                    files=(("root/versions/{v0}/envs/e/bin/python", "#!/bin/sh\n"),),
                    links=(("root/versions/e", "root/versions/{v0}/envs/e"),))
                places = diff.build("unused", case, plugin_dir=plugin)
                link = os.path.join(places["root"], "versions", "e")
                self.assertEqual(os.readlink(link),
                                 os.path.join(places["root"], "versions", diff.VERSIONS[0], "envs", "e"))
                self.assertEqual(os.readlink(os.path.join(places["root"], "plugins", "pyenv-virtualenv")), plugin)
                places = diff.build("unused", case, plugin_dir=None)
                self.assertFalse(os.path.lexists(os.path.join(places["root"], "plugins", "pyenv-virtualenv")))
            finally:
                diff.BASE = old
```

These tests use `tempfile` and `diff_cases`. Add `import tempfile` and `import diff_cases` at the top if they're missing.

- [ ] **Step 3: Run the tests to see them fail.**

Run: `python -m unittest discover -s parity -p "test_*.py"`
Expected: 3 failures, from `M4b` (unknown tag), `SUITES` (no attribute), and `build()` (unexpected keyword `plugin_dir`).

- [ ] **Step 4: Implement.**

`parity/allowlist.py`:
```python
MILESTONES = ("M2a", "M2b", "M2", "M3", "M4", "M4a", "M4b", "M5", "M6", "M7", "M8", "M9")
DELIVERED = ("M1", "M2a", "M2b", "M3", "M4a")
```

`parity/bats_check.py`:
- Add after `HERE`:

```python
# --suite NAME: (expected-failure list, files list, summary heading), all under parity/expected/.
SUITES = {
    "pyenv": ("bats.txt", "bats-files.txt", "Upstream pyenv bats suite"),
    "virtualenv": ("bats-virtualenv.txt", "bats-virtualenv-files.txt",
                   "Upstream pyenv-virtualenv bats suite"),
}
```

- In `structure_problems`, take the list's name as a parameter: `def structure_problems(text, files, files_name="bats-files.txt"):`, and use `{files_name}` in the "not listed in" message.
- Replace the start of `main`:

```python
def main(argv):
    suite = "pyenv"
    if argv[:1] == ["--suite"]:
        suite, argv = argv[1], argv[2:]
    exp_name, files_name, heading = SUITES[suite]
    with open(argv[0], encoding="utf-8") as f:
        text = f.read()
    results = parse_tap(text)
    expected = allowlist.read_expected(os.path.join(HERE, "expected", exp_name), 2)
    found = structure_problems(text, read_files(os.path.join(HERE, "expected", files_name)), files_name)
```

  Then use `heading` in place of the literal `"Upstream pyenv bats suite"` in the summary. Make the printed line start with `bats ({suite}):` only when `suite != "pyenv"`, so the existing core log format is unchanged.

`parity/diff_cases.py`, in `Case`, after `executable`:
```python
    plugin: bool = False  # Linux: upstream runs with pyenv-virtualenv in root/plugins (diff.py --pyenv-virtualenv)
    links: tuple = ()  # (path, target) symlinks made after the files; both are root/… or work/… places, {v0}/{v1} expand
```

`parity/diff.py`:
- Change `def build(upstream, case):` to `def build(upstream, case, plugin_dir=None):`.
- Before `return places`, add:

```python
    for rel, target in case.links:
        os.symlink(resolve(places, expand(target)), resolve(places, expand(rel)))
    if case.plugin and plugin_dir:
        os.makedirs(os.path.join(root, "plugins"), exist_ok=True)
        os.symlink(plugin_dir, os.path.join(root, "plugins", "pyenv-virtualenv"))
```

- In `main`, add `p.add_argument("--pyenv-virtualenv")`, then build per tool:

```python
        if case.plugin and not a.pyenv_virtualenv:
            failures.append(f"{case.name}: needs --pyenv-virtualenv")
            continue
        results = {}
        for tool in ("upstream", "rpyenv"):
            plugin = os.path.abspath(a.pyenv_virtualenv) if tool == "upstream" and case.plugin else None
            places = build(upstream, case, plugin_dir=plugin)
            results[tool] = run(tool, rpyenv, upstream, places, case)
```

  Check that `resolve` already expands `{v0}`; if it doesn't, `expand` before `resolve` as shown.

`parity/bats_venv_run.sh` (new, mode 755):
```bash
#!/usr/bin/env bash
# Runs pyenv-virtualenv's bats suite against rpyenv's built-in virtualenv commands (spec §12.3,
# plan M4b Task 1; harness doc §1.3). Run as root, in a Debian container on CI or a Debian host:
#   bash parity/bats_venv_run.sh <rpyenv-bin-dir> <pyenv-virtualenv-src> <bats-dir> <tap-out>
# The suite calls `pyenv-<cmd>` names on a PATH it builds from `<test>/../bin`, so rpyenv and its
# multicall links take the place of the plugin's own bin/. Each file runs as `tester`, against
# root-owned binaries. installer.bats is left out: it tests the plugin's install.sh, which
# rpyenv doesn't have.
set -euo pipefail
for a in "$1" "$2" "$3"; do
  [ -e "$a" ] || { echo "bats_venv_run.sh: no such file or directory: $a" >&2; exit 2; }
done
bin=$(realpath "$1") up=$(realpath "$2") bats=$(realpath "$3") out=$(realpath -m "$4")
work=$(mktemp -d /tmp/rpyenv-bats-venv.XXXXXX)
chmod 755 "$work"
install -d -m 755 "$work/run" "$work/run/bin"
install -m 755 "$bin/pyenv" "$bin/pyenv-shim" "$work/run/bin/"
for c in activate deactivate sh-activate sh-deactivate virtualenv virtualenv-delete \
         virtualenv-init virtualenv-prefix virtualenvs; do
  ln -s pyenv "$work/run/bin/pyenv-$c"
done
cp -r "$up/test" "$work/run/test"
rm -f "$work/run/test/installer.bats"
# The lenient overlay (user decision 2026-10-05; harness doc §1.3 point 2): rpyenv answers these
# in-process (allowlist D-52), so a test's stub of one is never run, and unstub only removes it.
# Every other unstub verifies as upstream's does.
cat >> "$work/run/test/test_helper.bash" <<'OVERLAY'
# --- rpyenv overlay, appended by parity/bats_venv_run.sh ---
_RPY_INPROC=" pyenv-version-name pyenv-version-origin pyenv-prefix pyenv-hooks pyenv-rehash pyenv-exec pyenv-which pyenv-whence pyenv-latest python-build curl pyenv-virtualenv-prefix pyenv-sh-deactivate "
eval "_rpy_up_unstub() $(declare -f unstub | tail -n +2)"
unstub() {
  if [[ $_RPY_INPROC == *" $1 "* ]]; then
    rm -f "$TMP/bin/$1" "$TMP/$1-stub-plan" "$TMP/$1-stub-run" "$TMP/$1-stub-log"
    return 0
  fi
  _rpy_up_unstub "$@"
}
OVERLAY
id tester >/dev/null 2>&1 || useradd --create-home tester
chown -R tester "$work/run/test"
: > "$out"
cd "$work/run/test"
for f in $(ls -- *.bats); do
  runuser -u tester -- "$bats/bin/bats" --tap "./$f" 2>&1 | sed "s|^|$f\t|" >> "$out" || true
done
echo "bats: $(grep -c -P '\tok ' "$out") ok, $(grep -c -P '\tnot ok ' "$out") not ok"
```

`parity/expected/bats-virtualenv-files.txt`:
```
# pyenv-virtualenv v1.4.0's test files that parity/bats_venv_run.sh runs (installer.bats is left out).
activate.bats
conda-activate.bats
conda-deactivate.bats
conda-prefix.bats
conda.bats
deactivate.bats
delete.bats
envs.bats
hooks.bats
init.bats
pip.bats
prefix.bats
python.bats
pyvenv.bats
version.bats
virtualenv.bats
virtualenvs.bats
```

- [ ] **Step 5: Run the parity tests.**

Run: `python -m unittest discover -s parity -p "test_*.py"`
Expected: `OK`.

- [ ] **Step 6: Commit, then record the baseline.**
  1. Commit: `git commit -m "Add pyenv-virtualenv's bats suite as a third upstream suite, and the plugin to diff.py"`.
  2. Run `m4b_wsl.sh cargo build -q --workspace`, then `m4b_venv_bats_root.sh` as root.
  3. Write `parity/expected/bats-virtualenv.txt`: a header comment, then one line per failing test, `<file> | <test> | M4b not built yet`. Generate it from the TAP log with a short Python script. Only the tests that pass by accident stay out; the harness doc names `activate.bats` 21, `deactivate.bats` 12 and `init.bats` 2.

  Expected from `m4b_venv_bats_check.sh`: `bats (virtualenv): 108 tests, 105 failing, 105 expected to fail, 0 problems`.

  If the counts differ, the TAP log is the truth. Record the actual counts in the ledger.

- [ ] **Step 7: CI and spec.**

In `.github/workflows/parity.yml`, job `linux`:
- add `PYENV_VIRTUALENV_SHA: eda64556af9b2992386deeb75dad2130899fc4c9` next to `PYENV_SHA`;
- in the fetch step, add:

```bash
curl -fsSL "https://github.com/pyenv/pyenv-virtualenv/archive/${PYENV_VIRTUALENV_SHA}.tar.gz" | tar -xz -C upstream
mv "upstream/pyenv-virtualenv-${PYENV_VIRTUALENV_SHA}" upstream/pyenv-virtualenv
```

- add `--pyenv-virtualenv upstream/pyenv-virtualenv` to the Linux `diff.py` invocation;
- after the pyenv bats step, add:

```yaml
      - name: Upstream pyenv-virtualenv bats suite in debian:trixie-slim (spec §12.3)
        if: ${{ !cancelled() }}
        run: |
          docker run --rm -v "$PWD:/w" -w /w debian:trixie-slim \
            bash parity/bats_venv_run.sh /w/target/debug /w/upstream/pyenv-virtualenv /w/upstream/bats /w/bats-venv-tap.txt
          python3 parity/bats_check.py --suite virtualenv bats-venv-tap.txt
```

  Match the paths the existing pyenv bats step uses for `upstream/bats` and `target/debug`.

In spec §12 item 3, add a third bullet after the pyenv-win suite:

> pyenv-virtualenv v1.4.0's bats suite (commit `eda6455`), on Linux, against rpyenv's built-in commands through `pyenv-<cmd>` links. An overlay skips the "stub was called" check for commands rpyenv answers in-process (allowlist D-52). It has its own expected-failure list, `parity/expected/bats-virtualenv.txt`. `installer.bats` is left out.

Commit: `git commit -m "Run pyenv-virtualenv's bats suite in CI and record its baseline"`.

---

### Task 2: `rpyenv_core::venv`: `pyvenv.cfg`, an env's base prefix, and the `which` fallbacks

**Files:**
- Create: `crates/rpyenv-core/src/venv.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (add `pub mod venv;`), `crates/rpyenv-core/src/lookup.rs:96-130` (the Linux loop) and `:269-293` (`which_win_with`)
- Test: unit tests in `venv.rs` and `lookup.rs`

**Interfaces:**
- Produces:
  - `venv::Cfg { home: Option<PathBuf>, system_site_packages: bool }`
  - `venv::read_cfg(env_dir: &Path, flavor: Flavor) -> Option<Cfg>`
  - `venv::version_dir(ctx: &Ctx, name: &str) -> PathBuf`, which turns `/` into `\` on Windows
  - `venv::VenvError { NotVenv(String), NoPython(String), Prefix(String) }`, with `.message() -> String`
  - `venv::base_prefix(ctx: &Ctx, name: &str) -> Result<PathBuf, VenvError>`
  - `venv::is_conda(dir: &Path) -> bool`
  - `venv::fallback(ctx: &Ctx, version: &str, dir: &Path, command: &str) -> Option<PathBuf>` (Linux)

- [ ] **Step 1: Write the failing unit tests** at the bottom of `venv.rs`. Create the file with just `#[cfg(test)] mod tests { … }` and the `use`s, so it compiles to RED.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root() -> (tempfile::TempDir, Ctx) {
        let t = tempfile::tempdir().unwrap();
        let ctx = Ctx::for_test(Flavor::Pyenv, t.path(), t.path());
        (t, ctx)
    }

    #[cfg(unix)]
    fn exe(p: &Path) {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "#!/bin/sh\n").unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// `versions/<base>/envs/<name>`, a venv of `versions/<base>` (reference "virtualenv-prefix").
    #[cfg(unix)]
    fn env(ctx: &Ctx, base: &str, name: &str, cfg: &str) -> PathBuf {
        let b = ctx.versions_dir().join(base);
        exe(&b.join("bin/python"));
        let e = b.join("envs").join(name);
        exe(&e.join("bin/python"));
        fs::write(e.join("bin/activate"), "").unwrap();
        fs::write(e.join("pyvenv.cfg"), cfg.replace("{base}", &b.display().to_string())).unwrap();
        std::os::unix::fs::symlink(&e, ctx.versions_dir().join(name)).unwrap();
        e
    }

    #[test]
    fn cfg_home_loses_one_bin_on_linux_and_reads_the_flag_in_any_case() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("pyvenv.cfg"), "  home = /opt/py/bin\nInclude-System-Site-Packages = TRUE\n").unwrap();
        assert_eq!(
            read_cfg(t.path(), Flavor::Pyenv),
            Some(Cfg { home: Some(PathBuf::from("/opt/py")), system_site_packages: true })
        );
        fs::write(t.path().join("pyvenv.cfg"), "home = C:\\py\\3.13\\\r\ninclude-system-site-packages = false\r\n").unwrap();
        assert_eq!(
            read_cfg(t.path(), Flavor::PyenvWin),
            Some(Cfg { home: Some(PathBuf::from("C:\\py\\3.13")), system_site_packages: false })
        );
    }

    #[cfg(unix)]
    #[test]
    fn base_prefix_as_virtualenv_prefix() {
        let (_t, ctx) = root();
        env(&ctx, "3.12.1", "venv1", "home = {base}/bin\n");
        let base = ctx.versions_dir().join("3.12.1");
        assert_eq!(base_prefix(&ctx, "venv1"), Ok(base.clone()));
        assert_eq!(base_prefix(&ctx, "3.12.1/envs/venv1"), Ok(base));
        assert_eq!(base_prefix(&ctx, "system").unwrap_err().message(),
                   "pyenv-virtualenv: version `system' is not a virtualenv");
        assert_eq!(base_prefix(&ctx, "3.12.1").unwrap_err().message(),
                   "pyenv-virtualenv: version `3.12.1' is not a virtualenv");
        fs::create_dir_all(ctx.versions_dir().join("nopy")).unwrap();
        assert_eq!(base_prefix(&ctx, "nopy").unwrap_err().message(),
                   "pyenv-virtualenv: `python' not found in version `nopy'");
        assert_eq!(base_prefix(&ctx, "nosuch").unwrap_err().message(),
                   "pyenv: version `nosuch' not installed");
    }

    #[cfg(unix)]
    #[test]
    fn conda_base_conda_env_and_old_virtualenv() {
        let (_t, ctx) = root();
        let conda = ctx.versions_dir().join("miniconda3-4.7.12");
        exe(&conda.join("bin/python"));
        exe(&conda.join("bin/conda"));
        fs::write(conda.join("bin/activate"), "").unwrap();
        let cenv = conda.join("envs/cenv");
        exe(&cenv.join("bin/python"));
        fs::create_dir_all(cenv.join("conda-meta")).unwrap();
        assert_eq!(base_prefix(&ctx, "miniconda3-4.7.12"), Ok(conda.clone()));
        assert_eq!(base_prefix(&ctx, "miniconda3-4.7.12/envs/cenv"),
                   Ok(fs::canonicalize(&conda).unwrap()));
        let old = ctx.versions_dir().join("oldvenv");
        exe(&old.join("bin/python"));
        fs::write(old.join("bin/activate"), "").unwrap();
        fs::create_dir_all(old.join("lib/python2.7")).unwrap();
        fs::write(old.join("lib/python2.7/orig-prefix.txt"), conda.display().to_string()).unwrap();
        assert_eq!(base_prefix(&ctx, "oldvenv"), Ok(conda));
    }

    /// allowlist D-95: the fallbacks give a file, never a folder.
    #[cfg(unix)]
    #[test]
    fn fallbacks_follow_spec_10() {
        let (_t, ctx) = root();
        let base = ctx.versions_dir().join("3.12.1");
        let ssp = env(&ctx, "3.12.1", "ssp", "home = {base}/bin\ninclude-system-site-packages = true\n");
        let plain = env(&ctx, "3.12.1", "plain", "home = {base}/bin\n");
        exe(&base.join("bin/basetool"));
        exe(&base.join("bin/python3.12-config"));
        fs::create_dir_all(base.join("bin/adir")).unwrap();
        assert_eq!(fallback(&ctx, "ssp", &ssp, "basetool"), Some(base.join("bin/basetool")));
        assert_eq!(fallback(&ctx, "plain", &plain, "basetool"), None);
        assert_eq!(fallback(&ctx, "plain", &plain, "python3.12-config"),
                   Some(base.join("bin/python3.12-config")));
        assert_eq!(fallback(&ctx, "ssp", &ssp, "adir"), None);
        assert_eq!(fallback(&ctx, "ssp", &ssp, "nosuch"), None);
    }
}
```

- [ ] **Step 2: Run them to see them fail.**

Run (Windows, compiles the non-unix test): `cargo test -q -p rpyenv-core --lib venv::`. Then in WSL: `m4b_wsl.sh cargo test -q -p rpyenv-core --lib venv::`.
Expected: compile errors, because `read_cfg`, `base_prefix`, `fallback` and `Cfg` don't exist yet.

- [ ] **Step 3: Implement `venv.rs`**, above the tests:

```rust
//! Virtualenv facts shared by `which`, the virtualenv commands, `uninstall` and `sh-activate`
//! (spec §10; docs/parity/pyenv-virtualenv-m4-reference.md "virtualenv-prefix" and
//! "Hooks shipped in etc/pyenv.d").

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{pathsearch, prefix};
use std::path::{Path, PathBuf};

/// What an env's `pyvenv.cfg` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cfg {
    /// `home`: on Linux minus one trailing `/bin` (the base prefix); on Windows the base
    /// folder itself, minus a trailing `\`.
    pub home: Option<PathBuf>,
    /// `include-system-site-packages = true`, in any case (`grep -q -i`).
    pub system_site_packages: bool,
}

/// `key *= *value` at the start of `line`, leading spaces allowed (`sed -n '/^ *home *= */s///p'`).
fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.trim_start_matches(' ');
    let rest = rest.get(..key.len()).filter(|k| k.eq_ignore_ascii_case(key)).map(|_| &rest[key.len()..])?;
    let rest = rest.trim_start_matches(' ').strip_prefix('=')?;
    Some(rest.trim_start_matches(' ').trim_end_matches(['\r', ' ']))
}

pub fn read_cfg(env_dir: &Path, flavor: Flavor) -> Option<Cfg> {
    let bytes = std::fs::read(env_dir.join("pyvenv.cfg")).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut home = None;
    let mut ssp = false;
    for line in text.lines() {
        if home.is_none() {
            if let Some(h) = value_of(line, "home") {
                let h = match flavor {
                    Flavor::Pyenv => h.strip_suffix("/bin").unwrap_or(h),
                    Flavor::PyenvWin => h.trim_end_matches('\\'),
                };
                home = Some(PathBuf::from(h));
            }
        }
        if value_of(line, "include-system-site-packages").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
            ssp = true;
        }
    }
    Some(Cfg { home, system_site_packages: ssp })
}

/// `versions/<name>`; on Windows `<base>/envs/<name>` is accepted with either separator
/// (allowlist D-101).
pub fn version_dir(ctx: &Ctx, name: &str) -> PathBuf {
    match ctx.flavor {
        Flavor::Pyenv => ctx.versions_dir().join(name),
        Flavor::PyenvWin => ctx.versions_dir().join(name.replace('/', "\\")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VenvError {
    NotVenv(String),
    NoPython(String),
    /// Core's `pyenv-prefix` message for a version that isn't installed.
    Prefix(String),
}

impl VenvError {
    pub fn message(&self) -> String {
        match self {
            VenvError::NotVenv(v) => format!("pyenv-virtualenv: version `{v}' is not a virtualenv"),
            VenvError::NoPython(v) => format!("pyenv-virtualenv: `python' not found in version `{v}'"),
            VenvError::Prefix(m) => m.clone(),
        }
    }
}

/// A conda install or env: `conda-meta/`, or an executable `bin/conda`.
pub fn is_conda(dir: &Path) -> bool {
    dir.join("conda-meta").is_dir() || pathsearch::is_runnable(&dir.join("bin").join("conda"))
}

/// The libdir an old virtualenv keeps `orig-prefix.txt` in: `Lib` (jython), `lib-python`
/// (pypy) or `lib`.
fn libdir(dir: &Path) -> Option<PathBuf> {
    ["Lib", "lib-python", "lib"].iter().map(|l| dir.join(l)).find(|p| p.is_dir())
}

/// The first `name` within `<libdir>/` to depth 2 (`find <libdir>/ -maxdepth 2`).
fn within(dir: &Path, name: &str) -> Option<PathBuf> {
    let lib = libdir(dir)?;
    if lib.join(name).is_file() {
        return Some(lib.join(name));
    }
    let mut subs: Vec<PathBuf> = std::fs::read_dir(&lib).ok()?.flatten().map(|e| e.path()).collect();
    subs.sort();
    subs.into_iter().map(|s| s.join(name)).find(|p| p.is_file())
}

fn orig_prefix(dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(within(dir, "orig-prefix.txt")?).ok()?;
    Some(PathBuf::from(text.trim_end_matches(['\n', '\r'])))
}

/// `pyenv virtualenv-prefix <name>` for one version (reference "virtualenv-prefix", steps 1-6).
pub fn base_prefix(ctx: &Ctx, name: &str) -> Result<PathBuf, VenvError> {
    let not_venv = || VenvError::NotVenv(name.to_string());
    if name == "system" {
        return Err(not_venv());
    }
    let mut p = version_dir(ctx, name);
    if !p.is_dir() {
        p = prefix::prefix_of(ctx, name).map_err(|e| VenvError::Prefix(e.message()))?;
    }
    let found = match ctx.flavor {
        Flavor::Pyenv => {
            if !pathsearch::is_runnable(&p.join("bin").join("python")) {
                return Err(VenvError::NoPython(name.to_string()));
            }
            if p.join("bin").join("activate").is_file() {
                if p.join("bin").join("conda").is_file() {
                    Some(p.clone())
                } else if let Some(cfg) = read_cfg(&version_dir(ctx, name), Flavor::Pyenv) {
                    cfg.home
                } else {
                    orig_prefix(&p)
                }
            } else if p.join("conda-meta").is_dir() {
                std::fs::canonicalize(p.join("..").join("..")).ok()
            } else {
                None
            }
        }
        Flavor::PyenvWin => {
            if !(p.join("python.exe").is_file() || p.join("Scripts").join("python.exe").is_file()) {
                return Err(VenvError::NoPython(name.to_string()));
            }
            read_cfg(&p, Flavor::PyenvWin).and_then(|c| c.home)
        }
    };
    found.filter(|d| d.is_dir()).ok_or_else(not_venv)
}

/// The command fallbacks of spec §10 for one selected Linux version whose own `bin` lacks
/// `command` (allowlist D-95: always a file, never a folder):
/// - `conda` in a conda env: the owning install's `bin/conda`;
/// - in a venv or an old virtualenv (not a conda one), the base's `bin/<command>` when the
///   env includes system site packages, or when `command` is `python*-config`.
pub fn fallback(ctx: &Ctx, version: &str, dir: &Path, command: &str) -> Option<PathBuf> {
    let file = |p: PathBuf| (p.is_file() && pathsearch::is_runnable(&p)).then_some(p);
    if command == "conda" && dir.join("conda-meta").is_dir() {
        return base_prefix(ctx, version).ok().and_then(|b| file(b.join("bin").join("conda")));
    }
    if !dir.join("bin").join("activate").is_file() || dir.join("bin").join("conda").is_file() {
        return None;
    }
    let (base, ssp) = match read_cfg(dir, Flavor::Pyenv) {
        Some(c) => (c.home?, c.system_site_packages),
        None => (orig_prefix(dir)?, within(dir, "no-global-site-packages.txt").is_none()),
    };
    let config = command.starts_with("python") && command.ends_with("-config");
    (ssp || config).then(|| file(base.join("bin").join(command))).flatten()
}
```

`value_of` slices by byte. Guard the slice with `.get(..)`, as above, so a multi-byte character at that position returns `None` rather than panicking.

- [ ] **Step 4: Wire the fallbacks into `lookup`.**

In `which_pyenv`'s loop, after the `candidate` check fails:
```rust
                if let Some(path) = crate::venv::fallback(ctx, v, &dir, command) {
                    return Ok(Found { path, warnings });
                }
```

Make the binding `Ok(dir)`; that's already the name used. Add a lookup unit test next to the existing `which` tests:

```rust
    /// allowlist D-95: through `which` (so through shims and `exec`), an env with system site
    /// packages finds a base command before `system` is tried.
    #[cfg(unix)]
    #[test]
    fn an_env_with_system_site_packages_finds_the_base_command() {
        let r = Root::new(&["3.12.1"]);
        let base = r.ctx.versions_dir().join("3.12.1");
        let tool = r.bin("3.12.1", "basetool");
        let env = base.join("envs/ssp");
        std::fs::create_dir_all(env.join("bin")).unwrap();
        std::fs::write(env.join("bin/activate"), "").unwrap();
        std::fs::write(env.join("pyvenv.cfg"),
            format!("home = {}/bin\ninclude-system-site-packages = true\n", base.display())).unwrap();
        let mut ctx = r.ctx.clone();
        ctx.pyenv_version = Some("3.12.1/envs/ssp".into());
        assert_eq!(which_pyenv(&ctx, "basetool", false, &Skip::default()).unwrap().path, tool);
    }
```

Adapt the names to the test module's `Root` helper: read how `Root::new`, `r.bin` and `r.ctx` are spelled there, and use those.

In `which_win_with`, after `hits` gives nothing for a selected version:
```rust
        // allowlist D-101: only the system-site-packages fallback applies on Windows.
        if let Some(home) = crate::venv::read_cfg(&dir, Flavor::PyenvWin)
            .filter(|c| c.system_site_packages)
            .and_then(|c| c.home)
        {
            if let Some(path) = win_hits(&home, program, &exts).into_iter().find(|h| !runnable_only || is_runnable_win(h)) {
                return Ok(Found { path, warnings: Vec::new() });
            }
        }
```

- [ ] **Step 5: Run the tests.**

Run: `cargo test -q -p rpyenv-core` (Windows), then `m4b_wsl.sh cargo test -q -p rpyenv-core` (Linux).
Expected: all pass, the new ones included.

- [ ] **Step 6: Commit**: `git commit -m "Add the shared virtualenv facts and make the which fallbacks work"`. Ledger D-95 as "added in Task 10".

---
### Task 3: `pyenv virtualenv`: create an env

**Files:**
- Create: `crates/pyenv/src/commands/venv.rs`, `crates/pyenv/tests/cli_venv.rs` (`#![cfg(unix)]`)
- Modify:
  - `crates/pyenv/src/commands/mod.rs`: `pub mod venv;`, plus a `("virtualenv", venv::virtualenv)` row in `COMMANDS` (both flavors);
  - `crates/pyenv/src/help.rs`: the `virtualenv` topic in `PYENV` and the Windows topic table;
  - `crates/pyenv/src/commands/completions.rs`: a new `Tail::VersionsBareSkipEnvs`, and `e("virtualenv", true, Words(&[], VersionsBareSkipEnvs))` in both tables.

**Interfaces:**
- Consumes: `venv::{base_prefix, version_dir}` (Task 2), `lookup::{which_pyenv, which_win_runnable, whence_pyenv, whence_win}`, `crate::commands::latest::latest`, `crate::commands::rehash::rehash`, `crate::install::{prompt, Reply}`.
- Produces:
  - `commands::venv::virtualenv(ctx: &Ctx, args: &[&str]) -> Output`;
  - `pub const HELP: &str`;
  - `fn link_env(ctx: &Ctx, env_dir: &Path, link: &Path) -> std::io::Result<()>`; Task 7 adds its Windows branch;
  - `fn bin_name(ctx: &Ctx) -> &'static str`: `"bin"` or `"Scripts"`.

- [ ] **Step 1: Write the failing tests.** `crates/pyenv/tests/cli_venv.rs`:

```rust
//! Built-in virtualenvs on Linux (spec §10; docs/parity/pyenv-virtualenv-m4-reference.md).
#![cfg(unix)]

mod common;
use common::Fixture;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A stand-in base Python: `-m venv [opts] DIR` (fails with FAKE_VENV_FAIL, skips pip with
/// FAKE_NO_PIP or --without-pip), `-m venv --help`, and `-s -m ensurepip`. Calls are logged
/// to `<prefix>/calls.log`, where `<prefix>` is the folder above the script's `bin`.
pub const FAKE_PYTHON: &str = r#"#!/bin/sh
here=$(cd "$(dirname "$0")/.." && pwd)
if [ "$1" = "-m" ] && [ "$2" = "venv" ]; then
  shift 2
  [ "$1" = "--help" ] && exit 0
  echo "venv $*" >> "$here/calls.log"
  if [ -n "$FAKE_VENV_FAIL" ]; then echo "venv failed"; exit 5; fi
  for a in "$@"; do dir=$a; done
  ssp=false
  for a in "$@"; do [ "$a" = "--system-site-packages" ] && ssp=true; done
  mkdir -p "$dir/bin"
  printf 'home = %s/bin\ninclude-system-site-packages = %s\n' "$here" "$ssp" > "$dir/pyvenv.cfg"
  ln -sf "$here/bin/python" "$dir/bin/python"
  : > "$dir/bin/activate"
  case " $* " in *" --without-pip "*) ;; *) [ -z "$FAKE_NO_PIP" ] && { printf '#!/bin/sh\n' > "$dir/bin/pip"; chmod 755 "$dir/bin/pip"; } ;; esac
  exit 0
fi
if [ "$1" = "-s" ] && [ "$2" = "-m" ] && [ "$3" = "ensurepip" ]; then
  echo "ensurepip" >> "$here/calls.log"
  printf '#!/bin/sh\n' > "$(dirname "$0")/pip"
  chmod 755 "$(dirname "$0")/pip"
  exit 0
fi
exit 3
"#;

fn exe(f: &Fixture, p: &Path, body: &str) {
    f.file(p, body);
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// `versions/<v>` with the fake `bin/python`.
fn fake_base(f: &Fixture, v: &str) -> PathBuf {
    let b = f.root.join("versions").join(v);
    exe(f, &b.join("bin/python"), FAKE_PYTHON);
    b
}

fn calls(prefix: &Path) -> String {
    std::fs::read_to_string(prefix.join("calls.log")).unwrap_or_default()
}

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv(args);
    (r.stdout, r.stderr, r.code)
}

fn run_env(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let r = f.pyenv_env(args, env);
    (r.stdout, r.stderr, r.code)
}

/// `pyenv <args>` with `input` on stdin (the y/N questions).
fn run_stdin(f: &Fixture, args: &[&str], input: &str) -> (String, String, i32) {
    for _ in 0..100 {
        let mut c = f
            .command(Path::new(env!("CARGO_BIN_EXE_pyenv")), &f.work, &[])
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let o = c.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        if o.status.code() == Some(126) && err.contains("Text file busy") {
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        }
        return (String::from_utf8_lossy(&o.stdout).into_owned(), err, o.status.code().unwrap());
    }
    panic!("busy");
}

#[test]
fn creates_an_env_its_link_pydoc_and_shims() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    let (out, err, code) = run(&f, &["virtualenv", "3.12.1", "venv1"]);
    assert_eq!((out.as_str(), code), ("", 0), "{err}");
    let env = base.join("envs/venv1");
    assert!(env.join("pyvenv.cfg").is_file());
    assert_eq!(std::fs::read_link(f.root.join("versions/venv1")).unwrap(), env);
    assert_eq!(calls(&base), format!("venv {}\n", env.display()));
    let pydoc = std::fs::read_to_string(env.join("bin/pydoc")).unwrap();
    assert_eq!(pydoc, format!("#!{}/bin/python\nimport pydoc\nif __name__ == '__main__':\n      pydoc.cli()\n", env.display()));
    assert!(f.root.join("shims/python").exists(), "rehash ran");
}

#[test]
fn one_name_uses_the_current_version_and_a_prefix_is_resolved() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    assert_eq!(run_env(&f, &["virtualenv", "venv2"], &[("PYENV_VERSION", "3.12.1")]).2, 0);
    assert!(base.join("envs/venv2").is_dir());
    assert_eq!(run(&f, &["virtualenv", "3.12", "venv3"]).2, 0);
    assert!(base.join("envs/venv3").is_dir());
}

/// Review focus 1: names that would leave `versions/` are refused, and nothing is made.
#[test]
fn name_checks() {
    let f = Fixture::new();
    fake_base(&f, "3.12.1");
    let err = |args: &[&str]| run(&f, args).1;
    assert_eq!(err(&["virtualenv"]), "pyenv-virtualenv: no virtualenv name given.\n");
    assert_eq!(err(&["virtualenv", "3.12.1", "system"]), "pyenv-virtualenv: `system' is not allowed as virtualenv name.\n");
    assert_eq!(err(&["virtualenv", "3.12.1", "a b"]), "pyenv-virtualenv: no whitespace allowed in virtualenv name.\n");
    assert_eq!(err(&["virtualenv", "3.12.1", "x/y"]), "pyenv-virtualenv: no slash allowed in virtualenv name.\n");
    assert_eq!(err(&["virtualenv", "3.12.1", "../x"]), "pyenv-virtualenv: no slash allowed in virtualenv name.\n");
    assert_eq!(err(&["virtualenv", "3.12.1", ".."]), "pyenv-virtualenv: `..' is not allowed as virtualenv name.\n");
    assert_eq!(run(&f, &["virtualenv", "3.12.1", "3.12.1/envs/ok"]).2, 0);
    assert!(!f.base.join("x").exists() && !f.root.join("x").exists());
}

#[test]
fn a_base_that_is_not_installed() {
    let f = Fixture::new();
    assert_eq!(
        run(&f, &["virtualenv", "9.9.9", "v"]),
        (String::new(),
         "pyenv-virtualenv: `9.9.9' is not installed in pyenv.\nIt does not look like a valid Python version. See `pyenv install --list' for available versions.\n".into(),
         1)
    );
}

/// allowlist D-97: `-u` alone reruns venv with `--upgrade`; the env's own link isn't "taken".
#[test]
fn upgrade_implies_force() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    run(&f, &["virtualenv", "3.12.1", "venv1"]);
    assert_eq!(run(&f, &["virtualenv", "-u", "3.12.1", "venv1"]).2, 0);
    let env = base.join("envs/venv1");
    assert!(calls(&base).ends_with(&format!("venv --upgrade {}\n", env.display())));
}

/// allowlist D-97 and review focus 2: a name taken by another env's link, or by a real
/// version folder (even with -f), is refused, and the folder is kept.
#[test]
fn a_taken_name_is_refused() {
    let f = Fixture::new();
    fake_base(&f, "3.12.1");
    let other = fake_base(&f, "3.12.2");
    run(&f, &["virtualenv", "3.12.1", "venv1"]);
    let link = f.root.join("versions/venv1");
    assert_eq!(run(&f, &["virtualenv", "3.12.2", "venv1"]).1,
               format!("pyenv-virtualenv: `{}' already exists.\n", link.display()));
    assert_eq!(run(&f, &["virtualenv", "-f", "3.12.1", "3.12.2"]),
               (String::new(), format!("pyenv-virtualenv: `{}' already exists.\n", other.display()), 1));
    assert!(other.join("bin/python").is_file());
}

#[test]
fn an_existing_env_asks_first() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    run(&f, &["virtualenv", "3.12.1", "venv1"]);
    let env = base.join("envs/venv1");
    let (_, err, code) = run_stdin(&f, &["virtualenv", "3.12.1", "venv1"], "n\n");
    assert_eq!((err.as_str(), code), (format!("pyenv-virtualenv: {} already exists\n", env.display()).as_str(), 1));
    assert_eq!(run_stdin(&f, &["virtualenv", "3.12.1", "venv1"], "yes\n").2, 0);
}

/// Review focus 4: a failed venv leaves no env and no link; an env that was there before
/// `-f` is kept.
#[test]
fn a_failed_venv_cleans_up() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    let (out, _, code) = run_env(&f, &["virtualenv", "3.12.1", "bad"], &[("FAKE_VENV_FAIL", "1")]);
    assert_eq!((out.as_str(), code), ("venv failed\n", 5));
    assert!(!base.join("envs/bad").exists());
    assert!(std::fs::symlink_metadata(f.root.join("versions/bad")).is_err());
    run(&f, &["virtualenv", "3.12.1", "keep"]);
    assert_eq!(run_env(&f, &["virtualenv", "-f", "3.12.1", "keep"], &[("FAKE_VENV_FAIL", "1")]).2, 5);
    assert!(base.join("envs/keep/pyvenv.cfg").is_file());
}

/// allowlist D-96: `-p` takes the next word, and that interpreter runs `-m venv`.
#[test]
fn dash_p_takes_the_next_word() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    let other = fake_base(&f, "3.13.1");
    let py = other.join("bin/python");
    assert_eq!(run(&f, &["virtualenv", "3.12.1", "-p", py.to_str().unwrap(), "pv"]).2, 0);
    assert!(calls(&other).contains(&base.join("envs/pv").display().to_string()));
    assert_eq!(calls(&base), "");
}

/// allowlist D-99: pip is ensured when venv left it out, unless declined.
#[test]
fn pip_is_ensured_unless_declined() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    assert_eq!(run_env(&f, &["virtualenv", "3.12.1", "e1"], &[("FAKE_NO_PIP", "1")]).2, 0);
    assert!(base.join("envs/e1/bin/pip").is_file());
    assert!(calls(&base.join("envs/e1")).contains("ensurepip"));
    assert_eq!(run(&f, &["virtualenv", "--without-pip", "3.12.1", "e2"]).2, 0);
    assert!(!base.join("envs/e2/bin/pip").exists());
}

/// allowlist D-98: an env made from `system` lives in `versions/<name>`, with no self-link.
#[test]
fn a_system_env_has_no_self_link() {
    let f = Fixture::new();
    exe(&f, &f.syspath.join("python3"), FAKE_PYTHON);
    exe(&f, &f.syspath.join("python"), FAKE_PYTHON);
    assert_eq!(run(&f, &["virtualenv", "system", "sysvenv"]).2, 0);
    let env = f.root.join("versions/sysvenv");
    assert!(env.join("pyvenv.cfg").is_file());
    assert!(std::fs::symlink_metadata(env.join("sysvenv")).is_err());
}

#[test]
fn version_help_and_completion() {
    let f = Fixture::new();
    fake_base(&f, "3.12.1");
    assert_eq!(run_env(&f, &["virtualenv", "--version"], &[("PYENV_VERSION", "3.12.1")]).0,
               "pyenv-virtualenv 1.4.0 (python -m venv)\n");
    let help = run(&f, &["virtualenv", "--help"]);
    assert_eq!((help.0.lines().next(), help.2),
               (Some("Usage: pyenv virtualenv [-f|--force] [VIRTUALENV_OPTIONS] [version] <virtualenv-name>"), 0));
    assert_eq!(run(&f, &["completions", "virtualenv"]).0, "--help\n3.12.1\n");
}
```

- [ ] **Step 2: Run them to see them fail.**

Run: `m4b_wsl.sh cargo test -q -p pyenv --test cli_venv`
Expected: every test fails; `pyenv virtualenv` prints `pyenv: no such command`.

- [ ] **Step 3: Implement `crates/pyenv/src/commands/venv.rs`, the create part.**

```rust
//! The virtualenv commands built in (spec §10, D4; docs/parity/pyenv-virtualenv-m4-reference.md):
//! `virtualenv`, `virtualenvs`, `virtualenv-prefix`, `virtualenv-delete`, and the `uninstall`
//! cascade. Linux matches pyenv-virtualenv v1.4.0 where it works and does what it meant where
//! it doesn't (allowlist D-96 to D-99); Windows is new (D-101).

use crate::install::{prompt, Reply};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::{lookup, pathsearch, prefix, select, venv};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const VERSION: &str = "1.4.0";

pub const HELP: &str = "Usage: pyenv virtualenv [-f|--force] [VIRTUALENV_OPTIONS] [version] <virtualenv-name>\n       pyenv virtualenv --version\n       pyenv virtualenv --help\n\n  -f/--force       Install even if the version appears to be installed already. Skip\n                   prompting for confirmation\n\nNotable VIRTUALENV_OPTIONS passed to venv-creating executable, if applicable:\n  -u/--upgrade     Imply --force\n\n";

pub(crate) fn bin_name(ctx: &Ctx) -> &'static str {
    match ctx.flavor {
        Flavor::Pyenv => "bin",
        Flavor::PyenvWin => "Scripts",
    }
}

#[derive(Default)]
struct Opts {
    force: bool,
    upgrade: bool,
    no_pip: bool,
    quiet: bool,
    verbose: bool,
    help: bool,
    version: bool,
    python: Option<String>,
    pass: Vec<String>,
}

/// `parse_options`, except that `-p`/`--python` take the next word (allowlist D-96): `--x` is
/// one option, `-xy` one per letter, a lone `-` nothing, anything else a positional.
fn parse(args: &[&str]) -> (Opts, Vec<String>) {
    let (mut o, mut pos, mut names) = (Opts::default(), Vec::new(), Vec::<String>::new());
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        if let Some(long) = a.strip_prefix("--") {
            if long == "python" {
                o.python = args.get(i + 1).map(|s| s.to_string());
                i += 1;
            } else {
                names.push(long.to_string());
            }
        } else if a.len() > 1 && a.starts_with('-') {
            for c in a[1..].chars() {
                if c == 'p' {
                    o.python = args.get(i + 1).map(|s| s.to_string());
                    i += 1;
                } else {
                    names.push(c.to_string());
                }
            }
        } else if a != "-" {
            pos.push(a.to_string());
        }
        i += 1;
    }
    for n in names {
        match n.as_str() {
            "f" | "force" => o.force = true,
            "h" | "help" => o.help = true,
            "no-pip" | "no-setuptools" | "without-pip" => {
                o.no_pip = true;
                o.pass.push(format!("--{n}"));
            }
            "q" | "quiet" => o.quiet = true,
            "u" | "upgrade" => o.upgrade = true,
            "v" | "verbose" => o.verbose = true,
            "version" => o.version = true,
            _ => match n.strip_prefix("python=") {
                Some(p) => o.python = Some(p.to_string()),
                None => o.pass.push(format!("--{n}")),
            },
        }
    }
    (o, pos)
}

/// The first selected version, or `system` (Linux); the first pyenv-win selection (Windows).
pub(crate) fn current_names(ctx: &Ctx) -> Vec<String> {
    match ctx.flavor {
        Flavor::Pyenv => select::version_name(ctx, false).names,
        Flavor::PyenvWin => select::win_select(ctx).into_iter().map(|s| s.name).collect(),
    }
}

fn not_installed(v: &str) -> Output {
    let mut o = Output::new();
    o.err(format!("pyenv-virtualenv: `{v}' is not installed in pyenv."));
    #[cfg(unix)]
    let known = crate::install::defs::known(&|k| std::env::var(k).ok()).iter().any(|d| d == v);
    #[cfg(not(unix))]
    let known = true;
    if known {
        o.err(format!("Run `pyenv install {v}' to install it."));
    } else {
        o.err("It does not look like a valid Python version. See `pyenv install --list' for available versions.");
    }
    o.with_code(1)
}

fn which(c: &Ctx, cmd: &str) -> Option<PathBuf> {
    match c.flavor {
        Flavor::Pyenv => {
            let nosystem = c.pyenv_version.as_deref() != Some("system");
            lookup::which_pyenv(c, cmd, nosystem, &lookup::Skip::default()).ok().map(|f| f.path)
        }
        Flavor::PyenvWin => lookup::which_win_runnable(c, cmd).ok().map(|f| f.path),
    }
}

fn venv_works(p: &Path) -> bool {
    Command::new(p)
        .args(["-m", "venv", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The interpreter that runs `-m venv` (Decision 2, allowlist D-96).
fn interpreter(ctx: &Ctx, base: &str, wanted: Option<&str>) -> Result<PathBuf, Output> {
    let mut c = ctx.clone();
    c.pyenv_version = Some(base.to_string());
    if let Some(w) = wanted {
        let p = Path::new(w);
        let bare = !w.contains(['/', '\\']) || p.parent() == Some(ctx.shims_dir().as_path());
        if !bare {
            return Ok(p.to_path_buf());
        }
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(found) = which(&c, &name) {
            return Ok(found);
        }
        let last = match ctx.flavor {
            Flavor::Pyenv => lookup::whence_pyenv(ctx, &name).pop().map(|(_, p)| p),
            Flavor::PyenvWin => lookup::whence_win(ctx, &name, true).pop().map(PathBuf::from),
        };
        return last.ok_or_else(|| not_installed(&name));
    }
    if base == "system" {
        for py in ["python3", "python", "python2"] {
            if let Some(p) = which(&c, py).filter(|p| venv_works(p)) {
                return Ok(p);
            }
        }
        return Err(Output::error("pyenv-virtualenv: no Python with `venv' found on the system"));
    }
    which(&c, "python").ok_or_else(|| Output::error(format!("pyenv-virtualenv: `python' not found in version `{base}'")))
}

fn version_line(ctx: &Ctx) -> Output {
    let cur = current_names(ctx).into_iter().next().unwrap_or_else(|| "system".into());
    let backend = match prefix::prefix_of(ctx, &cur) {
        Ok(p) if ctx.flavor == Flavor::Pyenv && pathsearch::is_runnable(&p.join("bin/conda")) => {
            let v = Command::new(p.join("bin/conda")).arg("--version").output().ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".into());
            format!("conda {v}")
        }
        _ if cur == "system" => "python3 -m venv".into(),
        _ => "python -m venv".into(),
    };
    let mut o = Output::new();
    o.out(format!("pyenv-virtualenv {VERSION} ({backend})"));
    o
}

fn is_link(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
}

/// `versions/<name>` pointing at `env_dir` (Decision 3; Windows: Decision 4, Task 7).
/// Replaces an existing link only; the caller has refused anything else.
pub(crate) fn link_env(ctx: &Ctx, env_dir: &Path, link: &Path) -> std::io::Result<()> {
    match ctx.flavor {
        Flavor::Pyenv => {
            #[cfg(unix)]
            {
                let tmp = link.with_file_name(format!(".{}.{}.tmp", link.file_name().unwrap().to_string_lossy(), std::process::id()));
                let _ = std::fs::remove_file(&tmp);
                std::os::unix::fs::symlink(env_dir, &tmp)?;
                std::fs::rename(&tmp, link).inspect_err(|_| {
                    let _ = std::fs::remove_file(&tmp);
                })
            }
            #[cfg(not(unix))]
            {
                let _ = (env_dir, link);
                Err(std::io::ErrorKind::Unsupported.into())
            }
        }
        Flavor::PyenvWin => {
            #[cfg(windows)]
            {
                if is_link(link) {
                    std::fs::remove_dir(link)?;
                }
                rpyenv_core::junction::create(link, env_dir)
            }
            #[cfg(not(windows))]
            {
                Err(std::io::ErrorKind::Unsupported.into())
            }
        }
    }
}

/// A name segment that can't leave its folder (review focus 1).
fn plain(seg: &str, ctx: &Ctx) -> bool {
    !seg.is_empty()
        && seg != "."
        && seg != ".."
        && !seg.contains('\0')
        && (ctx.flavor == Flavor::Pyenv || crate::install::is_safe_win_segment(seg))
}

pub fn virtualenv(ctx: &Ctx, args: &[&str]) -> Output {
    if args.first() == Some(&"--complete") {
        return crate::commands::versions::versions(ctx, &["--bare", "--skip-envs"]);
    }
    let (o, pos) = parse(args);
    if o.help {
        return Output { stdout: HELP.into(), ..Output::new() };
    }
    if o.version {
        return version_line(ctx);
    }
    let (base, name) = match pos.as_slice() {
        [] => return Output::error("pyenv-virtualenv: no virtualenv name given."),
        [name] => (current_names(ctx).into_iter().next().unwrap_or_default(), name.clone()),
        [base, name, ..] => (base.clone(), name.clone()),
    };
    let base = if base.is_empty() {
        base
    } else {
        let r = crate::commands::latest::latest(ctx, &["-f", &base]);
        Some(r.stdout.trim().to_string()).filter(|s| r.code == 0 && !s.is_empty()).unwrap_or(base)
    };
    if base.is_empty() || name.is_empty() {
        return Output { stdout: HELP.into(), code: 1, ..Output::new() };
    }
    let sep = |c: char| c == '/' || (ctx.flavor == Flavor::PyenvWin && c == '\\');
    let last = name.rsplit(sep).next().unwrap_or("").to_string();
    if last == "system" {
        return Output::error("pyenv-virtualenv: `system' is not allowed as virtualenv name.");
    }
    if name.chars().any(char::is_whitespace) {
        return Output::error("pyenv-virtualenv: no whitespace allowed in virtualenv name.");
    }
    let base_first = base.split(sep).next().unwrap_or("");
    if name.contains(sep) && name.replace('\\', "/") != format!("{base_first}/envs/{last}") {
        return Output::error("pyenv-virtualenv: no slash allowed in virtualenv name.");
    }
    if !plain(&last, ctx) || rpyenv_core::installed::is_staging_name(&last) {
        return Output::error(format!("pyenv-virtualenv: `{last}' is not allowed as virtualenv name."));
    }
    if !base.split(sep).all(|s| plain(s, ctx)) {
        return not_installed(&base);
    }
    let base_dir = match prefix::prefix_of(ctx, &base) {
        Ok(d) if d.is_dir() => d,
        _ => return not_installed(&base),
    };
    let versions = ctx.versions_dir();
    let (full, env_dir, link) = if base == "system" {
        (last.clone(), versions.join(&last), None)
    } else {
        let owner = venv::base_prefix(ctx, &base).ok()
            .filter(|p| p.parent() == Some(versions.as_path()))
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let full = format!("{}/envs/{last}", owner.unwrap_or_else(|| base.clone()));
        let dir = venv::version_dir(ctx, &full);
        (full, dir, Some(versions.join(&last)))
    };
    let force = o.force || o.upgrade;
    if let Some(l) = &link {
        if std::fs::symlink_metadata(l).is_ok() {
            let ours = is_link(l) && std::fs::read_link(l).ok().as_deref() == Some(env_dir.as_path());
            if !is_link(l) || (!ours && !force) {
                return Output::error(format!("pyenv-virtualenv: `{}' already exists.", l.display()));
            }
        }
    }
    let existed = env_dir.is_dir();
    if env_dir.join(bin_name(ctx)).exists() && !force {
        rpyenv_core::textout::write(true, &format!("pyenv-virtualenv: {} already exists\n", env_dir.display()));
        match prompt("continue with installation? (y/N) ") {
            Reply::Line(r) if r.starts_with(['y', 'Y']) => {}
            Reply::Interrupted => return Output::new().with_code(130),
            _ => return Output::new().with_code(1),
        }
    }
    let cleanup = |code: i32| -> Output {
        if let Some(l) = &link {
            if is_link(l) && std::fs::read_link(l).ok().as_deref() == Some(env_dir.as_path()) {
                let _ = if ctx.flavor == Flavor::Pyenv { std::fs::remove_file(l) } else { std::fs::remove_dir(l) };
            }
        }
        if !existed {
            let _ = std::fs::remove_dir_all(&env_dir);
        }
        Output::new().with_code(code)
    };
    let cache = std::env::var_os("PYENV_VIRTUALENV_CACHE_PATH").filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("PYTHON_BUILD_CACHE_PATH").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.root.join("cache"));
    let conda = ctx.flavor == Flavor::Pyenv && pathsearch::is_runnable(&base_dir.join("bin").join("conda"));
    let status = if conda {
        conda_create(&base_dir, &last, &o, &cache, &base)
    } else {
        let python = match interpreter(ctx, &base, o.python.as_deref()) {
            Ok(p) => p,
            Err(out) => return out,
        };
        let mut cmd = Command::new(&python);
        cmd.args(["-m", "venv"]);
        if o.upgrade {
            cmd.arg("--upgrade");
        }
        cmd.args(&o.pass).arg(&env_dir);
        if std::fs::create_dir_all(&cache).is_ok() {
            cmd.current_dir(&cache);
        }
        cmd.env("PYENV_VERSION", &base)
            .env_remove("PIP_REQUIRE_VENV")
            .env_remove("PIP_REQUIRE_VIRTUALENV")
            .env_remove("VIRTUALENV_PYTHON");
        cmd.status().map_err(|e| (python, e))
    };
    let code = match status {
        Ok(s) => s.code().unwrap_or(1),
        Err((p, e)) => {
            let mut out = cleanup(126);
            out.err(format!("pyenv: {}: {}", p.display(), rpyenv_core::launch::io_reason(&e)));
            return out;
        }
    };
    if code != 0 {
        return cleanup(code);
    }
    #[cfg(unix)]
    if ctx.flavor == Flavor::Pyenv && !conda {
        config_links_and_pydoc(&base_dir, &env_dir);
    }
    if let Some(l) = &link {
        if let Err(e) = link_env(ctx, &env_dir, l) {
            let mut out = cleanup(1);
            out.err(format!("pyenv-virtualenv: cannot link {} to {}: {e}", l.display(), env_dir.display()));
            return out;
        }
    }
    if !o.no_pip && !conda {
        if let Err(msg) = ensure_pip(ctx, &full, &env_dir) {
            let mut out = cleanup(1);
            out.err(msg);
            return out;
        }
    }
    let r = crate::commands::rehash::rehash(ctx, &[]);
    Output { stderr: r.stderr, code: r.code, ..Output::new() }
}

/// `python*-config` links from the base, and `bin/pydoc` (reference "The creation run", 4 and 6).
#[cfg(unix)]
fn config_links_and_pydoc(base_dir: &Path, env_dir: &Path) {
    let bin = env_dir.join("bin");
    if let Ok(rd) = std::fs::read_dir(base_dir.join("bin")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("python") && n.ends_with("-config") && std::fs::symlink_metadata(bin.join(&n)).is_err() {
                let _ = std::os::unix::fs::symlink(e.path(), bin.join(&n));
            }
        }
    }
    let pydoc = bin.join("pydoc");
    if std::fs::symlink_metadata(&pydoc).is_err() {
        use std::os::unix::fs::PermissionsExt;
        let body = format!("#!{}/bin/python\nimport pydoc\nif __name__ == '__main__':\n      pydoc.cli()\n", env_dir.display());
        if std::fs::write(&pydoc, body).is_ok() {
            let _ = std::fs::set_permissions(&pydoc, std::fs::Permissions::from_mode(0o755));
        }
    }
}

/// allowlist D-99: ensurepip when the env has no pip, then a local `GET_PIP`; never a download.
fn ensure_pip(ctx: &Ctx, full: &str, env_dir: &Path) -> Result<(), String> {
    let pip = env_dir.join(bin_name(ctx)).join(if ctx.flavor == Flavor::Pyenv { "pip" } else { "pip.exe" });
    if pip.exists() {
        return Ok(());
    }
    let mut c = ctx.clone();
    c.pyenv_version = Some(full.to_string());
    let Some(py) = which(&c, "python") else {
        return Ok(());
    };
    let ok = Command::new(&py).args(["-s", "-m", "ensurepip"]).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if ok {
        return Ok(());
    }
    match std::env::var_os("GET_PIP").map(PathBuf::from).filter(|p| p.is_file()) {
        Some(get_pip) => {
            rpyenv_core::textout::write(true, &format!("Installing pip from {}...\n", get_pip.display()));
            let opts = std::env::var("GET_PIP_OPTS").unwrap_or_default();
            let ok = Command::new(&py).arg("-s").arg(&get_pip).args(opts.split_whitespace())
                .stdout(Stdio::from(std::io::stderr()))
                .status().is_ok_and(|s| s.success());
            if ok { Ok(()) } else { Err("error: failed to install pip via get-pip.py".into()) }
        }
        None => Err(format!("pyenv-virtualenv: pip could not be installed in `{full}': ensurepip failed, and rpyenv doesn't download get-pip.py (set GET_PIP to a local copy)")),
    }
}

/// A conda base: `conda create` (reference "The creation run", step 3).
fn conda_create(base_dir: &Path, last: &str, o: &Opts, cache: &Path, base: &str) -> Result<std::process::ExitStatus, (PathBuf, std::io::Error)> {
    let conda = base_dir.join("bin").join("conda");
    let mut cmd = Command::new(&conda);
    cmd.arg("create");
    if o.quiet { cmd.arg("--quiet"); }
    if o.verbose { cmd.arg("--verbose"); }
    cmd.args(["--name", last, "--yes"]).args(&o.pass).env("PYENV_VERSION", base);
    let list = cache.join(format!("conda-python.{}.txt", std::process::id()));
    match &o.python {
        Some(p) => {
            let n = Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            cmd.arg(format!("python={}", n.strip_prefix("python").unwrap_or(&n)));
        }
        None => {
            let out = Command::new(&conda).args(["list", "python", "--full-name", "--export"]).output().map_err(|e| (conda.clone(), e))?;
            let _ = std::fs::create_dir_all(cache);
            std::fs::write(&list, out.stdout).map_err(|e| (list.clone(), e))?;
            cmd.arg("--file").arg(&list);
        }
    }
    let r = cmd.status().map_err(|e| (conda.clone(), e));
    let _ = std::fs::remove_file(&list);
    r
}
```

Notes for the executor:
- `latest(ctx, &["-f", base])` is the Linux `pyenv-latest -f` call. If the Windows flavor's `latest` doesn't take `-f`, gate the call on `Flavor::Pyenv` and use the name as given on Windows. Ledger the ruling.
- Check the exact name of `launch::io_reason`, and whether `crate::install::defs` is `cfg(unix)`; adjust the two `cfg` lines in `not_installed` to match.
- `Output::error` puts the message on stderr with exit 1, as everywhere else.
- Register the command:
  - in `commands/mod.rs`, add `("virtualenv", venv::virtualenv)` to `COMMANDS`;
  - in `help.rs`, add `topic("virtualenv", Some("Create a Python virtualenv using the pyenv-virtualenv plugin"), Some("Usage: pyenv virtualenv [-f|--force] [VIRTUALENV_OPTIONS] [version] <virtualenv-name>\n       pyenv virtualenv --version\n       pyenv virtualenv --help"), commands::venv::HELP)` to `PYENV`, in name order, and the same entry to the Windows topic table;
  - in `completions.rs`, add `Tail::VersionsBareSkipEnvs` (answered by `commands::versions::versions(ctx, &["--bare", "--skip-envs"])`) and the entries.

- [ ] **Step 4: Run the tests until they pass.**

Run: `m4b_wsl.sh cargo test -q -p pyenv --test cli_venv`
Expected: 11 passed.

Then fix the hard-coded command lists that now include `virtualenv`: `cli_prefix_versions::full_help_listing_and_commands`, the `commands` diff goldens, and `help`. Each one gains the line, as `hooks` did in M4a. The diff goldens are regenerated in Task 9.

- [ ] **Step 5: Run the suites:** Windows `cargo test --workspace` and Linux `m4b_wsl.sh cargo test -q --workspace`; clippy with `-D warnings` on both. Commit: `git commit -m "Add pyenv virtualenv: create a venv from a version, linked into versions"`.

---

### Task 4: `virtualenvs`, `virtualenv-prefix`, `virtualenv-delete`, and the uninstall cascade (Linux)

**Files:**
- Modify:
  - `crates/pyenv/src/commands/venv.rs`;
  - `crates/pyenv/src/commands/mod.rs`, adding the rows `virtualenvs`, `virtualenv-prefix` and `virtualenv-delete` to `COMMANDS`;
  - `help.rs`, adding the three topics, with texts copied verbatim from upstream `bin/pyenv-virtualenvs:2-6`, `bin/pyenv-virtualenv-prefix:3-4` and `bin/pyenv-virtualenv-delete:3-11`;
  - `completions.rs`, adding `Tail::VirtualenvsBare` and the entries `virtualenvs` (`--bare`, `--skip-aliases`) and `virtualenv-delete` (`VirtualenvsBare`);
  - `crates/pyenv/src/commands/uninstall.rs`.
- Test: `crates/pyenv/tests/cli_venv.rs`.

**Interfaces:**
- Produces:
  - `venv::virtualenvs(ctx, args) -> Output`
  - `venv::virtualenv_prefix(ctx, args) -> Output`
  - `venv::virtualenv_delete(ctx, args) -> Output`
  - `venv::DELETE_HELP: &str`
  - `venv::delete_target(ctx, arg: &str, force: bool) -> Result<Option<Target>, Output>`
  - `venv::Target { env: PathBuf, link: Option<PathBuf> }`
  - `venv::remove_target(ctx, &Target, force: bool) -> Result<(), Output>`, which prompts unless forced
  - `venv::uninstall_related(ctx, arg: &str, force: bool) -> Result<Related, Output>`
  - `venv::Related { Env, Base }`

- [ ] **Step 1: Write the failing tests.** Append to `cli_venv.rs`:

```rust
/// An env made on disk (no `pyenv virtualenv`): `versions/<base>/envs/<name>` and its link.
fn make_env(f: &Fixture, base: &str, name: &str, ssp: bool) -> PathBuf {
    let b = fake_base(f, base);
    let e = b.join("envs").join(name);
    std::fs::create_dir_all(e.join("bin")).unwrap();
    std::os::unix::fs::symlink(b.join("bin/python"), e.join("bin/python")).unwrap();
    std::fs::write(e.join("bin/activate"), "").unwrap();
    std::fs::write(e.join("pyvenv.cfg"), format!("home = {}/bin\ninclude-system-site-packages = {ssp}\n", b.display())).unwrap();
    let link = f.root.join("versions").join(name);
    if std::fs::symlink_metadata(&link).is_err() {
        std::os::unix::fs::symlink(&e, &link).unwrap();
    }
    e
}

#[test]
fn virtualenvs_lists_long_names_then_links() {
    let f = Fixture::new();
    let e1 = make_env(&f, "3.12.1", "venv1", false);
    let b = f.root.join("versions/3.12.1");
    assert_eq!(
        run_env(&f, &["virtualenvs"], &[("PYENV_VERSION", "venv1")]).0,
        format!("  3.12.1/envs/venv1 (created from {})\n* venv1 --> {} (set by PYENV_VERSION environment variable)\n", b.display(), e1.display())
    );
    assert_eq!(run(&f, &["virtualenvs", "--bare"]).0, "3.12.1/envs/venv1\nvenv1\n");
    assert_eq!(run(&f, &["virtualenvs", "--bare", "--skip-aliases"]).0, "3.12.1/envs/venv1\n");
    assert_eq!(run(&f, &["virtualenvs", "--foo"]),
               (String::new(), "Usage: pyenv virtualenvs [--bare] [--skip-aliases]\n".into(), 1));
}

#[test]
fn virtualenv_prefix_as_upstream() {
    let f = Fixture::new();
    make_env(&f, "3.12.1", "venv1", false);
    make_env(&f, "3.12.1", "venv2", false);
    let b = f.root.join("versions/3.12.1").display().to_string();
    assert_eq!(run(&f, &["virtualenv-prefix", "venv1"]).0, format!("{b}\n"));
    assert_eq!(run(&f, &["virtualenv-prefix", "venv1", "venv2"]).0, format!("{b}:{b}\n"));
    assert_eq!(run(&f, &["virtualenv-prefix", "3.12.1"]),
               (String::new(), "pyenv-virtualenv: version `3.12.1' is not a virtualenv\n".into(), 1));
    assert_eq!(run(&f, &["virtualenv-prefix"]).1, "pyenv-virtualenv: version `system' is not a virtualenv\n");
}

#[test]
fn delete_by_link_or_long_name_removes_both() {
    let f = Fixture::new();
    let e1 = make_env(&f, "3.12.1", "venv1", false);
    let e2 = make_env(&f, "3.12.1", "venv2", false);
    assert_eq!(run(&f, &["virtualenv-delete", "-f", "venv1"]), (String::new(), String::new(), 0));
    assert!(!e1.exists() && std::fs::symlink_metadata(f.root.join("versions/venv1")).is_err());
    assert_eq!(run(&f, &["virtualenv-delete", "-f", "3.12.1/envs/venv2"]).2, 0);
    assert!(!e2.exists() && std::fs::symlink_metadata(f.root.join("versions/venv2")).is_err());
    assert!(f.root.join("versions/3.12.1/bin/python").is_file());
}

#[test]
fn delete_asks_unless_forced() {
    let f = Fixture::new();
    let e1 = make_env(&f, "3.12.1", "venv1", false);
    assert_eq!(run_stdin(&f, &["virtualenv-delete", "venv1"], "n\n").2, 1);
    assert!(e1.exists());
    assert_eq!(run_stdin(&f, &["virtualenv-delete", "venv1"], "Yup\n").2, 0);
    assert!(!e1.exists());
}

/// Review focus 1, 2 and 3: an escaping name, a real version, and a link pointing outside are
/// never deleted.
#[test]
fn delete_refuses_what_is_not_an_env() {
    let f = Fixture::new();
    fake_base(&f, "3.12.1");
    let outside = f.base.join("outside");
    std::fs::create_dir_all(outside.join("bin")).unwrap();
    std::os::unix::fs::symlink(&outside, f.root.join("versions/mine")).unwrap();
    assert_eq!(run(&f, &["virtualenv-delete", "3.12.1"]).1, "pyenv-virtualenv: `3.12.1' is not a virtualenv.\n");
    assert_eq!(run(&f, &["virtualenv-delete", "-f", "3.12.1"]), (String::new(), String::new(), 0));
    assert!(f.root.join("versions/3.12.1/bin/python").is_file());
    let link = f.root.join("versions/mine");
    assert_eq!(run(&f, &["virtualenv-delete", "-f", "mine"]).1,
               format!("pyenv-virtualenv: `{}' is a symlink for unknown location.\n", link.display()));
    assert!(outside.join("bin").is_dir());
    assert_eq!(run(&f, &["virtualenv-delete", "-f", "../../outside"]).2, 0);
    assert!(outside.join("bin").is_dir());
    assert_eq!(run(&f, &["virtualenv-delete", "3.12.1/envs/nosuch"]).1, "pyenv-virtualenv: virtualenv `nosuch' not installed\n");
}

/// Spec §10 and the v1.4.0 uninstall hook: an env by link name goes with its link; a base goes
/// with its envs and their links; refusing one env stops the whole uninstall.
#[test]
fn uninstall_cascades() {
    let f = Fixture::new();
    let e1 = make_env(&f, "3.12.1", "venv1", false);
    assert_eq!(run(&f, &["uninstall", "-f", "venv1"]), (String::new(), String::new(), 0));
    assert!(!e1.exists() && std::fs::symlink_metadata(f.root.join("versions/venv1")).is_err());
    let e2 = make_env(&f, "3.12.1", "a", false);
    let e3 = make_env(&f, "3.12.1", "b", false);
    let (_, _, code) = run_stdin(&f, &["uninstall", "3.12.1"], "y\nn\n");
    assert_eq!(code, 1);
    assert!(e2.exists() && e3.exists() && f.root.join("versions/3.12.1").is_dir());
    assert_eq!(run(&f, &["uninstall", "-f", "3.12.1"]).0, "pyenv: 3.12.1 uninstalled\n");
    for n in ["3.12.1", "a", "b"] {
        assert!(std::fs::symlink_metadata(f.root.join("versions").join(n)).is_err(), "{n}");
    }
}
```

- [ ] **Step 2: Run them to see them fail.**

Run: `m4b_wsl.sh cargo test -q -p pyenv --test cli_venv`
Expected: the 6 new tests fail.

- [ ] **Step 3: Implement.** Append to `venv.rs`:

```rust
pub const DELETE_HELP: &str = "Usage: pyenv virtualenv-delete [-f|--force] <virtualenv>\n\n   -f  Attempt to remove the specified virtualenv without prompting\n       for confirmation. If the virtualenv does not exist, do not\n       display an error message.\n\nSee `pyenv virtualenvs` for a complete list of installed versions.\n\n";

fn eol_name(ctx: &Ctx, name: &str) -> String {
    match ctx.flavor {
        Flavor::Pyenv => name.to_string(),
        Flavor::PyenvWin => name.replace('/', "\\"),
    }
}

/// `pyenv virtualenvs [--bare] [--skip-aliases]` (reference "pyenv virtualenvs").
pub fn virtualenvs(ctx: &Ctx, args: &[&str]) -> Output {
    let mut o = Output::new();
    if args.contains(&"--complete") {
        o.out("--bare");
        o.out("--skip-aliases");
        return o;
    }
    let (mut bare, mut skip_aliases) = (false, false);
    for a in args {
        match *a {
            "--bare" => bare = true,
            "--skip-aliases" => skip_aliases = true,
            _ => return Output::error("Usage: pyenv virtualenvs [--bare] [--skip-aliases]"),
        }
    }
    let current = if bare { Vec::new() } else { current_names(ctx) };
    let origin = match ctx.flavor {
        Flavor::Pyenv => select::version_origin(ctx),
        Flavor::PyenvWin => select::win_origin(ctx),
    };
    let line = |name: &str, text: String| -> String {
        if bare {
            name.to_string()
        } else if current.iter().any(|c| c.replace('\\', "/") == name.replace('\\', "/")) {
            format!("* {text} (set by {origin})")
        } else {
            format!("  {text}")
        }
    };
    let created = |name: &str| format!("{name} (created from {})", venv::base_prefix(ctx, name).map(|p| p.display().to_string()).unwrap_or_default());
    let tops = rpyenv_core::installed::top_level(&ctx.versions_dir(), ctx.flavor);
    for t in &tops {
        if t.link.is_some() {
            continue;
        }
        for e in rpyenv_core::installed::envs_of(t) {
            let n = eol_name(ctx, &e.name);
            o.out(line(&n, created(&n)));
        }
    }
    for t in &tops {
        match &t.link {
            Some(target) if !skip_aliases => o.out(line(&t.name, format!("{} --> {}", t.name, target.display()))),
            Some(_) => {}
            None if t.path.join(bin_name(ctx)).join("activate").is_file() => o.out(line(&t.name, created(&t.name))),
            None => {}
        }
    }
    o
}

/// `pyenv virtualenv-prefix [<virtualenv>...]` (reference "pyenv virtualenv-prefix").
pub fn virtualenv_prefix(ctx: &Ctx, args: &[&str]) -> Output {
    let names: Vec<String> = if args.is_empty() {
        let c = current_names(ctx);
        if c.is_empty() { vec!["system".into()] } else { c }
    } else {
        select::split_colon(&args.join(":"))
    };
    let mut found = Vec::new();
    for n in &names {
        match venv::base_prefix(ctx, n) {
            Ok(p) => found.push(p.display().to_string()),
            Err(e) => return Output::error(e.message()),
        }
    }
    let mut o = Output::new();
    o.out(found.join(if ctx.flavor == Flavor::Pyenv { ":" } else { ";" }));
    o
}

/// What `virtualenv-delete <arg>` removes.
#[derive(Debug)]
pub struct Target {
    pub env: PathBuf,
    pub link: Option<PathBuf>,
}

/// `<root>/versions/<v>/envs/<e>`, checked by structure, not by text.
fn is_env_path(ctx: &Ctx, p: &Path) -> bool {
    let envs = p.parent();
    envs.and_then(Path::file_name).is_some_and(|n| n == "envs")
        && envs.and_then(Path::parent).and_then(Path::parent) == Some(ctx.versions_dir().as_path())
}

/// Resolves `arg` as v1.4.0 does (reference "Resolving what to delete"). `Ok(None)`: nothing to
/// delete, silently (`-f`). Every name segment must be plain (review focus 1).
pub fn delete_target(ctx: &Ctx, arg: &str, force: bool) -> Result<Option<Target>, Output> {
    let sep = |c: char| c == '/' || (ctx.flavor == Flavor::PyenvWin && c == '\\');
    let name = arg.rsplit(sep).next().unwrap_or("").to_string();
    let not_venv = || Output::error(format!("pyenv-virtualenv: `{arg}' is not a virtualenv."));
    let missing = |force: bool| if force { Ok(None) } else { Err(Output::error(format!("pyenv-virtualenv: virtualenv `{name}' not installed"))) };
    if !arg.split(sep).all(|s| plain(s, ctx)) {
        return if force { Ok(None) } else { Err(not_venv()) };
    }
    let compat = ctx.versions_dir().join(&name);
    let long = arg.replace('\\', "/").contains("/envs/");
    let (env, link) = if long {
        let env = venv::version_dir(ctx, arg);
        if !is_env_path(ctx, &env) {
            return if force { Ok(None) } else { Err(not_venv()) };
        }
        let link = (is_link(&compat) && std::fs::read_link(&compat).ok().as_deref() == Some(env.as_path())).then_some(compat);
        (env, link)
    } else if is_link(&compat) {
        let target = std::fs::read_link(&compat).unwrap_or_default();
        if !is_env_path(ctx, &target) {
            return Err(Output::error(format!("pyenv-virtualenv: `{}' is a symlink for unknown location.", compat.display())));
        }
        (target, Some(compat))
    } else if venv::base_prefix(ctx, &name).is_ok() {
        (compat, None)
    } else if force {
        return Ok(None);
    } else {
        return Err(not_venv());
    };
    if !env.is_dir() {
        return missing(force);
    }
    Ok(Some(Target { env, link }))
}

/// Asks unless forced, removes the env (never following a link inside it), then its link,
/// then rehashes.
pub fn remove_target(ctx: &Ctx, t: &Target, force: bool) -> Result<(), Output> {
    if !force {
        match prompt(&format!("pyenv-virtualenv: remove {}? (y/N) ", t.env.display())) {
            Reply::Line(r) if r.starts_with(['y', 'Y']) => {}
            Reply::Interrupted => return Err(Output::new().with_code(130)),
            _ => return Err(Output::new().with_code(1)),
        }
    }
    let gone = if is_link(&t.env) {
        if ctx.flavor == Flavor::Pyenv { std::fs::remove_file(&t.env) } else { std::fs::remove_dir(&t.env) }
    } else {
        std::fs::remove_dir_all(&t.env)
    };
    if let Err(e) = gone {
        return Err(Output::error(format!("pyenv-virtualenv: cannot remove {}: {e}", t.env.display())));
    }
    if let Some(l) = &t.link {
        let _ = if ctx.flavor == Flavor::Pyenv { std::fs::remove_file(l) } else { std::fs::remove_dir(l) };
    }
    let r = crate::commands::rehash::rehash(ctx, &[]);
    if r.code != 0 {
        return Err(r);
    }
    Ok(())
}

pub fn virtualenv_delete(ctx: &Ctx, args: &[&str]) -> Output {
    match args.first() {
        Some(&"--complete") => return virtualenvs(ctx, &["--bare"]),
        Some(&"-h" | &"--help") => return Output { stdout: DELETE_HELP.into(), ..Output::new() },
        _ => {}
    }
    let (force, rest) = match args.first() {
        Some(&"-f" | &"--force") => (true, &args[1..]),
        _ => (false, args),
    };
    if rest.len() != 1 || rest[0].is_empty() || rest[0].starts_with('-') {
        return Output { stderr: DELETE_HELP.into(), code: 1, ..Output::new() };
    }
    match delete_target(ctx, rest[0], force) {
        Err(o) => o,
        Ok(None) => Output::new(),
        Ok(Some(t)) => remove_target(ctx, &t, force).err().unwrap_or_default(),
    }
}

/// What `pyenv uninstall <arg>` did for envs (v1.4.0's uninstall/envs.bash).
pub enum Related {
    /// `arg` was an env (long name or link): deleted with its link; nothing is left to remove.
    Env,
    /// `arg` is a base: its envs (and their links) are gone; remove the base.
    Base,
}

pub fn uninstall_related(ctx: &Ctx, arg: &str, force: bool) -> Result<Related, Output> {
    let sep = |c: char| c == '/' || (ctx.flavor == Flavor::PyenvWin && c == '\\');
    let name = arg.rsplit(sep).next().unwrap_or("");
    let compat = ctx.versions_dir().join(name);
    let long = arg.replace('\\', "/").contains("/envs/");
    let link_to_env = is_link(&compat) && std::fs::read_link(&compat).is_ok_and(|t| is_env_path(ctx, &t));
    if long || link_to_env {
        if let Some(t) = delete_target(ctx, arg, force)? {
            remove_target(ctx, &t, force)?;
        }
        return Ok(Related::Env);
    }
    let envs = ctx.versions_dir().join(name).join("envs");
    let mut names: Vec<String> = std::fs::read_dir(&envs).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    for n in names {
        let long = format!("{name}/envs/{n}");
        if let Some(mut t) = delete_target(ctx, &long, force)? {
            // The link may have any name: find the ones that point here (Windows: no dangling junctions).
            if t.link.is_none() {
                t.link = links_to(ctx, &t.env);
            }
            remove_target(ctx, &t, force)?;
        }
    }
    Ok(Related::Base)
}

/// A top-level link of `versions/` whose target is `env`.
fn links_to(ctx: &Ctx, env: &Path) -> Option<PathBuf> {
    rpyenv_core::installed::top_level(&ctx.versions_dir(), ctx.flavor)
        .into_iter()
        .find(|t| t.link.as_deref() == Some(env))
        .map(|t| t.path)
}
```

Register the rows, topics and completions listed under **Files**.

`Output` must implement `Default` for `unwrap_or_default()`. If it doesn't, use `.unwrap_or_else(Output::new)`.

In `uninstall.rs`, after the prompt block and before `if present {`:
```rust
        match crate::commands::venv::uninstall_related(ctx, v, force) {
            Ok(crate::commands::venv::Related::Env) => continue,
            Ok(crate::commands::venv::Related::Base) => {}
            Err(o) => {
                o.emit(ctx.flavor);
                return out.with_code(o.code.max(1));
            }
        }
```

Upstream checks "not installed" and asks its own question before the hook, so the ordering above keeps that. `continue` skips the "uninstalled" line, as the probe shows ("No output").

- [ ] **Step 4: Run the tests.**

Run: `m4b_wsl.sh cargo test -q -p pyenv --test cli_venv` and `--test cli_uninstall`
Expected: all pass, the existing uninstall tests included.

- [ ] **Step 5: Run the bats and remove the passing rows.**
  1. Rebuild in WSL, then run `m4b_venv_bats_root.sh` and `m4b_venv_bats_check.sh`.
  2. Every "listed, but it passed" row is fixed by this task or Task 3: remove it from `bats-virtualenv.txt`.
  3. Any "failed, and it is not listed" row is a bug, unless the harness doc's by-design table names it. Fix the bug.

  Expected: 0 problems. Record the new counts in the ledger.

- [ ] **Step 6: Suites, clippy, commit:** `git commit -m "Add virtualenvs, virtualenv-prefix and virtualenv-delete, and delete envs with their base on uninstall"`.

---
### Task 5: `sh-activate`, `sh-deactivate`, and `activate`/`deactivate` without the shell function

**Files:**
- Create: `crates/pyenv/src/commands/activate.rs`
- Modify:
  - `commands/mod.rs`:
    - `pub mod activate;`
    - `COMMANDS` gains `("sh-activate", activate::sh_activate)` and `("sh-deactivate", activate::sh_deactivate)`
    - `LINUX_ONLY` gains `("activate", activate::activate)` and `("deactivate", activate::deactivate)`
  - `help.rs`:
    - topics `activate` and `sh-activate` (the same text);
    - topics `deactivate` and `sh-deactivate`;
    - each copied verbatim from upstream `bin/pyenv-sh-activate:3-12` and `bin/pyenv-sh-deactivate:3-7`.
  - `completions.rs`: `e("activate", true, Words(&["--unset"], VirtualenvsBare))` only. `sh-activate` and the deactivates have no marker (reference "No completion through `pyenv completions`").
- Test: `crates/pyenv/tests/cli_venv.rs`; diff cases in Task 9.

**Interfaces:**
- Consumes:
  - `venv::{base_prefix, is_conda}`;
  - `commands::venv::{virtualenvs, current_names}`;
  - `commands::shell_win::ps_literal`.
- Produces:
  - `activate::sh_activate`, `activate::sh_deactivate`, `activate::activate`, `activate::deactivate`;
  - `enum Sh { Posix, Fish, Pwsh, Cmd }` and `fn shell(ctx) -> Sh`. Task 8 adds the Windows detection.

- [ ] **Step 1: Write the failing tests.** Append to `cli_venv.rs`:

```rust
fn bash(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let mut e = vec![("PYENV_SHELL", "bash")];
    e.extend_from_slice(env);
    run_env(f, args, &e)
}

const DEACTIVATE_TAIL: &str = "if [ -n \"${_OLD_VIRTUAL_PATH:-}\" ]; then\n  export PATH=\"${_OLD_VIRTUAL_PATH}\";\n  unset _OLD_VIRTUAL_PATH;\nfi;\nif [ -n \"${_OLD_VIRTUAL_PYTHONHOME:-}\" ]; then\n  export PYTHONHOME=\"${_OLD_VIRTUAL_PYTHONHOME}\";\n  unset _OLD_VIRTUAL_PYTHONHOME;\nfi;\nif [ -n \"${_OLD_VIRTUAL_PS1:-}\" ]; then\n  export PS1=\"${_OLD_VIRTUAL_PS1}\";\n  unset _OLD_VIRTUAL_PS1;\nfi;\nif declare -f deactivate 1>/dev/null 2>&1; then\n  unset -f deactivate;\nfi;\n";

#[test]
fn sh_activate_prints_upstream_s_posix_code() {
    let f = Fixture::new();
    let e = make_env(&f, "3.12.1", "venv1", false);
    let p = e.display();
    let (out, err, code) = bash(&f, &["sh-activate", "venv1"], &[]);
    assert_eq!((err.as_str(), code), ("", 0));
    assert_eq!(out, format!(
        "unset PYENV_VIRTUAL_ENV;\nunset VIRTUAL_ENV;\n{DEACTIVATE_TAIL}export PYENV_VERSION=\"venv1\";\nexport PYENV_ACTIVATE_SHELL=1;\nexport PYENV_VIRTUAL_ENV=\"{p}\";\nexport VIRTUAL_ENV=\"{p}\";\nexport _OLD_VIRTUAL_PS1=\"${{PS1:-}}\";\nexport PS1=\"(venv1) ${{PS1:-}}\";\n"
    ));
}

#[test]
fn sh_activate_refusals() {
    let f = Fixture::new();
    let e = make_env(&f, "3.12.1", "venv1", false);
    make_env(&f, "3.12.1", "venv2", false);
    assert_eq!(bash(&f, &["sh-activate", "3.12.1"], &[]),
               ("false\n".into(), "pyenv-virtualenv: version `3.12.1' is not a virtualenv\n".into(), 1));
    assert_eq!(bash(&f, &["sh-activate", "--quiet", "3.12.1"], &[]), ("false\n".into(), String::new(), 1));
    assert_eq!(bash(&f, &["sh-activate", "venv1", "venv2"], &[]).1,
               "pyenv-virtualenv: cannot activate multiple versions at once: venv1 venv2\n");
    assert_eq!(bash(&f, &["sh-activate", "venv1"], &[("VIRTUAL_ENV", "/opt/other")]),
               ("true\n".into(), "pyenv-virtualenv: virtualenv `/opt/other' is already activated\n".into(), 0));
    let p = e.display().to_string();
    assert_eq!(bash(&f, &["sh-activate", "venv1"], &[("VIRTUAL_ENV", &p), ("PYENV_VIRTUAL_ENV", &p)]),
               ("true\n".into(), "pyenv-virtualenv: version `venv1' is already activated\n".into(), 0));
}

#[test]
fn sh_deactivate_as_upstream() {
    let f = Fixture::new();
    assert_eq!(bash(&f, &["sh-deactivate"], &[]),
               ("false\n".into(), "pyenv-virtualenv: no virtualenv has been activated.\n".into(), 1));
    let (out, _, code) = bash(&f, &["sh-deactivate"], &[("VIRTUAL_ENV", "/opt/other"), ("PYENV_ACTIVATE_SHELL", "1")]);
    assert_eq!(code, 0);
    assert_eq!(out, format!("unset PYENV_VERSION;\nunset PYENV_ACTIVATE_SHELL;\nunset PYENV_VIRTUAL_ENV;\nunset VIRTUAL_ENV;\n{DEACTIVATE_TAIL}"));
}

#[test]
fn activate_without_the_shell_function() {
    let f = Fixture::new();
    assert_eq!(run(&f, &["activate", "venv1"]),
               (String::new(), "\u{1b}[31;1m\n`pyenv activate' requires Pyenv and Pyenv-Virtualenv to be loaded into your shell.\nCheck your shell configuration and Pyenv and Pyenv-Virtualenv installation instructions.\n\n\u{1b}[0m".into(), 1));
    make_env(&f, "3.12.1", "venv1", false);
    assert_eq!(run(&f, &["completions", "activate"]).0, "--help\n--unset\n3.12.1/envs/venv1\nvenv1\n");
}

/// allowlist D-100: pwsh gets PowerShell, every value through ps_literal (review focus 5).
#[test]
fn pwsh_gets_powershell() {
    let f = Fixture::new();
    make_env(&f, "3.12.1", "v ñ", false);
    let (out, _, code) = run_env(&f, &["sh-activate", "v ñ"], &[("PYENV_SHELL", "pwsh")]);
    assert_eq!(code, 0);
    assert!(out.contains("$Env:VIRTUAL_ENV = "), "{out}");
    assert!(out.is_ascii(), "{out}");
    assert!(!out.contains("export "), "{out}");
}
```

- [ ] **Step 2: Run them to see them fail.**

Run: `m4b_wsl.sh cargo test -q -p pyenv --test cli_venv`
Expected: the 5 new tests fail.

- [ ] **Step 3: Implement `activate.rs`.**

Copy the POSIX and fish texts **from the upstream source**, not from memory:
- the deactivate tails: `bin/pyenv-sh-deactivate`, the heredocs after line 160;
- the fish prompt block: `bin/pyenv-sh-activate:247-261`.

Read them in WSL at `/home/jm/m4b-upstream/pyenv-virtualenv-1.4.0`, substituting `${venv}` where the script does. The diff cases in Task 9 check every byte.

```rust
//! `sh-activate`, `sh-deactivate`, `activate`/`deactivate` without the shell function, and
//! `virtualenv-init` (spec §10; reference "pyenv sh-activate", "pyenv sh-deactivate",
//! "pyenv virtualenv-init"). POSIX and fish code is v1.4.0's byte for byte; pwsh gets
//! PowerShell (allowlist D-100); Windows shells: D-101.

use crate::commands::shell_win::ps_literal;
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::shellname::{self, Family};
use rpyenv_core::venv;
use std::path::{Path, PathBuf};

fn var(k: &str) -> String {
    std::env::var(k).unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Sh {
    Posix,
    Fish,
    Pwsh,
    Cmd,
}

/// `${PYENV_SHELL:-${SHELL##*/}}` on Linux; Task 8 adds Windows.
pub(crate) fn shell(ctx: &Ctx) -> Sh {
    let name = shellname::from_env(std::env::var("PYENV_SHELL").ok().as_deref(), std::env::var("SHELL").ok().as_deref());
    match shellname::family(&name, ctx.flavor) {
        Family::Fish => Sh::Fish,
        Family::Pwsh => Sh::Pwsh,
        Family::Cmd => Sh::Cmd,
        _ => Sh::Posix,
    }
}

const TAIL_POSIX: &str = "if [ -n \"${_OLD_VIRTUAL_PATH:-}\" ]; then\n  export PATH=\"${_OLD_VIRTUAL_PATH}\";\n  unset _OLD_VIRTUAL_PATH;\nfi;\nif [ -n \"${_OLD_VIRTUAL_PYTHONHOME:-}\" ]; then\n  export PYTHONHOME=\"${_OLD_VIRTUAL_PYTHONHOME}\";\n  unset _OLD_VIRTUAL_PYTHONHOME;\nfi;\nif [ -n \"${_OLD_VIRTUAL_PS1:-}\" ]; then\n  export PS1=\"${_OLD_VIRTUAL_PS1}\";\n  unset _OLD_VIRTUAL_PS1;\nfi;\nif declare -f deactivate 1>/dev/null 2>&1; then\n  unset -f deactivate;\nfi;\n";

const TAIL_FISH: &str = "if [ -n \"$_OLD_VIRTUAL_PATH\" ];\n  set -gx PATH \"$_OLD_VIRTUAL_PATH\";\n  set -e _OLD_VIRTUAL_PATH;\nend;\nif [ -n \"$_OLD_VIRTUAL_PYTHONHOME\" ];\n  set -gx PYTHONHOME \"$_OLD_VIRTUAL_PYTHONHOME\";\n  set -e _OLD_VIRTUAL_PYTHONHOME;\nend;\n# check if old prompt function exists\nif functions -q _pyenv_old_prompt\n  # remove old prompt function if exists.\n  functions -e fish_prompt\n  functions -c _pyenv_old_prompt fish_prompt\n  functions -e _pyenv_old_prompt\nend\nif functions -q deactivate;\n  functions -e deactivate;\nend;\n";

const TAIL_PWSH: &str = "if ($Env:_OLD_VIRTUAL_PATH) { $Env:PATH = $Env:_OLD_VIRTUAL_PATH; Remove-Item Env:_OLD_VIRTUAL_PATH }\nif ($Env:_OLD_VIRTUAL_PYTHONHOME) { $Env:PYTHONHOME = $Env:_OLD_VIRTUAL_PYTHONHOME; Remove-Item Env:_OLD_VIRTUAL_PYTHONHOME }\nif (Test-Path function:_pyenv_old_prompt) { Set-Item function:global:prompt (Get-Item function:_pyenv_old_prompt).ScriptBlock; Remove-Item function:_pyenv_old_prompt }\nif (Test-Path function:deactivate) { Remove-Item function:deactivate }\n";

fn set(sh: Sh, k: &str, v: &str) -> String {
    match sh {
        Sh::Posix => format!("export {k}=\"{v}\";"),
        Sh::Fish => format!("set -gx {k} \"{v}\";"),
        Sh::Pwsh => format!("$Env:{k} = {}", ps_literal(v)),
        Sh::Cmd => format!("set \"{k}={v}\""),
    }
}

fn unset(sh: Sh, k: &str) -> String {
    match sh {
        Sh::Posix => format!("unset {k};"),
        Sh::Fish => format!("set -e {k};"),
        Sh::Pwsh => format!("Remove-Item Env:{k} -ErrorAction SilentlyContinue"),
        Sh::Cmd => format!("set \"{k}=\""),
    }
}

/// `<dir>/*.<ext>` in glob order.
fn scripts(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == ext)).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// The failure shape: stdout `false` (or `true`) for an evaluating shell, nothing for pwsh,
/// whose function prints stdout on failure.
fn verdict(o: &mut Output, sh: Sh, word: &str) {
    if sh != Sh::Pwsh {
        o.out(word);
    }
}

pub fn sh_deactivate(ctx: &Ctx, args: &[&str]) -> Output {
    deactivate_code(ctx, args, shell(ctx))
}

pub(crate) fn deactivate_code(ctx: &Ctx, args: &[&str], sh: Sh) -> Output {
    let (mut force, mut quiet, mut verbose) = (false, false, false);
    for a in args {
        match *a {
            "-f" | "--force" => force = true,
            "-q" | "--quiet" => quiet = true,
            "-v" | "--verbose" => {
                quiet = false;
                verbose = true;
            }
            _ => break,
        }
    }
    verbose |= !var("PYENV_VIRTUALENV_VERBOSE_ACTIVATE").is_empty();
    let venv_path = var("VIRTUAL_ENV");
    let mut o = Output::new();
    if venv_path.is_empty() && !force {
        if !quiet {
            o.err("pyenv-virtualenv: no virtualenv has been activated.");
        }
        verdict(&mut o, sh, "false");
        return o.with_code(1);
    }
    let root = format!("{}/", ctx.versions_dir().display());
    let name = match venv_path.strip_prefix(&root) {
        Some(rest) if rest.contains("/envs/") => rest.to_string(),
        _ => venv_path.rsplit(['/', '\\']).next().unwrap_or("").to_string(),
    };
    if verbose {
        o.err(format!("pyenv-virtualenv: deactivate {name}"));
    }
    let prefix = Path::new(&venv_path);
    if !venv_path.is_empty() && venv::is_conda(prefix) && sh != Sh::Fish {
        if sh == Sh::Posix {
            for s in scripts(&prefix.join("etc/conda/deactivate.d"), "sh") {
                o.out(format!(". \"{}\";", s.display()));
            }
            o.out("unset CONDA_PREFIX");
        } else {
            o.out(unset(sh, "CONDA_PREFIX"));
        }
    }
    if !var("PYENV_ACTIVATE_SHELL").is_empty() {
        o.out(unset(sh, "PYENV_VERSION"));
        o.out(unset(sh, "PYENV_ACTIVATE_SHELL"));
    }
    o.out(unset(sh, "PYENV_VIRTUAL_ENV"));
    o.out(unset(sh, "VIRTUAL_ENV"));
    if !var("CONDA_DEFAULT_ENV").is_empty() {
        o.out(unset(sh, "CONDA_DEFAULT_ENV"));
    }
    match sh {
        Sh::Posix => o.stdout.push_str(TAIL_POSIX),
        Sh::Fish => o.stdout.push_str(TAIL_FISH),
        Sh::Pwsh => o.stdout.push_str(TAIL_PWSH),
        Sh::Cmd => o.out("set \"PROMPT=%_OLD_VIRTUAL_PROMPT%\""),
    }
    o
}

/// `versions/<v>`, one level of link resolved, as v1.4.0 computes `VIRTUAL_ENV`.
fn env_prefix(ctx: &Ctx, name: &str) -> PathBuf {
    let p = rpyenv_core::prefix::prefix_of(ctx, name).unwrap_or_else(|_| venv::version_dir(ctx, name));
    std::fs::read_link(&p).map(|t| if t.is_absolute() { t } else { p.parent().unwrap_or(&p).join(t) }).unwrap_or(p)
}

pub fn sh_activate(ctx: &Ctx, args: &[&str]) -> Output {
    let sh = shell(ctx);
    let (mut force, mut quiet, mut verbose) = (false, false, false);
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--complete" => {
                let mut o = Output::new();
                o.out("--unset");
                o.stdout.push_str(&crate::commands::venv::virtualenvs(ctx, &["--bare"]).stdout);
                return o;
            }
            "-f" | "--force" => force = true,
            "-q" | "--quiet" => quiet = true,
            "--unset" => return deactivate_code(ctx, &[], sh),
            "-v" | "--verbose" => {
                quiet = false;
                verbose = true;
            }
            _ => break,
        }
        i += 1;
    }
    verbose |= !var("PYENV_VIRTUALENV_VERBOSE_ACTIVATE").is_empty();
    let mut versions: Vec<String> = args[i..].iter().map(|s| s.to_string()).collect();
    let mut no_shell = false;
    if versions.is_empty() {
        no_shell = true;
        versions = crate::commands::venv::current_names(ctx);
        if versions.is_empty() {
            versions.push("system".into());
        }
    }
    if var("PYENV_VIRTUALENV_INIT").is_empty() {
        no_shell = false;
    }
    let mut venv_name = versions[0].clone();
    let mut o = Output::new();
    let (virtual_env, pyenv_virtual_env) = (var("VIRTUAL_ENV"), var("PYENV_VIRTUAL_ENV"));
    if !virtual_env.is_empty() && (pyenv_virtual_env.is_empty() || virtual_env != pyenv_virtual_env) && !force {
        if !quiet {
            o.err(format!("pyenv-virtualenv: virtualenv `{virtual_env}' is already activated"));
        }
        verdict(&mut o, sh, "true");
        return o;
    }
    if venv::base_prefix(ctx, &venv_name).is_err() {
        let cur = crate::commands::venv::current_names(ctx).into_iter().next().unwrap_or_default();
        let long = format!("{}/envs/{venv_name}", cur.split("/envs/").next().unwrap_or(""));
        if venv::base_prefix(ctx, &long).is_ok() {
            venv_name = long;
        } else {
            if !quiet {
                o.err(format!("pyenv-virtualenv: version `{venv_name}' is not a virtualenv"));
            }
            verdict(&mut o, sh, "false");
            return o.with_code(1);
        }
    }
    if versions[1..].iter().any(|v| venv::base_prefix(ctx, v).is_ok()) {
        if !quiet {
            o.err(format!("pyenv-virtualenv: cannot activate multiple versions at once: {}", versions.join(" ")));
        }
        verdict(&mut o, sh, "false");
        return o.with_code(1);
    }
    let prefix = env_prefix(ctx, &venv_name);
    let p = prefix.display().to_string();
    if virtual_env == p && !force {
        if !quiet {
            o.err(format!("pyenv-virtualenv: version `{venv_name}' is already activated"));
        }
        verdict(&mut o, sh, "true");
        return o;
    }
    let d = deactivate_code(ctx, &["--force", "--quiet"], sh);
    o.stdout.push_str(&d.stdout);
    o.stderr.push_str(&d.stderr);
    if verbose {
        o.err(format!("pyenv-virtualenv: activate {venv_name}"));
    }
    if !no_shell {
        o.out(set(sh, "PYENV_VERSION", &versions.join(":")));
        o.out(match sh {
            Sh::Posix => "export PYENV_ACTIVATE_SHELL=1;".to_string(),
            Sh::Fish => "set -gx PYENV_ACTIVATE_SHELL 1;".to_string(),
            _ => set(sh, "PYENV_ACTIVATE_SHELL", "1"),
        });
    }
    o.out(set(sh, "PYENV_VIRTUAL_ENV", &p));
    o.out(set(sh, "VIRTUAL_ENV", &p));
    let conda = venv::is_conda(&prefix);
    if conda {
        let name = if p.contains("/envs/") { venv_name.rsplit('/').next().unwrap_or("").to_string() } else { "root".into() };
        o.out(set(sh, "CONDA_DEFAULT_ENV", &name));
    }
    let pythonhome = var("PYTHONHOME");
    if !pythonhome.is_empty() {
        o.out(set(sh, "_OLD_VIRTUAL_PYTHONHOME", &pythonhome));
        o.out(unset(sh, "PYTHONHOME"));
    }
    let disabled = ["PYENV_VIRTUALENV_DISABLE_PROMPT", "PYENV_VIRTUAL_ENV_DISABLE_PROMPT", "VIRTUAL_ENV_DISABLE_PROMPT"]
        .iter()
        .any(|k| !var(k).is_empty());
    if !disabled {
        let tag = match var("PYENV_VIRTUALENV_PROMPT") {
            t if t.is_empty() => format!("({venv_name})"),
            t => t.replacen("{venv}", &venv_name, 1),
        };
        match sh {
            Sh::Posix => {
                o.out("export _OLD_VIRTUAL_PS1=\"${PS1:-}\";");
                o.out(format!("export PS1=\"{tag} ${{PS1:-}}\";"));
            }
            Sh::Fish if !quiet => o.stdout.push_str(&fish_prompt(&venv_name)),
            Sh::Fish => {}
            Sh::Pwsh => {
                o.out("if (-not (Test-Path function:_pyenv_old_prompt)) { Set-Item function:global:_pyenv_old_prompt (Get-Item function:prompt).ScriptBlock }");
                o.out(format!("function global:prompt {{ Write-Host -NoNewline {}; _pyenv_old_prompt }}", ps_literal(&format!("{tag} "))));
            }
            Sh::Cmd => o.out(format!("set \"PROMPT={tag} $P$G\"")),
        }
    }
    if conda {
        match sh {
            Sh::Posix => {
                o.out(format!("export CONDA_PREFIX=\"{p}\";"));
                for s in scripts(&prefix.join("etc/conda/activate.d"), "sh").into_iter().chain(scripts(&prefix.join("etc/profile.d"), "sh")) {
                    o.out(format!(". \"{}\";", s.display()));
                }
            }
            Sh::Fish => {
                for s in scripts(&prefix.join("etc/fish/conf.d"), "fish") {
                    o.out(format!("source \"{}\";", s.display()));
                }
            }
            _ => o.out(set(sh, "CONDA_PREFIX", &p)),
        }
    }
    o
}

/// bin/pyenv-sh-activate:247-261 with `${venv}` substituted: copy the bytes from the source.
fn fish_prompt(venv: &str) -> String {
    format!(
        "functions -e _pyenv_old_prompt              # remove old prompt function if exists. \n                                            # since everything is in memory, it's safe to\n                                            # remove it.\nfunctions -c fish_prompt _pyenv_old_prompt  # backup old prompt function\n\n# from python-venv\nfunction fish_prompt\n    set -l prompt (_pyenv_old_prompt)       # call old prompt function first since it might \n                                            # read exit status\n    echo -n \"({venv}) \"                    # add virtualenv to prompt\n    string join -- \\n $prompt              # handle multiline prompts\nend\n"
    )
}

/// `pyenv activate`/`deactivate` when the shell function isn't loaded (bin/pyenv-activate:23-30).
fn needs_shell(cmd: &str) -> Output {
    Output {
        stderr: format!("\u{1b}[31;1m\n`pyenv {cmd}' requires Pyenv and Pyenv-Virtualenv to be loaded into your shell.\nCheck your shell configuration and Pyenv and Pyenv-Virtualenv installation instructions.\n\n\u{1b}[0m"),
        code: 1,
        ..Output::new()
    }
}

pub fn activate(ctx: &Ctx, args: &[&str]) -> Output {
    if args.first() == Some(&"--complete") {
        let mut o = Output::new();
        o.out("--unset");
        o.stdout.push_str(&crate::commands::venv::virtualenvs(ctx, &["--bare"]).stdout);
        return o;
    }
    needs_shell("activate")
}

pub fn deactivate(_ctx: &Ctx, _args: &[&str]) -> Output {
    needs_shell("deactivate")
}
```

`Flavor` is imported for Task 8. Silence `unused_imports` until then, or add the `use` in Task 8.

- [ ] **Step 4: Run the tests.** Run `m4b_wsl.sh cargo test -q -p pyenv --test cli_venv`, then the Linux suite. `cli_init` gains `activate|deactivate|` in the routed set of `init - bash`: update its expected `commands --sh` (`activate\ndeactivate\nrehash\nshell\n`) and the case line.

  Expected: all pass.

- [ ] **Step 5: Bats.** Run `m4b_venv_bats_root.sh` and the checker. Remove the rows that now pass (`activate.bats`, `deactivate.bats`, `conda-*`), and fix any unlisted failure.

  Expected: 0 problems. Commit: `git commit -m "Add sh-activate and sh-deactivate, and activate and deactivate without the shell function"`.

---

### Task 6: `pyenv virtualenv-init` (Linux; nothing on Windows)

**Files:**
- Modify:
  - `activate.rs`;
  - `commands/mod.rs`: `COMMANDS` gains `("virtualenv-init", activate::virtualenv_init)`;
  - `help.rs`: the topic, from `bin/pyenv-virtualenv-init:2-6`.
- Test: `cli_venv.rs`, plus the Windows check in Task 8's test file.

**Interfaces:**
- Consumes: `commands::misc::hooks` (M4a), the shell detection `init` uses (`crate::commands::init::detected` or its equivalent; read `init.rs` for the name).
- Produces: `activate::virtualenv_init(ctx, args) -> Output`; the helper shims folder `$PYENV_ROOT/.rpyenv/virtualenv/shims` (allowlist D-102).

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn virtualenv_init_prints_the_hook_for_bash() {
    let f = Fixture::new();
    let (out, _, code) = run(&f, &["virtualenv-init", "-", "bash"]);
    assert_eq!(code, 0);
    let shims = f.root.join(".rpyenv/virtualenv/shims");
    assert!(out.starts_with(&format!("export PATH=\"{}:${{PATH}}\";\nexport PYENV_VIRTUALENV_INIT=1;\n_pyenv_virtualenv_hook() {{\n", shims.display())), "{out}");
    assert!(out.ends_with("if ! [[ \"${PROMPT_COMMAND-}\" =~ _pyenv_virtualenv_hook ]]; then\n  PROMPT_COMMAND=\"_pyenv_virtualenv_hook;${PROMPT_COMMAND-}\"\nfi\n"), "{out}");
    let act = std::fs::read_to_string(shims.join("activate")).unwrap();
    assert!(act.contains("eval \"$(pyenv sh-activate --verbose \"$@\" || true)\""));
    // ksh: only the two lines; `bash -` drops the shell name.
    assert_eq!(run(&f, &["virtualenv-init", "-", "ksh"]).0.lines().count(), 2);
    assert_eq!(run(&f, &["virtualenv-init", "bash", "-"]).0.lines().count(), 2);
}

#[test]
fn virtualenv_init_help_mode() {
    let f = Fixture::new();
    assert_eq!(run(&f, &["virtualenv-init", "zsh"]),
               (String::new(), "# Load pyenv-virtualenv automatically by adding\n# the following to ~/.zshrc:\n\neval \"$(pyenv virtualenv-init -)\"\n\n".into(), 1));
}

/// allowlist D-102: the helper scripts are never written through a symlinked folder.
#[test]
fn virtualenv_init_never_writes_through_a_link() {
    let f = Fixture::new();
    let mine = f.base.join("mine");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::create_dir_all(f.root.join(".rpyenv")).unwrap();
    std::os::unix::fs::symlink(&mine, f.root.join(".rpyenv/virtualenv")).unwrap();
    assert_eq!(run(&f, &["virtualenv-init", "-", "bash"]).2, 0);
    assert_eq!(std::fs::read_dir(&mine).unwrap().count(), 0);
}
```

- [ ] **Step 2: Run them to see them fail.**

Run: `m4b_wsl.sh cargo test -q -p pyenv --test cli_venv virtualenv_init`
Expected: 3 failures.

- [ ] **Step 3: Implement.** Append to `activate.rs`. Copy the hook texts verbatim from upstream `bin/pyenv-virtualenv-init:104-269`, and the helper scripts from `shims/activate` and `shims/deactivate`. The structure:

```rust
/// `virtualenv-init [-] [<shell>]` (reference "pyenv virtualenv-init"). Windows: nothing
/// (allowlist D-101).
pub fn virtualenv_init(ctx: &Ctx, args: &[&str]) -> Output {
    if ctx.flavor == Flavor::PyenvWin {
        return Output::new();
    }
    // Each `-` sets print mode and shifts away the first remaining argument, whichever it is.
    let mut rest: Vec<&str> = args.to_vec();
    let mut print = false;
    for a in args {
        if *a == "-" {
            print = true;
            if !rest.is_empty() {
                rest.remove(0);
            }
        }
    }
    let shell = rest.first().map(|s| s.to_string())
        .or_else(|| std::env::var("PYENV_SHELL").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(crate::commands::init::detected);
    if !print {
        let profile = match shell.as_str() {
            "bash" => "~/.bashrc",
            "zsh" => "~/.zshrc",
            "ksh" => "~/.profile",
            "fish" => "~/.config/fish/config.fish",
            _ => "your profile",
        };
        let line = if shell == "fish" {
            "status --is-interactive; and source (pyenv virtualenv-init -|psub)"
        } else {
            "eval \"$(pyenv virtualenv-init -)\""
        };
        return Output {
            stderr: format!("# Load pyenv-virtualenv automatically by adding\n# the following to {profile}:\n\n{line}\n\n"),
            code: 1,
            ..Output::new()
        };
    }
    let shims = helper_shims(ctx);
    let mut o = Output::new();
    if shell == "fish" {
        o.stdout.push_str(&format!("while set index (contains -i -- \"{s}\" $PATH)\nset -eg PATH[$index]; end; set -e index\nset -gx PATH '{s}' $PATH;\nset -gx PYENV_VIRTUALENV_INIT 1;\n", s = shims.display()));
    } else {
        o.out(format!("export PATH=\"{}:${{PATH}}\";", shims.display()));
        o.out("export PYENV_VIRTUALENV_INIT=1;");
    }
    let cached = crate::commands::misc::hooks(ctx, &["version-name"]).stdout.is_empty();
    match shell.as_str() {
        "bash" | "zsh" => {
            o.stdout.push_str(&posix_hook(cached));
            o.stdout.push_str(if shell == "bash" { BASH_REGISTER } else { ZSH_REGISTER });
        }
        "fish" => o.stdout.push_str(&fish_hook(cached)),
        _ => {}
    }
    o
}
```

- `posix_hook(cached)` returns the `_pyenv_virtualenv_hook() { … };` text: reference lines 933-976, which come from `bin/pyenv-virtualenv-init:186-247`, with the GNU stat format `-L -c %Y`. When `cached` is false, the cache blocks are left out, as upstream does.
- `BASH_REGISTER` and `ZSH_REGISTER` are reference lines 984-994. `fish_hook` is `bin/pyenv-virtualenv-init:121-184`.
- `helper_shims(ctx)`:
  - With `PYENV_VIRTUALENV_ROOT` set, it returns `<that>/shims` and writes nothing.
  - Otherwise it returns `<root>/.rpyenv/virtualenv/shims`, after writing `activate` and `deactivate` (mode 755) when missing. It writes only if neither `<root>/.rpyenv` nor `<root>/.rpyenv/virtualenv` is a symlink (`symlink_metadata(..).file_type().is_symlink()`), the rule M4a's links follow.
  - It always returns the path.

- [ ] **Step 4: Run the tests**, then the bats.

Run: `m4b_wsl.sh cargo test -q -p pyenv --test cli_venv`, then the venv bats.
Expected:
- the tests pass;
- `init.bats` rows pass, so remove them from the expected list. If one of them depends on the `PATH` line pointing at the plugin's folder, its row moves to D-102. Fix every other mismatch.

  Then commit: `git commit -m "Add virtualenv-init: auto-activation for bash, zsh and fish"`.

---

### Task 7: Windows: junctions, creating envs, listing, names, deleting, and uninstall

**Files:**
- Create: `crates/rpyenv-core/src/junction.rs` (`#[cfg(windows)]`), `crates/pyenv/tests/cli_venv_win.rs` (`#![cfg(windows)]`)
- Modify:
  - `crates/rpyenv-core/Cargo.toml`: windows-sys feature `Win32_System_IO`;
  - `rpyenv-core/src/lib.rs`: `#[cfg(windows)] pub mod junction;`;
  - `rpyenv-core/src/prefix.rs`: the Windows branch builds `version_dir`, so `/` becomes `\`;
  - `commands/versions.rs`: `versions_win`;
  - `commands/uninstall_win.rs`: the cascade, names of the form `<v>/envs/<n>`, and no `-win32` for env links.
- Test: `cli_venv_win.rs`

**Interfaces:**
- Produces: `junction::create(link: &Path, target: &Path) -> std::io::Result<()>`.

- [ ] **Step 1: Write the failing tests.** `cli_venv_win.rs`:

```rust
//! Built-in virtualenvs on Windows (allowlist D-101; spec §10).
#![cfg(windows)]

mod common;
use common::Fixture;
use std::path::{Path, PathBuf};

/// A stand-in base `python.bat`: `-m venv --help`, and `-m venv DIR` (no options).
const FAKE_PY: &str = "@echo off\r\nif \"%~1\"==\"-m\" if \"%~2\"==\"venv\" goto venv\r\nexit /b 3\r\n:venv\r\nif \"%~3\"==\"--help\" exit /b 0\r\nif defined FAKE_VENV_FAIL (echo venv failed& exit /b 5)\r\nmkdir \"%~3\\Scripts\"\r\n(echo home = %~dp0& echo include-system-site-packages = false)> \"%~3\\pyvenv.cfg\"\r\ncopy /y \"%~f0\" \"%~3\\Scripts\\python.bat\" >nul\r\ntype nul > \"%~3\\Scripts\\activate\"\r\ntype nul > \"%~3\\Scripts\\pip.exe\"\r\nexit /b 0\r\n";

fn base(f: &Fixture, v: &str) -> PathBuf {
    let b = f.root.join("versions").join(v);
    f.file(&b.join("python.bat"), FAKE_PY);
    b
}

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv_env(args, &[("PATHEXT", ".COM;.EXE;.BAT;.CMD")]);
    (r.stdout, r.stderr, r.code)
}

#[test]
fn creates_an_env_behind_a_junction_and_lists_it() {
    let f = Fixture::new();
    let b = base(&f, "3.13.1");
    let (_, err, code) = run(&f, &["virtualenv", "3.13.1", "foo"]);
    assert_eq!(code, 0, "{err}");
    let env = b.join("envs").join("foo");
    assert_eq!(std::fs::read_link(f.root.join("versions").join("foo")).unwrap(), env);
    let listed = run(&f, &["versions"]).0;
    assert!(listed.contains("  3.13.1\\envs\\foo\r\n"), "{listed}");
    assert!(listed.contains(&format!("  foo --> {}\r\n", env.display())), "{listed}");
    assert_eq!(run(&f, &["prefix", "3.13.1/envs/foo"]).0, format!("{}\r\n", env.display()));
}

/// Spec §10: uninstalling by link name removes the env and the junction; uninstalling the base
/// removes its envs and the junctions into them (no dangling junction).
#[test]
fn uninstall_takes_the_env_and_its_junction() {
    let f = Fixture::new();
    let b = base(&f, "3.13.1");
    run(&f, &["virtualenv", "3.13.1", "foo"]);
    run(&f, &["virtualenv", "3.13.1", "bar"]);
    assert_eq!(run(&f, &["uninstall", "-f", "foo"]).2, 0);
    assert!(!b.join("envs").join("foo").exists());
    assert!(std::fs::symlink_metadata(f.root.join("versions").join("foo")).is_err());
    assert_eq!(run(&f, &["uninstall", "-f", "3.13.1"]).2, 0);
    assert!(std::fs::symlink_metadata(f.root.join("versions").join("bar")).is_err());
    assert!(!b.exists());
}

/// Review focus 1 and 3: a name that leaves `versions`, and a user's junction pointing outside.
#[test]
fn names_and_foreign_junctions_are_safe() {
    let f = Fixture::new();
    base(&f, "3.13.1");
    assert_eq!(run(&f, &["virtualenv", "3.13.1", "x\\..\\..\\y"]).1,
               "pyenv-virtualenv: no slash allowed in virtualenv name.\r\n");
    let outside = f.base.join("outside");
    std::fs::create_dir_all(outside.join("Scripts")).unwrap();
    rpyenv_core::junction::create(&f.root.join("versions").join("mine"), &outside).unwrap();
    let (_, err, code) = run(&f, &["virtualenv-delete", "-f", "mine"]);
    assert_eq!(code, 1, "{err}");
    assert!(outside.join("Scripts").is_dir());
}

/// Review focus 4: a failed venv leaves no env and no junction.
#[test]
fn a_failed_venv_cleans_up() {
    let f = Fixture::new();
    let b = base(&f, "3.13.1");
    let r = f.pyenv_env(&["virtualenv", "3.13.1", "bad"], &[("PATHEXT", ".COM;.EXE;.BAT;.CMD"), ("FAKE_VENV_FAIL", "1")]);
    assert_eq!(r.code, 5);
    assert!(!b.join("envs").join("bad").exists());
    assert!(std::fs::symlink_metadata(f.root.join("versions").join("bad")).is_err());
}

#[test]
fn virtualenv_init_prints_nothing() {
    let f = Fixture::new();
    assert_eq!(run(&f, &["virtualenv-init", "-"]), (String::new(), String::new(), 0));
}
```

`rpyenv_core` is a dev-dependency of the `pyenv` crate's tests if `cli_*_win` already use it. If it isn't, add it under `[target.'cfg(windows)'.dev-dependencies]`.

- [ ] **Step 2: Run them to see them fail.**

Run: `cargo test -q -p pyenv --test cli_venv_win`
Expected: compile error (`rpyenv_core::junction`); then, after Step 3's module exists, the behavior failures.

- [ ] **Step 3: Implement.**

`junction.rs`:
```rust
//! Directory junctions for virtualenv links on Windows (spec §10, plan M4b Decision 4).
//! A junction needs no privilege; its target must be absolute. Remove one with
//! `std::fs::remove_dir`, never `remove_file` (Access is denied) or `remove_dir_all`.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().collect()
}

/// Creates the folder `link` as a junction to the absolute folder `target`.
pub fn create(link: &Path, target: &Path) -> std::io::Result<()> {
    if !target.is_absolute() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "junction target must be absolute"));
    }
    std::fs::create_dir(link)?;
    let result = (|| {
        let substitute: Vec<u16> = wide(std::ffi::OsStr::new("\\??\\")).into_iter().chain(wide(target.as_os_str())).collect();
        let print = wide(target.as_os_str());
        // REPARSE_DATA_BUFFER, MountPointReparseBuffer: tag, data length, reserved, then
        // substitute offset/length, print offset/length, then both names with NULs.
        let names_len = (substitute.len() + 1 + print.len() + 1) * 2;
        let data_len = 8 + names_len;
        let mut buf: Vec<u8> = Vec::with_capacity(8 + data_len);
        buf.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        buf.extend_from_slice(&(data_len as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
        buf.extend_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
        buf.extend_from_slice(&((print.len() * 2) as u16).to_le_bytes());
        for u in substitute.iter().chain([0u16].iter()).chain(print.iter()).chain([0u16].iter()) {
            buf.extend_from_slice(&u.to_le_bytes());
        }
        let path: Vec<u16> = wide(link.as_os_str()).into_iter().chain(Some(0)).collect();
        // SAFETY: path is NUL-terminated; the handle is closed below.
        let h = unsafe {
            CreateFileW(path.as_ptr(), FILE_GENERIC_WRITE, 0, std::ptr::null(), OPEN_EXISTING,
                        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT, std::ptr::null_mut())
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let mut returned = 0u32;
        // SAFETY: buf outlives the call; h is a valid handle.
        let ok = unsafe {
            DeviceIoControl(h, FSCTL_SET_REPARSE_POINT, buf.as_ptr().cast(), buf.len() as u32,
                            std::ptr::null_mut(), 0, &mut returned, std::ptr::null_mut())
        };
        let err = std::io::Error::last_os_error();
        // SAFETY: h came from CreateFileW above.
        unsafe { CloseHandle(h) };
        if ok == 0 { Err(err) } else { Ok(()) }
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir(link);
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_junction_reads_back_and_removes_without_its_target() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("target");
        std::fs::create_dir_all(target.join("inside")).unwrap();
        let link = t.path().join("link");
        super::create(&link, &target).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        assert!(link.join("inside").is_dir());
        std::fs::remove_dir(&link).unwrap();
        assert!(target.join("inside").is_dir());
    }
}
```

Check the windows-sys 0.61 type of `HANDLE` (a pointer, `*mut c_void`): `INVALID_HANDLE_VALUE` compares as written. If `FILE_GENERIC_WRITE`'s path differs in 0.61, use the constant `0x4000_0000` (`GENERIC_WRITE`) and ledger it.

Windows behavior:
- `prefix.rs`, Windows branch: join `name.replace('/', "\\")`. The probe found mixed separators.
- `versions_win`:
  1. After each top-level entry's line, add its envs: `installed::envs_of(&entry)`, with names printed as `<base>\envs\<name>`, using the same `*`/two-space marker.
  2. Print a junction as `<name> --> <read_link target>`.
  3. With `--bare`, print the names only, as today.
- `uninstall_win.rs`:
  1. `is_version` also accepts `<v>/envs/<n>` and `<v>\envs\<n>` when every segment passes `is_version`.
  2. `Check32Bit` is skipped when `versions\<name>` is a link, or when the name contains `envs`.
  3. Before `remove(&versions, &p, n)`, call `crate::commands::venv::uninstall_related(ctx, n, force)`:
     - `Related::Env`: print `pyenv: Successfully uninstalled {n}` and continue;
     - `Related::Base`: go on and remove the base;
     - `Err(o)`: emit it, set status 1, and continue.
  4. A long name skips `target()`'s plain-name check, because `uninstall_related` validated it.

- [ ] **Step 4: Run the tests.**

Run: `cargo test -q -p pyenv --test cli_venv_win`, then `cargo test -q -p rpyenv-core junction`, then the Windows suite, `cargo test --workspace`.
Expected: all pass. `cli_uninstall_win` and the pyenv-win overlay run unchanged: run `parity/pyenv_win_run.py` against a fresh clone, as in M4a.

- [ ] **Step 5: Probe with a real Python, record it in the ledger, and commit.**
  1. In scratch (`C:\tmp\m4b_probe\`, with `PYENV`, `PYENV_ROOT`, `PYENV_HOME` and `HOME` all pointing there), copy a CPython install from uv's cache as `versions\3.13.12`. Never link to it.
  2. Run `pyenv virtualenv 3.13.12 foo`, then `pyenv exec python -c "import sys; print(sys.prefix)"` with `PYENV_VERSION=foo`, then `pyenv uninstall -f foo`.
  3. Record the outputs in the ledger, then delete the scratch dir by literal path.
  4. Commit: `git commit -m "Windows virtualenvs: junction links, listing, and uninstall with their base"`.

---

### Task 8: Windows activation: PowerShell, cmd, Git Bash

**Files:**
- Modify:
  - `activate.rs`: `shell()` on Windows uses `shell_win::code_family`, made `pub(crate)`. Without integration, Windows `activate`/`deactivate` print the code for the detected shell and exit 1, as `shell_win::shell` does. Git Bash's `VIRTUAL_ENV` is in MSYS form;
  - `commands/mod.rs`: `WIN_ONLY` gains `("activate", activate::activate)` and `("deactivate", activate::deactivate)`;
  - `init_win.rs`: route `activate` and `deactivate` in `PWSH_FUNCTION`.
- Test: `crates/pyenv/tests/cli_venv_win.rs`

**Interfaces:**
- Consumes:
  - `shell_win::{code_family, lf, ps_literal}`;
  - `init_win::msys`, made `pub(crate)` if it isn't. Find the function that writes the shims path in MSYS form, and reuse it.

- [ ] **Step 1: Write the failing tests.** Append to `cli_venv_win.rs`:

```rust
#[test]
fn pwsh_activate_and_deactivate_round_trip() {
    let f = Fixture::new();
    base(&f, "3.13.1");
    run(&f, &["virtualenv", "3.13.1", "foo"]);
    let env = f.root.join("versions").join("3.13.1").join("envs").join("foo");
    // Same host helper as cli_shell_win.rs: Windows PowerShell -NoProfile, scratch HOME.
    let script = "iex ((pyenv init - powershell) -join \"`n\"); pyenv activate foo; Write-Output \"[$Env:VIRTUAL_ENV][$Env:PYENV_VERSION]\"; pyenv deactivate; Write-Output \"[$Env:VIRTUAL_ENV]\"";
    let out = common::powershell(&f, script);
    assert_eq!(out, format!("[{}][foo]\r\n[]\r\n", env.display()));
}

#[test]
fn cmd_and_no_integration_print_the_set_lines() {
    let f = Fixture::new();
    base(&f, "3.13.1");
    run(&f, &["virtualenv", "3.13.1", "foo"]);
    let r = f.pyenv_env(&["activate", "foo"], &[("PATHEXT", ".COM;.EXE;.BAT;.CMD"), ("PYENV_SHELL", "cmd")]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains("set \"VIRTUAL_ENV="), "{}", r.stdout);
    assert!(r.stderr.contains("cmd has no shell integration"), "{}", r.stderr);
}

#[test]
fn git_bash_gets_an_msys_virtual_env() {
    let f = Fixture::new();
    base(&f, "3.13.1");
    run(&f, &["virtualenv", "3.13.1", "foo"]);
    let r = f.pyenv_env(&["sh-activate", "foo"], &[("PATHEXT", ".COM;.EXE;.BAT;.CMD"), ("PYENV_SHELL", "bash")]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("export VIRTUAL_ENV=\"/"), "{}", r.stdout);
    assert!(!r.stdout.contains('\r'), "LF only");
}
```

`common::powershell` is whatever `cli_shell_win.rs` uses to run PowerShell with `-NoProfile` and a scratch `HOME`. Reuse it under its real name. If it's private to that file, move it to `common`.

- [ ] **Step 2: Run them to see them fail.**

Run: `cargo test -q -p pyenv --test cli_venv_win`
Expected: the 3 new tests fail.

- [ ] **Step 3: Implement.**
- `activate::shell(ctx)` on `Flavor::PyenvWin`: `shell_win::code_family()` mapped as `Pwsh` → `Sh::Pwsh`, `Cmd` → `Sh::Cmd`, `Fish` → `Sh::Fish`, everything else → `Sh::Posix`. With no family, `sh_activate` returns `Output::error("pyenv: can't tell which shell to print code for: set PYENV_SHELL, or load the integration (`pyenv init`)")`.
- In `sh_activate` and `deactivate_code`, on Windows with `Sh::Posix`:
  - the `VIRTUAL_ENV` and `PYENV_VIRTUAL_ENV` values go through the MSYS converter;
  - the whole output goes through `shell_win::lf`.
- Windows `activate`/`deactivate` (no function): like `shell_win::shell`:
  1. Validate by running `sh_activate`.
  2. Print its stdout for the detected shell, or for every shell with the shell labels when none is detected.
  3. Add the D-89-style stderr line (`cmd has no shell integration, so nothing was changed: run the command above.` for cmd, and the PowerShell line otherwise).
  4. Exit 1.
- `PWSH_FUNCTION`: the condition becomes

  ```powershell
  if (($command -eq 'shell' -and $rest.Count -gt 0 -and $rest[0] -ne '--help') -or $command -eq 'activate' -or $command -eq 'deactivate') {
    $shell_cmds = & $pyenv "sh-$command" @rest
  ```

  Keep the existing `$LASTEXITCODE` and `Invoke-Expression` lines. Update `cli_init_win`'s expected function text.

- [ ] **Step 4: Run the tests.**

Run: `cargo test -q -p pyenv --test cli_venv_win`, then `--test cli_init_win`, then the Windows suite.
Expected: all pass. Commit: `git commit -m "Windows activate and deactivate for PowerShell, cmd and Git Bash"`.

---

### Task 9: Parity finish: diff cases, the expected lists, allowlist rows, `DELIVERED`, spec §10

**Files:**
- Modify: `parity/diff_cases.py`, `parity/golden/**`, `parity/expected/bats-virtualenv.txt`, `parity/expected/bats.txt`, `parity/allowlist.py`, `docs/parity/allowlist.md`, `docs/specs/2026-09-27-rpyenv-design.md` §10

- [ ] **Step 1: Diff cases against the plugin (Linux).** Add these to `diff_cases.py`, each with `plugin=True` and an env fixture built from `files` and `links`:

```python
    # M4b: pyenv-virtualenv v1.4.0 installed on the upstream side (diff.py --pyenv-virtualenv).
    *[Case(name, args, os="Linux", plugin=True, env=env,
           files=(("root/versions/{v0}/envs/venv1/bin/python", "#!/bin/sh\n"),
                  ("root/versions/{v0}/envs/venv1/bin/activate", ""),
                  ("root/versions/{v0}/envs/venv1/pyvenv.cfg", "home = {root}/versions/{v0}/bin\ninclude-system-site-packages = false\n")),
           executable=("root/versions/{v0}/envs/venv1/bin/python",),
           links=(("root/versions/venv1", "root/versions/{v0}/envs/venv1"),))
      for name, args, env in (
          ("sh-activate bash", ("sh-activate", "venv1"), (("PYENV_SHELL", "bash"),)),
          ("sh-activate fish", ("sh-activate", "venv1"), (("PYENV_SHELL", "fish"),)),
          ("sh-activate --quiet fish", ("sh-activate", "--quiet", "venv1"), (("PYENV_SHELL", "fish"),)),
          ("sh-activate not a virtualenv", ("sh-activate", "{v0}"), (("PYENV_SHELL", "bash"),)),
          ("sh-deactivate bash", ("sh-deactivate",), (("PYENV_SHELL", "bash"), ("VIRTUAL_ENV", "/opt/x"), ("PYENV_ACTIVATE_SHELL", "1"))),
          ("sh-deactivate fish", ("sh-deactivate",), (("PYENV_SHELL", "fish"), ("VIRTUAL_ENV", "/opt/x"))),
          ("virtualenvs", ("virtualenvs",), (("PYENV_VERSION", "venv1"),)),
          ("virtualenvs --bare", ("virtualenvs", "--bare"), ()),
          ("virtualenv-prefix", ("virtualenv-prefix", "venv1"), ()),
          ("virtualenv-prefix not a virtualenv", ("virtualenv-prefix", "{v0}"), ()),
          ("virtualenv no name", ("virtualenv",), ()),
          ("virtualenv slash", ("virtualenv", "{v0}", "a/b"), ()),
          ("virtualenv --help", ("virtualenv", "--help"), ()),
          ("virtualenv-delete not a virtualenv", ("virtualenv-delete", "{v0}"), ()),
          ("virtualenv-init - bash", ("virtualenv-init", "-", "bash"), ()),
          ("activate without the function", ("activate", "venv1"), ()),
          ("completions activate", ("completions", "activate"), ()),
      )],
```

- Check that `expand` knows `{root}`; if it doesn't, add it, mapping to the fixture's root.
- Run `m4b_wsl.sh python3 parity/diff.py --rpyenv target/debug --upstream <pyenv 2.8.8> --pyenv-virtualenv /home/jm/m4b-upstream/pyenv-virtualenv-1.4.0`.
- **Expected:** every case is `same`, except two, each with `allow=` and a golden:
  - `virtualenv-init - bash` (D-102: the PATH line);
  - `virtualenv --help` (D-96: no backend help), only if upstream appends some.
- A `differs` anywhere else is a byte bug in Tasks 3–6: fix it there.
- The core cases `commands`, `help`, `no arguments` and `init - bash` (no plugin) now differ by the virtualenv commands. Give them `allow=("D-103",)` and regenerate their goldens.

- [ ] **Step 2: Allowlist rows.**
- Add D-95 to D-103 to `docs/parity/allowlist.md`, worded as in Decisions 1 to 6, each with OS, commands, upstream, rpyenv and reason.
- Widen D-51's commands column with `virtualenv`, `activate` and `deactivate`.
- Widen D-52's text: "pyenv-virtualenv's suite stubs core commands rpyenv answers in-process".
- Add `"M4b"` to `DELIVERED`, so any leftover `M4b` reason now fails the checker.
- Rewrite every remaining row in `bats-virtualenv.txt` with its by-design reason, per the harness doc's table:
  - "creation through a stubbed pyenv-exec" → D-52;
  - stub output → D-52;
  - stub facts → D-52;
  - hooks → D-51;
  - `pip.bats` → D-99;
  - the `PATH` line → D-102.
- Then run the venv bats.

  Expected: `0 problems`, and close to the harness doc's lenient estimate of 57 passes out of 108. Record the actual numbers.

- [ ] **Step 3: The core pyenv bats.** Run `sync288_bats_root.sh` and `m1cb_ctl_bats_check.sh`. A core test that now fails because the virtualenv commands are listed gets a D-103 reason; any other new failure is a bug.

  Expected: 0 problems.

- [ ] **Step 4: Spec §10.** Append:
  - **Windows:** a directory junction (Decision 4), `versions` env lines, activate/deactivate in PowerShell, cmd and Git Bash (Decision 6), no `virtualenv-init` (D-101);
  - **The intended fallbacks:** per selected env, before `system` (D-95);
  - **Creation:** `-p`, `-u`, the pip step and `GET_PIP` (D-96 to D-99).

- [ ] **Step 5: Every check.**
  - Windows: `cargo test --workspace`, clippy, fmt.
  - Linux: suite and clippy; both bats suites; the Linux diff with `--pyenv-virtualenv`; the Windows diff; the pyenv-win overlay on a fresh clone.
  - Parity: `coverage.py` (every row covered) and the parity unit tests on both OSes.

  Commit: `git commit -m "Record M4b's parity: plugin diff cases, the virtualenv bats list, rows D-95 to D-103"`.
