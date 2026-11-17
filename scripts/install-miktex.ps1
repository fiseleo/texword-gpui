<#
.SYNOPSIS
  Install MiKTeX (XeLaTeX + xdvipdfmx + bibtexu) for current user without admin privileges.
#>
$ErrorActionPreference = 'Stop'

$miktexBin = Join-Path $env:LOCALAPPDATA "Programs\MiKTeX\miktex\bin\x64"
$xelatexExe = Join-Path $miktexBin "xelatex.exe"

if (Test-Path $xelatexExe) {
    Write-Host "MiKTeX already installed at: $miktexBin" -ForegroundColor Green
    & $xelatexExe --version | Select-Object -First 2
    exit 0
}

$installer = Join-Path $env:TEMP "basic-miktex-25.12-x64.exe"
$url = "https://miktex.org/download/ctan/systems/win32/miktex/setup/windows-x64/basic-miktex-25.12-x64.exe"
Write-Host "Downloading MiKTeX (~142 MB)..." -ForegroundColor Cyan
curl.exe -fL $url -o $installer

Write-Host "Installing MiKTeX for current user (auto-install packages enabled)..." -ForegroundColor Cyan
& $installer --unattended --auto-install=yes --shared=no

if (Test-Path $xelatexExe) {
    Write-Host "Installed XeLaTeX at: $xelatexExe" -ForegroundColor Green
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$miktexBin*") {
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$miktexBin", "User")
        Write-Host "Added MiKTeX to user PATH." -ForegroundColor Green
    }
} else {
    Write-Host "MiKTeX installer finished; please check %LOCALAPPDATA%\Programs\MiKTeX or open MiKTeX Console." -ForegroundColor Yellow
}
