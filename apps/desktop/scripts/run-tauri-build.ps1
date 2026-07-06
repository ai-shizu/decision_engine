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

Write-Host "=== Step 1/2: Python エンジン (PyInstaller, 数分かかります) ==="
& powershell -NoProfile -ExecutionPolicy Bypass -File "$PSScriptRoot\build-engine.ps1"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "=== Step 2/2: Tauri release bundle ==="
& node "./node_modules/@tauri-apps/cli/tauri.js" build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$installer = Get-ChildItem "src-tauri\target\release\bundle\nsis\*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($installer) {
    Write-Host ""
    Write-Host "インストーラー: $($installer.FullName)"
    Write-Host "上記を実行して PKB をインストールしてください。"
    Write-Host "target\debug\pkb-desktop.exe を直接起動すると黒画面になります。"
} else {
    Write-Host "警告: NSIS インストーラーが見つかりませんでした" -ForegroundColor Yellow
}

exit 0
