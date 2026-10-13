# Inside Windows Sandbox only (run by run-checks.ps1 through `wsb exec`): one CHECKLIST.md
# step, driven through the installer's own pages: UI Automation finds the controls, and
# Win32 messages click and read them, since UI Automation offers no patterns on MSI's. Writes report.txt
# (PASS/FAIL lines), a screenshot of each page and the verbose MSI logs to $Out\<step>.
# Windows PowerShell 5.1: no ?? or ternaries.
param(
    [Parameter(Mandatory)][ValidateSet('active-setup-install', 'active-setup', 'just-for-me', 'migrate', 'all-users', 'standard-user')]
    [string] $Step,
    [Parameter(Mandatory)] [string] $Msi,
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
# active-setup-install runs as SYSTEM, so the Sandbox is known by its account's profile.
if (-not (Test-Path 'C:\Users\WDAGUtilityAccount')) { throw "checks.ps1 runs only inside Windows Sandbox" }

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SbxWin {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    public struct RECT { public int Left, Top, Right, Bottom; }
}
'@
[SbxWin]::SetProcessDPIAware() | Out-Null
$UIA = [System.Windows.Automation.AutomationElement]
$Scope = [System.Windows.Automation.TreeScope]

$dir = Join-Path $Out $Step
New-Item -ItemType Directory -Force $dir | Out-Null
$report = Join-Path $dir 'report.txt'
Set-Content -Path $report -Value "# $Step  $(Get-Date -Format o)  $Msi" -Encoding utf8
$script:shot = 0

function Note([string] $s) { Add-Content -Path $report -Value $s -Encoding utf8 }
function Check([string] $name, $expected, $actual) {
    $ok = "$expected" -eq "$actual"
    Note ("{0}  {1}: expected {2}, got {3}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $name, $expected, $actual)
}
function Like([string] $name, [string] $pattern, [string] $actual) {
    $ok = $actual -match $pattern
    Note ("{0}  {1}: expected /{2}/, got {3}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $name, $pattern, $actual)
}

# --- state -------------------------------------------------------------------------------
$AsKey = 'SOFTWARE\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}'
$Root = Join-Path $HOME '.pyenv\pyenv-win'
$UserBin = Join-Path $env:LOCALAPPDATA 'Programs\rpyenv\bin'
$MachineBin = Join-Path $env:ProgramFiles 'rpyenv\bin'
function UserPath { [Environment]::GetEnvironmentVariable('Path', 'User') }
function MachinePath { [Environment]::GetEnvironmentVariable('Path', 'Machine') }
function Entries([string] $p) { if ($p) { @($p -split ';' | Where-Object { $_ }) } else { @() } }
function WinPsProfile { Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'WindowsPowerShell\Microsoft.PowerShell_profile.ps1' }

# A new Windows PowerShell, as a new terminal would start it: the profile loads, PATH from
# the registry (this process's own PATH predates the install).
function NewWindowsPowerShell([string] $command) {
    $ErrorActionPreference = 'Continue'  # its stderr is output to check, not an error
    $saved = $env:Path
    $env:Path = (MachinePath) + ';' + (UserPath)
    try { $o = & powershell.exe -NonInteractive -Command $command 2>&1 | ForEach-Object { "$_" } }
    finally { $env:Path = $saved }
    @($o)
}

function Clean {
    # From a known state: nothing installed, no pyenv-win, no profiles, no markers. An
    # installer window left by an earlier run holds Windows Installer's lock: closed first.
    Get-Process msiexec -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | ForEach-Object {
        Note "clean: closed an installer window ($($_.MainWindowTitle))"
        Stop-Process -Id $_.Id -Force
    }
    Start-Sleep 2
    # Any rpyenv build, found by its upgrade code (each build has its own product code).
    $installer = New-Object -ComObject WindowsInstaller.Installer
    foreach ($code in @($installer.RelatedProducts('{A3154079-CE28-4416-B9A5-9FDB628BA522}'))) {
        if (-not $code) { continue }
        $p = Start-Process msiexec -ArgumentList "/x $code /qn /l*v `"$dir\clean-uninstall.log`"" -Wait -PassThru
        Note "clean: uninstalled $code (exit $($p.ExitCode))"
    }
    if (Test-Path (Join-Path $HOME '.pyenv')) { Remove-Item -Recurse -Force (Join-Path $HOME '.pyenv') }
    $kept = Entries (UserPath) | Where-Object { $_ -notmatch '\\\.pyenv\\|\\rpyenv\\' }
    [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')
    foreach ($k in "HKCU:\Software\rpyenv", "HKCU:\$AsKey") { if (Test-Path $k) { Remove-Item -Recurse -Force $k } }
    $docs = [Environment]::GetFolderPath('MyDocuments')
    foreach ($f in 'WindowsPowerShell\Microsoft.PowerShell_profile.ps1', 'PowerShell\Microsoft.PowerShell_profile.ps1') {
        $p = Join-Path $docs $f
        if (Test-Path $p) { Remove-Item -Force $p }
    }
}

function Get-LocalMsi {
    # Installs run from a local copy: the mapped folder keeps the host's file open (error
    # 1631 on the host's next build), and it may be what makes the trust check slow.
    $local = Join-Path 'C:\rpx-local' (Split-Path $Msi -Leaf)
    if (-not (Test-Path $local)) {
        New-Item -ItemType Directory -Force 'C:\rpx-local' | Out-Null
        Copy-Item $Msi $local
    }
    $local
}

# --- UI Automation ---------------------------------------------------------------------------
function Strip([string] $s) { ($s -replace '&', '').Trim() }

function Elements($window) {
    $window.FindAll($Scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
}

$BM_GETCHECK = 0x00F0
$BM_CLICK = 0x00F5
function Hwnd($el) { [IntPtr] $el.Current.NativeWindowHandle }
function Checked($el) { [int] [SbxWin]::SendMessage((Hwnd $el), $BM_GETCHECK, [IntPtr]::Zero, [IntPtr]::Zero) }

function State($el) {
    $h = Hwnd $el
    $s = ''
    if ($el.Current.ClassName -eq 'Button') { $s = "check=$(Checked $el)" }
    if (-not [SbxWin]::IsWindowVisible($h)) { $s += ' hidden' }
    if (-not [SbxWin]::IsWindowEnabled($h)) { $s += ' disabled' }
    $s
}

function Dump($window, [string] $label) {
    Note "page ${label}:"
    foreach ($el in Elements $window) {
        $c = $el.Current
        if (-not $c.Name) { continue }
        Note ("    {0,-8} {1}  {2}" -f $c.ClassName, (Strip $c.Name), (State $el))
    }
}

function Snap($window, [string] $label) {
    $h = [IntPtr] $window.Current.NativeWindowHandle
    $r = New-Object SbxWin+RECT
    [SbxWin]::GetWindowRect($h, [ref] $r) | Out-Null
    $bmp = New-Object System.Drawing.Bitmap ([Math]::Max(1, $r.Right - $r.Left)), ([Math]::Max(1, $r.Bottom - $r.Top))
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [SbxWin]::PrintWindow($h, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc)
    $file = Join-Path $dir ('{0:D2}-{1}.png' -f $script:shot, $label)
    $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    $script:shot++
}

# The installer's top-level window whose text matches $page (each MSI dialog is a new window).
function WaitPage([int] $processId, [string] $page, [int] $seconds) {
    $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $processId)
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        foreach ($w in $UIA::RootElement.FindAll($Scope::Children, $cond)) {
            try {
                foreach ($el in Elements $w) {
                    if ((Strip $el.Current.Name) -match $page) { return $w }
                }
            } catch { }  # the dialog went away while being read
        }
        Start-Sleep -Milliseconds 500
    }
    throw "no installer page matching /$page/ within $seconds s"
}

# Buttons, radio buttons and checkboxes are all of class Button.
function Find($window, [string] $name) {
    foreach ($el in Elements $window) {
        $c = $el.Current
        if ($c.ClassName -eq 'Button' -and (Strip $c.Name) -match $name) { return $el }
    }
    $null
}

function Press($el) { [SbxWin]::PostMessage((Hwnd $el), $BM_CLICK, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }

function Click($window, [string] $button) {
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        $b = Find $window "^$button$"
        if ($b -and [SbxWin]::IsWindowEnabled((Hwnd $b))) { Press $b; return }
        Start-Sleep -Milliseconds 300
    }
    throw "no enabled '$button' button"
}

# A hidden control may be missing from the tree altogether.
function Visible($window, [string] $name) {
    $el = Find $window $name
    [bool] ($el -and [SbxWin]::IsWindowVisible((Hwnd $el)))
}

function Toggled($window, [string] $name) {
    $el = Find $window $name
    if (-not $el) { return 'absent' }
    if ((Checked $el) -eq 1) { 'On' } else { 'Off' }
}

function Selected($window, [string] $name) {
    $el = Find $window $name
    if (-not $el) { return 'absent' }
    (Checked $el) -eq 1
}

# Installs through the pages. $options gets the Options page window and records or changes it.
function InstallByDialog([string] $log, [scriptblock] $options) {
    $local = Get-LocalMsi
    $t0 = Get-Date
    $p = Start-Process msiexec -ArgumentList "/i `"$local`" /l*v `"$log`"" -PassThru
    $w = WaitPage $p.Id '^Welcome to the' 600
    Note ("welcome page after {0:N0} s" -f ((Get-Date) - $t0).TotalSeconds)
    Snap $w 'welcome'
    Click $w 'Next'
    $w = WaitPage $p.Id '^Options$' 60
    Snap $w 'options'
    Dump $w 'options'
    & $options $w
    Click $w 'Next'
    $w = WaitPage $p.Id '^Ready to install' 60
    Snap $w 'ready'
    $t1 = Get-Date
    Click $w 'Install'
    $w = WaitPage $p.Id '^Completed the' 900
    Note ("install took {0:N0} s after Install" -f ((Get-Date) - $t1).TotalSeconds)
    Snap $w 'completed'
    Dump $w 'completed'
    $last = (Elements $w | ForEach-Object { Strip $_.Current.Name }) -join ' '
    Like 'last page names a new terminal' 'Open a new terminal' $last
    Like 'last page names Set-ExecutionPolicy' 'Set-ExecutionPolicy -Scope CurrentUser RemoteSigned' $last
    Click $w 'Finish'
    $p.WaitForExit(60000) | Out-Null
    Check 'install exit code' 0 $p.ExitCode
}

function Uninstall([string] $log) {
    # Settings, Apps runs the same msiexec /x; /passive shows progress without questions.
    $p = Start-Process msiexec -ArgumentList "/x `"$(Get-LocalMsi)`" /passive /l*v `"$log`"" -Wait -PassThru
    Check 'uninstall exit code' 0 $p.ExitCode
}

function ServerCommandLine([string] $log) {
    $m = Select-String -Path $log -Pattern 'MSI \(s\).*Command Line: (.*)' | Select-Object -First 1
    if ($m) { $m.Matches[0].Groups[1].Value } else { '<none>' }
}

function CommonUninstalled {
    Check 'per-user bin gone' False (Test-Path (Split-Path $UserBin))
    Check 'Program Files\rpyenv gone' False (Test-Path (Split-Path $MachineBin))
    Check 'no rpyenv or shims on the user PATH' 0 @(Entries (UserPath) | Where-Object { $_ -match 'rpyenv|pyenv-win\\shims' }).Count
    Check 'no rpyenv on the machine PATH' 0 @(Entries (MachinePath) | Where-Object { $_ -match 'rpyenv' }).Count
    Check 'HKCU marker gone' False (Test-Path HKCU:\Software\rpyenv)
    Check 'HKLM marker gone' False (Test-Path HKLM:\Software\rpyenv)
    Check 'Active Setup key gone' False (Test-Path "HKLM:\$AsKey")
    $o = NewWindowsPowerShell "'started'"
    Check 'new Windows PowerShell starts cleanly' 'started' ($o -join ' | ')
}

# --- steps -------------------------------------------------------------------------------
function JustForMe {
    # CHECKLIST 1 and 2, with the default choices.
    InstallByDialog "$dir\install.log" {
        param($w)
        Check 'Options: Just for me selected' True (Selected $w '^Just for me')
        Check 'Options: live rehash unchecked' Off (Toggled $w '^Enable live rehash')
        Check 'Options: no pyenv-win checkbox' False (Visible $w '^Move pyenv-win aside')
    }
    $cl = ServerCommandLine "$dir\install.log"
    Note "server command line: $cl"
    Check 'server: no ALLUSERS=1' False ($cl -match '(^| )ALLUSERS=1( |$)')
    # MSIINSTALLPERUSER=1 is the package's default, so it isn't passed (Sandbox 2026-10-13).
    Like 'server: per-user INSTALLFOLDER' '(^| )INSTALLFOLDER=C:\\Users\\' $cl
    Check 'files in the per-user folder' True (Test-Path "$UserBin\pyenv.exe")
    Check 'nothing in Program Files' False (Test-Path (Split-Path $MachineBin))
    $u = @(Entries (UserPath))
    Note "user PATH: $($u -join ';')"
    Like 'user PATH starts with the shims' '\\\.pyenv\\pyenv-win\\shims\\?$' $u[0]
    Check 'user PATH has the per-user bin' 1 @($u | Where-Object { $_.TrimEnd('\') -eq $UserBin }).Count
    Check 'machine PATH has no rpyenv' 0 @(Entries (MachinePath) | Where-Object { $_ -match 'rpyenv' }).Count
    Check 'HKCU marker' True (Test-Path HKCU:\Software\rpyenv)
    Check 'HKLM marker' False (Test-Path HKLM:\Software\rpyenv)
    Check 'Active Setup key' False (Test-Path "HKLM:\$AsKey")
    Check 'setup marker' True (Test-Path "$Root\.rpyenv-setup")
    Check 'Windows PowerShell profile left alone (Restricted)' False (Test-Path (WinPsProfile))
    $o = NewWindowsPowerShell 'pyenv --version'
    Like 'new Windows PowerShell: only pyenv --version' '^pyenv \S+ \(rpyenv \S+\)$' ($o -join ' | ')

    # CHECKLIST 2.
    New-Item -ItemType Directory -Force "$Root\versions\3.0.0-kept" | Out-Null
    Uninstall "$dir\uninstall.log"
    CommonUninstalled
    Check 'versions kept' True (Test-Path "$Root\versions\3.0.0-kept")
}

function FakePyenvWin {
    $o = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'fake-pyenv-win.ps1') 2>&1
    Note "fake pyenv-win: $o"
}

function Migrate {
    # CHECKLIST 3.
    FakePyenvWin
    InstallByDialog "$dir\install.log" {
        param($w)
        Check 'Options: pyenv-win checkbox shown' True (Visible $w '^Move pyenv-win aside')
        Check 'Options: pyenv-win checkbox checked' On (Toggled $w '^Move pyenv-win aside')
    }
    Check 'pyenv-win launcher moved out of bin' False (Test-Path "$Root\bin\pyenv.ps1")
    Check 'pyenv-win launcher in .rpyenv-migrate' True (Test-Path "$Root\.rpyenv-migrate\bin\pyenv.ps1")
    Uninstall "$dir\uninstall.log"
    Check 'pyenv-win launcher back in bin' True (Test-Path "$Root\bin\pyenv.ps1")
    Check '.rpyenv-migrate removed' False (Test-Path "$Root\.rpyenv-migrate")
    CommonUninstalled
}

function AllUsers {
    # CHECKLIST 4, with a pyenv-win present so the checkbox's hiding is seen.
    FakePyenvWin
    InstallByDialog "$dir\install.log" {
        param($w)
        Check 'Options: pyenv-win checkbox shown for Just for me' True (Visible $w '^Move pyenv-win aside')
        Press (Find $w '^For all users')
        Start-Sleep -Milliseconds 700
        Snap $w 'options-all-users'
        Dump $w 'options after For all users'
        Check 'Options: For all users selected' True (Selected $w '^For all users')
        Check 'Options: pyenv-win checkbox hidden for all users' False (Visible $w '^Move pyenv-win aside')
    }
    $cl = ServerCommandLine "$dir\install.log"
    Note "server command line: $cl"
    Like 'server: ALLUSERS=1' '(^| )ALLUSERS=1( |$)' $cl
    Check 'files in Program Files' True (Test-Path "$MachineBin\pyenv.exe")
    Check 'nothing in the per-user folder' False (Test-Path (Split-Path $UserBin))
    Check 'machine PATH has Program Files\rpyenv\bin' 1 @(Entries (MachinePath) | Where-Object { $_.TrimEnd('\') -eq $MachineBin }).Count
    Check 'user PATH has no rpyenv' 0 @(Entries (UserPath) | Where-Object { $_ -match 'rpyenv' }).Count
    Check 'HKLM marker' True (Test-Path HKLM:\Software\rpyenv)
    Check 'Active Setup key' True (Test-Path "HKLM:\$AsKey")
    Check 'HKCU marker' False (Test-Path HKCU:\Software\rpyenv)
    Check 'setup marker (Active Setup not yet run)' False (Test-Path "$Root\.rpyenv-setup")
    Check 'pyenv-win left in place' True (Test-Path "$Root\bin\pyenv.ps1")
    Uninstall "$dir\uninstall.log"
    CommonUninstalled
}

function ActiveSetupInstall {
    # As SYSTEM, before anyone has signed in: silent, for all users, as deployment tools do.
    $p = Start-Process msiexec -ArgumentList "/i `"$(Get-LocalMsi)`" /qn ALLUSERS=1 MSIINSTALLPERUSER=`"`" /l*v `"$dir\install.log`"" -Wait -PassThru
    Check 'install exit code' 0 $p.ExitCode
    Check 'files in Program Files' True (Test-Path "$MachineBin\pyenv.exe")
    Check 'Active Setup key' True (Test-Path "HKLM:\$AsKey")
    Check 'machine PATH has Program Files\rpyenv\bin' 1 @(Entries (MachinePath) | Where-Object { $_.TrimEnd('\') -eq $MachineBin }).Count
}

function ActiveSetup {
    # CHECKLIST 5 at the user's first sign-in after ActiveSetupInstall: Windows has run the
    # StubPath, pyenv setup, for them.
    $machine = (Get-ItemProperty "HKLM:\$AsKey").Version
    $user = (Get-ItemProperty "HKCU:\$AsKey" -ErrorAction SilentlyContinue).Version
    Check 'Active Setup ran at sign-in (HKCU copy of the version)' $machine $user
    Check 'setup marker' True (Test-Path "$Root\.rpyenv-setup")
    $u = @(Entries (UserPath))
    Note "user PATH: $($u -join ';')"
    Like 'user PATH starts with the shims' '\\\.pyenv\\pyenv-win\\shims\\?$' $u[0]
    Check 'user PATH has no rpyenv bin (it is on the machine PATH)' 0 @($u | Where-Object { $_ -match 'rpyenv' }).Count
    $o = NewWindowsPowerShell 'pyenv --version'
    Like 'new Windows PowerShell: only pyenv --version' '^pyenv \S+ \(rpyenv \S+\)$' ($o -join ' | ')
    Uninstall "$dir\uninstall.log"
    CommonUninstalled
}

function StandardUser {
    # CHECKLIST 6: a real standard user, silent, asking for all users.
    $name = 'rpyenvstd'
    $pw = 'Rp-' + [Guid]::NewGuid().ToString('N').Substring(0, 12) + '-a1'
    if (Get-LocalUser -Name $name -ErrorAction SilentlyContinue) { Set-LocalUser -Name $name -Password (ConvertTo-SecureString $pw -AsPlainText -Force) }
    else { New-LocalUser -Name $name -Password (ConvertTo-SecureString $pw -AsPlainText -Force) | Out-Null }
    Check 'rpyenvstd is not an administrator' 0 @(Get-LocalGroupMember Administrators | Where-Object { $_.Name -match "\\$name$" }).Count
    $pub = Join-Path $env:PUBLIC (Split-Path $Msi -Leaf)
    Copy-Item (Get-LocalMsi) $pub -Force
    $log = Join-Path $env:PUBLIC 'all.log'
    $cred = New-Object System.Management.Automation.PSCredential($name, (ConvertTo-SecureString $pw -AsPlainText -Force))
    $t0 = Get-Date
    $p = Start-Process msiexec -Credential $cred -LoadUserProfile -WorkingDirectory $env:PUBLIC -Wait -PassThru `
        -ArgumentList "/i `"$pub`" /qn ALLUSERS=1 MSIINSTALLPERUSER=`"`" /l*v `"$log`""
    Note ("standard user's install: exit {0} after {1:N0} s" -f $p.ExitCode, ((Get-Date) - $t0).TotalSeconds)
    Check 'standard user: install fails' True ($p.ExitCode -ne 0)
    Check 'standard user: nothing in Program Files' False (Test-Path (Split-Path $MachineBin))
    if (Test-Path $log) { Copy-Item $log "$dir\all.log" }
    $err = Select-String -Path $log -Pattern 'Error \d{4}\.|Return value 3' -ErrorAction SilentlyContinue | Select-Object -First 3
    foreach ($e in $err) { Note "log: $($e.Line)" }
}

try {
    # The Active Setup steps start from a fresh Sandbox and must keep its state.
    if ($Step -notlike 'active-setup*') { Clean }
    switch ($Step) {
        'active-setup-install' { ActiveSetupInstall }
        'active-setup' { ActiveSetup }
        'just-for-me' { JustForMe }
        'migrate' { Migrate }
        'all-users' { AllUsers }
        'standard-user' { StandardUser }
    }
} catch {
    Note "FAIL  $Step stopped: $($_.Exception.Message) at line $($_.InvocationInfo.ScriptLineNumber)"
    # Whatever the installer shows now, for the record.
    foreach ($w in $UIA::RootElement.FindAll($Scope::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
        try { if ($w.Current.Name -match 'rpyenv') { Snap $w 'stopped'; Dump $w 'stopped' } } catch { }
    }
}
Note "done $(Get-Date -Format o)"
