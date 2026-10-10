# Writes dist\sandbox.wsb: a Windows Sandbox that sees this repository's dist folder and
# installer\sandbox read-only (M6b design §7.4). The host's own setup is never touched.
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$dist = Join-Path $repo 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$wsb = @"
<Configuration>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$dist</HostFolder>
      <SandboxFolder>C:\rpyenv-dist</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
    <MappedFolder>
      <HostFolder>$PSScriptRoot</HostFolder>
      <SandboxFolder>C:\rpyenv-sandbox</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>explorer.exe C:\rpyenv-dist</Command>
  </LogonCommand>
</Configuration>
"@
$out = Join-Path $dist 'sandbox.wsb'
Set-Content -Path $out -Value $wsb -Encoding utf8
$out
