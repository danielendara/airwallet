param(
    [string]$Version = "0.2.0"
)

$ErrorActionPreference = "Stop"

$PackageVersion = $Version -replace '^v', ''
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$dist = Join-Path $root "dist"
$packageName = "Cofferly-$PackageVersion-windows-x64"
$packageDir = Join-Path $dist $packageName
$zipPath = Join-Path $dist "$packageName.zip"
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"

if (-not (Test-Path $cargo)) {
    $cargo = "cargo"
}

Push-Location $root
try {
    & $cargo build --release

    if (Test-Path $packageDir) {
        Remove-Item -LiteralPath $packageDir -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $packageDir | Out-Null

    Copy-Item -LiteralPath (Join-Path $root "target\release\Cofferly.exe") -Destination $packageDir
    Copy-Item -LiteralPath (Join-Path $root "README.md") -Destination $packageDir
    Copy-Item -LiteralPath (Join-Path $root "LICENSE") -Destination $packageDir
    Copy-Item -LiteralPath (Join-Path $root "CHANGELOG.md") -Destination $packageDir
    Copy-Item -LiteralPath (Join-Path $root "SECURITY.md") -Destination $packageDir
    Copy-Item -LiteralPath (Join-Path $root "docs") -Destination $packageDir -Recurse

    @"
Cofferly $PackageVersion

Run Cofferly.exe to start.
On first run, Cofferly generates a six-object Coffer Story. Write it down and store it safely.

Data is stored locally in the Windows app data folder.
"@ | Set-Content -LiteralPath (Join-Path $packageDir "START-HERE.txt")

    if (Test-Path $zipPath) {
        Remove-Item -LiteralPath $zipPath -Force
    }
    Compress-Archive -Path (Join-Path $packageDir "*") -DestinationPath $zipPath

    Write-Host "Created $zipPath"
}
finally {
    Pop-Location
}
