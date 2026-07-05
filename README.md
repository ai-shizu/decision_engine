# PKB — 完全オフライン・ローカル完結型 自己エミュレーション・システム

Snapdragon X (Windows on ARM) の ARM NEON + OpenMP を用いた個人ナレッジベース検索コアと、
深層プロファイリング + ローカルLLM (llama.cpp) によるハイブリッド意思決定支援。

## 構成

```
data/raw/diary.md            日記 (無ければ pipeline.py が自動生成)
data/raw/line_history.txt    LINEトーク履歴 (無ければ profiler.py が自動生成)
data/knowledge/*.md          外部知識 (キャリア要件・意思決定フレームワーク等)
data/processed/vectors.bin   AoSoA バイナリ (C++ とレイアウト完全同期)
data/processed/metadata.json チャンク本文・レイアウト情報
data/processed/deep_profile.json 深層プロファイル (4フレームワーク分析)
data/processed/user_profile.json ユーザー属性(手動) + 自動抽出価値観 (profiler更新)
data/processed/knowledge_vectors.bin 外部知識のAoSoAインデックス
src/python/data_merger.py    日付正規化 + 日記×LINEの日単位結合 (DailyContext)
src/python/pipeline.py       DailyContext → 384次元ベクトル化 → AoSoA → バイナリ出力
src/python/profiler.py       マルチソース・プロファイリング・エンジン (日跨ぎ共起対応)
src/python/consultation_engine.py 相談時のみ起動する最適化パイプライン
src/python/app.py            CLI版意思決定支援 (検索→プロンプト合成→LLM推論)
src/ui/app.py                意思決定支援ダッシュボード (textual TUI)
tests/ui_smoke.py            TUIヘッドレススモークテスト
src/cpp/search_engine.cpp    NEON SIMD 内積 + OpenMP 並列 Top-K + mmap
tools/llama-arm64/           llama.cpp Windows ARM64 ネイティブバイナリ
models/Qwen2.5-7B-Instruct-Q4_K_M.gguf  ローカルLLM (7B優先、未配置時は models/*.gguf)
build/                       ビルド成果物
```

## 実行手順

```powershell
# 一括 (パイプライン → コンパイル → 検索デモ)
.\build.ps1

# 個別実行
python src\python\pipeline.py
clang++ -O3 -std=c++17 -march=armv8-a+simd -fopenmp src\cpp\search_engine.cpp -o build\search_engine.exe
.\build\search_engine.exe data\processed\vectors.bin data\processed\query.bin 5

# 深層プロファイル生成・更新
python src\python\profiler.py

# 意思決定支援ダッシュボード (TUI)
python src\ui\app.py

# CLI版 意思決定支援 (相談クエリ → 4セクション回答)
python src\python\app.py "今の会社で低レイヤ技術を極めるべきか悩んでいる"
python src\python\consultation_engine.py "相談内容"   # 知識検索統合版
```

## ダッシュボード (textual TUI)

| タブ | 役割 |
|---|---|
| RECORD | 日記入力 (日付+Markdown)。保存/ファイルドロップ時は裏で**ベクトル同期のみ**実行し分析は走らない。LINE履歴(.txt)をドロップすると裏で profiler が価値観を自動抽出し user_profile.json を更新 |
| CONSULT | 属性サイドバー常駐 + チャット。**送信時のみ** ベクトル化→NEON検索(日記+外部知識)→プロンプト合成→ローカルLLM推論 が起動 |
| SETTINGS | user_profile.json の手動編集と profiler 再分析 |

UX: タブ切替で主要ウィジェットに自動フォーカス (入力集中/対話集中)。
`F2` でサイドバー非表示 (対話没入)、`Ctrl+S` で日記保存。
インデックスはソースの mtime 比較で古い時のみ再構築される。

## DailyContext (日付ベースのクロスソース統合)

日記とLINE履歴は `data_merger.py` が日付 (YYYY-MM-DD に正規化) で結合し、
1日 = 1チャンクの「DailyContext」としてベクトル化される:

```
# DailyContext: 2026-06-21
## Diary
- 健康: 睡眠6時間。やや寝不足で午後に集中が切れた。 ...
## LINE_Interactions
### 同僚・田中
- 21:14 田中: 今夜さくっと飲み会どう？
- 21:20 自分: ごめん、今実装が乗ってるからパスするわ
```

- CONSULT検索は日記+LINEが統合されたチャンクに直接ヒットする。
- 日記とLINEの両方が存在する日は「その日の行動ログ」として
  リランキングで1.15倍に重み付けされ優先抽出される。
- profiler は「日記の状況 → 同日のLINE反応」の日跨ぎ共起ルールと、
  疲労日の対人感情の変化量を `cross_source_patterns` として定量化する。

## ConversationSession (状態保持型セッション抽出)

LINE履歴は単純ペアではなく、コンタクト別の**状態保持型セッション**として抽出:

- 相手の発言でセッション開始・追記
- ユーザーが最初の返信をするまでクローズしない (24h超え可)
- ユーザー返信後、30分以上の空白で1セッション確定
- ヘッダ: `[Session: 21:14 - 21:21 同僚・田中 | Response Latency: 6m]`
- 日跨ぎ例: `[Session: 2026-06-23 12:03 - 2026-06-25 12:10 同僚・田中 | Response Latency: 38h 49m]`

## 7B LLM と llama-server 最適化

- 推論モデル: `models/Qwen2.5-7B-Instruct-Q4_K_M.gguf` (4.68GB, bartowski量子化)
- 取得例:
  ```powershell
  curl.exe -L -o models\Qwen2.5-7B-Instruct-Q4_K_M.gguf `
    https://huggingface.co/bartowski/Qwen2.5-7B-Instruct-GGUF/resolve/main/Qwen2.5-7B-Instruct-Q4_K_M.gguf
  ```
- `llm_config.find_gguf()` は7Bを最優先。4GB未満のファイルは破損/部分DLとみなし1.5B等へフォールバック
- 起動引数: `--ctx-size 8192 --threads 8 --batch-size 512` (`PKB_LLAMA_*` 環境変数で上書き可)
- 7Bは初回相談/プロファイラ実行まで遅延ロード。起動待ちはモデルサイズに応じて最大300秒
- atexit/on_unmount によるプロセス回収は維持 (profiler/CLI/TUI 終了後 llama-server=0 を確認済み)

## 刺激→反応 ペアリング分析 (Interaction Pairs)

- `data_merger.py` はLINE会話を「相手の連続発話 (Stimulus) → 自分の連続発話
  (Response)」の対話ペアとして構造化する。自分発の会話はペアに含めない。
- `profiler.py` はペアを【状況→帰結の因果関係】として分析する:
  - ルールベース: 刺激分類(誘い/依頼/気遣い/情報共有) × 反応分類(承諾/謝罪付き辞退/
    保留/感謝/自己開示) の共起を確信度付きで `interaction_patterns` に出力
  - ローカルLLM: ペアを渡して価値観・バイアス・意思決定傾向を自由記述で抽出
    (`llm_interaction_insights`)。プロンプトで「分析対象はユーザーの Response のみ。
    相手の価値観・感情を混入させない」ことを強制。`--no-llm` でスキップ可
- いずれの分析も Stimulus は状況の分類にのみ使用し、相手の発言でユーザーの
  プロファイルが汚染されないよう設計されている。

## セッション単位の行動心理学分析

- ルールベース: セッション内の刺激×反応共起 + レイテンシ×日記矛盾 (心理的抵抗/優先度)
- 7B LLM: マルチターン + Response Latency + 同日日記から深層分析 (`llm_interaction_insights`)
- 結果は `user_profile.json` の `interaction_tendencies` / `latency_insights` に統合

## llama-server ライフサイクル管理

- バックエンドは自分が spawn したサーバープロセスだけを所有し、
  `stop()` で Terminate → 5秒待機 → Kill の段階的終了を行う。
- インスタンス生成時に `atexit` へ登録されるため、TUI終了 (`on_unmount`)、
  CLI終了、例外死のいずれでもゾンビプロセスは残らない。
- 既存サーバーへの相乗り時は所有権が無いため終了させない (安全側)。

## 自己エミュレーション層

- `profiler.py` は diary.md + line_history.txt をタグ付けし、以下を定量化して
  `deep_profile.json` に出力する (ルールベース・決定論的・完全オフライン):
  1. **認知的バイアス** — サンクコスト/確証バイアス等を語彙パターンで検出し証拠付きでスコア化
  2. **価値観の階層構造** — 行動を駆動する根源的欲求 (自己決定理論ベース) を頻度加重でランク付け
  3. **感情的反応パターン** — 相手別の平均感情・揺らぎ(SD)・典型反応を定量化
  4. **意思決定アルゴリズム** — 「状況A→帰結B」の共起マイニング + 日記「学び」欄の明示的自己ルール
- `app.py` の推論フロー: NEON検索(Top-3) → プロファイル+日記+外部知識をプロンプトに結合 →
  llama.cpp で「現状分析 / 価値観との整合性 / 必要なスキルギャップ / 次の一手」を生成。
- LLMバックエンドは自動選択: `llama-server` (127.0.0.1, OpenAI互換) → `llama cli` →
  ルールベース応答 (GGUF未配置でも動作)。外部APIへの通信は一切ない
  (`HF_HUB_OFFLINE=1` を強制し埋め込みもローカルキャッシュのみ使用)。

- 埋め込みは `sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2` (384次元)。
  未導入・完全オフラインでモデル未取得の場合は、決定論的 hashed n-gram
  フォールバックに自動切替してパイプラインを完走する。
- OpenMP ランタイム (libomp) が無い場合は `-fopenmp` を外せば直列動作でビルド可。

## バイナリレイアウト (little-endian)

| 領域 | 内容 |
|---|---|
| ヘッダ 32B | `magic "PKBVEC01"`, dim=384, lanes=4, num_vectors, num_blocks, block_bytes=6160, reserved |
| VectorBlock × N | `float data[384][4]` (dim-major/lane-minor AoSoA) + `int32 chunk_ids[4]` (パディングは -1) |

全ベクトルは L2 正規化済みのため、内積がそのままコサイン類似度になる。
検索結果の `chunk_id` は `metadata.json` の `chunks[].id` に対応する。

## 設計ポイント

- **AoSoA**: `data[d][0..3]` に「4本のベクトルの同一次元」を並べることで、
  `vld1q_f32` 1回で4レーンが埋まり、`vfmaq_f32` で4本の内積を同時に進める。
- **4段アンロール**: 独立アキュムレータ4本で FMA レイテンシを隠蔽。
- **並列 Top-K**: 各 OpenMP スレッドがローカル Top-K を保持し最後に直列マージ。
  共有構造へのロック競合ゼロ。
- **mmap**: Windows は `CreateFileMapping`/`MapViewOfFile`、POSIX は `mmap` で
  ゼロコピー読み込み。ヘッダ検証でPython側とのレイアウト不一致を即検出。
