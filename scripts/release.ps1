<#
.SYNOPSIS
    WorDF automated release packaging script for Windows.
    Builds optimized release binaries, bundles documentation, and computes SHA-256 checksums.

.PARAMETER Flavor
    Which flavor to build: 'all' (default), 'default' (with embedded CJK font), or 'lite' (no embedded font).
#>

[CmdletBinding()]
param (
    [ValidateSet("all", "default", "lite")]
    [string]$Flavor = "all"
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $ProjectRoot

$CargoToml = Get-Content (Join-Path $ProjectRoot "Cargo.toml") -Raw
if ($CargoToml -match 'version\s*=\s*"([^"]+)"') {
    $Version = $matches[1]
} else {
    $Version = "0.1.0"
}

$DistDir = Join-Path $ProjectRoot "dist"
if (Test-Path $DistDir) {
    Remove-Item -Recurse -Force $DistDir
}
New-Item -ItemType Directory -Path $DistDir | Out-Null

Write-Host "==================================================" -ForegroundColor Cyan
Write-Host " WorDF Release Packager - v$Version" -ForegroundColor Cyan
Write-Host "==================================================" -ForegroundColor Cyan

function Package-Binary([string]$FlavorName, [string]$CargoFlags, [string]$ArchiveSuffix) {
    Write-Host "`n>> Building [$FlavorName] flavor: cargo build --release $CargoFlags..." -ForegroundColor Yellow
    if ($CargoFlags) {
        $BuildCmd = "cargo build --release $CargoFlags"
    } else {
        $BuildCmd = "cargo build --release"
    }
    Invoke-Expression $BuildCmd
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Build failed for flavor [$FlavorName]"
        exit $LASTEXITCODE
    }

    $TargetExe = Join-Path $ProjectRoot "target\release\wordf.exe"
    if (-not (Test-Path $TargetExe)) {
        Write-Error "Release executable not found: $TargetExe"
        exit 1
    }

    $SizeMb = [math]::Round(((Get-Item $TargetExe).Length / 1MB), 2)
    Write-Host ">> Build succeeded. Binary size: $SizeMb MB" -ForegroundColor Green

    $StagingDir = Join-Path $DistDir "stage_$FlavorName"
    New-Item -ItemType Directory -Path $StagingDir | Out-Null

    Copy-Item $TargetExe -Destination (Join-Path $StagingDir "wordf.exe")
    Copy-Item (Join-Path $ProjectRoot "README.md") -Destination (Join-Path $StagingDir "README.md")
    Copy-Item (Join-Path $ProjectRoot "LICENSE") -Destination (Join-Path $StagingDir "LICENSE")

    $ZipName = "wordf-$ArchiveSuffix-v$Version-windows-x86_64.zip"
    $ZipPath = Join-Path $DistDir $ZipName

    Write-Host ">> Creating archive: $ZipName..." -ForegroundColor Yellow
    Compress-Archive -Path "$StagingDir\*" -DestinationPath $ZipPath -Force
    Remove-Item -Recurse -Force $StagingDir

    $ZipSizeMb = [math]::Round(((Get-Item $ZipPath).Length / 1MB), 2)
    $Hash = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLower()

    Write-Host ">> Package ready: $ZipName ($ZipSizeMb MB) SHA256: $Hash" -ForegroundColor Green
    "$Hash  $ZipName" | Out-File -Append -Encoding ascii (Join-Path $DistDir "SHA256SUMS.txt")
}

if ($Flavor -eq "all" -or $Flavor -eq "default") {
    Package-Binary -FlavorName "default" -CargoFlags "" -ArchiveSuffix "full"
}

if ($Flavor -eq "all" -or $Flavor -eq "lite") {
    Package-Binary -FlavorName "lite" -CargoFlags "--no-default-features" -ArchiveSuffix "lite"
}

Write-Host "`n==================================================" -ForegroundColor Cyan
Write-Host " Distribution packages generated in: $DistDir" -ForegroundColor Cyan
Get-ChildItem -Path $DistDir | Format-Table Name, Length, LastWriteTime
Write-Host "`nSHA256SUMS:" -ForegroundColor Cyan
Get-Content (Join-Path $DistDir "SHA256SUMS.txt")
Write-Host "==================================================" -ForegroundColor Cyan
