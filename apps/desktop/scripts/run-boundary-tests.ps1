#Requires -Version 5.1
param(
    [Parameter(Mandatory = $false)]
    [string]$Python
)

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\ensure-node-path.ps1"
Set-Location (Join-Path $PSScriptRoot "..")

function Resolve-PkbPython {
    param([string]$Explicit)
    if ($Explicit) {
        return $Explicit
    }
    if ($env:PKB_PYTHON) {
        return $env:PKB_PYTHON
    }
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\Python\Python312-arm64\python.exe"),
        (Join-Path $env:LOCALAPPDATA "Programs\Python\Python312\python.exe")
    )
    foreach ($c in $candidates) {
        if ($c -and (Test-Path -LiteralPath $c)) {
            return $c
        }
    }
    $cmd = Get-Command python -ErrorAction SilentlyContinue
    if ($cmd -and $cmd.Source) {
        return $cmd.Source
    }
    return $null
}

$OutDir = ".boundary-tests-out"
$overallFailed = $false

try {
    $PythonExe = Resolve-PkbPython -Explicit $Python
    if (-not $PythonExe -or -not (Test-Path -LiteralPath $PythonExe)) {
        throw "Python not found. Set PKB_PYTHON or pass -Python <path> to run-boundary-tests.ps1."
    }

    if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

    Write-Host "-- generating golden fixtures --"
    $fixtureGens = Get-ChildItem -Path "tests-runtime" -Filter "gen_*_fixture.py" |
        Sort-Object Name
    foreach ($gen in $fixtureGens) {
        $outName = if ($gen.BaseName -eq "gen_manifest_fixture") {
            "golden.json"
        } elseif ($gen.BaseName -eq "gen_tensor_report_fixture") {
            "tensor_golden.json"
        } else {
            ($gen.BaseName -replace "^gen_", "" -replace "_fixture$", "") + ".json"
        }
        & $PythonExe $gen.FullName (Join-Path $OutDir $outName)
        if ($LASTEXITCODE -ne 0) { throw "$($gen.Name) failed" }
    }

    Write-Host "-- compiling boundary suite (tsconfig.boundary.json) --"
    & node "./node_modules/typescript/bin/tsc" -p tsconfig.boundary.json
    if ($LASTEXITCODE -ne 0) { throw "tsc -p tsconfig.boundary.json failed" }

    # Compiled output is CommonJS but package.json declares "type": "module" —
    # a scoped package.json inside the compiled output directory forces Node
    # to treat these .js files as CommonJS without touching the real package files.
    Set-Content -Path "$OutDir/package.json" -Value '{"type":"commonjs"}' -Encoding utf8

    $suiteDir = Join-Path $OutDir "tests-runtime"
    $suites = @()
    if (Test-Path -LiteralPath $suiteDir) {
        $suites = @(Get-ChildItem -Path $suiteDir -Filter "*.test.js" | Sort-Object Name)
    }
    if ($suites.Count -eq 0) {
        throw "no boundary test suites compiled under $suiteDir (*.test.js)"
    }

    foreach ($suite in $suites) {
        Write-Host "-- running $($suite.Name) --"
        & node $suite.FullName
        if ($LASTEXITCODE -ne 0) { $overallFailed = $true }
    }
}
finally {
    if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
}

if ($overallFailed) {
    Write-Host "BOUNDARY_EXIT=1"
    exit 1
}
Write-Host "BOUNDARY_EXIT=0"
exit 0
