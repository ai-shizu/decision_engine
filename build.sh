#!/usr/bin/env bash
# ============================================================================
# PKB ビルド & 実行スクリプト (macOS Apple Silicon / Intel, Linux)
#   bash build.sh            -> パイプライン実行 + C++ コンパイル + 検索デモ
#   bash build.sh --skip-py  -> C++ コンパイルと検索デモのみ
#
# Windows は build.ps1 を使うこと。
# 出力: build/search_engine (拡張子なし — core/paths.py の SEARCH_EXE と一致)
# ============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"
mkdir -p "$ROOT/build"

SKIP_PY=0
for arg in "$@"; do
    case "$arg" in
        --skip-py|-SkipPy) SKIP_PY=1 ;;
    esac
done

# ---- 1. Python パイプライン -------------------------------------------------
if [ "$SKIP_PY" -eq 0 ]; then
    echo "== [1/3] Python pipeline =="
    PYTHON="${PKB_PYTHON:-python3}"
    "$PYTHON" "$ROOT/src/python/core/pipeline.py"
fi

# ---- 2. C++ コンパイル --------------------------------------------------------
echo "== [2/3] C++ compile =="
SRC="$ROOT/src/cpp/search_engine.cpp"
EXE="$ROOT/build/search_engine"

if ! command -v clang++ >/dev/null 2>&1; then
    echo "ERROR: clang++ が見つかりません (xcode-select --install)" >&2
    exit 1
fi

OS="$(uname -s)"
ARCH="$(uname -m)"
CXXFLAGS=(-O3 -std=c++17)

# アーキテクチャ別 SIMD 分岐:
#   aarch64/arm64: NEON は AArch64 の必須機能。__ARM_NEON はコンパイラが自動定義
#                  するため search_engine.cpp の NEON パスが有効になる。
#   x86_64:        NEON パスは無いためスカラー + 自動ベクトル化。
#                  macOS 11 (minimumSystemVersion) の対応機種は Haswell (2013+)
#                  以降なので AVX2/FMA (x86-64-v3) を有効化できる。
#                  Linux は古い HW も想定し SSE4.2 世代 (x86-64-v2) に留める。
case "$ARCH" in
    arm64|aarch64)
        if [ "$OS" = "Linux" ]; then
            CXXFLAGS+=(-march=armv8-a+simd)
        fi
        # macOS arm64: デフォルトターゲットで NEON 有効。-march 指定は不要
        ;;
    x86_64)
        if [ "$OS" = "Darwin" ]; then
            CXXFLAGS+=(-march=x86-64-v3)  # AVX2 + FMA (Intel Mac 2013+)
        else
            CXXFLAGS+=(-march=x86-64-v2)  # SSE4.2 世代以降 (2009+)
        fi
        ;;
    *)
        echo "WARNING: 未知のアーキテクチャ $ARCH — 汎用フラグでビルド" >&2
        ;;
esac

# OpenMP:
#   Linux clang/gcc: -fopenmp がそのまま通る。
#   macOS Apple clang: libomp が別配布 (brew install libomp) のため
#   -Xpreprocessor -fopenmp + ヘッダ/ライブラリパス指定が必要。
#   失敗時は直列動作でフォールバック (build.ps1 と同じ方針)。
build_with_openmp() {
    if [ "$OS" = "Darwin" ]; then
        local OMP_PREFIX
        OMP_PREFIX="$(brew --prefix libomp 2>/dev/null || true)"
        if [ -n "$OMP_PREFIX" ] && [ -d "$OMP_PREFIX/include" ]; then
            echo "clang++ ${CXXFLAGS[*]} -Xpreprocessor -fopenmp (libomp: $OMP_PREFIX)"
            clang++ "${CXXFLAGS[@]}" \
                -Xpreprocessor -fopenmp \
                -I"$OMP_PREFIX/include" -L"$OMP_PREFIX/lib" -lomp \
                "$SRC" -o "$EXE"
            return $?
        fi
        return 1
    fi
    echo "clang++ ${CXXFLAGS[*]} -fopenmp"
    clang++ "${CXXFLAGS[@]}" -fopenmp "$SRC" -o "$EXE"
}

if ! build_with_openmp; then
    echo "WARNING: OpenMP リンク失敗 → -fopenmp なしで再試行 (直列動作)" >&2
    echo "         (macOS: brew install libomp で並列化が有効になります)" >&2
    clang++ "${CXXFLAGS[@]}" "$SRC" -o "$EXE"
fi
echo "built: $EXE"

# ---- 3. 検索デモ ---------------------------------------------------------------
echo "== [3/3] search demo =="
if [ -f "$ROOT/data/processed/vectors.bin" ] && [ -f "$ROOT/data/processed/query.bin" ]; then
    "$EXE" "$ROOT/data/processed/vectors.bin" "$ROOT/data/processed/query.bin" 5
else
    echo "(vectors.bin / query.bin 未生成のためデモをスキップ — bash build.sh で pipeline を先に実行)"
fi
