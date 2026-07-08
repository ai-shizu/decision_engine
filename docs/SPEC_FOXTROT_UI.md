# SPEC_FOXTROT_UI.md — Target Foxtrot: フロントエンド UI/UX 完全設計仕様書
# 発行: 2026-07-07 / 起草: fable5 (リード・アーキテクト) / 実装担当: Sonnet5
# Rev.1
# Rev.2 (2026-07-07): E4/Foxtrot 統合裁定 (SPEC_ECHO §5.10.5) を受け F-11/F-12 を
#   追加。実装順序は E4 完遂後に F0 から着手 (並行禁止)。
# Rev.3 (2026-07-08): F0 完遂を受けた裁定。F-14 (サイバーパンク演出の不変条件)、
#   §2.7 F7 (Desktop Native Chrome)、§3.6 (キーボード予約表)、§6 (W-22〜W-27
#   React 実装罠) を追加。F6 (PROBE) の様式定義は D2/E4 完成後の着手時に確定
#   させる (ゲートは不変)。
# Rev.4 (2026-07-08): F7 完遂を受けた F1 着工前裁定。§2.1 に F1 専用の 3 裁定
#   (recordDraft によるタブローカル state 隔離・isCommitEnter による IME
#   ガード共通化・Alt/Ctrl キーボード配線レイヤ) を追加。
# Rev.5 (2026-07-08): F1 完遂を受けた F2 着工前裁定。§1.5 に term- CSS レイヤ
#   (F-14 の合法実装語彙。IMPORT で建設・INTERVIEW/PROBE が流用) を新設。
#   §2.2 に非同期処理と importLog の裁定を追加。§6 に W-28〜W-32 (イベント
#   バス混線・unmount後setState・同一ファイル再選択・D&D 実装禁止・
#   ファイル名プライバシー) を追加。バックエンド新設 (status callback 配線・
#   import.stats) を初めて許可する — ロジック変更は禁止、配線のみ。
# Rev.6 (2026-07-08): F2 完遂を受けた F2-EXT (汎用インポート) 裁定。§2.2.2 に
#   「判別は提案、書き込みは明示」の権限分離設計 (import.classify 純関数 +
#   import_document の dest ホワイトリスト・冪等性・sanitize) を追加。
#   §6 に W-33 (フロント側の cp932 フォールバック) を追加。指揮官要求
#   (「その他」入力口 + 自動判別) を決定論の枠内で満たす。

> **読者への前提命令**: 本書を読む前に `docs/AI_SKILLS.md` §0 のルーティング表
> に従い §1 + UI タスク該当節を読め (2026-07-08 改訂 — 全文読了の強制は撤回済み)。
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

## 1.5 term- CSS レイヤ (Rev.5) — F-14 の合法実装語彙

サイバーパンク演出 (F-14) を 15 トークン + 等幅書体 + 擬似要素のみで実現する
共有基盤。**F2 (IMPORT) で建設し、INTERVIEW の SessionHUD・F6 (PROBE) が
そのまま流用する** — 3 箇所で同じ語彙を使うことで様式の一貫性を保証する。

```css
/* ---- term- レイヤ: 端末美学の共有基盤 ---- */
.term-panel {                          /* HUD フレームの土台 */
  position: relative;
  background: var(--bg-deep);
  border: 1px solid var(--border);
  border-radius: var(--radius-s);
  padding: var(--s4);
}
.term-panel::before,                   /* L字コーナーブラケット */
.term-panel::after {
  content: "";
  position: absolute;
  width: 8px; height: 8px;
}
.term-panel::before { top: -1px; left: -1px;
  border-top: 2px solid var(--accent); border-left: 2px solid var(--accent); }
.term-panel::after  { bottom: -1px; right: -1px;
  border-bottom: 2px solid var(--accent); border-right: 2px solid var(--accent); }

.term-header {                         /* ▍IMPORT // DATA_SOURCES 型見出し */
  font-family: var(--font-mono);
  font-size: 0.75rem;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-muted);
}
.term-header::before { content: "\258D"; color: var(--accent); margin-right: var(--s1); }

.term-row {                            /* 等幅1行レイアウト (SourceRow/ログ行) */
  display: flex; align-items: baseline; gap: var(--s2);
  font-family: var(--font-mono); font-size: 0.85rem;
}
.term-value { font-family: var(--font-mono); text-align: right; }
.term-glyph-ok    { color: var(--ok); }        /* ● Indexed */
.term-glyph-muted { color: var(--text-muted); } /* ○ Missing */
.term-glyph-err   { color: var(--err-soft); }   /* ▲ Error */
.term-log-line::before { content: "> "; color: var(--accent); } /* プロンプト行 */
```

**適用ルール (法制化)**: ① 適用先は「統制感」を担うタブのみ (IMPORT /
INTERVIEW HUD / PROBE)。RECORD・CONSULT・SETTINGS への適用は禁止 (§2 の
既定形を裏切らない)。② `.term-panel` のブラケットは1階層のみ・ネスト禁止。
③ box-shadow / gradient / filter / 新規 hex は引き続き禁止 — 「発光」は
`--accent` と `--bg-deep` の彩度対比のみで表現する。④ 全グリフはテキスト
(●○▲▍`>`)。絵文字は禁止 (F-9)。

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

### 2.1.1 F1 着工前裁定 (Rev.4) — State隔離・IMEガード・キーボード配線

**裁定1 (state 隔離 — F-11 の具現化)**: RECORD タブが非アクティブ (アンマウ
ント) の間、書きかけの日記・追加済み予定・家計簿行を**完全揮発させるのも
App.tsx へ昇格させるのも却下する**。前者はユーザーの記録を破壊する行為で
あり (RECORD の存在意義と矛盾)、後者は §3.4-5 (状態は最小に) の退行で
F-11 が消したい「非アクティブタブの生きた state」を親に移すだけで漏洩面積
は減らない。

採用案: `RecordTab.tsx` **ファイル内・コンポーネント外**のモジュールレベル
シングルトン `recordDraft` のみを唯一の例外として認可する
(Redux/Context/グローバルストアではない — ただのファイルローカル変数)。

```tsx
type DraftSnapshot = { events: RecordEvent[]; transactions: Transaction[]; diary: string };
let recordDraft: { date: string; subTab: RecordSubTab;
                   baseline: DraftSnapshot; work: DraftSnapshot } | null = null;
```

規約 (1 つでも欠けたらレビュー落ち):
1. アンマウント時 (`useEffect` cleanup) に `{date, subTab, baseline(=直近に
   ディスクからロードした内容), work(=現在の state)}` を保存する。
2. マウント/`loadDay(d)` 時: ディスク取得後、`recordDraft.date === d` **かつ**
   ディスク内容が `baseline` と深い等価 (`JSON.stringify` 比較で可 — 同一
   シリアライザ由来なので決定論) の場合**のみ** `work` を復元する。
   不一致ならディスクが正・キャッシュは破棄する。
3. 保存成功時に `recordDraft = null` にする (保存された瞬間、draft は
   ディスクの記録へ昇格済みのため)。
4. キャッシュに入れてよいのは上記 4 フィールドのみ。QuickAdd の入力途中
   文字列 (`eventTitle` 等の断片) は**含めない**。oracle/twin 系の値は
   RECORD が Echo データに触れない設計のため構造的に混入し得ない — これが
   F-11 (分析出力の漏洩面積) と draft 保全を両立させる根拠である。

**裁定2 (IME ガード — W-25 の共通化)**: `lib/keyUtils.ts` に `isCommitEnter()`
を新設し、**全ての単一行入力がこの1関数を通る**ことを規律とする (各所
コピペの微妙な差異がバグの温床になるため)。

```tsx
export function isCommitEnter(e: React.KeyboardEvent): boolean {
  return e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229;
}
```

適用規約: ①`onKeyDown` のみで判定 (`onKeyUp` 禁止)。②適用対象は QuickAdd
系の単一行 `<input>` のみ — **diary の `<textarea>` には絶対に付けない**
(Enter=改行が正)。③ commit 時は `e.preventDefault()` してから追加処理。
④ `busy` 中は発火させない。

**裁定3 (キーボード配線のレイヤ)**: Alt 系とCtrl 系は修飾キーで直交して
おり、`stopPropagation` は不要 (既存 Ctrl+S の `stopPropagation` は維持
してよいが、新設分には付けるな — 将来のリスナーを黙って殺す時限爆弾になる)。

| キー | 捕捉レイヤ | 根拠 |
|---|---|---|
| `Alt+1..5` (メインタブ) | `App.tsx` の `window` keydown (`ready` 後のみ登録・cleanup 必須) | タブ切替はシェルの責務。どのタブがマウントされていても効く必要がある |
| `Ctrl+1..3` (サブタブ) | `RecordTab` の既存 Ctrl+S リスナーへ相乗り (`window` + `capture:true`) | リスナーを増やさない。RECORD アンマウントで自然消滅 = F-11 がスコープ制御を無償で提供 |

判定は修飾キーの厳密一致 (`e.altKey && !e.ctrlKey` / `e.ctrlKey && !e.altKey`)。
`preventDefault()` は必須 (Windows の Alt メニューアクセラレータ抑止)。
入力欄フォーカス中でも発火させる — 筋肉記憶はフォーカス位置に依存しない。

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

### 2.2.1 F2 着工前裁定 (Rev.5) — 非同期処理・importLog・バックエンド配線

**実測に基づく前提**: `invoke_sync` (engine.rs) のイベント転送はコマンド
非依存の汎用機構であり、`engine_stdio` には既に `emit` コールバックが
存在する。つまり F-6 (status 逐次表示) はロジック変更なしの**配線のみ**
で合法的に実現できる。

**裁定 (1) 中断は永久禁止**: import は書き込み操作。タブアンマウント時に
**キャンセルしない・させない** (途中中断はデータ破損の温床)。処理はバック
エンドで自然に継続させる (stdio レーンは直列のため次コマンドは自然に
待ち行列化する)。

**裁定 (2) importLog によるログの不喪失**: `RecordTab.tsx` の `recordDraft`
と同族の、ファイルローカル・シングルトン `importLog: string[]` (上限 50 行、
アプリ終了で揮発) を認可する。status 行は「state に直接」ではなく
「`importLog` へ push → state へ反映」の順で書き、unmount 中に届いた完了
通知もログに残す。再マウント時は `importLog` から全行復元する — タブを
往復しても取込の経過と結果が消えない (一覧性 = 統制感)。コンポーネント内は
`mountedRef` ガードで unmount 後の setState を封じる (W-29)。

**裁定 (3) バックエンド最小配線 (F-6 の実装、初めて許可)**: `engine_stdio`
の `import.line` / `calendar.sync` ハンドラに consult と同型の `emit` を
渡し、`facade.import_line_text`/`import_line_batch` へ optional
`status=None` コールバックを追加する。発行するステージは「N件受信→追記
完了→profiler再分析中→テレメトリ/テンソル更新→完了」の**実際の処理段の
み** (F-14: 偽の進捗禁止。% 表示は計測できないので出さない — 段階名だけが
本物)。**ロジック変更は一切禁止、配線のみ**。フロントは `pkb-engine-event`
を購読する (W-22 の非同期 cleanup 厳守、W-28 のイベント混線ガード必須)。

**裁定 (4) SourceTable のデータ源**: 新 API `import.stats` を認可する
(`facade.data_source_stats()` → `engine_stdio` dispatch → `engine.ts`
ラッパーの正規 3 層経路 — §3.1 の配線順序を厳守)。中身は 6 ソースの
`{exists, count, mtime}` の軽量 stat (diary 行スキャン・json len・ファイル
mtime — stdlib のみ、LLM/埋め込み不使用、遅延初期化を起こさないこと)。
マウント時 + import 完了時に再取得する (§3.4-5)。

### 2.2.2 F2-EXT 着工前裁定 (Rev.6) — 汎用インポートの決定論的拡張

指揮官要求 (「その他」の入力欄 + 自動でファイル種別を判別して中身を読み込む
機構) は正当だが、「自動判別して読み込む」を素朴に実装すると、システムの
中で唯一「入力の型が保証されない箇所」が生まれる — IMP-2 の教訓 (取込 API
は「同じものを2回入れたら」「変なものを入れたら」を最初に問え) をここで
再適用する。

**裁定1: 自動判別は認可する — ただし「判別は提案、書き込みは明示」**

自動判別の認否を分ける本質はアルゴリズムではなく権限だ。判別器に書き込み
先の決定権を与えた瞬間、誤判別=聖域汚染になる。よって:

> 分類器 (`import.classify`) は読み取り専用の純関数とし、書き込み
> (`import.document`) は UI がユーザー確認済みの `dest` を明示的に渡した
> 時のみ実行する。バックエンドは自分の推測に基づいて書き込まない。

決定論的分類アルゴリズム (stdlib のみ・順序固定・先勝ち):

```
0. 拒絶ゲート (順不同・1つでも該当なら reject):
   - 拡張子ホワイトリスト外 (.txt / .md / .csv / .json / .ics のみ許可)
   - 先頭 8KB に NUL バイト (バイナリ混入)
   - サイズ > 10MB (データ爆弾トリップワイヤ — IMP の系譜。診断メッセージ付きで騒がしく拒否)
1. 先頭テキストに "[LINE]" ヘッダ        → line     (既存 import.line へ回送)
2. "BEGIN:VCALENDAR"                     → ics      (既存 calendar.sync へ回送)
3. ES シグナル (明示フィールド "志望職種:"/"志望動機:"/"自己PR"/"ガクチカ" 等の
   es_manager と同じ語彙)                 → es
4. 上記いずれでもないテキスト             → knowledge (既定の受け皿)
```

戻り値は `{type, reasons[], size, filename}` — reasons (マッチした根拠
シグナル) を必ず返す。UI が「なぜこの判定か」を表示できることが、
ブラックボックス化の防止線だ。

書き込み側の防衛 (多層防御):

- `import_document(content, filename, dest)` の `dest` は
  `"es" | "knowledge"` の2値ホワイトリスト。diary/finance/calendar への
  汎用書き込みは永久に不許可 — diary は RECORD で著述するもの、
  finance/calendar は専用フォーマット管理下にある。LINE/ICS と判定された
  ファイルは専用パイプラインへ回送する (汎用口からは書かない)。
- バックエンドは UI の確認を信用せず再検証する (拒絶ゲートを
  import_document 内でも再実行)。
- 冪等性 (T-20の直接適用): 書き込み前に内容の blake2b ハッシュを計算し、
  dest ディレクトリ内に同一ハッシュのファイルが既にあれば skip
  (「同一内容が既に存在」と報告)。ファイル名は sanitize (パス成分除去・
  危険文字除去) し、名前衝突時はハッシュ接尾辞を付与 — 既存ファイルの
  上書きは構造的に不可能にする。
- knowledge へ書いた場合のみ `sync_knowledge_index(force=True)` を続けて
  実行 (既存 knowledge_fetcher と同じ経路 — 専用機構を新設しない)。
- W-32続き: ファイル名・本文を stderr へ出さない。

**裁定2: 正規パイプライン仕様**

```
facade.classify_document(content: str, filename: str) -> dict     # 純関数・書き込みゼロ
facade.import_document(content, filename, dest, *, status=None) -> dict
stdio:  "import.classify" / "import.document"（status は emit 配線 — F2 と同型）
engine.ts: classifyDocument() / importDocument()（正規3層経路）
```

必須テスト (`test_import_stats.py` へ追加 or 新設): (a) 分類優先順位 —
`[LINE]`ヘッダを含む ES っぽい文書は line と判定される (先勝ちの証明)、
(b) NUL バイト・サイズ超過・拡張子外の拒絶、(c) 同一内容2回取込→2回目
skip (冪等)、(d) 名前衝突→上書きされず別名保存、(e) dest ホワイトリスト外
("diary"等)→例外、(f) knowledge 取込後に index 同期が呼ばれる。

**裁定3: UI統合 (term-調和・W-31遵守)**

`ImportTab.tsx` の LINE/ICS ブロックの後に「その他 (自動判別)」ブロックを
追加:

```
その他 (自動判別)
  [ファイルピッカー: accept=".txt,.md,.csv,.json,.ics"]   ← W-31: D&D禁止のまま
        │ 選択 → 各ファイルを classify (読み取りのみ)
        ▼
  ┌ term-panel: CLASSIFY_RESULT ─────────────────┐
  │ ● report.md      → KNOWLEDGE   [dest select] │  ← 判定結果+根拠を1行ずつ
  │ ● es_darft.txt   → ES          [dest select] │     dest は上書き可能
  │ ▲ photo.png      → 拒絶 (拡張子外)            │     (es/knowledge/スキップ)
  │ ● talk.txt       → LINE形式 — LINE取込へ回送  │
  │                            [取込を確定]       │  ← 確定1クリックで一括実行
  └──────────────────────────────────────────────┘
        │ 確定 → import.document / 回送、status は IMPORT_LOG へ逐次追記
```

- 「選択→判定表示→確定」の2段構造そのものが裁定1の権限分離のUI表現だ。
  判定根拠 (reasons) を `term-value` で右側に表示する — 透明性が統制感を
  生む (F-14: 本物のデータ密度)。
- 確定前のpendingリストは揮発でよい (recordDraft対象外 — 数秒で再選択
  できるファイル選択は「記録」ではない。過剰保全はしない)。

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

## 2.7 DESKTOP CHROME (F7) — OS ネイティブウィンドウ枠の排除

**「ブラウザで動く画面」ではなく「インストール版デスクトップアプリ」の
外郭を完遂する。** Tauri v2 標準機構のみで達成する (新規依存 0)。

```
TitleBar (新設。App.tsx 最上位・shell の最初の子要素)
├── DragRegion                    ← data-tauri-drag-region 属性の div。
│                                    左: "PKB" (--font-mono・小さく)
└── WindowControls                ← ドラッグ領域の【兄弟要素】(W-27)
    ├── MinimizeBtn                ← "─" (mono・絵文字禁止)
    ├── MaximizeBtn                ← "□"
    └── CloseBtn                   ← "✕"・hover 時のみ background: var(--err)
```

- `apps/desktop/src-tauri/tauri.conf.json` の `app.windows[].decorations` を
  `false` にする。OS 既定のタイトルバー・ボーダーを消す。
- ドラッグ・ダブルクリック最大化・Windows スナップは `data-tauri-drag-region`
  属性が**ネイティブに処理する** — mousedown リスナー等の自前実装をするな
  (Tauri が壊れていない機構を再発明する行為であり、OS ごとの挙動差異という
  罠を自ら踏みに行くことになる)。
- ウィンドウ操作は `@tauri-apps/api/window` の `getCurrentWindow()` から
  `minimize()` / `toggleMaximize()` / `close()` を呼ぶ。
- **ブラウザ互換性 (必須)**: `"__TAURI_INTERNALS__" in window` でフィーチャー
  検出し、ブラウザ (vite 単体プレビュー等) ではウィンドウボタン群を
  非表示にする。DragRegion 自体は残してよい (drag region 属性はブラウザで
  無害)。この検出を省くと `window.__TAURI__` 未定義で例外を投げ、
  ブラウザ側の検証パイプライン (F0 で使用した vite プレビュー) が死ぬ。
- ウィンドウ位置・サイズの永続化は本マイルストーンのスコープ外
  (プラグイン追加または Rust 側実装が要る別途裁定事項)。

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
- **F-14 (Rev.3)**: サイバーパンク演出 (F6 PROBE 等) は**「本物のデータの
  提示密度」でのみ**構成する。許される手段は等幅書体・大文字ラベル・
  `letter-spacing`・擬似要素によるコーナーブラケット (`::before`/`::after`
  の border) ・テキストプロンプト記号 (`>` 等)・既存 `chat-blink` keyframe
  の点滅キャレット・`stroke-dasharray` の未確定線のみ — 全て F-1〜F-3 の
  範囲内 (新規 box-shadow/gradient/filter/パーティクルは禁止のまま)。
  **偽のランダム点滅・演出目的の文字化け・データと無関係なダミー数値の
  流し込みは永久禁止** — deep_profile に嘘を書くのと同罪 (憲法 6 の系)。
  「発光」はネオンではなく `--accent`/`--ok` と `--bg-deep` の彩度対比で表現する。

## 3.5 Definition of Done (フロント変更時)

```powershell
npx tsc --noEmit          # apps/desktop
python tests\ui_smoke.py  # ALL PASS
cargo check               # src-tauri (Rust に触れた場合のみ)
```
+ F0 (トークン移行) のみ追加ゲート: 前後スクリーンショットの視覚的差分ゼロ。

## 3.6 キーボード予約表 (Rev.3 — 衝突防止の法制化)

**新規ショートカットを追加する前に必ずこの表を確認せよ。競合するキーを
無断で再割当てすることを禁ずる。**

| キー | スコープ | 用途 | 出典 |
|---|---|---|---|
| `Ctrl+S` | 全タブ共通 | 保存 | 既存規約 (AI_SKILLS §3.4-4) |
| `Ctrl+Enter` | 全タブ共通 | 送信 (consult 等) | 既存規約 |
| `Ctrl+1` / `Ctrl+2` / `Ctrl+3` | **RECORD タブ内のみ** | サブタブ切替
  (events\|money\|diary) | §2.1 (F-4) |
| `Alt+1`〜`Alt+5` | メインタブ切替 (グローバル) |
  RECORD/IMPORT/CONSULT/INTERVIEW/SETTINGS (PROBE 追加時は `Alt+6`) | Rev.3 新設 |
| `Enter` | RECORD QuickAddRow | 行追加 | §2.1 (**W-25 の IME ガード必須**) |

メインタブに `Ctrl+1〜5` を使わないのは、RECORD のサブタブ予約と衝突する
ため (グローバルとローカルで同じキーを持たせるとフォーカス依存の誤爆が
起きる)。メインタブ切替は `Alt` 系に系統的に分離する。

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
F7: Desktop Chrome (§2.7。カスタムタイトルバー・decorations:false)
    ゲート: tsc + cargo check + ui_smoke + tauri:dev 実起動でボタン動作確認
F1: RECORD フリクション監査 (autofocus / Enter 追加 / Ctrl+1-3)
F2: IMPORT 等幅ダッシュボード (term-レイヤ建設 + グリフバッジ + status 逐次表示)
F2-EXT: 汎用インポート (classify/import_document。dest 明示・冪等性・拒絶ゲート)
F3: CONSULT 可読測度 + スクロール追従規律
F4: INTERVIEW SessionHUD (PreSessionBriefing は E4 完成後に接続)
F5: SETTINGS iOS 化 (CSS Toggle + Advanced <details>)
F6: PROBE タブ (D2 + E4 完成が前提条件 — それまで着手禁止。様式は F-14 準拠)
各段: npx tsc --noEmit + ui_smoke ALL PASS。コミットは指揮官の指示時のみ。
```

---

# §6【実装者への警告 (W-22〜W-33)】Rev.3/Rev.5/Rev.6 — React 再構築の死角

Foxtrot 本格実装 (F7・F1〜F6) で踏み抜きやすい罠。**新規タブ実装のたびに
本リストと照合せよ。**

- **W-22 (Tauri `listen()` の非同期クリーンアップ漏れ)**: `listen()` は
  `Promise<UnlistenFn>` を返す。`useEffect` 内で解除関数を確実に `return`
  しないと、タブ往復のたびに購読が積み重なり、consult の chunk が
  二重三重に追記される (症状: 回答テキストの重複)。既存 ConsultTab.tsx の
  購読パターンを踏襲し、新規購読も同型で書け。
- **W-23 (React 19 StrictMode の二重実行)**: 開発モードは effect を 2 回
  発火する。購読・タイマーの初期化が冪等でないと開発時だけ壊れ、「本番では
  直る」という最悪の診断難易度を生む。
- **W-24 (F-11 の退行禁止)**: タブ切替を速くする目的で `useMemo` /
  `display:none` / keep-alive 化する誘惑を禁ずる。`{tab === "x" && <X/>}`
  の条件レンダリングは oracle/twin 系 state の構造的消去装置であることが
  F-11 で確定済み — 高速化の理由でこれを外すな。
- **W-25 (日本語 IME 合成中の Enter 誤爆)**: RECORD の「Enter で QuickAdd
  追加」は、日本語 IME の変換確定 Enter を誤って発火させ得る。
  `KeyboardEvent.isComposing` (または `keyCode === 229`) によるガード
  **無しの Enter ハンドラはレビュー落ち**。本アプリの入力は日本語が主であり、
  これを怠るとフリクションレスどころか入力破壊になる。
- **W-26 (`setInterval` の stale closure)**: LatencyTimer 等の秒表示が
  state を直接閉じ込めると表示が凍る。開始時刻は `useRef` に置き、
  再レンダリングで変化させるのは表示用 state のみにせよ。
- **W-27 (ドラッグ領域によるクリック横取り)**: `data-tauri-drag-region`
  の**子要素に置いたボタンはクリックがドラッグ判定に食われる**。
  WindowControls (§2.7) はドラッグ領域の兄弟要素として配置せよ。
- **W-28 (Rev.5: イベントバスの混線)**: `pkb-engine-event` は**全コマンド
  共有のグローバルバス**である。IMPORT のリスナーは「自分の import が
  in-flight の間だけ購読し、`event === "status"` のみ処理」せよ。無条件
  購読すると consult の status/chunk イベントが取込ログに混入する。
- **W-29 (Rev.5: unmount 後の setState)**: 完了ハンドラ・catch・finally の
  全てで `mountedRef` を確認せよ。React は警告を出さずに黙って捨てる —
  「動いているように見えて結果表示が消えた」という最悪の症状になる。
  §2.2.1 裁定 (2) の `importLog` が結果の保険になる。
- **W-30 (Rev.5: 同一ファイル再選択)**: `<input type="file">` は同じ
  ファイルの再選択で `onChange` が発火しない。既存の `resetInput` パターン
  (値を空文字へリセット) を維持せよ — 削るな。
- **W-31 (Rev.5: ドラッグ&ドロップの実装禁止)**: Tauri の WebView は
  HTML5 ファイルドロップを横取りし、Tauri 独自イベント (File オブジェクト
  ではなく**パス文字列**が届く) に変換する。パス読取は fs パーミッション
  拡張を要し、オフライン監査面積が広がる。**F2 で D&D を実装するな** —
  ファイルピッカーのみ。「便利だから」の追加はレビュー落ち。
- **W-32 (Rev.5: ファイル名のプライバシー)**: LINE エクスポートのファイル
  名は実名を含みうる (「[LINE] ○○とのトーク履歴.txt」)。UI の一時ログ表示
  は合法 (ユーザー自身が選んだファイル) だが、**stderr (engine.log) や
  いかなる永続化にもファイル名を書くな** (AI_SKILLS §1-2)。
- **W-33 (Rev.6: フロント側の cp932 フォールバック)**: `File.text()` は
  常に UTF-8 でデコードする。メモ帳の ANSI 保存 (cp932) の .txt を読むと
  本文が文字化けしたまま聖域に書き込まれる — バックエンドの
  `_read_text_lenient` と同じ配慮がフロントに必要だ。`ArrayBuffer` で読み、
  `TextDecoder("utf-8", {fatal:true})` を試して失敗したら
  `TextDecoder("shift_jis")` へフォールバックせよ (ブラウザ標準 API・
  決定論的)。これを怠ると「取り込めたのに中身がゴミ」という静かな破損に
  なる。

---
*装飾は 1 ピクセルも要らない (AI_SKILLS §3.4)。ハッカーが信頼するのは、
等幅で揃った本物の数値と、押した瞬間に応答する機械だけだ。*
