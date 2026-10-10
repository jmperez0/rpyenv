# Checks an rpyenv MSI's tables (M6b design §4) without installing it: safe on any machine.
param([Parameter(Mandatory)] [string] $Msi)
$ErrorActionPreference = 'Stop'

$installer = New-Object -ComObject WindowsInstaller.Installer
function Call($Target, [string] $Member, [System.Reflection.BindingFlags] $Kind, $Arguments) {
    $Target.GetType().InvokeMember($Member, $Kind, $null, $Target, $Arguments)
}
$database = Call $installer 'OpenDatabase' InvokeMethod @((Resolve-Path $Msi).Path, 0)
function Rows([string] $Sql) {
    $view = Call $database 'OpenView' InvokeMethod @($Sql)
    Call $view 'Execute' InvokeMethod $null | Out-Null
    $rows = @()
    while ($record = Call $view 'Fetch' InvokeMethod $null) {
        $count = Call $record 'FieldCount' GetProperty $null
        # [int]: COM rejects PowerShell's wrapped pipeline value (DISP_E_TYPEMISMATCH).
        $rows += , @(1..$count | ForEach-Object { Call $record 'StringData' GetProperty @([int]$_) })
    }
    Call $view 'Close' InvokeMethod $null | Out-Null
    , $rows
}

$failures = [System.Collections.Generic.List[string]]::new()
function Expect([bool] $Ok, [string] $What) { if (-not $Ok) { $failures.Add($What) } }

$props = @{}
foreach ($r in (Rows 'SELECT `Property`, `Value` FROM `Property`')) { $props[$r[0]] = $r[1] }
Expect ($props['ALLUSERS'] -eq '2') 'ALLUSERS=2 (one package, both scopes)'
Expect ($props['MSIINSTALLPERUSER'] -eq '1') 'MSIINSTALLPERUSER=1 (just for me by default)'
Expect ($props['ARPNOMODIFY'] -eq '1') 'ARPNOMODIFY=1'
# Windows Installer reserves MIGRATE (MigrateFeatureStates): MIGRATE=1 fails with error 2601.
Expect (-not (("$($props['SecureCustomProperties'])" -split ';') -contains 'MIGRATE')) 'no MIGRATE option (reserved)'

$files = (Rows 'SELECT `FileName` FROM `File`') | ForEach-Object { ($_[0] -split '\|')[-1] }
foreach ($f in 'pyenv.exe', 'pyenv-shim.exe', 'pyenv-shimw.exe', 'pyenv.pwsh', 'pyenv.bash', 'pyenv.zsh', 'pyenv.fish') {
    Expect ($files -contains $f) "file $f"
}

$environment = Rows 'SELECT `Name`, `Value` FROM `Environment`'
$paths = @($environment | Where-Object { $_[0] -match 'PATH$' })
Expect (@($paths | Where-Object { $_[0] -match '\*' }).Count -eq 1) 'bin on the machine PATH'
Expect (@($paths | Where-Object { $_[0] -notmatch '\*' }).Count -eq 1) 'bin on the user PATH'
Expect (@($environment | Where-Object { $_[0] -match 'RPYENV_LIVE_REHASH$' }).Count -eq 2) 'live rehash, both scopes'

$registry = Rows 'SELECT `Key`, `Name`, `Value` FROM `Registry`'
Expect (@($registry | Where-Object {
            $_[0] -eq 'SOFTWARE\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}' -and
            $_[1] -eq 'StubPath' -and $_[2] -like '*pyenv.exe" setup' }).Count -eq 1) 'Active Setup StubPath'

$sequence = @{}
foreach ($r in (Rows 'SELECT `Action`, `Condition` FROM `InstallExecuteSequence`')) { $sequence[$r[0]] = $r[1] }
foreach ($a in 'RpyenvMigrate', 'RpyenvSetup', 'RpyenvRestore', 'RpyenvUndo', 'RefuseMachineOverUser', 'RefuseUserOverMachine') {
    Expect ($sequence.ContainsKey($a)) "action $a scheduled"
}
Expect ("$($sequence['RpyenvUndo'])" -like '*NOT UPGRADINGPRODUCTCODE*') 'undo skipped during upgrades'

$targets = (Rows 'SELECT `Action`, `Target` FROM `CustomAction`') | ForEach-Object { "$($_[1])" }
foreach ($c in '" migrate', '" setup', '" migrate --restore', '" setup --undo') {
    Expect (@($targets | Where-Object { $_ -like "*pyenv.exe$c" }).Count -ge 1) "command pyenv.exe$c"
}

$dialogs = (Rows 'SELECT `Dialog` FROM `Dialog`') | ForEach-Object { $_[0] }
Expect ($dialogs -contains 'OptionsDlg') 'the Options page'

if ($failures.Count) {
    $failures | ForEach-Object { Write-Host "missing: $_" }
    exit 1
}
Write-Host "MSI tables: ok ($Msi)"
