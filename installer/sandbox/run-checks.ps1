# Runs CHECKLIST.md's automatable steps in Windows Sandbox through the wsb CLI (Windows 11
# 24H2+): starts a Sandbox (or uses -Id), shares the MSI's folder and this folder read-only
# and a results folder writable, runs checks.ps1 once per step in the signed-in session, and
# prints each step's PASS/FAIL lines. The host's own setup is never touched: everything that
# installs runs inside the Sandbox.
#   pwsh -NoProfile -File installer/sandbox/run-checks.ps1 [-Msi dist\...\rpyenv-<v>-x64.msi]
#        [-Steps just-for-me,migrate,all-users,standard-user] [-Id <running sandbox>] [-Stop]
# -Steps active-setup (not a default) needs a Sandbox nobody has signed in to yet, so it
# can't take -Id: the MSI is installed for all users as SYSTEM first, then the window opens
# for the first sign-in (CHECKLIST 5). In Windows Sandbox that sign-in records the version
# but launches no StubPath (2026-10-13, process-creation audit), so it fails there.
param(
    [string] $Msi,
    [string[]] $Steps = @('just-for-me', 'migrate', 'all-users', 'standard-user'),
    [string] $Id,
    [string] $Results,
    [switch] $Stop
)
$ErrorActionPreference = 'Stop'
# `pwsh -File` passes -Steps a,b as one string.
$Steps = @($Steps -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not (Get-Command wsb -ErrorAction SilentlyContinue)) { throw "wsb (the Windows Sandbox CLI) isn't installed" }
if (-not $Msi) {
    $Msi = Get-ChildItem (Join-Path $repo 'dist') -Recurse -Filter 'rpyenv-*-x64.msi' |
        Sort-Object LastWriteTime | Select-Object -Last 1 -ExpandProperty FullName
    if (-not $Msi) { throw "no dist\**\rpyenv-*-x64.msi; build one or pass -Msi" }
}
$Msi = (Resolve-Path $Msi).Path
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (-not $Results) { $Results = Join-Path $repo "dist\sandbox-results\$stamp" }
New-Item -ItemType Directory -Force $Results | Out-Null
"MSI:     $Msi"
"results: $Results"

function Exec([string] $command, [string] $as = 'ExistingLogin') {
    $o = & wsb exec --id $Id -r $as -c $command 2>&1 | Out-String
    [pscustomobject] @{ Ok = ($LASTEXITCODE -eq 0 -and $o -match 'exited with code: 0'); Text = $o.Trim() }
}

$fresh = $Steps -contains 'active-setup'
if ($fresh -and $Id) { throw "active-setup needs a Sandbox nobody has signed in to: leave out -Id" }
if (-not $Id) {
    $o = & wsb start 2>&1 | Out-String
    if ($o -notmatch '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}') { throw "wsb start: $o" }
    $Id = $Matches[0]
    "started Sandbox $Id"
}
function WaitFor([string] $as, [int] $minutes) {
    $deadline = (Get-Date).AddMinutes($minutes)
    while (-not (Exec 'cmd /c exit 0' $as).Ok) {
        if ((Get-Date) -gt $deadline) { return $false }
        Start-Sleep 5
    }
    $true
}

# Fresh paths per run: a Sandbox can't map two host folders to one path.
$base = "C:\rpx-$stamp"
$shared = $false
function Share {
    if ($script:shared) { return }
    & wsb share --id $Id -f (Split-Path $Msi) -s "$base\msi" | Out-Null
    & wsb share --id $Id -f $PSScriptRoot -s "$base\scripts" | Out-Null
    & wsb share --id $Id -f $Results -s "$base\results" --allow-write | Out-Null
    $script:shared = $true
}

$fail = 0
function RunStep([string] $step, [string] $as = 'ExistingLogin') {
    $t0 = Get-Date
    "== $step"
    $r = Exec "powershell.exe -NoProfile -ExecutionPolicy Bypass -File $base\scripts\checks.ps1 -Step $step -Msi $base\msi\$(Split-Path $Msi -Leaf) -Out $base\results" $as
    $report = Join-Path $Results "$step\report.txt"
    if (Test-Path $report) {
        $lines = Get-Content $report
        $lines | Where-Object { $_ -match '^(PASS|FAIL)' }
        $script:fail += @($lines | Where-Object { $_ -match '^FAIL' }).Count
        if (-not ($lines | Where-Object { $_ -match '^done ' })) { "   (no 'done' line: $($r.Text))"; $script:fail++ }
    } else {
        "FAIL  $step wrote no report: $($r.Text)"
        $script:fail++
    }
    "   {0:N0} s" -f ((Get-Date) - $t0).TotalSeconds
}

if ($fresh) {
    if (-not (WaitFor 'System' 5)) { throw "Sandbox $Id runs no commands after 5 minutes" }
    if ((Exec 'cmd /c exit 0').Ok) { throw "someone has already signed in to Sandbox ${Id}: its Active Setup can't be seen" }
    Share
    RunStep 'active-setup-install' 'System'
}
# Commands run in the signed-in session; a Sandbox started from the CLI signs in once a
# window connects to it.
if (-not (Exec 'cmd /c exit 0').Ok) {
    # wsb connect returns only when its window closes.
    Start-Process wsb -ArgumentList 'connect', '--id', $Id
    "opened a window on Sandbox $Id"
    if (-not (WaitFor 'ExistingLogin' 5)) { throw "Sandbox $Id has no signed-in session after 5 minutes" }
}
Share
foreach ($step in $Steps) { RunStep $step }
if ($Stop) { & wsb stop --id $Id | Out-Null; "stopped Sandbox $Id" }
"$fail failed; reports and screenshots in $Results"
exit [int] ($fail -gt 0)
