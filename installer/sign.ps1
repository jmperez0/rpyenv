# Signs release files (M6b design D3, plan P3). v0.1.0 ships unsigned: without
# RPYENV_SIGN_TOOL this only says so. The chosen provider's command goes here.
param([Parameter(ValueFromRemainingArguments)] [string[]] $Files)
$ErrorActionPreference = 'Stop'
if (-not $env:RPYENV_SIGN_TOOL) {
    Write-Host "unsigned (no signing configured): $($Files -join ', ')"
    exit 0
}
throw "RPYENV_SIGN_TOOL is set to '$env:RPYENV_SIGN_TOOL', but no provider is implemented yet"
