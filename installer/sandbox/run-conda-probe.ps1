# Runs conda-probe.ps1 (M7c design §3) in Windows Sandbox through the wsb CLI: starts a
# Sandbox (or uses -Id), shares the installers' folder read-only and a results folder
# writable, starts the probe in the signed-in session and waits for its done.txt (Anaconda
# alone takes minutes, so the probe runs detached and this script polls). Nothing installs
# on the host.
#   pwsh -NoProfile -File installer/sandbox/run-conda-probe.ps1 [-Installers C:\tmp\m7c-installers]
#        [-Id <running sandbox>] [-Stop] [-TimeoutMinutes 150] [-PathCharsOnly] [-CheckOnly]
# Each installer must match the SHA-256 the publisher lists (table below); -CheckOnly stops
# after that check, before any Sandbox starts.
param(
    [string] $Installers = 'C:\tmp\m7c-installers',
    [string] $Id,
    [string] $Results,
    [int] $TimeoutMinutes = 150,
    [switch] $PathCharsOnly,
    [switch] $Extra,
    [switch] $Stop,
    [switch] $CheckOnly
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not (Get-Command wsb -ErrorAction SilentlyContinue)) { throw "wsb (the Windows Sandbox CLI) isn't installed" }
$Installers = (Resolve-Path $Installers).Path
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (-not $Results) { $Results = Join-Path $repo "dist\sandbox-results\conda-$stamp" }
New-Item -ItemType Directory -Force $Results | Out-Null
"installers: $Installers"
"results:    $Results"
# Publisher SHA-256s, fetched 2026-10-16 from repo.anaconda.com's listing (SHA256 column) and
# the Miniforge release's <asset>.sha256. A missing or different file stops the run before
# anything is shared with the Sandbox.
$expected = @{
    'Miniforge3-26.7.2-0-Windows-x86_64.exe'       = '71cf9519087be74fa53021219ff292beb2fc05fa49e0bb6eb0e0b6b14fccbaab'
    'Miniconda3-py313_26.7.1-1-Windows-x86_64.exe' = 'ff485c617e0e15addc111aa1caa206679e214d223624297bf8aa0bc726ed67bd'
    'Anaconda3-2026.07-1-Windows-x86_64.exe'       = 'b545f4bd8ab3bf32d99002a0779a887668ebfe479ee32ecbf060375670d5ee09'
}
$bad = 0
foreach ($name in $expected.Keys) {
    $f = Join-Path $Installers $name
    if (-not (Test-Path -LiteralPath $f)) { "MISSING   $name"; $bad++; continue }
    $got = (Get-FileHash $f -Algorithm SHA256).Hash.ToLower()
    if ($got -ne $expected[$name]) { "MISMATCH  $name  got $got"; $bad++ } else { "ok        $name  $got" }
}
if ($bad) { "$bad installer(s) failed the publisher check"; exit 1 }
if ($CheckOnly) { exit 0 }

function Exec([string] $command, [string] $as = 'ExistingLogin') {
    $o = & wsb exec --id $Id -r $as -c $command 2>&1 | Out-String
    [pscustomobject] @{ Ok = ($LASTEXITCODE -eq 0 -and $o -match 'exited with code: 0'); Text = $o.Trim() }
}
if (-not $Id) {
    $o = & wsb start 2>&1 | Out-String
    if ($o -notmatch '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}') { throw "wsb start: $o" }
    $Id = $Matches[0]
    "started Sandbox $Id"
}
# Commands run in the signed-in session; a Sandbox started from the CLI signs in once a
# window connects to it (wsb connect returns only when its window closes).
if (-not (Exec 'cmd /c exit 0').Ok) {
    Start-Process wsb -ArgumentList 'connect', '--id', $Id
    "opened a window on Sandbox $Id"
    $deadline = (Get-Date).AddMinutes(5)
    while (-not (Exec 'cmd /c exit 0').Ok) {
        if ((Get-Date) -gt $deadline) { throw "Sandbox $Id has no signed-in session after 5 minutes" }
        Start-Sleep 5
    }
}
$base = "C:\rpx-$stamp"
& wsb share --id $Id -f $Installers -s "$base\installers" | Out-Null
& wsb share --id $Id -f $PSScriptRoot -s "$base\scripts" | Out-Null
& wsb share --id $Id -f $Results -s "$base\results" --allow-write | Out-Null

$probeArgs = $(if ($PathCharsOnly) { ",'-PathCharsOnly'" } else { '' }) + $(if ($Extra) { ",'-Extra'" } else { '' })
$r = Exec "powershell.exe -NoProfile -Command `"Start-Process powershell.exe -WindowStyle Hidden -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','$base\scripts\conda-probe.ps1','-Installers','$base\installers','-Out','$base\results'$probeArgs`""
if (-not $r.Ok) { throw "could not start the probe: $($r.Text)" }
"probe started; waiting for $Results\done.txt"
$deadline = (Get-Date).AddMinutes($TimeoutMinutes)
$fail = 0
while (-not (Test-Path (Join-Path $Results 'done.txt'))) {
    if ((Get-Date) -gt $deadline) { "FAIL  no done.txt after $TimeoutMinutes minutes"; $fail = 1; break }
    Start-Sleep 30
}
Get-ChildItem $Results -Filter '*-run.txt' | ForEach-Object { "$($_.Name): $((Get-Content $_.FullName) -join ' ')" }
if ($Stop) { & wsb stop --id $Id | Out-Null; "stopped Sandbox $Id" }
"results in $Results"
exit $fail
