# rpyenv MSI: interactive checks in Windows Sandbox

CI already installs, upgrades and uninstalls the MSI silently, both ways, and as a
non-administrator (`installer/test_msi.py`). These checks cover what CI can't see: the
dialogs, the UAC prompt, Active Setup and a silent install from a non-elevated prompt.

Start: `pwsh -NoProfile -File installer/sandbox/new-wsb.ps1`, then open the printed
`dist\sandbox.wsb`. Inside the Sandbox, the MSI is in `C:\rpyenv-dist`. Record each
result (as seen, plus anything unexpected).

Things that are the Sandbox, not the MSI:

- Each install waits about 4 minutes: Windows Installer's check of the unsigned package
  times out twice, after 120 s each ("SOFTWARE RESTRICTION POLICY" in a verbose log). CI
  installs take seconds.
- The Sandbox account is an administrator without UAC: no UAC prompt appears.
- Windows PowerShell's execution policy is the client default, `Restricted`: `.ps1` files
  don't run, which is what step 1 checks rpyenv copes with.
- Process `PATH` puts the machine PATH before the user's, so check the user PATH with
  `[Environment]::GetEnvironmentVariable('Path','User')`, not `$env:Path`.

1. **Just for me.** Double-click the x64 MSI.
   - Expect: the Welcome page, then Options with "Just for me" selected, "Enable live
     rehash" unchecked and no pyenv-win checkbox. The last page says to open a new
     terminal.
   - In a new Windows PowerShell: no error at startup, and no "No version installed"
     line; `pyenv --version` works; the user PATH starts with `…\.pyenv\pyenv-win\shims`
     and contains `…\AppData\Local\Programs\rpyenv\bin\`; `Test-Path $PROFILE` is False
     (setup leaves the profile alone while scripts can't run).
2. **Uninstall from Settings, Apps.**
   - Expect: it completes. `…\Programs\rpyenv` is gone, both user PATH entries are gone,
     and `…\.pyenv\pyenv-win\versions` stays. A new Windows PowerShell starts without an
     error.
3. **Migrate.** Run `powershell -NoProfile -ExecutionPolicy Bypass -File
   C:\rpyenv-sandbox\fake-pyenv-win.ps1`, then install again, just for me.
   - Expect: the Options page shows "Move pyenv-win aside", checked. After install,
     `…\pyenv-win\bin\pyenv.ps1` is gone and in `…\pyenv-win\.rpyenv-migrate\bin\`.
     Uninstall puts it back and removes `.rpyenv-migrate`.
4. **For all users.** Install, choosing "For all users".
   - Record: what the Options page shows (no pyenv-win checkbox is expected).
   - Expect: files in `C:\Program Files\rpyenv\bin`; that folder on the **machine** PATH
     (`[Environment]::GetEnvironmentVariable('Path','Machine')`) and not on the user
     PATH; `HKLM:\Software\rpyenv` and the Active Setup key
     `HKLM:\SOFTWARE\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}`
     exist; `HKCU:\Software\rpyenv` and `…\pyenv-win\.rpyenv-setup` don't (yet).
5. **Active Setup.** After step 4, run:
   `reg delete "HKCU\Software\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}" /f`
   (it may already be absent), then `runonce.exe /AlternateShellStartup`.
   - Record: does a console window flash? Does the user PATH now start with the shims,
     and does `…\pyenv-win\.rpyenv-setup` exist?
6. **Silent all users as a standard user.** Uninstall, then in an administrator prompt:
   `net user rpyenvstd Sandbox-Std-2026 /add` and
   `copy C:\rpyenv-dist\rpyenv-<version>-x64.msi C:\Users\Public\`. Then
   `runas /user:rpyenvstd cmd` (password as above), and in that window:
   `msiexec /i C:\Users\Public\rpyenv-<version>-x64.msi /qn ALLUSERS=1 MSIINSTALLPERUSER="" /l*v %TEMP%\all.log`,
   then `echo %ERRORLEVEL%`.
   - Expect: no prompt, a non-zero exit code, and nothing in `C:\Program Files\rpyenv`.
     Record the exit code.
