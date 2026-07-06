$ErrorActionPreference = "Stop"
. "$PSScriptRoot\ensure-node-path.ps1"

if (-not $env:PKB_PYTHON) {
    $candidates = @(
        "$env:LOCALAPPDATA\Programs\Python\Python312-arm64\python.exe",
        "$env:LOCALAPPDATA\Programs\Python\Python312\python.exe",
        "$env:LOCALAPPDATA\Programs\Python\Python313-arm64\python.exe"
    )
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            $env:PKB_PYTHON = $candidate
            Write-Host "PKB_PYTHON=$candidate"
            break
        }
    }
}

Set-Location (Join-Path $PSScriptRoot "..")
& node "./node_modules/@tauri-apps/cli/tauri.js" dev
