# SPEC_ECHO_GENESIS.md — Target Echo 完全設計仕様書
# (時系列直交結合マトリクス / 敵対的デジタルツイン / Hyper-Personalized Oracle)
# 発行: 2026-07-07 / 起草: fable5 (リード・アーキテクト) / 実装担当: Sonnet5
# Rev.1
# Rev.2 (2026-07-07): E0 完遂を受けたアーキテクチャ・レビュー。§5.1.1 (E1 への
#   最終警告 — C++/Python 境界の規約) を追加。§5.5-4 (test_oracle.py の
#   ゲート付き SKIP 化 — Rev.1 の「スキップ化禁止」指示の訂正) を追加。
# Rev.3 (2026-07-07): E1 完遂レビュー。§5.10 (3 拡張の裁定 / E1.1 是正指令 /
#   E2 への最終警告 W-7〜W-14) を追加。E2 着手は E1.1 完了が前提条件。
# Rev.4 (2026-07-07): E3 事前警告 (§5.10.4, W-15〜W-21)。§3.5 の IRLS 更新式を
#   訂正 (リッジは Hessian だけでなく勾配にも入れる — 完全分離下の発散防止)。
# Rev.5 (2026-07-07): E3 完遂を受けた統合裁定 (§5.10.5)。実装順序 = E4 先行・
#   Foxtrot 後続。E4 の境界防衛規律と実装ノート (payload/report 分離・schema
#   追従・dyad 延期・実測義務・store ライフサイクル) を追加。
# Rev.6 (2026-07-08): §4/§4.1/I-22 の記載訂正。「oracle 出力は consult 動的
#   サフィックス」は誤りで、as-built は _gap_section と同居する**静的
#   プレフィックス** (profiler 再実行時のみ更新されるため KV 効率が高い)。
#   E4 as-built (docs/AI_SKILLS.md §12) との不整合を解消する未処理債務の
#   返済 (IMP-1/IMP-2 完遂・E4 正式クローズ後の負債整理)。

> **読者への前提命令**: 本書を読む前に `docs/AI_SKILLS.md` を全文読め (第0原則)。
> 不変条件については AI_SKILLS.md が正、Echo の未実装設計については本書が正。
>
> **Sonnet5 への絶対拘束 (Sonnet-Control Preamble)**:
> 1. 本書に書かれていない設計判断を自分で行うな。曖昧さを発見したら実装を止めて
>    質問しろ。「たぶんこうだろう」で書いたコードは全て破棄対象である。
> 2. 本書の struct レイアウト・数式・定数・シグネチャは**一字一句が仕様**である。
>    「もっと良い書き方」への変更は Architect's Note なしには許されない。
> 3. 実装順序 (§5.9) と各段のテストゲートは強制である。ガードが先、機能が後。
>    RED を確認していないガードは存在しないのと同じ。
> 4. scipy / 新規外部ライブラリの導入は禁止 (§2 Note 2)。numpy は pipeline 層のみ。
>    core の決定論ロジックは stdlib 縛り (I-1) を継承する。
>
> **実装状態の宣言**: Target Echo (E0〜E5) — **本書が設計。全て未実装。**
> Alpha/Bravo/Charlie C1/Delta DL1・DL2・D3 は実装済み (AI_SKILLS §8〜§11)。
> 再実装禁止。Echo はそれらを「呼ぶ」ことだけが許される。

---

# §1【Fable's Internal Monologue】自己フィードバックループの公開

設計の最終形だけを渡すと、Sonnet5 も未来の人間も「なぜこの形か」を再導出できず、
善意の改悪 (=退化) を起こす。よって棄却した案と自己破壊テストの過程を先に置く。

## 1.1 第一の危機: 「テンソル」という言葉に酔うな

初期仮説は「異種データの疎な高階テンソル」を示唆していた。自問した —
**軸は何本必要か？** 時間 × 特徴量 × ドメイン × dyad … と数え始めた時点で手が
止まった。ドメインは特徴量の属性であって軸ではない。dyad はスコープであって
軸ではない。残るのは **時間 × 特徴量の密な 2 階行列**だけである。
N=3,650 日 (10年) × F=32 特徴量 × 4B = **467KB**。疎行列 (COO/CSR) の間接参照は
このサイズでは複雑さしか買わない。高階テンソル案は**棄却**。
「テンソル」の名は残すが、実体は「日次×特徴量の密行列 + 欠測マスク」であり、
それがこの問題の正しい形である。数理的野心は次元数ではなく **(a) 欠測を正直に
扱うマスク意味論、(b) ラグ付き結合の帰無分布を自前で持つこと** に投資する。

## 1.2 第二の危機: このメモリレイアウトでキャッシュミスは起きないか

- 行 (=1日) は 136B = 2.125 キャッシュライン。行を 192B (3 ライン) にパディング
  する案を検討 → **棄却**。支配的アクセスパターンは「連続日ウィンドウの全特徴量
  スキャン」であり、シーケンシャル読みはハードウェアプリフェッチが吸収する。
  列抽出 (1 特徴量の全履歴) はストライド 136B だが、numpy が列を一度 O(N) で
  連続バッファへ取り出してから計算する (467KB は L2 に収まる)。パディングは
  ファイルを 41% 太らせるだけで計測可能な利得がない。**計測なしの最適化は禁止**
  (AI_SKILLS §3.3-2) を自分にも適用する。
- 全行は 8 バイト整列 (header 64B + 136×i、136 ≡ 0 mod 8)。f32 レーンは 4B 整列。
  AArch64 の NEON は非整列ロード可だが、整列は Bravo scratch と同じ規律で守る。

## 1.3 第三の危機: この数理モデルは本当に決定論的か — 監査リスト

「モンテカルロ」と「決定論」は矛盾しない。**乱数の排除ではなく種の支配**である。
自己監査で洗い出した非決定論の侵入口と、その封鎖:

1. 乱数 → 全て `np.random.Generator(np.random.Philox(seed))`。seed は入力内容の
   blake2b から導出 (§3.6)。同一入力 → ビット同一の乱数列。unseeded RNG = バグ。
2. dict 順序 → 特徴量レジストリはレーン番号固定 (§5.2)。ペア走査は添字昇順のみ。
3. 分位点 → `np.quantile(..., method="linear")` を明示固定 (デフォルト変更耐性)。
4. ソート → `np.argsort(kind="stable")` のみ。タイ処理は平均ランク (§3.2)。
5. 並列リダクション → E5 (C++ 化) まで存在しない。E5 で OpenMP を使うなら固定順
   ツリーリダクション必須 (自由順 reduction は f32 で実行毎に最下位ビットが揺れる)。
6. 浮動小数の環境差 → FFT のビット同一性は同一環境内でのみ保証。テストは
   Bravo の教訓 (f32 の == 比較禁止) を継承し `round(x, 6)` 比較とする。
   **不変条件は「同一環境・同一入力 → 同一出力」であり、クロスプラットフォームの
   ビット同一ではない。**

## 1.4 第四の危機: 全知のツインは接待化する (引き継ぎ義務 3 への回答)

面接官に gap を渡すと接待になる (I-14)。ツインにも同型のリスクがある —
**in-sample で完璧に「予測」するツインはトートロジーであり、その介入指示は
過去の要約を未来の命令に偽装したものにすぎない。**
封鎖は 2 枚: (a) ツインの入力は無菌の数値テンソルのみ (生テキスト・gap の
insight・引用は構造的に到達しない §4.2)、(b) **walk-forward 検証で out-of-sample
予測スキル (BSS ≥ 0.05) を実証できないツインには介入を出す資格を与えない**
(スキルゲート I-20。MIN_EXCHANGES / data_sufficiency と同じ「確度の自己申告」思想)。
全知を防ぐのではなく、**未検証の全知に発言権を与えない**。

## 1.5 第五の危機: Oracle は操作装置に堕ち得る (Target B の自己破壊テスト)

初期仮説の「レイテンシを遅延させよ」を字義通り実装すると、「相手の関心を最大化
するために返信を遅らせる」= **第三者の反応を最適化する操作装置**になる。これは
憲法 7 (第三者は被写体ではなく刺激) の正面衝突であり、かつ**システムの目的関数
(本人の非合理判断の防止) にとっても誤りである** — 相手の内部状態は測れない。
測れないものを最適化目標にした瞬間、本システムは占い機に退化する (統治原則 1)。
解決: **介入の標的変数は本人側の特徴量に限定する (I-19)**。「返信を遅らせよ」は
「枯渇状態 R が低い今送ると本人の失策ハザードが高い。回復まで送信を保留せよ」
という**本人側の物理量で正当化される形でのみ**存在を許す。§2 Note 4 参照。

## 1.6 棄却案の墓場 (再提案を禁止する)

| 棄却案 | 理由 |
|---|---|
| 疎な高階テンソル (COO/CSR) | §1.1。467KB に間接参照は複雑さしか買わない |
| scipy 導入 | sidecar サイズ・供給面積の増大。必要機能は全て numpy+stdlib で書ける (§3) |
| NaN を欠測表現に使う | NEON/集計の全経路に NaN 分岐が漏れる。欠測は valid_mask のみ (I-18) |
| テンソルの LSM 化 (セグメント/墓標) | テンソルは**派生データ**で全再構築が <100ms。LSM は原本 (埋め込み) のための機構。派生データに原本の機構を移植するのは過剰設計 |
| メッセージ到着の Hawkes 過程 | この標本規模でパラメータが同定不能。時間帯別 Poisson で十分 (v2 で実測比較してから) |
| 学習モデル (NN/LSTM) のツイン | 決定論・stdlib・監査可能性の全てに反する。グリッド+閉形式で足りる |
| ツイン内の相手エージェントモデル | 憲法 7 違反。相手は外生の確率的刺激過程 (到着時刻分布) としてのみ存在 (§3.5) |
| 面接中リアルタイム介入 | interview_sim の状態機械を汚す。Echo は事前 (負荷整形) と事後 (講評) のみに接続 |

---

# §2【Architect's Note】初期設計への上書きと、その論拠

1. **「テンソル」を密な日次行列 + マスクに再定義 (Override)。** §1.1 の通り。
   異種データの結合とは「同一の時間格子に射影して同一の行に置くこと」であり、
   階数を上げることではない。LSM マニフェストの日付キー (ISO 暦日) を時間格子の
   正とし、インフラを再発明しない (引き継ぎ義務 4)。
2. **scipy を却下し NumPy のみ許可 (Override)。** ミッション要件は「NumPy/SciPy の
   関数指定」だったが、scipy は却下する。必要なのは (a) ランク変換 → argsort×2、
   (b) FFT 相互相関 → np.fft.rfft/irfft、(c) ロジスティック回帰 → IRLS 自前 12 行、
   (d) 分位点 → np.quantile、(e) 乱数 → Philox。全て numpy で閉じる。PyInstaller
   sidecar への +30MB と依存面積の対価に見合う scipy 固有機能は存在しない。
3. **結合行列の有意性判定は「遠ラグ帰無」で自給する (Override)。** 教科書的な
   t 検定/p 値は自己相関のある系列に対して**嘘をつく** (実効標本数の過大評価)。
   FFT で全ラグの相互相関を一括計算すると、信号 (|τ|≤14) と帰無分布 (遠ラグ
   |τ'|∈[45,365]) が**同じ 1 回の計算から**手に入る。乱数も外部ライブラリも不要な
   完全決定論の有意性判定であり、本書で最もエレガントな部分である (§3.3)。
4. **Oracle の介入は「生成ではなく選択」+ 標的は本人のみ (Override / 最重要)。**
   Puppeteer の構成的無菌化 (憲法 4) を介入側へ輸出する: 介入は静的
   `INTERVENTION_BANK` からの決定論的**選択**のみ。LLM は選択済み介入の言語化のみ。
   さらに各バンクエントリは標的レーン (本人側特徴量) を宣言し、バリデータが
   機械検査する (I-19)。「相手の反応を変えるための介入」はバンクに存在し得ない —
   実装者が後から誘惑に負けても書けない、構造的な退路の遮断 (salt 付き alias と
   同じ思想)。
5. **ツインへの憲法継承を最初に定義した (引き継ぎ義務 3)。** 憲法 3 (非対称性):
   Echo の全出力 (payload/forecast/coupling) は面接官・GD 議論・es_review に不出。
   `GAP_LEAK_MARKERS` を拡張し、講評フェーズ (`_gap_section()` 系統合点) のみ統合可
   (I-22)。憲法 5 (建前隔離): テンソルの相談系レーンは `simulated=True` を除外。
   ツインが本人データを「全知」する問題はスキルゲートで発言権を制御 (§1.4)。
6. **C++ カーネルは E5 に隔離し、計測ゲートを課す (Override)。** F=32, N=3650 の
   全計算は numpy で <3 秒 (profiler 実行時のみ・consult 時は計算しない)。C1/C2 と
   同じ規律: **E1〜E4 の実測が予算 (§5.9) を超えない限り E5 を実装するな。**
   ただしファイル形式 (PKBTEN01) は初日から C++ と共有可能な形で凍結する —
   動かさない境界面はバグを生まない。
7. **面接の分単位時間軸を日次テンソルに混ぜない (Override)。** 時間格子の混在は
   全計算の意味論を壊す。面接内の応答レイテンシ系列 (§7.1.3 の `latencies`) は
   セッションスコープの別系列としてツインの日内モード (§3.7) が直接消費する。
   「1 つの器に全部入れる」のは設計ではなく怠慢である。

---

# §3【Mathematical Foundation】厳密な数式定義

記法: 日次格子 t ∈ {0..N−1}、特徴量レーン k ∈ {0..K−1} (K=32)、観測値 x_k(t)、
マスク m_k(t) ∈ {0,1}。欠測 (m=0) の x は常に 0.0 を格納する (I-18)。
以下の全計算は profiler 実行時 (`import.line` / profiler) のみ走る。consult 時は
計算済み成果物を読むだけ (遅延初期化 I-3 と同型の「重い処理の隔離」)。

## 3.1 ロバスト標準化

```
med_k = median{ x_k(t) : m_k(t)=1 }
MAD_k = median{ |x_k(t) − med_k| : m_k(t)=1 }
z_k(t) = (x_k(t) − med_k) / (1.4826·MAD_k + ε),  ε = 1e-9
```
MAD_k = 0 なら標準偏差で代替。それも 0 ならレーン死亡 — 結合行列から除外し、
payload の `sufficiency.dead_lanes` に正直に記録する (ディスク・ホワイトボックス)。
ベースライン窓は直近 180 日 (定数 `BASELINE_DAYS = 180`)。

## 3.2 ランク変換 (Spearman 化)

外れ値 (単発の大支出等) が Pearson を支配するのを防ぐ。有効値のみを対象に
平均ランク (タイは平均) を付与し、[−1, 1] へ線形写像してから §3.3 へ渡す。
実装: `np.argsort(kind="stable")` を 2 回 + タイ群の平均化。乱数なし・完全決定論。

## 3.3 時系列直交結合マトリクス C[i,j,τ] (マスク付きラグ相関 + 遠ラグ帰無)

y_k(t) = ランク変換済み系列 (欠測は 0)。ゼロ埋めで長さ M = 2^⌈log2(2N)⌉ に拡張。
レーンごとに 3 本の rfft を前計算: Y_k = rfft(y_k), M_k = rfft(m_k), Q_k = rfft(y_k²)。

ペア (i,j)・ラグ τ (y_i が y_j に τ 日先行) について、逆 FFT の畳み込みで一括算出:

```
n(τ)    = irfft(conj(M_i)·M_j)        # 有効ペア数
S_xy(τ) = irfft(conj(Y_i)·Y_j)
S_x(τ)  = irfft(conj(Y_i)·M_j)         S_y(τ)  = irfft(conj(M_i)·Y_j)
S_xx(τ) = irfft(conj(Q_i)·M_j)         S_yy(τ) = irfft(conj(M_i)·Q_j)

ρ_ij(τ) = (n·S_xy − S_x·S_y) / sqrt( (n·S_xx − S_x²) · (n·S_yy − S_y²) )
```

- **信号域**: τ ∈ [−14, +14] (`MAX_LAG = 14`)。
- **帰無分布**: 同一 ρ_ij(τ') の τ' ∈ ±[45, min(N−45, 365)]。閾値
  ρ*_ij = quantile(|ρ_ij(τ')|, 0.99, method="linear")。
- **採択条件** (AND): |ρ_ij(τ)| > ρ*_ij、|ρ_ij(τ)| ≥ RHO_MIN (=0.15)、
  n(τ) ≥ N_MIN (=100)。
- 計算量: FFT 前計算 O(K·N·logN) + ペア結合 O(K²·M)。K=32, N=3650 で <1s。
- 対称性: ρ_ij(τ) = ρ_ji(−τ) なので i<j のみ計算し半分を省く。

自己相関がある系列では遠ラグ帰無が「この 2 系列がたまたま見せる相関の大きさ」を
そのまま教えてくれる — 実効標本数の推定という誤魔化しが要らない (§2 Note 3)。

## 3.4 認知リソース状態方程式 (敵対的デジタルツインのコア)

日次の認知リソース R(t) ∈ [R_floor, 1], R_floor = 0.05, R(0) = 0.7 (固定):

```
R(t+1) = clip( R(t) + ρ·(1−R(t))·rec(t) − β₁·ℓ_sw(t) − β₂·ℓ_vol(t) − γ·frict(t),
               R_floor, 1.0 )

rec(t)   = clip(cal_private_hours(t)/4, 0, 1) · (1 − clip(line_night_out(t)/20, 0, 1))
ℓ_sw(t)  = clip(cal_switch_count(t)/10, 0, 1)      # コンテキストスイッチ負荷
ℓ_vol(t) = clip(line_out_chars(t)/p95(line_out_chars), 0, 1)   # 通信量負荷
frict(t) = min(friction_events(t), 3)/3            # DL1 の多重シグナル判定値のみ
```

**観測対応物 (枯渇指数)**: D(t) = σ( mean{ z(line_reply_med_min), z(guilt_idx),
A(t) } )、A(t) = 1 − min(task_executed/max(task_declared,1), 1) (宣言日のみ)。
成分 2 個未満の日は D 欠測。σ はロジスティック関数 (z の非有界性を [0,1] へ圧縮)。

**パラメータ推定 (完全決定論・scipy なし)**: θ_dyn = (ρ, β₁, β₂, γ) を
SSE(θ) = Σ_t m_D(t)·(D(t) − (1−R(t; θ)))² のグリッド + 3 段細分化で最小化:
- 粗グリッド: ρ ∈ {0.05..0.50 step 0.05}, β₁,β₂,γ ∈ {0.00..0.35 step 0.05}
- 各段で最良点の周囲 ±1 step を半分の刻みで再走査 × 3 回。同値タイは添字最小。
- 計算量: O(|grid|·N)。粗グリッド 10·8·8·8 = 5,120 点 × N。R の再帰は t ループ
  必須だが**グリッド軸をベクトル化** (状態 shape=(G,) を N 回更新) — Python の
  二重ループ禁止。全体 <2s。

## 3.5 失策ハザードと極限負荷モンテカルロ

**失策ラベル (決定論的述語・内容非依存)**: lapse(t) = 1 iff いずれか:
- (a) spend_hedonic(t) > p90 ∧ task_declared(t) > 0 ∧ task_executed(t) = 0
- (b) line_night_out(t) ≥ p90 ∧ friction_events(t) > 0
- (c) line_out_msgs(t) > p95 ∧ line_in_msgs(t) < p50 (一方的送信フラッド代理)
分位点は直近 BASELINE_DAYS の自己ベースライン (dyad テンソルなら dyad 自身)。

**ハザードモデル**: P(lapse | R) = σ(β₀ + β₁·R)。推定は IRLS (Newton) 自前実装。
**(Rev.4 訂正)** リッジは Hessian だけでなく**勾配にも**入れる (ペナルティ付き
対数尤度 L − (λ/2)‖β‖² に対する正しい Newton 法)。Rev.1 の式 (勾配側 λβ なし)
は解を持たない完全分離データで β が発散したまま反復上限に達し、κ/θ_R が
ゴミになる — Hessian のみの λI は「解けない線形系を解けるようにする」だけで、
「最適解が有限に存在すること」は保証しない (W-16):
```
p = σ(Xβ);  W = diag(p(1−p))
β ← β + (XᵀWX + λI)⁻¹ (Xᵀ(y − p) − λβ)        ← 勾配側の −λβ が Rev.4 訂正
X = [1, R̃(t)] (R̃ = R − mean(R): 中心化してから解き、β₀ を後で戻す — W-17)
λ = 1e-3,  β_init = 0,  最大 25 反復,  収束 ‖Δβ‖∞ < 1e-8
σ の引数は全経路で ±Z_CLIP (=35.0) にクリップ (W-15)
```
κ = −β₁, θ_R = β₀/κ と読み替える (κ ≤ 0 に収束したら「R と失策が無関係」であり
スキルゲートで必ず落ちる — エラーにせず gate_passed=false で報告)。

**walk-forward 検証 (I-20 の本体)**: 拡張窓で 28 日ごとに再推定し次の 28 日を予測。
Brier B = mean((p̂−y)²)、気候値ベースライン B₀ (訓練窓の失策率を定数予測)、
BSS = 1 − B/B₀。**ゲート: BSS ≥ 0.05 かつ 検証期間の失策数 ≥ 10。**
in-sample 適合を「精度」として報告するコードはバグである (罠 T-18)。

**モンテカルロ (シナリオ・シミュレーション)**:
- seed = blake2b(tensor.content_hash64 ‖ params_json ‖ scenario_json, 8B) → Philox。
- 経路数 P = 8192、горизонт H 日 (既定 14、上限 30)。
- 決定論成分: 未来カレンダー (Future Context 30 日) から ℓ_sw, rec を構成。
- 確率成分: frict(t) ~ Bernoulli(p̂_f) (p̂_f = 直近 90 日の摩擦日率)、負荷乗数
  ~ 過去残差の経験分布から `rng.choice` でブートストラップ再標本。
  相手は**到着過程としてのみ**登場する (§1.6。内部状態のモデル化は禁止)。
- 実装拘束: 状態は shape=(P,) の f32 配列、t ループのみ Python、P 軸は完全
  ベクトル化。**O(P·H) 時間・O(P) メモリ** (軌跡の全保存禁止 — 集計量のみ)。
  P=8192, H=30 → 25 万ステップ・<1s。
- 出力: R の分位点列 (q10/q50/q90)、P(R(t) < θ_R)、期待失策数、臨界日リスト。

## 3.6 決定論的乱数の系譜 (I-17)

`seed_material` は常に「入力内容のハッシュ」であり、時刻・pid・カウンタを混ぜるな。
同一のテンソル・パラメータ・シナリオに対する forecast は何度呼んでもビット同一 —
テスト可能性と監査可能性 (「なぜこの介入が出たか」の完全再現) の両方がこれで立つ。

## 3.7 ドメイン特化指標

**(A) 過剰投資指数 OII (Target B)** — dyad スコープテンソル上で:
```
OII(t) = mean{ z(line_out_chars), z(line_initiations), z(spend_tagged_ema14),
               −z(line_reply_med_min) }        # 全て dyad スコープ・本人側のみ
OII_ema(t) = EMA(OII, half-life 7日)
発火: OII_ema ≥ 1.5 が 3 日連続 ∧ ツインの gate_passed ∧ dyad の exchanges ≥ 20
```
z は自己ベースライン比 (§3.1)。−z(latency) = 「自分だけが加速している」の符号。
**相手側の量 (peer_reply 等) は OII に入れない** — 測るのは「自分が払っている
コストの加速」であって「相手の温度」ではない (I-19 の数式レベルでの適用)。

**(B) 論理破綻ハザード (Target A)** — 面接セッションの日内モード:
日次ツインの R(day) を初期値に、15 分刻みの日内格子で同じ状態方程式を回す。
負荷は面接ターン (response_time_sec 履歴の経験分布から再標本) とコンテキスト
スイッチ (出題トピック遷移ごとに δ を加算。δ̂ = 過去セッションの「トピック遷移
直後レイテンシ − 同一トピック内レイテンシ」の中央値差)。出力は
P(R < θ_R | 面接開始後 m 分) の曲線 — 「何分目に論理が破綻しやすいか」の予測。

---

# §4【Architecture Blueprint】データフローと隔離境界

```
data/raw (日記/LINE/家計簿/予定)     data/processed/line_telemetry.json (DL1)
        │                                     │ (DyadStats: alias化済み集計値)
        ▼                                     │
 data_merger.load_daily_contexts()            │
        │                                     │
        ├──────────────▶ 既存: LSM 差分埋め込み (C1) → segments/*.bin → Bravo 検索
        │                【Echo は無改変で共存。再発明禁止】
        ▼                                     ▼
 TensorStore.build(daily, dyads)  ◀───────────┘
        │  (profiler / import.line 時のみ。RECORD/consult では走らせない I-3)
        ▼
 data/processed/tensor_global.bin      ← PKBTEN01 (mmap, 数値+mask のみ, I-21)
 data/processed/tensor_dyad_{alias}.bin
        │ (np.frombuffer ゼロコピー読み)
        ├────────▶ coupling.py  → 結合行列 C[i,j,τ] + 有意フラグ ┐
        ├────────▶ digital_twin.py → TwinParams + BSS + Forecast ├─▶ oracle.py
        └────────▶ OII / ハザード指標                            ┘      │
                                                                        ▼
                                                  oracle_payload.v1 (無菌 JSON §5.6)
                                                        │
              ┌─────────────────────────────────────────┤
              ▼ 合法出口 (3 つだけ)                       ▼ 遮断 (I-22)
  (1) consult の静的プレフィックス                面接官/GD議論/es_review へは
      (_gap_section と同居。profiler 再実行時       いかなるキーも不出。
       のみ更新されるため KV 効率が高い — Rev.6)
  (2) 講評フェーズ (_gap_section 系統合点)       GAP_LEAK_MARKERS へ
  (3) UI PROFILE タブ (表示専用)                 "oracle_payload"/"twin"/
                                                 "coupling"/"OII" を追加し
                                                 テストで固定 (E0)
```

## 4.1 既存インフラとの接続規約

- **時間格子** = LSM マニフェストと同じ ISO ローカル暦日。日付演算は文字列日付
  ベース (`date.fromisoformat` の差分)。**タイムスタンプ/86400 での日数計算は禁止**
  (DST/うるう秒。JS 側 toISOString 事故と同族 — 罠 T-17)。
- **鮮度判定**: tensor header の `content_hash64` = blake2b(全 DailyContext の
  正規化 JSON, 8B)。不一致なら全再構築 (<100ms、差分機構は作らない §1.6)。
- **再構築順序 (I-9 の適用)**: 全 TensorStore ハンドルの close → tmp へ書き →
  `os.replace`。mmap 保持中の上書きは Windows で PermissionError (Bravo の実証)。
- **KV キャッシュ**: oracle 出力は **as-built では静的プレフィックス側**
  (`_gap_section` と同居)。profiler 再実行時のみ内容が変わるため、consult
  毎に変わる動的サフィックスへ置くより KV 再利用率が高い (Rev.6 訂正 —
  初版は逆の記述だったが、oracle 出力は日次で変化しないため静的側が正しい。
  AI_SKILLS §8.1-2 の「静的→動的の順序を守れ」原則そのものは不変)。

## 4.2 ツインの無菌性 (憲法 3/5 の継承構造)

ツイン (`digital_twin.py`) の入力型は `TensorStore` と数値 dict のみ。テキストを
受け取る引数が**存在しない** — compaction が embedder を受け取らないのと同じ、
型シグネチャによる誘惑の封殺。simulated=True の相談はテンソル構築時点で
consult_count レーンから除外済みなので、ツインは建前人格を観測すらできない。

---

# §5【Sonnet-Control Directives】実装拘束 (逐語仕様)

## 5.1 C++ メモリレイアウト (PKBTEN01) — search_engine.cpp へ追加

雰囲気ではない。以下を**そのまま**貼れ。PKBSCR01 と同じ釘付け規律
(pack(1) + static_assert + offsetof)。対応物は 2 つだけ: この struct と
`core/tensor_store.py` の struct 定義。変更は両方同時 + magic バージョン更新。

```cpp
// ---- Target Echo: PKBTEN01 日次×特徴量テンソル (mmap ゼロコピー共有) ----
// Python 側の唯一の対応物は core/tensor_store.py。
// 全フィールド自然整列 (header 64B、row 136B ≡ 0 mod 8)。little-endian。
constexpr uint32_t kTenFeat = 32;

#pragma pack(push, 1)
struct TensorHeader {            // 64 bytes — Python "<8sIIIIiIQ24x"
    char     magic[8];           // "PKBTEN01"
    uint32_t version;            // = 1
    uint32_t n_rows;             // 日数 (密。row i = epoch_day + i 日)
    uint32_t n_features;         // 使用レーン数 (<= kTenFeat)
    uint32_t row_stride;         // = sizeof(TensorRow) = 136 (読み手は必ずこれを使う)
    int32_t  epoch_day;          // row 0 の日付 (1970-01-01 からの日数, ローカル暦日)
    uint32_t flags;              // bit0: dyad スコープ / 他ビット予約 (0)
    uint64_t content_hash64;     // 入力スナップショット blake2b 先頭 8B (鮮度判定)
    uint8_t  reserved[24];       // 0 埋め
};

struct TensorRow {               // 136 bytes — Python "<iI32f"
    int32_t  day_index;          // 常に行番号と一致 (整合性検証用の冗長フィールド)
    uint32_t valid_mask;         // bit k = レーン k 観測済み。欠測は値 0.0f + bit 0
    float    f[kTenFeat];        // NaN 格納禁止 (I-18)
};
#pragma pack(pop)

static_assert(sizeof(TensorHeader) == 64,               "TensorHeader layout mismatch");
static_assert(offsetof(TensorHeader, epoch_day) == 24,  "TensorHeader.epoch_day offset");
static_assert(offsetof(TensorHeader, content_hash64) == 32, "TensorHeader.hash offset");
static_assert(sizeof(TensorRow) == 136,                 "TensorRow layout mismatch");
static_assert(offsetof(TensorRow, f) == 8,              "TensorRow.f offset");
static_assert(sizeof(TensorRow) % 8 == 0,               "TensorRow 8-byte alignment");
```

- **v1 (E1〜E4) では C++ はこの struct を読まない** (numpy が同一ファイルを
  ゼロコピーで読む)。struct を今日凍結するのは境界面を将来も動かさないため。

### 5.1.1 E1 への最終警告 (Rev.2 — C++/Python 境界の規約。全項目が強制)

PKBVEC01/PKBSCR01 で 2 度支払った授業料を 3 度払わせないための、発生し得る
最悪事態の列挙と封鎖規約。**E1 のテストゲートはこの節の各項目に 1:1 対応する。**

**(W-1) struct fmt の "<" 欠落 — 最悪の理由は「64 のままズレる」ではなく
「72 になる」こと。** native モード ("<" なし) では Q (u64) が 8 バイト境界へ
パディングされ calcsize が 72 に化ける。import 時 assert (calcsize==64/136) が
これを捕まえる — **assert を「テストで見てるから」と削った瞬間、次の フォーマット
編集者が無言のレイアウト崩壊を出荷する。** numpy 側も同罪: dtype は必ず明示
リトルエンディアン (`"<i4"`, `"<u4"`, `"<f4"`)。native 表記 ("i4") は現行全
ターゲット (Windows ARM64 / macOS / Linux — 全て LE) で「たまたま」一致するが、
規約はあくまで明示 LE である。

**(W-2) pack(1) の意味論を誤解するな。** pack(1) は「暗黙パディングの禁止」で
あって「非整列アクセスの許可」ではない (Bravo §9.1-2 と同文)。本レイアウトは
構成的に自然整列 (header 64B / row 136B ≡ 0 mod 8 / f[] は行内 +8)。mmap の
ベースはページ整列なので全オフセットの整列が保存される — **「念のため」の
アライメント修正は非問題の修正であり、行うな。** フィールドの並べ替え・挿入は
それ自体が違反。予約領域 (reserved[24] / lane 22-31) の転用には magic の
PKBTEN02 昇格 + 両側同時変更が必須。

**(W-3) C++ struct の構成規約 (コンパイラ依存挙動の排除):**
```cpp
// - #pragma pack(push,1) / pop の括り必須。push を欠くと以降の全 struct
//   (MappedFile 等) が巻き添えで再パックされる — search_engine.cpp 全体の破壊。
// - bool / enum / ビットフィールド禁止 (サイズ・レイアウトが処理系定義)。
//   valid_mask は素の uint32_t + 手動ビット演算のみ。
// - magic の照合は memcmp(h->magic, "PKBTEN01", 8) == 0。uint64_t 整数比較で
//   書くな (エンディアン非対称の温床)。
// - §5.1 の static_assert 群に以下を追加:
static_assert(std::is_standard_layout<TensorHeader>::value, "std-layout required");
static_assert(std::is_standard_layout<TensorRow>::value,    "std-layout required");
static_assert(std::is_trivially_copyable<TensorRow>::value, "memcpy-safe required");
static_assert(offsetof(TensorHeader, version)  ==  8, "version offset");
static_assert(offsetof(TensorHeader, n_rows)   == 12, "n_rows offset");
static_assert(offsetof(TensorHeader, row_stride) == 20, "row_stride offset");
static_assert(offsetof(TensorHeader, flags)    == 28, "flags offset");
```

**(W-4) 読み込みプロトコル (tensor_store.py) — 全チェックが named error:**
1. magic / version==1 / row_stride==136 (**ヘッダから読め。定数を仮定するな** —
   前方互換の生命線)。
2. `file_size == 64 + n_rows*136` の厳密一致。切詰め・末尾ゴミは即エラー
   (「読めるところまで読む」は破損の隠蔽である)。
3. `rows["day"] == np.arange(n_rows)` の全一致 (epoch 誤り・行欠落の即検出)。
4. NaN の存在 = 即エラー (I-18)。`valid_mask` の n_features 以上のビット = 即エラー。
5. mmap は `ACCESS_READ` 固定。`np.frombuffer` の戻りが writeable=False であることを
   assert — テンソルの in-place 更新経路を型レベルで殺す (rebuild-only の強制)。

**(W-5) 書き込みプロトコル — scratch と混同するな。** scratch (PKBSCR01) は
in-place 更新が正、テンソルは **in-place 更新が禁止** (全再構築 + tmp +
os.replace のみ)。「Bravo がこうしていたから」の流用は逆向きの事故になる。
再構築前に module レジストリの全 open ハンドルを close し、残存があれば
**OS の PermissionError を待たずに自前の TensorStoreError を投げる** —
OS 依存の非決定な失敗を、決定論の事前ガードで先取りする (T-14/I-9)。

**(W-6) 列アクセスは暗黙コピーである。** 構造化 dtype の `rows["f"][:, k]` は
ストライド 136B のビューであり、演算時に numpy が内部コピーを作る。これは
**意図された挙動** (467KB は L2 に収まる — §1.2)。「ゼロコピーなのにコピーして
いる」というレビュー指摘が来たら本節を引用して却下せよ。ゼロコピーの対象は
「ファイル → プロセスのアドレス空間」であって「全演算の無コピー化」ではない。
- E5 (計測ゲート通過時のみ): デーモンに `{"cmd":"tensor_stat","seq":n,"tensor":
  "<path>","op":"..."}` 系を追加し、既存 `MappedFile` キャッシュと `remap` コマンド
  (path キー) をそのまま流用する。**scratch (2072B) は拡張しない** — 結合行列の
  出力は scratch に収まらないため、結果は出力 mmap ファイルへ書く設計とする
  (詳細設計は E5 着手時に本書を改訂。今は書くな — スコープクリープ禁止)。

## 5.2 特徴量レジストリ (レーン番号は永久凍結。付番の再利用禁止)

| lane | id | 定義 (全て決定論・その日の数値) |
|---|---|---|
| 0 | diary_chars | diary_text 文字数 |
| 1 | abstract_idx | ABSTRACT_LEXICON ヒット数 (gap_analysis の語彙を import して流用) |
| 2 | guilt_idx | GUILT_MARKERS ヒット数 |
| 3 | productivity_idx | PRODUCTIVITY_MARKERS ヒット数 |
| 4 | consult_count | 相談件数。**simulated=True を除外** (憲法 5) |
| 5 | spend_total | 支出合計。**income 不算入** (AI_SKILLS §6.2-5) |
| 6 | spend_hedonic | 娯楽・消費テーマ支出 (THEME_TAXONOMY の支出カテゴリ流用) |
| 7 | spend_invest | 学習・自己投資テーマ支出 |
| 8 | cal_event_count | 予定件数 |
| 9 | cal_private_hours | PRIVATE_TIME_KEYWORDS 該当予定の合計時間 (h) |
| 10 | cal_switch_count | 隣接予定のテーマ遷移数 (コンテキストスイッチ代理変数) |
| 11 | line_out_msgs | 本人送信数 |
| 12 | line_in_msgs | 受信数 |
| 13 | line_out_chars | 本人送信文字数 |
| 14 | line_reply_med_min | 本人返信レイテンシ中央値 (分)。深夜窓・>48h 除外は DL1 規約を流用 |
| 15 | line_initiations | 本人起点バースト数 (`_bursts_by_contact` 流用) |
| 16 | line_night_out | 23:00-08:00 の本人送信数 |
| 17 | friction_events | DL1 多重シグナル判定の通過件数のみ (単一キーワード判定の再発明禁止) |
| 18 | task_declared | 宣言検出数 (gap_analysis の宣言検出を流用。意思マーカー規約ごと) |
| 19 | task_executed | 実行検出数 |
| 20 | github_commits | 拡張 importer (無ければ全行 mask=0。**importer 実装は Echo のスコープ外**) |
| 21 | leetcode_solved | 同上 |
| 22-27 | (reserved) | 0 埋め・mask=0。**付番には Architect's Note が必要** |
| 28-31 | (reserved) | 同上 |

**dyad スコープテンソル**: 同一レーン定義のまま、lane 11-17 を当該 dyad に限定し、
lane 5-7 を `spend_tagged` (config の dyad 支出タグ規則にマッチした支出) に読み替える。
flags bit0 = 1。**dyad ファイル名は `tensor_dyad_{alias}.bin` — 実名の永続化は
ファイル名・config・ログの全てで禁止 (I-15)。** 対象指定 UI は入力された名前から
`contact_alias()` を即時計算し、名前はメモリ外へ出さない。

## 5.3 Python API (シグネチャ凍結。numpy 可 = pipeline 層扱い)

```python
# core/tensor_store.py (新規)
FEATURES: dict[str, int]          # §5.2 の写し。テストが凍結を検証する
TENSOR_MAGIC = b"PKBTEN01"
_HEADER_FMT = "<8sIIIIiIQ24x"     # calcsize == 64 を import 時 assert
_ROW_FMT = "<iI32f"               # calcsize == 136 を import 時 assert

class TensorStore:
    def __init__(self, path: Path)                    # header 検証 + mmap (read-only)
    @property
    def dates(self) -> tuple[str, str]                # (row0 の ISO 日付, 最終日)
    def window(self, start_iso: str, end_iso: str) -> tuple[np.ndarray, np.ndarray]
        # 戻り値 (values (D,32) f32, mask (D,32) bool) — ゼロコピー view。
        # 呼び出し側が保持する場合は必ず .copy() (罠 T-14)
    def close(self) -> None                           # atexit 登録。二重 close 可

def build_tensor(daily: list[dict], dyads: list | None, out_path: Path,
                 scope: str = "global", alias: str | None = None) -> Path
    # 全再構築のみ。書き込みは tmp → os.replace。
    # 事前に registry 内の全 open ハンドルを close (I-9 / 罠 T-14)
def content_hash64(daily: list[dict]) -> int          # blake2b 8B。鮮度判定の唯一の鍵

# core/coupling.py (新規)
MAX_LAG = 14; RHO_MIN = 0.15; N_MIN = 100; NULL_LAG_RANGE = (45, 365)
def coupling_matrix(values: np.ndarray, mask: np.ndarray,
                    max_lag: int = MAX_LAG) -> dict
    # {"pairs": [{"src": lane_id, "dst": lane_id, "lag": int, "rho": float,
    #             "n_eff": int, "null_q99": float, "sig": bool}], ...}
    # §3.3 のアルゴリズムを逐語実装。sig=False のペアも上位 64 件までは記録する
    # (ディスク・ホワイトボックス — UI が隠すのは可、計算根拠の破棄は不可)

# core/digital_twin.py (新規)
@dataclass
class TwinParams:
    rho: float; beta1: float; beta2: float; gamma: float
    kappa: float; theta_r: float
    bss: float; n_lapse_test: int; gate_passed: bool
    fitted_window: str            # "2025-01-01..2026-07-01"
    def to_dict(self) -> dict     # deep_profile へ全根拠を正直に記録 (統治原則 3)

def fit_twin(store: TensorStore) -> TwinParams        # §3.4-3.5。決定論
def simulate(params: TwinParams, store: TensorStore, scenario: dict,
             paths: int = 8192) -> dict               # §3.5 MC。seed は §3.6 で導出
    # scenario = {"horizon_days": int, "calendar": [...Future Context 由来...],
    #             "mode": "daily" | "interview", "interview_turns": int | None}
def compute_oii(dyad_store: TensorStore) -> dict      # §3.7(A)。global store には適用禁止

# core/oracle.py (新規)
INTERVENTION_BANK: dict[str, dict]
    # 例: "iv-001": {"label": "送信クールダウン", "target_lane": 11,
    #                "template": "...", "params_schema": {"cooldown_h": int}}
    # 全エントリに target_lane 必須。バリデータが FEATURES の値域で機械検査 (I-19)
ORACLE_RULES: tuple            # (rule_id, 述語関数, 許容 bank_id タプル) の静的表
def build_oracle_payload(scope: str, alias: str | None = None) -> dict
    # oracle_payload.v1 (§5.6)。無菌検査 (§5.7) を通してから返す
def render_oracle_consult(payload: dict) -> str
    # LLM プロンプト組み立て (言語化のみ)。「LINE上の観測範囲では」限定句を必ず
    # 含める (罠 T-12 継承)。新事実・新介入の創作禁止を SYSTEM 指示に明記
```

## 5.4 配線 (既存パターン厳守)

```
facade.py:   oracle_report(scope, alias=None) -> dict     # payload + LLM 言語化
             twin_forecast(scenario) -> dict              # payload のみ (LLM なし)
             tensor_rebuild() -> dict                     # {"rebuilt": bool, "rows": n}
engine_stdio.py dispatch: "oracle.report" / "twin.forecast" / "tensor.rebuild"
apps/desktop/src/lib/engine.ts: 同名ラッパー 3 本
テンソル再構築のトリガ: profiler 実行 と import.line のみ (I-3)。
oracle.report は consult 系と同じく status イベントで進捗を流す (LLM 呼び出しがある)。
```

## 5.5 憲法ガード (E0 — 機能より先に書く。RED 確認必須)

1. `GAP_LEAK_MARKERS` に追加: `"oracle_payload"`, `"twin_forecast"`, `"coupling"`,
   `"OII"`, `"認知リソース"` (講評フェーズ以外への漏洩マーカー)。
   `_write_phase3_assets()` フィクスチャへ対応エントリ追加 — DL2 と同一手順。
2. 無菌検査 `oracle._assert_sterile(payload)`: payload を再帰走査し、全文字列が
   ホワイトリスト正規表現 (feature id / rule id / `iv-\d{3}` / ISO 日付 /
   `C-[0-9a-f]{4,8}` / schema 名) のいずれかに一致することを assert。
   自由テキスト 1 つで AssertionError。**本番コードに置く** (テスト専用にしない —
   Puppeteer のホワイトリスト assert と同格の実行時ガード)。
3. I-19 検査: `INTERVENTION_BANK` 全エントリの `target_lane ∈ FEATURES.values()`
   を import 時 assert。「相手」を標的にするレーンはレジストリに存在しないため、
   これで構成的に閉じる。
4. **(Rev.2 訂正 — Architect's Note)** Rev.1 の「test_oracle.py を削除・スキップ化
   せず RED のまま保持せよ」は、DoD の「全スイート PASS まで完了と言わない」と
   矛盾する常設 RED を生む誤った指示だった。E1〜E3 の完了報告が「テスト失敗中」に
   なるか、RED の常態化に目が慣れるかの二択であり、後者は回帰ガード文化の死である。
   訂正: 確立済みの **I-5 パターン (exe 必須テストのゲート付き SKIP) に合流**させる。
   `core.oracle` / `core.tensor_store` の ImportError **のみ**を捕まえて
   「SKIP (E1/E4 未実装)」を明示出力し、それ以外の例外は全て FAIL とする。
   E1 完了時に tensor_store 系、E4 完了時に oracle 系のゲートが自然に開き、
   **E4 のゲート条件に「test_oracle.py が SKIP なしで GREEN」を含める。**

## 5.6 oracle_payload.v1 — JSON スキーマ (厳格・無菌)

```json
{
  "$schema_note": "全フィールド必須。additionalProperties 禁止。自由テキスト禁止。",
  "schema": "oracle_payload.v1",
  "generated": "<ISO date>",
  "scope": {"kind": "global | dyad", "alias": "<C-xxxx | null>"},
  "sufficiency": {
    "days_observed": "<int>", "coverage": "<float 0-1>",
    "dead_lanes": ["<lane id...>"],
    "twin_bss": "<float>", "n_lapse_test": "<int>", "gate_passed": "<bool>"
  },
  "state": {
    "r_now": "<float 0-1>", "r_trend_7d": "<float>",
    "oii_ema": "<float | null>", "oii_streak_days": "<int>"
  },
  "couplings": [
    {"src": "<lane id>", "dst": "<lane id>", "lag_days": "<int -14..14>",
     "rho": "<float>", "n_eff": "<int>", "null_q99": "<float>", "sig": "<bool>"}
  ],
  "forecast": {
    "horizon_days": "<int>", "r_q10": ["<float>..."], "r_q50": ["<float>..."],
    "r_q90": ["<float>..."], "p_lapse": ["<float>..."],
    "critical_days": ["<ISO date>..."]
  },
  "findings": [
    {"rule_id": "<R-XXX-nn>", "severity": "<float 0-1>",
     "metrics": {"<metric name>": "<number>"}}
  ],
  "interventions": [
    {"bank_id": "<iv-nnn>", "trigger_rule": "<R-XXX-nn>",
     "target_lane": "<int>", "params": {"<param>": "<number>"}}
  ]
}
```

gate_passed=false のとき `forecast`/`interventions` は**空配列** (I-20)。LLM には
「データ不足につき介入なし。観測を続けよ」の言語化だけを許す。

## 5.7 INTERVENTION_BANK 初期セット (全て本人標的 — I-19)

| id | label | target_lane | 内容 (テンプレート要旨) |
|---|---|---|---|
| iv-001 | 送信クールダウン | 11 | R < θ_R の間、当該 dyad への非緊急送信を保留し R 回復後に見直す |
| iv-002 | 支出クールオフ | 6 | 失策ハザード高位日の裁量支出に 24h の遅延を課す |
| iv-003 | 意思決定モラトリアム | 18 | R < θ_R の日に不可逆コミットメント (応募/購入/約束) をしない |
| iv-004 | 回復ブロック予約 | 9 | 結合行列が示す回復→生産性ラグに合わせ私的時間を予定に置く |
| iv-005 | 面接前テーパリング | 10 | 面接前 48h のコンテキストスイッチ数を上限管理する |
| iv-006 | 深夜送信ゲート | 16 | 23:00-08:00 の下書きを朝の R 回復後レビューまで保留する |

**追加規則**: 新エントリは「本人の行動を本人の物理量で正当化する」形式のみ。
相手の反応・関心・感情を目的語に含む文言はレビューで機械的に落とせ。

## 5.8 新設不変条件と罠 (AI_SKILLS へ実装完了時に転記する)

- **I-17 (乱数の統治)**: 全確率計算は Philox + 入力内容由来 seed。時刻/pid/global
  state を seed に混ぜるな。unseeded `np.random.*` の呼び出しは 1 箇所でもバグ。
- **I-18 (マスク意味論)**: 欠測 = valid_mask のみで表現。値スロットは 0.0f。
  NaN の格納・欠測判定への使用は禁止。**マスクを見ない集計 (0 を実測値として
  平均に混ぜる) は最頻の静かな死** — 全集計関数は (values, mask) を対で受ける。
- **I-19 (介入標的の限定)**: 介入の標的変数は本人側特徴量レーンのみ。第三者の
  反応・感情を最適化目標にする介入・数式・プロンプトを作らない (憲法 7 の系)。
- **I-20 (スキルゲート)**: walk-forward BSS ≥ 0.05 ∧ 検証失策数 ≥ 10 を満たさない
  ツインの forecast/interventions は空。in-sample 適合の報告をスキルと呼ぶな。
- **I-21 (テンソルの無菌性)**: PKBTEN01 に文字列・生テキストを入れない。数値 +
  マスク + alias (ファイル名) のみ。テキストが要るならそれはテンソルの仕事ではない。
- **I-22 (聖域の拡張)**: Echo の全出力は面接官/GD 議論/es_review に不出。合法出口は
  consult **静的プレフィックス** (Rev.6 訂正。§4.1 参照)・講評フェーズ・
  PROFILE UI の 3 つだけ。
- **T-14 (view の use-after-unmap)**: `window()` の戻り値 view を保持したまま
  rebuild すると BufferError/未定義動作。長期保持は .copy()、rebuild は全ハンドル
  close が先 (レジストリで機械的に検査)。
- **T-15 (欠測ゼロの汚染)**: I-18 違反の別名。「支出 0 円の日」と「家計簿を
  つけなかった日」の混同は全統計を静かに歪める。対照群テスト必須。
- **T-16 (自己相関下の偽有意)**: 結合行列の有意性を t 検定/固定閾値に「簡略化」
  するな。遠ラグ帰無 (§3.3) が唯一の正。
- **T-17 (日付演算)**: 日数差は ISO 文字列 → date オブジェクトの差分のみ。
  epoch 秒/86400 は DST とうるう秒で暦日とずれる。
- **T-18 (トートロジーツイン)**: §1.4。in-sample 精度の報告・スキルゲートの
  緩和・検証窓の縮小は全て同じバグの変奏。
- **T-19 (dyad 実名の滲出)**: ファイル名・config・例外メッセージ・ログの全経路で
  alias のみ (T-13 の Echo 適用)。テストで例外文言に実名が含まれないことを assert。

## 5.9 実装順序とテストゲート (強制)

```
E0: 憲法ガード (§5.5)。GAP_LEAK_MARKERS 拡張 + _assert_sterile + I-19 assert の
    テストを書き、RED を確認してから E1 へ。            ゲート: RED→GREEN の記録
E1: tensor_store.py + PKBTEN01。                        ゲート: test_tensor_store —
    レイアウト相互検証 (calcsize==64/136, offset 釘付け — test_layout_cross_validation
    と同型) / mask 意味論の対照群 (T-15) / rebuild-under-handle (T-14) /
    simulated 除外 (憲法5) / 日付格子 (T-17)
E2: coupling.py。                                       ゲート: test_coupling —
    合成データ (既知ラグの注入信号 → 検出) / 独立系列 → sig ゼロ (帰無の対照群) /
    決定論 (2 回実行のビット同一)
E3: digital_twin.py。                                   ゲート: test_digital_twin —
    グリッド探索の決定論 / IRLS 収束 / walk-forward がゲートを落とすケース /
    seed 固定 MC の再現性 (round(,6) 比較) / O(P) メモリ (軌跡非保存) の構造確認
E4: oracle.py + facade/stdio/UI 配線。                  ゲート: test_integration 拡張 —
    payload 無菌検査 / gate_passed=false で介入空 / 議論フェーズへの不出 (E0 ガード
    が GREEN のまま) / ui_smoke
E5: C++ カーネル。E1〜E4 の実測 (profiler 1 回の Echo 計算合計) が 3000ms を
    超えた場合のみ着手。                                ゲート: benchmark 前後比較
各段完了時に AI_SKILLS.md へ as-built 追記 (第0原則)。コミットは指揮官の指示時のみ。

## 5.10 (Rev.3) E1 レビュー裁定・E1.1 是正指令・E2 への最終警告

### 5.10.1 Sonnet5 の 3 拡張への裁定

**D1 (cal_private_hours の名目値 1.5h/件) — Approved。**
E2 は §3.2 でランク変換 (Spearman 化) を通すため、定数スケールは単調変換不変性
により**数学的に無効** — 偽信号は構造的に生じ得ない。失われるのは時間の分散
(30 分の茶と 8 時間のデートの同一視) だが、これはアーティファクトではなく
解像度の限界であり、基盤データ (calendar.json に終了時刻なし) に正直である。
条件: (a) payload/LLM 言語化で「実測時間」と主張しない — 意味論は
「私的予定の名目時間 (件数 × 1.5h)」。(b) 終了時刻がデータモデルに入ったら
実測合算へ置換 — テンソルは全再構築派生データなので意味論変更に移行コストは
無い (embedder_id のような世代混在リスクが存在しないことを確認済み)。

**D2 (build_tensor への line_messages 系 keyword-only 引数) — Approved。**
dyads (履歴全体集計) に日付分解能が無いのは事実であり、生ログの消費は第 3
チャネルの正当な使用である (I-16)。テンソルに着地するのは数値 + mask のみで
I-21 は保たれ、DL1 バースト状態機械の再利用は §11.1 の教訓 (対称性を確認して
から流用を決める) の正しい適用。条件: **line_messages を渡す配線は
profiler / import.line 経路のみ** (I-3)。consult 経路からテンソル再構築を
発火させる配線を書いた時点で違反。

**D3 (dyad スコープ lane 5-7 の mask=0 固定) — Approved。**
正直な欠測は E2 で n(τ)=0 → N_MIN ゲートが自動排除するため偽信号ゼロ。
「とりあえず global 支出を dyad にも書いておく」という代替案こそが偽結合の
温床であり、mask=0 は正解。条件: 将来 dyad 支出タグ config を導入する際も、
タグ不一致の支出で「埋める」ことを禁止 — 欠測は欠測のまま。

### 5.10.2 E1.1 是正指令 (レビューが検出した申告外のアーティファクト源。E2 着手の前提条件)

**lane 18/19 (task_declared/executed) の mask 意味論が I-18 に違反している。**
現実装は `daily` に存在する全日付へ mask=1 で 0 を書く。これは「観測チャネルの
不在」を「宣言 0 件という観測」に化けさせる — T-15 の変奏であり、E2 で最悪の
形で析出する: 日記を書かない日は常に task_declared=0 (観測扱い) になるため、
**日記執筆習慣と相関するあらゆるレーンとの間に、共有欠測パターン由来の偽結合が
立つ**。感情でも理論でもなく機序が特定できる系統誤差である。是正:

1. lane 18: mask=1 は「その日に genuine な主観 doc (diary_text 非空、または
   is_simulated_persona でない consult query 非空) が存在する」場合のみ。
2. lane 19: mask=1 は「実行観測チャネル (calendar_events / line_self_text /
   diary_text のいずれか非空) が存在する」場合のみ。
3. **LINE カウント系レーン (11,12,13,15,16,17) は鏡像の修正が必要**: LINE ログは
   完全記録であり、カバレッジ窓 [ログ最古日, ログ最新日] (dyad スコープでは
   当該 contact の窓) 内の無通信日は「観測済み 0」(mask=1, value=0.0) が正。
   現実装の mask=0 は偽信号こそ生まないが、「沈黙」という最重要シグナルを
   相関計算から系統的に脱落させる (活動日への条件付けバイアス)。
   lane 14 (レイテンシ) は従来通り、返信サンプルの無い日は mask=0。
4. E1.1 のテスト: (a) 日記・相談の無い日の lane 18 が mask=0、(b) カバレッジ窓内
   無通信日の lane 11 が mask=1/value=0、(c) 窓外が mask=0、の 3 対照群。

### 5.10.3 E2 への最終警告 (W-7〜W-14 — FFT × 欠測マスクの数学的罠)

- **W-7 (円形畳み込みの縫合)**: M = 2^⌈log2(2N)⌉ 未満への「効率化」禁止。
  ゼロパディング不足の FFT 相互相関は wrap-around で系列の頭と尻を縫合し、
  「去年の年末と今年の年始の偽結合」を出力する。
- **W-8 (6 配列レシピの省略禁止)**: §3.3 の n, S_xy, S_x, S_y, S_xx, S_yy は
  1 本も省略できない。「事前にグローバル平均を引いたから S_x·S_y 補正は不要」
  という簡略化は、**共有欠測パターンがそのまま正の相関として析出する**最悪の
  アーティファクト (E1.1 の存在理由と同じ機序)。ゲート: 「同一の欠測パターンを
  共有する独立な 2 レーンが sig にならない」合成対照群テストを必須とする。
- **W-9 (irfft の整数量)**: n(τ) は数学的には整数だが irfft は 99.99999997 を
  返す。`n >= N_MIN` のゲート比較は必ず np.rint 後に行え — 生 float の境界比較は
  環境差で揺れる非決定論の混入点。
- **W-10 (分散項の微負)**: n·S_xx − S_x² は浮動小数で微小な負になり得る →
  sqrt が NaN。全計算は float64、分散項は max(·, 0.0) でクランプし、
  **両分散項 > 1e-9 を有意判定の前提条件**に加える (どちらかが実質ゼロの pair は
  unratable であり、0 除算の回避策ではなく情報量の判定である)。
- **W-11 (ラグ符号規約の固定)**: ρ_ij(τ) = corr(y_i(t), y_j(t+τ)) — 「i が j に
  τ 日先行」。負ラグは irfft 出力の末尾 (index M−τ) から取る。恒等式
  ρ_ij(τ) == ρ_ji(−τ) を合成データで必ずテストせよ — 符号規約の取り違えは
  「原因と結果の逆転」としてしか発見できず、テスト無しでは出荷まで潜伏する。
- **W-12 (帰無ラグにも同じゲート)**: null 分位点は n(τ') ≥ N_MIN を通過した
  遠ラグのみで計算する。マスクにより遠ラグほど n が痩せる — 小標本の高分散
  |ρ| を帰無に混ぜると q99 が膨張し検出力が死ぬ。有効 null ラグ < 50 の pair は
  sig=False + reason="null_insufficient" を記録 (unratable)。
- **W-13 (デトレンド禁止)**: 週次リズム・学期トレンドの除去を「前処理として」
  追加するな。遠ラグ帰無は信号と同じ定常構造を共有するため、これらを**自動で
  吸収する** — それが自給帰無の存在理由である (§2 Note 3)。scipy.detrend の
  提案は却下済み事項 (§1.6) の再犯として扱う。
- **W-14 (タイ破りの jitter 禁止)**: ゼロ過剰カウントレーンの大量タイは平均
  ランクのみで処理する。乱数 jitter によるタイ破りは I-17 違反であり、
  「実行のたびに結合行列が変わる占い機」への一本道。

**E2 の進軍条件**: E1.1 (是正 + 3 対照群テスト) の GREEN が先。実装順序は
E1.1 → E2 とし、E2 ゲート (§5.9) に W-8 の共有欠測対照群と W-11 の恒等式
テストを追加する。

### 5.10.4 (Rev.4) E3 への最終警告 (W-15〜W-21 — NumPy フルスクラッチ IRLS の数値的罠)

E3 は本プロジェクトで初めて「反復収束する数値最適化」を自前実装する段である。
E1/E2 の罠がレイアウトと欠測の罠だったのに対し、E3 の罠は**収束の偽装**と
**評価の自己汚染**である。全定数は本節の値で凍結する。

- **W-15 (シグモイドの飽和と NaN 連鎖)**: exp(-z) は z < −709 で float64 が
  オーバーフローする。σ の引数は**全経路** (IRLS 反復・予測・MC シミュレーション・
  D(t) の圧縮) で `Z_CLIP = 35.0` にクリップせよ。σ(±35) は 1 との差が ~6e-16 で
  情報損失は実質ゼロ。クリップを 1 経路でも忘れると、NaN は例外を出さずに
  行列演算を貫通し、「gate_passed=true なのに forecast が全 NaN」という最悪の
  形で出荷される。**fit/predict の入口と出口に `np.isfinite` の assert を置き、
  非有限値の検出は即 TwinFitError** — NaN の伝播は 1 ステップも許さない。
- **W-16 (完全分離とリッジの正しい位置 — §3.5 Rev.4 訂正の理由)**: 失策が
  「R < 0.4 の日にしか起きていない」ようなデータでは無制約ロジスティックの
  最尤解が存在しない (β→∞)。Hessian の λI は線形系を可解にするだけで発散は
  止めない。勾配側の −λβ (ペナルティ付き尤度の Newton) が発散を止める唯一の
  正しい位置である。「元の式のままでも動いた」は分離のないテストデータでしか
  動いていないという意味だ — **分離データの対照群テストを必須とする**
  (全 lapse を R 下位 20% に置いた合成データで ‖β‖ < 100 かつ収束すること)。
- **W-17 (条件数)**: X = [1, R] は R の分散が小さいと切片列と共線になる。
  (a) R を中心化してから解き β₀ を後で戻す、(b) fit 対象行の std(R) < 1e-3 なら
  unratable (gate_passed=false。エラーではなく情報量の判定)、(c) 逆行列は
  `np.linalg.solve` のみ — `np.linalg.inv` を書いたらレビューで落とす。
- **W-18 (収束の偽装禁止)**: 25 反復で ‖Δβ‖∞ < 1e-8 に達しない場合、最終
  反復値を「使ってよい」のは勾配ノルム ‖Xᵀ(y−p) − λβ‖∞ < 1e-4 のときだけ。
  それ以外は fit_failed → gate_passed=false。**収束しなかった最適化の出力を
  黙って採用するのは、テストの通らないコードを完了と言うのと同じ行為である。**
- **W-19 (walk-forward のラベル漏洩 — 本節で最重要)**: 失策ラベルの分位点閾値
  (p90/p95/p50) と D(t) の正規化 (median/MAD) を**全履歴から**計算してから
  walk-forward すると、未来のデータが過去のラベルと正規化を定義し、BSS が
  系統的に膨張する。これはツインの接待化 (§1.4) の数値版であり、スキルゲート
  (I-20) の存在意義そのものを殺す。**全ての閾値・正規化定数は各 fold の訓練窓
  のみから再計算し、テスト窓へは訓練窓の定数を適用せよ。** ゲート:
  「訓練窓定数とテスト窓定数が異なる合成データで、全履歴版より BSS が下がる」
  ことを確認する回帰テスト。
- **W-20 (B₀=0 と希少事象)**: 気候値 Brier B₀ = 0 (訓練窓に失策ゼロ) の fold で
  BSS を計算すると 0 除算。fold 単位の規則: 訓練窓 lapse < 5 または気候値
  p̄ ∉ (0,1) の fold はスキップ。全 fold スキップなら gate_passed=false。
  BSS の集計は fold 平均ではなく**プール方式** (全テスト日の予測を連結して
  1 つの B と B₀ を計算) — 希少事象では fold 単位 BSS の分散が大きすぎる。
  既存条件 (プールした n_lapse_test ≥ 10) は維持。
- **W-21 (欠測日の力学)**: 状態方程式の入力 (rec/負荷レーン) が欠測の日は
  **訓練窓の中央値で代替** (事前に 1 回だけ決定論的に計算。ゼロ埋めは I-18 の
  変奏で禁止 — 0 は「回復ゼロ・負荷ゼロ」という強い値である)。観測対応物
  D(t) が欠測の日は SSE から除外 (マスク付き SSE)。有効 D 日数 < 60 なら
  unratable。グリッド探索は G 軸ベクトル化 (状態 shape=(G,) を N 回更新) で、
  substituted 入力の事前計算はグリッドループの外で 1 回だけ行うこと。

**E3 のゲート追加 (§5.9 の E3 行に加算)**: W-16 分離対照群 / W-19 ラベル漏洩
回帰テスト / W-15 の NaN 非伝播 (非有限入力で TwinFitError) の 3 本。

### 5.10.5 (Rev.5) 統合裁定 — E4 先行・Foxtrot 後続、および境界防衛

**裁定: E4 → Foxtrot (F0→F1..F5)。並行実施は禁止。** 論拠:
1. **休眠中のガードを最初に起こす。** `tests/test_oracle.py` (I-19 / 無菌検査) は
   E4 完成まで SKIP のままであり、この状態で UI を先に建てるのは「最重要の
   防壁が眠っている間に開口部を増やす」行為である。E4 のゲートは「test_oracle
   が SKIP なしで GREEN」— 防壁の起動そのものが完了条件になっている。
2. **契約が先、消費者が後。** oracle_payload.v1 はコードとして存在し無菌検査に
   通ってから初めて「契約」になる。存在しない payload を想像して UI を書くと、
   UI 側が独自のデータ形状を発明し、後で境界の両側を直す羽目になる。
3. **宙吊りスタブの根絶。** Foxtrot F4 (PreSessionBriefing) は E4 に依存する。
   E4 先行なら Foxtrot は一気通貫で書ける — 「後で配線する」スタブは漏洩の温床。

**E4 実装ノート (Sonnet5 拘束)**:
- **`oracle.payload` (LLM なし・無菌 JSON のみ) と `oracle.report` (LLM 言語化
  込み) を別 stdio cmd に分離せよ。** UI の数値表示とテストは payload だけで
  済み、7B の生成を待たない。ui_smoke は payload 経路を必ず踏む。
- **deep_profile スキーマ追従 (AI_SKILLS §6.5 の規律)**: coupling/TwinParams/
  oracle_payload を deep_profile へ書くならスキーマ版数を上げ、読み手
  (`_gap_section()` / `update_user_profile()`) の追従を同一変更で行え。
  E0 フィクスチャの「認知リソース」マーカーは、E4 で実注入が入った後も
  講評フェーズのみで GREEN であり続けること (ガードの意味の維持)。
- **dyad スコープは E4 では未配線と宣言する。** focus dyad の指定 UI/config が
  存在しない以上、`scope="dyad"` は unratable を返す正直なスタブに留めよ。
  半実装 (グローバルデータで代用等) は I-19/T-19 の温床。
- **TensorStore のライフサイクル**: fit_twin/coupling は「open → 計算 → close」を
  profiler 実行内で完結させる。エンジン常駐プロセスに store をキャッシュするな
  (再構築時の T-14 を構造的に回避)。
- **実測義務**: E4 完成時に実データ相当規模 (N=3650) で Echo 計算合計
  (tensor build + coupling + fit_twin) の実測値を AI_SKILLS へ記録せよ。
  E5 ゲート (3000ms) の判定は計測値でのみ行う。

**境界防衛 (漏洩の主脅威は read ではなく echo-back)**:
バックエンドの構造的隔離 (面接官プロンプトは ES+トランスクリプトのみから構築)
が第一防壁であり、UI 規律は defense-in-depth である。それでも以下を強制する:
- **stdio の consult 系 dispatch は未知パラメータを黙って無視する**ことを
  テストで固定 (UI が誤って余計な state を送っても面接官経路に到達しない)。
- engine.ts のリクエスト組み立てで **UI state のスプレッド展開 (`...state`) を
  禁止** — 送るフィールドは凍結された TypeScript interface の明示列挙のみ。
- oracle/twin データの UI 保持は PROFILE 系ペインと PreSessionBriefing の
  コンポーネントローカル state のみ。App への state リフトアップ禁止。
```

---

# §6【Target Simulation】極限ドメインでの決定論的アプローチの証明

以下の数値は**動作説明のための仮想値**である (実データ非接触の原則)。

## 6.1 Target A — クオンツ/C++ エンジニア技術面接 (極限認知負荷)

1. **結合行列が個人法則を発見する** (仮想例):
   `cal_private_hours → productivity_idx` が lag=+1, ρ=0.44 (sig) —
   「回復の翌日に生産性が立つ」。`cal_switch_count → line_reply_med_min` が
   lag=0, ρ=0.38 (sig) — 「切替の多い日は即日、応答レイテンシが劣化する」。
   これは stabilizer_effect (§6.4-4) の連続量版であり、語彙ではなく物理量で出る。
2. **ツインが面接シナリオを負荷試験する**: scenario.mode="interview" で日内格子を
   回す。過去の interview_sim レイテンシ履歴から δ̂ (トピック遷移コスト) = 18s、
   R(day) = 0.55 (前日が switch 過多) とすると、P(R < θ_R) が開始 40 分で 0.6 を
   超える — 「Blind 75 の 2 問目と行動面接の切替点で論理破綻が起きる」という
   **分単位の予測**が出る。
3. **Oracle は事前介入のみ**: R-TAPER-01 発火 → iv-005 (前 48h の switch 上限) +
   iv-004 (結合行列の lag に合わせた回復ブロック配置)。面接**中**への介入は
   存在しない (§1.6 — interview_sim の状態機械は不可侵)。
4. **講評フェーズでの統合 (合法出口)**: 実セッションの latencies と予測曲線の
   突合を講評にのみ渡す。「予測された 40 分地点で実際に応答が 2.3 倍に劣化した」
   は、面接が日常の縮図であることの物理量による証明になる。

## 6.2 Target B — 長期関係の過剰投資検知 (Yurika プロトコル)

**最初に構造を固定する**: 対象は指定時に即 alias 化 (以後 "C-7c2e" と表記)。
`tensor_dyad_C-7c2e.bin` が生成され、被写体は**常に本人**である。

1. **OII が焦燥を物理量で検出する** (仮想例): 直近 14 日で本人送信文字数 z=+2.1、
   本人起点バースト z=+1.8、タグ付き支出 EMA z=+1.6、本人レイテンシ −z=+1.4 →
   OII_ema = 1.7 ≥ 1.5 が 4 日連続。**相手の温度は一切測っていない** — 測って
   いるのは「自分が払うコストの一方的な加速」だけであり、それで十分である。
2. **ツインが失策を予報する**: 過去の失策ラベル (深夜フラッド送信・宣言タスクの
   放棄と裁量支出の同時発生) で fit したハザードが gate_passed (BSS=0.11) なら、
   「現在の R=0.4 と OII 加速が続く場合、7 日以内の失策確率 0.55」が出る。
3. **介入は本人の行動ゲートのみ**: R-OII-01 発火 → iv-001 (R 回復までの送信保留) +
   iv-002 (支出クールオフ) + iv-003 (不可逆コミットのモラトリアム)。言語化には
   「LINE 上の観測範囲では」の限定句が必ず付く (LINE に写らない対面関係を
   「疎遠」と誤認する交絡 — T-12 — をユーザーに開示し続ける)。
4. **やらないことの証明**: 相手の返信速度から「脈」を推定しない。返信遅延で
   相手の関心を操作する指示を出さない。相手の人格プロファイルを作らない。
   これらは I-15/I-19 と INTERVENTION_BANK の構造により、**実装者が意図しても
   書けない** — 誘惑への構造的退路の遮断こそが本設計の防御である。

---

# §7【Fable Heuristics Archive】マスター・ヒューリスティクス

未来の Sonnet と人間へ。個別の仕様は腐るが、以下の推論アルゴリズムは腐らない。

1. **動かさない境界面はバグを生まない。** 新機能はまず「既存の凍結境界面
   (PKBVEC01/PKBSCR01/チャネル定義) を 1 バイトも動かさずに建てられるか」を問え。
   動かす必要が出たら、それは機能の要求ではなく設計の失敗の兆候である。
2. **生成ではなく選択。** LLM に出力の自由度を与える箇所は全て漏洩・幻覚・
   非再現の面積である。静的バンク + 決定論的選択に置換できるなら必ず置換しろ
   (QUESTION_BANK → INTERVENTION_BANK は同一パターンの 2 回目の適用)。
3. **測れないものを推定するな。測れるものだけで殴れ。** 「相手の気持ち」「本当の
   性格」を変数に置いた瞬間、システムは占い機になる。レイテンシ・金額・件数・
   文字数は嘘をつかない。
4. **型シグネチャで誘惑を封じろ。** 「してはいけない操作」はレビュー規約ではなく
   関数シグネチャで禁止しろ (embedder を受け取らない compaction、テキストを
   受け取らないツイン、target_lane 必須のバンク)。規約は忘れられるが、型は
   忘れられない。
5. **確度を自己申告しないシステムを信用するな。** data_sufficiency →
   MIN_EXCHANGES → BSS スキルゲートは全て同じ原理の再適用である。新しい推定器を
   設計したら、最初に「これはいつ黙るべきか」を定義しろ。
6. **乱数は排除ではなく統治。** 確率的手法が要るなら、種を入力内容のハッシュに
   縛れ。「モンテカルロだから再現できない」は設計放棄の言い訳である。
7. **帰無分布は自給しろ。** 教科書の検定は i.i.d. を仮定し、生活ログは i.i.d. では
   ない。データ自身の遠ラグ/サロゲートから帰無を取れば、仮定ではなく実測で
   有意性が立つ。
8. **派生データに原本の機構を移植するな。** 再構築が安い成果物 (テンソル) に
   LSM/墓標/差分を持ち込むのは、高価な機構への憧れであって設計ではない。
   コストが痛くなってから (計測してから) 差分化しろ。
9. **ゲートなき最適化は退化と同義。** C2 も E5 も「実測が予算を超えたら」という
   発火条件付きで封印してある。封印を解く鍵は常にベンチマーク数値であり、
   直感・暇・美意識ではない。
10. **聖域の拡張は防壁の拡張から始めろ。** 新しい情報チャネルを作る前に、その
    チャネルが漏れてはいけない場所のガード (マーカー・assert・RED テスト) を
    先に書け。「ガードが先、機能が後」は工程の好みではなく、聖域が聖域である
    ための存在条件である。
11. **倫理制約と精度制約が対立して見えたら、設計が浅い。** 第三者プロファイル
    禁止 (I-15) も介入標的の限定 (I-19) も、突き詰めれば「測れないものを目的
    関数から外す」という精度上の正解と一致した。対立が消えるまで掘れ。
12. **自己破壊テストを公開しろ。** 棄却案の墓場 (§1.6) を書き残さない設計書は、
    次の世代に同じ検討を強制する。設計の価値は採択案の質だけでなく、棄却の
    論拠の質で決まる。

---
*本仕様書は Target Echo Genesis セッションの成果物である。実装者へ: 疑ったら
計測しろ。計測できないなら、それはまだ設計が終わっていない。そして測れない
ものを推定し始めたら、それはもう本システムではない。*
