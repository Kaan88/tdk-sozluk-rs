# Builds the release exe and the installer: target\installer\Sozluk-Setup-<version>.exe
param([string]$Version = '')
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)
if (-not $Version) { $Version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value }
cargo build --release --locked
if ($LASTEXITCODE) { exit $LASTEXITCODE }
$iscc = Get-ChildItem "${env:ProgramFiles(x86)}\Inno Setup *\ISCC.exe", "$env:ProgramFiles\Inno Setup *\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup *\ISCC.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $iscc) { throw 'ISCC.exe (Inno Setup) not found' }
& $iscc.FullName "/DAppVersion=$Version" installer\sozluk.iss
exit $LASTEXITCODE
