# PKB プロジェクト — コンテキスト引継ぎ (2026-07-05 更新)

> 新規チャットセッションの AI が瞬時に開発再開できるための高密度サマリ。  
> 対象環境: **Snapdragon X / Windows on ARM64**, **Python 3.12-arm64**, **llvm-mingw clang++**。

---

## 1. プロジェクト目的と全体アーキテクチャ

### 目的
**PKB (Personal Knowledge Base)** — 完全オフライン・ローカル完結型の自己エミュレーション・意思決定支援システム。

- 日記・LINE・予定・家計簿・AI相談履歴を統合し、行動ログから深層プロファイルを自動生成
- **CONSULT 送信時のみ** C++ NEON ベクトル検索 + 7B ローカル LLM で 4 セクション回答
- 外部 API 不使用 (`HF_HUB_OFFLINE=1`, llama-server は 127.0.0.1 のみ)

### 二層 + 推論パイプライン

```
┌──────────────────────────────────────────────────────────────┐
│  UI / オーケストレーション (Python 3.12 + Textual 8.x)         │
│  src/ui/app.py              — 4タブ TUI                       │
│  src/python/consultation_engine.py — 相談時のみ起動           │
│  src/python/pipeline.py       — ベクトル化                     │
│  src/python/data_merger.py    — DailyContext 結晶化            │
│  src/python/profiler.py       — 深層プロファイル (手動/IMPORT)  │
│  src/python/calendar_manager.py / finance_manager.py / ...     │
│  src/python/calendar_sync.py      — ICS 取込 (stdlib のみ)      │
│  src/python/apple_calendar_sync.py — macOS ローカル DB 取込     │
└────────────────────────┬─────────────────────────────────────┘
                         │ subprocess / mmap / JSON
┌────────────────────────▼─────────────────────────────────────┐
│  検索コア (C++17, ARM NEON + OpenMP)                            │
│  src/cpp/search_engine.cpp → build/search_engine.exe           │
│  AoSoA 384-dim 内積, ローカル Top-K マージ, Windows mmap      │
└────────────────────────┬─────────────────────────────────────┘
                         │ HTTP 127.0.0.1 (相談/プロファイラ時のみ)
┌────────────────────────▼─────────────────────────────────────┐
│  推論 (llama.cpp b9870 ARM64 native)                            │
│  tools/llama-arm64/llama-server.exe                             │
│  models/Qwen2.5-7B-Instruct-Q4_K_M.gguf (優先, 4.68GB)         │
└─────────────────────────────────────────────────────────────────┘
```

### 設計原則
| 原則 | 内容 |
|---|---|
| **記録と相談の分離** | RECORD / カレンダー同期 → `sync_diary_index()` のみ。CONSULT → 検索+LLM |
| **記録と分析の分離** | RECORD / カレンダー同期は profiler 非実行。IMPORT の LINE 取込のみ profiler 自動実行 |
| **手動カレンダー取込** | ICS / Apple は人間がトリガー。クラウド API・バックグラウンド同期なし |
| **完全遅延初期化** | 埋め込み・llama-server は初回 `consult()` / profiler 実行まで起動しない |
| **インデックス mtime 同期** | ソース JSON/md/txt の mtime が古い時のみ `vectors.bin` 再構築 |
| **プロファイル二層** | `fixed_attributes` (手入力・保持) / `inferred_profile` (自動・読取専用) |
| **ゾンビ禁止** | `LlamaServerBackend.stop()` Terminate→Kill + `atexit` + TUI `on_unmount` |

---

## 2. コア技術仕様

### 2.1 DailyContext (1日 = 1ベクトルチャンク)

**生成**: `data_merger.load_daily_contexts()` — 以下を日付キーで結合:

| ソース | ファイル | セクション |
|---|---|---|
| 予定 | `data/raw/calendar.json` | `## Calendar` |
| 家計簿 | `data/raw/finance.json` | `## Finance (家計簿)` |
| 日記 | `data/raw/diary.md` | `## Diary` |
| AI相談 | `data/raw/ai_consultations.json` | `## AI_Consultations` |
| LINE | `data/raw/line_history.txt` | `## LINE_ConversationSessions` |

**主要 dict フィールド**: `date`, `title`, `text`, `self_text`, `diary_text`, `line_self_text`, `calendar_events`, `finance_text`, `consultation_text`, `conversation_sessions[]`, `has_*` フラグ群。

**ベクトル化**: `pipeline.build_index()` → `data/processed/vectors.bin` + `metadata.json`  
- 384次元 L2正規化 (`paraphrase-multilingual-MiniLM-L12-v2` / オフライン時 hashed n-gram fallback)

### 2.2 ConversationSession (状態保持型)

**生成**: `data_merger.extract_conversation_sessions()` — 単純 Stimulus→Response ペアは廃止。

| ルール | 内容 |
|---|---|
| 開始 | 相手発言でセッション開始 |
| 継続 | ユーザー初回返信までクローズしない (24h超可) |
| 確定 | ユーザー返信後 **30分** 空白で1セッション確定 |
| 未返信 | `awaiting_user=True` は確定しない |

**Response Latency** = 相手最終発言 → ユーザー初回返信。profiler のレイテンシ分析・矛盾検出に使用。

### 2.3 C++ AoSoA バイナリ (Python/C++ 完全同期)

```
FileHeader (32B): magic "PKBVEC01", dim=384, lanes=4, num_vectors, num_blocks, block_bytes=6160
VectorBlock (6160B): float data[384][4] + int32 chunk_ids[4]  (パディング=-1)
```

- CLI: `build/search_engine.exe vectors.bin query.bin [top_k]`
- 内蔵ベンチ: 100 iter avg → `latency: X us/query`
- `consultation_engine.search_daily()`: 日記+LINE両方の日は **ranking_score × 1.15**

**ベンチマーク**: `tests/benchmark.py` — AoSoA ダミーデータ生成 + QPS計測 (`--quick` 可)

### 2.4 プロファイリング (`profiler.py` → `deep_profile.v5`)

| レイヤー | 内容 |
|---|---|
| ルールベース | 認知バイアス, 価値観階層, 感情パターン, 意思決定ルール, cross_source, interaction_sessions |
| factual_signals | ログから推定 (職業文脈, 健康シグナル等) — **手入力は使わない** |
| abstract_identity | コアドライブ, 認知スタイル, 内的葛藤, 抽象ヒューリスティック, 対人スタンス |
| meta_narrative | 上記を2〜3文に統合 |
| llm_interaction_insights | 7B 深層分析 (セクション7: 抽象自己モデル含む) + 家計簿×感情相関 |

**user_profile.json (`user_profile.v2`)**:
```json
{
  "schema": "user_profile.v2",
  "fixed_attributes": { "age", "gender", "height", "weight", "address", "occupation" },
  "inferred_profile": { "meta_narrative", "factual_signals", "abstract_identity", "llm_deep_synthesis" },
  "auto_extracted": { "value_hierarchy", "dominant_biases", "abstract_heuristics", ... }
}
```

**重要**: `update_user_profile()` は `fixed_attributes` を**保持するのみ**。profiler の分析入力には未使用。CONSULT プロンプトでは `fixed_attributes` を「基本情報 (本人入力・固定)」として注入。

### 2.5 llm_config.py

```python
PREFERRED_7B = models/Qwen2.5-7B-Instruct-Q4_K_M.gguf
MIN_7B_BYTES = 4_000_000_000   # 破損検出 → 1.5B フォールバック

find_gguf()              # 7B優先
llama_server_cmd()       # --ctx-size 8192 --threads 8 --batch-size 512 --no-webui
model_startup_timeout()  # 7B: 300s

# 環境変数: PKB_LLAMA_THREADS, PKB_LLAMA_CTX, PKB_LLAMA_BATCH, PKB_LLM_PORT, PKB_MODELS_DIR, PKB_LLAMA_DIR
```

### 2.6 相談パイプライン (`consultation_engine.py`)

```
consult(query):
  1. embed(query)
  2. sync_diary_index() / sync_knowledge_index()   # mtime チェック
  3. search_daily(qvec) + search_index(knowledge)  # NEON Top-K
  4. build_prompt:
       - 基本情報 (fixed_attributes)
       - 推定プロフィール (inferred_profile)
       - 深層プロファイル (deep_profile.json 要約)
       - 過去 DailyContext ヒット
       - 外部知識ヒット
       - Future Context (calendar.json 30日先)
       - 相談 + 4セクション出力指示
  5. LlamaServerBackend.generate()
  6. consultation_log 保存 → sync_diary_index(force=True)
```

### 2.7 TUI (`src/ui/app.py`)

| タブ | 内容 |
|---|---|
| **RECORD** | 週/月カレンダー + 日付バナー + 縦スクロール領域 + **[予定\|家計簿\|日記]** サブタブ + 保存ボタン (Ctrl+S) |
| **IMPORT** | LINE `.txt` ドロップ → `line_history.txt` 追記 → **profiler 自動実行** / **ICS ファイル選択** (tkinter) / **Apple カレンダー同期** (macOS のみ) |
| **CONSULT** | フル幅チャットのみ (サイドバーなし) |
| **SETTINGS** | 基本情報6項目 (手入力・保存) + 自動プロフィール (読取専用) + 再分析(profiler) |

**RECORD サブタブ詳細**:
- **予定**: 時刻ピッカー (時/分を ▲数字▼ で縦ロール) + 内容 + 追加 → `calendar.json`
- **家計簿**: 支出/収入を上下2行フォーム → `finance.json`
- **日記**: TextArea → `diary.md` (日付単位)

**UI レイアウト (2026-07-05)**:
- カレンダー: 上部固定 + グリッド横スクロール (`HorizontalScroll`)
- サブタブ内容: `#record-scroll` 内で縦スクロール
- 保存ボタン: スクロール外・下部固定

**IMPORT タブ詳細**:
- **LINE**: 公式エクスポート `.txt` を Input に D&D または Paste → 追記 → profiler ワーカー
- **Google (ICS)**: 手動エクスポート `.ics` → 「ICSファイルを同期」→ ネイティブファイルダイアログ (tkinter, オフライン)
- **Apple**: macOS ローカル SQLite (`CalendarItem` / `Event`) を読取。Windows ではボタン無効 + ヒント表示
- **マージモード**: `#ics-merge-mode` Select — **追記** (`append`) / **上書き** (`overwrite`)。ICS・Apple **共通**
- **取込後 (ICS/Apple)**: `calendar.json` 更新 → `sync_diary_index(force=True)` のみ。**profiler は走らない**
- **ICS アーカイブ**: 選択ファイルを `data/raw/calendar_import.ics` にコピー保存

**ウィジェット**:
- `src/ui/calendar_widget.py` — 月/週切替, 予定マーク `*`, `DateSelected` メッセージ (day button は **id なし**, `name=date` で DuplicateIds 回避)
- `src/ui/time_picker.py` — `VerticalRollColumn` + `TimePicker`

### 2.8 カレンダー同期 (`calendar_sync.py` / `apple_calendar_sync.py`)

**共通エントリポイント**: `apply_calendar_import(imported, mode, source=...)`  
→ `calendar_manager.load_calendar()` / `save_calendar()` → `data/raw/calendar.json`

**calendar.json 形式**:
```json
{"YYYY-MM-DD": [{"time": "HH:MM", "title": "..."}, ...]}
```

| モジュール | 入力 | 主 API |
|---|---|---|
| `calendar_sync.py` | 手動エクスポート `.ics` | `parse_ics()`, `merge_calendar()`, `sync_from_ics()` |
| `apple_calendar_sync.py` | macOS SQLite DB | `find_calendar_databases()`, `parse_apple_calendars()`, `sync_from_apple_calendar()` |

**ICS パース** (stdlib のみ):
- RFC 5545 line folding 展開
- `DTSTART` (TZID / UTC `Z` / 終日 `VALUE=DATE`) → ローカル `(date, time)`
- `SUMMARY` → `title`。同一日内は `time` 昇順ソート

**マージ** (`merge_calendar`):
| モード | 動作 |
|---|---|
| `append` | 同一 `(time, title)` は重複排除。既存日付に追加 |
| `overwrite` | 同一日付キーを取込データで置換 |

**Apple DB**:
- 候補: `~/Library/Group Containers/group.com.apple.calendar/Calendar.sqlitedb`, `~/Library/Calendars/**/*.sqlite*`
- テーブル自動検出: `CalendarItem` 優先、次に `Event`
- Core Data epoch (2001-01-01 起点) をローカル日時へ変換
- デフォルト範囲: 過去 365 日 + 未来 365 日。複数 DB は `time+title` で統合
- **非公式スキーマ** — 将来 macOS 更新で壊れる可能性あり。ICS フォールバック推奨

**TUI からの呼び出し** (`src/ui/app.py`):
- ICS: ワーカースレッド → `_pick_ics_file()` → `calendar_sync.sync_from_ics(path, mode)` → `sync_diary_index(force=True)`
- Apple: ワーカースレッド → `apple_calendar_sync.sync_from_apple_calendar(mode)` → 同上
- 完了後: RECORD タブの予定リスト・カレンダーマークを再描画

**CLI / テスト**:
```powershell
python tests\test_calendar_sync.py
python tests\test_apple_calendar_sync.py
```

---

## 3. 完了済み実装ステータス

### インフラ
- [x] Python 3.12 ARM64 + numpy + sentence-transformers (オフライン fallback あり)
- [x] llvm-mingw clang + OpenMP, llama.cpp ARM64, Qwen2.5-7B Q4_K_M

### データ / パイプライン
- [x] DailyContext: Calendar + Finance + Diary + AI_Consultations + LINE Sessions
- [x] ConversationSession + Response Latency
- [x] `calendar_manager.py`, `finance_manager.py`, `consultation_log.py`
- [x] **ICS カレンダー同期** (`calendar_sync.py`) + **Apple カレンダー同期** (`apple_calendar_sync.py`)
- [x] IMPORT タブ: LINE + ICS + Apple、共通マージモード Select
- [x] カレンダー取込 → `sync_diary_index(force=True)` (profiler 非実行)
- [x] Future Context (30日) を CONSULT プロンプトに注入
- [x] AI相談ログ → DailyContext + profiler 入力

### C++ / ベンチ
- [x] AoSoA NEON 4-lane + OpenMP Top-K + Windows mmap
- [x] `tests/benchmark.py` (PKBVEC01 ダミー生成 + QPS)

### プロファイラ
- [x] `deep_profile.v5` (abstract_identity, factual_signals, meta_narrative)
- [x] `user_profile.v2` (fixed / inferred / auto_extracted 分離)
- [x] LLM 深層分析 (対話+日記+家計簿相関)
- [x] `--no-llm` でルールベースのみ完走

### TUI
- [x] 4タブ構成 (RECORD / IMPORT / CONSULT / SETTINGS)
- [x] RECORD: カレンダー + 予定/家計簿/日記サブタブ + TimePicker + 保存
- [x] IMPORT: LINE + ICS + Apple カレンダー (macOS)
- [x] `tests/test_calendar_sync.py`, `tests/test_apple_calendar_sync.py` PASS
- [x] SETTINGS: fixed_attributes + 読取専用自動プロフィール
- [x] `tests/ui_smoke.py` PASS

### 品質
- [x] llama-server ライフサイクル管理
- [x] カレンダー DuplicateIds 修正
- [x] RECORD レイアウト: スクロール + 入力欄表示修正

---

## 4. 次に実装すべきタスク

### 4.1 プロファイル連携の強化 (要望あり)
- [ ] **fixed_attributes を profiler LLM プロンプトに注入** (分析の文脈として。上書きはしない)
- [ ] 基本情報保存時の **自動 profiler 再実行** は未実装 — 必要なら SETTINGS 保存後に `_profiler_worker` 呼び出し

### 4.2 RECORD / カレンダー UX
- [ ] 家計簿入力日・相談日のカレンダーマーク (現状は予定 `*` のみ)
- [ ] 月表示時の縦スクロール UX 改善 (狭い端末)
- [ ] CONSULT 回答の **streaming 表示**

### 4.3 分析・品質
- [ ] `data_merger` 未返信セッション (`awaiting_user`) の UI 表示
- [ ] CI: `tests/ui_smoke.py` + `tests/boost_check.py` + `tests/benchmark.py --quick`
- [ ] 実機 ARM64 で benchmark フルサイズ (10k/100k/500k) 計測・記録

### 4.4 バックログ
- [ ] Apple カレンダー DB スキーマ変更への追従 (ICS フォールバック維持)
- [ ] 予定密度 × 意思決定ルールの profiler 共起 (Future Context 深掘り)
- [ ] `src/python/app.py` (CLI) と `src/ui/app.py` (TUI) 名称衝突 — import 時は `importlib` パターン (`ui_smoke.py` 参照)

---

## 5. クイックスタート

```powershell
cd C:\Users\badger\Documents\cursur\decision_engine

python src\python\pipeline.py          # インデックス再構築
python src\python\profiler.py          # 深層プロファイル (--no-llm 可)
python src\ui\app.py                   # TUI
python src\python\consultation_engine.py "相談内容"   # CLI 相談 (知識検索統合)
python tests\ui_smoke.py               # ヘッドレス UI テスト
python tests\test_calendar_sync.py     # ICS パース・マージ
python tests\test_apple_calendar_sync.py
python tests\benchmark.py --quick      # C++ ベンチ (要 build/search_engine.exe)

.\build.ps1                            # C++ ビルド + スモーク実行
```

### 重要パス
```
src/python/data_merger.py           DailyContext + ConversationSession
src/python/consultation_engine.py  相談 (遅延初期化, Future Context)
src/python/profiler.py             deep_profile.v5
src/python/calendar_sync.py          ICS 取込 (apply_calendar_import 共通)
src/python/apple_calendar_sync.py  Apple カレンダー (macOS SQLite)
src/python/llm_config.py           7B 選択・サーバー引数
src/ui/app.py                      Textual TUI
src/ui/calendar_widget.py          カレンダー
src/ui/time_picker.py              時刻ロール
src/cpp/search_engine.cpp          NEON 検索
tests/benchmark.py                 QPS ベンチ
tests/test_calendar_sync.py
tests/test_apple_calendar_sync.py
data/raw/{diary.md,calendar.json,calendar_import.ics,finance.json,line_history.txt,ai_consultations.json}
data/processed/{vectors.bin,metadata.json,deep_profile.json,user_profile.json}
```

### 既知の制約・注意
- **基本情報保存 ≠ 自動プロファイling** — profiler は別途「再分析」または IMPORT の LINE 取込時
- **カレンダー同期 (ICS/Apple) ≠ profiler** — インデックス更新 (`sync_diary_index`) のみ
- **Apple カレンダー同期は macOS のみ** — Windows TUI ではボタン無効
- **fixed_attributes は CONSULT に使う / profiler 分析入力には未使用**
- TUI 表示は端末サイズ依存 (24行端末では RECORD 内スクロール必須)
- 7B 初回ロード ~2–3分。`tests/benchmark.py` は `search_engine.exe` 要ビルド
- Textual カレンダー日ボタンに **id を付けない** (再描画時 DuplicateIds 防止)

---

*Generated: 2026-07-05 — PKB handoff for new chat session*
