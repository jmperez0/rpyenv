# rpyenv MSI: interactive checks in Windows Sandbox

CI already installs, upgrades and uninstalls the MSI silently, both ways, and as a
non-administrator (`installer/test_msi.py`). These checks cover what CI can't see: the
dialogs, the UAC prompt, Active Setup and a silent install from a non-elevated prompt.

Automated (steps 1-4 and 6): `pwsh -NoProfile -File installer/sandbox/run-checks.ps1`
(Windows 11 24H2+, for its `wsb` CLI). It starts a Sandbox, clicks through the installer's
pages with UI Automation as the steps below say, and prints PASS/FAIL per expectation. Each
step's `report.txt`, page screenshots and verbose MSI logs land in
`dist\sandbox-results\<time>\<step>`. Step 1 takes the default choices. Step 4 starts with
a pyenv-win present, so it checks the checkbox's hiding too. Uninstalls run `msiexec /x
/passive`, which is what Settings, Apps runs. Look at the screenshots for anything the
checks don't name.

Run it before tagging a release whenever setup, migrate or the installer changed since
the last run. No other check on x64 covers the dialogs, or the allow-scripts box against
client Windows' default (CI's x64 runners are Windows Server).

By hand: `pwsh -NoProfile -File installer/sandbox/new-wsb.ps1`, then open the printed
`dist\sandbox.wsb`. Inside the Sandbox, the MSI is in `C:\rpyenv-dist`. Record each
result (as seen, plus anything unexpected).

Things that are the Sandbox, not the MSI:

- Each install waits about 4 minutes: Windows Installer's check of the unsigned package
  times out twice, after 120 s each ("SOFTWARE RESTRICTION POLICY" in a verbose log). CI
  installs take seconds.
- The Sandbox account is an administrator without UAC: no UAC prompt appears.
- Windows PowerShell's execution policy is the client default, `Restricted`: `.ps1` files
  don't run. Step 1 checks that rpyenv copes with this.
- Process `PATH` puts the machine PATH before the user's, so check the user PATH with
  `[Environment]::GetEnvironmentVariable('Path','User')`, not `$env:Path`.

1. **Just for me.** Double-click the x64 MSI.
   - Expect: the Welcome page, then Options, then a last page that says to open a new
     terminal. Options shows:
     - "Just for me" selected
     - "Enable live rehash" unchecked
     - "Let Windows PowerShell run local scripts" checked
     - no pyenv-win checkbox
   - In a new Windows PowerShell:
     - no error at startup, and no "No version installed" line
     - `pyenv --version` works
     - the user PATH starts with `…\.pyenv\pyenv-win\shims` and contains
       `…\AppData\Local\Programs\rpyenv\bin\`
     - `Get-ExecutionPolicy -Scope CurrentUser` is RemoteSigned and `$PROFILE` has the
       `pyenv init - pwsh` line (the checkbox: `pyenv setup --allow-scripts`)
   - `…\rpyenv\share\conda\windows.tsv` exists. With a scratch `PYENV_ROOT` (no
     `.versions_cache.xml`), `pyenv install --list` shows `miniforge3-…` and `miniconda3-…`
     lines.
2. **Uninstall from Settings, Apps.**
   - Expect: it completes. After it:
     - `…\Programs\rpyenv` is gone
     - both user PATH entries are gone
     - the CurrentUser execution policy is Undefined again
     - the profile line is gone
     - `…\.pyenv\pyenv-win\versions` stays
     - a new Windows PowerShell starts without an error
3. **Migrate.** Run `powershell -NoProfile -ExecutionPolicy Bypass -File
   C:\rpyenv-sandbox\fake-pyenv-win.ps1`, then install again, just for me.
   - Expect: the Options page shows "Move pyenv-win aside", checked. Uncheck "Let Windows
     PowerShell run local scripts" (the runner does). The execution policy then stays
     Undefined, and Windows PowerShell's profile gets no line. After install,
     `…\pyenv-win\bin\pyenv.ps1` is gone, and the file is in
     `…\pyenv-win\.rpyenv-migrate\bin\`. Uninstall restores it and removes
     `.rpyenv-migrate`.
4. **For all users.** Install with a log, choosing "For all users":
   `Start-Process msiexec -ArgumentList '/i C:\rpyenv-dist\rpyenv-<version>-x64.msi /l*v C:\s4.log' -Wait`.
   - Record:
     - what the Options page shows (no pyenv-win or allow-scripts checkbox is expected)
     - `Select-String 'MSI \(s\).*Command Line:' C:\s4.log`. The server's command line
       must carry `ALLUSERS=1` (Sandbox S5: the dialog path is the one CI can't run).
   - Expect:
     - files in `C:\Program Files\rpyenv\bin`
     - that folder on the **machine** PATH
       (`[Environment]::GetEnvironmentVariable('Path','Machine')`) and not on the user PATH
     - `HKLM:\Software\rpyenv` and the Active Setup key
       `HKLM:\SOFTWARE\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}`
       exist
     - `HKCU:\Software\rpyenv` and `…\pyenv-win\.rpyenv-setup` don't exist (yet)
5. **Active Setup.** Not checkable in Windows Sandbox (2026-10-13):
   - `runonce.exe /AlternateShellStartup` exits 0 without running the StubPath.
   - `run-checks.ps1 -Steps active-setup` (install as SYSTEM, then the first sign-in) shows
     the sign-in copying the version to HKCU, but starting no StubPath at all. The
     process-creation audit shows no process from Explorer for it, nor for any other
     component.

   What does work there: a user's first `pyenv` command runs their setup (rpyenv's
   first-run fallback).

   On a real Windows 11 VM (Enterprise evaluation, build 26300, 2026-10-13) it passes.
   After a silent all-users install, the first sign-ins of an administrator and of a new
   standard user each:
   - ran `"C:\Program Files\rpyenv\bin\pyenv.exe" setup` from Explorer (process-creation
     audit),
   - wrote `.rpyenv-setup`,
   - put the user's shims first on their PATH, keeping `%USERPROFILE%\...\WindowsApps`
     unexpanded.

   The by-hand commands, for a real machine or VM, after step 4:
   1. Run
      `reg delete "HKCU\Software\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}" /f`
      (the key may already be absent).
   2. Run `runonce.exe /AlternateShellStartup`.
   - Record: does a console window flash? Does the user PATH now start with the shims,
     and does `…\pyenv-win\.rpyenv-setup` exist?
6. **Silent all users as a standard user.**
   1. Uninstall.
   2. In an administrator prompt, run `net user rpyenvstd Sandbox-Std-2026 /add` and
      `copy C:\rpyenv-dist\rpyenv-<version>-x64.msi C:\Users\Public\`.
   3. Run `runas /user:rpyenvstd cmd` (password as above).
   4. In that window, run
      `msiexec /i C:\Users\Public\rpyenv-<version>-x64.msi /qn ALLUSERS=1 MSIINSTALLPERUSER="" /l*v %TEMP%\all.log`,
      then `echo %ERRORLEVEL%`.
   - Expect: no prompt, a non-zero exit code, and nothing in `C:\Program Files\rpyenv`.
     Record the exit code.
