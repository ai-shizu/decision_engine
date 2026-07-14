# THE_ARCHITECTS_MANIFESTO.md — PKB 全状態圧縮引き継ぎ書 (第2世代)
# 発行: 2026-07-07 / 発行元: Target Echo 完遂セッション (fable5, リード・アーキテクト)
# 宛先: 新セッションの AI。本書を読了した時点で、あなたは本プロジェクトの
#        リード・アーキテクトである。読了後は「コンテキスト・ロード完了」とだけ応答し、
#        指揮官 (ユーザー) からのミッション投下を待て。
# 本書はブートローダである。真の記憶は docs/ に永続化済み — 本書の役割は
# 「どこに何があるか」と「ドキュメント未反映の生きた状態」の伝達のみ。

---

## 0. 体制と行動規約 (前世代から不変)

- **指揮官**: ユーザー。ミッション形式で指示。**コミット・プッシュは明示指示時のみ**。
- **設計**: fable5 (あなた)。Architect's Override 権限 — ベースラインの上書きは推奨
  されるが、論拠を必ず Architect's Note として文書化。**自分の式・指示も再監査対象**
  (本セッションで IRLS 式と test_oracle 指示の 2 件を自己訂正した実績がある)。
- **実装**: Sonnet5。SPEC は「一発実装できる粒度 + 拘束条件」で書く。Sonnet5 の
  自己判断は禁じるが、**SPEC 未規定領域での最小拡張は「申告義務付き」で許す**
  (E1 の 3 拡張 → レビューで全承認、の運用実績)。
- **第0原則 (2026-07-08 改訂 — ルーティング化)**: 全作業は `docs/AI_SKILLS.md`
  §0 のルーティング表に従い「§1 (絶対原則) + タスク種別に対応する節」を読んで
  から開始する (全文読了の強制は撤回済み — コンテキスト汚染とクレジット浪費が
  Foxtrot 突入前のチェックポイントで問題視されたため)。終了時に as-built・
  不変条件・ハマりどころを実際に触れた節へ追記してから離れる規律は不変。
  省略した作業は未完了扱い。
- **実証済み規律**: ガードが先 (RED 確認→機能)、計測なき最適化・完了宣言の禁止、
  スコープクリープ禁止、テスト全 PASS まで「完了」と言わない、**完了の偽装
  (ファイル退避で ui_smoke を通す等) の絶対禁止**。

## 1. The Constitution (絶対憲法 — 交渉不可)

1. **完全オフライン**: TCP/IPはloopbackを含め全面禁止。許可IPCはRust↔Python stdioと、
   PKB所有のllama.cpp子へ接続するWindows Named Pipe / POSIX `/dev/stdin`のみ。
   `knowledge_fetcher`を含む外向き通信例外はE0aで撤廃済み。
2. **感情推定の排除・物理量の絶対視**: 発見は決定論、LLM は言語化のみ。レイテンシ・
   頻度・金額・文字数という嘘をつかない物理量だけで殴る。
3. **情報の非対称性 (聖域)**: gap/テレメトリ/Bounty/**Echo 全出力 (oracle_payload/
   twin/coupling/OII)** は面接官・GD 議論・es_review に絶対不出。統合は講評フェーズ
   + consult 静的プレフィックス + PROFILE UI の合法出口のみ (I-22)。
4. **Puppeteer の構成的無菌化**: 質問・介入は静的バンクからの「選択」のみ。LLM
   生成・リライトは情報漏洩バグ (INTERVENTION_BANK にも同原理を適用済み)。
5. **建前人格の隔離**: シミュレーター由来は simulated=True。テンソルの consult
   レーンも除外済み。
6. **UI ブラックボックス / ディスク ホワイトボックス**: UI は要約・演出可、ただし
   捏造禁止。ディスクには全計算根拠を正直に記録。
7. **第三者は刺激であって被写体ではない**: 実名は salt 付き一方向 alias のみ。
   **介入の標的は本人側特徴量レーンのみ (I-19)** — 相手の反応・感情を目的関数に
   置いた瞬間、システムは占い機に堕ちる。
8. **チャネル分離凍結**: 主観 / 客観 / 対人テレメトリの 3 チャネル定義は不変。
9. **stdlib 縛り** (core 決定論) + **決定論的乱数** (I-17: Philox + 入力内容由来 seed。
   unseeded RNG は 1 箇所でもバグ)。
10. **stdio プロトコル純度** + 全ファイル I/O に encoding="utf-8"。

## 2. The As-Built (到達地点 — 再実装禁止)

| Target | 状態 | 記録 |
|---|---|---|
| Alpha (KV ピニング) | 完了 | AI_SKILLS §8 |
| Bravo (mmap ゼロコピー IPC, 109µs/query) | 完了 (commit ffb1fbd) | §9 |
| Charlie C1 (LSM 化) | 完了 (commit 5773486, **未プッシュ**) | §10 |
| Delta DL1/DL2/D3 (テレメトリ/Bounty/Puppeteer/Narrative) | 完了 (同上) | §11 |
| Delta D1/D2 (5軸/PROBE/HistoricalNode) | **未実装** (設計は SPEC_CHARLIE_DELTA §3) | — |
| **Echo E0〜E4** | **完了 (全て未コミット・ワークツリーのみ)** | §12 + SPEC_ECHO Rev.5 |
| Echo E5 (C++ カーネル) | **不着手が正** — N=3650 実測 1.6〜2.2s < 3000ms ゲート | §12 |
| Foxtrot (UI) | 設計のみ (SPEC_FOXTROT Rev.2)。実装凍結中 (§3 参照) | §13 |

**Echo の中身 (全てワークツリー、コミットなし)**: `core/tensor_store.py` (PKBTEN01:
header 64B "<8sIIIIiIQ24x" / row 136B "<iI32f" / 22 レーン凍結 / 欠測=mask のみ I-18)、
`core/coupling.py` (FFT 相互相関 + **遠ラグ帰無** — 自給的有意性判定)、
`core/digital_twin.py` (状態方程式 + ペナルティ付き IRLS + **trailing causal
baseline (W-19)** + walk-forward スキルゲート I-20 + Philox MC)、`core/oracle.py`
(無菌 payload + 実行時 `_assert_sterile` + INTERVENTION_BANK)、C++ struct
(search_engine.cpp へ追加済み・コンパイル検証済み)、配線 (profiler/facade/
engine_stdio/engine.ts)、テスト 4 ファイル (tensor_store/coupling/digital_twin/oracle)。

**決定論的勝利の記録 (新セッションが誇りと規律の両方として継承すべきもの)**:
- W-19 実測: 素朴なグローバル閾値は無関係データに BSS=+0.12 (偽スキル) を与え、
  trailing causal baseline は BSS=-0.0007 で正しく棄却 — 接待化の失敗モードを
  実測で再現・防止した。
- 警告が実装に先行した E2/E3 では、W-9/W-10/W-15/W-16 系のバグが**一度も発現
  しなかった**。「バグが起きなかった」のではなく「起こす前に塞いだ」。
- E4 では逆に「hand-crafted payload のテストは実経路を実行しない」という盲点から
  実バグ 2 件 (T-14 の再発 + 再エクスポート漏れ) が N=3650 ベンチで発覚 →
  E2E 回帰ガード化。**形のテストと経路のテストは別物**。

## 3. The Current Crisis & Active Directives

### IMP-1 (最優先。AI_SKILLS §14 に指令全文)
- **事象**: data/processed/metadata.json が 4.2GB。ui_smoke が MemoryError で RED。
- **検死確定事実**: LSM (Charlie) は**無罪** (台帳 208 件・重複ゼロ)。真犯人は
  import 層の冪等性欠如 — line_history.txt に同一エクスポートが **12 回** append
  され (facade._append_line_text は無条件追記)、「重複 × セッション橋渡し ×
  日数複製 × text/sessions 二重保持」の積で 7.3MB → 4.2GB (~600 倍)。
- **指令 (発行済み・完了報告は未受領 — 新セッションはまず git status と
  テスト実行で現在状態を実測せよ)**: (1) `_quarantine_20260707/` へ move (削除禁止・
  tensor_*.bin と raw は不可触)、(2) 根治 = load_line_messages のエクスポート
  ブロック単位**多重集合デデュープ (ブロック間 max、sum ではない)** + 対照群 3 本
  (本物の連投を失わないこと)、(3) `_save_meta` に 200MB トリップワイヤ、
  (4) フレッシュ再構築 → 13 スイート + ui_smoke GREEN。
- **E4 は ui_smoke GREEN まで正式クローズ不可** (DoD)。

### Foxtrot (凍結中)
IMP-1 完了 → E4 クローズ後に F0 (トークン移行・視覚差分ゼロ) から。絶対規律:
Tailwind/Framer Motion/D3/Three.js/Recharts 導入禁止 (SPEC §0 で却下済み — 再提案
するな)、**ツイン枯渇度による UI グリッチ禁止 (F-5: 自己成就予言ループ)**、タブは
unmount 必須 (F-11)、oracle 数値を共有クロームに出さない (F-12)、engine.ts で
UI state のスプレッド送信禁止 (F-13)。グラフは全てインライン SVG (閉形式座標)。

### SPEC 訂正の未処理 1 件
SPEC_ECHO_GENESIS §4 の「oracle 出力は consult **動的サフィックス**」は誤り。
as-built は **静的プレフィックス** (_gap_section と同居 — profiler 再実行時のみ
更新されるため KV 効率が高い)。次回 SPEC 改訂時に本文を訂正せよ (AI_SKILLS §12
E4 as-built に記録済み)。

## 4. The Future — The 5 Legacies (docs/MASTER_PLAN_LEGACIES.md)

着手順は **PHANTOM が最初** (校正装置なしの計測器増築は倒錯)。各着手時は
「個別 SPEC 錬成 → 憲法ガード RED → 実装」。

1. **PHANTOM** — 合成ペルソナ校正装置: 種付き決定論の合成生活ログに既知の真実を
   埋め込み、パイプライン全段の known-answer test を CI 強制。fixture-blindness
   規律。**IMP-1 の教訓により「実データ健全性トリップワイヤ (サイズ・重複率の
   定期 assert)」を設計に追加すること** (今セッションの実証済み必要性)。
2. **SCAVENGER** — デジタル排気 importer 群 (git/LeetCode/ブラウザ/フォーカス時間
   → 予約レーン 20-27)。importer 基底に `coverage()` を型強制 (E1.1 の教訓)。
3. **LARYNX** — 発話物理テレメトリ (whisper.cpp ローカル。ポーズ/フィラー/速度のみ。
   音調感情推定は永久禁止)。
4. **BLACKBOX** — 実戦飛行記録 + 選考結果台帳 + 実合否によるシステム校正
   (面接官プロファイル構築禁止)。
5. **CHRONOSCOPE** — 自己差分 + 介入効果の会計監査 (E2 遠ラグ帰無を再利用。
   n=1 の効果推定を「証明」と呼ぶな)。

## 5. ドキュメント地図 (真の記憶の所在)

| ファイル | 役割 |
|---|---|
| `docs/AI_SKILLS.md` | **全作業の起点**。憲法 + as-built §8〜13 + IMP-1 指令 §14 + Legacies §15 |
| `docs/SPEC_ECHO_GENESIS.md` (Rev.5) | Echo 全設計 + W-1〜W-21 + 裁定史 §5.10 |
| `docs/SPEC_FOXTROT_UI.md` (Rev.2) | UI 設計 + F-1〜F-13 |
| `docs/SPEC_CHARLIE_DELTA.md` (Rev.4) | D1/D2 の未実装設計 + I-1〜16 / T-1〜13 |
| `docs/MASTER_PLAN_LEGACIES.md` | 5 Legacies 青写真 |
| `tests/` | 品質ゲート 13 スイート + ui_smoke (現在 IMP-1 により RED) |

## 6. 環境要点 (前世代から不変 + 追記)

Windows on ARM (Snapdragon X)。clang++ (pwsh 不在時は `-O3 -std=c++17
-march=armv8-a+simd -fopenmp` 直叩き)。LLM 7B 未満禁止 (下限 3.5GB)。日付は
dateUtils.todayIso / date オブジェクト演算のみ (epoch 秒/86400 禁止 T-17)。
ui_smoke は実データ接触 + 数分かかる (バックグラウンド推奨)。コンソールは cp932 —
日本語を含む検証出力は UTF-8 ファイル経由で読め。**mmap 由来 ndarray は close 前に
del** (T-14 — 同一スコープの短命利用でも必要)。

## 7. 次のアクション (優先順)

1. **IMP-1 の完了状態を実測** (git status / 13 スイート / ui_smoke) → 未完なら完遂。
2. ui_smoke GREEN をもって **E4 正式クローズ** (AI_SKILLS へ記録)。
3. SPEC_ECHO §4 の動的/静的表記を訂正 (Rev.6)。
4. **Foxtrot F0〜F5** (SPEC Rev.2 準拠。F6/PROBE は D2+E4 依存のため後回し)。
5. 以降は指揮官の指示: D1/D2 (SPEC_CHARLIE_DELTA §3) または Legacies (PHANTOM から)。

---
*疑ったら計測しろ。計測できないなら、それはまだ設計が終わっていない。
測れないものを推定し始めたら、それはもう本システムではない。
そして自分の計測器は自分で校正しろ — 4.2GB の死体が今日それを教えた。*
