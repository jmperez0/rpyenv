# Measures the real conda installers for M7c (design §3, §5 step 2). Runs INSIDE Windows
# Sandbox (Windows PowerShell 5.1), started by run-conda-probe.ps1; never run it on a
# development machine: it installs three conda distributions.
#   powershell -NoProfile -ExecutionPolicy Bypass -File conda-probe.ps1 -Installers <dir> -Out <dir>
# Per installer: registry (HKCU, HKLM\SOFTWARE), user and machine PATH, Start menu (user and
# common) and the profile's top-level folders before and after; whether /S blocks; the
# install's file tree and conda-meta; `import ssl`. Then one install per path character
# (Miniforge and Miniconda; Anaconda only for characters either of them refused).
param([string] $Installers, [string] $Out, [switch] $PathCharsOnly, [switch] $Extra)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null
# A failure ends the run at once, so the host doesn't wait for a done.txt that never comes.
trap { ($_ | Out-String) | Set-Content "$Out\error.txt"; 'failed' | Set-Content "$Out\done.txt"; exit 1 }
function Utf8([string] $path, $lines) { [IO.File]::WriteAllLines($path, [string[]] @($lines), (New-Object Text.UTF8Encoding $false)) }
function Snap([string] $tag) {
    reg export HKCU "$Out\$tag-hkcu.reg" /y | Out-Null
    reg export HKLM\SOFTWARE "$Out\$tag-hklm.reg" /y | Out-Null
    Utf8 "$Out\$tag-userpath.txt" ([Environment]::GetEnvironmentVariable('PATH', 'User'))
    Utf8 "$Out\$tag-machinepath.txt" ([Environment]::GetEnvironmentVariable('PATH', 'Machine'))
    $menus = @("$env:APPDATA\Microsoft\Windows\Start Menu", "$env:ProgramData\Microsoft\Windows\Start Menu")
    Utf8 "$Out\$tag-startmenu.txt" (Get-ChildItem $menus -Recurse -Force -ErrorAction SilentlyContinue | ForEach-Object FullName)
    Utf8 "$Out\$tag-profile.txt" (Get-ChildItem $env:USERPROFILE -Force -ErrorAction SilentlyContinue | ForEach-Object Name)
}
function Install([string] $exe, [string] $target) {
    $args2 = @('/S', '/InstallationType=JustMe', '/AddToPath=0', '/RegisterPython=0', '/NoRegistry=1', '/NoShortcuts=1', "/D=$target")
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process $exe -ArgumentList $args2 -PassThru
    $p.WaitForExit()
    [pscustomobject] @{ Exit = $p.ExitCode; Seconds = [int] $sw.Elapsed.TotalSeconds; PythonAtExit = (Test-Path -LiteralPath "$target\python.exe") }
}
$all = @(Get-ChildItem $Installers -Filter '*-Windows-x86_64.exe')
foreach ($exe in $(if ($PathCharsOnly) { @() } else { $all })) {
    $fam = ($exe.Name -split '-')[0].ToLower()
    $target = "C:\probe\$fam"
    "== $fam"
    Snap "$fam-before"
    $r = Install $exe.FullName $target
    # /S blocks if python.exe exists when the stub exits, and nothing from the install is left running.
    $left = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($target, 'OrdinalIgnoreCase') } | ForEach-Object Name)
    Utf8 "$Out\$fam-run.txt" "exit=$($r.Exit) seconds=$($r.Seconds) python_at_exit=$($r.PythonAtExit) processes_left=$($left -join ',')"
    Start-Sleep 20
    Add-Content "$Out\$fam-run.txt" "python_after_20s=$(Test-Path -LiteralPath "$target\python.exe")"
    Snap "$fam-after"
    Utf8 "$Out\$fam-tree.txt" (Get-ChildItem $target -Recurse -File -Force | ForEach-Object { $_.FullName.Substring($target.Length + 1) })
    Copy-Item "$target\conda-meta" "$Out\$fam-conda-meta" -Recurse
    & "$target\python.exe" -c "import sys, ssl; print(sys.version); print(ssl.OPENSSL_VERSION)" *> "$Out\$fam-import.txt"
    "import_exit=$LASTEXITCODE" | Add-Content "$Out\$fam-import.txt"
    Remove-Item $target -Recurse -Force -ErrorAction SilentlyContinue
}
# Path characters. Built from code points so this file needs no encoding.
$chars = @(' ', [string][char]0xE9, '^', '%', '!', '=', '&', '(', ';', ',', "'", '#', '$', '@', '+', '~', '[', '{', '-', '.', [string][char]0x4E2D)
# -Extra: a second round with only the characters in $more.
$more = @([string][char]0x5F, ')', ']', '}', [string][char]0x416, [string][char]0xFC)
if ($Extra) { $chars = $more } else { $chars += $more }
$refused = @{}
foreach ($fam in 'miniforge3', 'miniconda3', 'anaconda3') {
    $exe = $all | Where-Object { $_.Name.ToLower().StartsWith($fam) } | Select-Object -First 1
    if (-not $exe) { continue }
    foreach ($c in $chars) {
        if ($fam -eq 'anaconda3' -and -not $refused.ContainsKey($c)) { continue }
        $code = [int][char]$c
        $t = "C:\probe\c$code-a$($c)b"
        $r = Install $exe.FullName $t
        $ok = Test-Path -LiteralPath "$t\python.exe"
        if (-not $ok) { $refused[$c] = $true }
        # No -f: $c itself can be a brace.
        Add-Content "$Out\path-chars.txt" ("$fam`tU+" + $code.ToString('X4') + "`t$c`texit=$($r.Exit)`tseconds=$($r.Seconds)`tpython=$ok") -Encoding UTF8
        # a refusal may leave a folder behind or a mangled sibling: record BEFORE deleting, or the check cannot fail
        Add-Content "$Out\path-chars-leftovers.txt" ("$fam U+" + $code.ToString('X4') + ': ' + ((Get-ChildItem C:\probe -Force -ErrorAction SilentlyContinue | ForEach-Object Name) -join '|')) -Encoding UTF8
        Remove-Item -LiteralPath $t -Recurse -Force -ErrorAction SilentlyContinue
    }
}
'done' | Set-Content "$Out\done.txt"
