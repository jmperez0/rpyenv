# Phase 3 spec §8: proves ci/check_signed.py accepts a file that only gained a signature and
# rejects one that differs by one byte. Signs copies of this run's x64 build with a throwaway
# self-signed certificate, trusted in LocalMachine\Root on the GitHub runner only.
param([Parameter(Mandatory)] [string] $Exes, [Parameter(Mandatory)] [string] $Msi)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$version = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value
$signtool = (Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\signtool.exe" |
    Sort-Object { [version]$_.Directory.Parent.Name } | Select-Object -Last 1).FullName
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=rpyenv CI self-test' `
    -CertStoreLocation Cert:\CurrentUser\My -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
    -NotAfter (Get-Date).AddDays(1)
$cer = Join-Path $env:RUNNER_TEMP 'selftest.cer'
Export-Certificate -Cert $cert -FilePath $cer | Out-Null
Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
$work = Join-Path $env:RUNNER_TEMP 'selftest'

function Copy-To([string] $Dir, [string[]] $Files) {
    New-Item -ItemType Directory -Force $Dir | Out-Null
    foreach ($f in $Files) { Copy-Item $f $Dir }
    (Get-ChildItem $Dir -File).FullName
}
function Sign([string[]] $Files, [switch] $NoTimestamp) {
    $stamp = if ($NoTimestamp) { @() } else { @('/tr', 'http://timestamp.digicert.com', '/td', 'sha256') }
    & $signtool sign /sha1 $cert.Thumbprint /fd sha256 @stamp /d rpyenv $Files
    if ($LASTEXITCODE) { throw "signtool sign failed ($LASTEXITCODE)" }
}
function Flip-Byte([string] $File, [int] $Offset) {
    $b = [IO.File]::ReadAllBytes($File); $b[$Offset] = $b[$Offset] -bxor 1; [IO.File]::WriteAllBytes($File, $b)
}
# -PinnedThumbprint checks against installer/signing.json's thumbprint (the Certum card's)
# instead of the throwaway certificate's.
function Check([string] $Kind, [string[]] $Unsigned, [string[]] $Signed, [switch] $PinnedThumbprint) {
    $zips = Join-Path $work "zips-$Kind-$([guid]::NewGuid())"
    New-Item -ItemType Directory $zips | Out-Null
    Compress-Archive -Path $Unsigned -DestinationPath (Join-Path $zips "tmp-unsigned-$Kind-x64.zip")
    Compress-Archive -Path $Signed -DestinationPath (Join-Path $zips "tmp-signed-$Kind-x64.zip")
    $pin = if ($PinnedThumbprint) { @() } else { @('--thumbprint', $cert.Thumbprint) }
    $out = & python ci/check_signed.py $Kind --version $version --zips $zips --out (Join-Path $zips out) `
        --no-attestation @pin --arches x64 2>&1 | Out-String
    $code = $LASTEXITCODE
    Write-Host $out
    @{ Code = $code; Text = $out }
}
function Expect([string] $Case, $Result, [int] $Code, [string] $Pattern) {
    if ($Result.Code -ne $Code -or $Result.Text -notmatch $Pattern) {
        throw "self-test '$Case': expected exit $Code and /$Pattern/, got exit $($Result.Code)"
    }
    Write-Host "self-test '$Case': as expected"
}

Push-Location $repo
try {
    $u = Copy-To (Join-Path $work 'u-exes') (Get-ChildItem $Exes -Filter *.exe).FullName
    $s = Copy-To (Join-Path $work 's-exes') $u
    Sign $s
    # 1. Only a signature added: every executable passes. If this fails, the comparison
    #    method in ci/signing.py is wrong for what signtool writes (spec §8.1).
    Expect 'exes signed' (Check exes $u $s) 0 'ok x64/pyenv\.exe'
    # 2. The unsigned reference differs by one byte in the code: the comparison alone must fail.
    $m = Copy-To (Join-Path $work 'm-exes') $u
    Flip-Byte (Join-Path $work 'm-exes\pyenv.exe') 0x400
    Expect 'exes one byte' (Check exes $m $s) 1 'FAIL x64/pyenv\.exe: differs from the unsigned file'
    # 3. The signed file changed after signing: the whole check must fail.
    $t = Copy-To (Join-Path $work 't-exes') $s
    Flip-Byte (Join-Path $work 't-exes\pyenv.exe') 0x400
    Expect 'exes tampered' (Check exes $u $t) 1 'FAIL x64/pyenv\.exe'
    # 6. A valid signature by another certificate: the pinned thumbprint must refuse it.
    Expect 'exes other signer' (Check exes $u $s -PinnedThumbprint) 1 'signed by [0-9A-F]{40}, expected 58E0373B2F4DF17A924C8B1EAB467FC7619A4403'
    # 7. A signature without a timestamp must fail.
    $n = Copy-To (Join-Path $work 'n-exes') $u
    Sign $n -NoTimestamp
    Expect 'exes no timestamp' (Check exes $u $n) 1 'FAIL x64/pyenv\.exe: no timestamp'

    $um = Copy-To (Join-Path $work 'u-msi') (Get-ChildItem $Msi -Filter *.msi).FullName
    $sm = Copy-To (Join-Path $work 's-msi') $um
    Sign $sm
    # 4. Only a signature added to the MSI.
    Expect 'msi signed' (Check msi $um $sm) 0 "ok x64/rpyenv-$([regex]::Escape($version))-x64\.msi"
    # 5. The unsigned MSI's largest stream differs by one byte: the comparison must fail.
    $mm = Copy-To (Join-Path $work 'm-msi') $um
    # One file comes back as a plain string, so index nothing: name it again.
    $mmFile = (Get-ChildItem (Join-Path $work 'm-msi') -Filter *.msi | Select-Object -First 1).FullName
    python -c "import sys, olefile; o = olefile.OleFileIO(sys.argv[1], write_mode=True); e = max(o.listdir(), key=o.get_size); d = bytearray(o.openstream(e).read()); d[0] ^= 1; o.write_stream(e, bytes(d)); o.close()" $mmFile
    if ($LASTEXITCODE) { throw "could not change the MSI stream" }
    Expect 'msi one byte' (Check msi $mm $sm) 1 'differs from the unsigned file'
}
finally { Pop-Location }
# The last case leaves $LASTEXITCODE at 1 on purpose, and GitHub's pwsh step exits with
# $LASTEXITCODE. Every case passed to get here; a failed case throws above.
exit 0
