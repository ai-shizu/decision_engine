# T4-F-3a 完遂報告 — プロセス帰属つきキャプチャ装置（xctrace）

- **相位**: T4-F-3a（指示書 2026-08-05 / 前提 tip `00f988fb6764420da33933f67fe6c7a6d61070a9`）
- **枝**: `feature/t4f-absolute-isolation-runtime`
- **HEAD**: `19a14b7acd228bf544e20c0bbdddc58a188fc113`
- **採取時刻 (UTC)**: `2026-08-05T03:44:34Z`
- **証跡ディレクトリ**: `/tmp/t4f-evidence-xctrace`

本フェーズの成果物は**指揮官が回せる帰属つき接続計測装置**である。実装担当はライブ `xctrace record` を起動していない（ハング実測あり・C-4）。

---

## 指揮官裁定の記録

1. **接続 0 はバイト 0 より強い §15.2 条項 2 の主張**である（経路未成立）。バイト未測の代用ではない。
2. 陽性対照必須（別アプリ `--attach`）。
3. **NEFilter / Network Extension による in-app 帰属は禁止**（条項 1 を壊す）。

rvi0 装置（`t4f_capture_session.sh`）は**無改変で残置**。置き換えではない。

---

## J-5 — 機械的抽出手順（目視不可）

正本スキーマ: `network-connection-detected`（Networking.instrdst `schemas.xml` 実測）。

手順（証跡 `extraction_procedure.txt` digest `54b3143a21cccf9b0778373850c86759949b3b2952a8d9ecd504f75584232caf`）:

1. `xcrun xctrace export --input <run>.trace --toc --output toc.xml`
2. toc に `network-connection-detected` を確認（無ければ `*network-connection*` へフォールバック）
3. `xctrace export --xpath '/trace-toc/run[@number="1"]/data/table[@schema="network-connection-detected"]'`
4. `<row>`（無ければ `<node>`）を数える → `connection_events`
5. 失敗時は **`NOT_PARSEABLE`**（0 を捏造しない）

件数のみ残し、`.trace` は既定で削除（`T4F_KEEP_TRACE=1` で監査保持可）。

---

## J-6 — 特権要否の実測

`/tmp/t4f-evidence-xctrace/tool_inventory.txt` digest `b201379ebf2157ceb9cf5a297ea87c744ce15aa1578ef4eab7cc4383aa62db0a`:

| 項目 | 実測 |
|---|---|
| xctrace | 16.0 (17F113) / user-executable |
| `id -u` | 501 |
| `sudo -n true` | exit 1 / password required |
| root 要否 | **バイナリは root 不要** |
| ライブ実行者 | **指揮官**（デバイス操作・ハングリスク・`--time-limit` 不信） |

---

## 装置構造（J-1 順序）

`scripts/t4f_xctrace_attribution.sh` digest `1f77af7940a6d7169f6572232c04cb75da5e9fb9c9d4c9ae4ff8de8b55466653`:

| Phase | 内容 |
|---|---|
| 0 | device / bundle / xctrace 存在。`xctrace list devices` の Offline を USB 不在と読まない |
| 1 | `--attach MobileSafari`（既定）陽性対照。失敗なら本計測拒否 |
| 2 | `--attach Coraxis` + 操作 checklist（アイドル禁止） |
| 3 | `pgrep` で `xctrace record` 残骸ゼロ |

外側 **watchdog**（`T4F_WATCHDOG_SECONDS` ≫ `--time-limit`）。タイムアウトは **exit 3 / `HUNG`**。0 件と書かない。

---

## J-2 / J-3 / J-4 / J-5b — 変異ドリル

`scripts/t4f_xctrace_positive_control_mutation_drill.sh` digest `72483d84c0119abd32226800e669f08951ca863620f5e2c866e180ddf8d69543`  
stdout digest `09538738feb414b227fb7301a07a2d3af0483fbf0040a83aac1c903bce2e4ead` → `DRILL[t4f_xctrace]: PASS`

| Step | 結果 |
|---|---|
| operational `2>/dev/null` | 0 件（禁止遵守） |
| control fail | exit≠0 / measurement **ABSENT** |
| gate 破壊 | artifact **PRESENT** |
| 復元 | 再 ABSENT |
| pass | PRESENT（harness measure events=0 は対照後の正常値） |
| `T4F_HARNESS_HUNG=1` | **exit=3 HUNG** / ABSENT |
| parse fail | NOT_PARSEABLE 経路・非 0・計測主張なし |
| leftover `xctrace record` | NONE |

異常終了は `trap 't4f_on_exit $?' EXIT`（`local rc; rc=$?` はこの bash で `$?` を潰す — ハマりどころ）。

---

## J-7 — 操作範囲

装置は `operations_checklist.txt` を本計測前に書き、指揮官に記入を要求する。

**本フェーズで実機操作は未実施。** Harness が模擬した範囲:

- done: launch / record UI / profile（模擬）
- skipped: consult（モデル）、import UI
- **NOT_EXERCISED（実機未）**: 実 LLM 推論、LINE import、archive 往復、全タブ網羅

アイドル splash のみの 0 は条項 2 の証拠にしない（C-6）。

---

## J-1 ライブ陽性対照

**NOT RUN（実装担当）.** 起草者実測どおり `--time-limit` 無視ハングがあり、かつ本環境の `xctrace list devices` は対象 UDID を Offline 側に出す（USB 不在の証明ではない — AI_SKILLS）。ライブ対照の生出力は指揮官実行後に証跡へ追加すること。

指揮官コマンド:

```bash
T4F_UDID=00008150-000269103CF0401C \
  T4F_EVIDENCE_DIR=/tmp/t4f-evidence-xctrace/live \
  T4F_CONTROL_ATTACH=MobileSafari \
  T4F_TARGET_ATTACH=Coraxis \
  T4F_RECORD_SECONDS=30 \
  T4F_WATCHDOG_SECONDS=90 \
  bash scripts/t4f_xctrace_attribution.sh
```

Harness 正系（対照 5 → 計測 0）stdout digest `fccb88a34a0cad540d7f8d98b4be1e664774a9ff4ac750ed27870eb1c86b3d0f` — ライブ主張ではない。

---

## J-8 / J-9

- 封緘ゲート・既存 t4f_capture/port_scan は無改変。CI 必須セットへの追加なし。
- **実施しなかったこと**: ライブ xctrace record / ライブ陽性対照生出力 / Coraxis 実機機能横断 / rvi0 との同一セッション併用 / NEFilter / commit・push。

---

## 検証

```bash
bash scripts/t4f_xctrace_positive_control_mutation_drill.sh
bash scripts/verify_report.sh docs/T4F_XCTRACE_ATTRIBUTION_REPORT.md /tmp/t4f-evidence-xctrace
```
