# Intentional differences from upstream

rpyenv matches the contract of pyenv on Linux and pyenv-win on Windows, but not their bugs (design spec §4, "Parity policy"). Every intentional difference is listed here with its reason. The differential tests (M1c) read this file.

| ID | OS | Command | Upstream | rpyenv | Reason |
|---|---|---|---|---|---|
| D-01 | both | `--version` | `pyenv 2.8.6` (Linux), `pyenv 3.1.1` (Windows) | `pyenv 2.8.6 (rpyenv 0.1.0)`, `pyenv 3.1.1 (rpyenv 0.1.0)` | Identifies rpyenv, and keeps the upstream version it matches parseable. |
