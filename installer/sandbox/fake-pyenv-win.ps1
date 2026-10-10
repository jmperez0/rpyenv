# Inside the Sandbox only: a stand-in pyenv-win (its launchers, and bin on the user PATH),
# so the installer offers `pyenv migrate`.
$ErrorActionPreference = 'Stop'
$bin = Join-Path $env:USERPROFILE '.pyenv\pyenv-win\bin'
New-Item -ItemType Directory -Force $bin | Out-Null
foreach ($n in 'pyenv.ps1', 'pyenv.bat', 'pyenv') { Set-Content (Join-Path $bin $n) "$n from pyenv-win" }
$path = [Environment]::GetEnvironmentVariable('Path', 'User')
[Environment]::SetEnvironmentVariable('Path', "$bin;$path", 'User')
"fake pyenv-win in $bin"
