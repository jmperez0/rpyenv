# rpyenv MSI: interactive checks in Windows Sandbox

CI already installs, upgrades and uninstalls the MSI silently, both ways, and as a
non-administrator (`installer/test_msi.py`). These checks cover what CI can't see: the
dialogs, the UAC prompt, Active Setup and a silent install from a non-elevated prompt.

Start: `pwsh -NoProfile -File installer/sandbox/new-wsb.ps1`, then open the printed
`dist\sandbox.wsb`. Inside the Sandbox, the MSI is in `C:\rpyenv-dist`. Record each
result (as seen, plus anything unexpected).

1. **Just for me.** Double-click the x64 MSI.
   - Expect: the Welcome page, then Options with "Just for me" selected, "Enable live
     rehash" unchecked and no pyenv-win checkbox. Install raises no UAC prompt. The last
     page says to open a new terminal.
   - In a new PowerShell: `pyenv --version` works; `$env:Path` starts with
     `…\.pyenv\pyenv-win\shims` and contains `…\AppData\Local\Programs\rpyenv\bin\`;
     `Get-Content $PROFILE` contains the `iex ((pyenv init - pwsh) …` line.
2. **Uninstall from Settings, Apps.**
   - Expect: it completes. `…\Programs\rpyenv` is gone, both PATH entries are gone, the
     profile line is gone, and `…\.pyenv\pyenv-win\versions` stays.
3. **Migrate.** In PowerShell run `C:\rpyenv-sandbox\fake-pyenv-win.ps1`, then install
   again, just for me.
   - Expect: the Options page shows "Move pyenv-win aside", checked. After install,
     `…\pyenv-win\bin\pyenv.ps1` is gone and in `…\pyenv-win\.rpyenv-migrate\bin\`.
     Uninstall puts it back.
4. **For all users.** Install, choosing "For all users".
   - Expect: a UAC prompt, or note that the Sandbox runs without UAC. Files land in
     `C:\Program Files\rpyenv\bin`, and that folder is on the machine PATH.
5. **Active Setup.** After step 4, run:
   `reg delete "HKCU\Software\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}" /f`
   (it may already be absent), then `runonce.exe /AlternateShellStartup`.
   - Record: does a console window flash? Does `$env:Path` (new terminal) start with the
     shims, and does `…\pyenv-win\.rpyenv-setup` exist?
6. **Silent all users from a non-elevated prompt.** Uninstall, then from a normal
   (non-admin) Command Prompt run:
   `msiexec /i C:\rpyenv-dist\rpyenv-<version>-x64.msi /qn ALLUSERS=1 MSIINSTALLPERUSER="" /l*v %TEMP%\all.log`,
   then `echo %ERRORLEVEL%`.
   - Record: the exit code, and whether anything was installed (expected: fails without
     a prompt, nothing installed; if the Sandbox account is elevated, record that
     instead).
