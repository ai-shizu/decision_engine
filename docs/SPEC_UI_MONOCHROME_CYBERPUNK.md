# SPEC — UI Overhaul: Monochrome Cyberpunk (Composer-Executable Blueprint)

> **読者:** 自律型コーディング AI（Cursor Composer 等）。人間向け解説ではない。**一字一句従え。裁量なし。**
> **起草:** Fable（主任アーキテクト）。デザイン裁定は本書で完結している。Composer は美学を再解釈するな。
> **親仕様:** `docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md` / `docs/SPEC_E0B_STEP8_UX_CONSENT.md`（承認済み・ロック）。
> **前提:** E0b STEP 0–8 ロック済み。本 SPEC が触れるのは**表層（CSS・DOM 構造・見た目）のみ**。
> ロジック（純関数・純 reducer・状態管理・二要素 Egress ゲート）は **1 バイトも変更しない**。
> **ACK 規律:** STEP UI.D 完了時に HARD STOP。指揮官 ACK まで先へ進むな。

---

## 0. 起草時に検証済みのリポジトリ事実（Composer はこれを前提にせよ・推測禁止）

- **Tailwind は存在しない。** スタイルは `apps/desktop/src/App.css`（1,733 行・単一ファイル）+
  `apps/desktop/index.html` のブート用インライン style のみ。**Tailwind / CSS-in-JS / UI ライブラリ /
  新規 npm 依存を一切導入するな。** 既存様式＝`:root` の CSS 変数トークン + プレーンなクラス。
- **現行トークン**（`App.css:1-34`）: `--bg:#0f1419` `--bg-raised:#121820` `--bg-deep:#0b1016`
  `--bg-hover:#1a2533` `--bg-selected:#1e2a38` `--bg-info:#0d1b2a` `--border:#2a3441` `--text:#e8eaed`
  `--text-muted:#9aa4b2` `--accent:#3498db` `--ok:#7dcea0` `--ok-strong:#2e7d4f` `--err:#e74c3c`
  `--err-soft:#f1948a` `--err-bg:#2a1215` / `--font-ui:"Segoe UI",…` `--font-mono:"Cascadia Mono",…` /
  余白 `--s1..--s6` / `--radius:8px` `--radius-s:4px` / `--t-fast:120ms`。
- **フォントはローカルのみ（既存規律・App.css:19 コメント）。** 本アプリはゼロ Egress 要塞である。
  **`@import` / `url(http…)` / Web フォント / CDN / 新規 `@font-face` は絶対禁止**（UI からの資源取得も egress とみなす）。
- **スタック:** React 19 + Vite 7 + Tauri 2。**React render テスト基盤は無い。** boundary テスト
  （`apps/desktop/tests-runtime/*.test.ts`・`npm.cmd run test:boundary`）は**純関数のみ**を検証する。
  よって CSS/DOM の変更はテストを壊さない——**`src/lib/` に触れない限り**。
- **STEP 8 資産の所在（全て凍結・§3.1）:**
  `src/lib/researchUiReducer.ts`（純 reducer + `deriveProvenance` + `provenanceChipText`）、
  `src/lib/policyStore.ts` / `parseKnowledgePolicy.ts` / `engine.ts`。
  provenance chip の文字列 `"🔗 Wikipediaより参照"` は **`provenanceChipText`（lib・凍結）が生成**する。
  UI 側で文字列を組み替えるな。装飾は CSS でのみ行え。
- **既存 UX/a11y 資産（維持必須）:** `role="tablist/tab/tabpanel"`・`aria-selected`・`aria-busy`・
  `role="status"/"alert"`・`focus-visible` アウトライン・Alt+[1-7] タブ切替・矢印キーのタブ移動・
  `Toggle.tsx` の native checkbox + label 構造・ConsultTab の非ブロッキング規律（モーダル/alert/confirm 皆無）。
- **リポジトリ規律:** `.claude/` を stage しない。push 禁止。`git add -A`/`.` 禁止。

---

# セクション 1: Fable の美学と UI 規則 (Fable's Aesthetics & UI Rules)

## 1.1 デザイン宣言 — 「沈黙する端末 (The Silent Terminal)」

この UI は、STEP 0–8 で築いた検証パイプラインの性格をそのまま表層に写す。すなわち:

1. **信号の希少性。** 色は情報である。色を捨てることで、残された唯一の信号——白——の一滴が意味を持つ。
   蛍光色は使わない。闇（黒）と信号（白）の間の灰色階調だけで階層を作る。
2. **動きは状態である。** アニメーションは装飾ではなく「いま生きているプロセス」の表示にのみ許される。
   research 中の呼吸、ストリーミング中のカーソル。それ以外の恒常ループは禁止。
3. **装飾は構造の可視化である。** 罫線・ブラケット・等幅グリッド。フレームは常に「境界＝検証境界」の
   メタファーとして 1px で引く。丸みは嘘である——**角丸は全廃する（radius 0）**。

## 1.2 カラーコード（トークン全交換・単一ソース）

`App.css` の `:root` を以下へ**値だけ**差し替える（変数名は既存のまま——参照側の改修を最小化する）:

```css
:root {
  /* ---- Monochrome ladder: 黒 → 白 の 8 階調のみ。R=G=B 以外の色値は全域で禁止 ---- */
  --bg:          #0a0a0a;   /* 基調: ほぼ黒。純黒 #000 は --bg-deep に予約 */
  --bg-raised:   #111111;   /* パネル・カード */
  --bg-deep:     #000000;   /* 入力欄・チャットログ = 最深部 */
  --bg-hover:    #1a1a1a;
  --bg-selected: #202020;
  --bg-info:     #101010;
  --border:      #2e2e2e;   /* 基本罫線。強調時のみ #f2f2f2 へ */
  --text:        #f2f2f2;   /* 信号色 = ほぼ白 (対比 ~17:1) */
  --text-muted:  #8c8c8c;   /* 減光 (対比 ~5.6:1 — AA 維持) */
  --accent:      #ffffff;   /* 操作・選択・focus = 純白。青 #3498db は死んだ */
  --ok:          #d9d9d9;   /* 稼働表示。緑は捨てる。文言が意味を運ぶ */
  --ok-strong:   #f2f2f2;
  --err:         #ffffff;   /* エラーは色相でなく「反転」で表す (§1.5) */
  --err-soft:    #d0d0d0;
  --err-bg:      #161616;
  /* ---- 形状: 角丸全廃 ---- */
  --radius: 0px;  --radius-s: 0px;
}
```

- **単色律:** 新規に書く色値はすべて灰色階調（`#xyxyxy` 形式で R=G=B、または `rgba(255,255,255,α)` /
  `rgba(0,0,0,α)`）。**青・緑・赤の残滓を App.css 全域から掃討せよ**（§3.3 grep ゲートで機械検証）。
- **白の予算:** 純白 `#ffffff` は「いま操作可能・いま起きている」ものにのみ使う。恒常テキストは `--text`。

## 1.3 タイポグラフィ — 全面等幅化

- `--font-ui` を**等幅スタックへ差し替える**（UI 全体が端末になる）:
  ```css
  --font-ui:   "Cascadia Mono", "BIZ UDGothic", "MS Gothic", Consolas, ui-monospace, monospace;
  --font-mono: "Cascadia Mono", "BIZ UDGothic", "MS Gothic", Consolas, ui-monospace, monospace;
  ```
  Cascadia Mono（Win11 同梱）が欧文、BIZ UDGothic（Win10/11 日本語環境同梱・可読性の高い等幅和文）が
  CJK を受ける。**すべてローカルフォント。ダウンロード資源ゼロ（§0）。**
- `:root` の `line-height` を `1.5` → `1.6`（等幅和文の可読性補償）。
- **ラベル律:** 英大文字ラベル（タブ・タイトルバー・role タグ）には `letter-spacing: 0.08em`。
  見出し（`h1/h2/h3`）は `border-bottom: 1px solid var(--border)` を敷き、`::before` で `"§ "` 等の
  プレフィックスグリフを付けてよい（CSS `content` のみ・DOM にテキストを足すな）。

## 1.4 装飾ボキャブラリ（許可される表現はこれだけ）

| 表現 | 規則 |
|---|---|
| 罫線フレーム | `1px solid var(--border)`。活性/focus 時のみ白系へ。二重線・太枠禁止 |
| 反転ビデオ (inverse video) | `background: var(--text); color: var(--bg)`。**「選択」「エラー」「同意 ON」の 3 用途専用** |
| 白グロー | `box-shadow` の白 α のみ（`rgba(255,255,255, ≤0.25)`）。色付きグロー禁止 |
| スキャンライン | `body::after` に固定オーバーレイ 1 枚のみ（§2 UI.A）。`opacity` 実効 ≤ 0.02・`pointer-events:none` |
| プレフィックスグリフ | CSS `content` による `">"` `"§"` `"["` `"]"` 等。**DOM テキストノードには足すな** |
| グリッチ | hover/活性遷移スコープのみ。`text-shadow`/`clip-path`、変位 ≤2px、≤200ms、**無限ループ絶対禁止** |

グラデーション・ドロップシャドウ（白グロー以外）・角丸・絵文字装飾の新規追加は禁止。

## 1.5 運動規則 (Motion Discipline)

1. **無限ループは「生きているプロセス」専用:** 許可は 3 つだけ——(a) `chat-blink`（ストリーミング
   カーソル・既存）、(b) research 中の呼吸グロー、(c) research スピナー。同時稼働は 1 画面 2 つまで。
2. **`transform` / `opacity` / `box-shadow` のみ**をアニメーションせよ。レイアウトシフト（width/height/
   margin のアニメ）禁止。入力・スクロール・送信を阻害する演出は STEP 8 の UX 至上命題違反。
3. **`prefers-reduced-motion: reduce` で全アニメーション停止**を App.css 末尾に一括で入れる:
   ```css
   @media (prefers-reduced-motion: reduce) {
     *, *::before, *::after { animation: none !important; transition: none !important; }
   }
   ```
4. エラー表示は動かない。**反転ビデオ（静止）**が最大音量である。

## 1.6 a11y 床 (絶対最低線)

- 本文コントラスト ≥ 7:1、muted ≥ 4.5:1（§1.2 の値は充足済み。新規灰色を足すときは維持せよ）。
- `focus-visible` アウトラインは**白 1px + offset**（青の置換。消すことは許さない）。
- `role` / `aria-*` / `id` / `htmlFor` / `hidden` / キーボードハンドラを**一切削除・改名しない**。

---

# セクション 2: コンポーネント改修計画 (STEP UI.A 〜 UI.D)

各 STEP: **実装 → 検証コマンド → 完了条件 → コミット**。render テスト基盤は無いため、機械検証は
boundary + tsc + vite build + grep ゲート、外観検証は各 STEP の目視チェックリストで行う。

**改変を許可するファイル（全 STEP 通算・これ以外の diff はゼロであること）:**
```text
apps/desktop/src/App.css          # 主戦場
apps/desktop/index.html           # ブート画面の配色・フォントのみ
apps/desktop/src/components/*.tsx # 表層のみ (className / 静的ラッパー要素)。原則ゼロ diff を目指せ
```
**`.tsx` 改変の追加制約:** import・hooks・ハンドラ・状態・`src/lib` 呼び出し・ユーザー可視文言
（日本語コピー）を変更するな。`role`/`aria-*`/`id` を維持せよ。`.tsx` に diff を作った場合は
HARD STOP 報告でファイルごとに理由を 1 行で述べよ。

---

### STEP UI.A: 基盤 — トークン全交換とベースレイアウト

1. **実装（App.css / index.html）:**
   - §1.2 のトークン差し替え + §1.3 のフォント/`line-height` + §1.5.3 の reduced-motion ブロック。
   - `index.html` のインライン style を新基調へ（`background:#0a0a0a; color:#f2f2f2;` + 等幅スタック）。
     文言・構造は不変。
   - **App.css 全域の色残滓を掃討:** ハードコードされた `#3498db` 系 rgba（例: `rgba(96,165,250,…)`）、
     緑・赤系を §1.2 の灰色階調 or 白 α へ置換。ハードコード `border-radius`（`8px`/`999px` 等）を
     `var(--radius)` または `0` へ。
   - **タイトルバー:** 現状維持ベース。`.titlebar-btn-close:hover` は反転ビデオ
     （`background:var(--text); color:var(--bg)`）へ——赤の追放。
   - **タブ:** `.tabs button` は `border-radius:0`・等幅・`letter-spacing:0.08em`。`::before`/`::after`
     の `content` で `"[ "` / `" ]"` を付す。`active` は**反転ビデオ**（白地黒字）。hover に §1.4 の
     グリッチ（`text-shadow` 1px オフセット・120ms transition）を許可。
   - **スキャンライン:** `body::after { content:""; position:fixed; inset:0; pointer-events:none;
     background: repeating-linear-gradient(0deg, rgba(255,255,255,0.012) 0 1px, transparent 1px 3px); }`
     （CSS のみ・DOM 追加なし・z-index はコンテンツ下にならないよう注意しつつ操作を阻害しない）。
   - `.tabs button:focus-visible` 等の focus リングを `var(--accent)`（=白）で維持。
2. **検証コマンド:**
   ```
   cd apps/desktop && npm.cmd run test:boundary
   npm.cmd exec tsc --noEmit
   npm.cmd run build
   ```
3. **目視チェックリスト:** 全 7 タブを巡回し、(a) 有彩色が視界に無い、(b) active タブが白地黒字、
   (c) フォーカスリングが見える、(d) Alt+1..7 が生きている、を確認。
4. **完了条件:** 全コマンド GREEN + `git diff --name-only` が許可ファイルのみ。**→ コミット。**

---

### STEP UI.B: 同意トグルと SETTINGS — 「契約の刻印」

同意トグルは二要素 Egress ゲートの第一因子である。ON は重い決定であり、UI 上も**反転＝最大音量**で刻む。

1. **実装（App.css のみ。`Toggle.tsx` / `SettingsTab.tsx` は原則ゼロ diff）:**
   - `.toggle-track`: 長方形（radius 0）・`border:1px solid var(--border)`・OFF は中空（背景 `--bg-deep`）。
   - `.toggle-knob`: 正方形ブロック。OFF は `--text-muted` の塗り。
   - `.toggle-input:checked + .toggle-track`: **反転ビデオ**——トラック白塗り・ノブ黒。
     ON 状態は一目で「白＝信号が通っている」と読めること。transition は `var(--t-fast)` の平行移動のみ。
   - `.settings-row`: 台帳（ledger）様式——行間罫線 `1px solid var(--border)` の等幅グリッド。
     `.settings-row-label` は muted・ラベル律適用。
   - 同意トグル行そのものへの恒常アニメーション・グロー付与は**禁止**（§1.5.1 の予算外。静的な刻印であること）。
2. **検証コマンド:** UI.A と同一 3 コマンド。
3. **目視チェックリスト:** (a) OFF=中空/ON=反転が瞬時に判別できる、(b) キーボード（Tab→Space）で
   切替可能、(c) `toggle-readonly` の減光が維持、(d) トグル操作で research 同意が実際に永続化
   （挙動は STEP 8 のまま——見た目しか変わっていないことの確認）。
4. **完了条件:** 全コマンド GREEN + diff が App.css（+許可時 .tsx 表層）のみ。**→ コミット。**

---

### STEP UI.C: チャット画面 — 転写ログとアンビエント・スキャン

チャットは「吹き出し」をやめ、**端末の転写ログ (transcript)** になる。research 中の表示は
STEP 8 の非ブロッキング規律のまま、青いグローを**白い呼吸**に置き換える。

1. **実装（App.css 中心。`ConsultTab.tsx` は className 追加が必要な場合のみ）:**
   - `.chat-log`: `background: var(--bg-deep)`（純黒）・radius 0。周囲 1px 罫線維持。
   - `.chat-bubble.user`: 背景 `--bg-selected`・**右端 2px 白罫**（`border-right: 2px solid var(--text)`）。
   - `.chat-bubble.assistant`: 背景を透明化し、**左端 2px 罫**（`border-left: 2px solid var(--border)`、
     streaming 中のみ白へ遷移可）。引用された端末出力として読ませる。
   - `.chat-role`: 等幅・大文字ラベル律。`::before` で `"> "` を付してよい。
   - `.chat-cursor`: 色はトークン経由で白になる（既存 `chat-blink` 維持——これが許可ループ (a)）。
   - **アンビエント research 表示（STEP 8.B 資産・ロジック不変）:**
     - `.consult-form.researching-ambient`: 青グロー → **白の呼吸**へ。
       `box-shadow: 0 0 0 1px rgba(255,255,255,0.35)` を基点に、2.4s `ease-in-out` `alternate` で
       `0 0 18px rgba(255,255,255,0.08)` まで拡散する `@keyframes`（許可ループ (b)）。
     - `.consult-research-spinner`: 円形スピナー → **正方形アウトラインの段階回転**。
       `border-radius:0`・白 α ボーダー・`animation: … 1.2s steps(8) infinite`（許可ループ (c)。
       steps() の機械的な回転が端末の質感を作る）。
     - `.consult-research-ambient`: 等幅・muted。`::before` の `content` で `">> "` を付してよい。
       **テキストノード「外部知識を補強中…」は不変。**
   - **非ブロッキング再確認:** research 中に textarea / 送信 / スクロールを disabled にする変更は
     一切加えていないこと（STEP 8 の配線に触れない＝自動的に満たされる）。
2. **検証コマンド:** UI.A と同一 3 コマンド。
3. **目視チェックリスト:** (a) research 中も入力・送信・スクロールが自由、(b) 呼吸グローと
   スピナーが白のみ、(c) ストリーミング → 確定置換の見た目遷移が自然、(d) `prefers-reduced-motion`
   有効時に全て静止（DevTools でエミュレート）。
4. **完了条件:** 全コマンド GREEN + `researchUiReducer.test.ts` を含む boundary 全 PASS。**→ コミット。**

---

### STEP UI.D: 取得痕跡 (Provenance) の刻印 + 全体磨き + 総回帰

1. **実装:**
   - `.provenance-chip`: 丸薬 (999px) をやめ、**点線罫の刻印**へ——`border: 1px dotted var(--border);
     border-radius: 0; font-family: var(--font-mono); font-size: 0.72rem; color: var(--text-muted);
     background: transparent; padding: 2px 8px; letter-spacing: 0.04em;`。hover で罫線・文字を白へ
     （`var(--t-fast)`）。外部知識が混入した応答であることを、静かだが消えない検収印として刻む。
     **chip の文字列は `provenanceChipText`（凍結）由来のまま。**
   - `.status-line` / `.error-text`: エラーは**反転ビデオ**——`background: var(--text);
     color: var(--bg); padding: 2px 6px; display: inline-block;`。動かさない（§1.5.4）。
   - 全画面最終掃討: 残った有彩色・角丸・旧フォント指定・不要になったスタイルを App.css から除去。
2. **検証コマンド（フル回帰ゲート・§3.2 と同一）:** 全て実行し実測を記録。
3. **完了条件:** §3.2 全 GREEN + §3.3 grep ゲート全通過。**→ コミット → HARD STOP（§3.5）。**

---

# セクション 3: ロジック保護と停止規律 (Logic Protection & HARD STOP)

## 3.1 凍結領域（1 バイトの diff も許さない）

```text
apps/desktop/src/lib/**            # researchUiReducer / policyStore / parseKnowledgePolicy / engine / 全 parser
apps/desktop/tests-runtime/**      # boundary テストの改変・削除・skip 化は重大違反
apps/desktop/src-tauri/**          # Rust 検証コア (STEP 1–7) / policy_store / commands / lib.rs
src/** tests/** scripts/** config/**   # Python コア・憲法ガード・golden
apps/desktop/package.json / package-lock.json / vite.config.ts / tsconfig*.json   # 依存追加禁止 (§0)
```
**機械検証（UI.D 報告に出力を添付）:**
```
git diff --stat HEAD~4                       # 許可 3 種 (App.css / index.html / components/*.tsx) のみ
git diff HEAD~4 -- apps/desktop/src/lib apps/desktop/tests-runtime apps/desktop/src-tauri src tests   # 出力ゼロ
```

## 3.2 フル回帰ゲート（UI.D で全実行・全 GREEN が HARD STOP の入場券）

```
cd apps/desktop && npm.cmd run test:boundary          # 純 reducer / deriveProvenance / policy 含む全 PASS
npm.cmd exec tsc --noEmit                              # 型 GREEN
npm.cmd run build                                      # vite build GREEN
cd ../.. && cargo test -p pkb-desktop --lib            # Rust 非回帰 (UI は触れていない証明)
cargo clippy -p pkb-desktop --all-targets -- -D warnings
python -m pytest tests/test_e0b_constitutional_guard.py -v   # reqwest:: 封じ込め維持
```

## 3.3 grep ゲート（美学とゼロ Egress の機械検証・全て該当 0 件であること）

```
# 1) ブロッキング UI の不在 (STEP 8 規律の再証明)
grep -rnE "window\.alert|window\.confirm|showModal" apps/desktop/src
# 2) 有彩色の残滓 (旧トークン値・青グロー rgba)
grep -nE "#(3498db|7dcea0|2e7d4f|e74c3c|f1948a|0d1b2a|2a3441|9aa4b2)|rgba\(96, ?165, ?250|rgba\(52, ?152, ?219" apps/desktop/src/App.css
# 3) 外部資源ゼロ (egress 要塞の表層版)
grep -nE "@import|@font-face|url\(https?:" apps/desktop/src/App.css apps/desktop/index.html
```

## 3.4 コミット規律

`git status --short` 目視 → 当該 STEP の対象のみ stage（`git add -A`/`.` 禁止・`.claude/` を stage しない・
push 禁止・revert/reset/`checkout --` 禁止）。

| STEP | stage 対象 | コミットメッセージ |
|---|---|---|
| UI.A | `apps/desktop/src/App.css`, `apps/desktop/index.html` | `feat(ui-mono): STEP UI.A — monochrome token swap, mono-first type, zero-radius base layout` |
| UI.B | `apps/desktop/src/App.css` | `feat(ui-mono): STEP UI.B — consent toggle as inverse-video seal, ledger settings rows` |
| UI.C | `apps/desktop/src/App.css` (+`src/components/ConsultTab.tsx` 表層のみの場合) | `feat(ui-mono): STEP UI.C — terminal transcript chat, white-breath ambient research scan` |
| UI.D | `apps/desktop/src/App.css` | `feat(ui-mono): STEP UI.D — dotted provenance seal, inverse-video errors, final sweep` |

各コミット末尾:
```
Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
```

## 3.5 HARD STOP（正常時・報告必須項目）

1. §3.2 全コマンドの実測結果（PASS 数を含む）。§3.3 grep 3 種が 0 件である証拠。
2. §3.1 の diff-empty 証明（`git diff … -- lib tests-runtime src-tauri` の出力ゼロ）。
3. **ロジック不変の宣言:** 純 reducer・`deriveProvenance`・`provenanceChipText`・二要素 Egress ゲート・
   consent 既定 OFF に**バイト単位の変更ゼロ**であること。
4. `.tsx` に diff がある場合、ファイルごとに「表層のみ」である理由 1 行。
5. 目視チェックリスト（UI.A/B/C）の結果と、`prefers-reduced-motion` 検証の結果。
6. `git log --oneline`（UI.A–UI.D）。

**ACK なしに先へ進むな。** 追加の演出・ダークモード切替・テーマ設定 UI 等のスコープ拡大を勝手に行うな。

## 3.6 HARD STOP（異常時・即停止して報告）

- **boundary テストが 1 件でも RED になった:** 表層改修でテストは壊れないはず（§0）。壊れた＝ロジックに
  触れた証拠。**テストを直すな。自分の diff を疑え。** 停止・報告。
- **Tailwind / ライブラリ / Web フォントを入れたくなった:** 禁止（§0）。App.css とローカルフォントで作る。
- **蛍光色・有彩色を 1 箇所だけ使いたくなった:** 禁止。白の予算（§1.2）で解決せよ。
- **モーダル/オーバーレイ/入力ブロックで「演出」したくなった:** STEP 8 UX 至上命題違反。アンビエントに直す。
- **`provenanceChipText` や UI 文言を変えたくなった:** 凍結。CSS の `content` グリフで装飾せよ。
- **アニメーションがカクつく・入力遅延が出た:** §1.5.2 違反の兆候。`transform`/`opacity` へ還元せよ。

## 3.7 完了の定義 (DoD)

- UI 全域が灰色階調 8 段 + 白のみで構成され、grep ゲート（§3.3）が 0 件。
- 等幅タイポグラフィ・radius 0・反転ビデオ（選択/エラー/同意 ON の 3 用途）・白の呼吸（research 中）が
  §1 の規則どおり実装されている。
- STEP 8 のロジック・テスト・二要素 Egress ゲート・非ブロッキング UX が**無傷**（§3.1/§3.2 で機械証明）。
- `prefers-reduced-motion` で全静止。a11y 床（§1.6）維持。
- コミット 4 件（§3.4）。HARD STOP・ACK 待ち。
