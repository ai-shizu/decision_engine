# PKB — 安定アーキテクチャ索引

> 本書は **安定構造と正本への索引** である。揮発的な進捗・差分・件数の正本ではない。

---

## 1. 文書の役割

| 文書・実装 | 所有する情報 |
|---|---|
| `docs/AI_SKILLS.md` | 不変規律、タスク別読み込み手順（§0） |
| `docs/HANDOFF.md` | 現在地、worktree、直近作業、volatile な進捗 |
| `docs/CONTEXT.md`（本書） | 安定アーキテクチャ、所有権、正本への索引 |
| `docs/architecture/INCIDENT_LEDGER.md` | 事故と絶対裁定 |
| 実コード | タブ、IPC コマンド、schema、現在挙動の最終的正本 |
| SPEC 群（`docs/SPEC_FOXTROT_UI.md` ほか） | 個別機能の設計契約 |
| `docs/AUDIT_FINDINGS_2026-07-11.md` | findings の状態と解決記録 |

**優先関係（競合時）:** 実コードと `INCIDENT_LEDGER` を最優先し、文書側を修正する。
`AI_SKILLS`（規律）→ `HANDOFF`（現在地）→ 本書（安定索引）の順で役割を区別せよ。

本書について:

- current HEAD、worktree、進捗、次タスクは **HANDOFF** の責務
- 規律は **AI_SKILLS** の責務
- 事故裁定は **INCIDENT_LEDGER** の責務
- 実装詳細は各ソースファイルが正本
- **本書だけを根拠に機能追加・削除を判断してはならない**
- 完了の定義（DoD）とバックログは本書が **所有しない**（DoD は AI_SKILLS、進捗/次タスクは HANDOFF）

---

## 2. システム概要

PKB（Personal Knowledge Base）は、完全オフラインを原則とする自己エミュレーション・意思決定支援システムである。

- 構成: **Tauri v2 + React + Rust + Python + C++**
- ローカル LLM は `127.0.0.1` のみ（外部 API・CDN 禁止）
- UI と Python は Tauri/Rust 管理の **stdio JSON IPC**
- 個人データはローカル永続化（コミット禁止領域あり）
- 埋め込みモデル・LLM は遅延初期化（初回 consult / profiler まで起動しない）
- オンライン knowledge fetch は既存の明示許可例外（`knowledge_fetcher` + 環境フラグ）のみ

モデル名、容量、具体的テスト件数は本書に書かない（揮発するため。正本は `config/model_params.json` とテスト実行結果）。

---

## 3. 正本マトリクス

| 関心事 | 正本 |
|---|---|
| UI navigation（タブ ID・順序） | `apps/desktop/src/App.tsx` |
| frontend types | `apps/desktop/src/lib/types.ts` |
| frontend IPC wrappers | `apps/desktop/src/lib/engine.ts` |
| Rust transport / replay policy | `apps/desktop/src-tauri/src/engine.rs` |
| Python command dispatch（IPC コマンドの唯一の正本） | `src/python/engine_stdio.py` |
| backend facade | `src/python/core/facade.py` |
| persistence paths | `src/python/core/paths.py` |
| retrieval / compiler | `src/python/core/session_memory.py` / `src/python/core/retrieval_manifest.py` |
| consultation | `src/python/core/consultation_engine.py` |
| C++ search contract | `src/python/core/pipeline.py` / `src/cpp/search_engine.cpp` |
| runtime parsers | `apps/desktop/src/lib/parseConsultResponse.ts` / `apps/desktop/src/lib/parseManifest.ts` |
| incidents | `docs/architecture/INCIDENT_LEDGER.md` |
| UI / Echo 等の設計契約 | `docs/SPEC_FOXTROT_UI.md` / `docs/SPEC_ECHO_GENESIS.md` ほか SPEC 群 |

IPC コマンドの完全一覧や固定件数は複製しない。変更・監査時は必ず `src/python/engine_stdio.py` を読め。

---

## 4. Runtime flow

```text
React
→ Tauri command
→ Rust EngineManager
→ Python stdio dispatch
→ facade
→ core modules
→ local C++ search / local LLM
```

契約（詳細は AI_SKILLS §2 / §16 と `engine.rs`）:

- stdout は JSON protocol 専用（print は stderr）
- status / chunk は cid 付き中間イベント（`ok` キーを持たせない）
- Remote / NotReady は再送しない
- mutating / unknown command は NoReplay（restart のみ、結果不明エラー）
- retry-safe command だけ restart 後に最大 1 回再送
- runtime parser は `unknown` 入力を検証する
- raw cast（`as T` / 未検証 generic）を境界検証の代用にしない

---

## 5. UI surface

`apps/desktop/src/App.tsx` の `TABS` と同一順序。行の追加・並べ替え時は App.tsx と同じ変更で本書を更新する。

| id | label | owner | 主要責務 |
|---|---|---|---|
| `record` | RECORD | `apps/desktop/src/components/RecordTab.tsx` | 予定・家計簿・日記の記録とカレンダー表示 |
| `import` | IMPORT | `apps/desktop/src/components/ImportTab.tsx` | LINE / 文書 / カレンダー取込 |
| `consult` | CONSULT | `apps/desktop/src/components/ConsultTab.tsx` | 通常相談（streaming・status 進捗） |
| `interview` | INTERVIEW | `apps/desktop/src/components/InterviewTab.tsx` | 面接 / GD / ES / 感想戦。講評の実測 `tensor_profile.6d.v1` 表示 |
| `probe` | PROBE | `apps/desktop/src/components/ProbeTab.tsx` | HistoricalNode 探索インタビュー |
| `profile` | PROFILE | `apps/desktop/src/components/ProfileTab.tsx` | 深層プロファイル、Context Observatory、Oracle / Twin / Tensor diagnostics |
| `settings` | SETTINGS | `apps/desktop/src/components/SettingsTab.tsx` | 設定・profiler 手動実行など |

---

## 6. 永続化と privacy

- `data/raw/`、`data/processed/`、`models/`、`logs/` は commit 禁止
- 開発 / インストール版の data root 所有者は `src/python/core/paths.py`
- RetrievalManifest 読込破損は hard-fail（`NO_MANIFEST` 化しない）
- 保存失敗は typed `RetrievalManifestPersistenceError` だけ caller 隔離（INC-PHASE4A-05）
- corrupt store を修復・削除・上書き・`NO_MANIFEST` 化しない
- warning / log へ query、本文、path、例外値を出さない

---

## 7. 変更経路

新しい backend 機能の標準順序:

```text
core implementation
→ facade API
→ engine_stdio dispatch
→ frontend engine wrapper
→ runtime parser/type
→ UI
→ boundary tests
```

読むべき規律（AI_SKILLS）:

| 変更種 | 必読 |
|---|---|
| Rust IPC / replay | §1, §2.1, §16、`docs/architecture/INCIDENT_LEDGER.md` |
| 永続化・Manifest・runtime 境界 | §1, §16、INCIDENT_LEDGER |
| UI (React) | §1, §2.1, §3.4, §3.5、関連 SPEC |
| 検索 / mmap / LSM | §1, §9, §10 |

---

## 8. 維持規律

本書へ次を置くことを **禁止** する:

- 現在の commit / hash
- 未 commit 差分
- 完了件数、テスト件数
- 「次に実装すべき」バックログ
- 一時的な作業手順
- exact IPC command count
- モデルバージョン・容量
- ファイル内容ハッシュの固定値
- 日付だけで正当化されたスナップショット

タブ構成、責務境界、正本パスが変わった場合だけ、同じ変更で本書を更新する。
