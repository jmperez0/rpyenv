# Intentional differences from upstream

rpyenv matches the contract of pyenv on Linux and pyenv-win on Windows, but not their bugs (design spec §4, "Parity policy"). Every intentional difference is listed here with its reason. The differential tests (M1c) read this file.

| ID | OS | Command | Upstream | rpyenv | Reason |
|---|---|---|---|---|---|
| D-01 | both | `--version` | `pyenv 2.8.6` (Linux), `pyenv 3.1.1` (Windows) | `pyenv 2.8.6 (rpyenv 0.1.0)`, `pyenv 3.1.1 (rpyenv 0.1.0)` | Identifies rpyenv, and keeps the upstream version it matches parseable. |
| D-02 | Windows | `--version` | Warns when `PYENV`, `PYENV_ROOT` or `PYENV_HOME` is unset | No warnings | Advice specific to pyenv-win's installer; rpyenv doesn't need these variables. |
| D-03 | both | version files | A leading UTF-8 BOM becomes part of the first version name | The BOM is ignored | Files saved by some Windows editors would otherwise never match an installed version. |
| D-04 | Linux | version files | A word longer than 1024 characters is split in two (`read -n 1024`) | The word is kept whole | Mis-parsing artifact of bash `read`. |
| D-07 | Linux | `versions` | Envs in bash glob order, which depends on the locale | Envs in byte order (C locale) | Deterministic output. |
| D-17 | Windows | `versions` and others | A missing `versions` folder is created as a side effect | Nothing is created | Read-only commands shouldn't change the disk. |
