#!/usr/bin/env bash
# ============================================================================
# PKB Python Sidecar ビルド (macOS / Linux) — build-engine.ps1 の Unix 版
#
#   bash scripts/build-sidecar.sh
#
# やること:
#   1. Python 3.11+ を検出し、ビルド専用 venv を作成
#   2. ランタイム依存 (numpy) + PyInstaller を venv にインストール
#   3. run_engine.py (stdio JSON エンジン) を --onefile でバイナリ化
#   4. src-tauri/binaries/pkb-engine-<target-triple> へ配置
#      (Tauri externalBin は target triple サフィックスで自動解決する)
#
# 注意: PyInstaller はクロスコンパイル不可。aarch64 バイナリは Apple Silicon、
#       x86_64 バイナリは Intel Mac (または macos-13 ランナー) 上で実行すること。
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DESKTOP_ROOT="$(dirname "$SCRIPT_DIR")"
REPO_ROOT="$(cd "$DESKTOP_ROOT/../.." && pwd)"
RUN_ENGINE="$REPO_ROOT/src/python/run_engine.py"
REQUIREMENTS_LOCK="$DESKTOP_ROOT/requirements-sidecar.lock"
OUT_DIR="$DESKTOP_ROOT/src-tauri/binaries"
WORK_DIR="$DESKTOP_ROOT/build-engine"
VENV_DIR="$WORK_DIR/.venv-sidecar"

# ---- 1. Python 検出 (PKB_PYTHON > python3.12 > python3) --------------------
PYTHON="${PKB_PYTHON:-}"
if [ -z "$PYTHON" ]; then
    for c in python3.12 python3.13 python3.11 python3; do
        if command -v "$c" >/dev/null 2>&1; then
            PYTHON="$c"
            break
        fi
    done
fi
if [ -z "$PYTHON" ]; then
    echo "ERROR: python3 が見つかりません (brew install python@3.12)" >&2
    exit 1
fi
if [ "$($PYTHON -c 'import platform; print(platform.python_version())')" != "3.12.10" ]; then
    echo "ERROR: release sidecar requires Python 3.12.10 exactly" >&2
    exit 1
fi
echo "Python : $($PYTHON --version) ($(command -v "$PYTHON"))"
echo "Entry  : $RUN_ENGINE"

# ---- 2. venv + 依存 ---------------------------------------------------------
mkdir -p "$WORK_DIR"
if [ ! -x "$VENV_DIR/bin/python" ]; then
    "$PYTHON" -m venv "$VENV_DIR"
fi
VPY="$VENV_DIR/bin/python"
# Runtime and build tools are accepted only from the hash-locked wheel set.
"$VPY" -m pip install --quiet --disable-pip-version-check \
    --only-binary=:all: --require-hashes -r "$REQUIREMENTS_LOCK"

# ---- 3. PyInstaller ---------------------------------------------------------
"$VPY" -m PyInstaller \
    --onefile \
    --name pkb-engine \
    --distpath "$OUT_DIR" \
    --workpath "$WORK_DIR" \
    --specpath "$WORK_DIR" \
    --paths "$REPO_ROOT/src/python" \
    --hidden-import engine_stdio \
    --hidden-import core.facade \
    --hidden-import core.paths \
    --hidden-import core.settings_api \
    --hidden-import core.profile_store \
    --hidden-import numpy \
    --collect-submodules core \
    --collect-data core \
    --noconfirm \
    "$RUN_ENGINE"

# ---- 4. target triple へリネーム --------------------------------------------
if command -v rustc >/dev/null 2>&1; then
    TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
else
    ARCH="$(uname -m)"
    case "$ARCH" in
        arm64|aarch64) ARCH="aarch64" ;;
        x86_64) ARCH="x86_64" ;;
        *) echo "ERROR: 未対応アーキテクチャ: $ARCH" >&2; exit 1 ;;
    esac
    case "$(uname -s)" in
        Darwin) TRIPLE="$ARCH-apple-darwin" ;;
        Linux) TRIPLE="$ARCH-unknown-linux-gnu" ;;
        *) echo "ERROR: 未対応 OS: $(uname -s)" >&2; exit 1 ;;
    esac
fi

BUILT="$OUT_DIR/pkb-engine"
DEST="$OUT_DIR/pkb-engine-$TRIPLE"
if [ ! -f "$BUILT" ]; then
    echo "ERROR: PyInstaller の出力が見つかりません: $BUILT" >&2
    exit 1
fi
cp -f "$BUILT" "$DEST"
chmod +x "$DEST"

# A macOS helper must inherit App Sandbox and carry no network entitlement.
if [ "$(uname -s)" = "Darwin" ]; then
    ENTITLEMENTS="$DESKTOP_ROOT/src-tauri/sidecar-entitlements.plist"
    SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:--}"
    codesign --force --options runtime --timestamp=none \
        --sign "$SIGNING_IDENTITY" \
        --entitlements "$ENTITLEMENTS" \
        "$DEST"
    codesign --verify --strict --verbose=2 "$DEST"
    ACTUAL_ENTITLEMENTS="$(codesign -d --entitlements :- "$DEST" 2>&1)"
    printf '%s\n' "$ACTUAL_ENTITLEMENTS" | grep -q 'com.apple.security.app-sandbox'
    printf '%s\n' "$ACTUAL_ENTITLEMENTS" | grep -q 'com.apple.security.inherit'
    if printf '%s\n' "$ACTUAL_ENTITLEMENTS" | grep -q 'com.apple.security.network.client'; then
        echo "ERROR: sidecar has forbidden network client entitlement" >&2
        exit 1
    fi
    if printf '%s\n' "$ACTUAL_ENTITLEMENTS" | grep -q 'com.apple.security.network.server'; then
        echo "ERROR: sidecar has forbidden network server entitlement" >&2
        exit 1
    fi
fi

SIZE_BYTES="$(wc -c < "$DEST" | tr -d ' ')"
if [ "$SIZE_BYTES" -lt 1048576 ]; then
    echo "ERROR: エンジンバイナリが小さすぎます ($SIZE_BYTES bytes)" >&2
    exit 1
fi

# アーキテクチャの自己検証 (クロスコンパイル事故の早期検出)
if command -v file >/dev/null 2>&1; then
    file "$DEST"
fi
echo "Engine ready: $DEST ($((SIZE_BYTES / 1048576)) MB)"
