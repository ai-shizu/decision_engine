# PKB — Claude Code 引き継ぎ書

> **更新:** 2026-07-06  
> **対象:** Snapdragon X / Windows on ARM64、Python 3.12-arm64  
> **リポジトリ:** `C:\Users\badger\Documents\cursur\decision_engine`  
> **最新コミット:** `6d1dec9` — `feat: Phase B refactoring and Tauri desktop integration`

---

## 0. 最初に読むもの

| ファイル | 内容 |
|----------|------|
| 本書 (`docs/HANDOFF.md`) | 引き継ぎ・作業の入口 |
| `docs/CONTEXT.md` | 技術仕様・DailyContext・profiler・カレンダー同期の詳細 |
| `README.md` | ユーザー向けクイックスタート・ディレクトリ構造 |
| `apps/desktop/README.md` | デスクトップ開発・ビルド手順 |

**設計の核:** 完全オフライン、外部 API 不使用、記録と相談の分離、手動インポートのみ。

---

## 1. プロジェクト概要

**PKB (Personal Knowledge Base)** — 日記・LINE・予定・家計簿・AI 相談履歴を **DailyContext**（1 日 = 1 ベクトル）に統合し、C++ NEON ベクトル検索 + ローカル LLM (llama-server @ 127.0.0.1) で意思決定を支援する。

| レイヤ | 技術 | 役割 |
|--------|------|------|
| デスクトップ UI | Tauri v2 + React | **主軸** — NSIS インストーラー配布 |
| IPC | Rust `EngineManager` + Python stdio JSON | UI ↔ エンジン（HTTP なし） |
| Python コア | `src/python/core/` | UI 非依存ビジネスロジック |
| 検索 | `build/search_engine.exe` (C++17, PKBVEC01) | 384 次元 AoSoA Top-K |
| TUI | Textual (`ui_tui/app.py`) | 開発・デバッグ用 |
| LLM | `tools/llama-arm64/llama-server.exe` + GGUF | 相談・profiler 時のみ起動 |

---

## 2. アーキテクチャ（フェーズ B 完了）

```
React (apps/desktop/src/)
  │  engine.ts → invoke("pkb_invoke", { cmd, params })
  ▼
Rust (src-tauri/src/engine.rs) — EngineManager
  │  子プロセス: python run_engine.py  (dev) / pkb-engine-*.exe (release)
  │  stdin/stdout: JSON 1 行ずつ
  ▼
engine_stdio.py — dispatch(cmd, params)
  ▼
core/facade.py — load_record, save_record, consult, import_line, sync_calendar, ...
  ▼
core/consultation_engine.py → search_engine.exe + llama-server
```

**TUI** は stdio を経由せず `core/facade.py` を直接 import。

### Stdio プロトコル

起動直後 stdout:
```json
{"event": "ready", "offline": true}
```

リクエスト (stdin):
```json
{"id": 1, "cmd": "record.load", "params": {"date": "2026-07-06"}}
```

応答 (stdout):
```json
{"id": 1, "ok": true, "result": {...}}
```

| cmd | 説明 |
|-----|------|
| `health` | 生存確認 |
| `record.load` / `record.save` | 予定・家計簿・日記 |
| `calendar.event_dates` | カレンダー `*` マーク用 |
| `calendar.sync` | ICS / Apple (`source`, `mode`, `ics_content` 等) |
| `import.line` | LINE 取込 → **profiler 自動実行** |
| `consult` | 相談（検索 + LLM） |
| `settings.get` / `settings.save_fixed` | 基本情報 6 項目 |
| `settings.run_profiler` | 深層プロファイル再分析 |
| `shutdown` | エンジン停止 |

実装: `src/python/engine_stdio.py`  
Rust 側: `apps/desktop/src-tauri/src/engine.rs`, `commands.rs`

---

## 3. ディレクトリマップ（重要パスのみ）

```
decision_engine/
├── apps/desktop/              # Tauri + React（主軸 UI）
│   ├── src/components/        # RecordTab, ImportTab, ConsultTab, SettingsTab
│   ├── src/lib/engine.ts      # pkb_invoke ラッパー
│   ├── src-tauri/src/         # engine.rs, paths.rs, commands.rs
│   ├── dev.cmd / build.cmd
│   └── scripts/build-engine.ps1  # PyInstaller → binaries/pkb-engine-*.exe
│
├── src/python/
│   ├── core/                  # ★ ビジネスロジック（ここを触る）
│   │   ├── facade.py          # TUI / stdio 共通 API
│   │   ├── consultation_engine.py
│   │   ├── pipeline.py, data_merger.py, profiler.py
│   │   ├── calendar_*.py, finance_manager.py, settings_api.py
│   │   └── paths.py           # PROJECT_ROOT, データパス
│   ├── ui_tui/app.py          # Textual 4 タブ
│   ├── engine_stdio.py        # デスクトップ IPC
│   └── run_engine.py          # エンジンエントリ
│
├── src/ui/app.py              # TUI 互換ラッパー（中身は ui_tui へ委譲）
├── src/cpp/search_engine.cpp
├── data/raw/                  # gitignore — 個人データ
├── data/processed/            # gitignore — vectors.bin, プロファイル
├── data/knowledge/            # 外部知識 Markdown（コミット可）
├── build/search_engine.exe    # gitignore
├── models/*.gguf              # gitignore
└── tools/llama-arm64/         # gitignore
```

---

## 4. 環境変数

| 変数 | 用途 |
|------|------|
| `PKB_PROJECT_ROOT` | データルート（未設定時 = リポジトリルート / 本番 = `%LOCALAPPDATA%\PKB`） |
| `PKB_PYTHON` | 開発時 Python 実行ファイル |
| `PKB_MODELS_DIR` | GGUF 配置 |
| `PKB_LLAMA_DIR` | llama-server 配置 |
| `PKB_LLAMA_THREADS/CTX/BATCH`, `PKB_LLM_PORT` | LLM チューニング |
| `HF_HUB_OFFLINE=1` | 埋め込みモデルオフライン運用 |

---

## 5. 開発コマンド

```powershell
cd C:\Users\badger\Documents\cursur\decision_engine

# ── デスクトップ（推奨）──
cd apps\desktop
.\dev.cmd                    # ★ pkb-desktop.exe 直接起動は NG（黒画面）

# ── TUI ──
python src\python\ui_tui\app.py
python src\ui\app.py         # 互換ラッパー

# ── パイプライン / C++ ──
.\build.ps1                  # pipeline + search_engine.exe ビルド
python src\python\core\pipeline.py
python src\python\core\profiler.py --no-llm

# ── テスト ──
python tests\ui_smoke.py
python tests\test_calendar_sync.py
python tests\test_apple_calendar_sync.py
python tests\benchmark.py --quick

# ── リリース ──
cd apps\desktop
.\build.cmd                  # → src-tauri\target\release\bundle\nsis\PKB_*-setup.exe
```

**前提:** Python 3.12 (ARM64 推奨), Node 20+, Rust stable, clang++ + OpenMP または MSVC。

---

## 6. 設計原則（破ってはいけないもの）

| 原則 | 挙動 |
|------|------|
| 記録と相談の分離 | RECORD / カレンダー同期 → `sync_diary_index()` のみ |
| 記録と分析の分離 | RECORD 保存は profiler **走らない** |
| LINE 取込のみ例外 | IMPORT LINE → profiler 自動実行 |
| 手動カレンダー | ICS / Apple — クラウド API・自動同期なし |
| 遅延初期化 | 埋め込み・llama-server は初回 consult / profiler まで |
| プロファイル二層 | `fixed_attributes`（手入力）/ `inferred_profile`（自動・読取専用） |
| UI とコア分離 | ビジネスロジックは `core/` のみ。UI は `facade` または stdio 経由 |

---

## 7. 直近まで完了した作業（2026-07-06）

- [x] Python コアを `src/python/core/` へ分離
- [x] `core/facade.py` — TUI / デスクトップ共通 API
- [x] TUI を `src/python/ui_tui/` へ移行（`src/ui/app.py` はラッパー）
- [x] `engine_stdio.py` + Rust `EngineManager` — stdio JSON IPC
- [x] Tauri v2 + React 4 タブ UI (`apps/desktop/`)
- [x] `README.md` / `docs/CONTEXT.md` を現行構成に同期
- [x] `.gitignore` 整備（node_modules, target, data/, models, logs）
- [x] コミット `6d1dec9` — Phase B + デスクトップ統合

---

## 8. 次に着手すべきタスク（優先度順）

### 高 — 機能ギャップ

1. **fixed_attributes を profiler LLM プロンプトに注入**（上書きはしない。分析文脈のみ）  
   対象: `core/profiler.py`

2. **CONSULT streaming 表示**（デスクトップ）  
   現状: stdio は一括応答。status コールバックを IPC に載せる必要あり  
   対象: `engine_stdio.py`, `engine.rs`, `ConsultTab.tsx`

3. **カレンダーマーク拡張** — 家計簿入力日・相談日も `*` 表示  
   対象: `facade.calendar_event_dates()`, `RecordTab.tsx`, TUI カレンダー

### 中 — UX / 品質

4. SETTINGS 基本情報保存後の profiler 再実行（任意・要望次第）
5. `awaiting_user` セッションの UI 表示（`data_merger`）
6. CI: `ui_smoke.py` + `boost_check.py` + `benchmark.py --quick`

### 低 — バックログ

7. Apple カレンダー DB スキーマ追従（ICS フォールバック維持）
8. 予定密度 × profiler 共起分析

---

## 9. 既知の落とし穴

| 問題 | 対処 |
|------|------|
| `pkb-desktop.exe` 直接起動 → 黒画面 | 必ず `apps/desktop/dev.cmd` |
| Windows cp932 と JSON | `engine_stdio.py` は UTF-8 専用 stdout + `text_utils.sanitize_obj` |
| Textual DuplicateIds | カレンダー日ボタンに **id を付けない** |
| 7B 初回ロード ~2–3 分 | `llm_config.model_startup_timeout()` 参照 |
| Apple カレンダー | macOS のみ。Windows デスクトップ/TUI ではボタン無効 |
| Git author（Cursor 環境） | サンドボックスで `git config` が空のことがある → 環境変数 `GIT_AUTHOR_*` で回避 |
| `search_engine.exe` 未ビルド | `.\build.ps1` または benchmark/consult 前にビルド |

---

## 10. データファイル形式（クイックリファレンス）

**calendar.json**
```json
{"2026-07-06": [{"time": "09:00", "title": "会議"}, ...]}
```

**user_profile.json** — schema `user_profile.v2`  
- `fixed_attributes`: age, gender, height, weight, address, occupation  
- `inferred_profile`: profiler 自動生成（UI 読取専用）

**vectors.bin** — magic `PKBVEC01`, 384 dim, AoSoA 4-lane（Python `pipeline.py` と C++ 完全同期）

---

## 11. テスト方針

- **ui_smoke.py** — Textual ヘッドレス。facade をモック。UI 配線のみ。
- **test_calendar_sync.py** — ICS パース・マージ（stdlib のみ）
- **benchmark.py** — C++ QPS（`--quick` で短縮）

変更後の最低確認:
```powershell
python tests\test_calendar_sync.py
python tests\ui_smoke.py
```

---

## 12. コーディング規約（このリポジトリ）

- **最小 diff** — 依頼外のリファクタ禁止
- **core/ に UI 依存を入れない**
- 新 UI 機能 → まず `facade.py` に API を足す → `engine_stdio.py` dispatch → React `engine.ts`
- コメントは非自明なビジネスロジックのみ
- コミットはユーザー明示指示時のみ（メッセージは why 重視）
- `data/raw/`, `models/`, `logs/` はコミットしない

---

## 13. Git 履歴（参考）

```
6d1dec9 feat: Phase B refactoring and Tauri desktop integration
db3bebe Update README and CONTEXT for calendar sync.
c57a139 Initial commit: PKB offline decision engine prototype
```

push 前確認: `git status -sb`

---

## 14. Claude Code への最初のプロンプト例

```
docs/HANDOFF.md と docs/CONTEXT.md を読んで PKB の文脈を把握してください。
次のタスク: [例: profiler に fixed_attributes をプロンプト注入]
変更は core/ と必要最小限の UI のみ。完了後 tests/test_calendar_sync.py と tests/ui_smoke.py を実行。
```

---

*この引き継ぎ書は 2026-07-06 時点のローカル main (`6d1dec9`) に基づく。*
