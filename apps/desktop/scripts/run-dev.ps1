$ErrorActionPreference = "Stop"
. "$PSScriptRoot\ensure-node-path.ps1"
Set-Location (Join-Path $PSScriptRoot "..")
& node "./node_modules/vite/bin/vite.js"
