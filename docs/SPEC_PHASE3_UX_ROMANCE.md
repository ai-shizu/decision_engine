# SPEC: Phase 3 UX (Romance preview boundary)

## Scope

Phase 3-A is **completed** (UI-only; historical). Interview custom-theme / NARRATIVE_DRAFT
surfaces remain. PROFILE permanent mocks (MBTI + fixed 6D preview) were **retired in
Finding 9 (2026-07-12)** — they are not current deliverables.

Phase 3-B (**Romance / 交流パルス解析**) is the **current** confirmed specification and implementation target for `romance_analysis` mode.

## Phase 3-B boundary (constitutional — applies to 3-B implementation)

- Do **not** infer third-party emotions.
- Observable quantities only: speaker counts, turn switches, balance, reply coverage (no character-length or timestamp inference).
- Third parties appear as `contact_alias` only — no real names, no raw LINE body in derived UI.
- Empty body lines (`[self]` / `[contact_alias]` prefix only, whitespace-only, control-char-only) are **invalid** and excluded from counts.
- Insufficient data → `affinity_score=null`; never fabricate 0 or 100.
- `interaction_tendency` / `next_best_action` are derived from measured metrics only; validator requires exact key set match.
- UI clears previous romance panel and success message when a new analysis starts.
- **MBTI**: 測定契約ができるまで非表示。Do **not** derive MBTI from D1, Source Code, or Echo values.
- **6D tensor**: display only via Interview/GD measured `MISSION_RESULT` → `report.tensor_profile` → `TensorProfilePanel`. No PROFILE fixed preview.

## Phase 3-A deliverables (historical — Finding 9 retirement note)

### Interview tab (still current)

- Custom Theme textarea: `rows={6}`, `className="custom-theme-textarea"`, `resize: vertical` (CSS), `maxLength={240}` unchanged.
- NARRATIVE_DRAFT copy: gap analysis + ES draft explanation; mock interview mount blocks execution.

### Profile tab (Finding 9)

- ~~`MbtiGradientBars` fixed mock~~ — **removed**. Component deleted; CSS `.mbti-preview-*` removed.
- ~~PROFILE fixed `TENSOR_RADAR_PREVIEW`~~ — **removed**. Measured 6D lives on Interview/GD only.
- `TensorRadarChart` remains for **measured** display (legend `[?]` tooltips, keyboard focus + `aria-describedby`); `preview` prop deleted.

### Accessibility

- Tooltips: `role="tooltip"`, unique IDs, `tabIndex={0}` on triggers, hover + focus visibility.
- MBTI mock labeling is historical only (surface gone).

## CSS discipline

- Reuse existing CSS variables (`--accent`, `--ok`, `--text`, spacing, radius).
- No new hex colors, no literal `px` (use `rem` or variables), no `letter-spacing`, no purple glow/shadows/decorative cards/animations.

## Tests

- `tests/test_phase3_ux_contract.py` — textarea, CSS, narrative copy, mock-removal guards, 6D tooltips, forbidden tokens.
- `tests/test_profile_mock_removal_contract.py` — Finding 9 reintroduction prevention.

## Stop conditions

- Changes outside allowed files.
- package.json / backend / `data/` diffs.
- Breaking Phase 1/2 tensor radar geometry, redactor, or IPC contracts.
- Reintroducing PROFILE MBTI/6D mocks or inventing MBTI from D1/Source Code/Echo.

---

## Phase 3-B — Romance（交流パルス解析）確定仕様

### Mode

- **mode**: `romance_analysis`
- **UI表示名**: `Romance（交流パルス解析）`
- **回答文言**: `交流パルス解析が完了しました。`（UI送信表示は `会話履歴を解析しました`）

### 憲法（最優先）

- 第三者の感情・好意・恋愛感情を推定しない
- 実名を派生結果へ出さない（`contact_alias` のみ）
- 観測可能量だけを扱う
- データ不足時は `affinity_score: null`（捏造した 0 を返さない）
- UI は LLM 出力を `JSON.parse` しない（検証済み構造体のみ）
- 生 LINE 本文をログ・派生結果・解析結果パネルへ表示しない
- 完全オフラインを維持する

### 入力

- 形式: `[self] 本文` / `[contact_alias] 本文`（1行1メッセージ）
- 上限: 12,000 文字、最大 500 行
- 制御文字除去、改行正規化（`\r\n` → `\n`）
- 実名 prefix や解析不能行は評価対象外（スキップ）
- **空本文は無効**: prefix のみ、空白のみ、制御文字除去後に空となる本文は count しない
- 生本文の永続化・ログ出力禁止

### 有効データ条件

- **有効本文**のメッセージ 6 件以上
- 有効な `self` と `contact_alias` が各 2 件以上

不足時:

```json
{
  "schema": "romance_analysis.v1",
  "affinity_score": null,
  "interaction_tendency": "判定に必要な観測量が不足しています",
  "next_best_action": "会話履歴を追加して再分析する"
}
```

### `affinity_score`（交流往復指数）

API 互換のためフィールド名 `affinity_score` を維持するが、意味は「脈あり度」ではなく観測可能な会話往復量から決定論的に算出する**交流往復指数**とする。UI にも脈あり判定ではないことを明示する。

```
balance = 1 - abs(self_count - contact_count) / total_count
switch_rate = speaker_switches / (total_count - 1)
reply_coverage = self_to_contact_transitions / max(1, self_turns_with_successor)
score = ROUND_HALF_UP(100 * (0.40*balance + 0.40*switch_rate + 0.20*reply_coverage))
```

最終値を 0〜100 へ clamp。`integer | null` のみ。LLM が自己申告した score / tendency / action は決定論値と一致しなければ拒否。

### 傾向・次の一手（実測限定）

`interaction_tendency` は次の観測量のみから `deterministic_tendency(metrics)` で決定する。

- `self_count`, `contact_count`, `balance`, `switches`, `switch_rate`, `reply_coverage`

許可文言（未観測の文字数・時刻差に言及しない）:

- `発話数とターン切り替えは概ね均衡しています`
- `発話数に偏りがあります`
- `ターン切り替えが少ない状態です`
- `観測範囲では中程度の往復です`

`next_best_action` は `deterministic_action(metrics, score)` で決定。fallback は必ずこの結果を使用する。

### 構造化出力

- 有効データ時のみ `generate_structured()` を使用
- LLM へ渡すのは count / balance / switch_rate / reply_coverage / 決定済み score・tendency・action のみ
- 生本文・名前・quote を prompt へ渡さない
- `HiddenReasoningRedactor` 適用後に `json.loads`
- 最大 2 回 attempt、失敗時は決定論的 fallback
- `validate_result()` は required key の完全一致と score の完全一致（`null` 含む）を強制
- streaming で JSON chunk を送らない

### 最終構造体 (`romance_analysis.v1`)

```json
{
  "schema": "romance_analysis.v1",
  "affinity_score": 0,
  "interaction_tendency": "<許可済み短文>",
  "next_best_action": "<許可済み短文>"
}
```

`additionalProperties: false` を強制。`interaction_tendency` / `next_best_action` は固定 allowlist のみ。

### Consult 隔離

- `romance_analysis` 分岐は通常検索より前
- diary / knowledge 検索を行わない
- `last_consultation.md` へ保存しない
- raw 入力を state・ログへ残さない
- `_last_romance_analysis` は呼び出しごとに reset
- 他 mode では `romance_analysis` を必ず `None`

### UI

- Mode dropdown: `通常相談` / `Romance（交流パルス解析）`
- `RomanceAnalysisPanel`: 見出し `ROMANCE / INTERACTION_PULSE`、ラベル `交流往復指数`
- `null` は `N/A`、数値は 0〜100 clamp
- CSS 変数のみの percentage meter
- 固定免責: この指数は会話の往復量を示すもので、相手の好意や感情を判定するものではありません
- raw 入力を chat log / 解析パネルへ複製しない
- 再解析開始時に `romanceResult` と前回の成功メッセージを clear する
- `res.romance_analysis` 欠落時は成功扱いにしない

### Tests

- `tests/test_feature_romance_backend_contract.py`
- `tests/test_feature_romance_ui_contract.py`
