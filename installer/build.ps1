# Builds rpyenv's MSI with the pinned WiX 5.0.2 (M6b design §4). Prints the MSI's path.
param(
    [Parameter(Mandatory)] [ValidateSet('x64', 'arm64')] [string] $Arch,
    [Parameter(Mandatory)] [string] $BinDir,
    [string] $OutDir = 'dist',
    # '' = the workspace version; 'Next' = its patch + 1 (the upgrade test, plan P7).
    [string] $Version = ''
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$workspace = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value
if ($Version -eq '') { $Version = $workspace }
elseif ($Version -eq 'Next') {
    $parts = $workspace.Split('.')
    $Version = "$($parts[0]).$($parts[1]).$([int]$parts[2] + 1)"
}
$comma = $Version.Replace('.', ',')
$bin = (Resolve-Path $BinDir).Path
Push-Location $repo
try {
    dotnet tool restore | Out-Null
    if ($LASTEXITCODE) { throw "dotnet tool restore failed ($LASTEXITCODE)" }
    dotnet tool run wix extension add WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2 | Out-Null
    if ($LASTEXITCODE) { throw "wix extension add failed ($LASTEXITCODE)" }
    New-Item -ItemType Directory -Force $OutDir | Out-Null
    $out = Join-Path (Resolve-Path $OutDir).Path "rpyenv-$Version-$Arch.msi"
    dotnet tool run wix build installer/rpyenv.wxs -arch $Arch `
        -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
        -d "Version=$Version" -d "VersionComma=$comma" -d "BinDir=$bin" -d "RepoDir=$repo" `
        -o $out
    if ($LASTEXITCODE) { throw "wix build failed ($LASTEXITCODE)" }
    $out
}
finally { Pop-Location }
