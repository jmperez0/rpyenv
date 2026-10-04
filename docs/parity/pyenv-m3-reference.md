# pyenv behavioral reference: rpyenv M3 shell integration (Linux)

Parity target for rpyenv's shell integration on Linux: `pyenv init` in every mode, the `pyenv` shell function it
defines, `pyenv shell` / `pyenv sh-shell`, `pyenv sh-rehash` (and `pyenv rehash` through the function),
`pyenv completions` with the four completion scripts, and every `--complete` output that the M1 and M2 references do
not already record. This file records facts only. Each fact carries an evidence tag.

How `file:line` paths are written:
- A path is relative to the root of the pyenv repository at the pinned commit below. So `libexec/pyenv-init:57` is
  that file's line 57, and `test/init.bats:96` is a line of upstream's bats suite.
- `plugins/<name>/…` is a vendored plugin of the same commit.
- `parity/…` is a file in this (rpyenv) repository.

Companions: `pyenv-m1-reference.md` (abbreviated **M1**) covers the dispatcher, hooks, `pyenv rehash`, `pyenv help`,
`pyenv commands`, and the `--complete` output of every M1 command. `pyenv-m2-reference.md` (**M2**) covers
`pyenv install`, `uninstall`, `install-prerequisites` and their `--complete` output. This file cross-references them
and records only what is new or what differs at the new pin.

## Source, version, and evidence method

- **Source.** GitHub `pyenv/pyenv`, commit `07171d013cac53d1cc9248b4c160217f288a2965`, a full unpacked tree read at
  `/home/jm/m1cb-upstream/pyenv-07171d013cac53d1cc9248b4c160217f288a2965` in WSL Debian.
- **Version.** `libexec/pyenv---version:15` sets `version="2.8.8"`, and
  `plugins/python-build/bin/python-build:17` sets `PYTHON_BUILD_VERSION="2.8.8"` [probe].
- **Earlier pin.** M1 and M2 were written at 2.8.6 (`ab74141ab4bcd34aa9b11f363dd8ad4f6fc01cd2`). That tree is
  unpacked next to this one. The section "Changes from 2.8.6 to 2.8.8" lists every difference in this file's area,
  found with `diff`.

Evidence tags:

| Tag | Meaning |
|---|---|
| `[src f:l]` | Derived by reading the code at that line. Not executed by itself. |
| `[test f:l]` | A value asserted by upstream's own bats suite. |
| `[probe]` | Observed by running the pinned scripts in WSL. See the next paragraphs. |
| `[probe, win-pwsh]` | A pwsh snippet that pyenv emitted, copied verbatim and run in PowerShell 7.6.6 on the Windows host. See below. |
| **UNCONFIRMED** | Not established by any of the above. |

**Probe environment.** The probes ran on the WSL2 Debian host (`/etc/os-release`: "Debian GNU/Linux forky/sid";
kernel 6.18.40.1-microsoft-standard-WSL2), as the unprivileged user `jm`, with GNU bash 5.3.3. `/bin/sh` is `dash`.

- **Shells present in WSL** (`command -v`): `bash`, `dash`, `sh` (= dash). **Absent:** `zsh`, `fish`, `pwsh`, `ksh`,
  `ksh93`, `mksh`, `nu`, `tcsh`. Nothing was installed. pyenv's *output* for any shell name can still be probed,
  because pyenv itself is bash. But *running* the emitted code was possible only for bash and dash. Facts about how
  zsh, fish or ksh run the emitted code are `[src]` or `[test]`.
- **pwsh.** PowerShell 7.6.6 exists on the Windows host. The env-var snippets that `pyenv sh-shell` emits for pwsh,
  and the PATH lines that `pyenv init - pwsh` emits, were run there in a child `pwsh -NoProfile` process, with no
  `pyenv` command involved (`[probe, win-pwsh]`). The full pwsh `pyenv` function was **not** run, because it calls
  whatever `pyenv` application is on PATH, and on that host that would be the real pyenv-win install. Environment
  variables are case-insensitive on Windows, which does not affect the snippets tested.
- **Isolation.** For each probe script, the pinned tree was copied to a fresh `mktemp -d /tmp/m3ref.XXXXXX`
  directory (written `<S>` below), with `bin/pyenv -> ../libexec/pyenv`. Each script exported
  `HOME=<S>/home`, `PYENV_ROOT=<S>/home/.pyenv`, `ZDOTDIR=<S>/zdot`, `XDG_CONFIG_HOME=<S>/xdg`, `TMPDIR=<S>/tmp`,
  `LANG=C.UTF-8`, `SHELL=/bin/bash` and `PATH=<S>/pyenv/bin:/usr/local/bin:/usr/bin:/bin`. It unset `PYENV_VERSION`,
  `PYENV_DIR`, `PYENV_DEBUG`, `PYENV_SHELL`, `PYENV_VERSION_OLD` and every other `XDG_*` variable. Each `--install`
  case got its own `HOME=<S>/h_<case>`, and the script aborts unless `HOME` is under `<S>`.
- **Real home untouched.** The size, mtime and SHA-256 of `/home/jm/{.bashrc,.profile,.bash_profile,.zshrc,.zprofile,
  .config/fish/config.fish,.config/powershell/profile.ps1}` were recorded before the `--install` probes and after the
  last probe. The two snapshots are identical.
- **Capture.** Stdout and stderr were captured separately and shown with `cat -A`, so the end of every line is known
  exactly. In quoted output, `<root>` is `PYENV_ROOT`, `<prefix>` is `_PYENV_INSTALL_PREFIX` (the copied tree), and
  `<home>` is `HOME`.

**Harness check** [probe]. Upstream's `test/init.bats`, `test/shell.bats`, `test/rehash.bats`,
`test/completions.bats` and `test/exec.bats` ran with Bats 1.11.1 against the copied tree: **97 tests, 89 ok, 8 skipped
(all `-- fish not installed` or `-- pwsh not installed`), 0 not ok**. The per-file `@test` counts (from `grep -c
'^@test'`) are init 41, shell 16, rehash 27, completions 3, exec 10, which sum to 97.

### How the bats suite observes these commands

M1 "How the bats suite observes output" applies: `run` merges stdout and stderr, and trailing newlines are invisible.
In addition, `test/test_helper.bash:29-30` sets `PYENV_ROOT=$BATS_TEST_TMPDIR/pyenv/root` and
`HOME=$BATS_TEST_TMPDIR/pyenv/home`, so `PYENV_ROOT` is **not** `$HOME/.pyenv` in the tests unless a test sets it.
Lines 43-44 unset every `XDG_*` variable, and line 50 exports `NO_COLOR=1` for pwsh [src test/test_helper.bash].
`_PYENV_INSTALL_PREFIX` is exported as the repository root (line 7). The init tests call `libexec/pyenv-init`
directly, so the parent process seen by shell detection is the bats process, which is bash.

## Changes from 2.8.6 to 2.8.8 in this area

Found with `diff` over `libexec/`, `completions/`, `plugins/*/bin/`, and the five test files [probe]:

- **`libexec/pyenv-init`.** The `PYENV_ROOT` line in the help text and in `--install` output used to be fixed
  (`export PYENV_ROOT="$HOME/.pyenv"`, `set -Ux PYENV_ROOT $HOME/.pyenv`, `$Env:PYENV_ROOT="$Env:HOME/.pyenv"`). At
  2.8.8 that fixed form is printed only when `PYENV_ROOT` equals `$HOME/.pyenv`. Any other root is printed literally,
  quoted for the target shell (new functions at `libexec/pyenv-init:192-258`). Nothing else in `pyenv-init` changed.
- **`libexec/pyenv-exec`.** New `-N`/`--environment` flag. Its `--complete` now prints `--environment` before the
  shim names (`libexec/pyenv-exec:30-33`). M1's `exec` section, which says `--complete` only runs
  `pyenv-shims --short`, is out of date at 2.8.8.
- **`test/init.bats`.** Five `install setup …` tests gained the suffix ` (default PYENV_ROOT)` and now set
  `PYENV_ROOT="$HOME/.pyenv"`. New test `init honors custom PYENV_ROOT and quotes it according to each shell's rules`.
- **`test/exec.bats`.** The completion test expects `--environment`. Two new `--environment` tests (not M3).
- **`test/test_helper.bash`.** `assert_success` and `assert_failure` gained an explicit `return 1` after `flunk`.
- **Unchanged:** `pyenv-sh-shell`, `pyenv-sh-rehash`, `pyenv-rehash`, `pyenv-completions`, `pyenv-commands`, `pyenv-help`,
  the dispatcher `libexec/pyenv`, all four `completions/pyenv.*` scripts, the `pyenv-install`, `pyenv-uninstall`,
  `pyenv-install-prerequisites`, `pyenv-binary`, `pyenv-link` and `pyenv-link-version` entry points, and
  `test/shell.bats`, `test/rehash.bats`, `test/completions.bats`.

---

## `pyenv init` — `libexec/pyenv-init`

### Synopsis, arguments and modes

```text
# Summary: Configure the shell environment for pyenv
# Usage: eval "$(pyenv init [-|--path] [--no-push-path] [--no-rehash] [<shell>])"
#        pyenv init --install [<shell>]
#        pyenv init --detect-shell [<shell>]
```
[src libexec/pyenv-init:2-5]

- **`--complete`** must be the first argument. It prints these 11 lines and exits 0
  [src libexec/pyenv-init:11-24] [probe] [test test/init.bats:90]:

  ```text
  -
  --path
  --install
  --no-push-path
  --no-rehash
  --detect-shell
  bash
  fish
  ksh
  pwsh
  zsh
  ```

- **Argument loop** [src libexec/pyenv-init:28-53] [probe]. Every argument is read, in any order:
  - `-` sets mode `print`, `--path` sets mode `path`, `--install` sets `install`, and `--detect-shell` sets
    `detect-shell`. **The last mode flag wins.** `pyenv init --path - bash` runs print mode, and
    `pyenv init - --path bash` runs path mode [probe].
  - `--no-push-path` and `--no-rehash` set flags. They are accepted in every mode but only print and path modes use
    them. Help mode ignores them: `pyenv init --no-rehash bash` prints the same help as `pyenv init bash` [probe].
  - **Any other argument becomes the shell name, and the last one wins.** There is no validation. Unknown options are
    taken as shell names: `pyenv init --detect-shell --foo` prints `PYENV_SHELL_DETECT=--foo`, and
    `pyenv init - -x` prints `export PYENV_SHELL=-x` [probe]. `pyenv init --detect-shell zsh fish` reports `fish`
    [probe].
- **Default mode** is `help` [src libexec/pyenv-init:26].
- **Exit codes** [src libexec/pyenv-init:69-101] [probe]: help mode exits **1**. Print, path and detect-shell modes
  exit 0. Install mode exits 0 on success and 1 on any refusal.

### Shell detection

Applies only when no shell name was given [src libexec/pyenv-init:56-67]:

1. `shell=$(tr '\0' ' ' </proc/$PPID/cmdline)`, which is the parent's full command line with NULs turned into
   spaces. If reading `/proc` fails, it falls back to `ps p "$PPID" -o 'args='` [src]. The fallback was not probed.
2. Keep the text up to the first space, strip **one** leading `-` (login shells), fall back to `$SHELL` if the result
   is empty, keep the part after the last `/`, then keep the part before the first `-`.

`PPID` is the process that ran `pyenv`. The dispatcher `exec`s `pyenv-init` [src libexec/pyenv:142], so no extra
process sits in between.
Observed by running `pyenv init --detect-shell` from a bash process whose `argv[0]` was set with `exec -a` [probe]:

| parent `argv[0]` | detected | | parent `argv[0]` | detected |
|---|---|---|---|---|
| `bash`, `-bash`, `/usr/bin/bash` | `bash` | | `sh` | `sh` |
| `zsh`, `-zsh` | `zsh` | | `dash` | `dash` |
| `/usr/local/bin/zsh-5.9` | `zsh` | | `nu` | `nu` |
| `bash-5.3` | `bash` | | `xonsh.py` | `xonsh.py` |
| `fish` | `fish` | | `/usr/bin/zsh -l` (one argv element) | `zsh` |
| `pwsh`, `-pwsh` | `pwsh` | | `-` with `SHELL=/opt/x/fish` | `fish` |
| `ksh93`, `mksh` | as given | | empty with `SHELL=/opt/x/zsh` | `zsh` |

- With an empty `argv[0]` and `SHELL` unset, the result was `bash` [probe]. bash sets `SHELL` from the passwd entry
  when the variable is unset at startup, and `jm`'s login shell is bash. That mechanism is inferred, not traced.
- `SHELL=/bin/false pyenv init -` from a bash parent prints `export PYENV_SHELL=bash`. The parent wins over `$SHELL`
  [probe] [test test/init.bats:32-36].
- A `#!/bin/sh` script that runs `eval "$(pyenv init -)"` gets `PYENV_SHELL=sh`, even though `/bin/sh` is dash,
  because the kernel puts `/bin/sh` in `argv[0]` [probe] [test test/init.bats:38-49]. `dash -c 'eval "$(pyenv init -)"'`
  gets `dash` [probe].

### Shell families and startup files

`detect_profile` [src libexec/pyenv-init:103-142]. The `~` is literal text in every value:

| shell | `profile` | `rc` | used by |
|---|---|---|---|
| `bash` | `~/.bash_profile` if `$HOME/.bash_profile` exists, else `~/.profile` | `~/.bashrc` | help text, `--detect-shell`, `--install` |
| `zsh` | `~/.zprofile` | `~/.zshrc` | same |
| `fish` | `~/.config/fish/config.fish` | the same file | same |
| `pwsh` | `~/.config/powershell/profile.ps1` | the same file | same |
| `ksh`, `ksh93`, `mksh` | `~/.profile` | `~/.profile` | same |
| anything else (`sh`, `dash`, `nu`, …) | empty | empty | help text uses the descriptive wording below |

- `ZDOTDIR` and `XDG_CONFIG_HOME` are **ignored**. With both set to other directories, `--install zsh` wrote
  `$HOME/.zshrc` and `$HOME/.zprofile`, and `--install pwsh` wrote `$HOME/.config/powershell/profile.ps1`. The
  `ZDOTDIR` and `XDG_CONFIG_HOME` directories stayed empty [probe].
- **Code-family dispatch** differs per mode. For print and path modes and the shell function: `fish`, `pwsh`, and
  everything else (POSIX family). The function's header has a third variant for `ksh`, `ksh93` and `mksh`
  [src libexec/pyenv-init:518-580]. Completion scripts exist only for `bash`, `zsh`, `fish` and `pwsh`.

### `--detect-shell`

Prints three lines and exits 0 [src libexec/pyenv-init:144-148] [probe]:

```text
PYENV_SHELL_DETECT=<shell>
PYENV_PROFILE_DETECT=<profile>
PYENV_RC_DETECT=<rc>
```

Observed values [probe] [test test/init.bats:76-88]:

| invocation | `PROFILE_DETECT` | `RC_DETECT` |
|---|---|---|
| `--detect-shell bash`, no `~/.bash_profile` | `~/.profile` | `~/.bashrc` |
| `--detect-shell bash`, `~/.bash_profile` exists | `~/.bash_profile` | `~/.bashrc` |
| `zsh` | `~/.zprofile` | `~/.zshrc` |
| `fish` | `~/.config/fish/config.fish` | `~/.config/fish/config.fish` |
| `pwsh` | `~/.config/powershell/profile.ps1` | `~/.config/powershell/profile.ps1` |
| `ksh`, `ksh93`, `mksh` | `~/.profile` | `~/.profile` |
| `sh`, `dash`, `nu` | (empty) | (empty) |

For an unknown shell the lines are `PYENV_PROFILE_DETECT=` and `PYENV_RC_DETECT=`, with nothing after the `=`.

### Help mode (no mode flag)

Everything goes to **stderr**, stdout is empty, and the exit code is **1** [src libexec/pyenv-init:150-190] [probe].
The shell is the argument, or the detected one when there is none. `pyenv init` from a bash parent prints the bash
text.

**bash** (the same text whether or not `~/.bash_profile` exists) [probe]:

```text
# Load pyenv automatically by appending
# the following to 
# ~/.bash_profile if it exists, otherwise ~/.profile (for login shells)
# and ~/.bashrc (for interactive shells) :

export PYENV_ROOT="$HOME/.pyenv"
[[ -d $PYENV_ROOT/bin ]] && export PATH="$PYENV_ROOT/bin:$PATH"
eval "$(pyenv init - bash)"

# Restart your shell for the changes to take effect.

```

- The second line is `# the following to ` with a **trailing space**. It comes from `echo -n "# the following to "`
  followed by a bare `echo` [src libexec/pyenv-init:174-178].
- The output ends with one empty line [probe].

**zsh**: as bash, with lines 3-4 `# ~/.zprofile (for login shells)` and `# and ~/.zshrc (for interactive shells) :`,
and `eval "$(pyenv init - zsh)"` [probe].

**ksh, ksh93, mksh** (`profile == rc`, so a single-line target) [probe]:

```text
# Load pyenv automatically by appending
# the following to ~/.profile :

export PYENV_ROOT="$HOME/.pyenv"
[[ -d $PYENV_ROOT/bin ]] && export PATH="$PYENV_ROOT/bin:$PATH"
eval "$(pyenv init - ksh)"

# Restart your shell for the changes to take effect.

```

The `eval` line names the shell exactly as given (`ksh93`, `mksh`).

**Any other shell** (`sh`, `nu`, …) [probe]:

```text
# Load pyenv automatically by appending
# the following to 
# your shell's login startup file (for login shells)
# and your shell's interactive startup file (for interactive shells) :

export PYENV_ROOT="$HOME/.pyenv"
[[ -d $PYENV_ROOT/bin ]] && export PATH="$PYENV_ROOT/bin:$PATH"
eval "$(pyenv init - sh)"

# Restart your shell for the changes to take effect.

```

The `[[ … ]]` line is printed even for `sh`, where dash cannot run it.

**fish** [probe] [test test/init.bats:57-61]:

```text
# Add pyenv executable to PATH by running
# the following interactively:

set -Ux PYENV_ROOT $HOME/.pyenv
if functions -q fish_add_path
  test -d $PYENV_ROOT/bin; and fish_add_path $PYENV_ROOT/bin
else
  test -d $PYENV_ROOT/bin; and set -U fish_user_paths $PYENV_ROOT/bin $fish_user_paths
end

# Load pyenv automatically by appending
# the following to ~/.config/fish/config.fish:

pyenv init - fish | source


# Restart your shell for the changes to take effect.

```

Fish has **two** empty lines before `# Restart…`, because its branch ends with `echo` and the shared tail starts with
another `echo` [src libexec/pyenv-init:164,186]. There is no space before `:` in `config.fish:`.

**pwsh** [probe] [test test/init.bats:70-74]:

```text
# Load pyenv automatically by appending
# the following to ~/.config/powershell/profile.ps1 :

$Env:PYENV_ROOT="$Env:HOME/.pyenv"
if (Test-Path -LP "$Env:PYENV_ROOT/bin" -PathType Container) {
  $Env:PATH="$Env:PYENV_ROOT/bin:$Env:PATH" }
iex ((pyenv init -) -join "`n")

# Restart your shell for the changes to take effect.

```

The pwsh `iex` line runs `pyenv init -` with no shell argument, so the shell is detected at run time.

#### The `PYENV_ROOT` line: default versus custom root (new in 2.8.8)

`pyenv_root_is_default` is `[[ ${PYENV_ROOT} == "${HOME}/.pyenv" ]]`, a plain string comparison
[src libexec/pyenv-init:193-195]. `PYENV_ROOT` is the dispatcher-normalized value (M1: an unset root becomes
`$HOME/.pyenv`, and one trailing `/` is removed). So:

- unset `PYENV_ROOT` → default form [probe];
- `PYENV_ROOT=$HOME/.pyenv/` → default form, because the dispatcher strips the slash [probe];
- `PYENV_ROOT=$HOME/../home/.pyenv`, the same directory spelled differently → **custom** form, printed literally
  [probe].

Custom-root forms [src libexec/pyenv-init:197-258] [probe] [test test/init.bats:161-175]:

| family | escaping | root | printed line |
|---|---|---|---|
| POSIX | `\`→`\\`, `"`→`\"`, `$`→`\$`, `` ` ``→``\` `` inside `"…"` | `/tmp/pyenv$root/"quote"` | `export PYENV_ROOT="/tmp/pyenv\$root/\"quote\""` |
| POSIX | same | ``/tmp/a`b\c`` | ``export PYENV_ROOT="/tmp/a\`b\\c"`` |
| fish | `\`→`\\`, `"`→`\"`, `$`→`\$` (backtick **not** escaped) | `/tmp/pyenv$root/"quote"` | `set -Ux PYENV_ROOT "/tmp/pyenv\$root/\"quote\""` |
| fish | same | ``/tmp/a`b\c`` | ``set -Ux PYENV_ROOT "/tmp/a`b\\c"`` |
| pwsh | single-quoted literal, `'`→`''` | `/tmp/pyenv'root` | `$Env:PYENV_ROOT='/tmp/pyenv''root'` |
| pwsh | same (`$` is literal in `'…'`) | `/tmp/py$env` | `$Env:PYENV_ROOT='/tmp/py$env'` |

The other lines of the help text do not change with the root.

### Print mode (`pyenv init -`)

Steps, in order [src libexec/pyenv-init:80-88]:

1. `init_dirs`: `mkdir -p "${PYENV_ROOT}/"{shims,versions}` [src libexec/pyenv-init:408-410]. Print mode is the
   only mode that creates them [test test/init.bats:5-12] [probe].
   - **If mkdir fails**, `set -e` aborts before any stdout. Exit 1, and stderr carries one `mkdir` message per
     brace-expanded directory. With `PYENV_ROOT` a missing directory inside a mode-555 parent, both messages name
     the root itself, which `mkdir -p` could not create [probe]:
     ```text
     mkdir: cannot create directory ‘<root>’: Permission denied
     mkdir: cannot create directory ‘<root>’: Permission denied
     ```
     The quotes are U+2018/U+2019, as GNU mkdir prints them under `LANG=C.UTF-8`; they come from mkdir, not pyenv.
2. `print_path`: the PATH lines (see below).
3. `print_env`: `export PYENV_SHELL=<shell>` / fish `set -gx PYENV_SHELL <shell>` / pwsh
   `$Env:PYENV_SHELL="<shell>"` [src libexec/pyenv-init:477-489]. The shell name is not quoted.
4. `print_completion`: only if `<prefix>/completions/pyenv.<shell>` is readable. pwsh gets
   `iex (gc <path> -Raw)` (path not quoted), every other shell gets `source '<path>'` [src libexec/pyenv-init:491-503].
   So `ksh`, `sh`, `nu` and others get **no** completion line [probe]. When `pyenv-init` runs without
   `_PYENV_INSTALL_PREFIX`, for example `libexec/pyenv-init` run directly outside the dispatcher, the path is
   `/completions/pyenv.bash`, which does not exist, so no line is printed [probe].
5. `print_rehash`: `command pyenv rehash`, or `& pyenv rehash` for pwsh. Omitted under `--no-rehash`
   [src libexec/pyenv-init:505-516] [test test/init.bats:14-18].
6. `print_shell_function` [src libexec/pyenv-init:518-580]. See "The `pyenv` shell function" below.

Nothing is written to stderr on success. Exit 0.

**bash** (`pyenv init - bash`), verbatim [probe]. Lines 1-4 end with a **trailing space** (`… paths=($PATH); `,
`… do `, `…'\''; `, `fi; done; `):

```text
PATH="$(bash --norc -ec 'IFS=:; paths=($PATH); 
for i in ${!paths[@]}; do 
if [[ ${paths[i]} == "''<root>/shims''" ]]; then unset '\''paths[i]'\''; 
fi; done; 
echo "${paths[*]}"')"
export PATH="<root>/shims:${PATH}"
export PYENV_SHELL=bash
source '<prefix>/completions/pyenv.bash'
command pyenv rehash
pyenv() {
  local command=${1:-}
  [ "$#" -gt 0 ] && shift
  case "$command" in
  rehash|shell)
    eval "$(pyenv "sh-$command" "$@")"
    ;;
  *)
    command pyenv "$command" "$@"
    ;;
  esac
}
```

- **zsh**: identical except `export PYENV_SHELL=zsh` and `source '<prefix>/completions/pyenv.zsh'` [probe]. The PATH
  code still runs a nested `bash --norc` [src libexec/pyenv-init:452-456].
- **ksh / ksh93 / mksh**: identical to bash except `export PYENV_SHELL=<name>`, **no** `source` line, and a different
  function header [probe]:
  ```text
  function pyenv {
    typeset command=${1:-}
  ```
- **sh, dash, nu, any other name**: identical to bash except `export PYENV_SHELL=<name>` and no `source` line. The
  header stays `pyenv() {` / `local command=${1:-}` [probe].

**fish** (`pyenv init - fish`), verbatim [probe]:

```text
while set pyenv_index (contains -i -- "<root>/shims" $PATH)
set -eg PATH[$pyenv_index]; end; set -e pyenv_index
set -gx PATH '<root>/shims' $PATH
set -gx PYENV_SHELL fish
source '<prefix>/completions/pyenv.fish'
command pyenv rehash
function pyenv
  set command $argv[1]
  set -e argv[1]

  switch "$command"
  case rehash shell
    source (pyenv "sh-$command" $argv|psub)
  case "*"
    command pyenv "$command" $argv
  end
end
```

**pwsh** (`pyenv init - pwsh`), verbatim [probe]:

```text
$Env:PATH="$(($Env:PATH -split ':' | where { -not ($_ -match '<root>/shims') }) -join ':')"
$Env:PATH="<root>/shims:$Env:PATH"
$Env:PYENV_SHELL="pwsh"
iex (gc <prefix>/completions/pyenv.pwsh -Raw)
& pyenv rehash
function pyenv {
  $command=""
  if ( $args.Count -gt 0 ) {
    $command, $args = $args
  }

  if ( ("rehash shell" -split ' ') -contains $command ) {
    $shell_cmds = (& (get-command -commandtype application pyenv -totalcount 1) sh-$command $args)
    if ( $shell_cmds.Count -gt 0 ) {
      iex ($shell_cmds -join "`n")
    }
  } else {
    & (get-command -commandtype application pyenv -totalcount 1) $command $args
  }
}
```

The pwsh `pyenv` function was not executed (see "Probe environment").

#### PATH handling (`print_path`)

`PYENV_ROOT` is pasted into the emitted code **without any escaping** in print and path modes
[src libexec/pyenv-init:412-475]. A root containing a space still works in bash, because it stays inside the double
quotes [probe]. A root containing `"` produces broken code: with root `<S>/x$y"z`, the emitted line was
`export PATH="<S>/x$y"z/shims:${PATH}"` [probe].

Default behavior (no `--no-push-path`): **remove every existing shims entry, then prepend one.**
- **POSIX family.** A nested `bash --norc -ec` splits `$PATH` on `:` and drops entries **exactly equal** to
  `<root>/shims`. Then `export PATH="<root>/shims:${PATH}"`. `--norc` is there to avoid an infinite `.bashrc` loop on
  SSH builds of bash (#2367) [src libexec/pyenv-init:445-451]. Executed in bash [probe]:
  - `PATH=<bin>:/a:<root>/shims:/b:<root>/shims:/c:<root>/shims-old:/usr/bin:/bin` becomes
    `<root>/shims:<bin>:/a:/b:/c:<root>/shims-old:/usr/bin:/bin`. Both exact copies are removed, and `shims-old` is
    kept.
  - Running `--path` twice leaves exactly one shims entry at the front [probe].
  - Upstream asserts the same with a `nonexistent` dir in front [test test/init.bats:258-267].
- **fish.** Loops `contains -i` / `set -eg PATH[$i]`, removing exact matches, then `set -gx PATH '<root>/shims' $PATH`
  [src]. Not executed (fish absent). The upstream fish test was skipped.
- **pwsh.** Removes every entry for which `$_ -match '<root>/shims'` is true. That is a **regular-expression substring
  match**, not equality [probe, win-pwsh]. For root `/r/.pyenv`, the PATH
  `/a:/r/.pyenv/shims:/b:/r/.pyenv/shims-old:/x/r/.pyenv/shims/y:/r/Xpyenv/shims:/c` became `/a:/b:/c`. It removed
  `shims-old`, an entry merely containing the text, and `/r/Xpyenv/shims`, because `.` matched `X`. A root containing
  `(` makes the emitted line fail to parse: ``$(subexpression) is missing the closing ')'`` [probe, win-pwsh].

`--no-push-path`: **prepend only if absent; never remove** [src libexec/pyenv-init:415-432]:

```text
if [[ ":$PATH:" != *':<root>/shims:'* ]]; then
export PATH="<root>/shims:${PATH}"
fi
```
```text
if not contains -- "<root>/shims" $PATH
set -gx PATH '<root>/shims' $PATH
end
```
```text
if ( $Env:PATH -notmatch "<root>/shims" ) {
$Env:PATH="<root>/shims:$Env:PATH"
}
```

These are the POSIX, fish and pwsh forms in that order. The inner lines are **not** indented [probe].
- bash: with shims already present (twice), PATH is unchanged. With shims absent, they are prepended [probe]
  [test test/init.bats:295-303, 330-338].
- **dash**: the `[[` test is not POSIX. `eval "$(pyenv init - sh --no-push-path --no-rehash)"` in dash prints
  `dash: 1: eval: [[: not found` on stderr, leaves PATH **unchanged**, and the `eval` returns 0 [probe].
- pwsh: `-notmatch` is a regex substring test. With only `/r/.pyenv/shims-old` on PATH, shims were **not** added
  [probe, win-pwsh].

### Path mode (`pyenv init --path`)

Prints `print_path` then `print_rehash`, and exits 0 [src libexec/pyenv-init:75-79]. It does **not** create
`shims/` or `versions/`: run against a nonexistent root, the root still did not exist afterwards [probe]. It honors
`--no-push-path` and `--no-rehash` [probe]. `command pyenv rehash` is printed by default
[test test/init.bats:20-24]. Path mode prints no `PYENV_SHELL`, no completion line and no function [probe].

### The `pyenv` shell function and which commands it routes

- **The routed set is derived at init time** from `pyenv-commands --sh` [src libexec/pyenv-init:519]. That is, every
  executable named `pyenv-sh-*` in any directory on the `PATH` that `pyenv-init` sees, with the prefix stripped,
  through `sort -u` [src libexec/pyenv-commands:28-33]. That `PATH` is the caller's PATH plus what the dispatcher
  prepends: `<prefix>/libexec`, `<prefix>/plugins/*/bin`, and `$PYENV_ROOT/plugins/*/bin` (M1 dispatcher).
- At 2.8.8 the tree ships exactly `libexec/pyenv-sh-rehash` and `libexec/pyenv-sh-shell`. No vendored plugin
  (`python-build`, `pyenv-binary`, `pyenv-link`) has an `sh-` command. So `pyenv commands --sh` prints `rehash` and
  `shell`, and the function routes **`rehash` and `shell`** [probe].
- Adding `$PYENV_ROOT/plugins/fake/bin/pyenv-sh-activate` changed the emitted lists to `activate|rehash|shell)`,
  `case activate rehash shell` and `("activate rehash shell" -split ' ')` [probe]. Upstream's test stubs
  `pyenv-commands` to print `activate deactivate rehash shell` and expects `  activate|deactivate|rehash|shell)`
  [test test/init.bats:375-381].
- **POSIX join.** The names are joined with `|` (`IFS="|"`). An empty list becomes the pattern `/`, which matches no
  command name. With `pyenv-commands` stubbed to print one empty line, the result is `  /)`
  [src libexec/pyenv-init:567-578] [test test/init.bats:383-388].
- **Routed command** (POSIX): `eval "$(pyenv "sh-$command" "$@")"`. Inside the function, the inner `pyenv` is the
  function itself. It falls to `*)` and runs `command pyenv sh-<cmd> …` [src].
- **Everything else**: `command pyenv "$command" "$@"`. With no arguments, `command` is empty and the function calls
  `command pyenv ""`. The dispatcher then prints the version and help to stderr and exits 1 (M1). Through the
  function, `pyenv` alone returned 1 [probe].
- bash's `type pyenv` shows the function pretty-printed (`rehash | shell)` with spaces), which is bash's own
  formatting [probe].

**Behavior through the bash function, executed** [probe]. The session ran `eval "$(pyenv init - bash)"` with versions
3.12.1 and 3.11.9 installed:

| step | stdout | stderr | `$?` | `PYENV_VERSION` / `PYENV_VERSION_OLD` after |
|---|---|---|---|---|
| `pyenv shell` (nothing set) | | `pyenv: no shell-specific version configured` | **0** | unset / unset |
| `pyenv shell 3.12.1` | | | 0 | `3.12.1` / `` (set, empty) |
| `pyenv shell` | `3.12.1` | | 0 | unchanged |
| `pyenv shell 3.11.9` | | | 0 | `3.11.9` / `3.12.1` |
| `pyenv shell -` | | | 0 | `3.12.1` / `3.11.9` |
| `pyenv shell -` | | | 0 | `3.11.9` / `3.12.1` |
| `pyenv shell --unset` | | | 0 | unset / `3.11.9` |
| `pyenv shell -` | | | 0 | `3.11.9` / `` (empty) |
| `pyenv shell -` | | | 0 | unset / `3.11.9` |
| `pyenv shell -` (both unset first) | | `pyenv: PYENV_VERSION_OLD is not set` | 1 | unset / unset |
| `pyenv shell 1.2.3` | | ``pyenv: version `1.2.3' not installed`` | 1 | unchanged |
| `pyenv shell 3.12` (a prefix) | | | 0 | `3.12` (the literal argument) |
| `pyenv shell --help` | the `pyenv help shell` text | | 0 | |
| `pyenv rehash` | | | 0 | creates shims and empties bash's hash table (`hash: hash table empty` afterwards) |

- `pyenv shell` with nothing set **returns 0** through the function. `sh-shell` exits 1, but its stdout is empty, and
  `eval ""` succeeds [probe] [test test/shell.bats:10-14 asserts `assert_success` with that message].
- `pyenv shell --help` goes through three steps. The dispatcher sees `sh-shell --help` and prints
  `pyenv help "sh-shell"` [src libexec/pyenv:135-138] [probe]. The function evals that, which calls the function
  again with `help`, which runs `command pyenv help sh-shell`.
- The dash function session (`eval "$(pyenv init - sh)"`) behaved the same for `shell <v>`, `shell -`,
  `shell --unset` and `rehash` [probe].

### `--install`

[src libexec/pyenv-init:291-406]. Steps:

1. If `HOME` is empty: stderr `pyenv: HOME must be set to configure shell startup files`, exit 1. Nothing is
   written [probe].
2. Choose files and text by shell:
   - `bash | zsh | ksh | ksh93 | mksh`: write the POSIX setup (the 3 lines from help mode, with the `PYENV_ROOT` line
     chosen as described there) to the expanded `rc`. Then, if `profile` differs from `rc`, write the same text to
     `profile`. **Order: rc first, then profile.**
   - `fish`: write `pyenv init - fish | source` to `~/.config/fish/config.fish`.
   - `pwsh`: write the 4-line pwsh setup to `~/.config/powershell/profile.ps1`.
   - **Anything else, including `sh` and `dash`**: stderr `pyenv: cannot automatically configure startup files for
     <shell>`, exit 1 [probe for sh, dash, nu, tcsh] [test test/init.bats:224-229].
3. `~` is expanded to `$HOME` by `${path/#\~/$HOME}` [src libexec/pyenv-init:286-289].
4. **Check every file before writing any** (`check_startup_file`) [src libexec/pyenv-init:345-371]:
   - The file does not exist (`! -e`; a dangling symlink counts as absent): OK.
   - It is not a regular file, or it is unreadable (`! -f || ! -r`): stderr `pyenv: failed to inspect <path>`,
     exit 1. Observed for a `.bashrc` directory, a mode-000 `.bashrc`, and a mode-000 `.profile` [probe]
     [test test/init.bats:200-208].
   - `grep -Fi pyenv` matches anywhere in the file, **case-insensitively and as a substring**. That includes
     `PYENV_ROOT`, a comment `# managed by PYENV-tools`, and `alias mypyenvthing=true` [probe]. stderr gets three
     lines, then exit 1 [probe] [test test/init.bats:177-198]:
     ```text
     pyenv: cannot automatically apply changes to <home>/.bashrc: it appears to already contain Pyenv-related code.
     pyenv: review the file's contents and apply changes manually if necessary.
     pyenv: run `pyenv init bash` to see the suggested setup.
     ```
     `<path>` is the absolute expanded path, and the last line names the shell being installed.
   - grep fails for another reason (exit status ≥ 2): `pyenv: failed to inspect <path>`, and the exit code is grep's
     status [src libexec/pyenv-init:364-369].
   - Because all checks run first, **a refusal on the second file leaves the first untouched**. With only `.profile`
     containing pyenv code, `.bashrc` was not created [probe].
5. **fish only**: after the checks and before any write, `install_fish_user_paths` [src libexec/pyenv-init:373-386]:
   - no `fish` on PATH: stderr `pyenv: fish is not available to configure fish universal variables`, exit 1, nothing
     written [probe];
   - otherwise it runs `fish -c "<the 6-line fish PATH block from help mode>"`. A stub `fish` recorded exactly two
     arguments, `-c` and that block [probe] [test test/init.bats:134-148]. The custom-root form is used when it
     applies, for example `set -Ux PYENV_ROOT "/opt/a\$b"` [probe];
   - `fish` exits non-zero: stderr `pyenv: failed to configure fish universal variables`, exit 1, and `config.fish` is
     not written [probe].
   - The fish block is never written to `config.fish`. Only `pyenv init - fish | source` is.
6. **Append** each text with `append_lines` [src libexec/pyenv-init:388-406]:
   - `mkdir -p` the parent directory (`~/.config/fish`, `~/.config/powershell`);
   - if the file is non-empty and its last byte is not a newline, first append `\n`. An empty (0-byte) file gets no
     extra newline [probe];
   - then append the text plus `\n`. No blank separator line and no marker comment is added.
   - Writes go through symlinks: a `.bashrc` symlink to `dotfiles/bashrc` had the target appended, and a dangling
     `.profile` symlink had its missing target **created** [probe].
7. On success: no stdout, no stderr, exit 0 [probe].

Resulting files [probe] [test test/init.bats:96-159]:

| invocation (fresh `HOME`, default root) | files written |
|---|---|
| `--install bash` or `--install` from a bash parent | `.bashrc`, `.profile` |
| `--install bash` with an existing `.bash_profile` | `.bashrc`, `.bash_profile`; no `.profile` |
| `--install zsh` | `.zshrc`, `.zprofile` |
| `--install ksh` / `ksh93` / `mksh` | `.profile` only |
| `--install fish` | `.config/fish/config.fish`, plus the `fish -c` call |
| `--install pwsh` | `.config/powershell/profile.ps1` |

Content for bash (zsh and ksh differ only in the shell name on the last line):

```text
export PYENV_ROOT="$HOME/.pyenv"
[[ -d $PYENV_ROOT/bin ]] && export PATH="$PYENV_ROOT/bin:$PATH"
eval "$(pyenv init - bash)"
```

pwsh:

```text
$Env:PYENV_ROOT="$Env:HOME/.pyenv"
if (Test-Path -LP "$Env:PYENV_ROOT/bin" -PathType Container) {
  $Env:PATH="$Env:PYENV_ROOT/bin:$Env:PATH" }
iex ((pyenv init -) -join "`n")
```

- Appending to `alias ll=ls` (no trailing newline) gave `alias ll=ls\n` followed by the 3 lines [probe]. Appending to
  `config.fish` containing `end` with no newline gave `end\npyenv init - fish | source\n` [probe]
  [test test/init.bats:210-222 uses `end\n`].
- Custom root `/opt/py env/$x"y`z\w` wrote `export PYENV_ROOT="/opt/py env/\$x\"y\`z\\w"` to both bash files.
  `/opt/it's` with pwsh wrote `$Env:PYENV_ROOT='/opt/it''s'` [probe].
- **Idempotence by refusal:** a second `--install bash` fails with the "already contain Pyenv-related code" message
  for `.bashrc`, exit 1, and changes nothing [probe].

---

## `pyenv shell` and `pyenv sh-shell` — `libexec/pyenv-sh-shell`

### Without the shell function

`pyenv shell …` with any arguments: the dispatcher finds no `pyenv-shell` and has a special case for that name. It
prints ``pyenv: shell integration not enabled. Run `pyenv init' for instructions.`` on stderr and exits 1
[src libexec/pyenv:126-129] [probe] [test test/shell.bats:5-8]. Calling `pyenv sh-shell …` directly works and prints
shell code.

Help: `pyenv help shell` and `pyenv help sh-shell` print the same text, because `pyenv-help` falls back to
`pyenv-sh-<cmd>` [src libexec/pyenv-help:26] [probe]. `pyenv help --usage shell` prints the three `Usage:` lines
[probe]:

```text
Usage: pyenv shell <version>...
       pyenv shell -
       pyenv shell --unset
```

### Behavior of `pyenv sh-shell`

The shell is `basename "${PYENV_SHELL:-$SHELL}"` [src libexec/pyenv-sh-shell:33]. So `PYENV_SHELL=/usr/bin/pwsh`
selects pwsh, and with `PYENV_SHELL` unset, `SHELL=/usr/bin/fish` selects fish [probe]. The families are `fish`,
`pwsh`, and everything else (POSIX), including `zsh`, `ksh`, `sh` and `nu` [probe]. Only the **first** argument is
checked for `--unset` / `-` [src libexec/pyenv-sh-shell:35,45,62].

1. **`--complete`** (first argument): prints `--unset`, `system`, then execs `pyenv-versions --bare`
   [src libexec/pyenv-sh-shell:26-30] [probe].
2. **No arguments, or an empty first argument** [src :35-43] [probe] [test test/shell.bats:16-37]:
   - `PYENV_VERSION` unset or empty: stderr `pyenv: no shell-specific version configured`, exit 1, empty stdout;
   - otherwise stdout `echo "$PYENV_VERSION"`, exit 0, **for every shell, pwsh included**. In pwsh that line prints
     the *pwsh variable* `$PYENV_VERSION`, not `$Env:PYENV_VERSION`, so it prints an empty line. Running
     `iex 'echo "$PYENV_VERSION"'` with `$Env:PYENV_VERSION='3.12.1'` produced one empty string
     [probe, win-pwsh].
3. **`--unset`** (extra arguments ignored), exit 0 [src :45-60] [probe] [test test/shell.bats:57-81]:
   ```text
   PYENV_VERSION_OLD="${PYENV_VERSION-}"
   unset PYENV_VERSION
   ```
   ```text
   set -gu PYENV_VERSION_OLD "$PYENV_VERSION"
   set -e PYENV_VERSION
   ```
   ```text
   $Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = $null, $Env:PYENV_VERSION
   ```
   These are the POSIX, fish and pwsh forms in that order.
4. **`-`** (extra arguments ignored), exit 0. The code is printed unconditionally, and the check happens when the
   shell runs it [src :62-112] [probe] [test test/shell.bats:39-55]. POSIX:
   ```text
   if [ -n "${PYENV_VERSION_OLD+x}" ]; then
     if [ -n "$PYENV_VERSION_OLD" ]; then
       PYENV_VERSION_OLD_="$PYENV_VERSION"
       export PYENV_VERSION="$PYENV_VERSION_OLD"
       PYENV_VERSION_OLD="$PYENV_VERSION_OLD_"
       unset PYENV_VERSION_OLD_
     else
       PYENV_VERSION_OLD="$PYENV_VERSION"
       unset PYENV_VERSION
     fi
   else
     echo "pyenv: PYENV_VERSION_OLD is not set" >&2
     false
   fi
   ```
   fish:
   ```text
   if set -q PYENV_VERSION_OLD
     if [ -n "$PYENV_VERSION_OLD" ]
       set PYENV_VERSION_OLD_ "$PYENV_VERSION"
       set -gx PYENV_VERSION "$PYENV_VERSION_OLD"
       set -gu PYENV_VERSION_OLD "$PYENV_VERSION_OLD_"
       set -e PYENV_VERSION_OLD_
     else
       set -gu PYENV_VERSION_OLD "$PYENV_VERSION"
       set -e PYENV_VERSION
     end
   else
     echo "pyenv: PYENV_VERSION_OLD is not set" >&2
     false
   end
   ```
   pwsh. The fifth line `} ` ends with a **trailing space**:
   ```text
   if ( Get-Item -Path Env:\PYENV_VERSION* ) {
     $Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = $Env:PYENV_VERSION_OLD, $Env:PYENV_VERSION
   } else {
     Write-Error "pyenv: Env:PYENV_VERSION_OLD is not set"
     return $false
   } 
   ```
5. **A version list.** `pyenv-prefix "${versions[@]}" >/dev/null` validates **all** arguments [src :115]:
   - **On failure**, pyenv-prefix's message goes to stderr, for example ``pyenv: version `1.2.3' not installed``.
     Stdout is exactly `false`, and the exit code is 1. So `eval` makes the function return 1 [probe]
     [test test/shell.bats:83-90]. One bad name among good ones fails the whole call. An option-looking argument
     is just a version name: `pyenv sh-shell -x` and `pyenv sh-shell 3.12.1 --unset` both fail with
     ``version `-x' `` / ``version `--unset' not installed`` [probe].
   - **On success**, the arguments are joined with `:` [src :116-118]. If the result equals the current
     `PYENV_VERSION`, **nothing is printed** and the exit code is 0 [probe]. Otherwise, exit 0 [probe]
     [test test/shell.bats:92-119]:
     ```text
     PYENV_VERSION_OLD="${PYENV_VERSION-}"
     export PYENV_VERSION="3.12.1"
     ```
     ```text
     set -gu PYENV_VERSION_OLD "$PYENV_VERSION"
     set -gx PYENV_VERSION "3.12.1"
     ```
     ```text
     $Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = "3.12.1", $Env:PYENV_VERSION
     ```
   - The value is the **literal argument**, not the resolved version: `pyenv sh-shell 3.12` prints
     `export PYENV_VERSION="3.12"`, and `3` prints `"3"`, when only 3.12.1 is installed [probe]. `pyenv version` then
     reports `3.12.1 (set by PYENV_VERSION environment variable)` [probe]. Prefix resolution belongs to
     `pyenv-prefix` (M1).
   - Multiple versions: `pyenv sh-shell 3.12.1 3.11.9` prints `export PYENV_VERSION="3.12.1:3.11.9"` (fish and pwsh
     the same way) [probe].
   - `system` succeeded on the probe host, which has a system Python [probe]. When no system Python exists,
     pyenv-prefix prints `pyenv: system version not found in PATH` [src libexec/pyenv-prefix:44]. Not probed.
   - The version is placed inside `"…"` without escaping [src :123,126,130].

**pwsh semantics** of these snippets [probe, win-pwsh]. They were run in a function with `iex`, the way the `pyenv`
function runs them:

| sequence | `PYENV_VERSION` / `PYENV_VERSION_OLD` after | output |
|---|---|---|
| set 3.12.1 (nothing set) | `3.12.1` / unset | |
| set 3.11.9 | `3.11.9` / `3.12.1` | |
| `-` | `3.12.1` / `3.11.9` | |
| `--unset` (from `3.11.9`/`3.12.1`) | unset / `3.11.9` | |
| `-` after `--unset` | `3.11.9` / unset | |
| `-` with **only `PYENV_VERSION`** set to 3.12.1 | unset / `3.12.1` | (none) |
| `-` with nothing set | unset / unset | error record `pyenv: Env:PYENV_VERSION_OLD is not set`, then `False` |
| `--unset` with nothing set | unset / unset | |

Two differences from POSIX: assigning `$null` or `""` to an `$Env:` variable removes it, so pwsh never holds an
"empty but set" `PYENV_VERSION_OLD`. And `-` with only `PYENV_VERSION` set **swaps** (unsetting `PYENV_VERSION`),
where bash prints `pyenv: PYENV_VERSION_OLD is not set` and fails. `return $false` emits `False` into the output.

---

## `pyenv sh-rehash` — `libexec/pyenv-sh-rehash`, and `pyenv rehash` through the function

- **`--complete`** (first argument) runs `exec pyenv-rehash --complete` [src libexec/pyenv-sh-rehash:6-8].
  `pyenv-rehash` has no `--complete` handling and ignores its arguments (M1). **So this performs a real rehash**:
  in a fresh root with versions installed, `pyenv sh-rehash --complete` printed nothing, exited 0, and created
  `shims/pip` and `shims/python` [probe]. The same happens through `pyenv completions sh-rehash`, which prints
  `--help` and then rehashes [probe].
- Otherwise, the shell is `basename "${PYENV_SHELL:-$SHELL}"` [src :10]. Extra arguments are ignored [probe]. Exit 0.
  `sh-rehash` **does not rehash itself**: it only prints code [probe: no `shims` directory after `pyenv sh-rehash`].

| shell | stdout |
|---|---|
| `bash`, `zsh`, `ksh`, `sh`, `nu`, empty | `command pyenv rehash` then `hash -r 2>/dev/null \|\| true` |
| `fish` | `command pyenv rehash` |
| `pwsh` | `& (get-command pyenv -commandtype application) rehash` |

[src :14-29] [probe] [test test/rehash.bats:285-317]. Note that the pwsh line here uses `get-command pyenv
-commandtype application` without `-totalcount 1`, unlike the `pyenv` function from `pyenv init`.

- `pyenv sh-rehash --help` prints `pyenv help "sh-rehash"` and exits 0 (dispatcher) [probe]. `pyenv help sh-rehash`
  prints `Sorry, this command isn't documented yet.` on stderr and exits 1, because the script has no
  `Summary:`/`Usage:` block [probe].
- `pyenv rehash` **without** the function runs `pyenv-rehash` directly (M1), with no `hash -r`.
- `pyenv rehash` **through the bash function** evals the two lines. Shims were created, and the shell's hash table was
  empty afterwards [probe] [test test/rehash.bats:292-297]. `pyenv rehash foo` behaves the same [probe].

---

## `pyenv completions` — `libexec/pyenv-completions`

```text
# Usage: pyenv completions <command> [arg1 arg2...]
```
[src libexec/pyenv-completions:2]. Steps [src :7-26] [probe]:

1. No argument (or an empty one): stderr `Usage: pyenv completions <command> [arg1 arg2...]`, exit 1.
2. `--complete` as the command: `exec pyenv-commands`, the full command list (see below).
3. `COMMAND_PATH=$(command -v pyenv-<cmd> || command -v pyenv-sh-<cmd>)`. If neither exists, `set -e` aborts:
   **no output at all (not even `--help`), exit 1** [probe: `pyenv completions nosuchcmd`].
4. Print `--help`.
5. If the command file has a line matching `^([#%]|--|//) provide pyenv completions` (grep `-iE`, case-insensitive),
   drop the command name and `exec <path> --complete <remaining args>`. Otherwise stop, exit 0.

Consequences:
- **`pyenv-<cmd>` wins over `pyenv-sh-<cmd>`.** `pyenv completions rehash` finds `pyenv-rehash`, which has no
  marker, and prints only `--help`. `pyenv completions shell` finds only `pyenv-sh-shell` and prints `--help --unset
  system <versions>`. `pyenv completions sh-shell` gives the same [probe].
- **The marker decides, not the handler.** `pyenv-version` handles `--complete` (prints `--bare`, M1), but has no
  marker line, so `pyenv completions version` prints only `--help` [probe].
- **Extra arguments are forwarded** [test test/completions.bats:35-52]. For the built-in commands the extras are
  ignored: `pyenv completions shell foo bar` gives the same output as `pyenv completions shell` [probe].
- `pyenv completions --help` is intercepted by the dispatcher (M1) and prints `pyenv help completions`: the usage line
  on **stdout**, exit 0 [probe].

**Commands with a marker** (found with `grep -ilE` over `libexec/*` and `plugins/*/bin/*`) [probe]: `commands`,
`completions`, `exec`, `global`, `help`, `hooks`, `init`, `local`, `prefix`, `shims`, `sh-rehash`, `sh-shell`,
`versions`, `whence`, `which`, `binary`, `link`, `link-version`, `install`, `install-prerequisites`, `uninstall`.
**Without a marker:** `pyenv` (dispatcher), `--version`, `latest`, `rehash`, `root`, `version`, `version-file`,
`version-file-read`, `version-file-write`, `version-name`, `version-origin`, and `python-build` (not a command).

`pyenv commands` at 2.8.8, with the vendored plugins [probe]:

```text
--version binary commands completions exec global help hooks init install install-prerequisites latest link link-version local prefix rehash root shell shims uninstall version version-file version-file-read version-file-write version-name version-origin versions whence which
```

(one name per line in real output). `shell` is listed because `pyenv-commands` strips `sh-` (M1), and `rehash`
appears once because `sort -u` merges `pyenv-rehash` and `pyenv-sh-rehash`. `pyenv commands --no-sh` prints the
same list without `shell`. `rehash` stays, from `pyenv-rehash` [probe].

### `pyenv completions <cmd>` for every command, at 2.8.8

Produced mechanically by looping over `pyenv commands` plus `sh-shell` and `sh-rehash`. The root had one version,
3.12.1, with executables `pip` and `python`. Every call exited 0 [probe]. Output is shown space-joined:

| command | output |
|---|---|
| `--version`, `latest`, `rehash`, `root`, `version`, `version-file`, `version-file-read`, `version-file-write`, `version-name`, `version-origin` | `--help` |
| `sh-rehash` | `--help` (and rehashes, see above) |
| `commands` | `--help --sh --no-sh` |
| `completions` | `--help` then the full `pyenv commands` list |
| `help` | `--help --usage` then the full `pyenv commands` list |
| `exec` | `--help --environment pip python` |
| `global`, `prefix` | `--help system 3.12.1` |
| `local`, `shell`, `sh-shell` | `--help --unset system 3.12.1` |
| `hooks` | `--help exec rehash version-name version-origin which` |
| `init` | `--help - --path --install --no-push-path --no-rehash --detect-shell bash fish ksh pwsh zsh` |
| `shims` | `--help --short` |
| `versions` | `--help --bare --skip-aliases --skip-envs` |
| `whence` | `--help --path pip python` |
| `which` | `--help pip python` |
| `install` | `--help --bare --list --force --skip-existing --keep --patch --verbose --version --debug`, then every definition name (1135 lines in all) |
| `uninstall` | `--help --force 3.12.1` |
| `install-prerequisites` | `--help --all` |
| `binary` | `--help generate-installer install package package-name relocate save` |
| `link` | `--help version` |
| `link-version` | `--help --dry --quiet` |

- `install`: the 1125 lines after the 10 option lines equal `pyenv install --list --bare` byte for byte [probe]. M2
  records the nine option names.
- `exec` at 2.8.8 is `--help`, `--environment`, then `pyenv-shims --short` [probe] [test test/exec.bats:27-41].
- `pyenv completions binary package --x` forwards to `pyenv-binary --complete package --x`. That runs
  `pyenv-binary-package --complete`, which prints `--archive-base-url`, `--verbose`, then
  `pyenv install --list --bare` [src plugins/pyenv-binary/bin/pyenv-binary:32-42,
  plugins/pyenv-binary/libexec/pyenv-binary-package:25-29] [probe: the output ended with the definition list].
- `pyenv completions link version` → `--help --dry --quiet` [probe].

### `--complete` outputs not recorded in M1 or M2

| invocation | stdout | exit | evidence |
|---|---|---|---|
| `pyenv init --complete` | `-`, `--path`, `--install`, `--no-push-path`, `--no-rehash`, `--detect-shell`, `bash`, `fish`, `ksh`, `pwsh`, `zsh` | 0 | [probe] [src libexec/pyenv-init:11-24] |
| `pyenv sh-shell --complete` | `--unset`, `system`, then `pyenv-versions --bare` | 0 | [probe] |
| `pyenv sh-rehash --complete` | (nothing; performs a rehash) | 0 | [probe] |
| `pyenv completions --complete` | `pyenv-commands` output | 0 | [probe] |
| `pyenv exec --complete` | `--environment`, then `pyenv-shims --short` (**changed at 2.8.8**) | 0 | [src libexec/pyenv-exec:30-33] [test test/exec.bats:27-41] |
| `pyenv version --complete` | `--bare` (reachable only directly, not via `pyenv completions`) | 0 | [probe] |
| `pyenv-binary --complete` | the subcommand list; with a subcommand, that subcommand's `--complete` | 0 | [src plugins/pyenv-binary/bin/pyenv-binary:32-42] [probe] |
| `pyenv-link --complete` | `version`; with a second argument, `pyenv-link-version --complete` | 0 | [src plugins/pyenv-link/bin/pyenv-link:16-24] [probe] |
| `pyenv-link-version --complete` | `--dry`, `--quiet` | 0 | [src plugins/pyenv-link/bin/pyenv-link-version:71-75] [probe] |

`latest` has no `--complete` handling at all (no marker, no branch) [src].

### The completion scripts — `completions/pyenv.{bash,zsh,fish,pwsh}`

These are loaded by the `source` / `iex` line of print mode. All four call only `pyenv commands` and
`pyenv completions …`. They print nothing themselves.

- **bash** (`completions/pyenv.bash:1-16`). It registers `complete -F _pyenv pyenv`. For the first word,
  `COMPREPLY` is `compgen -W "$(pyenv commands)" -- "$word"`. Otherwise it calls `pyenv completions` with all words
  except `pyenv` and the current word, then filters with `compgen -W` by prefix. Executed by sourcing the file and
  setting `COMP_WORDS`/`COMP_CWORD` [probe]:
  - `pyenv ""` → all 30 command names;
  - `pyenv sh` → `shell shims`;
  - `pyenv re` → `rehash`;
  - `pyenv shell ""` → `--help --unset system 3.12.1`;
  - `pyenv shell --` → `--help --unset`;
  - `pyenv init --n` → `--no-push-path --no-rehash`;
  - `pyenv exec ""` → `--help --environment pip python`;
  - `pyenv rehash ""` → `--help`;
  - `pyenv local 3.12.1 ""` → `--help --unset system 3.12.1`. The earlier word `3.12.1` is passed through and
    ignored.
  - `sh-*` names are not offered for the first word, because `pyenv commands` strips the prefix.
- **zsh** (`completions/pyenv.zsh:1-18`) [src]. It returns immediately unless the shell is interactive
  (`[[ ! -o interactive ]]`). It uses the old `compctl -K _pyenv pyenv` system. `read -cA words` reads the command
  line. With 2 words it uses `pyenv commands`, otherwise `pyenv completions ${words[2,-2]}`. The reply splits on
  newlines, and zsh does the prefix filtering. Not executed (zsh absent).
- **fish** (`completions/pyenv.fish:1-23`) [src]. It defines `__fish_pyenv_needs_command` (only `pyenv` typed) and
  `__fish_pyenv_using_command <cmd>`. It registers `complete -f -c pyenv -n '__fish_pyenv_needs_command' -a
  '(pyenv commands)'`, then, **once per command at load time** (`for cmd in (pyenv commands)`), a completion whose
  candidates are `(pyenv completions (commandline -opc)[2..-1])`. Loading the file therefore runs `pyenv commands`
  once. Not executed.
- **pwsh** (`completions/pyenv.pwsh:1-17`) [src]. It registers a native argument completer for `pyenv`. It splits the
  text up to the cursor on whitespace. With two or more words it runs `pyenv completions $words[1]`, so **only the
  subcommand name is passed, never the later words**. Otherwise it runs `pyenv commands`. Results are filtered with
  `-match $wordToComplete`, a **regex substring** match, not a prefix match. Not executed.

---

## Upstream bats tests in rpyenv's M3 target

`parity/expected/bats.txt` has 58 rows tagged M3. Each row below is matched by test name to the pinned files. Line
numbers come from `grep -n '^@test'` [probe]. Every one passed in the harness run above.

### `test/init.bats` (35 rows)

| line | test | asserts |
|---|---|---|
| 5 | creates shims and versions directories | `pyenv-init -` succeeds and creates `$PYENV_ROOT/shims` and `/versions` |
| 14 | auto rehash | `pyenv-init -` output has the line `command pyenv rehash` |
| 20 | auto rehash for --path | `pyenv-init --path` output has `command pyenv rehash` |
| 26 | setup shell completions | `pyenv-init - bash` has `source '<prefix>/completions/pyenv.bash'` |
| 32 | detect parent shell | with `SHELL=/bin/false`, `pyenv-init -` prints `export PYENV_SHELL=bash` (parent is bash) |
| 38 | detect parent shell from script | a `#!/bin/sh` script that evals `pyenv-init -` sees `PYENV_SHELL` = `sh` |
| 51 | setup shell completions (fish) | `pyenv-init - fish` has `source '<prefix>/completions/pyenv.fish'` |
| 57 | fish instructions | `pyenv-init fish` exits 1 and prints `pyenv init - fish \| source` |
| 63 | setup shell completions (pwsh) | `pyenv-init - pwsh` has `iex (gc <prefix>/completions/pyenv.pwsh -Raw)` |
| 70 | pwsh instructions | `pyenv-init pwsh` exits 1 and prints ``iex ((pyenv init -) -join "`n")`` |
| 76 | shell detection for installer | `pyenv-init --detect-shell` succeeds with `PYENV_SHELL_DETECT=bash` |
| 82 | shell detection for fish startup file | `--detect-shell fish` prints fish and both `~/.config/fish/config.fish` lines |
| 90 | completion includes install option | `pyenv-init --complete` has the line `--install` |
| 96 | install setup for detected shell startup files (default PYENV_ROOT) | `--install` (bash parent) writes the 3-line setup to `.bashrc` and `.profile` |
| 108 | install setup for bash uses existing bash_profile (default PYENV_ROOT) | with `.bash_profile` present, `--install bash` writes `.bashrc` and `.bash_profile`, not `.profile` |
| 122 | install setup for zsh startup files (default PYENV_ROOT) | `--install zsh` writes the `pyenv init - zsh` setup to `.zshrc` and `.zprofile` |
| 134 | install setup for fish startup file (default PYENV_ROOT) | `--install fish` passes the 6-line block as `fish -c`'s 2nd argument and writes `pyenv init - fish \| source` to `config.fish` |
| 150 | install setup for pwsh startup file (default PYENV_ROOT) | `--install pwsh` writes the 4-line pwsh setup to `~/.config/powershell/profile.ps1` |
| 161 | init honors custom PYENV_ROOT and quotes it according to each shell's rules | help for bash, fish and pwsh (exit 1) prints the escaped custom-root lines in the table above |
| 177 | install refuses to modify files with pyenv-related code | `.bashrc` holding `eval "$(pyenv init -)"`: fails with the 3 messages, file unchanged, no `.profile` |
| 191 | install treats PYENV_ROOT as pyenv-related code | `.bashrc` holding `export PYENV_ROOT=…`: fails with the "already contain" message |
| 200 | install refuses unreadable startup file without partial writes | `.bashrc` is a directory: fails with `pyenv: failed to inspect <home>/.bashrc`, no `.profile` |
| 210 | install setup keeps fish block intact when generic lines already exist | `config.fish` = `end`, stub fish: succeeds, file becomes `end` + `pyenv init - fish \| source` |
| 224 | install setup fails gracefully for unsupported shell | `--install nu` fails with output exactly `pyenv: cannot automatically configure startup files for nu` |
| 231 | option to skip rehash | `pyenv-init - --no-rehash` succeeds and has no line `pyenv rehash 2>/dev/null` (a line pyenv never prints, so the refute is vacuous) |
| 237 | adds shims to PATH | `pyenv-init - bash` has `export PATH="<root>/shims:${PATH}"` |
| 244 | adds shims to PATH (fish) | `pyenv-init - fish` has `set -gx PATH '<root>/shims' $PATH` |
| 251 | adds shims to PATH (pwsh) | `pyenv-init - pwsh` has `$Env:PATH="<root>/shims:$Env:PATH"` |
| 258 | removes existing shims from PATH | in bash, eval of `pyenv-init -` moves shims from position 2 to the front, once |
| 295 | adds shims to PATH with --no-push-path if they're not on PATH | in bash, eval of `- --no-push-path` prepends shims when absent |
| 330 | doesn't change PATH with --no-push-path if shims are already on PATH | in bash, eval of `- --no-push-path` leaves PATH identical when shims are present |
| 365 | outputs sh-compatible syntax | `- bash` and `- zsh` both have the line `  case "$command" in` |
| 375 | outputs sh-compatible case syntax | with `pyenv-commands` stubbed: line `  activate\|deactivate\|rehash\|shell)`; with an empty list: `  /)` |
| 391 | outputs fish-specific syntax (fish) | `- fish` has `  switch "$command"` and not `  case "$command" in` |
| 398 | outputs pwsh-specific syntax (pwsh) | `- pwsh` has neither `  switch "$command"` nor `  case "$command" in` |

### `test/shell.bats` (15 rows)

| line | test | asserts |
|---|---|---|
| 10 | shell integration enabled | after `eval "$(pyenv init -)"`, `pyenv shell` **succeeds** with output `pyenv: no shell-specific version configured` |
| 16 | no shell version | `PYENV_VERSION=""` (with a `.python-version` present): `pyenv-sh-shell` fails with that message |
| 24 | shell version | `PYENV_SHELL=bash PYENV_VERSION=1.2.3`: succeeds with `echo "$PYENV_VERSION"` |
| 29 | shell version (fish) | same output for fish |
| 34 | shell version (pwsh) | same output for pwsh |
| 39 | shell revert | bash `-`: first line `if [ -n "${PYENV_VERSION_OLD+x}" ]; then` |
| 45 | shell revert (fish) | fish `-`: first line `if set -q PYENV_VERSION_OLD` |
| 51 | shell revert (pwsh) | pwsh `-`: first line `if ( Get-Item -Path Env:\PYENV_VERSION* ) {` |
| 57 | shell unset | bash `--unset`: exactly the two POSIX lines |
| 66 | shell unset (fish) | fish `--unset`: exactly the two fish lines |
| 75 | shell unset (pwsh) | pwsh `--unset`: exactly the one pwsh line |
| 83 | shell change invalid version | `pyenv-sh-shell 1.2.3` fails; merged output is ``pyenv: version `1.2.3' not installed`` then `false` |
| 92 | shell change version | bash, 1.2.3 installed: exactly `PYENV_VERSION_OLD="${PYENV_VERSION-}"` / `export PYENV_VERSION="1.2.3"` |
| 102 | shell change version (fish) | exactly `set -gu PYENV_VERSION_OLD "$PYENV_VERSION"` / `set -gx PYENV_VERSION "1.2.3"` |
| 112 | shell change version (pwsh) | exactly `$Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = "1.2.3", $Env:PYENV_VERSION` |

### `test/rehash.bats`, the `sh-rehash` tests (4 rows)

| line | test | asserts |
|---|---|---|
| 285 | sh-rehash in bash | `PYENV_SHELL=bash pyenv-sh-rehash` outputs exactly `command pyenv rehash` / `hash -r 2>/dev/null \|\| true` |
| 292 | sh-rehash in bash (integration) | `eval "$(pyenv-sh-rehash)"` succeeds and creates the executable `shims/python` |
| 299 | sh-rehash in fish | fish output is exactly `command pyenv rehash` |
| 313 | sh-rehash in pwsh | pwsh output is exactly `& (get-command pyenv -commandtype application) rehash` |

### `test/completions.bats` (3 rows) and `test/exec.bats` (1 row)

| file:line | test | asserts |
|---|---|---|
| completions.bats:12 | command with no completion support | a `pyenv-hello` without the marker: `pyenv-completions hello` succeeds with exactly `--help` |
| completions.bats:19 | command with completion support | with `# Provide pyenv completions`: output `--help`, then `hello` |
| completions.bats:35 | forwards extra arguments | lowercase `# provide pyenv completions` marker; `hello happy world` → `--help`, `happy`, `world` |
| exec.bats:27 | completes with names of executables | `pyenv-completions exec` with `fab` and `python` in version 3.4: exactly `--help`, `--environment`, `fab`, `python` |

### Tests in the same files that are not M3 rows

[probe: names from `grep -n '^@test'`]:
- `shell.bats:5` "shell integration disabled": `pyenv shell` fails with the "shell integration not enabled" message.
  It is not in `bats.txt`. Inferred reason: rpyenv already passes it, since `bats.txt` lists only expected failures.
- Eight skip-guarded tests that need a real fish or pwsh: `init.bats:269, 282, 305, 317, 340, 352` (fish and pwsh
  versions of the three PATH tests) and `rehash.bats:305, 319` (sh-rehash fish and pwsh integration). Each starts
  with `command -v fish/pwsh >/dev/null || skip`. rpyenv's checker counts a skip as a pass
  [src parity/bats_check.py:21]. They skipped in this harness too.
- The other 21 `rehash.bats` tests and 9 `exec.bats` tests belong to M1 (some appear in `bats.txt` with D-numbers)
  or to `--environment`.

---

## Open points

- **zsh, fish, ksh/mksh execution: UNCONFIRMED.** None of these shells exists on the probe host. How they *run* the
  emitted code is known only from source and from upstream's fish/pwsh tests, and those tests skipped here. Examples:
  zsh evaluating the bash-built PATH code, fish's `contains`/`set -eg` loop, ksh's `typeset`, and the zsh/fish
  completion scripts.
- **The pwsh `pyenv` function and `completions/pyenv.pwsh`: UNCONFIRMED.** Only the env-var and PATH snippets ran
  (on Windows pwsh 7.6.6, not Linux pwsh). The full function was deliberately not run (see "Probe environment").
- **`ps` fallback of shell detection: UNCONFIRMED.** It is used when `/proc/$PPID/cmdline` cannot be read, which
  could not be arranged on Linux.
- **`SHELL` unset with an empty `argv[0]`:** the observed `bash` result is attributed to bash filling `SHELL` from
  passwd. That attribution is inferred, not traced.
- **`grep` exit ≥ 2 path in `check_startup_file`: UNCONFIRMED** by probe. The unreadable and non-regular cases are
  caught earlier by `! -f || ! -r`, so that branch is hard to reach.
- **`pyenv sh-shell system` without any system Python: UNCONFIRMED** by probe. Source says pyenv-prefix prints
  `pyenv: system version not found in PATH`, and sh-shell then prints `false` and exits 1.
- **Why the 9 non-M3 tests are absent from `bats.txt`:** inferred, not checked against rpyenv's CI image.
