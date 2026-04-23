#!/usr/bin/env pwsh
#Requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

<#
.SYNOPSIS
    Installs the `stardive` CLI on Windows without permanently installing pkgx or Rust.

.DESCRIPTION
    Uses the pkgx PowerShell bootstrap to create a temporary Rust toolchain environment,
    runs `cargo install stardive` in that temporary environment, and installs only the
    final `stardive` binary to a target directory (default: C:\ProgramData\stardive\bin).

.PARAMETER StardiveVersion
    Optional version to install (e.g., 0.1.0). If not specified, installs the latest.

.PARAMETER StardiveInstallDir
    Directory where the stardive binary will be installed.
    Default: C:\ProgramData\stardive\bin

.EXAMPLE
    .\install-stardive.ps1

.EXAMPLE
    .\install-stardive.ps1 -StardiveVersion 0.1.0 -StardiveInstallDir "$env:LOCALAPPDATA\bin"

.EXAMPLE
    # One-liner from README (download and run):
    powershell -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/nmb4/stardive-api/main/installers/install-stardive.ps1 | iex"
#>

param(
    [string]$StardiveVersion = "",
    [string]$StardiveInstallDir = "$env:ProgramData\stardive\bin"
)

function Write-ErrorAndExit {
    param([string]$Message)
    Write-Host $Message -ForegroundColor Red
    exit 1
}

# --- Prerequisites ---

# Check for curl/irm availability (Invoke-RestMethod is built into PowerShell)
try {
    $null = Invoke-RestMethod -Uri "https://pkgx.sh" -Method Head -UseBasicParsing -TimeoutSec 5
}
catch {
    Write-ErrorAndExit "Network access to pkgx.sh failed. Check your internet connection."
}

# Check for install capability (we'll use Copy-Item + Set-ItemProperty as fallback)
$hasInstallCmd = $false
$installCmd = Get-Command "install" -ErrorAction SilentlyContinue
if ($installCmd) {
    $hasInstallCmd = $true
}

# --- Bootstrap pkgx with Rust toolchain ---

Write-Host "Bootstrapping temporary Rust toolchain via pkgx..." -ForegroundColor Cyan

try {
    $pkgxBootstrap = Invoke-RestMethod -Uri "https://pkgx.sh" -UseBasicParsing
    Invoke-Expression $pkgxBootstrap
    # Load pkgx into the current session and add Rust + curl
    $pkgxEnv = pkgx +rust-lang.org +curl.se -e
    if ($pkgxEnv) {
        Invoke-Expression $pkgxEnv
    }
}
catch {
    Write-ErrorAndExit "Failed to acquire temporary pkgx rust toolchain: $_"
}

# Verify cargo is available
if (-not (Get-Command "cargo" -ErrorAction SilentlyContinue)) {
    Write-ErrorAndExit "cargo unavailable after pkgx bootstrap"
}

Write-Host "cargo found: $(cargo --version)" -ForegroundColor Green

# --- Build and install ---

$WorkDir = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null

function Cleanup {
    if (Test-Path $WorkDir) {
        Remove-Item -Recurse -Force $WorkDir -ErrorAction SilentlyContinue
    }
}

try {
    $env:CARGO_HOME = Join-Path $WorkDir "cargo-home"
    $env:RUSTUP_HOME = Join-Path $WorkDir "rustup-home"
    $InstallRoot = Join-Path $WorkDir "stardive-root"

    $CrateSpec = "stardive"
    if ($StardiveVersion) {
        $CrateSpec = "stardive@${StardiveVersion}"
    }

    Write-Host "Installing $CrateSpec to temporary root..." -ForegroundColor Cyan
    cargo install --locked --root "$InstallRoot" "$CrateSpec"

    $BinSrc = Join-Path $InstallRoot "bin" "stardive.exe"
    if (-not (Test-Path $BinSrc)) {
        Write-ErrorAndExit "Build completed but stardive binary was not produced at $BinSrc"
    }

    $TargetDir = $StardiveInstallDir
    $TargetPath = Join-Path $TargetDir "stardive.exe"

    # Create target directory if it doesn't exist
    if (-not (Test-Path $TargetDir)) {
        New-Item -ItemType Directory -Path $TargetDir -Force | Out-Null
    }

    # Check write access
    $canWrite = $false
    try {
        $testFile = Join-Path $TargetDir ".write-test-$(Get-Random).tmp"
        [System.IO.File]::WriteAllText($testFile, "") | Out-Null
        Remove-Item $testFile -Force -ErrorAction SilentlyContinue
        $canWrite = $true
    }
    catch {
        $canWrite = $false
    }

    if ($canWrite) {
        Copy-Item -Path $BinSrc -Destination $TargetPath -Force
        # Ensure it's executable (on Windows this mainly means ensuring the file exists with correct name)
    }
    else {
        Write-Host "No write access to $TargetDir, attempting to run as Administrator via sudo..." -ForegroundColor Yellow
        # Try using sudo if available (e.g., from WSL or Git Bash)
        $sudoCmd = Get-Command "sudo" -ErrorAction SilentlyContinue
        if ($sudoCmd) {
            # Use a PowerShell script block elevated via sudo powershell
            $copyScript = {
                Copy-Item -Path '$BinSrc' -Destination '$TargetPath' -Force
            }
            $scriptContent = $copyScript.ToString().Replace('$BinSrc', $BinSrc).Replace('$TargetPath', $TargetPath)
            sudo powershell -Command $scriptContent
        }
        else {
            Write-ErrorAndExit "Need write access to $TargetDir (try running as Administrator: 'Start-Process powershell -Verb RunAs')"
        }
    }

    Write-Host "Installed stardive to $TargetPath" -ForegroundColor Green
    Write-Host "Run: stardive --help" -ForegroundColor Green

    # Add to PATH hint if not already present
    $pathDirs = $env:PATH -split [System.IO.Path]::PathSeparator
    if ($TargetDir -notin $pathDirs) {
        Write-Host ""
        Write-Host "Note: $TargetDir is not in your PATH. Add it with:" -ForegroundColor Yellow
        Write-Host "  [Environment]::SetEnvironmentVariable('PATH', `"`$env:PATH;$TargetDir`", 'User')" -ForegroundColor Yellow
        Write-Host "  Or add to current session: `$env:PATH += `";$TargetDir`"" -ForegroundColor Yellow
    }
}
finally {
    Cleanup
}
