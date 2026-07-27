# SPEC: BLACKBOX SIMULATOR — オフライン金融シミュレータ / 定量的意思決定プロファイラ

> **Parent authority:** `docs/AI_SKILLS.md` v2（至高律十律・FLR プロトコル）。本 SPEC は十律への適合を暗黙の前提とし、矛盾して見える記述はすべて読み違いとして十律側が勝つ。
> **Status:** Phase 0（設計 + コアデータ構造）提出・**指揮官裁定待ち**。Phase 1 以降の着工は裁定後のみ（FLR 統治手続）。
> **実装正本:** `apps/desktop/src-tauri/src/blackbox_sim/`（feature `blackbox-sim`、既定ビルド非包含）。
> **記号:** 不変条件 `BXS-I-nn` / 罠 `BXS-W-nn`（§19.4 の記号法に合流）。

---

## 0. 権威と裁定要請（FLR-1 正本検問の報告）

本 SPEC は指揮官の 2026-07-27 直接指令（「BLACKBOX SIMULATOR」コアアーキテクチャの設計・実装）を founding directive とする。起草時の憲法監査で以下 4 件の矛盾・緊張を検出した。**無言で埋めず、ここに報告する（前文「勝手に埋めるな」）。** 各件に既定案を付すが、確定は裁定による。

| # | 事項 | 検出した矛盾 | 既定案（裁定までの仮置き） |
|---|---|---|---|
| R-1 | コードネーム衝突 | 「BLACKBOX」は §15 / `MASTER_PLAN_LEGACIES.md` の **L3 BLACKBOX（実戦結果台帳）** に予約済み。本機能（シミュレータ）とは別物 | 機能名は指令どおり「BLACKBOX SIMULATOR」、モジュール名 `blackbox_sim` / 記号 `BXS-` で L3 の namespace（将来 `blackbox_ledger` 想定）と分離。L3 の青写真は不変 |
| R-2 | PHANTOM 先行封印 | §18「Legacies の PHANTOM 以外からの着手」封印。本機能は 5 遺産外の新規指令だが、**新しい計測器**である以上「校正装置なしの計測器増築は倒錯」の理は及ぶ | 校正装置を本機能の内部に必須ゲートとして組み込む（§12 PHANTOM-BOT）。既知解校正 GREEN + 裁定まで、profile/tensor への書込 API はコード上存在させない |
| R-3 | 表の顔の美学 | 指令の「ポップでカジュアル（Coffee Inc 2 様）」と §3.4（装飾 1px 不要）・§4.66〜4.77（端末美学 全域統一法）・§18（UI ライブラリ封印） | 「ポップ」は視覚装飾ではなく**コピーライティングとゲームループ設計**（テンポ・報酬構造・文言）で実現し、視覚は端末美学に完全従属する。緩和には明示裁定が要る |
| R-4 | UI マウント位置 | モバイル 7 面 parity は指揮官裁定（§4.25 台帳）。ドックタブ追加は勝手にできない | INTERVIEW → Coliseum シェル配下の新アリーナ「BLACKBOX」として搭載（GD / 鬼モードの隣。Phase 14 UI 法をそのまま継承）。ドック構成は不変 |

**裁定記録（2026-07-27・指揮官）:** R-1〜R-4 **全件正式承認**。確定事項: (R-1) `blackbox_sim` / `BXS-` で完全カプセル化し将来統合時も親空間を汚染しない。(R-2) `CalibrationCertificate` の production 構築経路不在（uninstantiable）による書込ブロックを校正 GREEN まで維持。(R-3) 表の顔は**「レトロインダストリアル・ターミナル風 UI」**（ハードボイルドなガジェット感・情報密度と没入感優先）として構築、段階的オンボーディング設計を許可。(R-4) ドック 7 面 parity 死守・Coliseum 配下の新アリーナとして統合。同裁定により Phase 1 射程は「市場カーネル + ActionCompiler + 相互依存テスト」へ拡大（旧 P1 + 旧 P2 の ActionCompiler 前倒し。§17 の表は裁定後の正）。

**本機能の憲法上の存在意義（設計者の主張):** FSA-05 により 6D Tensor の `authoritative_profile()` は全 score=N/A である — 面接トランスクリプトには決定論的観測器が存在しないからだ。シミュレータは**全ての意思決定が最初から機械可読な数値**として発生する統制環境であり、LLM を一切介さずコードだけで完全かつ一意に導出できる観測値を生む。つまり本機能は、第五律 2 が要求する「決定論的観測器」を初めて実装可能にする計測装置である。

---

## 1. 目的と非目的

### 1.1 目的

1. **表の顔:** 経営シミュレーションゲーム。プレイヤーは消費財チェーン企業を四半期単位で経営し、調達・価格設定・投資・財務・M&A/LBO の意思決定を行う。
2. **裏の顔:** 内部は SDE による市場生成、複式簿記による財務生成、VaR 制約、クローズドフォーム企業価値評価で駆動する決定論エンジン。
3. **真の目的:** プレイ履歴（テレメトリ）から認知バイアスを**純粋な決定論数式**で抽出し（損失回避 λ、処分効果、アンカリング、過信、コミットメントのエスカレーション、プレッシャー下劣化）、校正ゲート通過後に deep_profile へ供給する。

### 1.2 非目的

- 実市場データの取得・再現（第一律。全て手続き生成）。
- 金融教育・投資助言（ゲーム内世界は架空通貨 CRD の架空経済。実在銘柄・実在企業を生成しない）。
- 面接 6D の代替（本機能は**別の計器**。§11 の計器分離を参照）。
- LLM による採点・評価・数値生成（第五律 2 / FSA-05。LLM はフレーバ文言のみ）。
- プレイヤーの「攻略」への適応（deep_profile を読んでの出題適応は封印 — §2 の壁 W-b）。

---

## 2. 三層アーキテクチャと隔離壁

```
┌────────────────────────────────────────────────────────────────┐
│ L1 Presentation（React / Coliseum アリーナ）                    │
│   純 reducer + Dumb View。数値は全て L2 のビューモデル転記。     │
│   LLM フレーバ文言（§14 ガード通過後のみ）。                     │
└──────────────┬─────────────────────────────────────────────────┘
               │ typed IPC（閉じた command 集合・§15）
┌──────────────▼─────────────────────────────────────────────────┐
│ L2 Simulation Core（Rust worker スレッド専有・決定論）           │
│   Genesis（凍結パラメータ）→ 市場カーネル（SDE）→ ActionCompiler │
│   → 複式簿記 → 決算 → スナップショット。全て整数権威状態。       │
│   隠しオラクル（真値評価・最適解）は L2 内部に封緘。             │
└──────────────┬─────────────────────────────────────────────────┘
               │ 決定ログ（append-only・観測記録のみ）
┌──────────────▼─────────────────────────────────────────────────┐
│ L3 Observation & Analysis（決定論推定器）                        │
│   テレメトリ → バイアス推定器（§10）→ blackbox_profile.v1        │
│   → 校正ゲート（§12）→ 合法出口 3 つ（consult / 講評 / PROFILE） │
└────────────────────────────────────────────────────────────────┘
```

**隔離壁（新設。1 つでも破れば機能の魂が死ぬ):**

- **壁 W-a（真値の封緘):** Genesis の真パラメータ（ドリフト・真値・最適解）は L2 内部のみ。ビューモデル・LLM プロンプト・プレイヤー可視アーティファクトへの直列化を禁止する。漏れた瞬間、最適性ギャップ計測は自己申告に退化する（第九律の系）。
- **壁 W-b（profile→game 禁止):** シナリオ生成・難度・出題は deep_profile / gap / tensor / twin / oracle を**読まない**。データフローは sim → 分析 → profile の一方向のみ。逆流は §18 封印「面接 UI を Echo で駆動 = 自己成就予言ループ」（F-5）と同型であり、かつ適応的刺激は計測の比較可能性を破壊する。難度適応に使ってよいのは**同一セッション内のゲーム内成績のみ**。
- **壁 W-c（チャネル分離):** テレメトリは `channel="blackbox_sim"` を焼き込み、gap_analysis の主観/客観コーパスへ**合流させない**（第八律。シミュレータ内行動は統制環境下の行動計測であり、生活の資源配分ではない）。供給先は §11 の専用レーンのみ。
- **壁 W-d（LLM は数値に触れない):** 会計・価格・スコアの唯一の書き手は L2 の ActionCompiler と決算器。LLM 出力は表示専用文字列で、数値発明ガード（§14）を通過しないものは破棄される。

---

## 3. 決定論基盤（第五律の実装）

### 3.1 PRNG — Philox4x32-10

- 実装は `blackbox_sim/rng.rs`（依存ゼロ・自前実装）。定数: 乗数 `0xD2511F53` / `0xCD9E8D57`、Weyl `0x9E3779B9` / `0xBB67AE85`、10 ラウンド。
- Random123 公表 known-answer vectors（zero / max / π-digits の 3 本）をユニットテストで釘付けし、Python 参照実装（`tests/test_blackbox_sim_contract.py` 内・stdlib のみ）と**言語間相互 assert**する（PKBVEC01 の 1 バイト同期規律と同型）。
- counter-based であるため、ブロック添字による**ランダムアクセス再生**が可能（リプレイ検証の基盤）。

### 3.2 seed の由来（I-17 準拠）

- `campaign_seed = SHA-256(canonical_genesis_request)` 先頭 8 バイト。canonical_genesis_request = `scenario_id || difficulty || campaign_index || created_date(YYYY-MM-DD)` の固定順連結。OS エントロピー・時刻 ns・unseeded 乱数は 1 箇所でも違憲。
- ストリーム分離は**ドメイン定数表**（固定・再利用禁止。PKBTEN01 レーン番号の規律と同型）:

| ドメイン | 定数 | 用途 |
|---|---|---|
| `DOM_GENESIS` | 1 | 世界パラメータ抽選 |
| `DOM_MARKET` | 2 | 市場ショック列 |
| `DOM_EVENTS` | 3 | 手続きイベント |
| `DOM_STIMULI` | 4 | 計測刺激の割付（§9） |
| `DOM_BOTS` | 5 | 校正ボット（§12） |

- key = campaign_seed XOR domain 定数、counter = (substream, block) の固定レイアウト。

### 3.3 浮動小数点政策

- **権威状態は整数のみ。** 金額 = i64 minor units（100 minor = 1 CRD）、価格 = i64 tick、金利 = i32 bp、スコア = i64 micro（×1e6、floor-half-up — §4.15 の確立量子化）。
- SDE ステッパ内部のみ f64 を許すが、使用可能演算を `{+,−,×,÷,sqrt}`（IEEE 754 正確丸め・全プラットフォーム bit 同一）+ 自前 `det_exp` / `det_ln`（多項式・上記演算のみで構成）に**制限**する。libm の超越関数（`f64::exp/ln/sin/cos/powf`）は禁止 — プラットフォーム間で bit が揺れる。
- 正規乱数は Marsaglia polar 法（`sqrt` + `det_ln` のみ使用。棄却ループはストリーム決定論下で決定論）。Box-Muller は `cos` を要するため禁止。
- 各 tick の出力は権威状態へ入る**前**に量子化する。ゴールデンベクタ（seed → 系列先頭 N 値の bit 同一）を CI で macOS arm64 / iOS sim / x86_64 の 3 環境釘付け（Phase 1 ゲート）。

### 3.4 リプレイ恒等式（最上位不変条件）

```
state(t) = fold(Genesis, decision_log[0..t])           …(BXS-I-01)
digest(replay(Genesis, log)) == digest(live_state)      恒常検証
```

- 実時間・壁時計はゲーム状態遷移に**入力されない**。タイムアウトによる強制デフォルト決定も「明示イベントとしてログに記録された decision」であり、リプレイはログだけで完結する。レイテンシ（ms）は**観測データ**であってシミュレーション入力ではない。

---

## 4. 世界生成 — CampaignGenesis（凍結アーティファクト）

- キャンペーン開始時に `DOM_GENESIS` ストリームから全パラメータ（各 SDE の κ, θ, σ, λ_J, レジーム遷移行列、需要季節形状、競合プロファイル、イベント日程骨格）を校正済みレンジ内で抽選し、`CampaignGenesis` として**一度だけ**生成・SHA-256 fingerprint 付きで凍結する（§4.60 InterviewSessionArtifact の同型。hash-before-construction — §16.1）。
- 以後の全計算は Genesis のみを参照。途中変更 API を作るな。fingerprint 不一致ロードは fail-closed。
- **壁 W-a:** Genesis はプレイヤー可視面へ出ない。可視化されるのは観測可能な市況（価格・出来高・ニュース）のみ。

## 5. 市場カーネル（SDE 群）

tick = 1 週。四半期 = 13 tick。Euler–Maruyama、log 空間、Δ=1。

| 系列 | 模型 | 離散化 |
|---|---|---|
| 原材料価格 | Schwartz 1-factor（O-U in log） | `x' = x + κ(θ−x) + σ(M)·ε` |
| 市場需要 | 決定論季節形状 × AR(1) 乗法ショック | `y' = φy + σ_d·ε`, `D = S(t)·det_exp(y)` |
| 金利 | Vasicek | `r' = r + a(b−r) + σ_r·ε`（bp 量子化・0 床） |
| 株式市況指数 | レジーム切替 GBM + Merton ジャンプ | ジャンプは `Bernoulli(λ_J(M))`、サイズ `det_exp(μ_J + σ_J ε)` |
| レジーム M | 2 状態 Markov（Calm / Stress） | 遷移は `DOM_MARKET` の uniform 閾値 |

- ε は §3.3 の polar 法。系列間相関は固定 Cholesky 因子（Genesis で凍結、i64 に量子化した係数）で合成。
- 全系列は**リングバッファ（固定 512 tick）**保持。全履歴のオンメモリ展開禁止（第三律）。リプレイで任意時点を再構成できるため、履歴の保持はビュー都合の evicting キャッシュにすぎない。

## 6. 複式簿記エンジン（Zero Panic 会計）

### 6.1 構造

- `Money(i64)`: minor units。全演算 checked（オーバーフロー = 型付きエラー、wrap/saturate/panic 禁止）。中間和は i128。float 換算 API を**作らない**。
- `AccountCode`: 閉じた enum（勘定科目表）。5 分類（Asset/Liability/Equity/Revenue/Expense）は `class()` で静的に決まる。
- `Posting { account, side: Debit|Credit, amount: Money }`: amount > 0 を構築時強制（符号は side が持つ）。
- `Transaction::new(kind, postings) -> Result<Transaction, LedgerError>`: **balanced-by-construction** — Σdebit == Σcredit（i128 比較）でなければインスタンスが存在しない。空・零額・件数超過も構築時拒否（Silent Sanitization 禁止 — §16.2）。
- `TxKind`: 閉じたビジネスイベントカタログ。**UI/LLM は仕訳を書けない** — UI が送るのは `ActionIntent`（閉 enum）のみで、L2 の ActionCompiler だけが Intent → Transaction 展開を所有する（壁 W-d の実体）。

### 6.2 不変条件

- **BXS-I-02（試算表恒等):** 符号付き残高表現（借方正）で `Σ_all_accounts balance == 0`。balanced tx の帰納で定理として成立するが、**受け側検証を免除しない**（第六律） — 決算時とロード時に必ず再検証。
- **BXS-I-03（CF 整合):** 間接法 CF 計算書の純増減 == Cash 勘定の期中増減。不一致 = `SimError::AccountingBreach` → セッション `Dead`（fail-closed・修復禁止・テレメトリ保全）。
- **BXS-I-04（原子性):** `apply(tx)` は all-or-nothing。オーバーフロー事前検査 → 全 posting コミット。部分適用状態は存在しない（FLR-3「片側成功」の封殺）。
- 決算ごとに世代スナップショット（append-only、pointer-payload ID 結合 — §16.3、保持上限 8 世代 — §16.5.1 の精神）。破損 latest のロードは hard-fail し、旧世代ロードは**明示のユーザー操作**としてのみ提供（silent fallback 禁止）。

## 7. ゲーム FSM（決定表 — FLR-3。実装はこの表の転記のみ）

```
SessionState = Genesis | Active(TurnPhase) | Sealed | Dead(reason)
TurnPhase    = Observe → Decide → Execute → Settle → Report →（次 tick の Observe）
```

| From | Event | To |
|---|---|---|
| Genesis | StartConfirmed | Active(Observe) |
| Genesis | AbortRequested | Sealed |
| Active(Observe) | ObservationClosed | Active(Decide) |
| Active(Decide) | DecisionSubmitted | Active(Execute) |
| Active(Execute) | ExecutionApplied | Active(Settle) |
| Active(Settle) | SettlementVerified | Active(Report) |
| Active(Report) | ReportAcknowledged | Active(Observe) |
| Active(Report) | CampaignCompleted | Sealed |
| Active(＊) | AbortRequested | Sealed |
| Active(＊) | CorruptionDetected | Dead |
| 上記以外の全組合せ | — | `FsmError::IllegalTransition`（丸め・発明禁止） |

- `Sealed` / `Dead` は吸収状態（いかなるイベントも IllegalTransition）。
- タイムアウト強制決定は Director が `DecisionSubmitted(action=ForcedDefault, forced_default=true)` を**明示イベント**として投入する（§3.4）。

## 8. 金融工学レイヤ（隠しオラクル — 壁 W-a の内側）

生成パラメータが既知であることを利用し、**閉形式の真値**を L2 内部で計算する。これが最適性ギャップ計測のグラウンドトゥルースである。

- **本源価値:** 真のドリフト・割引率による DCF / Gordon 成長。プレイヤーの買収・売却価格判断との乖離 = バリュエーション誤差。
- **拡張オプション価値:** CRR 二項木（加減乗除のみ・決定論）。
- **VaR 制約:** パラメトリック VaR（真の σ(M) から）+ ヒストリカル VaR（リング窓）。リスク予算超過 → マージンコールイベント（プレッシャー計測の刺激源）。
- **LBO モジュール:** 整数デットスケジュール（senior/mezz、期限・金利 bp）、コベナンツ（Net Debt / EBITDA の量子化比率）判定。破り = デフォルトイベント。
- 真値・最適解は決定ログの**分析時参照**としてのみ露出（L3 推定器へ）。ビューモデル直列化は leak-guard（marker 走査テスト）で封鎖する（`render_guard::verify_no_leakage` の同型）。

## 9. 計測設計 — 刺激プランティングとテレメトリ

### 9.1 刺激（Stimulus）

Director が `DOM_STIMULI` ストリームで**自然なゲームイベントとして**計測刺激を植え込む。全刺激は `StimulusRef {kind, params_digest}` として決定ログに記録され、推定器はこの参照で刺激と応答を突合する。

| 刺激 | ゲーム内の姿 | 計測対象 |
|---|---|---|
| 混合ギャンブル対 | 保険・ヘッジ・新規出店オファー（既知 (G, L, p) 格子） | 損失回避 λ |
| アンカー A/B | アナリスト・コンセンサス表示 = 真値×(1±δ)（被験者内交互割付） | アンカリング |
| 区間予測 | 四半期 Report で次期売上の 80% 信頼区間を申告 | 過信（較正） |
| サンクコスト対 | 既往プロジェクト継続 vs 同一条件の新規案件 | エスカレーション |
| 危機イベント | マージンコール・カウントダウン | プレッシャー下劣化 |
| ポジション清算機会 | 含み益/含み損ポジションの売却 UI | 処分効果 |

### 9.2 テレメトリ（第八律 — 記録は聖域）

- `DecisionEvent { seq, tick, phase, action, stimulus, latency_ms, forced_default, state_digest }`。append-only。編集・削除 API を作るな。
- `latency_ms` は monotonic clock の観測値（状態入力ではない — §3.4）。
- リングは**満杯時 reject + 明示 flush**（Settle 時に vault へ）。記録の silent evict は違憲（第八律 1）。市場系列リング（派生データ）のみ evicting を許す。
- PII は構造的に存在しない（フィールドは全て enum / 数値 / digest。自由文フィールドを追加するな）。

## 10. バイアス推定器（全て決定論・stdlib 相当のみ）

出力は全軸 `BiasEstimate { value_micro: Option<i64>, n_obs, sufficiency_micro }`。**確度ゲート未達は N/A（None）— 0 の捏造は嘘である（LAW-20）。** タイブレークまで決定論（グリッド探索・同点は小さい側 — §4.15 の確立形）。

| lane | 軸 | 数式（骨子） | 確度ゲート |
|---|---|---|---|
| 0 | `loss_aversion` | 受諾 iff `p·G − (1−p)·λ·L > 0` のロジット。λ をグリッド `[0.5, 5.0] step 0.05` で決定論 MLE | 選択 ≥ 12 |
| 1 | `disposition_effect` | Odean: `PGR − PLR`、`PGR = RG/(RG+PG)` | 実現+含み ≥ 各 10 |
| 2 | `anchoring` | `mean( (V_answer − V_true) / (A − V_true) )` を A/B 対で | 対 ≥ 6 |
| 3 | `overconfidence` | 80% 区間の実被覆率との乖離 `0.8 − coverage` | 予測 ≥ 8 |
| 4 | `escalation_commitment` | 継続率(サンク有) − 継続率(同一条件・サンク無) | 対 ≥ 4 |
| 5 | `pressure_degradation` | 最適性ギャップ(Stress) − 最適性ギャップ(Calm)（各 tick の真値オラクル比） | 各群 ≥ 6 |

- lane 番号は**恒久固定・再利用禁止**（PKBTEN01 の規律）。
- 推定器は L2 の状態を直接読まない。入力は決定ログ + 刺激参照 + オラクル別表のみ（記録と分析の分離）。
- 反復解法（ロジット等）は固定回数・固定格子。収束判定の浮動小数比較で分岐する実装を書くな（プラットフォーム揺れの温床）。

## 11. deep_profile 接続 — 計器の分離

- 成果物は `blackbox_profile.v1`（vault 新テーブル。schema v12、Phase 3 で migration）。面接 6D (`tensor_profile.6d.v1`) とは**別の計器**であり、直接合流しない（構成概念の純粋性 — §4.59 の精神）。
- 6D への射影は**決定論的観測器が存在する 4 次元のみ**、`instrument="blackbox_sim"` の provenance 付きで Phase 6 に定義する（重み定数は校正データ取得後に凍結 — 投機的定数の先行凍結は LAW-19 違反）。`communication` / `collaboration_adaptability` は本計器からは**恒久 N/A**（ソロシムに観測器なし。捏造禁止）。
- 露出の合法出口は Echo と同じ 3 つのみ: **consult 注入（`consult_context.rs` の fail-safe 読込に合流）/ 講評・Debrief / PROFILE UI**（I-22 と同型）。面接出題・GD 議論・es_review へは不出。
- **書込は二重ゲート:** (a) §12 校正 GREEN、(b) 指揮官裁定。それまで書込 API はコード上**存在しない**（型ゲート: `CalibrationCertificate` を要求する関数署名のみ先行定義し、production 構築経路を置かない）。校正前に到達可能な唯一の射影は全 N/A + `calibration="uncalibrated-instrument"` マーカー（`authoritative_profile()` の `no-llm-authority` と同じ「正直な未測定」の作法）。

## 12. 校正 — PHANTOM-BOT 既知解ゲート（R-2 の履行）

- 合成エージェント（BOT）に**既知のバイアス真値**（λ*, 処分効果*, …）をプラントし、公開決定 API だけを通してシミュレータをプレイさせる（`DOM_BOTS` ストリーム・全経路決定論）。
- 合格判定: 全パイプライン（sim → telemetry → 推定器）が真値を**回収率下限**（軸別の絶対誤差上限 + 順位相関下限）で回収すること。下限は「完全回収 100%」に強化するな — 雑音劣化への余裕こそが検定力の定義である（L5 PHANTOM の fixture-blindness 規律をそのまま継承）。
- **fixture-blindness:** BOT の方策モジュールと推定器モジュールは定数・実装を共有しない（相互 import ゼロを契約テストで走査）。推定器をフィクスチャに合わせて調整する変更は校正の自殺。
- known-answer suite が CI で GREEN になるまで、本機能は**テレメトリ収集のみのモード**で動く（プロファイル出力なし）。

## 13. メモリ予算（第三律 — 確保の前に上限）

| 領域 | 方式 | 上限 |
|---|---|---|
| 市場系列リング | evicting（派生データ） | 8 系列 × 512 tick × 8B = 32 KiB |
| ジャーナル尾部 | reject-full → Settle で flush | 1,024 postings ≈ 56 KiB |
| 決定ログリング | reject-full → vault flush | 512 event ≈ 64 KiB |
| 世代スナップショット | append-only・保持 8 世代 | 直列化 ≤ 64 KiB/世代 |
| **SimCore 常駐合計** | — | **< 1 MiB（size_of ガードテスト + 実測）** |

- SimCore は**専有 worker スレッド**が所有（第四律。LLM worker の M6 パターン同型）。UI は Channel + atomic のみで話しかける。GB 級資源を持たないため purge 対象はないが、memory warning 時は Settle 済みスナップショットへの即時 flush のみ行う（ObjC コールバック内は atomic store のみ — §1.1-1 を破らない）。
- background 遷移: 進行中 tick を Settle → snapshot → idle。タイマー・ポーリングを持たない。
- 全確保は固定容量（`with_capacity(定数)`）。自己申告サイズ・可変長の事前信頼は禁止（§16 allocation-bounded）。

## 14. LLM 境界 — フレーバ層（任意・存在しなくても完全動作）

- feature 依存: `blackbox-sim` は `pocket-brain` に**依存しない**。LLM 不在時は固定テンプレート文言で完全プレイ可能（劣化ではなく既定）。
- LLM 入力は `FlavorRequest { template_id, slots: Vec<(SlotId, String)> }` のみ。slot 値は L2 が量子化済み数値を文字列化したもの。Genesis・profile・gap・vault 由来の値は型として渡せない（壁 W-a / W-b / §4.57 の型遮断と同型）。
- LLM 出力は `flavor_guard::verify_no_numeric_invention(text, slot_allowlist)` を通過しない限り破棄 → 固定テンプレートへ fail-closed。判定: 出力中の全数字列が slot allowlist の完全一致集合に含まれること。ゲーム進行は LLM 応答を**待たない**（アンビエント — §3.4-9）。

## 15. I/O 境界・IPC・FE 契約（§16 準拠）

- Tauri command は機能単位の明示関数のみ（Phase 5）: `bxs_start_campaign` / `bxs_get_view` / `bxs_submit_decision` / `bxs_advance` / `bxs_abort` / `bxs_load_generation`。request は `#[serde(deny_unknown_fields)]` の閉じた型。汎用 cmd・`serde_json::Value` 受けは禁止。
- Serde 契約: fields=`camelCase` / variants=`snake_case`（§4.12a-2）。
- FE は `invoke<unknown>` → command 固有 exact-key parser（backend validator の鏡像）。状態は純 reducer（`researchUiReducer.ts` パターン）+ 依存ゼロ Harness テスト。Zustand 等は封印済み。
- ビューモデルは**表示に必要な最小集合**のみ（全状態ダンプ禁止 — 壁 W-a の漏出面を最小化）。
- エラーは二層: core は具体的 `SimError`、境界で `UiErrorCode` 固定文言へ（生エラーは必ず `console.error` — §4.52a-3）。

## 16. 不変条件台帳と罠

### 不変条件（回帰ガード名は Phase 0 実装の実名）

| ID | 内容 | ガード |
|---|---|---|
| BXS-I-01 | `state = fold(Genesis, decision_log)`。壁時計はシミュレーション入力にならない | Phase 2: replay digest 恒等 property |
| BXS-I-02 | 試算表恒等（符号付き Σ==0）。受け側再検証を省略しない | `ledger::tests::balanced_apply_preserves_zero_sum` ほか |
| BXS-I-03 | CF 純増減 == Cash 期中増減。不一致は Dead（修復禁止） | Phase 2 決算テスト |
| BXS-I-04 | `apply(tx)` は all-or-nothing。オーバーフローで状態不変 | `ledger::tests::overflow_apply_is_atomic` |
| BXS-I-05 | Transaction は balanced-by-construction（不均衡値は存在自体が不可能） | `ledger::tests::unbalanced_rejected` |
| BXS-I-06 | PRNG は Philox4x32-10・内容由来 seed・ドメイン分離のみ。KAT 3 本 + Python 相互 assert | `rng::tests::philox_known_answer_*` / `test_blackbox_sim_contract.py` |
| BXS-I-07 | FSM は §7 決定表の転記のみ。Sealed/Dead は吸収 | `fsm::tests::decision_table_exhaustive` |
| BXS-I-08 | 決定ログ append-only・満杯 reject（silent evict 禁止）・seq 単調 | `telemetry::tests::*` / `ring::tests::try_push_rejects_when_full` |
| BXS-I-09 | バイアス値の確度ゲート未達は N/A。0 捏造禁止 | `bias::tests::insufficient_is_none` |
| BXS-I-10 | 6D 射影は `CalibrationCertificate` なしに呼べない（production 構築経路なし） | `bias::tests::uncalibrated_projection_is_all_na` + 型検査 |
| BXS-I-11 | 壁 W-b: `blackbox_sim` から `analytics::` / `db::` / profile 系への import ゼロ | `test_blackbox_sim_contract.py::test_no_profile_dependency` |
| BXS-I-12 | 権威状態は整数のみ。f64 は SDE ステッパ内部 + 制限演算集合に限定 | `det_math::tests::golden_bits_pinned`（macOS arm64 で実測・釘付け済。3 環境一致は CI 整備後） |
| BXS-I-13 | lane 番号（§10）・ドメイン定数（§3.2）・勘定科目 enum 判別値は恒久固定・再利用禁止 | `bias::tests::lane_numbers_frozen` |
| BXS-I-14 | Genesis 真値はビューモデル・LLM prompt・可視アーティファクトへ不出（壁 W-a） | `blackbox_sim_interop::market_view_never_carries_genesis_truth` + `..._key_set_is_an_exact_allowlist` |
| BXS-I-15 | ActionIntent → Transaction の唯一の経路は ActionCompiler（壁 W-d）。拒否は常に型付きで、**`NoLedgerEffect` へ退化させない**（黙殺された intent は BXS-W-07 と同型の系統的欠測を生む）。P2 で全 intent がコンパイル可能になったため `UnsupportedInPhase1` は廃止、拒否理由は全て業務規則（在庫不足・資金不足・死んだプロジェクト等）| `blackbox_sim_interop::every_intent_now_compiles_and_refusals_stay_side_effect_free` |
| BXS-I-16 | 台帳・firm・journal をまたぐ合成原子性: journal が満杯で拒否したら残高も操業状態も動かない（片側成功の封殺・FLR-3）。複数仕訳を 1 行為として適用する場合は**書く前に journal 容量を予約**する（`Journal::remaining_capacity`）| `action::tests::journal_rejection_rolls_back_every_store` |
| BXS-I-17 | 在庫は「単位数」と「簿価」の二重表現を持ち、両者は 1 minor 単位まで一致する。最終単位の払出は残簿価を**全額**掃き出す（比例配分の丸め残渣を空の品目に残さない）。`units == 0 ⇒ value == 0` | `firm::tests::selling_the_last_unit_sweeps_all_residual_value` / `settle::verify_inventory_reconciled` |
| BXS-I-18 | 勘定 → CF セクション写像は全域（`section_of` にワイルドカード腕なし）。勘定の追加は分類するまでコンパイルが通らない | `settle::tests::every_account_has_exactly_one_home` |
| BXS-I-19 | state digest は権威状態を**漏れなく**覆う: 台帳 + firm + カーネル連続状態 + **全乱数ストリームの位置**（見える数値だけでは、次 tick で分岐する二状態を同一と判定する）| `snapshot::tests::stream_position_is_part_of_the_state` / `..._a_firm_only_change_changes_the_digest` |
| BXS-I-20 | 刺激の**封緘パラメータ**（参照真値 / `delta_micro` / `good_state` / `sunk_minor` / `required_cash_minor` / arm 割付）は公開ビュー `StimulusView` に載らない。被験者に「測っている値」が見えた時点でそれは測定ではない（壁 W-a）| `director::tests::published_views_carry_no_answer_key` + `test_blackbox_sim_contract.py::test_published_stimulus_view_names_no_sealed_field` |
| BXS-I-21 | 永続化ポート `DecisionSink` は**送出専用**。メソッドはちょうど 1 本（`persist`）で、読み取り経路が型として存在しない（壁 W-b を規律ではなく形状で担保）| `director::tests::a_sink_can_only_be_written_to` + `test_blackbox_sim_contract.py::test_the_sink_port_is_write_only` |
| BXS-I-22 | vault v12 の記録 2 表（`blackbox_decisions` / `blackbox_stimuli`）は append-only: 書き込みは `INSERT OR IGNORE` のみ、`UPDATE`/`DELETE` の対象にならない。再 flush は冪等な no-op（第八律）| `db::blackbox_repo::tests::reflushing_the_same_records_cannot_duplicate_history` + `test_blackbox_sim_contract.py::test_the_vault_record_lane_is_append_only` |
| BXS-I-23 | fixture-blindness（§12）: `bias.rs`（推定器）と `phantom_bot.rs`（校正 BOT）は互いに import しない。両方を import してよいのは `calibration.rs` 1 ファイルのみ。BOT が推定器の定数に合わせて書かれた瞬間、校正は同語反復になる | `test_blackbox_sim_contract.py::test_bias_and_phantom_bot_never_import_each_other` |
| BXS-I-24 | `CalibrationCertificate` は production 構築経路を持たない。唯一のコンストラクタ `test_only()` は `#[cfg(test)]` のまま（選択肢 A 裁定、2026-07-27）。校正 suite が全軸 GREEN を返しても、この一線を跨いで証明書を鋳造する経路はコード上存在しない — `calibration.rs` 自体も `#[cfg(test)]` でしか到達できない | `bias::tests::a_passing_calibration_run_does_not_by_itself_mint_a_certificate` + `test_blackbox_sim_contract.py::test_calibration_certificate_has_no_production_constructor` |
| BXS-I-25 | 拒否テレメトリの記録範囲は許可リスト（裁定、2026-07-27）: 「有効な形式の intent が、アクティブな刺激下で、業務ルールにより拒否された」場合のみ `RefusalLog` へ記録する。`classify_refusal` は `_ => None` で終わる allowlist であり、型・形式エラー（`InvalidForecastInterval` 等）は既定で無記録に留まる | `director::tests::a_business_rule_refusal_with_no_active_stimulus_is_not_logged` + `test_blackbox_sim_contract.py::test_refusal_classifier_is_an_allowlist_not_a_catchall` |

### 罠（予測。W-nn は実装より先に読め）

- **BXS-W-01:** `latency_ms` をうっかり難度計算・イベント抽選に使うとリプレイ恒等（BXS-I-01）が静かに死ぬ。レイテンシは観測専用フィールド。
- **BXS-W-02:** f64 の `exp/ln` を「1 箇所だけ」libm で済ませると CI の golden が環境間で割れる。det_math 以外の超越関数は grep で落とせ。
- **BXS-W-03:** 市場リングと決定ログリングの溢れ方針は**逆**（evict 可 vs reject 必須）。共通化した瞬間に第八律違反（LAW-17 善意の一本化）。
- **BXS-W-04:** 「含み損益」は勘定ではなく評価差分。台帳へ時価評価仕訳を自動起票すると CF 整合（BXS-I-03）が壊れる。評価はビュー層の別表で持つ。
- **BXS-W-05:** 刺激の A/B 割付を「プレイヤーに自然に見せる」ための微調整が割付の決定論を破りがち。割付は `DOM_STIMULI` のみから導出し、表示都合での再抽選を禁止。
- **BXS-W-06:** BOT を推定器の期待値に合わせて書くと校正が恒真化する（fixture-blindness）。BOT は決定 API と真値パラメータだけから書け。
- **BXS-W-07:** タイムアウト強制決定を「ユーザー決定が無かった」として無記録にすると、エスカレーション/プレッシャー軸が系統的に欠測する。ForcedDefault も一級の決定イベント。
- **BXS-W-08（実測で踏んだ）:** `det_math.rs` の float 定数に clippy の助言を適用するな。`approx_constant` は `LN2_HI` を `f64::consts::LN_2` へ、`excessive_precision` は桁の切り詰めを勧めてくるが、`LN2_HI` は**意図的に切り詰めた ln2 の上位部**であり、下位ビットが 0 であることによってのみ Cody-Waite の誤差相殺が成立する。従うと全指数計算が静かに劣化する（エラーは出ない）。ファイル冒頭の `#![allow(clippy::approx_constant, clippy::excessive_precision)]` と理由コメントを消すな。
- **BXS-W-09（実測で踏んだ）:** golden ビットは「数学的に正しい値」ではなく「この実装の実測出力」を釘付けせよ。Phase 0 は `det_exp(1.0)` に libm の正確丸め値（= 数学定数 e、`…769`）を書いていたが、実装は設計どおり 1〜2 ULP ずれる（`…768`）ため、モジュールを配線した瞬間 RED になった。**精度は許容誤差テストの責務、golden はドリフト検出の責務** — 役割を混ぜると、将来の本物のドリフトを「また定数がズレてる」と再測定で握り潰す。
- **BXS-W-11:** CF 整合を「NI に減価償却を足して運転資本を引く」と**手で組み立てるな**。勘定を漏れなく 3 セクションへ分割し各セクションを `−Σ Δsigned` で定義すれば、ゼロサム（BXS-I-02）から `Δ現金 = 営業 + 投資 + 財務` が**定理として**従う。手組みは新勘定の追加時に静かに壊れ、しかも「調整項」を足せば数字が合ってしまうため、壊れたことに気付けない。
- **BXS-W-12:** Genesis の抽選は**末尾追加のみ**。中間に 1 本挿すと以降の全パラメータがシフトし、既存キャンペーンが別世界になる（エラーは出ない）。P2 の firm ブロックは market ブロックの後ろに追加し、`market_draws_are_unchanged_by_later_appends` が 19 個の実測値を釘付けして回帰を止めている。
- **BXS-W-13:** 「まだ観測していない市場」をゼロ値の `MarketTickView` で埋めるな（§16.1 の dummy-then-overwrite と同型）。`Session::market()` は `Option` を返す。ゼロ埋めは UI に**捏造された観測**を描かせる。
- **BXS-W-14:** スナップショットは「状態を丸ごと serialize する」コードであるがゆえに、壁 W-a の最大の漏洩面である。表示されるオラクルより永続化されるオラクルの方が有害（セッションを越えて残る）。`snapshots_never_serialize_the_oracle` の走査対象に、新しく永続化するフィールドを必ず含めろ。
- **BXS-W-15:** 世代スナップショットの「最古を捨てる」は正しいが、これは**冗長なチェックポイントに限った特例**である。journal / event log は満杯で reject（第八律）。同じ ring 語彙で書かれているからといって溢れ方針を揃えるな（BXS-W-03 の変奏）。
- **BXS-W-16:** 刺激のカデンツを**プレイヤーの状態に適応させるな**（「資金が潤沢だから賭けを出す」「連敗したから休ませる」）。親切な調整は測定と被測定量を交絡させ、リスク選好の推定値に本人の過去成績を混ぜ込む。出題は tick のみから決まる固定周期・固定位相であり、割付（High/Low・WithSunk/WithoutSunk）は出現順序から決定論的に交互化する（BXS-W-05 の実装形）。
- **BXS-W-17:** 固定容量レジストリ（オファー 8 / プロジェクト 8）に刺激を植え続けると、キャンペーン中盤で**本人の行為が `RegistryFull` で拒否される** — 実測で踏んだ。正しい修理は容量拡大でも記録の削除でもなく、**終端状態（`Accepted`/`Declined`/`Completed`/`Abandoned`）のスロットだけを回収すること**。レジストリは現在の操業資源であって記録ではない。記録は `StimulusLedger` と決定ログに append-only で残り続ける — 資源の回収と記録の削除を同一視した瞬間に第八律違反になる。
- **BXS-W-18:** flush の**部分成功を成功として扱うな**。ack が送出件数と一致しない限りリングは消さず、刺激台帳の watermark も進めない（「失敗 = まだ」であって「失敗 = 諦め」ではない）。ここを緩めると、vault が一時的に落ちていた区間の決定だけが欠測し、しかも欠測は静かなので推定器は「その期間は何もしなかった」と読む。
- **BXS-W-10:** モジュールを `mod.rs` に宣言し忘れると、そのファイルはコンパイルすらされないまま `cargo test` が GREEN を報告する（テストは「0 tests」として静かに素通りする）。Phase 0→1 の引き継ぎで実際に 4 ファイルが未検証のまま「完遂」扱いされていた。新規ファイルを足したら、テスト件数が**増えたこと**を実測で確認しろ（LAW-22）。
- **BXS-W-19（実測で踏んだ）:** PHANTOM-BOT が複数の刺激に「毎ターン 1 つだけ」応答する設計では、**固定カデンツの衝突ターンでの優先順位**が校正結果を静かに決める。`GAMBLE_PERIOD=4,phase=1` と `SUNK_PERIOD=6,phase=3` は 12 tick ごとに衝突し、しかもサンクの出現順アームは常に同じ側（`WithSunk`）に当たる — ギャンブル優先の実装は、何キャンペーン積んでもレーン 4 の片アームを恒久的に 0 件へ飢えさせる（乱数の問題ではなくスケジュールの数論的性質なので、プーリングでは直らない）。処分効果（レーン 1）も同様に、BOT がポジションを一度も開かなければ `DispositionWindow` 自体が一切プラントされない。優先順位は各カデンツの mod-LCM 衝突集合を実際に列挙し、**どのレーンも両アームに使える割り当てが残ること**を確認してから固定しろ。
- **BXS-W-20（実測で踏んだ）:** レーン 3（過信）の「意図的に外す」区間予測で `lo_minor < 0` を生成すると、`ActionCompiler` は `InvalidForecastInterval` で**丸ごと拒否**する。拒否された submission は `DecisionEvent` にならないため、`extract_forecast_trials` からは的中/外れのどちらでもなく**存在しなかったことになる** — 「意図的な外れ」のつもりが「観測の消失」になり、回収率を静かに半減させる（実測: 宣言 40% に対し回収 17.6%）。ずらした区間の下限は事前に非負が保証される構成にしろ（事後の `.max(0)` clamp は下記 BXS-W-21 の別の症状を生む）。
- **BXS-W-21（実測で踏んだ）:** ある推定レーンの「意図的な逸脱」の大きさを、**別レーンの比率の分母と無関係な量**（例: 固定定数、あるいはプレイヤー側の値 `pulled`）で作ると、その別レーンの平均に無制限の分散を持つ外れ値として漏れ出す。レーン 3 の過信オフセットをレーン 2 の分母 `anchor − reference` と無関係なスケールで組んだところ、`ANCHOR_DELTA_LATTICE` の最小デルタを引いたトライアルだけ比率が爆発し、たった数件のプールでレーン 2 の平均を大きく押し流した（符号をどれだけ丁寧に乱数化してもここは救えない — 偏りではなく分散の問題だから）。逸脱の大きさは**隣接レーンが実際に割る分母と同じ量に比例**させ、寄与を符号だけが確率的な有界量に固定しろ。

## 17. フェーズ分割（各フェーズ着工時に FLR 着工宣言 → HARD STOP）

| Phase | 射程 | 検証ゲート |
|---|---|---|
| **P0（本提出）** | SPEC + コアデータ構造（rng/money/ledger/fsm/ring/telemetry/bias）+ ガード | `cargo test --features blackbox-sim blackbox_sim` GREEN / feature 無し build 不変 / `pytest tests/test_blackbox_sim_contract.py` GREEN |
| P1（裁定拡大） | det_math + 市場カーネル + Genesis + leak-guard + **ActionCompiler（壁 W-d パイプライン）+ 相互依存テスト** | golden digest 釘付け（3 環境 bit 一致は CI 整備後）・leak guard GREEN・二重実行 bit 同一・会計不変条件の毎 tick 検証 |
| **P2（完）** | **FirmState** + 決算ループ（CF 整合）+ snapshot/replay + Director 前段 | replay digest 恒等 property・CF 整合・世代 pointer 結合 — 全て GREEN（§20.3） |
| P3 | Director + 刺激 + vault v12 永続化 | §16.3 pointer 結合・append-only 契約 |
| **P4（完）** | 推定器 6 レーン + PHANTOM-BOT 校正 suite | 既知解回収率下限 GREEN — 全 6 軸実測（§22.3） |
| **P5-A（完・backend-first）** | IPC command 層 + vault 永続化配線（FE・フレーバ層は裁定によりこの回は非射程） | `cargo test --features blackbox-sim` 全 GREEN・`pytest tests/test_blackbox_sim_contract.py` 全 GREEN（§23.5） |
| **P5-B（完・FE / 固定テンプレート）** | Coliseum BLACKBOX アリーナ FE（フレーバ層は依然非射程） | `npm run test:boundary` GREEN・`tsc --noEmit` GREEN・契約テスト GREEN（§24.5） |
| P6（未着手） | profile bridge（二重ゲート） | 校正 GREEN ∧ 指揮官裁定 |
| P6 | profile bridge + 憲法 as-built 追記・§0 表更新 | 二重ゲート（校正 GREEN + 裁定）確認後のみ |

## 18. Phase 0 as-built（2026-07-27）

- 実装: `apps/desktop/src-tauri/src/blackbox_sim/{mod,rng,money,ledger,fsm,ring,telemetry,bias}.rs`。lint は `#![deny(clippy::unwrap_used, expect_used, panic, indexing_slicing, string_slice)]`（knowledge/ の確立ヘッダ）。
- 配線: `lib.rs` に feature ゲート 1 行（`#[cfg(feature = "blackbox-sim")] pub mod blackbox_sim;`・doc(hidden)）+ `Cargo.toml` `blackbox-sim = []`（依存追加ゼロ）。既定ビルドは byte 不変。
- command 登録・State 管理・vault migration は**未実装**（P3/P5 の射程。llm モジュール Phase 0「module tree only」の前例に従う）。
- 憲法本文（`AI_SKILLS.md`）への追記は**行っていない** — as-built 追記・§0 タスク表への行追加は裁定後の P6 で実施（統治手続 2「コミットは明示指示のみ」と同じ理由で、未裁定アーキテクチャの憲法編入を自制）。

## 19. Phase 1 as-built（2026-07-27）

### 19.1 射程と実装

- **P1-A 決定論演算器の配線:** `det_math` / `normal` / `genesis` / `market` は Phase 0 の引き継ぎ時点でファイルとしては存在したが `mod.rs` に未宣言 = **一度もコンパイルされていなかった**（BXS-W-10）。4 宣言を追加し初めて実検証。`golden_bits_pinned` が RED になり、原因は実装ではなく未実測の釘付け定数だった（BXS-W-09。実測 exp 4 本 + ln 3 本へ差し替え、恒真ループを削除。指揮官追認済み）。
- **P1-C ActionCompiler（壁 W-d の実体）:** `action.rs`。`compile(intent, &Balances)` は副作用ゼロの読取専用、`execute_intent(intent, &mut Balances, &mut Journal)` が唯一の変異経路。`ActionIntent` の match に**ワイルドカード腕を置かない** — 変種追加はコンパイルエラーで気付かせる（黙って no-op に落ちる経路を型で消す）。
- **Phase 1 の intent 三分類（指揮官裁定 2026-07-27）:** (a) 台帳を動かす = `Borrow` / `Repay` / `Invest`、(b) 証明可能に金銭を動かさない計測行為 = `ForecastInterval` / `Abstain` / `ForcedDefault`、(c) 存在しない操業モデル（SKU 表・オファー台帳・プロジェクト台帳・ポジション簿）を要するもの = `UnsupportedInPhase1 { intent, missing }` で fail-closed。**FirmState の導入は P2 の決算ループと同時**（Phase 1 では壁 W-d の骨格に集中する裁定）。
- **P1-B / P1-D:** `tests/blackbox_sim_interop.rs`（`#![cfg(feature = "blackbox-sim")]`、`render_guard.rs` と同型の外部結合テスト）。7 件 = leak guard 2 / replay 恒等 2 / latency 非入力 1 / 混合セッション試算表 1 / 未実装 intent 拒否 1。

### 19.2 掟（新設・Phase 2 以降も拘束する）

1. **leak guard は「マーカーが genesis 側に実在すること」を先に assert せよ。** フィールド名を rename した瞬間、走査は「存在しない名前を探して必ず PASS する」空虚なテストへ静かに退化する。`market_view_never_carries_genesis_truth` の前段 sanity assert を消すな。
2. **ビューの鍵集合は denylist ではなく exact allowlist で固定する。** 新フィールドの公開は壁 W-a のレビュー事項であり、テストの更新作業ではない（§16.4 の exact-key 規律）。
3. **合成原子性の順序:** staged コピーへ apply → 不変条件検証 → **journal append（最後の可謬ステップ）** → 残高コミット。この順序でのみ「片側成功」が構造的に存在しない。append の後に可謬処理を足すな。
4. **会計不変条件は毎 tick 検証する。** 決算時のみの検証は、壊れた tick ではなく決算 tick を犯人として指す。
5. **`Money` / `TurnPhase` は再エクスポートしていない** — `ledger::Money` ではなく `money::Money`、`telemetry::TurnPhase` ではなく `fsm::TurnPhase` を使え（各モジュールの `use super::` は private import）。

### 19.3 検証ログ（実測）

| ゲート | 実測結果 |
|---|---|
| `cargo test --features blackbox-sim --lib blackbox_sim` | 65 passed / 0 failed（P0 の 37 → +28） |
| `cargo test --features blackbox-sim --test blackbox_sim_interop` | 7 passed / 0 failed |
| `cargo check --features blackbox-sim --all-targets` | warning 0 |
| `cargo check --all-targets`（feature 無し） | Finished・既定ビルド不変 |
| `cargo clippy --features blackbox-sim --all-targets` | `blackbox_sim/` 由来の指摘 0（`src/knowledge/` の 280 件は Phase 1 以前からの既存事象・射程外） |
| `pytest tests/test_blackbox_sim_contract.py` | 5 passed |

### 19.4 未実施・次フェーズへの申告

- **3 環境 bit 一致は未達。** golden は macOS arm64（rustc 1.96.1）の実測のみ。iOS sim / x86_64 での確認は CI 整備が前提であり、P1 の射程内では実施できていない。SPEC §17 の P1 ゲート文言どおり「CI 整備後」の残件。
- FirmState・決算ループ・snapshot/replay・Director・IPC・FE は未着手（P2 以降）。
- `state_digest` の production 実装は未着手 — 現状 `DecisionEvent.state_digest` の書き手は存在せず、相互依存テストはテスト側で独自にダイジェストを組んでいる（意図的: 実装と期待値が同じ関数を呼ぶ恒真比較を避けるため。§16.6）。

## 20. Phase 2 as-built（2026-07-27）

### 20.1 射程と実装

- **P2-A `firm.rs` — FirmState:** SKU 3 品目 / プロジェクト 8 / オファー 8 / ポジション 8 の固定容量レジストリ。全て整数・キャンペーン中の追加確保ゼロ（§13）。`FirmEffect` は非金銭変異の閉じたカタログで、`compile` が生成し `apply_effect` だけが実行する（純粋な計画 + 機械的な適用）。
- **Genesis 拡張:** `FirmParams`（SKU 別 base/参照価格/原価倍率 + 開業資本 + 減価償却/税/シニア/メザニン各 bp）を **market ブロックの末尾に追加**。メザニンは独立抽選ではなくシニア + スプレッドで導出する（独立抽選は資本構造を逆転させ得る）。既存 19 パラメータの不変を実測で釘付け（BXS-W-12）。
- **P2-B ActionCompiler 拡張:** 繰延していた 8 intent が全てコンパイル可能に。`UnsupportedInPhase1` を廃止。`ActionPlan { transaction: Option<Transaction>, firm_effect: FirmEffect }` を返す純関数へ変更し、`SimBooks { balances, firm, journal }` に対して 3 ストア同時に staged-commit する。
- **P2-C `settle.rs` — 操業 + 決算 + CF:** 線形需要（`units = base × index × max(0, 2·ref − price) / (1e6 × ref)`。`powf` による弾性は BXS-W-02 に抵触するため採らない）→ 売上認識と原価振替を**単一仕訳**で計上。四半期 13 tick で決算（減価償却 → 利息発生 → 前払費用償却 → 税）。税額は他 3 項目から**解析的に**先に求め、4 仕訳を 1 原子行為として適用する（半端に適用された決算は「もっともらしい嘘」になる）。
- **P2-D `snapshot.rs`:** `digest_state` は台帳 + firm + カーネル連続状態 + 全乱数ストリーム位置を覆う（BXS-I-19）。世代は append-only・上限 8・1 世代 ≤ 64 KiB。`state_digest`（リプレイ錨）と `payload_digest`（ポインタ-ペイロード結合）を**別フィールド**として持つ — 1 本に畳むと改竄されたペイロードが自分自身と一致してしまう。
- **P2-E `director.rs`:** FSM を駆動する turn driver。1 ターン = 1 決定（§10 の全レーンが「同一決定の反実仮想」比較を要求するため、バッチ投入は測定単位を破壊する）。拒否された intent は決定ではない — Decide に留まりログに入らない。`replay_digest(request, log)` が BXS-I-01 の実行可能形。

### 20.2 掟（新設・Phase 3 以降も拘束する）

1. **恒等式は「合わせる」のではなく「従うように構造を選ぶ」。** CF 整合は勘定の全域分割から定理として出る（BXS-W-11）。それでも受け側検証（`verify()`）は毎決算で実行する（第六律）。
2. **決算エンジンの起票は壁 W-d の穴ではない。** 壁が律するのは**外部起源の intent**であり、減価償却・利息・税は暦と過去の決定の帰結であって決定ではない。ただし `apply_plans` を通すので staging・ゼロサム・恒等式検査は同一に受ける。
3. **digest に含めないものを明記せよ。** レイテンシ・実時刻・市場履歴リング（表示キャッシュで evict する）は**意図的に除外**。含めると「人間が速く押したか」「セッションが長かったか」でリプレイが割れる。
4. **二重表現を持つ量は、どちらか一方を台帳に合わせて動かす。** 在庫は台帳へ計上したのと同じ整数だけ簿価を動かし、最終単位で全額掃き出す（BXS-I-17）。除算で平均原価を再計算する設計は空の品目に残渣を残す。
5. **含み損益は決済時にのみ台帳へ触れる。** ポジションは entry 水準だけを保持し、評価差分は導出ビュー（BXS-W-04 の履行）。
6. **複数仕訳を 1 行為にする時は容量を先に予約する。** staging だけでは、最初の append 成功と最後の append 失敗の間に journal が埋まる経路が残る。

### 20.3 検証ログ（実測）

| ゲート | 実測結果 |
|---|---|
| `cargo test --features blackbox-sim --lib blackbox_sim` | 125 passed / 0 failed（P1 の 65 → +60） |
| `cargo test --features blackbox-sim --lib`（全体） | 336 passed / 0 failed |
| `cargo test --features blackbox-sim --test blackbox_sim_interop` | 10 passed / 0 failed |
| `cargo test --lib`（feature 無し） | 212 passed / 0 failed（P1 と同数・既定ビルド不変） |
| `cargo clippy --features blackbox-sim --all-targets` | `blackbox_sim/` 由来の指摘 0 |
| `pytest tests/test_blackbox_sim_contract.py` | （§20.4 参照） |

### 20.4 未実施・次フェーズへの申告

- **3 環境 bit 一致は依然未達**（P1 からの持ち越し・指揮官裁定により P2 でも CI 整備待ち）。golden は macOS arm64 実測のみ。
- **スナップショットからの復元は未実装。** `GenerationStore` は capture / verify（ポインタ-ペイロード結合）まで。ペイロードを逆直列化して状態を再構築し `state_digest` を再計算する経路は P3（vault v12 永続化）の射程。現状 `Generation` は `Serialize` のみで `Deserialize` を持たない。
- **拒否された intent はテレメトリにも残らない。** リプレイには不要（状態が動いていない）だが、「繰り返し資金不足で弾かれる」は行動として有意。記録するか否かは P3 の刺激設計と併せて裁定を仰ぐ。
- 売上は全額現金取引。売掛・買掛は CF 機構としては全域分類済みだが、操業エンジンはまだ AR/AP を動かさない（信用取引レバーは P3 以降）。
- 刺激プランティング・vault 永続化・IPC・FE・推定器・校正は未着手（P3 以降）。

## 21. Phase 3 as-built（2026-07-27）

### 21.1 射程と実装

- **P3-A `oracle.rs` — L2 封緘の参照評価器:** アンカー刺激には「アンカーがそこからどれだけ離れているか」を測るための真値が要る。`reference_valuation(kernel, ctx, firm)` は 1 tick 先の期待需要指数（`market::oracle_expected_next_demand_micro`、拡散項を落とした drift のみ）から期待収益を組む純関数。戻り値 `ReferenceValuation` は **`Serialize` を実装しない** — 壁 W-a の漏洩面はビューではなく「うっかり derive」なので、型として直列化不能にしてある。実現売上と同じ `settle::demand_units` を共有する（別式で近似すると、真値がゲーム世界とずれた「別の宇宙の正解」になる）。
- **P3-B `stimulus.rs` — 固定カデンツ + 封緘/公開の二型:** 6 種の刺激（GamblePair / AnchorProbe / ForecastElicitation / SunkCostPair / CrisisCountdown / DispositionWindow）を tick のみから決まる周期・位相で出題（BXS-W-16）。パラメータは `DOM_STIMULI` ストリームから抽選し、`StimulusParams`（封緘・正準直列化 + SHA256 の `params_digest`）と `StimulusView`（公開・安全なフィールドのみ）を**別の型**として持つ。同一型に `#[serde(skip)]` を撒く設計は採らない — skip の付け忘れはコンパイルが通り、テストも書き足すまで沈黙する。`StimulusLedger` は上限固定の append-only（満杯なら reject、evict しない）。
- **P3-C Director 統合:** `observe` が市場 step の直後に植え込み、`submit` が `stimulus::attribute(intent, active)` で決定を刺激へ帰属させる。帰属は **intent の形状**から引く（「このターンに賭けが出ていたから賭けへの応答」ではなく「オファー ID が一致する `RespondToOffer` だから」）。応答しなかったターンは無帰属で記録される — 沈黙も決定である（BXS-W-07 の同型）。受諾された賭けは `settle` で `good_state` に従い決着する。プロジェクト枠は終端状態のスロットのみ回収（BXS-W-17）。
- **P3-D `persist.rs` — 送出専用ポート:** 依存を反転させた。`blackbox_sim` は「永続化バックエンドが**何をするか**」だけを宣言し（`DecisionSink::persist` の 1 本のみ）、SQLCipher を知る実装はモジュールの外に置く。simulator は約束に依存し、データベースには依存しない。既定は `NullSink`（テストダブルではなく既定 — バックエンド無しで完全にプレイ可能であること自体が要件）。`flush_to` は ack が全件一致した時にのみリングを空け、刺激台帳の watermark を進める（BXS-W-18）。
- **P3-E vault v12:** 3 表（`blackbox_campaigns` / `blackbox_decisions` / `blackbox_stimuli`）。自由文カラムはゼロなので PII は構造的に着地できない。キャンペーンの鍵は Genesis **fingerprint** であって seed ではない（seed を置けば答えが復元できてしまう）。`channel` 列を全決定行に刻んで壁 W-c を機械化する。`verify_v12_schema` は列集合を**完全一致**で検査する（部分一致にすると、後から足された列が検証を素通りする）。

### 21.2 掟（新設・Phase 4 以降も拘束する）

1. **測定器の出題スケジュールは被験者から独立させよ。** 適応的な出題は親切に見えて交絡因子である（BXS-W-16）。
2. **封緘と公開は「同じ型のフィールド属性」ではなく「別の型」で分けよ。** 属性の付け忘れは静かに通る。型が違えば、公開経路に封緘値を載せるコードはコンパイルしない。
3. **壁は規律ではなく形状で守れ。** 送出専用ポートに読み取りメソッドが無いなら、simulator が profile を読む経路は「書かないよう気を付ける」ものではなく「書けない」ものになる（BXS-I-21）。
4. **資源の回収と記録の削除を混同するな。** レジストリのスロットは有限の操業資源、台帳は聖域。前者は終端状態のみ回収してよく、後者は決して縮まない（BXS-W-17）。
5. **永続化の失敗は「まだ」であって「諦め」ではない。** 部分 ack は失敗として扱い、リングは保持する（BXS-W-18）。
6. **リプレイは残高だけでなく答え合わせ表も再現せよ。** 刺激台帳が再現しないなら、そのキャンペーンの分析はもう検証できない（`replay_reproduces_the_answer_key_not_just_the_balances`）。

### 21.3 検証ログ（実測）

| ゲート | 実測結果 |
|---|---|
| `cargo test --features "secure-vault blackbox-sim" --lib blackbox` | 165 passed / 0 failed（P2 の 125 → +40） |
| `cargo test --features blackbox-sim --test blackbox_sim_interop` | 10 passed / 0 failed |
| `cargo clippy --features "secure-vault blackbox-sim" --lib --tests` | lib 警告 50 件 = P3 着工前と同数・`blackbox_sim/` および `db/blackbox_repo.rs` 由来の指摘 **0** |
| `pytest tests/test_blackbox_sim_contract.py` | 11 passed（P2 の 7 → +4: 公開ビュー封緘 / 送出専用ポート / append-only 記録レーン / seed 不保持） |

### 21.4 未実施・次フェーズへの申告

- **3 環境 bit 一致は依然未達**（P1 からの持ち越し。CI 整備待ち）。
- **Tauri command 層への配線は未着手。** `VaultDecisionSink` は本モジュールのテストで端から端まで動いているが、production の呼び出し元は P5 の射程。`db/mod.rs` の `#[allow(dead_code)]` はそのための一時措置であり、command が着地したら外す。
- **スナップショットからの復元は依然未実装**（P2 からの持ち越し）。v12 は決定ログと刺激台帳を持つが、`Generation` ペイロードの逆直列化経路は未着手。
- **拒否された intent はテレメトリに残らない**（P2 からの持ち越しの裁定事項）— **P4 で裁定・実装済み。** §22 参照（`RefusalLog` / BXS-I-25）。
- IPC・FE・フレーバ層は未着手（P5 以降）。

## 22. Phase 4 as-built（2026-07-27）

### 22.1 裁定記録（2026-07-27・指揮官、founding directive に追加）

Phase 4 着工にあたり指揮官から 2 件の裁定を得た。§0 の裁定台帳と同じ扱いとしてここに記録する。

- **R-5（拒否テレメトリの記録範囲）:** 「刺激の配賦下における業務ルール拒否（有効な形式だがリソース不足や条件不整合で弾かれた試行）」に限り、専用のフラグ付きイベントとして決定ログに記録する。プレイヤーがプレッシャー下・刺激の誘引下で「不可能な行動を連続して試みる」行動パターン自体が、レーン 4（エスカレーション）とレーン 5（プレッシャー下劣化）の測定に価値の高いシグナルであるため。不正な構造・型エラーの試行はログを汚染させない — 実装は `classify_refusal` の allowlist（BXS-I-25）。
- **R-6（校正証明書、選択肢 A 採用）:** `CalibrationCertificate` の型レベル封印（uninstantiable）を完全維持し、`#[cfg(test)]` 等の迂回路も作らない。P4 の校正 suite は真値と応答のペアから推定器の回収率（6 軸）を測定し、テストランナーに GREEN/RED を返す「検証装置」としてのみ機能する。証明書の鋳造は行わない（実装は BXS-I-24）。

### 22.2 射程と実装

- **P4-A `oracle.rs` 拡張 — レーン 5 のオラクル:** `pricing_optimality_gap(kernel, ctx, sku, chosen_price)` が `settle.rs` の線形需要モデルから利益最大化価格を整数演算のみで導出する（`PricingOptimum { optimal_price_minor, optimal_profit_minor, actual_profit_minor, gap_minor }`）。連続最適解の周囲 2 点 + 価格域の両端を整数評価して比較する閉形式であり、反復ソルバーではない。`action::unit_cost_minor` を `pub(super)` に開放し、同じ原価式を共有する（別式で近似すると、レーン 5 が「別の宇宙の正解」と比較することになる — P3-A の `reference_valuation` と同じ理由）。
- **P4-B `telemetry.rs` 拡張 — 拒否テレメトリ:** `RefusalReason`（enum, 判別値固定）+ `RefusalEvent` + `RefusalLog`（上限固定・append-only・満杯 reject）。R-5 裁定の実装。決定ログ（`EventLog`）とは別レーンであり、記録と分析の分離（第五律）を保つ。
- **P4-C `director.rs` 配線:** `classify_refusal(CompileError) -> Option<RefusalReason>` は許可リスト（BXS-I-25）。`submit_inner` が `execute_intent` の失敗時にこれを呼び、アクティブな刺激がある場合のみ `Session.refusals` へ記録する。`Session` に `pricing_trials: FixedRing<PricingTrial>` を追加し、`SetPrice` が成功するたび `pricing_optimality_gap` を呼んでレーン 5 のサンプルを都度記録する — レーン 5 だけは固定カデンツの刺激参照を持たないため、他の 5 レーンのように決定ログから事後に掘り出す経路がない。
- **P4-D `bias.rs` — 6 レーン推定器:** 各レーンは「決定ログ + 刺激台帳 + オラクル別表」だけを読む純関数（`extract_*_trials` → レーン関数）。レーン 0（損失回避）は 9 点の固定格子上でのグリッド探索・同点は小さい側（決定論 MLE、§4.15 の確立形）。レーン 1（処分効果）・4（エスカレーション）は率の差、レーン 2（アンカリング）は比率の平均、レーン 3（過信）は区間外れ率、レーン 5（プレッシャー下劣化）は最適性ギャップの群間差。`estimate_profile` が複数キャンペーンのトライアルをプールしてから 6 レーンをまとめて評価する（レーン 1・5 は単一キャンペーンでは `min_n` に届かないことがある — §12 のプーリング規定）。
- **P4-E `phantom_bot.rs` — fixture-blind BOT（新規、`#[cfg(test)]`）:** `PhantomBotConfig` が 6 レーン分の真値パラメータを持つ。`decide()` は毎ターン、公開 `StimulusView` と自前の `PhiloxStream`（推定器と独立したドメイン分離ストリーム）だけから 1 つの `ActionIntent` を選ぶ。アンカリング/過信/価格付けの真値比較には `oracle::reference_valuation` / `pricing_optimality_gap` を呼ぶ — これは Director 自身が刺激をプラントする際に行う L2 内部読み取りと同じ特権であり、壁 W-a の新たな侵犯ではない（公開しているのは `ActionIntent` という同じ閉じた語彙のみ、壁 W-d）。`bias.rs` は一切 import しない（BXS-I-23）。
- **P4-F `calibration.rs` — 既知解校正 suite（新規、`#[cfg(test)]`）:** `bias` と `phantom_bot` の両方を import できる唯一のファイル。各レーンについて既知の真値を持つ BOT で `CAMPAIGNS_PER_BOT=10` キャンペーンをプレイし、`estimate_profile` の回収値が許容帯に入ることを確認する。証明書は鋳造しない（R-6 / BXS-I-24 — `a_passing_calibration_run_does_not_by_itself_mint_a_certificate` が全 6 レーン GREEN の直後でもこれを確認する）。

### 22.3 回収率下限（実測・§12 のゲート値）

| lane | 軸 | 判定方式 | 許容誤差 |
|---|---|---|---|
| 0 | `loss_aversion` | 完全一致（格子上のλは決定論 MLE で一意に決まる） | 0（exact） |
| 1 | `disposition_effect` | 宣言ギャップとの絶対誤差 | ±150,000 micro |
| 2 | `anchoring` | 宣言 pull との絶対誤差（0 / 0.5 / 1.0 の 3 点） | ±20,000〜40,000 micro |
| 3 | `overconfidence` | 宣言外れ率との絶対誤差 + 隣接レーン非汚染チェック | ±150,000 micro |
| 4 | `escalation_commitment` | 宣言バイアスとの絶対誤差 + 符号 | ±200,000 micro |
| 5 | `pressure_degradation` | 符号のみ（真の最適解の tick 間ドリフトにより厳密値は定義できない） | 符号一致 + 無反応 BOT で ±300,000 micro 以内 |

`min_n` はレーンごとに固定（`MIN_N_LOSS_AVERSION=12` 等、§10 表に同じ）。全テストは `CAMPAIGNS_PER_BOT=10` キャンペーンのプールでこの下限を満たすことを併せて確認する。

### 22.4 掟（新設・Phase 5 以降も拘束する）

1. **BOT の優先順位は固定カデンツの衝突集合を数論的に検算してから決めよ。** 「賭けを先に処理する」のような自然な順序が、特定のレーンの片アームを恒久的に飢えさせることがある（BXS-W-19）。プーリングは乱数由来の分散は均すが、スケジュールの構造的な衝突は均さない。
2. **推定レーンの「意図的な逸脱」は、行為が拒否されないことを構成的に保証せよ。** 事後の値クランプは「観測されたが外れた」ではなく「観測が消えた」を生み、回収率を静かに下げる（BXS-W-20）。
3. **ある逸脱の大きさは、それを読む比率の分母と同じ量に比例させよ。** 分母と無関係な大きさは、符号をどれだけ乱数化しても分散で近隣レーンに漏れ出す（BXS-W-21）。
4. **回収率下限は「完全回収」を要求するな。** 雑音耐性こそが検定力の定義であり、下限を 100% に強化した瞬間、次に本物のバイアスが乗ったときの検定力を失う（§12）。
5. **証明書の型的封印は、校正が GREEN になっても緩めるな。** 実装が「動く」ことと指揮官の裁定は別のゲートであり、二重ゲートの片方を通っただけでもう片方を省略するコードを書くな（R-6 / BXS-I-24）。

### 22.5 検証ログ（実測）

| ゲート | 実測結果 |
|---|---|
| `cargo test --features blackbox-sim --lib blackbox_sim::` | 200 passed / 0 failed（P3 の 165 → +35: bias 推定器・phantom_bot・calibration・refusal telemetry・lane 5 配線） |
| `cargo test --features blackbox-sim`（lib 全体 + 全 integration crate） | 412 passed / 0 failed（lib）+ 10 passed（`blackbox_sim_interop`）+ 既存 crate 群、全 GREEN |
| `cargo clippy --lib --features blackbox-sim -- -D warnings`（`blackbox_sim` 由来分のみ） | 0 件（他モジュールの既存指摘とは独立に確認） |
| `cargo check --features blackbox-sim --target aarch64-apple-darwin` | GREEN |
| `pytest tests/test_blackbox_sim_contract.py` | 14 passed（P3 の 11 → +3: fixture-blindness 走査 / 証明書封印検査 / 拒否 allowlist 検査） |

### 22.6 未実施・次フェーズへの申告

- **3 環境 bit 一致は依然未達**（P1 からの持ち越し。CI 整備待ち）。
- **Tauri command 層・FE・フレーバ層は未着手**（P5 の射程）。校正 suite は `#[cfg(test)]` のみに存在し、production からは推定器そのものが呼ばれていない — `Session::refusals()` / `events()` / `pricing_trials()` は現状 `#[allow(dead_code)]`（P5 で `estimate_profile` への実配線を行うまでの一時措置）。
- **6D 射影の重み定数は依然未凍結**（Phase 6 の射程、§11 二重ゲートの片方のみ通過した状態）。
- **スナップショットからの復元は依然未実装**（P2 からの持ち越し）。

**訂正（P5 着工時）:** 上記 2 点目の「P5 で `estimate_profile` への実配線を行うまでの一時措置」という見立ては、P5 着工時の指揮官裁定で更新された。P5 は明示的に **backend-first**（IPC command 層 + 永続化配線のみ）に絞られ、`estimate_profile` の production 配線と `Session` 推定器アクセサの解放は **P6 に据え置かれた**（§11 の二重ゲートは校正 GREEN と裁定の両方が要る一方、P5 完了時点ではまだ裁定を得ていない）。詳細は §23 as-built を参照。

## 23. Phase 5 as-built（2026-07-27・backend slice のみ）

### 23.1 裁定記録（2026-07-27・指揮官、射程確認）

Phase 4 完遂の正式承認・コミット指示（`feat(blackbox_sim): complete Phase 4 ...`）に続き、指揮官から Phase 5 着工許可と射程確認を得た。§0 の裁定台帳と同じ扱いとしてここに記録する。

- **射程は backend-first に確定:** SPEC §15 の 6 Tauri command + `VaultDecisionSink` の production 配線のみ。`apps/desktop/src/`（FE）は本フェーズでは無改造 — Coliseum BLACKBOX アリーナ UI は follow-up 裁定の射程。
- **LLM フレーバ層（§14）は明示的に非射程（延期ではなく今回の対象外）:** `FlavorRequest`/`flavor_guard` は本フェーズで一切実装しない。シミュレータは固定テンプレート文言のみで完全にプレイ可能な状態を保つ。
- **`bias::estimate_profile` の production 配線は明示的に非射程（延期ではなく別フェーズの管轄）:** §11 は「校正 GREEN ∧裁定」の二重ゲートを P6 に割り当てている。本フェーズは決定・刺激データの永続化配線（vault への書き込み経路）を対象とし、推定器ブリッジそのものには一切触れない。`Session::events()` / `refusals()` / `pricing_trials()` は P4 と同じく `#[allow(dead_code)]` のまま — 校正 suite（`#[cfg(test)]`）以外に呼び出し元を作らない。

### 23.2 射程と実装

- **新モジュール `blackbox_arena/`（`mod.rs` / `view.rs` / `handle.rs` / `commands.rs`）:** `blackbox_sim` と `db` の**両方**を import してよい 1 つ目の新規ファイル群（`db/blackbox_repo.rs` に続く 2 例目。fixture-blindness の `calibration.rs` と同じ「両側を跨いでよい 1 箇所」パターン）。`blackbox_sim` 自体は本フェーズで新規 import ゼロ・BXS-I-11 のゼロ import 走査は無改造のまま GREEN。`lib.rs` に `#[cfg(feature = "blackbox-sim")] mod blackbox_arena;` を追加。
- **`BlackboxSimHandle`（`handle.rs`）— 専用ワーカースレッド（SPEC §13 準拠、`db::worker::VaultHandle` と同型）:** `HashMap<String, Session>`（`campaign_id` = fingerprint の hex エンコード、`hex::encode`/`decode` は既存依存を再利用・新規依存ゼロ）を単一スレッド上で排他所有する。`SimRequest::{Start, GetView, SubmitDecision, Advance, Abort, LoadGeneration}` は `SyncSender<SimReply>` を伴うメッセージパッシング（`VaultRequest`/`VaultReply` と同一の idiom）。`secure-vault`/Apple が無いビルドでも `BlackboxSimHandle::spawn()` は無条件に構築可能（「バックエンド無しでもプレイ可能」が固い要求のため）— vault 能力は `attach_vault` で後付けし、`Arc<Mutex<Option<VaultHandle>>>` として保持する。
- **`view.rs` — FE 向け DTO 群:** `ArenaDifficulty`（`blackbox_sim::genesis::Difficulty` が `Serialize` のみ持つため、`Deserialize` 可能な閉じた双子として新設。`blackbox_sim` 自体には新規 derive を足さない）、`StartCampaignRequest`（`#[serde(deny_unknown_fields, rename_all = "camelCase")]`）、`ObservationView` / `DecisionOutcomeView` / `PeriodCloseView` / `AdvanceView`（すべて BXS-I-20 の最小表示集合規律を継承 — 封緘パラメータを一切含まない）、`SimUiErrorCode`（境界を跨ぐ唯一の閉じたエラー型。詳細な `DirectorError`/`SinkError`/`VaultErrorCode` は `eprintln!` でサーバ側に残してから畳む — 2026-07-24 のコンテキスト予算教訓と同じ「生エラーを握り潰すな」規律）。
- **`commands.rs` — 6 個の `#[tauri::command]`:** `bxs_start_campaign` / `bxs_get_view` / `bxs_submit_decision` / `bxs_advance` / `bxs_abort` / `bxs_load_generation`。全て `async fn` + `State<'_, BlackboxSimHandle>` + `spawn_blocking`（`analytics::commands` と同型）。`BlackboxSimHandle` が vault 能力を内部に持つため、command 側は `State<'_, VaultHandle>` を別途要求しない（`secure-vault` の有無でシグネチャが割れるのを避ける設計）。
- **永続化の配線先の変更（計画からの是正）:** 当初計画は `VaultDecisionSink`（`&Transaction` を borrow する既存の型）をそのまま production から呼ぶ想定だったが、`Session` はシムワーカースレッド、`Transaction` は vault ワーカースレッドに存在し、両者が同一コールスタックに乗ることは production では起こらない。実装は代わりに **owned-clone リレー**を採用: シムワーカー側で新設の `CollectingSink`（`DecisionSink` 実装、`persist()` 内で `DecisionBatch` を owned `Vec` へ複製し、`VaultHandle::blackbox_flush` を呼んで vault ワーカースレッドへブロッキング送信、そこで初めて `Transaction` を開いて `blackbox_repo::upsert_campaign` → `persist_batch` を実行し、結果を `flush_to` の呼び出し元へ送り返す）でスレッド境界を越える。`VaultDecisionSink` 自体は「`Session` と `Transaction` が同一コールスタックに乗る」場合にのみ有効な型として意味を保つため **`#[cfg(test)]` に限定**し、ドキュメントコメントを production 非経路である旨に更新した（既存の `db::blackbox_repo` テストからは変更なく呼べる）。
- **`db/worker.rs` の拡張:** `VaultRequest::BlackboxFlush { campaign, created_date, events, stimuli, now, reply }` / `VaultReply::BlackboxFlush(...)` を追加（`gap_analysis_insert` と同一 idiom）。ディスパッチ腕は 1 トランザクション内で `upsert_campaign` → `persist_batch` をコミットする。`VaultHandle::blackbox_flush(&self, ..., now: i64)` は `CollectingSink::persist()` が `SystemTime::now()` から生成した壁時計を運ぶだけで、シミュレーション入力には一切ならない（BXS-I-01 と同じ理由でシムワーカー内部は壁時計を読まない設計を維持）。
- **再開経路 `bxs_load_generation`:** `db/blackbox_repo.rs` に新設した読み取り専用ヘルパ `load_campaign_and_decisions`（`DecisionSink` へのメソッド追加ではない — `persist.rs` 冒頭のコメントが明示する「読み取りが要るなら sink にメソッドを足すな、別ローダーを書け」を踏襲）が fingerprint からキャンペーン行 + 決定ログ全件を読み出し、`replay_session` で要求された世代境界まで再生する。
- **`db/mod.rs` から `blackbox_repo` の `#[allow(dead_code)]` を撤去**（本フェーズで実呼び出し元 `blackbox_arena`/`db::worker` を得た）。`Session::events()` / `refusals()` / `pricing_trials()` の 3 アクセサは §23.1 の裁定どおり無改造（`#[allow(dead_code)]` のまま）。

### 23.3 掟（新設・実測で踏んだ）

1. **世代境界の可用性判定は「観測数」で数えよ。「最大 tick 値」で数えるな。** `market.rs::step()` は 0-index の tick を返す（第 1 四半期は tick 0〜12）ため、「四半期が閉じた後の最大 tick / TICKS_PER_QUARTER」は 12/13=0 となり、**閉じたばかりの世代 0 自体が「存在しない」と誤判定される** — off-by-one がエラーを出さずに正当な再開要求を `GenerationNotFound` へ落とす。決定ログの**件数**（1 tick = 1 決定、`ForcedDefault` が欠落を埋めるため常に成立）で割れば、13/13=1 となり正しく判定できる。この実装ミスは `blackbox_arena::handle` の単体テスト（下記 §23.5）を書く過程で実測発見・修正した — production コードを書いた直後の自作テストが「一度も検証されていない仮定」を洗い出した実例。
2. **借用トレイトでワーカースレッド境界を越えるときは、owned-clone リレーを対称に設計せよ。** `DecisionSink` は意図的に「送出専用・1 メソッドのみ」という**形状**で壁 W-b を担保している（BXS-I-21）。この形状を production の別スレッドまで持ち越そうとして `&Transaction` を跨がせるのは筋が悪い（ライフタイムがスレッドを跨げない）。正しい設計は「境界のこちら側で owned データへ複製し、あちら側で初めて借用を作る」— `CollectingSink` → `VaultHandle::blackbox_flush` → vault ワーカー内の `Transaction` という 3 段リレーは、`flush_to` の「ack が一致するまでリング未消去」という契約（BXS-W-18）をスレッド境界を挟んでも保ったまま満たす。
3. **「まだ vault に一度もフラッシュされていないキャンペーン」と「要求された世代がまだ存在しない」は別のエラーであり、混同するとデバッグ時間を溶かす。** `start()` はキャンペーン行を vault へ即座に登録しない（best-effort 登録は flush 時のみ、§13 のコメントどおり）。したがって 1 四半期も閉じていないキャンペーンへの `bxs_load_generation` は `CampaignNotFound`（vault に行自体が無い）であり、`GenerationNotFound`（行はあるが要求世代が無い）ではない。テストを書く際にこの 2 つを取り違えると「バグを直したはずなのに別のケースで落ちる」を繰り返す（§23.5 の実測で 2 パターンとも踏んだ）。

### 23.4 検証ログ（実測）

| ゲート | 実測結果 |
|---|---|
| `cargo test --features blackbox-sim --lib` | 415 passed / 0 failed（P4 の 412 → +3: `blackbox_arena::handle::tests` の非 vault 系列） |
| `cargo test --features blackbox-sim,secure-vault --lib` | 534 passed / 0 failed（`blackbox_arena::handle::tests::with_vault` 系列 5 本を含む） |
| `cargo test --features blackbox-sim,secure-vault,pocket-brain`（lib 全体 + 全 integration crate + doc-test） | 737 passed（lib）+ 全 integration crate（`blackbox_sim_interop` 10 本含む）+ doc-test 8 本、全 GREEN |
| `cargo check --features blackbox-sim` / `,secure-vault` / `,secure-vault,pocket-brain` / 既定（feature 無し） | 全 GREEN（`blackbox_arena`/`db` 由来の新規 warning ゼロ） |
| `cargo clippy --all-targets --features blackbox-sim,secure-vault,pocket-brain` | `knowledge/` 系のテストコードに既存の pre-existing lint 債務 280 件（`fact_merge.rs`/`edinet_*.rs` 等、`--all-targets` でのみ露出・本フェーズの変更と無関係。`blackbox_arena`/`db/worker.rs`/`db/blackbox_repo.rs` 由来の error は 0 件と個別確認済み） |
| `pytest tests/test_blackbox_sim_contract.py` | 17 passed（P4 の 14 → +3: §7 「Phase 5 IPC boundary guards」— dual-import 走査 / 閉じた request 型走査 / 推定器二重ゲート回帰ガード） |

### 23.5 未実施・次フェーズへの申告

- **Coliseum BLACKBOX アリーナ FE** は §24 で完遂。**LLM フレーバ層（§14）は依然未着手**（固定テンプレートのみでプレイ可能）。
- **`bias::estimate_profile` の production 配線・`blackbox_profile.v1` 書き込みは依然未着手**（§11 二重ゲートの片方＝校正 GREEN のみ通過。裁定は P6 まで得られていない）。`Session` の 3 アクセサは `#[allow(dead_code)]` のまま。
- **3 環境 bit 一致は依然未達**（P1 からの持ち越し。CI 整備待ち）。
- **スナップショットからの復元は依然未実装**（P2 からの持ち越し。`bxs_load_generation` は replay 経由の再構築であり、スナップショットのデシリアライズではない）。

## 24. Phase 5-B as-built（2026-07-28・Coliseum BLACKBOX Arena FE）

### 24.1 裁定記録（2026-07-28・指揮官）

- **マウント:** `ColiseumRoot` をアリーナ選択シェルに再構成し、`.gd-frozen` 3 層を GD 枝の内側へ移設。BLACKBOX は同じシェル配下でライブ描画（R-4 準拠）。
- **再開:** v1 はプロセス内 1 セッションのみ。`bxs_load_generation` は FE 未結線（6 本中 5 本を結線）。
- **フレーバ層:** 依然非射程。固定日本語テンプレート + 返却数値のみ。

### 24.2 射程と実装

- **`ObservationView` 最小拡張（`blackbox_arena/view.rs`）:** `BooksView`（SKU / active projects / open offers / open positions + cash/debt）と `ArenaLimitsView`（価格・注文・金額・SKU・キャンペーン tick 上限を同送）を追加。重複していた top-level `cash_minor` は `books.cash_minor` に一本化（BXS-I-20）。`blackbox_sim` は無改造 — 公開 accessor のみ使用。壁 W-a: 帳簿はプレイヤー自身の帰結のみ。
- **FE IPC:** `lib/parseBlackboxArena.ts`（exact-key 鏡像）+ `lib/blackboxArena.ts`（唯一の `invoke<unknown>` owner）+ `lib/blackboxIntent.ts`（12 種ワイヤビルダ、timeout-default 構築経路なし）+ `lib/blackboxArenaReducer.ts`（純 reducer）。
- **UI:** `BlackboxArena.tsx` コンテナ + Setup / MarketRail / BooksPanel / StimulusDeck / CommandConsole / TurnLog。`latency_ms` は観測描画完了→EXECUTE の `performance.now()` 差分。待機中も入力は `disabled` にしない（`BUS BUSY` アンビエント）。
- **エラー:** `BXS_ARENA_REJECTED` / `BXS_ARENA_STATE` / `BXS_ARENA_FAULT` の固定文言キー。

### 24.3 掟（新設）

1. **「プレイ可能」は市場観測だけでは足りない。対象 ID を持つ intent があるなら、その ID を選ぶための帳簿ビューを同送せよ。** `ObservationView` が cash/market/stimuli だけだと `ContinueProject` / `ClosePosition` / `AcceptOffer` 等が対象を選べず、推定器レーンが静かに餓死する（lane 1 / 3 / 4）。
2. **限界値定数を FE にハードコードするな。観測と一緒に `ArenaLimitsView` を同送し、ビルダはそれだけを読め。** 数値発明ガードはフレーバ層（§14）と同型の FE 側拘束である。
3. **凍結オーバーレイは「アリーナ単位」で掛けよ。シェル全体を凍らせると、同じシェル配下の新アリーナまで操作不能になる。** GD COMING SOON は GD 枝に閉じ、BLACKBOX は外に出す。

### 24.4 検証ログ

| ゲート | 結果 |
|---|---|
| `cargo check/test --features blackbox-sim` (+ `secure-vault`) | GREEN |
| `npx tsc --noEmit`（apps/desktop） | GREEN |
| `npm run test:boundary` | GREEN（`blackbox_arena_boundary` 8 本含む） |
| `pytest tests/test_blackbox_sim_contract.py` | GREEN（FE ガード追加） |

### 24.5 未実施・次フェーズへの申告

- **LLM フレーバ層（§14）** — 未着手。
- **`bxs_load_generation` FE 結線 / 保存キャンペーン一覧** — 再開 UX は follow-up。
- **`estimate_profile` production 配線** — P6。
