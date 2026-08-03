# Coraxis Desktop (Tauri v2)

Chrome と同様、**インストーラーをダウンロードして PC にインストールする**デスクトップアプリです。  
外部ネットワークは使用せず、Rust ↔ Python を **stdio JSON** で直接通信します（HTTP サーバーなし）。

## 配布モデル

| 項目 | 内容 |
|---|---|
| 配布物 | Windows: `PKB_x.x.x_aarch64-setup.exe` (NSIS) |
| 同梱 | Tauri シェル + Python エンジン (`pkb-engine-*.exe`) |
| 通信 | stdin/stdout JSON（完全オフライン・プロセス内 IPC） |
| データ | リポジトリ `data/` または `%LOCALAPPDATA%\PKB\` |

## 前提 (開発者)

- Node.js 20+
- Rust (stable)
- Python 3.12+（開発時のみ。リリース版は同梱エンジンを使用）

```powershell
cd apps\desktop
& "C:\Program Files\nodejs\npm.cmd" install
```

PowerShell の実行ポリシーで `npm` が使えない場合は `.cmd` を使ってください。

## 開発

```powershell
cd apps\desktop
.\dev.cmd
```

**重要:** `target\debug\pkb-desktop.exe` を直接起動しないでください（黒画面になります）。

## リリースビルド

```powershell
cd apps\desktop
.\build.cmd
```

成果物: `src-tauri\target\release\bundle\nsis\PKB_*-setup.exe`

エンジンのみ再ビルド:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-engine.ps1
```

## UI ↔ エンジン

フロントエンドは `src/lib/engine.ts` 経由で Tauri コマンド `pkb_invoke` を呼び出します。

詳細: `src-tauri/binaries/README.md`
