# M5 Action Plan: 構造化データ抽出とアプリ機能への統合

## 1. GBNF（文法制約）の組み込み方針
* **Cargo変更**: `llama-cpp-2` に `common` featureを追加（文法パーサ等のC++ライブラリを同梱するため）。
* **文法アセット**: 家計簿スキーマ厳密文法 `kakeibo_v1.gbnf` をRust側に静的埋め込み（`include_str!`）。フロントエンドからは識別子（task ID）のみを渡す。
* **二重検証**: LLM（GBNF）で「構文的にvalidなJSON」を保証し、Rust側（`serde` + 決定論的ロジック）で「意味の正規化・実在保証」を行う。不明フィールドは `unknown` として捏造を封じる。

## 2. プロンプトエンジニアリングと Context 管理
* **PromptSpec**: タスク識別子から `(system_prompt, few_shot, user_content)` を組み立てる純関数Builderを新設。
* **Chat Template API**: `model.chat_template()` を用いてモデル固有のテンプレートを適用し、プロンプトを構築。コンテキストはリクエストごとに使い捨て（決定論的動作）。

## 3. フロントエンド実装プラン
* **状態管理**: 純粋なReducer（`extractionReducer.ts`）を新設。UI表面は既存の `PocketBrainPanel` を `ExtractionPanel` へ段階進化させる。
* **データフロー**: テキスト入力 → `llm_extract` コマンド → トークンストリーム → 完了時に検証済みJSONを返却 → 構造化カード表示。
* **永続化スコープ**: プレビューとクリップボード書き出し（`ExtractionSink` の縫い目）までとし、実DB保存はM3で実装する。

## 4. フェーズ分け
* **Phase 0**: Cargo変更、GBNFアセット追加、xcodebuildリンクゲート確認。
* **Phase 1**: Rust側のPromptSpec、Chat Template経路、厳密パース、正規化ロジックの実装（単体テスト完備）。
* **Phase 2**: 文法サンプラの生成ループ合成、実機シミュレータでのJSON拘束E2E検証。
* **Phase 3**: フロントエンドReducer実装、Tauriコマンド拡張、UI結合テスト。
