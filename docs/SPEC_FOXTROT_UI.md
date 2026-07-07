# SPEC_FOXTROT_UI.md — Target Foxtrot: フロントエンド UI/UX 完全設計仕様書
# 発行: 2026-07-07 / 起草: fable5 (リード・アーキテクト) / 実装担当: Sonnet5
# Rev.1
# Rev.2 (2026-07-07): E4/Foxtrot 統合裁定 (SPEC_ECHO §5.10.5) を受け F-11/F-12 を
#   追加。実装順序は E4 完遂後に F0 から着手 (並行禁止)。

> **読者への前提命令**: 本書を読む前に `docs/AI_SKILLS.md` を全文読め (第0原則)。
> 特に §3.4 (React UI 規約) は本書の**上位法**である。本書と §3.4 が矛盾して
> 見えたら §3.4 が正 — ただし本書 §0 の Architect's Note は §3.4 の適用解釈を
> 確定させるものであり、両者は矛盾しない。
>
> **Sonnet5 への絶対拘束**: 本書に無いデザイン判断をするな。色・余白・フォント・
> 遷移時間は全てトークン (§1) から取れ。リテラル hex / リテラル px を新規に
> 書いた時点でレビュー落ちである。

---

# §0【Architect's Note】ミッション要求への上書き (5 件) — 憲法が勝つ

発注書 (Target Foxtrot ミッション) の要求のうち 5 件を上書きする。いずれも
AI_SKILLS §3.4 の as-built 法規、またはシステムの測定原理との衝突である。
**発注の意図 (ハッカー的体験・認知負荷の制御) は全て別の手段で達成する** —
上書きは目的の放棄ではなく、手段の合法化である。

1. **Tailwind CSS / Framer Motion / Recharts / D3 / Three.js — 全て導入却下。**
   §3.4-2 が名指しで禁じている (「新 UI ライブラリを導入するな。現状は素の
   React + CSS で足りている」)。加えて: (a) 完全オフライン保証 — 依存が増える
   ほど「ランタイムで外へ出ないこと」の監査面積が広がる。(b) 本アプリの
   グラフ描画対象は最大 22 レーンの円環と 5 軸レーダー — **三角関数で座標が
   閉形式で出る図に、力学シミュレーションライブラリは要らない**。D3 の
   force layout は実行毎に配置が揺れる = 決定論の放棄でもある。代替は
   インライン SVG コンポーネント (§4.3) と CSS transition (§5)。
2. **Record タブの「白黒基調」却下。** アプリ全体は #0f1419 系ダークで統一
   されており (§3.4-1「新しい色を発明するな」)、1 タブだけ白背景にするのは
   コヒーレンスの破壊 + 夜間入力 (日記の主用途) で網膜を焼く。フリクション
   レスの本体は**色ではなく入力レイテンシとキーストローク数**である —
   達成手段は §2.1 (autofocus・ショートカット・楽観的保存表示)。
3. **Consult の「OLED ピュアブラック (#000)」却下。** 既存パレット法規違反に
   加え、純黒はスクロール時の OLED スミアと過大コントラストを生む。既存の
   #0f1419 / #0b1016 の 2 段深度で「没入」を作る (§2.3)。
4. **INTERVIEW の「ツイン枯渇度による UI グリッチ」却下 — 本書で最重要の上書き。**
   理由は装飾禁止 (§3.4-1) だけではない。response_time_sec は講評とツイン学習の
   **測定値**である (AI_SKILLS §7.1.3)。ツインの予測でセッション中の UI を
   妨害すると、「ツインが枯渇を予測 → UI がユーザーを妨害 → 成績が落ちる →
   ツインの予測が的中」という**自己成就予言ループ**が閉じる。これは MBTI
   投影を分析入力に戻すな (罠 T-8) と同型の発振であり、ツインの予測スキル
   (BSS) を UI が偽装することになる。Echo データの UI 接続は**セッション前
   ブリーフィング / 講評後表示のみ** (F-5)。セッション中の緊張演出は
   セッション内観測量のみから作る (§2.4)。
5. **絵文字バッジ (🟢/🔴) 却下。** 絵文字はプラットフォーム毎に描画が揺れ、
   モノスペース表とベースラインが合わない。テキストグリフ (●/○/▲) +
   トークン色で置換する (§2.2)。ハッカーの美学は絵文字ではなく等幅の整列だ。

**採用した発注意図**: 認知負荷マッピング (タブごとに負荷プロファイルを変える)、
モノスペースの活用、HUD 的な INTERVIEW、PROBE の学術的静謐、iOS 的 SETTINGS、
「情報の引き算」(数理の内部ではなく結論だけを見せる — ただしディスク側は
ホワイトボックスのまま。憲法 6)。

---

# §1【Design System & Tokens】

## 1.1 実装形式 — CSS Custom Properties (App.css `:root`)

Tailwind の代替はトークンである。**全ての色・余白・書体は以下の変数から取る。**
値は既存 App.css からの抽出であり、1 色も発明していない (§3.4-1 準拠)。

```css
:root {
  /* ---- 色: 深度 3 段 + 状態 ---- */
  --bg:          #0f1419;   /* 基調 (既存 :root background) */
  --bg-raised:   #121820;   /* パネル・カード (既存 topbar/calendar) */
  --bg-deep:     #0b1016;   /* 入力欄・コード面 (既存 input/textarea) */
  --bg-hover:    #1a2533;   /* ホバー行 (既存) */
  --bg-selected: #1e2a38;   /* 選択行 (既存) */
  --bg-info:     #0d1b2a;   /* 情報ブロック (既存) */
  --border:      #2a3441;
  --text:        #e8eaed;
  --text-muted:  #9aa4b2;
  --accent:      #3498db;   /* 操作・選択・リンク */
  --ok:          #7dcea0;   /* 成功・稼働 (既存 subtitle/status-line) */
  --ok-strong:   #2e7d4f;
  --err:         #e74c3c;
  --err-soft:    #f1948a;
  --err-bg:      #2a1215;

  /* ---- タイポグラフィ (ローカルフォントのみ — CDN 禁止) ---- */
  --font-ui:   "Segoe UI", system-ui, sans-serif;
  --font-mono: "Cascadia Mono", Consolas, "SF Mono", ui-monospace, monospace;

  /* ---- 余白スケール (既存実測値の公理化) ---- */
  --s1: 4px;  --s2: 8px;  --s3: 12px;  --s4: 16px;  --s5: 24px;  --s6: 48px;

  /* ---- 形状・運動 ---- */
  --radius: 8px;  --radius-s: 4px;
  --t-fast: 120ms;         /* 状態フィードバック transition の唯一の時定数 */
}
```

## 1.2 書体の使い分け (F-10)

| 用途 | フォント | 例 |
|---|---|---|
| 散文・ラベル・ボタン | `--font-ui` | 日記本文、相談文、見出し |
| **全てのデータ値** | `--font-mono` | 日付、金額、件数、レイテンシ秒、バッジ、進行度%、タイマー |

データ値の等幅化が「ハッカーライク」の実体である。桁が揃い、更新しても
レイアウトが動かない。装飾ゼロで統制感が出る。

## 1.3 タイプスケール (既存実測の凍結)

h1 1.4rem / h2·h3 1.1rem / 本文 1rem / 補助 0.9rem / メタ 0.85rem。
新しいサイズを発明するな。

## 1.4 移行指令 (マイルストーン F0)

App.css の全リテラル hex (~80 箇所) を上記 var() へ機械置換する。
**視覚的差分ゼロが F0 のゲート** — 置換前後のスクリーンショットを比較し、
1px の差も出ないこと。`npx tsc --noEmit` + `python tests/ui_smoke.py` 必須。

---

# §2【Tab Architecture Details】6 タブの認知負荷マッピング

タブ構成: 既存 5 タブ + PROBE (新設・バックエンド D2/E4 ゲート付き)。
`App.tsx` の `TABS` 配列に追加するだけ (既存パターン)。状態管理は全タブ共通の
既存規約を凍結: **タブローカル useState / マウント時フェッチ / 保存成功時
再フェッチ / グローバルストア禁止** (§3.4-5)。

## 2.1 RECORD — 認知負荷: ゼロ (筋肉記憶で完結する入力機械)

```
RecordTab                          ← 状態: date, subTab, events[], transactions[],
├── CalendarPicker (既存)             diary, saveNotice, busy (全て既存)
├── SubTabs (events|money|diary)
├── QuickAddRow                    ← time+title / cat+amount の 1 行フォーム
├── DayList                       ← 当日の記録一覧 (mono で金額右揃え)
└── SaveBar                       ← Ctrl+S ヒント + saveNotice (--ok / --err)
```

- **起動フリクションの根絶**: タブ表示時に当日 (todayIso) を自動ロード (既存) +
  **diary サブタブでは textarea へ autofocus**。マウスに触れず「起動 → 打鍵 →
  Ctrl+S」で完結すること。
- キーボード経路 (F-4): Ctrl+S 保存 (既存)、Enter で QuickAddRow 追加、
  Ctrl+1/2/3 でサブタブ切替。
- 保存フィードバックは `--t-fast` の opacity transition で saveNotice を表示。
  ダイアログ・トースト禁止 (視線移動 = フリクション)。
- 「白基調」は却下済み (§0-2)。ミニマルの実体は**フォーム要素数を増やさない
  こと** — 新しい入力 UI を足す変更はこのタブでは原則リジェクト。

## 2.2 IMPORT — 認知負荷: 低 (一覧性 = 統制感)

```
ImportTab                          ← 状態: sources[] (name/status/count/mtime),
├── SourceTable                       busy, importStatus (status イベント文字列)
│   └── SourceRow × N             ← mono 1 行: グリフ + 名前 + 件数 + 最終同期
└── ImportActions                 ← 取込実行・再インデックス
```

- **VS Code 風ツリーは却下し、等幅フラットテーブルを採用。** データソースは
  高々 6 種 (diary / line / calendar / finance / es / knowledge) — 階層のない
  ものに階層 UI を与えるのは情報の足し算である。
- ステータスグリフ (§0-5): `●` (--ok) = Indexed / `○` (--text-muted) = Missing /
  `▲` (--err-soft) = Error。件数と mtime は mono 右揃え。
- 長時間処理 (import.line 等) は **status イベントを 1 行ずつ追記表示** (F-6。
  §3.4-7「busy フラグでボタンを殺すだけの UI は不合格」)。

## 2.3 CONSULT — 認知負荷: 会話のみ (チャットの既定形を裏切らない)

```
ConsultTab (既存を維持)            ← 状態: messages[], input, streaming,
├── MessageList                       statusLine (既存 pkb-engine-event 購読)
│   ├── UserBubble                ← --bg-selected / 右寄せ
│   └── AiBubble                  ← --bg-raised / 左寄せ / 本文 68ch 上限
├── StatusLine                    ← 検索中… 等の中間イベント (--ok, mono)
└── Composer                      ← textarea + Ctrl+Enter 送信 (既存)
```

- 没入 = **本文の可読測度 (max-width: 68ch)** と深度 2 段 (--bg の上に
  --bg-raised のバブル)。ピュアブラックは却下済み (§0-3)。
- ストリーミング chunk は逐次 append (既存)。スクロールは「最下端に居る時
  のみ」自動追従 — 読み返し中のユーザーを引きずり下ろすな。
- DeepSeek-R1 の `<think>` はバックエンドが除去済み (AI_SKILLS §5-4)。
  UI で再パースするコードを書くな。

## 2.4 INTERVIEW — 認知負荷: 意図的に高 (ただし全て正直な観測量で)

```
InterviewTab (既存 392 行を基盤に拡張)
├── PreSessionBriefing            ← ★Echo 合法出口: oracle の taper 介入等を
│                                    セッション開始前にのみ表示 (F-5)
├── SessionHUD                    ← 新設。全て mono・1 行固定高
│   ├── LatencyTimer              ← AI 発言表示完了からの経過 mm:ss (既存計測の可視化)
│   ├── TensionMeter              ← 10 セグメントバー (下記の決定論式)
│   └── TurnCounter               ← T03 のような 0 詰め表示
├── TranscriptList                ← 既存
└── AnswerComposer                ← 既存 (response_time_sec 計測点を変えるな)
```

- **TensionMeter の決定論式 (セッション内観測量のみ — F-5)**:
  `tension = clamp01(0.2 + 0.1·min(turn/10,1) + 0.25·min(lat_cur/lat_med − 1, 1)
  + 0.15·follow_up_depth/3)`。lat_med = セッション内自分レイテンシ中央値、
  follow_up_depth = 同一トピックへの連続深掘り回数 (UI が保持する表示用状態)。
  セグメント色: 0-4 --ok / 5-7 --accent / 8-9 --err。**バックエンドへ送らない・
  永続化しない・講評プロンプトに入れない** (表示専用 — 罠 T-8 の UI 版隔離)。
- LatencyTimer は「見せる」こと自体がストレッサとして機能する (本番面接の
  沈黙圧の再現)。計測起点は既存規約 (AI メッセージ表示完了時刻) を変えるな。
- **ツイン駆動グリッチは永久却下** (§0-4)。講評後に「予測 vs 実測」の
  レイテンシ比較を表示するのは合法 (講評フェーズ = Echo 統合の合法出口)。

## 2.5 PROBE — 認知負荷: 深く静かに (学術の静謐 = 装飾の不在)

**ゲート: バックエンド D2 (PROBE ファネル) + E4 (oracle/profile API) の完成まで
実装着手禁止。** タブ自体を追加しない (未実装機能のプレースホルダタブは
情報密度ゼロの UI であり不合格)。

```
ProbeTab
├── FunnelPane                    ← probe.next / probe.answer (D2 API)
│   ├── StageIndicator            ← FACT → CONTEXT → EMOTION → MEANING の
│   │                                4 点ステッパ (現在段のみ --accent)
│   ├── QuestionCard              ← 1 問のみ表示。前問・次問は見せない
│   └── AnswerComposer            ← autofocus + Ctrl+Enter
└── ProfilePane                   ← get_source_code / oracle.report (E4 API)
    ├── AxisRadar                 ← 5 軸レーダー (インライン SVG。§4.3)
    ├── ProgressLine              ← 進行度% (mono・高水位標は backend 実装)
    ├── MbtiProjection            ← "EsTj" 揺らぎ表示 (小文字 = 未確定。text のみ)
    └── CouplingChord             ← 結合行列の円環図 (インライン SVG。§4.3)
```

- 「畏敬の念」の設計: パーティクルではなく**余白と非対称**で作る。FunnelPane は
  1 問だけを --s6 の余白の中央に置く。装飾が無いほど質問の重さが立つ。
- 数理の内部 (FFT・帰無分布・IRLS) は**見せない** (情報の引き算)。見せるのは
  結論のみ: 「私的時間 → 生産性 (+1日, ρ=0.44)」のような有意結合の言明。
  監査したいユーザーは deep_profile.json を開けばよい (憲法 6 — UI ブラック
  ボックス / ディスク ホワイトボックス)。

## 2.6 SETTINGS — 認知負荷: ゼロ (iOS の既定形を裏切らない)

```
SettingsTab (既存 149 行を再構成)
├── SettingsList                  ← 角丸グループ (--bg-raised + --radius)
│   └── SettingRow × N           ← 左: ラベル / 右: 値 or Toggle
├── Toggle                        ← CSS-only スイッチ (§5.2)
└── AdvancedSection               ← <details> 1 個のみ (下記の限定例外)
```

- **Advanced 折りたたみは §3.4-3 (情報を隠すな) の限定例外として認可する。**
  射程: 「日常操作で不要かつ誤操作が consult を破壊し得る項目」(LLM モデル
  パラメータ・ポート・KV キャッシュ制御) のみ。日常情報 (データパス表示・
  同期状態) を畳んだらレビュー落ち。実装は native `<details>` — JS 状態を
  持たない。

---

# §3【Sonnet-Control Directives】実装拘束

## 3.1 技術スタック (凍結)

- **素の React + 単一 App.css。** ライブラリ追加は 0 件 (§0-1)。CSS-in-JS 禁止、
  CSS Modules 禁止 (既存構成の変更はスコープ外)。App.css が 1,200 行を超えたら
  タブ別分割を**提案** (勝手にやるな)。
- クラス命名: 既存踏襲のケバブケース + 機能接頭辞 (`hud-`, `probe-`, `import-`)。
  BEM 等の新命名体系を持ち込むな。
- コンポーネント: 名前付き export の関数コンポーネント (既存規約)。props は
  明示型。`useEffect` のリスナー (Tauri listen 含む) は必ずクリーンアップを
  返す (§3.4-6)。
- 新機能の配線順序 (§1-3 責務分離): `core/facade.py` → `engine_stdio.py` dispatch
  → `src/lib/engine.ts` ラッパー → コンポーネント。**UI から stdio ペイロードを
  直接組み立てるな。**

## 3.2 スタイル記述規則

1. 色・余白・フォント・時定数は **var() のみ**。リテラル値の新規記述は
   レビュー落ち (F-1)。
2. `box-shadow`・`linear-gradient`・`filter` 禁止 (§3.4-1 の機械的判定基準)。
   深度は背景色 3 段 (--bg / --bg-raised / --bg-deep) と 1px border で表現する。
3. レイアウトは flex / grid。position: absolute は SessionHUD のような
   固定高オーバーレイのみに限定。
4. `prefers-reduced-motion: reduce` で全 transition を無効化する media query を
   App.css 末尾に必ず置く (F-3)。

## 3.3 状態管理規則 (既存規約の凍結)

- サーバー状態の複製禁止 (§3.4-5)。取得はマウント時、更新は保存成功時の再取得。
- グローバルストア (Redux/Zustand/Context) 導入禁止。タブ間で共有したくなった
  データは「本当に両タブが必要か」をまず疑え — 大抵は片方の設計が間違っている。
- `MainTab` 型 (src/lib/types.ts) への `"probe"` 追加は D2/E4 完成時のみ。

## 3.4 F 系不変条件 (レビュー基準)

- **F-1**: 色は 15 トークンで閉じる。新 hex = 却下。
- **F-2**: UI ライブラリ 0 依存。グラフは全てインライン SVG コンポーネント。
- **F-3**: 運動は状態フィードバックの transition (`--t-fast`, opacity/transform
  のみ) に限る。入場アニメ・ループアニメ・パララックスは装飾 = 禁止。
- **F-4**: 全機能にキーボード経路。保存 Ctrl+S / 送信 Ctrl+Enter (既存規約)。
- **F-5**: **Echo/twin/oracle のデータはセッション中の面接 UI を駆動しない。**
  接続点はセッション前ブリーフィングと講評後表示の 2 つだけ。TensionMeter 等の
  セッション内演出はセッション内観測量のみから計算し、永続化もバックエンド
  送信もしない (自己成就予言ループの遮断 — 罠 T-8 の UI 適用)。
- **F-6**: 2 秒を超え得る処理は status イベントの逐次表示必須 (§3.4-7)。
- **F-7**: モーダルダイアログ禁止。破壊的操作の確認はインライン 2 段クリック
  (「削除」→「本当に削除」) で行う。
- **F-8**: `<details>` の使用は SETTINGS の Advanced 1 箇所のみ (§2.6 の限定例外)。
- **F-9**: UI クロームに絵文字禁止。グリフは ● ○ ▲ + トークン色。
- **F-10**: データ値は必ず `--font-mono` (§1.2)。
- **F-11 (Rev.2)**: **タブパネルは条件レンダリングで unmount する。display:none
  等の keep-alive 化は禁止。** 「タブ切替を速くするため」の hide 化は、面接
  セッション中に oracle/twin データ (PROFILE ペインの state と DOM) を生かした
  まま残す — 画面共有・スクリーンショット・state 参照の全てが漏洩面積になる。
  unmount は状態消去の構造的保証である (App.tsx の既存パターンを凍結)。
- **F-12 (Rev.2)**: **oracle/twin/OII 系の数値を共有クローム (topbar・subtitle・
  status line) に出さない。** 表示は PROFILE 系ペインと PreSessionBriefing の
  内部のみ。共有クロームに出した瞬間、面接セッション中に自分の R 値が視界に
  入り続け、F-5 が禁じた測定汚染 (自己成就予言) が UI 導線経由で復活する。
- **F-13 (Rev.2)**: engine.ts のリクエスト組み立てで UI state のスプレッド展開
  (`...state`) 禁止。送信フィールドは凍結 interface の明示列挙のみ
  (echo-back 漏洩の遮断 — SPEC_ECHO §5.10.5)。

## 3.5 Definition of Done (フロント変更時)

```powershell
npx tsc --noEmit          # apps/desktop
python tests\ui_smoke.py  # ALL PASS
cargo check               # src-tauri (Rust に触れた場合のみ)
```
+ F0 (トークン移行) のみ追加ゲート: 前後スクリーンショットの視覚的差分ゼロ。

---

# §4【Interaction & Animation】

## 4.1 運動の哲学

ハッカーライクの運動とは「機械が応答している証拠」であり「演出」ではない。
許される運動は 1 種類だけ: **ユーザー操作への状態フィードバック**
(`transition: opacity var(--t-fast), transform var(--t-fast)`)。
自発的に動く UI (点滅・パルス・パーティクル) は、ユーザーの注意を機械の都合で
奪う行為であり全面禁止。例外は LatencyTimer の秒表示更新 (これは情報の更新で
あって演出ではない) のみ。

## 4.2 マイクロインタラクション定義 (全箇所列挙 — ここに無いものは実装するな)

| 箇所 | トリガ | 効果 |
|---|---|---|
| ボタン hover | pointer | background → --bg-hover (--t-fast) |
| タブ切替 | click / Ctrl+n | active タブ背景 --accent (transition なし — 即時) |
| saveNotice | 保存成功/失敗 | opacity 0→1 (--t-fast)、3 秒後 1→0 |
| status イベント行 | 受信 | 追記のみ。スクロール追従は最下端時のみ |
| TensionMeter | ターン確定時 | セグメント色の即時更新 (アニメ禁止 — 計器は跳ねない) |
| Toggle | click / Space | ノブ transform translateX (--t-fast) |
| chat 送信 | Ctrl+Enter | Composer クリア + 楽観的 UserBubble 追加 |

## 4.3 インライン SVG コンポーネント規約 (グラフ描画の全て)

- **AxisRadar**: `<polygon>` 1 枚 + 軸線 5 本。頂点座標は
  `(cx + r·score·cos θₖ, cy + r·score·sin θₖ)`, θₖ = 2πk/5 − π/2。
  stroke --accent / fill --accent + fill-opacity 0.12。confidence < 0.5 の軸は
  stroke-dasharray "4 3" (未確定の視覚言語)。
- **CouplingChord**: 22 レーンを円周上に等間隔配置 (θᵢ = 2πi/22 − π/2)。
  sig ペアのみ `<path>` の 2 次ベジェ弦 (制御点 = 円中心) で結ぶ。
  stroke-width = `1 + 3·(|ρ|−0.15)/0.85`、正の ρ = --ok / 負 = --err。
  `<title>` 要素でネイティブツールチップ (`私的時間 → 生産性 | lag +1d |
  ρ +0.44 | n 210`) — JS ゼロ。**配置は決定論** (レーン番号順の円環) であり、
  力学レイアウトの揺らぎは存在しない。
- **TensionMeter**: `<rect>` × 10。点灯 = トークン色 / 消灯 = --border。
- viewBox 固定・width 100%。SVG 内にテキストを置く場合も --font-mono を継承。

## 4.4 ゲーミフィケーションの境界線

ゲーミフィケーションは「本物の観測量の提示方法」に限る (LatencyTimer・
TensionMeter・進行度高水位標)。**偽の数値・偽のランダム性・偽の緊急性**
(カウントダウン煽り・偽ロード) は、deep_profile に嘘を書くのと同じ側の行為
であり永久禁止 (憲法 6 の UI 側対偶: UI は要約してよいが、捏造してはならない)。

---

# §5【実装マイルストーンとゲート】

```
F0: App.css トークン移行 (視覚差分ゼロ)          ゲート: スクショ比較 + ui_smoke
F1: RECORD フリクション監査 (autofocus / Enter 追加 / Ctrl+1-3)
F2: IMPORT 等幅ダッシュボード (グリフバッジ + status 逐次表示)
F3: CONSULT 可読測度 + スクロール追従規律
F4: INTERVIEW SessionHUD (PreSessionBriefing は E4 完成後に接続)
F5: SETTINGS iOS 化 (CSS Toggle + Advanced <details>)
F6: PROBE タブ (D2 + E4 完成が前提条件 — それまで着手禁止)
各段: npx tsc --noEmit + ui_smoke ALL PASS。コミットは指揮官の指示時のみ。
```

---
*装飾は 1 ピクセルも要らない (AI_SKILLS §3.4)。ハッカーが信頼するのは、
等幅で揃った本物の数値と、押した瞬間に応答する機械だけだ。*
