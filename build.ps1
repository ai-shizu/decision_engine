# ============================================================================
# PKB ビルド & 実行スクリプト (Windows on ARM / Snapdragon X)
#   .\build.ps1          -> パイプライン実行 + C++ コンパイル + 検索デモ
#   .\build.ps1 -SkipPy  -> C++ コンパイルと検索デモのみ
# ============================================================================
param([switch]$SkipPy, [switch]$SkipDemo)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
Set-Location $root
New-Item -ItemType Directory -Force -Path "$root\build" | Out-Null

# ---- 1. Python パイプライン --------------------------------------------------
if (-not $SkipPy) {
    Write-Host "== [1/3] Python pipeline ==" -ForegroundColor Cyan
    $py = (Get-Command python -ErrorAction SilentlyContinue) ?? (Get-Command py -ErrorAction SilentlyContinue)
    if (-not $py) { throw "python が見つかりません" }
    & $py.Source "$root\src\python\core\pipeline.py"
    if ($LASTEXITCODE -ne 0) { throw "pipeline.py failed" }
}

# ---- 2. C++ コンパイル --------------------------------------------------------
Write-Host "== [2/3] C++ compile ==" -ForegroundColor Cyan
$src = "$root\src\cpp\search_engine.cpp"
$exe = "$root\build\search_engine.exe"

$clang = Get-Command clang++ -ErrorAction SilentlyContinue
if ($clang) {
    # Snapdragon X (ARMv8.7) — NEON は armv8-a+simd で有効化
    $args = @("-O3", "-std=c++17", "-march=armv8-a+simd", $src, "-o", $exe)
    Write-Host "clang++ $($args -join ' ') -fopenmp"
    & clang++ @args -fopenmp
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "OpenMP リンク失敗 → -fopenmp なしで再試行 (直列動作)"
        & clang++ @args
        if ($LASTEXITCODE -ne 0) { throw "clang++ compile failed" }
    }
} else {
    # フォールバック: MSVC (Developer PowerShell 必須)。/openmp 有効、NEON は ARM64 で既定
    $cl = Get-Command cl -ErrorAction SilentlyContinue
    if (-not $cl) { throw "clang++ も cl も見つかりません。LLVM (winget install LLVM.LLVM) を導入してください" }
    & cl /nologo /O2 /std:c++17 /openmp /EHsc $src /Fe:$exe /Fo:"$root\build\"
    if ($LASTEXITCODE -ne 0) { throw "cl compile failed" }
}
Write-Host "built: $exe"

# ---- 3. 検索デモ ---------------------------------------------------------------
Write-Host "== [3/3] search demo ==" -ForegroundColor Cyan
if (-not $SkipDemo) {
    & $exe "$root\data\processed\vectors.bin" "$root\data\processed\query.bin" 5
}
