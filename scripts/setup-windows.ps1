<#
.SYNOPSIS
  Setup Windows build environment for TeXWord GPUI (Rust + MSVC Build Tools + Local Cargo target cache).
#>
$ErrorActionPreference = 'Stop'

Write-Host "=== 1. Check / Install Rust (rustup) ===" -ForegroundColor Cyan
$cargoExe = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
if (-not (Test-Path $cargoExe)) {
    $rustupInit = Join-Path $env:TEMP "rustup-init.exe"
    Invoke-WebRequest -Uri "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe" -OutFile $rustupInit
    & $rustupInit -y --default-toolchain stable --default-host x86_64-pc-windows-msvc
} else {
    Write-Host "Rust already installed: $(& $cargoExe --version)" -ForegroundColor Green
}

Write-Host "=== 2. Check / Install Visual Studio C++ Build Tools (MSVC + Windows SDK) ===" -ForegroundColor Cyan
$vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$hasVcTools = $false
if (Test-Path $vsWhere) {
    $found = & $vsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($found) {
        $hasVcTools = $true
        Write-Host "MSVC Build Tools already installed at: $found" -ForegroundColor Green
    }
}

if (-not $hasVcTools) {
    $vsExe = Join-Path $env:TEMP "vs_BuildTools.exe"
    Write-Host "Downloading Visual Studio Build Tools..." -ForegroundColor Yellow
    Invoke-WebRequest -Uri "https://aka.ms/vs/17/release/vs_BuildTools.exe" -OutFile $vsExe
    Write-Host "Launching installer (accept UAC prompt to install C++ Build Tools + Windows SDK)..." -ForegroundColor Yellow
    $args = @(
        "--add", "Microsoft.VisualStudio.Workload.VCTools",
        "--includeRecommended",
        "--passive",
        "--norestart"
    )
    Start-Process -FilePath $vsExe -ArgumentList $args -Verb RunAs -Wait
}

Write-Host "=== 3. Configure local Cargo target-dir (avoid slow SMB builds on Z:) ===" -ForegroundColor Cyan
$repoRoot = Split-Path -Parent $PSScriptRoot
$cargoDir = Join-Path $repoRoot ".cargo"
New-Item -ItemType Directory -Force -Path $cargoDir | Out-Null
$localTarget = ($env:LOCALAPPDATA -replace '\\', '/') + "/texword-gpui/target"
@"
[build]
target-dir = "$localTarget"
"@ | Set-Content -Path (Join-Path $cargoDir "config.toml") -Encoding UTF8

Write-Host "=== 4. Build & Test ===" -ForegroundColor Cyan
Set-Location $repoRoot
& $cargoExe test --workspace --exclude texword_app
& $cargoExe build --release
