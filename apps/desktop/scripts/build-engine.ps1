#Requires -Version 5.1
param(
    [string]$Python = $env:PKB_PYTHON
)

$ErrorActionPreference = "Stop"
$DesktopRoot = Split-Path -Parent $PSScriptRoot
$RepoRoot = Resolve-Path (Join-Path $DesktopRoot "..\..")
$RunEngine = Join-Path $RepoRoot "src\python\run_engine.py"
$OutDir = Join-Path $DesktopRoot "src-tauri\binaries"

if (-not $Python) {
    $candidates = @(
        "$env:LOCALAPPDATA\Programs\Python\Python312-arm64\python.exe",
        "$env:LOCALAPPDATA\Programs\Python\Python312\python.exe"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { $Python = $c; break }
    }
}
if (-not $Python) { $Python = "python" }

Write-Host "Python: $Python"
Write-Host "Entry : $RunEngine"

& $Python -m pip install pyinstaller -q
& $Python -m PyInstaller `
    --onefile `
    --name pkb-engine `
    --distpath $OutDir `
    --workpath (Join-Path $DesktopRoot "build-engine") `
    --specpath (Join-Path $DesktopRoot "build-engine") `
    --paths (Join-Path $RepoRoot "src\python") `
    --hidden-import engine_stdio `
    --hidden-import core.facade `
    --hidden-import core.paths `
    --hidden-import core.settings_api `
    --hidden-import core.profile_store `
    --hidden-import numpy `
    --collect-submodules core `
    $RunEngine

$arch = if ($env:PROCESSOR_ARCHITECTURE -match "ARM") { "aarch64" } else { "x86_64" }
$target = "pkb-engine-$arch-pc-windows-msvc.exe"
$built = Join-Path $OutDir "pkb-engine.exe"
$dest = Join-Path $OutDir $target

if (-not (Test-Path $built)) {
    throw "PyInstaller output not found: $built"
}

Copy-Item -Force $built $dest
$sizeMb = [math]::Round((Get-Item $dest).Length / 1MB, 1)
if ((Get-Item $dest).Length -lt 1MB) {
    throw "Engine binary too small (${sizeMb} MB)."
}

Write-Host "Engine ready: $dest ($sizeMb MB)"
