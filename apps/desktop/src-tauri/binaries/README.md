# PKB Python Engine Binaries

Tauri `bundle.externalBin` 用の Python エンジン (PyInstaller) をここに配置します。

## 開発 (`dev.cmd`)

Debug ビルドでは **システム Python** で `src/python/run_engine.py` を起動します。  
同梱バイナリが無くても動作します。

環境変数 (任意):

- `PKB_PYTHON` — Python 実行ファイルパス
- `PKB_PROJECT_ROOT` — データルート (通常は自動解決)

## リリースバンドル

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-engine.ps1
```

| プラットフォーム | ファイル名 |
|---|---|
| Windows ARM64 | `pkb-engine-aarch64-pc-windows-msvc.exe` |
| Windows x64 | `pkb-engine-x86_64-pc-windows-msvc.exe` |

## 通信プロトコル

HTTP は使いません。起動時に stdout へ:

```json
{"event": "ready", "offline": true}
```

リクエスト/応答は stdin/stdout の JSON 1 行ずつ (`engine_stdio.py` 参照)。
