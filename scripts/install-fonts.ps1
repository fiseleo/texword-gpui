<#
.SYNOPSIS
  Install Adobe Source Han Sans TC (2.005R) and Source Han Serif TC (2.003R) for current user (no admin needed).
#>
$ErrorActionPreference = 'Stop'

$fontDir = Join-Path $env:LOCALAPPDATA "Microsoft\Windows\Fonts"
$miktexFontDir = Join-Path $env:USERPROFILE ".fonts"
New-Item -ItemType Directory -Force -Path $fontDir | Out-Null
New-Item -ItemType Directory -Force -Path $miktexFontDir | Out-Null

$regPath = "HKCU:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts"
if (-not (Test-Path $regPath)) {
    New-Item -Path $regPath -Force | Out-Null
}

$packages = @(
    @{
        Name = "SourceHanSansTC"
        Url  = "https://github.com/adobe-fonts/source-han-sans/releases/download/2.005R/10_SourceHanSansTC.zip"
        Zip  = Join-Path $env:TEMP "10_SourceHanSansTC.zip"
    },
    @{
        Name = "SourceHanSerifTC"
        Url  = "https://github.com/adobe-fonts/source-han-serif/releases/download/2.003R/10_SourceHanSerifTC.zip"
        Zip  = Join-Path $env:TEMP "10_SourceHanSerifTC.zip"
    }
)

foreach ($pkg in $packages) {
    if (-not (Test-Path $pkg.Zip) -or (Get-Item $pkg.Zip).Length -lt 10MB) {
        Write-Host "Downloading $($pkg.Name)..." -ForegroundColor Cyan
        curl.exe -fL --retry 3 $pkg.Url -o $pkg.Zip
        if ($LASTEXITCODE -ne 0) { throw "Failed to download $($pkg.Url)" }
    }
    $extractDir = Join-Path $env:TEMP "extract_$($pkg.Name)"
    Remove-Item $extractDir -Recurse -Force -ErrorAction SilentlyContinue
    Expand-Archive -Path $pkg.Zip -DestinationPath $extractDir -Force
    Get-ChildItem -Path $extractDir -Recurse -Filter "*.otf" | ForEach-Object {
        $dest = Join-Path $fontDir $_.Name
        Copy-Item -Path $_.FullName -Destination $dest -Force
        Copy-Item -Path $_.FullName -Destination (Join-Path $miktexFontDir $_.Name) -Force
        $regName = "$($_.BaseName) (OpenType)"
        Set-ItemProperty -Path $regPath -Name $regName -Value $dest
    }
    Remove-Item $extractDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "Installed $($pkg.Name) to $fontDir" -ForegroundColor Green
}

$miktexFc = Join-Path $env:LOCALAPPDATA "Programs\MiKTeX\miktex\bin\x64\fc-cache.exe"
if (Test-Path $miktexFc) {
    Write-Host "Refreshing MiKTeX fontconfig cache..." -ForegroundColor Cyan
    & $miktexFc -f
}
