# Installing rpyenv on Windows: details

The README has the short version. This page covers what the MSI's options do,
PowerShell's execution policy, silent installs, and uninstalling.

From v0.2.0 the MSIs and the programs in them are signed with a Certum code-signing
certificate. SmartScreen may still warn while the certificate has no download reputation:
choose "More info", then "Run anyway". v0.1.0 is **unsigned**: SmartScreen always warns.
On a PC with Smart App Control turned on, Windows blocks its programs, rpyenv's shims included.

## The MSI's options

Run `rpyenv-<version>-x64.msi` (or `-arm64.msi`). The Options page asks:

| Option | What it does |
|---|---|
| **Just for me** | No administrator rights needed. Installs to `%LOCALAPPDATA%\Programs\rpyenv`. |
| **For all users** | Windows asks for administrator rights. Installs to `%ProgramFiles%\rpyenv`. |
| **Enable live rehash** (off by default) | New scripts get shims while a program runs. |
| **Move pyenv-win aside** (just for me, and shown when pyenv-win is installed) | Runs `pyenv migrate`. |
| **Let Windows PowerShell run local scripts for me if it blocks them** (just for me, checked) | Runs `pyenv setup --allow-scripts` (see below). |

Each user's Python versions stay in their own `PYENV_ROOT` either way.

## What setup does

- **Just for me** also runs `pyenv setup`. It puts the shims first on your `PATH` and
  adds the PowerShell line to your profiles.
- **For all users:** Windows runs each user's setup at their next sign-in (Active Setup),
  or rpyenv runs it on their first `pyenv` command.

## PowerShell's execution policy

`python`, `pip` and the other shims work in every shell, cmd included, whatever
PowerShell's execution policy is. Only `pyenv shell`, `pyenv activate` and
`pyenv deactivate` need the profile line.

Windows PowerShell 5.1 runs a profile only where its execution policy allows local
scripts. Client Windows' default policy, `Restricted`, doesn't. So:

- **Without the checkbox**, setup doesn't change Windows PowerShell's profile there. It
  says how to allow scripts.
- **With the checkbox** (or `pyenv setup --allow-scripts`), setup first sets your own
  (CurrentUser) policy to `RemoteSigned`. It doesn't do this if Group Policy, an
  administrator (LocalMachine) or you already chose a policy.
- `pyenv setup --undo`, which uninstalling runs, restores the policy.

To do it by hand:

```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
pyenv init --install pwsh
```

PowerShell 7 runs profiles by default.

## Silent installs

```powershell
msiexec /i rpyenv-<version>-x64.msi /qn                                       # just for me
sudo msiexec /i rpyenv-<version>-x64.msi /qn ALLUSERS=1 MSIINSTALLPERUSER=""  # all users, elevated
# options: LIVEREHASH=1, MIGRATEPYENVWIN=1 and ALLOWSCRIPTS=1 (just for me)
```

- A silent install changes no execution policy unless you give it `ALLOWSCRIPTS=1`.
- On Windows Server, Windows Installer may be turned off for non-administrators (the
  `DisableMSI` policy). GitHub's windows-2025 image has it on (error 1625). Install as
  an administrator there.

## Uninstalling

Uninstall from Settings, Apps. Your Python versions stay. Uninstalling runs, for you:

- `pyenv migrate --restore`, if `pyenv migrate` moved pyenv-win aside.
- `pyenv setup --undo`.

**Other users of an all-users install** can run `pyenv setup --undo` themselves first.
If they don't, their PowerShell profiles keep the pyenv line. The line then fails in every
new PowerShell window ("pyenv is not recognized"). To fix that, delete the line with
`pyenv init` from the files that `$PROFILE` names, in Windows PowerShell and in
PowerShell 7.

**If the checkbox set your execution policy**, uninstalling restores it (to `Undefined`,
if it is still `RemoteSigned`). Windows PowerShell then blocks local scripts again. The
policy covers every script, not only rpyenv's. If another tool's profile or scripts
now rely on it, run this after uninstalling:

```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
```
