# pyenv-virtualenv behavioral reference: rpyenv M4b virtualenvs (Linux)

Parity target for rpyenv's built-in virtualenv support on Linux (spec §2 D4, §6 "Plugin dispatch", §10). It covers
every command that pyenv-virtualenv v1.4.0 ships, the hooks it installs into pyenv, its shell integration, and how
core pyenv 2.8.8 treats its environments. This file records facts only. Each fact carries an evidence tag.

How `file:line` paths are written:
- A bare path is relative to the root of the pyenv-virtualenv repository at the pinned tag below. So
  `bin/pyenv-sh-activate:141` is that file's line 141, and `test/init.bats:44` is a line of the plugin's bats suite.
- `pyenv:<path>` is a file of core pyenv at the M3 pin (2.8.8, commit `07171d013cac53d1cc9248b4c160217f288a2965`),
  for example `pyenv:libexec/pyenv-which:95`.

Companions: `pyenv-m1-reference.md` (**M1**: dispatcher, hooks, `versions`, `which`, `prefix`, `version-name`),
`pyenv-m2-reference.md` (**M2**: `install`, `uninstall`), `pyenv-m3-reference.md` (**M3**: `init`, the shell function,
`sh-*`, completions). This file cross-references them.

## Source, version, and evidence method

- **Source.** GitHub `pyenv/pyenv-virtualenv`, tag `v1.4.0`, fetched as
  `https://github.com/pyenv/pyenv-virtualenv/archive/refs/tags/v1.4.0.tar.gz` and unpacked at
  `/home/jm/m4b-upstream/pyenv-virtualenv-1.4.0` in WSL Debian.
  - Tarball SHA-256: `9aaf9f01660f10f538251fdaaf552d429e7fd41efb7b651b69a3a9768f4a181f`.
  - `git ls-remote https://github.com/pyenv/pyenv-virtualenv refs/tags/v1.4.0 'refs/tags/v1.4.0^{}'` returned one line,
    `eda64556af9b2992386deeb75dad2130899fc4c9 refs/tags/v1.4.0`, and no peeled `^{}` line. The tag is therefore
    lightweight, and that SHA is the commit.
- **Version.** `bin/pyenv-virtualenv:16` sets `PYENV_VIRTUALENV_VERSION="1.4.0"`. `CHANGELOG.md:3-5` lists two v1.4.0
  changes: POSIX sh (dash) compatibility in `pyenv-virtualenv-init`, and faster `pyenv virtualenvs` output.
- **Core pyenv.** All probes ran the plugin on pyenv 2.8.8, the M3 pin, unpacked at
  `/home/jm/m1cb-upstream/pyenv-07171d013cac53d1cc9248b4c160217f288a2965`.

Evidence tags:

| Tag | Meaning |
|---|---|
| `[src f:l]` | Derived by reading the code at that line. Not executed by itself. |
| `[test f:l]` | A value asserted by the plugin's own bats suite. |
| `[probe]` | Observed by running the pinned scripts in WSL. See the next paragraphs. |
| **UNCONFIRMED** | Not established by any of the above. |

**Probe environment.** WSL2 Debian (the M3 host), GNU bash 5.3.3, `/usr/bin/python3` = Python 3.13.12.

- **Shells.** Only `bash` and `dash` exist in WSL. Output for `zsh`, `fish`, `ksh` and `pwsh` was probed as text, by
  setting `PYENV_SHELL` or passing the shell name. It was never run in those shells.
- **No `ensurepip`.** Debian's `python3-venv` is not installed, and `import ensurepip` fails for every
  `/usr/bin/python3.9` … `python3.13` [probe]. A default `python -m venv` therefore fails on this host, so almost every
  creation probe passes `--without-pip`. The plugin's own "install pip into the new env" step is `[src]` only. The
  probes show only that it is skipped for 3.1x bases.
- **Fake bases.** No real pyenv-built Python was used. The base `<root>/versions/3.13.12` is a directory whose
  `bin/python`, `bin/python3` and `bin/python3.13` are symlinks to `/usr/bin/python3.13`, plus a script
  `bin/python3.13-config` and a script `bin/basetool`. Environments were created from it with the system interpreter
  only.
  - The base `3.12.99` has `bin/python -> /usr/bin/python3.12` and a fake `bin/virtualenv` script. That script logs
    its argv, then creates `bin/python`, `bin/activate` and `pyvenv.cfg` in its last argument.
  - The base `miniconda3-4.7.12` has `bin/python`, `bin/activate`, `conda-meta/` and a fake `bin/conda` script. That
    script logs its argv and, on `create --name N`, creates `envs/N/{bin/python,conda-meta}`. A hand-made
    `envs/cenv` with `conda-meta/` and `etc/conda/{activate.d,deactivate.d}`, `etc/profile.d` and `etc/fish/conf.d`
    scripts was added.
- **Isolation.** The core tree was copied to `/home/jm/m4b-scratch-a/pyenv`, with `bin/pyenv -> ../libexec/pyenv`. The
  plugin was copied to `$PYENV_ROOT/plugins/pyenv-virtualenv`, the usual `git clone` location.
  - Every probe script exported `HOME=/home/jm/m4b-scratch-a/home`, `PYENV_ROOT=$HOME/.pyenv`, `PYENV` and `PYENV_HOME`
    (both pointing into the scratch dir), `TMPDIR`, `LANG=C.UTF-8`, `SHELL=/bin/bash` and
    `PATH=<scratch>/pyenv/bin:/usr/local/bin:/usr/bin:/bin`.
  - It unset `PYENV_VERSION`, `PYENV_SHELL`, `VIRTUAL_ENV`, `PYENV_VIRTUAL_ENV`, `PYENV_VIRTUALENV_INIT`, `PS1`,
    `PROMPT_COMMAND`, `PYENV_HOOK_PATH` and every prompt and conda variable named below.
  - It aborted unless `HOME` and `PYENV_ROOT` were under the scratch dir. The scratch dir was deleted afterwards.
- **Capture.** Stdout and stderr were captured separately and shown with `cat -A`. Stdin was `/dev/null`, or the
  piped answer text that is shown with the command.
- **Placeholders** in quoted output: `<root>` is `PYENV_ROOT` (`/home/jm/m4b-scratch-a/home/.pyenv` in the probes),
  and `<plugin>` is `<root>/plugins/pyenv-virtualenv`.
- **Prompts.** `read -p` shows its prompt text only when stdin is a terminal. With piped answers, stderr stayed empty
  for every `y/N` question below [probe]. The prompt texts are therefore `[src]`.

**Harness check** [probe]. The plugin's own suite (`test/*.bats`, which stubs every core `pyenv-*` command) ran with
Bats 1.11.1 on a copy of the tree: `1..111`, **111 ok, 0 not ok, 0 skipped**. `grep -c '@test'` over `test/*.bats` sums
to 111. Upstream CI runs the same suite with Bats 1.10.0 on Ubuntu 22.04/24.04 and four macOS images
(`.github/workflows/tests.yml:8-19`).

---

## What v1.4.0 ships, and how pyenv finds it

| File | Role |
|---|---|
| `bin/pyenv-virtualenv` | create an env |
| `bin/pyenv-virtualenvs` | list envs |
| `bin/pyenv-virtualenv-delete` | delete an env |
| `bin/pyenv-virtualenv-prefix` | print the base Python prefix of an env |
| `bin/pyenv-virtualenv-init` | print shell code for auto-activation |
| `bin/pyenv-sh-activate`, `bin/pyenv-sh-deactivate` | print shell code that activates or deactivates |
| `bin/pyenv-activate`, `bin/pyenv-deactivate` | stubs that only print "requires … loaded into your shell" |
| `libexec/pyenv-virtualenv-realpath` | sourced by `virtualenv-prefix`: a `realpath` fallback |
| `etc/pyenv.d/which/{conda,python-config,system-site-packages}.bash` | `pyenv which` hooks |
| `etc/pyenv.d/rehash/envs.bash` | `pyenv rehash` hook |
| `etc/pyenv.d/uninstall/envs.bash` | `pyenv uninstall` hook |
| `shims/activate`, `shims/deactivate` | `source activate <env>` / `source deactivate` helpers |
| `install.sh` | copies `bin`, `libexec`, `shims`, `etc/pyenv.d` under `$PREFIX` (default `/usr/local`) (`install.sh:10-33`) |

- **Discovery.** The core dispatcher adds `${PYENV_ROOT}/plugins/*/bin` to `PATH` and appends
  `${PYENV_ROOT}/plugins/*/etc/pyenv.d` to `PYENV_HOOK_PATH` [src pyenv:libexec/pyenv:84-106]. With the plugin in
  `<root>/plugins/pyenv-virtualenv`, `pyenv commands` lists `activate`, `deactivate`, `virtualenv`, `virtualenv-delete`,
  `virtualenv-init`, `virtualenv-prefix` and `virtualenvs` among the core commands [probe]. `sh-activate` and
  `sh-deactivate` appear as `activate` and `deactivate` (M1 `commands`).
- **`pyenv hooks`** [probe]:
  - `pyenv hooks which` prints the three `which/*.bash` files in the order `conda`, `python-config`,
    `system-site-packages`.
  - `pyenv hooks rehash` prints core's `pyenv.d/rehash/conda.bash` and `source.bash`, then the plugin's
    `rehash/envs.bash`.
  - `pyenv hooks uninstall` prints `uninstall/envs.bash`.
  - `pyenv hooks virtualenv` prints nothing.

### Conventions shared by the plugin's commands

- **Shell options.** Every `bin/` script runs under `set -e`, plus `set -x` when `PYENV_DEBUG` is non-empty
  [src bin/pyenv-virtualenv:18-19, and the same lines near the top of each script].
- **Message style.** Messages start with `pyenv-virtualenv: `, except where noted. They quote a value with a backtick
  before it and an apostrophe after it, as core does. Stream and exit code are given per message below.
- **Default `PYENV_ROOT`.** It differs by script:
  - `virtualenv`, `sh-activate` and `sh-deactivate` call `pyenv-root` [src bin/pyenv-virtualenv:21-23,
    bin/pyenv-sh-activate:17-19, bin/pyenv-sh-deactivate:12-14].
  - `virtualenvs`, `virtualenv-delete` and `virtualenv-prefix` fall back to `${HOME}/.pyenv`
    [src bin/pyenv-virtualenvs:11-13, bin/pyenv-virtualenv-delete:34-36, bin/pyenv-virtualenv-prefix:25-27].
  - Through the dispatcher `PYENV_ROOT` is always set, so the difference is visible only when a script is run directly.
- **Plugin hook points.** Each sources `pyenv-hooks <name>`, and the hooks register code with these functions:

  | Command | Hook name | Registration functions | Where they run |
  |---|---|---|---|
  | `virtualenv` | `virtualenv` | `before_virtualenv`, `after_virtualenv` | before the backend runs, and after pip setup [src bin/pyenv-virtualenv:546-562, 591, 658] |
  | `sh-activate` | `activate` | `before_activate`, `after_activate` | after the built-in deactivate, and at the very end [src bin/pyenv-sh-activate:31-47, 162, 305] |
  | `sh-deactivate` | `deactivate` | `before_deactivate`, `after_deactivate` | before any output, and at the end [src bin/pyenv-sh-deactivate:22-38, 79, 228] |

  The hooks are sourced before option parsing in `sh-activate` and `sh-deactivate`, and after it in `virtualenv`. The
  plugin ships no hooks under these three names [probe: `pyenv hooks virtualenv` is empty]. rpyenv runs no bash hooks
  (D4), so these hook points have no rpyenv equivalent.

---

## `pyenv virtualenv` — `bin/pyenv-virtualenv`

### Synopsis and help

```text
# Summary: Create a Python virtualenv using the pyenv-virtualenv plugin
#
# Usage: pyenv virtualenv [-f|--force] [VIRTUALENV_OPTIONS] [version] <virtualenv-name>
#        pyenv virtualenv --version
#        pyenv virtualenv --help
#
#   -f/--force       Install even if the version appears to be installed already. Skip
#                    prompting for confirmation
#
# Notable VIRTUALENV_OPTIONS passed to venv-creating executable, if applicable:
#   -u/--upgrade     Imply --force
```
[src bin/pyenv-virtualenv:3-13]

- **`--help` / `-h`** calls `usage 0` [src bin/pyenv-virtualenv:268-270].
  - `usage` prints `pyenv-help virtualenv`, which is the block above without its first two comment lines.
  - It then appends the backend's own help: `conda create --help`, `<python> -m venv --help` or
    `virtualenv --help`, chosen by `USE_CONDA` / `USE_M_VENV` [src bin/pyenv-virtualenv:133-146].
  - The option loop runs before backend detection, so `USE_*` are still unset at that point, and only
    `pyenv-exec virtualenv --help 2>/dev/null` is tried.
  - Probe with no version selected: stdout was 10 lines, the last one empty, with no backend help, and the exit code
    was 0 [probe].
- **`--complete`** must be the first argument. It execs `pyenv-versions --bare --skip-envs`
  [src bin/pyenv-virtualenv:26-28]. So it lists base versions and the `versions/<name>` links and plain env
  directories, but not the `<base>/envs/<name>` long names. It does not list `system` [probe].
- **`--version`** [src bin/pyenv-virtualenv:113-131, 292-295] prints one line and exits 0. If `PYENV_VERSION` is
  empty, it first sets it to `pyenv-version-name`, then runs backend detection:
  - `pyenv-virtualenv 1.4.0 (conda <conda --version or "unknown">)`. Probe: `pyenv-virtualenv 1.4.0 (conda conda 4.99.0)`.
  - `pyenv-virtualenv 1.4.0 (<python> -m venv)`. With no version selected (`system`), the probe printed
    `pyenv-virtualenv 1.4.0 (python3 -m venv)`, and with `PYENV_VERSION=3.13.12` it printed
    `pyenv-virtualenv 1.4.0 (python -m venv)` [probe].
  - `pyenv-virtualenv 1.4.0 (virtualenv <virtualenv --version or "unknown">)`. Probe:
    `pyenv-virtualenv 1.4.0 (virtualenv virtualenv 20.99.0 from fake)`.
  - Because `--version` is handled inside the option loop, it wins over every later check, such as a missing name.

### Environment cleared and read at start

- `PIP_REQUIRE_VENV` and `PIP_REQUIRE_VIRTUALENV` are unset [src bin/pyenv-virtualenv:30-31]
  [test test/pyvenv.bats:154].
- `FORCE`, `NO_ENSUREPIP`, `QUIET`, `UPGRADE`, `VERBOSE` and **`VIRTUALENV_PYTHON`** are unset before parsing
  [src bin/pyenv-virtualenv:255-260]. A `VIRTUALENV_PYTHON` from the environment is therefore ignored.
- `PYENV_VIRTUALENV_CACHE_PATH` defaults to `${PYTHON_BUILD_CACHE_PATH:-${PYENV_ROOT}/cache}`
  [src bin/pyenv-virtualenv:250-252]. The directory is created and becomes the working directory for the backend run
  [src bin/pyenv-virtualenv:605-606]. In the probes, `<root>/cache` was created and stayed empty.
- `TMPDIR` (default `/tmp`) holds `requirements.<seed>.txt` during `--upgrade` [src bin/pyenv-virtualenv:370-374, 408-410].

### Argument parsing (`parse_options`)

`parse_options` walks **all** arguments first [src bin/pyenv-virtualenv:34-55]:
- An argument starting with `--` becomes one option, with the `--` removed (`--prompt=XX` becomes `prompt=XX`).
- An argument starting with a single `-` is split into one option per letter, so `-ab` becomes `a` and `b`.
- Everything else is appended to `ARGUMENTS`, in order. A lone `-` yields no option and no argument.

Then each option is handled in order [src bin/pyenv-virtualenv:262-308]:

| Option | Effect |
|---|---|
| `f`, `force` | `FORCE=true` |
| `h`, `help` | help, exit 0 |
| `no-pip`, `no-setuptools`, `without-pip` | `NO_ENSUREPIP=1`, and the option is passed through as `--<option>` |
| `p`, `python` | `VIRTUALENV_PYTHON=${ARGUMENTS[0]}`, then the **first positional argument** is removed |
| `python=<x>` | `VIRTUALENV_PYTHON=<x>` |
| `q`, `quiet` | `QUIET=--quiet` |
| `u`, `upgrade` | `UPGRADE=true` |
| `v`, `verbose` | `VERBOSE=--verbose` |
| `version` | print the version, exit 0 |
| anything else | passed through as `--<option>` |

Quirks, each confirmed by a probe:
- **`-p` takes the first positional argument, not the next word.**
  `pyenv virtualenv 3.13.12 -p python3 foo` sets `VIRTUALENV_PYTHON=3.13.12`, leaving `python3 foo`. It then fails with
  ``pyenv-virtualenv: `python3' is not installed in pyenv.`` and
  ``It does not look like a valid Python version. See `pyenv install --list' for available versions.``, exit 1.
  The documented form `pyenv virtualenv -p python3.13 3.12.99 fv2` works only because the interpreter happens to be
  the first positional.
- **Unknown single letters become long options.** `-q -v --clear -x -ab` reached `virtualenv` as
  `--quiet --verbose --clear --x --a --b` (fake-virtualenv log).
- **A separate option value becomes a positional argument.** `pyenv virtualenv --prompt XX 3.13.12 venvp` takes `XX` as
  the base version: ``pyenv-virtualenv: `XX' is not installed in pyenv.`` (plus the "does not look like a valid Python
  version" line), exit 1. `--prompt=XX` works and reached `venv` as `--prompt="XX"` (`pyvenv.cfg` `command` line).

### Base version and env name

- **No positional argument:** `pyenv-virtualenv: no virtualenv name given.` on stderr, exit 1
  [src bin/pyenv-virtualenv:310-312] [probe].
- **One positional:** it is the env name. The base is the **first** entry of `pyenv-version-name`, split on `:`
  [src bin/pyenv-virtualenv:313-320].
  - Probe: `PYENV_VERSION=3.13.12:system pyenv virtualenv --without-pip venv4b` created `3.13.12/envs/venv4b`.
  - With no version selected, the base is `system`. `pyenv virtualenv 3.13.12` therefore means "an env named
    `3.13.12` from system". It failed with ``pyenv-virtualenv: `<root>/versions/3.13.12' already exists.``, exit 1
    [probe].
- **Two or more positionals:** the first is the base and the second is the name. Extra positionals are ignored
  [src bin/pyenv-virtualenv:321-325].
- **Prefix resolution.** If the base is non-empty and `pyenv-latest` exists, the base becomes
  `pyenv-latest -f <base>` [src bin/pyenv-virtualenv:327-329]. Probe: `pyenv virtualenv --without-pip 3.13 venv2`
  created `3.13.12/envs/venv2`.
- **Empty base or empty name:** `usage 1`. The help text goes to **stdout**, with no backend help, and the exit code
  is 1 [src bin/pyenv-virtualenv:331-333]. Probe: `pyenv virtualenv 3.13.12 ""`.

Then the name is validated, in this order. Every failure is on stderr with exit 1, and each line was probed:

| Check | Message | Source |
|---|---|---|
| last path component is `system` | ``pyenv-virtualenv: `system' is not allowed as virtualenv name.`` | `bin/pyenv-virtualenv:335-338` |
| contains whitespace (`[[:space:]]`, so tabs too) | `pyenv-virtualenv: no whitespace allowed in virtualenv name.` | `:340-343` [test test/virtualenv.bats:167,176] |
| contains `/`, unless it equals `<first component of base>/envs/<last component>` | `pyenv-virtualenv: no slash allowed in virtualenv name.` | `:345-348` [test test/virtualenv.bats:194,203] |

- **Slash rule examples.**
  - `pyenv virtualenv 3.13.12 3.13.12/envs/venv3` is accepted.
  - `pyenv virtualenv 3.13.12 3.12.0/envs/venv3y` and `pyenv virtualenv system foo/sysvenv2` are rejected with the
    slash message.
  - `pyenv virtualenv 3.12.0 3.12.0/envs/venv3x` passes the slash check, then fails because 3.12.0 is not installed
    [probe].
- **`PYENV_VERSION` is exported as the base** for the rest of the script [src bin/pyenv-virtualenv:351].
- **The base must exist**: `PREFIX=$(pyenv-prefix)` must be a directory [src bin/pyenv-virtualenv:364-368]. Otherwise
  `not_installed_message` [src :353-361] prints two lines on stderr and the script exits 1:
  - Line 1: ``pyenv-virtualenv: `<v>' is not installed in pyenv.``
  - Line 2, when `python-build --definitions` lists `<v>` exactly: ``Run `pyenv install <v>' to install it.``
    Probe with `3.12.0`.
  - Line 2 otherwise: ``It does not look like a valid Python version. See `pyenv install --list' for available
    versions.`` Probe with `9.9.9`.

### Where the env goes, and the `versions/<name>` link

The full name is computed in this order [src bin/pyenv-virtualenv:376-391]:

1. **Base is `system`** (`${VERSION_NAME%/envs/*}` is `system`): the full name is the last component of the name. The
   env goes **directly** to `<root>/versions/<name>`, and no `envs` directory is created.
2. **Otherwise**, `pyenv-virtualenv-prefix` is computed for the base (with `PYENV_VERSION` = base):
   - If its parent is `${PYENV_ROOT}/versions`, so the base is itself an env of a pyenv version, the full name is
     `<that version>/envs/<name>`.
     - Probe: `pyenv virtualenv --without-pip venv1 venv5` created `3.13.12/envs/venv5`.
     - But it ran **venv1's** interpreter. The new `pyvenv.cfg` says `home = /usr/bin` and
       `command = <root>/versions/venv1/bin/python -m venv --without-pip …`.
     - So `pyenv virtualenv-prefix venv5` prints `/usr`, not `<root>/versions/3.13.12` [probe].
   - Otherwise the full name is `<base>/envs/<name>`.
3. `VIRTUALENV_PATH=<root>/versions/<full name>`.
4. **`COMPAT_VIRTUALENV_PATH=<root>/versions/<last component of name>` is always set.** The guard at line 389 is
   `"${VIRTUALENV_PATH/*/envs/*}" != "${PYENV_ROOT}/versions"`. In that expansion `*` replaces the whole value with the
   literal `envs/*`, so the comparison is always true [src bin/pyenv-virtualenv:389-391]. Consequences:
   - **Pre-check.** Without `-f`, an existing `COMPAT_VIRTUALENV_PATH` (file, directory or link, even a dangling one)
     stops the run: ``pyenv-virtualenv: `<root>/versions/<name>' already exists.`` on stderr, exit 1
     [src :393-400] [probe].
     - This applies whether the name was given short (`venv1`) or long (`3.13.12/envs/venv1`): both probes printed
       ``pyenv-virtualenv: `<root>/versions/venv1' already exists.``
     - It also catches a name that collides with an installed version.
   - **`-u` alone also fails here.** `UPGRADE` turns on `FORCE` only later (line 414), so
     `pyenv virtualenv --without-pip -u 3.13.12 venv1` failed with the same `already exists` message [probe].
   - **The y/N question is reachable only when the env exists but its link does not** (see "Existing env").
   - **A system-based env gets a link to itself inside itself.** For `system`, `COMPAT_VIRTUALENV_PATH` equals
     `VIRTUALENV_PATH`, and `ln -fsn <dir> <dir>` creates `<root>/versions/sysvenv/sysvenv -> <root>/versions/sysvenv`.
     Observed after `pyenv virtualenv --without-pip system sysvenv` [probe]. A second create of a system env without
     `-f` hits the `already exists` pre-check for the env directory itself.

### Choosing the backend: conda, venv or virtualenv

`detect_venv` runs after the checks above, with `PYENV_VERSION` = base [src bin/pyenv-virtualenv:148-184, 402-406]:

1. `prefix=$(pyenv-prefix)`. If `<prefix>/bin/conda` is executable, `HAS_CONDA=1`, and conda is used.
2. Otherwise:
   - `HAS_VIRTUALENV=1` if `<prefix>/bin/virtualenv` is executable.
   - The candidate interpreters are `python3 python python2` when the base is `system` [src :160-163] and `python`
     otherwise.
   - The first candidate for which `pyenv-exec <py> -m venv --help` succeeds sets `HAS_M_VENV=1` and
     `M_VENV_PYTHON_BIN`.
3. **venv is used only if** `HAS_M_VENV` is set, `HAS_VIRTUALENV` is not, and **no `-p`/`--python` was given**
   [src :180-182] [test test/pyvenv.bats:23,48,100,127].
   - **`-p` with a venv-only base therefore goes the virtualenv route.** It installs `virtualenv` with pip into the
     base. The message at line 454, ``pyenv-virtualenv: `--python=…' is not supported by `python -m venv'.``, is
     unreachable, because `USE_M_VENV` requires that no `-p` was given.
   - Probe: `pyenv virtualenv --python=python3 3.13.12 foo` on a base without pip printed
     `pyenv: pip: command not found` on stderr and exited **127**.

Backend-specific preparation [src bin/pyenv-virtualenv:413-487]:

- **`--upgrade`** sets `FORCE=1`. With venv, `UPGRADE` is cleared again and `--upgrade` is passed to `venv`
  [src :413-420].
- **Old bases, and every 3.1x base.** When `VIRTUALENV_VERSION` is unset and `PYENV_VERSION` (by then the resolved
  base name) matches `"3.0"*` or `"3.1"*`, `NO_ENSUREPIP=1`. For `"3.2"*` or `"stackless-3.2"*` it also sets
  `VIRTUALENV_VERSION=13.1.2` [src :422-437].
  - The pattern `"3.1"*` was meant for 3.1.x, but it also matches **3.10 to 3.19**. For every such base the plugin's
    own pip step (step 7 of "The creation run") is skipped.
  - The `--without-pip` flag is **not** passed, so `python -m venv` still runs its own ensurepip.
  - Evidence [probe]:
    - `pyenv virtualenv 3.12.99 fv1` (virtualenv route) ran with `GET_PIP_URL=file:///nonexistent/get-pip.py` on a host
      without `ensurepip`. The pip step would have printed `Installing pip from file:///nonexistent/get-pip.py...` to
      stderr. stderr was empty, and the env's `site-packages` stayed empty.
    - `pyenv virtualenv 3.13.12 failenv` likewise printed no such line (see the failure example).
  - So on modern bases the step runs only for 2.x and 3.2 to 3.9 bases.
- **conda.** A `-p`/`--python` value is reduced to its basename, minus a leading `python`, and passed as
  `python=<rest>` [src :439-447]. Probe: `-p python3.11` gave `python=3.11`.
- **venv.** `QUIET` and `VERBOSE` are dropped [src :449-452]. `-q`/`-v` are not passed to `venv`.
- **virtualenv.** The `-p` value is resolved as follows [src :457-476] [test test/python.bats:31,56,84,109]:
  - A bare name, or a path under `<root>/shims/`, is resolved with `pyenv-which <name>` in the base. If that fails,
    it is resolved in the last version that `pyenv-whence <name>` lists. The result is passed as `--python=<path>`.
  - Probe: `-p python3.13` on base 3.12.99 passed `--python=/usr/bin/python3.13`. That is the symlink target in
    version 3.13.12, as `pyenv-which` resolves it.
  - Not found: `not_installed_message <name>`, exit 1. Probe: ``pyenv-virtualenv: `nosuchpy' is not installed in
    pyenv.``
  - Any other path is passed through as `--python=<path>`. Probe: `--python=/usr/bin/python3`.
  - If the base has no `virtualenv`, `pyenv-exec pip install $QUIET $VERBOSE virtualenv[==$VIRTUALENV_VERSION]` runs
    **in the base version**, which modifies the base [src :477-485] [test test/pyvenv.bats:74].
- `VIRTUALENV_VERSION` is unset afterwards [src :491].

### Existing env, `-f`, and the y/N question

- `PREFIX_EXISTS=1` if `VIRTUALENV_PATH` is already a directory [src bin/pyenv-virtualenv:565].
- If `VIRTUALENV_PATH/bin` exists and `FORCE` is empty [src :569-578]:
  - stderr gets `pyenv-virtualenv: <VIRTUALENV_PATH> already exists`, with no quotes and no period;
  - then `read -p "continue with installation? (y/N) "`;
  - an answer starting with `y` or `Y` continues, anything else exits 1.
  - Probe, with an env made by hand and no link: answer `n` gave exit 1, and answer `y` gave exit 0 and created the
    missing `versions/<name>` link.
  - A piped answer with no trailing newline makes `read` fail, and `set -e` then exits 1 [probe].
- **`--upgrade` on an existing env** [src :580-587]. With `--no-setuptools`, `--no-pip` or `--without-pip`, it prints
  `pyenv-virtualenv: upgrading will not work with --no-setuptools or --no-pip` and exits 1. Otherwise it saves
  `pip freeze` of the env to the requirements file and **moves the env aside** to `<VIRTUALENV_PATH>.<YYYYmmddHHMMSS>.<pid>`.
  - Only the virtualenv route reaches this. venv clears `UPGRADE` at line 417, so `-f -u` with venv just reruns
    `python -m venv --upgrade` [probe: `pyenv virtualenv --without-pip -f -u 3.13.12 venv2` exited 0].

### The creation run

In order [src bin/pyenv-virtualenv:591-667]:

1. **`before_virtualenv` hooks.**
2. **Cleanup trap.** `trap cleanup SIGINT ERR`. `cleanup` removes `COMPAT_VIRTUALENV_PATH` if it is a symlink. If
   `PREFIX_EXISTS` is empty it also runs `rm -rf VIRTUALENV_PATH` [src :594-599].
3. **`cd` to the cache directory**, then run the backend. Its status goes to `STATUS` and does not trigger the trap.
   - conda:
     `pyenv-exec conda create $QUIET $VERBOSE --name <basename of VIRTUALENV_PATH> --yes <options…> --file <(pyenv-exec conda list python --full-name --export)`.
     The `--file` part is present only without `-p` [src :607-614].
     - Probe argv: `create --name cenv2 --yes --without-pip --file /dev/fd/63`, after
       `list python --full-name --export`.
     - conda decides where the env goes. The plugin assumes `<conda base>/envs/<name>`.
   - venv: `pyenv-exec <M_VENV_PYTHON_BIN> -m venv <options…> <VIRTUALENV_PATH>` [src :616-617].
     - Probe `pyvenv.cfg` `command` line:
       `<root>/versions/3.13.12/bin/python -m venv --without-pip <root>/versions/3.13.12/envs/venv1`.
     - For system the interpreter was `/usr/bin/python3`.
   - virtualenv: `pyenv-exec virtualenv $QUIET $VERBOSE <options…> <VIRTUALENV_PATH>` [src :619].
     Probe argv: `<root>/versions/3.12.99/envs/fv1`.
4. **`python*-config` links.** For each `<base prefix>/bin/python*-config`, it creates
   `<VIRTUALENV_PATH>/bin/<same name>` as an absolute symlink unless that name already exists [src :623-630].
   Probe: `bin/python3.13-config -> <root>/versions/3.13.12/bin/python3.13-config`.
5. **The `versions/<name>` link.** If `VIRTUALENV_PATH` is a directory, `ln -fsn <VIRTUALENV_PATH> <COMPAT>` runs, which
   replaces an existing link [src :633-636].
   - The link target is **absolute**, built from `PYENV_ROOT` as given: `<root>/versions/venv1 ->
     <root>/versions/3.13.12/envs/venv1` [probe].
   - For conda: `<root>/versions/cenv2 -> <root>/versions/miniconda3-4.7.12/envs/cenv2` [probe].
6. **`bin/pydoc`.** If `<VIRTUALENV_PATH>/bin/pydoc` does not exist, the plugin writes it with mode 755
   [src :638-647] [probe: `-rwxr-xr-x`]. The body is exactly:
   ```text
   #!<VIRTUALENV_PATH>/bin/python
   import pydoc
   if __name__ == '__main__':
         pydoc.cli()
   ```
   The last line is indented with 6 spaces. The shebang uses the long path, not the link.
7. **pip.** Unless `NO_ENSUREPIP` is set, it runs `PYENV_VERSION=<full name> build_package_ensurepip` and then
   `install_requirements` [src :649-655]:
   - `pyenv-exec python -s -m ensurepip 2>/dev/null`. stdout is not redirected. If that fails, `build_package_get_pip`
     runs [src :222-224].
   - `build_package_get_pip` [src :204-220]:
     - It writes `get-pip.py` into the cache dir: from `$GET_PIP` if that is set and is a file
       (`Installing pip from <GET_PIP>...` on stderr), otherwise downloaded from `$GET_PIP_URL`
       (`Installing pip from <URL>...` on stderr) with curl, or wget if there is no curl.
     - It then runs `pyenv-exec python -s get-pip.py $GET_PIP_OPTS`, with stdout sent to stderr.
     - On failure: `error: failed to install pip via get-pip.py` on stderr.
     - With neither curl nor wget: ``error: please install `curl' or `wget' and try again``, exit 1 [src :81-95].
   - **Default `GET_PIP_URL`** is chosen from the base name [src :503-540]:
     - `https://bootstrap.pypa.io/pip/<X.Y>/get-pip.py` for 2.6, 2.7, 3.2 to 3.6 (the base matches `X.Y` or `X.Y.*`);
     - `https://bootstrap.pypa.io/pip/get-pip.py` otherwise.
   - **`PIP_VERSION`** set: stderr gets `WARNING: Setting PIP_VERSION=<v> is no longer supported and may cause failures
     during the install process.`. `WARNING` is bold `\e[1m` when stdout is a terminal. The URL becomes
     `https://raw.githubusercontent.com/pypa/pip/<v>/contrib/get-pip.py`.
   - **`install_requirements`** reinstalls the frozen list (`pip install $QUIET $VERBOSE --requirement <file>`). On
     success it deletes the file and the moved-aside tree. On failure it prints a `PIP INSTALL FAILED` block that names
     the moved-aside tree and lists the packages, and its failure is ignored (`|| true`) [src :231-247, 654].
   - **There is no separate pip or setuptools upgrade.** pip is only *ensured*. `ensurepip` without `--upgrade`
     leaves an existing pip alone. The `ez_setup` code (`build_package_ez_setup`, `EZ_SETUP_URL`,
     `SETUPTOOLS_VERSION`) is defined but **never called** [src :186-202, 495-502; `grep` finds no call site].
   - The plugin pipes no output from ensurepip. On a host where it works, its stdout would reach the user
     (**UNCONFIRMED**: ensurepip was unavailable on the probe host, and 3.10+ bases skip this step anyway).
8. **`after_virtualenv` hooks.**
9. **Finish.** With `STATUS` 0 it runs `pyenv-rehash`. Otherwise it runs `cleanup`. It then exits with `STATUS`
   [src :661-667].

**A successful run prints nothing** on either stream for venv `--without-pip`, virtualenv and conda (the fakes print
nothing on create) [probe].

**Failure example** [probe]. `pyenv virtualenv 3.13.12 failenv` on this host:
- `venv`'s own message went to **stdout** ("The virtual environment was not created successfully because ensurepip is
  not available. … Failing command: <root>/versions/3.13.12/envs/failenv/bin/python").
- stderr was empty, and the exit code was 1.
- Afterwards `envs/failenv` and the `failenv` link were gone, and the empty `<root>/versions/3.13.12/envs/` directory
  remained.
- No `Installing pip from …` line appeared, because the base matches `"3.1"*`.
- The code explains the end state without the ERR trap [src :616-617, 661-667]:
  - `venv`'s status goes into `STATUS` through `||`;
  - the link and `pydoc` steps run;
  - the pip step is skipped;
  - `STATUS` is non-zero, so `cleanup` removes the link and, since the env is new, `rm -rf` removes the env;
  - the exit code is `venv`'s.
- Which of these intermediate steps ran on the partial directory was not traced.

### Environment variables

| Variable | Read | Set or exported |
|---|---|---|
| `PYENV_ROOT`, `PYENV_VERSION` | yes | `PYENV_VERSION` exported as the base [src :351] |
| `PYENV_VIRTUALENV_CACHE_PATH`, `PYTHON_BUILD_CACHE_PATH` | yes | — |
| `TMPDIR` | yes | — |
| `VIRTUALENV_VERSION` | yes | unset before the backend runs [src :491] |
| `GET_PIP`, `GET_PIP_URL`, `GET_PIP_OPTS`, `PIP_VERSION` | yes (pip step) | `PIP_VERSION` unset [src :510] |
| `EZ_SETUP`, `EZ_SETUP_URL`, `SETUPTOOLS_VERSION`, `EZ_SETUP_OPTS` | read, but the code that uses them is dead | — |
| `PIP_REQUIRE_VENV`, `PIP_REQUIRE_VIRTUALENV`, `VIRTUALENV_PYTHON` | — | unset [src :30-31, 260] |
| `PYENV_DEBUG` | yes | — |

---

## `pyenv virtualenvs` — `bin/pyenv-virtualenvs`

```text
# Summary: List all Python virtualenvs found in `$PYENV_ROOT/versions/*'.
# Usage: pyenv virtualenvs [--bare] [--skip-aliases]
#
# List all virtualenvs found in `$PYENV_ROOT/versions/*' and its `$PYENV_ROOT/versions/envs/*'.
```
[src bin/pyenv-virtualenvs:2-6]

- **Arguments** [src :18-31]. Every argument is checked:
  - `--complete` prints `--bare` and `--skip-aliases` and exits 0, wherever it appears.
  - `--bare` and `--skip-aliases` set flags.
  - Anything else prints `pyenv-help --usage virtualenvs` (`Usage: pyenv virtualenvs [--bare] [--skip-aliases]`) on
    stderr and exits 1 [probe with `--foo`].
- **What is listed** [src :87-128] [probe]:
  1. Every directory `<root>/versions/*/envs/*`, dotfiles included, as `<version>/envs/<name>`, version-sorted.
  2. Then every directory entry of `<root>/versions/*`, version-sorted:
     - a **symlink** to a directory is printed unless `--skip-aliases` is given (any link, not only env links);
     - a non-link directory is printed if it has `bin/activate`. That covers venv envs made from `system`, and also a
       conda base, which has `bin/activate`.
- **Line formats** (non-bare) [src :67-85]:
  - link: `<name> --> <readlink target>` (one level);
  - non-link: `<name> (created from <pyenv-virtualenv-prefix output>)`;
  - the line starts with `* ` if the name equals one of the `:`-separated `pyenv-version-name` entries, and then
    ends with ` (set by <pyenv-version-origin>)`; otherwise it starts with two spaces.
  - `--bare` prints only the names.
- **Probe output** (no version selected, so no `*`):

  ```text
    3.13.12/envs/ssp (created from <root>/versions/3.13.12)
    3.13.12/envs/venv1 (created from <root>/versions/3.13.12)
    3.13.12/envs/venv5 (created from /usr)
    miniconda3-4.7.12/envs/cenv (created from <root>/versions/miniconda3-4.7.12)
    miniconda3-4.7.12 (created from <root>/versions/miniconda3-4.7.12)
    ssp --> <root>/versions/3.13.12/envs/ssp
    sysvenv (created from /usr)
    venv1 --> <root>/versions/3.13.12/envs/venv1
  ```
  (some lines omitted). With `PYENV_VERSION=venv1`, only the link line is marked:
  `* venv1 --> <root>/versions/3.13.12/envs/venv1 (set by PYENV_VERSION environment variable)`. With
  `PYENV_VERSION=3.13.12/envs/venv1:venv2`, both matching lines are marked [probe]. Upstream's test expects
  `system_venv (created from /usr)` for a venv whose `pyvenv.cfg` says `home = /usr/bin`
  [test test/virtualenvs.bats:42-56].
- No env at all prints nothing and exits 0 [src].

---

## `pyenv virtualenv-prefix` — `bin/pyenv-virtualenv-prefix`

```text
# Summary: Display real_prefix for a Python virtualenv version
# Usage: pyenv virtualenv-prefix [<virtualenv>]
```
[src bin/pyenv-virtualenv-prefix:3-4]

- **No completion support.** There is no `--complete` branch, and `pyenv completions virtualenv-prefix` prints only
  `--help` [probe]. `--complete` passed as an argument would be treated as a version name.
- **Readlink.** If the script is itself a symlink, it needs `greadlink` or `readlink`, else
  `pyenv: cannot find readlink - are you missing GNU coreutils?`, exit 1 [src :9-21]. It then sources
  `libexec/pyenv-virtualenv-realpath` [src :23].
- **Versions.** With arguments, all of them are joined with `:` and exported as `PYENV_VERSION`. Without arguments,
  `pyenv-version-name` is split on `:` [src :29-39].
- **Per version, in order** [src :51-110]:
  1. `system`: ``pyenv-virtualenv: version `system' is not a virtualenv``, exit 1.
  2. `P=<root>/versions/<v>`. If that is not a directory, `P=$(pyenv-prefix <v>)`. For an unknown name this prints
     core's ``pyenv: version `<v>' not installed`` and exits 1 [probe with `nosuch`].
  3. `P/bin/python` not executable: ``pyenv-virtualenv: `python' not found in version `<v>'``, exit 1 [probe].
  4. `P/bin/activate` is a file:
     - with `P/bin/conda` a file: the result is `P` (a conda base);
     - else `<root>/versions/<v>/pyvenv.cfg` exists: the result is the `home` value with one trailing `/bin` removed.
       It is read with `cut -b 1-1024 | sed -n '/^ *home *= */s///p'`, so a `home` line indented with spaces counts,
       and two `home` lines yield both, joined by a newline;
     - else (old virtualenv): the result is the content of the first `orig-prefix.txt` found by
       `find <libdir>/ -maxdepth 2`, where `<libdir>` is `Lib` (jython), `lib-python` (pypy) or `lib`. Nothing found
       leaves the result empty, and then error 6 below applies.
  5. Else, if `P/conda-meta` is a directory, the result is `realpath P/../..` (a conda env).
  6. Else, or if the result is not a directory: ``pyenv-virtualenv: version `<v>' is not a virtualenv``, exit 1.
- **Output.** The results are joined with `:` on one line [src :112-115]. All messages go to stderr.

Probes (stdout, exit 0 unless noted):

| Argument | Result |
|---|---|
| `venv1` or `3.13.12/envs/venv1` | `<root>/versions/3.13.12` |
| `ssp` (created with `--system-site-packages`) | `<root>/versions/3.13.12` |
| `venv5` (created from venv1's interpreter) | `/usr` |
| `sysvenv` (from system) | `/usr` |
| `miniconda3-4.7.12` (conda base with `bin/activate`) | `<root>/versions/miniconda3-4.7.12` |
| `miniconda3-4.7.12/envs/cenv` | `<root>/versions/miniconda3-4.7.12` |
| `venv1 venv2` | `<root>/versions/3.13.12:<root>/versions/3.13.12` |
| `3.13.12`, `3.12.99`, `system`, `venv1 3.13.12` | ``pyenv-virtualenv: version `<v>' is not a virtualenv``, exit 1 |
| none, with `PYENV_VERSION=venv1` | `<root>/versions/3.13.12` |
| none, nothing selected | ``pyenv-virtualenv: version `system' is not a virtualenv``, exit 1 |

---

## `pyenv virtualenv-delete` — `bin/pyenv-virtualenv-delete`

```text
# Summary: Uninstall a specific Python virtualenv
#
# Usage: pyenv virtualenv-delete [-f|--force] <virtualenv>
#
#    -f  Attempt to remove the specified virtualenv without prompting
#        for confirmation. If the virtualenv does not exist, do not
#        display an error message.
#
# See `pyenv virtualenvs` for a complete list of installed versions.
```
[src bin/pyenv-virtualenv-delete:3-11]

- **`--complete`** (first argument) runs `exec pyenv virtualenvs --bare`, through the dispatcher [src :21-23].
- **Help.** `-h` or `--help` as the first argument prints the help on stdout and exits 0 [src :38-40] [probe].
- **Force.** `-f` or `--force` is accepted **only as the first argument** [src :42-46].
- **Usage errors.** Exactly one argument must remain, and it may not be empty or start with `-`. Otherwise the help
  text goes to **stderr** and the exit code is 1 [src :48-55]. Probed with no argument, `a b` and `-x`.
- **Resolving what to delete** [src :57-85]. `VERSION_NAME` is the last path component, and
  `ENV_COMPAT_PREFIX=<root>/versions/<VERSION_NAME>`.
  - **Long name** (contains `/envs/`): `ENV_PREFIX=<root>/versions/<arg>`. The compat link is kept as a deletion target
    only if it is a symlink whose one-level `readlink` equals `ENV_PREFIX` exactly [test test/delete.bats:41,57].
  - **Short name that is a symlink**: `ENV_PREFIX` = its one-level `readlink`. If that target is not of the form
    `<root>/versions/*/envs/*`, stderr gets ``pyenv-virtualenv: `<root>/versions/<name>' is a symlink for unknown
    location.`` and the exit code is 1 [probe].
  - **Short name, not a symlink**: if `pyenv-virtualenv-prefix <name>` succeeds, `ENV_PREFIX=<root>/versions/<name>` and
    there is no compat link (a system-based env).
    - Otherwise, without `-f`: ``pyenv-virtualenv: `<arg>' is not a virtualenv.`` on stderr, exit 1. Probed with
      `3.13.12` and `nosuch`.
    - With `-f` it falls through with `ENV_PREFIX` empty.
- **Missing target** [src :87-94]. If `ENV_PREFIX` is not a directory:
  - without `-f`: ``pyenv-virtualenv: virtualenv `<VERSION_NAME>' not installed`` on stderr, exit 1. Probe:
    `3.13.12/envs/nosuch` gave ``virtualenv `nosuch' not installed``;
  - with `-f`: exit 0, silently. Probed with `-f 3.13.12`, `-f nosuch` and `-f 3.13.12/envs/nosuch`. So
    `-f <base version>` is a silent no-op, never a deletion.
- **Prompt.** Without `-f`: `read -p "pyenv-virtualenv: remove <ENV_PREFIX>? (y/N) "`. An answer starting with `y` or
  `Y` continues, and anything else exits 1 [src :96-102]. Probe: `yes` and `Yup` deleted, and `n` exited 1.
- **Delete** [src :104-108]: `rm -rf <ENV_PREFIX>`, then `rm -rf <ENV_COMPAT_PREFIX>` if it is a symlink, then
  `pyenv-rehash`. Nothing is printed on success, and the exit code is 0 [probe].
  - `pyenv virtualenv-delete venv2` removed both `3.13.12/envs/venv2` and the `venv2` link.
  - `pyenv virtualenv-delete 3.13.12/envs/delme2` removed both too.
  - `pyenv virtualenv-delete -f sysvenv` removed `<root>/versions/sysvenv`.
- An empty `<base>/envs/` directory is left behind [probe].

---

## `pyenv activate` / `pyenv deactivate` without the shell function — `bin/pyenv-activate`, `bin/pyenv-deactivate`

- **`pyenv-activate --complete`** prints `--unset`, then execs `pyenv-virtualenvs --bare` [src bin/pyenv-activate:17-21].
  This is the file that `pyenv completions activate` finds first, since `pyenv-completions` tries `pyenv-<cmd>` before
  `pyenv-sh-<cmd>` [src pyenv:libexec/pyenv-completions:18].
- **Any other call** prints to stderr and exits 1 [src bin/pyenv-activate:23-30] [probe]:
  ```text
  ^[[31;1m
  `pyenv activate' requires Pyenv and Pyenv-Virtualenv to be loaded into your shell.
  Check your shell configuration and Pyenv and Pyenv-Virtualenv installation instructions.

  ^[[0m
  ```
  `^[` is ESC. The stream starts with `ESC[31;1m` and a newline, and ends with `ESC[0m` and **no** trailing newline.
- **`pyenv-deactivate`** is the same, with `` `pyenv deactivate' `` in the first line. It has no `--complete` branch
  [src bin/pyenv-deactivate:12-19] [test test/deactivate.bats:377].
- With the pyenv shell function loaded, `pyenv activate` and `pyenv deactivate` never reach these stubs. The function
  routes every command listed by `pyenv-commands --sh` to `eval "$(pyenv "sh-$command" "$@")"`
  [src pyenv:libexec/pyenv-init:519,570-572]. Probe: the function's `pyenv activate venv1` activated (see "End-to-end").

## `pyenv sh-activate` — `bin/pyenv-sh-activate`

Help is shared with `activate`. `pyenv help sh-activate` prints the same text as `pyenv help activate` [probe]:

```text
Usage: pyenv activate <virtualenv>
       pyenv activate --unset

Activate a Python virtualenv environment in current shell.
This acts almost as same as `pyenv shell`, but this invokes the `activate`
script in your shell.

<virtualenv> should be a string matching a Python version known to pyenv.
```
[src bin/pyenv-sh-activate:3-12]

### Options [src :49-74]

The options are read until the first non-option:

| Option | Effect |
|---|---|
| `--complete` | prints `--unset`, then execs `pyenv-virtualenvs --bare` |
| `-f`, `--force` | `FORCE=1` |
| `-q`, `--quiet` | `QUIET=1` |
| `--unset` | `exec pyenv-sh-deactivate`, which also drops any arguments after it |
| `-v`, `--verbose` | clears `QUIET`, sets `PYENV_VIRTUALENV_VERBOSE_ACTIVATE=1` |
| anything else | ends option parsing, including unknown options |

- **Unknown options are taken as env names.** Probe: `pyenv sh-activate --bogus venv1` failed with
  ``pyenv-virtualenv: version `--bogus' is not a virtualenv``.
- **No completion through `pyenv completions`.** The `# Provide pyenv completions` comment is indented (line 52),
  while `pyenv-completions` greps for `^#`. So `pyenv completions sh-activate` prints only `--help` [probe]
  [src pyenv:libexec/pyenv-completions:23].

### Which env, and when it refuses [src :76-157]

1. **`versions`** = the arguments. With no argument, `no_shell=1` and `versions` = `pyenv-version-name` split on `:`
   [src :81-88].
2. **`PYENV_VIRTUALENV_INIT` empty** forces `no_shell` back to empty [src :90-94]. So without `virtualenv-init` the
   `PYENV_VERSION` export is always printed.
3. **`venv`** = the first entry [src :96].
4. **Third-party venv.** If `VIRTUAL_ENV` is set and differs from `PYENV_VIRTUAL_ENV`, or `PYENV_VIRTUAL_ENV` is
   empty, and there is no `-f` [src :98-109]:
   - stderr gets ``pyenv-virtualenv: virtualenv `<VIRTUAL_ENV>' is already activated`` (unless `-q`);
   - stdout gets `true`;
   - the exit code is **0**.
   - Probed with `VIRTUAL_ENV=/opt/other`, with and without a different `PYENV_VIRTUAL_ENV`
     [test test/activate.bats:468,482].
5. **Not an env** [src :111-125]. If `pyenv-virtualenv-prefix <venv>` fails, it tries
   `<first current version minus /envs/…>/envs/<venv>`.
   - Probe: with `PYENV_VERSION=3.13.12`, `pyenv sh-activate nolink` activated `3.13.12/envs/nolink`, which has no
     link.
   - If that also fails: ``pyenv-virtualenv: version `<venv>' is not a virtualenv`` on stderr (unless `-q`), `false`
     on stdout, exit 1. Probed with `3.13.12`, `nosuch`, nothing selected (`system`), and `PYENV_VERSION=venv1` with
     `nolink`.
6. **Multiple envs** [src :129-139]. If any other entry of `versions` is also an env:
   ``pyenv-virtualenv: cannot activate multiple versions at once: <versions joined by spaces>`` on stderr (unless
   `-q`), `false` on stdout, exit 1.
   - Probe: `venv1 venv2` fails.
   - **Non-env extra entries are allowed.** `pyenv sh-activate venv1 3.13.12` succeeded with
     `export PYENV_VERSION="venv1:3.13.12";`.
   - The first entry must be the env: `3.13.12 venv1` failed with ``version `3.13.12' is not a virtualenv``.
7. **Shell** = `${PYENV_SHELL:-${SHELL##*/}}` [src :141].
   - Only `fish` gets fish syntax. Every other value, including `zsh`, `ksh` and `pwsh`, gets the POSIX form. The
     probe output for zsh, ksh and pwsh was byte-identical to bash.
   - There is no PowerShell syntax. So under the pwsh `pyenv` function from `pyenv init - pwsh`, which `iex`es the
     output, the lines would be POSIX `export` statements. Not run in pwsh: **UNCONFIRMED** how pwsh reacts.
8. **Prefix** = `pyenv-prefix <venv>`. If that is a symlink, it is resolved **one level** with `readlink` [src :142-146].
   - So for `venv1`, `VIRTUAL_ENV` is the real `<root>/versions/3.13.12/envs/venv1`, not the link path.
   - For a system env, which is not a link, it is `<root>/versions/sysvenv` [probe].
9. **Already active** [src :149-157]. If `VIRTUAL_ENV` equals the prefix and there is no `-f`: stderr gets
   ``pyenv-virtualenv: version `<venv>' is already activated`` (unless `-q`), stdout gets `true`, exit 0 [probe].
   - Probe: `VIRTUAL_ENV=<root>/versions/venv1`, the link path, does **not** count as already active, and it
     re-activates.

### What it prints on success

Output order [src :159-305]:

1. **The full `sh-deactivate --force --quiet` output.** It is not captured, so it goes to stdout first [src :159].
   - Probe: every successful `sh-activate` starts with the 17 POSIX lines of the deactivate script, or the fish
     equivalent (see `sh-deactivate`).
   - `PYENV_ACTIVATE_SHELL` and `CONDA_DEFAULT_ENV` inherited by that child process add their own `unset` lines.
2. **`before_activate` hooks.** With `-v` (or `PYENV_VIRTUALENV_VERBOSE_ACTIVATE` set), stderr gets
   `pyenv-virtualenv: activate <venv>` [src :164-166].
   - **The variable leaks.** With `PYENV_VIRTUALENV_VERBOSE_ACTIVATE=1` in the environment and `-q`, stderr got
     `pyenv-virtualenv: deactivate ` (empty name, trailing space) from the nested deactivate, then
     `pyenv-virtualenv: activate venv1` [probe].
3. **Unless `no_shell`** [src :168-188]:
   - POSIX:
     ```sh
     export PYENV_VERSION="<versions joined by :>";
     export PYENV_ACTIVATE_SHELL=1;
     ```
   - fish:
     ```fish
     set -gx PYENV_VERSION "<…>";
     set -gx PYENV_ACTIVATE_SHELL 1;
     ```
   - Probe: with `PYENV_VIRTUALENV_INIT=1` and no argument these lines are absent. With `PYENV_VIRTUALENV_INIT` unset
     and no argument they are present, as `PYENV_VERSION="venv1"`.
4. **The env variables** [src :191-204]:
   - POSIX: `export PYENV_VIRTUAL_ENV="<prefix>";` and `export VIRTUAL_ENV="<prefix>";`
   - fish: `set -gx PYENV_VIRTUAL_ENV "<prefix>";` and `set -gx VIRTUAL_ENV "<prefix>";`
5. **conda.** If `<prefix>/conda-meta` is a directory or `<prefix>/bin/conda` is executable [src :207-222]:
   - `CONDA_DEFAULT_ENV` is the part of the venv name after `/envs/` when the prefix contains `/envs/`, and `root`
     otherwise;
   - it is printed as `export CONDA_DEFAULT_ENV="<x>";`, or `set -gx CONDA_DEFAULT_ENV "<x>";` for fish.
   - Probe: `cenv`, `cenv2` and `root`.
6. **`PYTHONHOME` non-empty** [src :224-239]:
   - POSIX: `export _OLD_VIRTUAL_PYTHONHOME="<value>";` then `unset PYTHONHOME;`.
   - fish: `set -gx _OLD_VIRTUAL_PYTHONHOME "<value>";` then `set -e PYTHONHOME;` [probe].
7. **Prompt** [src :241-276]. Let `D` = `PYENV_VIRTUALENV_DISABLE_PROMPT`, else `PYENV_VIRTUAL_ENV_DISABLE_PROMPT`,
   else `VIRTUAL_ENV_DISABLE_PROMPT`. Any non-empty value of any of the three disables the prompt [probe]. If `D` is
   empty:
   - **POSIX, always, even with `-q`:**
     ```sh
     export _OLD_VIRTUAL_PS1="${PS1:-}";
     export PS1="<P> ${PS1:-}";
     ```
     - `<P>` is `(<venv>)` by default. Otherwise it is `PYENV_VIRTUALENV_PROMPT`, with only the **first** `{venv}`
       replaced.
     - Probe: `[{venv}]` gave `[venv1]`, and `<{venv}|{venv}>` gave `<venv1|{venv}>`.
     - `<venv>` is the name as given: `(3.13.12/envs/venv1)` for the long name, and `(3.13.12/envs/nolink)` after the
       fallback in step 5.
   - **fish, only without `-q`.** It emits a `fish_prompt` wrapper [src :247-261] [probe]:
     ```fish
     functions -e _pyenv_old_prompt              # remove old prompt function if exists. 
                                                 # since everything is in memory, it's safe to
                                                 # remove it.
     functions -c fish_prompt _pyenv_old_prompt  # backup old prompt function

     # from python-venv
     function fish_prompt
         set -l prompt (_pyenv_old_prompt)       # call old prompt function first since it might 
                                                 # read exit status
         echo -n "(<venv>) "                    # add virtualenv to prompt
         string join -- \n $prompt              # handle multiline prompts
     end
     ```
     The first and sixth lines end with a trailing space. `PYENV_VIRTUALENV_PROMPT` is ignored for fish.
8. **conda scripts** [src :279-302]. Same conda condition as step 5.
   - POSIX: `export CONDA_PREFIX="<prefix>";`, then `. "<script>";` for each `<prefix>/etc/conda/activate.d/*.sh`, then
     for each `<prefix>/etc/profile.d/*.sh`.
   - fish: `source "<script>";` for each `<prefix>/etc/fish/conf.d/*.fish`.
   - Probe output for `miniconda3-4.7.12/envs/cenv`, bash, after the prompt lines:
     ```sh
     export CONDA_PREFIX="<root>/versions/miniconda3-4.7.12/envs/cenv";
     . "<root>/versions/miniconda3-4.7.12/envs/cenv/etc/conda/activate.d/a.sh";
     . "<root>/versions/miniconda3-4.7.12/envs/cenv/etc/profile.d/conda.sh";
     ```
9. **`after_activate` hooks.** Exit 0.

**`sh-activate` never changes `PATH`.** It does not source the env's own `bin/activate` either. The env's `bin` is
reached through pyenv's shims, because `PYENV_VERSION`, or the file version for auto-activation, names the env. Probe:
after `pyenv activate venv1`, `command -v python` was `<root>/shims/python`, and `sys.prefix` was
`<root>/versions/venv1`.

## `pyenv sh-deactivate` — `bin/pyenv-sh-deactivate`

```text
Usage: pyenv deactivate

Deactivate a Python virtual environment.
```
[src bin/pyenv-sh-deactivate:3-7]

- **Options** [src :40-57]: `-f`/`--force`, `-q`/`--quiet`, and `-v`/`--verbose` (clears `QUIET`, sets
  `PYENV_VIRTUALENV_VERBOSE_ACTIVATE=1`). Option parsing stops at the first other word. **Extra arguments are
  ignored** [probe: `pyenv sh-deactivate extra args` deactivated normally].
- **No `--complete` branch.** `pyenv sh-deactivate --complete` simply runs: with nothing active it printed `false` on
  stdout and the "no virtualenv" message on stderr, exit 1 [probe]. `pyenv completions sh-deactivate` and
  `pyenv completions deactivate` print only `--help` [probe].
- **Nothing active.** With `VIRTUAL_ENV` empty and no `-f`: stderr gets `pyenv-virtualenv: no virtualenv has been
  activated.` (unless `-q`), stdout gets `false`, exit **1** [src :59-67] [probe].
- **Shell** = `basename "${PYENV_SHELL:-$SHELL}"` [src :69]. Only `fish` is special.
- **Name for messages** [src :70-76]: `<VIRTUAL_ENV minus "<root>/versions/">` when `VIRTUAL_ENV` has the form
  `<root>/versions/*/envs/*`, else its last component. With `-v`, stderr gets `pyenv-virtualenv: deactivate <name>`
  [src :81-83].
  - Probes: `deactivate 3.13.12/envs/venv1`, `deactivate other` for `/opt/other`, and
    `deactivate miniconda3-4.7.12/envs/cenv`.
- **It deactivates any `VIRTUAL_ENV`**, including a third-party one [probe: `VIRTUAL_ENV=/opt/other`].

Output, in order (POSIX form; fish form after the table) [src :85-225]:

| Condition | POSIX lines |
|---|---|
| prefix has `conda-meta/` or an executable `bin/conda` | `. "<script>";` per `<prefix>/etc/conda/deactivate.d/*.sh`, then `unset CONDA_PREFIX`, which has **no** semicolon (fish: nothing) |
| `PYENV_ACTIVATE_SHELL` non-empty | `unset PYENV_VERSION;` and `unset PYENV_ACTIVATE_SHELL;` |
| always | `unset PYENV_VIRTUAL_ENV;` and `unset VIRTUAL_ENV;` |
| `CONDA_DEFAULT_ENV` non-empty | `unset CONDA_DEFAULT_ENV;` |
| always | the `_OLD_VIRTUAL_PATH`, `_OLD_VIRTUAL_PYTHONHOME`, `_OLD_VIRTUAL_PS1` and `deactivate` blocks below |

The unconditional tail, exactly [probe]:

```sh
if [ -n "${_OLD_VIRTUAL_PATH:-}" ]; then
  export PATH="${_OLD_VIRTUAL_PATH}";
  unset _OLD_VIRTUAL_PATH;
fi;
if [ -n "${_OLD_VIRTUAL_PYTHONHOME:-}" ]; then
  export PYTHONHOME="${_OLD_VIRTUAL_PYTHONHOME}";
  unset _OLD_VIRTUAL_PYTHONHOME;
fi;
if [ -n "${_OLD_VIRTUAL_PS1:-}" ]; then
  export PS1="${_OLD_VIRTUAL_PS1}";
  unset _OLD_VIRTUAL_PS1;
fi;
if declare -f deactivate 1>/dev/null 2>&1; then
  unset -f deactivate;
fi;
```

The fish tail [probe]:

```fish
if [ -n "$_OLD_VIRTUAL_PATH" ];
  set -gx PATH "$_OLD_VIRTUAL_PATH";
  set -e _OLD_VIRTUAL_PATH;
end;
if [ -n "$_OLD_VIRTUAL_PYTHONHOME" ];
  set -gx PYTHONHOME "$_OLD_VIRTUAL_PYTHONHOME";
  set -e _OLD_VIRTUAL_PYTHONHOME;
end;
# check if old prompt function exists
if functions -q _pyenv_old_prompt
  # remove old prompt function if exists.
  functions -e fish_prompt
  functions -c _pyenv_old_prompt fish_prompt
  functions -e _pyenv_old_prompt
end
if functions -q deactivate;
  functions -e deactivate;
end;
```

fish's earlier lines use `set -e <VAR>;` in place of `unset <VAR>;`.

- **`_OLD_VIRTUAL_PS1` empty.** When `_OLD_VIRTUAL_PS1` is empty, `PS1` is not restored. An activation from an empty
  `PS1` therefore leaves `PS1="(env) "` after deactivation [src :202-205]. Not probed: **UNCONFIRMED** in a live shell.
- **The env's own activate.** `_OLD_VIRTUAL_PATH` and `deactivate` belong to the env's own `bin/activate`.
  `sh-deactivate` cleans them up, so a third-party `source env/bin/activate` is undone too.
- **Exit code** 0 [probe].

## `pyenv virtualenv-init` — `bin/pyenv-virtualenv-init`

```text
# Summary: Configure the shell environment for pyenv-virtualenv
# Usage: eval "$(pyenv virtualenv-init - [<shell>])"
#
# Automatically activates a Python virtualenv environment based on current
# pyenv version.
```
[src bin/pyenv-virtualenv-init:2-6]

- **No `--complete`.** `pyenv completions virtualenv-init` prints only `--help` [probe].
- **Arguments** [src :49-58]:
  - Every `-` in the argument list sets print mode, and **each `-` shifts away the first positional argument,
    whichever it is**. Then `shell=${1:-$PYENV_SHELL}`.
  - Probes: `virtualenv-init - bash` and `virtualenv-init - - zsh` work. `virtualenv-init bash -` shifts `bash` away
    and leaves `shell=-`, which prints only the two generic lines.
- **Detection** without a shell [src :59-66]: `ps -p $PPID -o args=`, strip one leading `-`, keep the text up to the
  first space, fall back to `$SHELL`, take the basename, keep the text before the first `-`.
  - This differs from core's `/proc/$PPID/cmdline` method (M3 "Shell detection").
  - Under the dispatcher `$PPID` is the caller of `pyenv`. Probe: from a bash script it detected `bash`
    [test test/init.bats:5-30].
- **Help mode** (no `-`) [src :68-102]. It prints to **stderr** and exits **1** [probe]:
  ```text
  # Load pyenv-virtualenv automatically by adding
  # the following to <profile>:

  eval "$(pyenv virtualenv-init -)"

  ```
  - `<profile>`: bash `~/.bashrc`, zsh `~/.zshrc`, ksh `~/.profile`, fish `~/.config/fish/config.fish`, anything else
    (pwsh and tcsh probed) `your profile`.
  - For fish the command line is `status --is-interactive; and source (pyenv virtualenv-init -|psub)`.
- **Init-time decisions** [src :14-27, 47]:
  - **The stat format** is `-L -c %Y` if `stat -L -c %Y /` works (GNU), else `-L -f %m` (BSD).
  - **Version-name hooks.** If `pyenv hooks version-name` prints anything, `_has_version_hooks=1`, and the hook
    function is emitted **without** the caching blocks. Probe: a dummy `<root>/pyenv.d/version-name/x.bash` removed
    them for bash and fish.
  - **The install prefix** is the plugin's root directory, with symlinks resolved. `PYENV_VIRTUALENV_ROOT`, if set,
    overrides it in the `PATH` line [probe: `/opt/pvroot/shims`].

### Print mode output, per shell (exit 0)

**Every shell** gets the PATH and flag lines first [src :104-119] [probe]:
- Every shell except fish:
  ```sh
  export PATH="<plugin>/shims:${PATH}";
  export PYENV_VIRTUALENV_INIT=1;
  ```
- fish:
  ```fish
  while set index (contains -i -- "<plugin>/shims" $PATH)
  set -eg PATH[$index]; end; set -e index
  set -gx PATH '<plugin>/shims' $PATH;
  set -gx PYENV_VIRTUALENV_INIT 1;
  ```

The POSIX line prepends **unconditionally**, so re-evaluating it duplicates the entry. fish removes the old entry
first.

**bash and zsh** then get the hook function [src :186-247] [test test/init.bats:44-97] [probe]:

```sh
_pyenv_virtualenv_hook() {
  local ret=$?
  if [ "${PYENV_VERSION-}" = "${_PYENV_VH_VERSION-}" ] \
    && [ "${VIRTUAL_ENV-}" = "${_PYENV_VH_VENV-}" ]; then
    if [ -n "${PYENV_VERSION-}" ]; then
      return $ret
    fi
    if [ "${PWD}" = "${_PYENV_VH_PWD-}" ] \
      && [ "$(stat -L -c %Y "${_PYENV_VH_PATHS[@]}" 2>/dev/null)" = "${_PYENV_VH_MTIMES-}" ]; then
      return $ret
    fi
  fi
  if [ -n "${VIRTUAL_ENV-}" ]; then
    eval "$(pyenv sh-activate --quiet || pyenv sh-deactivate --quiet || true)" || true
  else
    eval "$(pyenv sh-activate --quiet || true)" || true
  fi
  _PYENV_VH_PWD="${PWD}"
  _PYENV_VH_VERSION="${PYENV_VERSION-}"
  _PYENV_VH_VENV="${VIRTUAL_ENV-}"
  local _pvh_d="${PWD}" _pvh_found_local=0
  _PYENV_VH_PATHS=()
  while :; do
    if [ -f "${_pvh_d}/.python-version" ] || [ -L "${_pvh_d}/.python-version" ]; then
      _PYENV_VH_PATHS+=("${_pvh_d}/.python-version")
      if [ -f "${_pvh_d}/.python-version" ]; then 
        _pvh_found_local=1
        break
      fi
    else
      _PYENV_VH_PATHS+=("${_pvh_d}")
    fi
    [ "${_pvh_d}" = "/" ] && break
    _pvh_d="${_pvh_d%/*}"
    [ -z "${_pvh_d}" ] && _pvh_d="/"
  done
  if [ "${_pvh_found_local}" = "0" ]; then
    _PYENV_VH_PATHS+=("${PYENV_ROOT}/version")
  fi
  _PYENV_VH_MTIMES="$(stat -L -c %Y "${_PYENV_VH_PATHS[@]}" 2>/dev/null)"
  return $ret
};
```

The line `if [ -f "${_pvh_d}/.python-version" ]; then ` has a trailing space. That is the GNU-stat form; on BSD it is
`-L -f %m`.

The hook is registered per shell [src :250-269] [probe]:
- bash:
  ```sh
  if ! [[ "${PROMPT_COMMAND-}" =~ _pyenv_virtualenv_hook ]]; then
    PROMPT_COMMAND="_pyenv_virtualenv_hook;${PROMPT_COMMAND-}"
  fi
  ```
- zsh:
  ```sh
  typeset -g -a precmd_functions
  if [[ -z $precmd_functions[(r)_pyenv_virtualenv_hook] ]]; then
    precmd_functions=(_pyenv_virtualenv_hook $precmd_functions);
  fi
  ```
- **fish** gets `function _pyenv_virtualenv_hook --on-event fish_prompt; … end` [src :121-184] [probe]. It has the same
  cache logic in fish syntax, but calls `pyenv activate --quiet; or pyenv deactivate --quiet; or true` (with
  `VIRTUAL_ENV` set) or `pyenv activate --quiet; or true`. That relies on the fish `pyenv` function.
- **ksh, pwsh, and any other name** get only the two PATH and flag lines: no hook function and no registration
  [src :266-268] [probe for ksh and pwsh]. Auto-activation exists only for bash, zsh and fish.
- The v1.4.0 changelog lists "POSIX sh (dash) compatibility in pyenv-virtualenv-init" (`CHANGELOG.md:4`). The change
  was not traced to specific lines, so what it fixed is **UNCONFIRMED**. Upstream's test that runs the init from a
  `#!/bin/sh` script expects no `PROMPT_COMMAND` line [test test/init.bats:12-20].

**What the hook does.**
- The cache short-circuit applies when `PYENV_VERSION` and `VIRTUAL_ENV` are unchanged since the last run, and either
  `PYENV_VERSION` is set, or `PWD` is unchanged and the mtimes are unchanged. The mtimes are those of every directory
  walked from `PWD` up to the nearest `.python-version` file, or of `<root>/version` if there is none.
- Otherwise it re-evaluates `pyenv sh-activate --quiet`, with no argument, so `no_shell=1` and `PYENV_VERSION` is not
  exported.
- That activates the env named by the current version, or prints `false`/`true`, which the `eval` runs as a command.
- When that fails and an env is active, it deactivates.

## `shims/activate`, `shims/deactivate`

- **When sourced** (`$0` differs from `BASH_SOURCE`), they run `eval "$(pyenv sh-activate --verbose "$@" || true)"`,
  and `sh-deactivate` respectively [src shims/activate:2-3, shims/deactivate:2-3].
- **When executed**, they print to stderr and return false [src shims/activate:4-6, shims/deactivate:4-6]:
  - ``pyenv-virtualenv: activate must be sourced. Run 'source activate envname' instead of 'activate envname'``
  - ``pyenv-virtualenv: deactivate must be sourced. Run 'source deactivate' instead of 'deactivate'``
- **Probe** (bash, after both inits). `command -v activate` was `<plugin>/shims/activate`, ahead of `<root>/shims`,
  which also holds an `activate` shim made from the envs' `bin/activate`.
  - `source activate venv1` printed `pyenv-virtualenv: activate venv1` on stderr and activated.
  - `source deactivate` printed `pyenv-virtualenv: deactivate 3.13.12/envs/venv1`.

## `libexec/pyenv-virtualenv-realpath`

It is sourced by `virtualenv-prefix` only. It loads core's `realpath` builtin from
`<plugin>/../../libexec/pyenv-realpath.dylib`, or uses an existing `realpath`. Otherwise it defines a shell `realpath`.
If that fallback is needed while `PYENV_NATIVE_EXT` is set, it prints ``pyenv: failed to load `realpath' builtin`` and
exits 1 [src libexec/pyenv-virtualenv-realpath:5-50]. It is used only for the conda-env case
(`realpath <prefix>/../..`).

---

## Hooks shipped in `etc/pyenv.d`

These are what rpyenv builds in (spec §10 "Command fallbacks" and "Uninstall").

### How `pyenv which` runs them at 2.8.8, and the consequence

`pyenv-which` resolves the command first, then sources the `which` hooks [src pyenv:libexec/pyenv-which:65-100]:
- **Loop.** It tries each selected version, then `system` (or `""` with `--nosystem`), and stops at the first
  executable.
- **The `version` variable.** For each non-system entry it sets `version=$(basename "$(pyenv-prefix <entry>)")`
  [src :83-87]. For `3.13.12/envs/ssp` that is `ssp`, and the hooks then look under `<root>/versions/ssp`, the link.
- **The system step** sets `PYENV_COMMAND_PATH=$(command -v <cmd>)` with the shims removed from `PATH`. That is the
  empty string when the command is not on the system `PATH` [src :76-80].
- **When the loop finds nothing**, the hooks therefore see the system step's empty `PYENV_COMMAND_PATH` and
  `version=system`, not the env's candidate path, unless `--nosystem` was given.
- **`pyenv exec`, and so every shim**, calls `pyenv-which <cmd>` without `--nosystem` [src pyenv:libexec/pyenv-exec:57].

The three hooks are written against a non-empty, env-based `PYENV_COMMAND_PATH`. At 2.8.8 the result is [probe]:

| Situation (`PYENV_VERSION` = env) | `pyenv which <cmd>` | `pyenv which <cmd> --nosystem` |
|---|---|---|
| `ssp` (`include-system-site-packages = true`), `basetool` only in base | prints **`<root>/versions/3.13.12/bin/`** (a directory), exit 0 | `<root>/versions/3.13.12/bin/basetool`, exit 0 |
| `ssp`, `nosuchcmd` in no version | prints `<root>/versions/3.13.12/bin/`, exit 0 | `pyenv: nosuchcmd: command not found`, exit 127 |
| `venv1` (no system site packages), `basetool` | not found, exit 127, whence advice lists `3.13.12` | (same, by the code) |
| `venv2` with its `python3.13-config` link removed | not found, exit 127 | `<root>/versions/3.13.12/bin/python3.13-config` |
| `cenv2` (conda env with a `versions/` link), `conda` | not found, exit 127 | `<root>/versions/miniconda3-4.7.12/bin/conda` |
| `miniconda3-4.7.12/envs/cenv` (no link), `conda` | not found, exit 127 | not found, exit 127 |

- `PYENV_VERSION=ssp pyenv exec basetool` failed with
  `<prefix>/libexec/pyenv-exec: line 97: <root>/versions/3.13.12/bin/: Is a directory` and exit **126** [probe].
- So, through the shims, at 2.8.8:
  - the `python-config` and `conda` fallbacks never take effect;
  - the system-site-packages fallback returns a directory for **every** command found neither in the env nor on the
    system `PATH`.

### `which/system-site-packages.bash`

Source: `etc/pyenv.d/which/system-site-packages.bash:6-54`. It runs only when `PYENV_COMMAND_PATH` is not executable.
- **Version.** `version=$(pyenv-version-name)`. With several versions selected, this is the whole `:`-joined string,
  because the assignment does not split. The script then needs `<root>/versions/<version>/bin/activate` to be a file.
- **conda envs** (`bin/conda` is a file) are skipped.
- **venv** (`pyvenv.cfg` exists): `prefix` = the `home` value minus `/bin`. The include flag is set if the file matches
  `include-system-site-packages *= *true`, ignoring case (`grep -q -i`).
- **old virtualenv**: the flag is set when **no** `no-global-site-packages.txt` is found within
  `find <libdir>/ -maxdepth 2`. `prefix` = the content of `orig-prefix.txt`.
- **Result.** If the flag is set and `prefix` is non-empty, it tries `<prefix>/bin/<basename of PYENV_COMMAND_PATH>`
  and uses it if `-x` passes. `-x` is also true for a directory, which is the 2.8.8 consequence above.

### `which/python-config.bash`

Source: `etc/pyenv.d/which/python-config.bash:6-43`.
- **Condition.** It runs only when `PYENV_COMMAND_PATH` is not executable and its basename matches `python*-config`.
- **Prefix.** The same version, activate and conda tests as above. `prefix` comes from `pyvenv.cfg` `home` or
  `orig-prefix.txt`, with no site-packages condition.
- **Result.** It uses `<prefix>/bin/<name>` if executable.
- **Mostly redundant.** Creation already links `python*-config` into each new env (see "The creation run", step 4), so
  the hook matters only for envs made by other tools.

### `which/conda.bash`

Source: `etc/pyenv.d/which/conda.bash:4-11`.
- **Condition.** It runs when `PYENV_COMMAND_PATH` is not executable, its basename is `conda`, and
  `<root>/versions/<version>/conda-meta` is a directory. `version` is pyenv-which's loop variable (see above).
- **Result.** It uses `$(pyenv-virtualenv-prefix <version>)/bin/conda` if executable. That is the conda base that owns
  the env.

### `rehash/envs.bash`

Source: `etc/pyenv.d/rehash/envs.bash:1-11`. If `make_shims` is defined, it calls `make_shims` with the sorted, unique
basenames of `<root>/versions/*/envs/*/bin/*`.
- At 2.8.8 core already includes `versions/*/envs/*/bin/*` in `pyenv-versions --executables`
  [src pyenv:libexec/pyenv-versions:47], which `pyenv-rehash` feeds to `make_shims`
  [src pyenv:libexec/pyenv-rehash:208]. The hook adds no new names [src].
- **Every file in an env's `bin` becomes a shim.** Probe `<root>/shims` after creating venvs: `Activate.ps1`,
  `activate`, `activate.csh`, `activate.fish`, `basetool` (from the base), `pydoc`, `python`, `python3`, `python3.13`,
  `python3.13-config`.

### `uninstall/envs.bash`

Source: `etc/pyenv.d/uninstall/envs.bash:1-32`. It registers `before_uninstall "uninstall_related_virtual_env"`.
`pyenv-uninstall` runs it after its own prompt and **before** `rm -rf`
[src pyenv:plugins/python-build/bin/pyenv-uninstall:66-94]. With `DEFINITION` = the argument, the hook does:

| Argument | Hook action |
|---|---|
| contains `/envs/` | `pyenv-virtualenv-delete [-f] <DEFINITION>` |
| `<root>/versions/<last component>` is a symlink whose one-level target, minus `<root>/versions/`, contains `/envs/` | `pyenv-virtualenv-delete [-f] <that long name>` |
| otherwise (a base version) | `pyenv-virtualenv-delete [-f] <DEFINITION>/envs/<n>` for each entry of `<root>/versions/<name>/envs/*` |

- **Force.** `-f` is passed exactly when uninstall's `FORCE` is set (`${FORCE+-f}`).
- **After the hook,** core removes `<root>/versions/<last component>` only if it is still a directory, and then prints
  `pyenv: <name> uninstalled` [src pyenv:plugins/python-build/bin/pyenv-uninstall:87-91].

Probes:

| Command (stdin) | Result |
|---|---|
| `pyenv uninstall -f 3.13.12/envs/venv4b` | env and `venv4b` link removed. No output, exit 0. No "uninstalled" line, because the link is already gone when core checks it. |
| `pyenv uninstall -f venv5` | same, via the link. No output, exit 0. |
| `pyenv uninstall venv4` (`y` newline `y` newline) | two questions (core's `pyenv: remove <root>/versions/venv4? (y/N)`, then the plugin's `pyenv-virtualenv: remove <root>/versions/3.13.12/envs/venv4? (y/N)`). Both removed, no output, exit 0. |
| `pyenv uninstall 3.13.12` (`y` newline `n` newline) | the first env question (alphabetical: `ssp`) answered `n`. `virtualenv-delete` exits 1, `set -e` aborts the whole uninstall, exit 1. **Nothing** is removed, the base included. |
| `pyenv uninstall -f 3.13.12` | every `3.13.12/envs/*` and its `versions/<name>` link removed, then the base. stdout `pyenv: 3.13.12 uninstalled`, exit 0. |

- **Accepted answers differ.** Core accepts only `y`, `Y`, `yes` and `YES`
  [src pyenv:plugins/python-build/bin/pyenv-uninstall:79-82]. The plugin accepts anything that starts with `y` or `Y`.

---

## Interactions with core pyenv 2.8.8

### `pyenv versions`

`pyenv versions` lists each `versions/*` entry, and right after each one its `envs/*` subdirectories as
`<version>/envs/<name>` [src pyenv:libexec/pyenv-versions:180-197].
- **Links.** A symlink prints as `<name> --> <readlink target>`, one level only [src :138-141].
- **Current marker.** `* … (set by <origin>)` matches by exact name [src :145-150].

Probe with no version selected:

```text
* system (set by <root>/version)
  3.12.99
  3.13.12
  3.13.12/envs/ssp
  3.13.12/envs/venv1
  miniconda3-4.7.12
  miniconda3-4.7.12/envs/cenv
  ssp --> <root>/versions/3.13.12/envs/ssp
  sysvenv
  venv1 --> <root>/versions/3.13.12/envs/venv1
```

(some lines omitted). Variants:
- **Selected env.** `PYENV_VERSION=venv1` marks only `* venv1 --> … (set by PYENV_VERSION environment variable)`, and
  `PYENV_VERSION=3.13.12/envs/venv1` marks only the long-name line [probe].
- **`--skip-envs`** (`pyenv:libexec/pyenv-versions:3,9,30,189`) drops the `<version>/envs/<name>` lines but
  **keeps** the `venv1 --> …` links and `sysvenv` [probe].
- **`--skip-aliases`** drops every symlink whose `realpath` parent is `<root>/versions` or which matches
  `<root>/versions/*/envs/*` [src :182-186]. So it hides the env links but keeps the long names and `sysvenv`.
  Together, the two flags leave `system`, the base versions and `sysvenv` [probe].
- **`--bare`** prints names only, without `system` [probe].
- **Not listed.** A system-based env's self-link `sysvenv/sysvenv` is not listed (it is not under `envs/`).

### `version-name`, `version`, `prefix`

- **`pyenv version-name`** keeps whatever name exists as a directory under `versions/`
  [src pyenv:libexec/pyenv-version-name:40-43,57].
  - `PYENV_VERSION=venv1` gives `venv1`, and `PYENV_VERSION=3.13.12/envs/venv1` gives `3.13.12/envs/venv1`.
  - `pyenv version` prints `venv1 (set by PYENV_VERSION environment variable)` [probe].
- **`pyenv prefix <env>`** is `<root>/versions/<name as given>`, **not** resolved [src pyenv:libexec/pyenv-prefix:48-56]:
  - `pyenv prefix venv1` gives `<root>/versions/venv1`;
  - `pyenv prefix 3.13.12/envs/venv1` gives `<root>/versions/3.13.12/envs/venv1`;
  - `pyenv prefix sysvenv` gives `<root>/versions/sysvenv` [probe].
  - Compare `sh-activate`, which resolves the link one level to get `VIRTUAL_ENV`.
- **`pyenv which python`** with `PYENV_VERSION=venv1` gives `<root>/versions/venv1/bin/python`, the link path
  [probe].

### The `system` base

- **Location.** The env goes to `versions/<name>` with no `envs` level and gets the self-link described above
  [probe].
- **Interpreter.** Detection tries `python3`, then `python`, then `python2`. The first that runs `-m venv --help`
  through `pyenv-exec` is used [src bin/pyenv-virtualenv:160-163]. Probe: `/usr/bin/python3 -m venv`.
- **`virtualenv-prefix`** comes from `pyvenv.cfg` `home` (`/usr/bin` gives `/usr`).
- **`virtualenvs`** lists it as `<name> (created from /usr)`, and `versions` lists it as a plain version.

### Completions, at 2.8.8 with the plugin

Each list starts with `--help`, which core adds [src pyenv:libexec/pyenv-completions:21] [probe]:

| `pyenv completions <cmd>` | Output after `--help` |
|---|---|
| `virtualenv` | `pyenv versions --bare --skip-envs`: bases, `versions/<name>` links, plain env dirs |
| `virtualenvs` | `--bare`, `--skip-aliases` |
| `virtualenv-delete` | `pyenv virtualenvs --bare` |
| `activate` | `--unset`, then `pyenv virtualenvs --bare` |
| `virtualenv-prefix`, `virtualenv-init`, `deactivate`, `sh-activate`, `sh-deactivate` | nothing |
| `uninstall` (core) | `--force`, then `pyenv versions --bare`, which includes the envs' long names and links |

`virtualenvs --bare` output, in the plugin's order: long names first, then `versions/*` links and dirs with
`bin/activate`, each group version-sorted. For example: `3.13.12/envs/ssp`, …, `miniconda3-4.7.12/envs/cenv`,
`miniconda3-4.7.12`, `ssp`, `sysvenv`, `venv1`, … [probe].

---

## End-to-end shell session (probe)

A non-interactive `bash --norc --noprofile` ran `eval "$(pyenv init - bash)"` and then
`eval "$(pyenv virtualenv-init - bash)"`, with `PS1='P> '`. The hook was called by hand in place of a prompt.

After init:
- `PROMPT_COMMAND=_pyenv_virtualenv_hook;`.
- `PATH` starts with `<plugin>/shims`, then `<root>/shims`.
- `type pyenv` is `function`.

| Step | Result |
|---|---|
| `pyenv activate venv1` | exit 0. `VIRTUAL_ENV` and `PYENV_VIRTUAL_ENV` are `<root>/versions/3.13.12/envs/venv1`, `PYENV_VERSION=venv1`, `PYENV_ACTIVATE_SHELL=1`, `PS1='(venv1) P> '`, `_OLD_VIRTUAL_PS1='P> '` |
| `pyenv activate venv1` again | stderr ``pyenv-virtualenv: version `venv1' is already activated``, exit 0, unchanged |
| `pyenv activate venv2` | switches cleanly. `PS1='(venv2) P> '`, and `_OLD_VIRTUAL_PS1` stays `'P> '` |
| `pyenv deactivate` | everything unset, `PS1='P> '` |
| `pyenv deactivate` again | stderr `pyenv-virtualenv: no virtualenv has been activated.`, exit 1 |
| `pyenv activate 3.13.12` | stderr ``pyenv-virtualenv: version `3.13.12' is not a virtualenv``, exit 1 |
| hook, in `proj/sub` where `proj/.python-version` says `venv1` | venv1 active. `PYENV_VERSION` and `PYENV_ACTIVATE_SHELL` stay unset |
| hook, after `cd` to a dir with no local version | deactivated |
| hook, after `.python-version` changed to `venv2` with a newer mtime | venv2 active |
| `pyenv shell 3.13.12`, then hook | deactivated (`PYENV_VERSION=3.13.12`) |
| `pyenv shell --unset`, then hook | venv2 active again |
| `pyenv activate venv1` in `proj`, then `cd` out, then hook | **stays** on venv1, because `PYENV_VERSION=venv1` was exported by the manual activate |
| `pyenv deactivate`, then hook outside `proj` | everything unset |
| third-party `. thirdparty/bin/activate`, `cd proj`, hook | third-party venv **kept**, because the "already activated" branch prints `true` |
| `deactivate` (the venv's own function), then hook in `proj` | venv2 auto-activated |

---

## Open points

1. **The `which` fallbacks.** Through `pyenv exec`, the three fallbacks (system site packages, `python*-config`,
   conda) do not work at 2.8.8, and the system-site-packages one yields a directory and exit 126. rpyenv's spec §10
   describes the working behavior. rpyenv needs to choose between matching upstream byte-for-byte and the intended
   behavior. The latter needs an allowlist entry. Not verified on older pyenv releases.
2. **The ensurepip path.** The plugin's pip step runs only for 2.x and 3.2 to 3.9 bases. Its stdout on a host with a
   working `ensurepip` is **UNCONFIRMED**.
3. **The failure path.** For `pyenv virtualenv 3.13.12 failenv`, only the end state was observed. The intermediate
   steps on the partial directory were not traced.
4. **Non-bash shells.** fish, zsh, ksh and pwsh output was checked as text only. Running it, and pwsh's reaction to
   POSIX `export` lines from the pwsh `pyenv` function, are **UNCONFIRMED**.
5. **Interactive prompts.** The text of the `y/N` prompts is `[src]`, because `read -p` does not show it on piped
   stdin.
6. **Bugs to match or not.** These are artifacts rpyenv must decide whether to reproduce:
   - the `versions/<name>/<name>` self-link for system-based envs;
   - the "always set" `COMPAT_VIRTUALENV_PATH`, which makes `-u` without `-f` fail;
   - `-p` taking the first positional argument;
   - `virtualenv-init <shell> -` dropping the shell.
