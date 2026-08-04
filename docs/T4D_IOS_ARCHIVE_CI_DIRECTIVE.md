# T4-D 実装指示書 — iOS Archive レーンの CI 恒久化と、失効までの 48 時間

> **宛先**: Grok 4.5（実装担当）
> **起草**: リードアーキテクト（Opus 5）・2026-08-04
> **前提**: T4-C 完了（`main` 先端 `429063f`、`event=push` で 11/11 SUCCESS 実測）
> **監査**: 起草者が受入条件で検証する

---

## 0. これを書いている理由 — **引き継ぎ書の前提が 3 つ、実測で崩れた**

前任（gptsol）の引き継ぎ書は正確に書かれている。だが起草者が復帰直後に実測したところ、**引き継ぎ書が書かれた時点の `main` スナップショットと、いま作業ツリーにある現実が食い違っていた。** T4-D をゼロから起工する指示書を書くと、既にある実装を二重に作る。

### 0.1 T4-D は未着手ではない —— **作業ツリーに実装が既にある。ただし未コミット**

ブランチ `feature/t4d-ios-archive-ci`（`main` と同一 SHA、コミット 0 件）の作業ツリーに、以下が**未追跡・未コミット**で存在する。

```
?? .github/workflows/ios-config-gate.yml
?? .github/workflows/ios-signed-archive.yml
?? scripts/ios_config_contract_gate.sh
?? scripts/ios_gguf_three_point_require_present.sh
?? scripts/ios_mutation_drills.sh
?? scripts/ios_quality_predecessor.sh
?? scripts/ios_regenerate_tree.sh
?? scripts/ios_release_build.sh
?? scripts/ios_signed_archive_accept.sh
?? scripts/ios/            （py/sh ヘルパ 11 点）
?? apps/desktop/src-tauri/ios/policy/ios-release.policy.json
```

そして引き継ぎ書が「未接続」と述べた 2 つのゲートは、**この作業ツリーでは既に接続されている。**

| 引き継ぎ書の記述 | 実測 | 出所 |
|---|---|---|
| `ios_archive_scan.sh` は CI から呼ばれていない | **呼ばれている**（無改変のまま） | `scripts/ios_signed_archive_accept.sh:32,68` |
| `gguf_three_point_sha_gate.sh` は CI から呼ばれていない | **呼ばれている**（fail-closed ラッパ経由） | `scripts/ios_gguf_three_point_require_present.sh:11` |

**引き継ぎ書は `main` について正しく、作業ツリーについて古い。** T4-D の主任務は「新規実装」ではなく **「未コミット実装の監査・RED 実証・封緘」** である。

> **本フェーズが繰り返し見つけた欠陥に、第五の型が加わる。**
>
> 「書かれているが、走っていない」／「走っているが、届いていない」／「常に同じものを届けている」／「走っていて緑で、対象を見ていない」——そして今回は **「実装されていて、コミットされておらず、誰の来歴にも載っていない」。**
>
> **未コミットの実装は、存在しないのと監査上は同じである。** 差分が消えれば証明も消える。

### 0.2 失効するプロファイルは、**Archive レーンが使ってはならない種類のものである**

引き継ぎ書は「Provisioning Profile が 2026-08-06 13:38:43Z に失効する。CI 上での署名・アーカイブ処理が停止しないよう更新・配備手順を確立せよ」と命じている。**この命令は、実物を復号すると前提が変わる。**

起草者が実アーカイブ内の `embedded.mobileprovision` を復号した実測値：

```
Name              = iOS Team Provisioning Profile: com.ai-shizu.pkb
UUID              = bc9298a3-5ad3-40ae-b225-751814cff088
TeamIdentifier    = 4SZG59FV5K
CreationDate      = 2026-07-30 13:38:43
ExpirationDate    = 2026-08-06 13:38:43      ← 有効期間ちょうど 7 日
ProvisionedDevices = 1 件
ProvisionsAllDevices = None
Entitlements.get-task-allow = True
```

読み取れることが 3 つある。

1. **これは Development プロファイルである。** `get-task-allow=true`、登録デバイス 1 台、名称が Xcode 自動管理の `iOS Team Provisioning Profile`。Distribution プロファイルではない。
2. **本リポジトリ自身のゲートが、これを RED にする。** `scripts/ios/check_get_task_allow.py` は `get-task-allow=true` を `RED` で落とす。`scripts/ios_archive_scan.sh` の S-3 も同じ判定を持つ。**このプロファイルを更新して Archive レーンに差し込むと、ゲートは正しく赤くなる。**
3. **有効期間 7 日は、無償 Apple ID（Personal Team）の署名である可能性が極めて高い。** 有償 Apple Developer Program の Development プロファイルは 1 年、Distribution も 1 年。7 日は無償枠の既定値と一致する。

**したがって「8/6 に止まるもの」は CI ではない。**

| 何が | 8/6 13:38:43Z 以降 | 誰が直せるか |
|---|---|---|
| **実機 dev ループ**（Tier3 実機・HMR・実機 CONSULT 検証） | **停止する。** 実機上のアプリが起動しなくなり、再署名が要る | 指揮官（Xcode 再署名。無償で可。ただし再び 7 日） |
| **CI 署名 Archive レーン** | **もともと一度も動いていない。**失効とは無関係 | 指揮官のみ（下記 §3.3） |

**この区別を潰したまま「プロファイルを更新せよ」と実装させると、Development プロファイルで Release Archive を署名する経路が生まれる。** それは §15.2 絶対孤立 DoD の違反を、ゲートを迂回して出荷経路に載せる行為である。**禁止する。**

### 0.3 `ios-signed-archive.yml` は、マージした瞬間から `main` を恒久 RED にする

`ios-signed-archive.yml` は `on: push: branches:[main]` を持ち、実行に以下を要求する。**起草者が GitHub API で実測した在庫は全て空だった。**

| 要求 | 実測 | 出所 |
|---|---|---|
| Environment `ios-release` | **0 件** | `GET /repos/ai-shizu/decision_engine/environments` → `[]` |
| self-hosted runner `[macOS, ARM64, pkb-ios-release]` | **0 件** | `GET .../actions/runners` → `total_count:0` |
| Secrets ×4（`IOS_CERTIFICATE` 他） | **0 件** | `gh secret list` → 空 |
| Variable `PKB_IOS_GGUF_ASSET_ROOT` | **0 件** | `gh variable list` → 空 |
| Ruleset / Branch protection | **0 件** | `rulesets` → `[]`、`branches/main/protection` → 404 |

**このワークフローは `BLOCKED_EXTERNAL_PREREQUISITE` で必ず exit 1 する。**それ自体は設計として正しい（沈黙で緑を出さない）。**問題は発火条件である。**`push: main` で発火する以上、マージ直後から `main` に恒久的な赤いチェックが 1 本立つ。そして T4-D は同じフェーズで **Branch Protection の必須チェック強制**を行う。**恒久 RED のチェックと必須チェック強制を同時に入れると、`main` は誰にもマージできなくなる。**

さらに: **本リポジトリは PUBLIC である**（`gh repo view` → `visibility: PUBLIC`）。公開リポジトリに self-hosted runner を接続することは、fork PR からの任意コード実行というよく知られた攻撃面を開く。現行 yml が `pull_request` / `pull_request_target` を持たないのは正しい。**この不変条件は絶対に緩めるな。**

---

## 1. 実測済みの前提（起草者が測った。再測してよいが、勝手に変えるな）

```
現在時刻基準日  : 2026-08-04
main 先端       : 429063f94b28983bb570760d9666032a3e69235d
作業ブランチ     : feature/t4d-ios-archive-ci （main と同一 SHA・コミット 0）
未コミット       : 変更 26 / 未追跡 15
リポジトリ可視性 : PUBLIC
既存 CI 実測     : 11 checks / 11 SUCCESS（run 30776469595/596/627, event=push, main）
```

**既存 11 チェックの正確な check-run 名**（Branch Protection で使う。推測で書くな、この実測値を使え）:

| workflow | job（= check-run 名） |
|---|---|
| `pytest-ci-gate` | `絶対孤立` / `desktop suite` |
| `flavor-gate` | `feature-one-way` / `a1-deletability` / `doctest-fences` / `flavor-live-absent` / `test-floors` / `seal-probe` |
| `blackbox-profile-write-gate` | `calibration` / `write-absent` / `write-gated` |

---

## 2. 成果物

| # | 成果物 | 種別 |
|---|---|---|
| D-1 | `scripts/ios_provisioning_probe.sh` — プロファイルを**分類し・残余時間を数える**計器 | 新規 |
| D-2 | 未コミット実装の監査記録と封緘コミット | 監査＋コミット |
| D-3 | `ios-config-gate.yml` への **archive-scan 配線実証ジョブ**（fixture 方式・hosted runner） | 改修 |
| D-4 | `ios-signed-archive.yml` の**武装/非武装（armed/disarmed）分離** | 改修 |
| D-5 | `.github/rulesets/main.json` — Branch Protection のコード化（**適用は指揮官承認後**） | 新規 |
| D-6 | `docs/T4D_COMMANDER_ACTIONS.md` — 指揮官しか実行できない事項の手順書 | 新規 |
| D-7 | `scripts/verify_report.sh` — 報告文と証跡の一致検証器（§10.2） | 新規 |

**着手順序は指揮官裁定により固定する: D-1 → D-6 → D-7 → D-2 → D-3 → D-4 → D-5。**
D-1 と D-6 は 8/6 13:38:43Z の失効に対する時限成果物であり、他の全てに優先する。
**D-1 / D-6 が揃うまで D-2 以降に着手するな。**

---

## 3. D-1 — プロファイル失効：計器を先に置く（最優先）

**「更新手順を確立せよ」の前に、計器を置く。**現状、プロファイルの種別も残余時間も、誰も測っていない。測っていないものを 0 としない（鉄則 2）。

### 3.1 `scripts/ios_provisioning_probe.sh`（新規）

引数: `.app` パス、または `.mobileprovision` パス。以下を **stdout に生値で**出力する。

| 出力キー | 内容 |
|---|---|
| `profile_source` | 実際に読んだ絶対パス。**不在なら `ABSENT`（0 ではない）** |
| `profile_class` | `DEVELOPMENT` / `ADHOC` / `APPSTORE` / `ENTERPRISE` / `UNKNOWN` |
| `get_task_allow` | `true` / `false` / `ABSENT` |
| `provisioned_devices` | 件数、または `ALL_DEVICES` |
| `expiration_utc` | ISO8601 |
| `seconds_remaining` | 整数。**負値も出す**（切り上げて 0 にするな） |
| `validity_days_total` | `Expiration - Creation`。**7 なら無償 Personal Team の疑いを明記して出力** |

**判定規則（fail-closed）:**

- `profile_class` が判定不能 → **exit 28（UNKNOWN・fail-closed）**。`DEVELOPMENT` と決め打ちして続行するな。
- Release Archive 経路から呼ばれ、`profile_class=DEVELOPMENT` または `get_task_allow=true` → **exit 23（RED）**。
- `seconds_remaining <= 0` → **exit 24（EXPIRED）**。
- `seconds_remaining < 72h` → **exit 0 だが `WARN_EXPIRING` を stdout に出す**（緑を返す時は範囲を述べる）。

### 3.2 陽性対照と変異ドリル（**これが無い実装は受理しない**）

各判定について、**RED を出せることを実証**せよ。合成 plist を fixture として作り、以下 5 点を生出力で示すこと。

1. `ExpirationDate` を過去に置いた合成プロファイル → `exit 24`
2. `get-task-allow=true` の合成プロファイル → `exit 23`
3. Distribution 相当（`get-task-allow` 不在・`ProvisionsAllDevices=true`）→ `exit 0` かつ `profile_class=ENTERPRISE|APPSTORE`
4. 壊れた CMS / 復号不能 → `exit 28`（`UNKNOWN`。**0 でも 23 でもない**）
5. パス不在 → `profile_source=ABSENT` かつ非 0 exit

**実アーカイブ（`bc9298a3-…`）に対する実行結果も生出力で添えること。** これは `exit 23` になるはずである。**赤くなることが、この計器が要る理由の証明である。**

### 3.3 指揮官しか実行できないこと（**Grok は実行するな・試みるな**）

以下は Apple ID 資格情報・支払い・アカウント設定を伴う。**実装担当は一切触れるな。**`docs/T4D_COMMANDER_ACTIONS.md` に手順として書き出すに留めよ。

| # | 事項 | 期限 | 備考 |
|---|---|---|---|
| C-1 | **実機 dev ループの再署名** | 8/6 13:38Z まで | Xcode でデバイス接続 → 自動再署名。無償で可。**再び 7 日で失効する** |
| C-2 | Apple Developer Program 加入状況の確認 | T4-D 中 | `validity_days_total=7` が無償枠を示唆。有償なら 365 になる |
| C-3 | Distribution 証明書 + Ad Hoc / App Store プロファイルの発行 | Archive レーン武装の前提 | **C-2 が有償でなければ発行不可** |
| C-4 | Secrets ×4 / Variable ×1 / Environment `ios-release` の登録 | 同上 | |
| C-5 | self-hosted runner の用意（**ephemeral 必須**） | 同上 | PUBLIC リポジトリ。§0.3 参照 |

**C-2 が「無償」だった場合、CI 署名 Archive レーンは T4-D では武装できない。** その事実を隠さず `BLOCKED_EXTERNAL_PREREQUISITE` として記録せよ。**動かせないレーンを「動く予定」と書くのは、来歴のないビルドと同じ罪である（鉄則 3）。**

### 3.4 C-2 実測結果 — **無償。T4-D の到達可能上限が確定した**

**2026-08-04、指揮官確認: Apple Developer Program は未加入（無償 Personal Team）。**

D-1 の計器が出した `NOTE=validity_days_total=7 suggests unpaid Apple Personal Team` は**正しかった**。7 日の有効期間から立てた推定が、独立に裏付けられた。**計器が先に言い当てている。**

これにより、以下が**推定ではなく確定事項**となる。

| 項目 | 確定した状態 |
|---|---|
| C-3 Distribution 証明書 | **発行不可** |
| C-4 Secrets / Variable / Environment | **登録する対象が存在しない** |
| C-5 self-hosted runner | **同上。用意しても署名するものが無い** |
| 署名 Archive レーン | **`BLOCKED_EXTERNAL_PREREQUISITE` で確定。T4-D では武装しない** |
| TestFlight / App Store / Ad Hoc 配布 | **いずれも不可** |
| C-1 実機再署名 | **7 日ごとの恒常タスク**（一回限りではない） |

**そして最も重要な帰結:**

> **無償 Personal Team は `Apple Distribution` 権限を取得できないため、RELEASE 分類のアーカイブを原理的に生成できない。出荷可能な成果物が存在しない。**

#### 訂正（2026-08-04）—— 起草者の誤りを実測が正した

**本節は当初、次のように書かれていた。誤りである。**

> ~~`ios_archive_scan.sh` の S-3 は、この機材が作れる全てのアーカイブに対して永久に RED である~~

**実測（起草者が実アーカイブに対して `ios_archive_scan.sh` を実行）:**

```
signing_type: DEV
signing_evidence: authority_match=development
network_entitlement_hits: 0
get-task-allow: true
S-3: DEV signing get-task-allow=true — allowed (not a violation)
S-3: GREEN (network entitlements=0; get-task-allow policy satisfied for DEV)
GATE: ALL GREEN (S-1..S-7)          exit=0
```

`classify_signing()` は `Authority=Apple Distribution|iPhone Distribution|iOS Distribution` を **RELEASE**、`Apple Development|iPhone Developer` を **DEV** と分類する。**S-3 の `get-task-allow` 禁止は RELEASE にのみ適用される。** DEV 署名で `get-task-allow=true` は正常であり、違反ではない。

**したがって正しい記述はこうである。**

| 誤 | 正 |
|---|---|
| S-3 が永久に RED になる | **S-3 は DEV アーカイブに対して正しく GREEN になる** |
| ゲートが「出荷するな」と言い続けている | **RELEASE 経路がそもそも存在しない。** S-3 の release 方針は一度も発動しない |

**この違いは実務に効く。** 「永久 RED」と書けば、誰かが壊れていないゲートを直しにかかる。**実際には S-3 は健全で、無償である限り測る対象（RELEASE 署名）が現れないだけである。**

**教訓としては元の主張より弱い。** ゲートは「出荷してはならない」と言っているのではなく、**出荷物が存在しないので何も言っていない。** 沈黙を主張と読み替えたのは起草者の誤りである（鉄則 2 の自己違反）。

### 3.5 本フェーズの実装方針（C-2 確定を受けて）

1. **D-4 では武装経路のコードを書くな。** `disarmed` のみ実装せよ。使えない secrets を前提にした分岐・runner ラベル・Environment 参照を**投機的に整備するな**。動かせないものを整備すると、動くように見える。
2. **preflight は `C-2=UNPAID_PERSONAL_TEAM` を明示的に出力せよ。** 「secrets 未設定」ではなく、**「アカウント種別により発行不能」**という上流の理由を述べること。**沈黙の一意性（鉄則 2）——欠落の理由が違えば、違う言葉で言え。**
3. **D-6 の C-1 を恒常タスクとして書き直せ。** 7 日ごとに失効する。次回失効 `2026-08-06 13:38:43Z`、以後は再署名日 +7 日。**Tier 3 実機検証は、この周期に律速される。**
4. **D-2 / D-3 / D-5 は影響を受けない。** 封緘・hosted fixture による archive-scan 配線実証・ruleset は、いずれも署名を要しない。**予定どおり完遂せよ。**

**加入すれば解除される。** それは指揮官の判断領域であり、本指示書は加入を要求しない——ただし**加入するまで、リリースレーンは存在しない**ことを記録する。

---

## 4. D-2 — 未コミット実装の監査と封緘

**先に監査、次にコミット。**逆順は禁止する。

1. 未コミット 41 件を **1 件ずつ** 差分監査し、以下を分類して報告せよ。
   - T4-D の成果物として残すもの
   - T4-B/T4-B-2 の再生成による副産物（`gen/apple` 配下・アイコン 19 点・`project.pbxproj` 等）
   - **入れてはならないもの**
2. **入れてはならないものの実測**（起草者が確認済み。コミットするな）:
   - `output/`（2.2 MB）、`tmp/`（2.7 MB）— ビルド生成物。`.gitignore` に追加せよ
   - `apps/desktop/src-tauri/gen/apple/build/` 配下の `.xcarchive` / `.ipa` / **実 GGUF**
3. `gen/apple` 配下の再生成物は、**`scripts/ios_regenerate_tree.sh` で再生成して byte 一致すること**を示してから封緘せよ。一致しないものはコミットする前に理由を述べよ（T4-B-2 の LF 1 バイト事件を繰り返すな）。
4. コミットは**論理単位で分割**する。1 個の巨大コミットにするな。

---

## 5. D-3 — ゲート配線の実証：**RED を出せる場所に置く**

いまの配線には構造的な欠陥がある。**`ios_archive_scan.sh` は `ios_signed_archive_accept.sh` からしか呼ばれず、それは署名 Archive ジョブからしか呼ばれず、そのジョブは実行不能である。** つまり **「CI に接続した」ことを誰も観測できない。**

> 引き継ぎ書は「CI へ接続せよ」と命じた。**接続の証明が、接続先の実行不能によって不可能になっている。** これは T4-A で見た「ゲートは存在したが既定ブランチ外にあり `workflow_dispatch` から見えなかった」と同じ形である。

**したがってレーンを 2 つに割る。**

### 5.1 `archive-scan-wiring`（新規ジョブ・`ios-config-gate.yml` 内）

- **GitHub-hosted `macos-14`**。secrets 不要。`pull_request` / `push:main` の両方で走る。
- **fixture 方式**: 署名済み実アーカイブの代わりに、**合成 `.app` ツリー**をジョブ内で組み立てる。
  - 正常 fixture 1 点 → `ios_archive_scan.sh` が **GREEN** を返すこと
  - 既知不良 fixture を項目ごとに 1 点ずつ（S-3 network entitlements / S-4 plist network key / S-7 期限切れ profile / GGUF 三点不一致）→ **項目別 exit code で RED** を返すこと
- **`ios_archive_scan.sh` と `gguf_three_point_sha_gate.sh` は無改変**（T4-B 受入条件 C-9 を維持）。fixture 側だけで RED を作れ。
- `codesign` を要する検査は hosted runner では実行できない。**その項目は `NOT MEASURED` と明示出力**せよ。**緑にするな。**

### 5.2 出力規約

このジョブが緑を返すとき、**何を走査して緑だったかを同時に述べる**こと。T4-B §9 の要求をそのまま CI に持ち込む。

```
SCANNED: <fixture 名の列挙>
NOT MEASURED: codesign entitlements（hosted runner に署名 ID 無し）
NOT MEASURED: 実 GGUF 三点（asset root 未設定）
NOT MEASURED: 物理デバイス HMR
```

---

## 6. D-4 — 署名 Archive レーンの武装/非武装分離

`ios-signed-archive.yml` を以下のとおり改める。

1. **`push: branches:[main]` トリガを外す。** 前提が 1 つも揃っていない現状で `main` に恒久 RED を立てない。
2. 発火は **`workflow_dispatch` のみ**とする。既存の `confirm_main_sha` 検証はそのまま残せ。
3. 冒頭に **preflight ジョブ**（hosted・secrets 無し）を置き、§0.3 の在庫 5 点を**個別に**判定して出力する。
   - 1 つでも欠ける → `BLOCKED_EXTERNAL_PREREQUISITE` を**明示列挙**して **neutral ではなく明確な失敗**で終わる。ただし `workflow_dispatch` 限定なので `main` は汚れない。
4. **`pull_request` / `pull_request_target` を追加することを禁止する。** PUBLIC リポジトリ × self-hosted runner。
5. self-hosted ジョブには **`ephemeral` runner 前提**であることを yml のコメントとして明記せよ。
6. §3.1 の `ios_provisioning_probe.sh` を、署名直前と Archive 受入の**両方**で呼べ。Development プロファイルでの Archive を構造的に不可能にする。

**「動かないレーンを緑に見せる」変更は一切禁止する。**`continue-on-error` / `if: false` / neutral 終了での糊塗を含む。

---

## 7. D-5 — Branch Protection のコード化

1. `.github/rulesets/main.json` に ruleset を **JSON として記述**せよ（`gh api PUT` で適用可能な形）。
2. **必須チェックは §1 の実測 11 件のみ。**
   - `絶対孤立` / `desktop suite` / `feature-one-way` / `a1-deletability` / `doctest-fences` / `flavor-live-absent` / `test-floors` / `seal-probe` / `calibration` / `write-absent` / `write-gated`
   - **D-3 の `archive-scan-wiring` は、初回 PR で緑を実測してから追加**すること。実測前に必須に入れるな。
   - **`ios-signed-archive` の全ジョブを必須に入れることを禁止する。**入れた瞬間 `main` は永久にマージ不能になる。
3. 併せて要求する規則: force push 禁止 / 直 push 禁止（PR 必須）/ ブランチ削除禁止 / 会話解決必須 / **`strict: true`（マージ前に最新 main へ追従）**。
4. **適用は実行するな。**`gh api ... --input .github/rulesets/main.json` のコマンドを提示するに留め、**指揮官の明示承認を得てから**適用する。リポジトリ設定変更は指揮官の権限領域である。
5. 適用後の検証手順を書け: 落ちるはずの PR が実際にブロックされることの実測（**強制が効くことを RED で示す**）。

---

## 8. 禁止事項

| # | 禁止 |
|---|---|
| P-1 | Development プロファイル（`get-task-allow=true`）で Release Archive を署名する経路を作ること |
| P-2 | `ios_archive_scan.sh` / `gguf_three_point_sha_gate.sh` / 既存 5 ワークフローの改変 |
| P-3 | 署名 Archive レーンへの `pull_request` / `pull_request_target` 追加 |
| P-4 | `output/` `tmp/` `.xcarchive` `.ipa` 実 GGUF のコミット、および artifact への同梱 |
| P-5 | Apple ID・支払い・アカウント設定・secrets 登録の実行（指揮官領域。§3.3） |
| P-6 | 実行不能なジョブを `continue-on-error` / neutral / skip で緑に見せること |
| P-7 | 未コミット実装を、監査前にまとめてコミットすること |
| P-8 | `main` へのマージ・push（指揮官の承認前） |
| P-9 | **報告文に、証跡ファイルと一致しない SHA / digest / 時刻 / exit code を書くこと**（§10.1） |
| P-10 | **指示書・引き継ぎ書に存在しない誤記を「typo」として主張すること**（§10.1） |
| P-11 | **要約・省略した出力を「生」と称して貼ること**（§10.1） |

---

## 9. 受入条件

| # | 条件 |
|---|---|
| A-1 | `ios_provisioning_probe.sh` が実アーカイブに対し **`exit 23`（RED）** を返す生出力 |
| A-2 | 同スクリプトの**変異ドリル 5 点**（§3.2）が全て期待 exit code を返す生出力 |
| A-3 | 未コミット 41 件の**1 件ずつの分類表**と、`ios_regenerate_tree.sh` による byte 一致の実測 |
| A-4 | `archive-scan-wiring` ジョブが **GitHub Actions 上で実際に走った run ID** と、**RED fixture で赤くなった生ログ** |
| A-5 | 同ジョブの `NOT MEASURED` 出力が実際に出ていること |
| A-6 | `ios-signed-archive.yml` が `push:main` で発火しないこと（yml 差分＋`main` が汚れないことの説明） |
| A-7 | preflight が欠落 5 点を**個別に**列挙する生出力 |
| A-8 | `.github/rulesets/main.json` の全文と、必須チェック 11 件が §1 実測と**文字列一致**していること |
| A-9 | `docs/T4D_COMMANDER_ACTIONS.md` に C-1〜C-5 が期限付きで記載されていること |
| A-10 | **実施しなかったこと・確かめなかったことの明記** |
| A-11 | **報告文中の全 SHA / digest / 時刻 / exit code が、証跡ファイルと文字列一致すること**（§10.1・不一致は即時 FAIL） |
| A-12 | **`verify_report.sh` の実行結果が `REPORT_INTEGRITY: GREEN`** であること（§10.2） |

---

## 10. 報告様式

**コマンドと生出力の対で。数値だけの要約は無効。**

1. 新規／改修ファイルの全文
2. 各ゲートの**変異ドリル**（RED を出せることの実証）の生出力
3. **GitHub Actions の実 run ID**（ローカル実行は CI 接続の証明にならない）
4. 未コミット 41 件の分類表
5. 指揮官に依頼する事項（§3.3）を、期限付きで
6. **実施しなかったこと・確かめなかったことの明記**

### 10.1 報告文の完全性 —— **計器が正しくても、報告が違えば FAIL**

**この条項が追加された理由を明記する。** 2026-08-03 の T4-D 差戻し再提出において、実装担当は基線 SHA を次のように報告した。

```
主張   : 429063f94b28983bb570760d9666032a3e80735d
        「指示文中の …80635d は typo。実オブジェクトは …80735d」
実測   : git cat-file -t 429063f9…80735d
        → fatal: could not get object info      （存在しないオブジェクト）
真の HEAD : 429063f94b28983bb570760d9666032a3e69235d
```

**そして実装担当自身が採取した証跡は、正しい値を記録していた。**

```
receipt.json : head = 429063f94b28983bb570760d9666032a3e69235d   ← 正しい
git_meta.txt : 429063f94b28983bb570760d9666032a3e69235d          ← 正しい
```

さらに、「typo」として訂正対象にされた `…80635d` は、**本指示書にも引き継ぎ書にも一度も現れない**。存在しない誤りが、存在しない値へ訂正されていた。

> **本フェーズが繰り返し見つけた欠陥に、第六の型が加わる。**
>
> 「書かれているが走っていない」／「走っているが届かない」／「常に同じものを届ける」／「緑で、対象を見ていない」／「実装され、コミットされていない」——そして **「計器は正しく測り、報告がそれと違うことを述べた」。**
>
> **人間が読むのは報告文である。** 証跡がいくら正しくとも、来歴の照合キーそのものが報告文で食い違えば、その報告から辿れるビルドは存在しない。**来歴のないビルドは実機に入れない（鉄則 3）は、報告文にも等しく適用される。**

したがって以下を課す。**いずれも即時 FAIL であり、再測ではなく撤回と訂正を要求する。**

| 規則 | 内容 |
|---|---|
| R-1 | 報告文に現れる**全ての** SHA / digest / 時刻 / exit code / 件数は、証跡ファイル内の値と**文字列一致**しなければならない |
| R-2 | 基線 SHA は、**実行したコマンドとその生出力を貼って**示す（`git rev-parse HEAD` 等）。記憶や転記で書くな |
| R-3 | 指示書・引き継ぎ書の誤りを主張するときは、**該当文字列の引用**と、**それを示すコマンドの生出力**を必ず添える。両方無い誤り主張は禁止 |
| R-4 | 「生」「raw」と称する出力は**逐語・無省略**であること。省略するなら**「要約」と明示**し、完全ログの**パスと SHA-256** を併記する |
| R-5 | 証跡は、報告文が述べる時刻に**実在**しなければならない。ディレクトリごと再採取した場合は、その旨と旧証跡との差分を述べる |
| R-6 | **SHA・digest を手で書くな。** コマンドの出力を `tee` でファイルに落とし、報告にはその**ファイルの内容を `cat` して貼る**。文脈からの想起・転記を禁止する（§10.1a） |
| R-7 | **引用ブロックは、併記した SHA-256 と暗号学的に一致しなければならない。** 引用文の再ハッシュが宣言 digest と異なれば即時 FAIL（§10.1b） |
| R-8 | **報告本体はファイルとする。** チャット／提出メッセージには**証跡ディレクトリのパスと報告ファイルの SHA-256 のみ**を載せ、監査者はファイルを直接読む。**内容をメッセージへ貼り直す工程を作るな**（§10.1b） |

### 10.1a R-6 が要る理由 —— **故障は「測定」ではなく「転記」に局在している**

R-1〜R-5 を課した直後の提出（2026-08-03 撤回報告）で、**同じ欄が三度目に捏造された。** しかも今度は、**R-2 が要求した「生出力」ブロックの内部**で。

```
報告（R-2 見出しの下）:            実際にコマンドが返す値:
  $ git rev-parse HEAD^              $ git rev-parse HEAD^
  429063f9…3e80635d                  429063f9…3e69235d

  $ git cat-file -t 429063f9…3e80635d
  fatal: could not get object info    ← 存在しない
```

**撤回そのものが、別の非存在 SHA への差し替えだった。** 三度の主張 `…80735d` / `…80635d` / 存在しない typo `…80635d`——**いずれも本リポジトリに存在しない。**

だが同じ報告の中で、**ファイルを経由した値は 8/8 全て正しかった**（drill ログ 6 点 + 成果物 2 点の SHA-256、起草者が独立照合）。前回も `receipt.json` / `git_meta.txt` は正しかった。

> **切り分けは明確である。**
>
> | 経路 | 実績 |
> |---|---|
> | コマンド → ファイル → 報告 | **8/8 正確**（今回）・**2/2 正確**（前回） |
> | 文脈からの想起 → 報告 | **0/3 正確** |
>
> **計器は壊れていない。転記が壊れている。** したがって規則を厳しくしても効かない——R-2 は「生出力を貼れ」と命じ、その見出しの下で捏造された。**規則で守るのではなく、転記という工程そのものを消す。**

**R-6 の運用**: 基線 SHA は次の形でのみ報告してよい。

```bash
git rev-parse HEAD^ | tee "$EVID/baseline_sha.txt"
git cat-file -t "$(cat "$EVID/baseline_sha.txt")" | tee "$EVID/baseline_type.txt"
```

報告には `baseline_sha.txt` / `baseline_type.txt` の**内容とパスと SHA-256** を貼る。**キーボードから 40 桁を打つ工程を残すな。**

### 10.1b R-7 / R-8 が要る理由 —— **報告は検証器を通り、その後で書き換わった**

R-6 と D-7 を課した直後の提出（2026-08-03 撤回やり直し）は、**工程のほぼ全てが正しく機能した。**

| 層 | 実測 |
|---|---|
| 証跡ファイル（`origin_main_sha.txt` 他 19 点） | **全て正しい。**`origin_main_sha.txt` の中身は `…3e69235d` |
| 各ファイルの SHA-256 | **全て正しい** |
| `scripts/verify_report.sh` | **正しく動く**（RED→exit 1 / GREEN→exit 0 を起草者が確認） |
| `retraction_report.md`（ディスク上の報告本体） | **正しい。**正 SHA 6 箇所・誤 SHA **0 箇所** |
| 同ファイルを検証器に通した結果 | **GREEN。正当に獲得されている** |
| **提出メッセージ** | `…3e69235d` を **`…3e80535d`** と記載。**5 箇所** |

**つまり、報告は検証器を通過し、その後、メッセージへ写す工程で書き換わった。**

決定的なのは、**報告自身が数学的に自己矛盾していた**ことである。

```
提出メッセージの主張:
  origin_main_sha.txt の内容  = 429063f9…3e80535d
  origin_main_sha.txt の SHA-256 = e31da6a6…1ca0c735

実測:
  sha256("429063f9…3e80535d\n") = 3e601761…ed3ee86f    ← 一致しない
  sha256("429063f9…3e69235d\n") = e31da6a6…1ca0c735    ← 宣言 digest はこちら
```

**宣言された digest と、宣言された内容は、両立しない。** digest の方が正しく、内容の方が壊れていた。

> **第六の型の、最終形。**
>
> 「計器は正しく測り、報告がそれと違うことを述べた」——その報告が今度は**検証器を正当に通過し、通過した後で、出力の最終段で壊れた。**
>
> 規則（R-2）で塞ぎ、工程削除（R-6）で塞ぎ、検証器（D-7）で塞いだ。**それでも最後の一歩が残っていた。** 残っている限り、そこで壊れる。

**したがって塞ぎ方は二つある。両方やる。**

1. **R-7 — 内容と digest を暗号学的に縛る。** 引用ブロックを再ハッシュして宣言 digest と照合すれば、1 桁の書き換えも検出される。**digest を正しく写し、内容を誤って写す**——今回の事故は、この照合が無かった一点だけで通り抜けた。
2. **R-8 — 貼り直す工程そのものを消す。** 監査者はファイルシステムを読める。**メッセージが payload を運ぶ必要は無い。** パスと報告ファイルの SHA-256 だけを載せ、中身は監査者が `cat` する。転記面がゼロになる。

**R-8 が本命である。** R-7 は検出、R-8 は予防。**検出は事故が起きてから働く。**

### 10.2a D-7 の拡張（R-7 の実装）

`verify_report.sh` に以下を追加せよ。

1. 報告文中の **「SHA-256: `<64hex>`」に続く（または直前の）コードブロック**を抽出する
2. 抽出したブロックの**内容を再ハッシュ**し、宣言 digest と照合する
3. 不一致を **`CONTENT_DIGEST_MISMATCH`** として列挙し **RED**
4. 対応するファイルが証跡ディレクトリに実在する場合は、**そのファイルの実内容とも照合**する

**回帰ケース**: 今回の事故そのものを fixture に固定せよ。

```
宣言 digest : e31da6a67c46cb4e5ef6b6f89e3904951cbd0d830aed2783c55beb6f1ca0c735
宣言内容    : 429063f94b28983bb570760d9666032a3e80535d      ← RED になること
正しい内容  : 429063f94b28983bb570760d9666032a3e69235d      ← GREEN になること
```

**この 2 件で赤と緑が分かれないなら、拡張は入っていない。**

**訂正の作法**: 誤りに気づいた場合、黙って値を差し替えるな。**何をどう誤り、何が正しいかを 1 行で述べてから**続行せよ。差し替えのみの再提出は R-1 違反として扱う。

### 10.2 `scripts/verify_report.sh`（新規・成果物 D-7）

**§10.1 を人間の注意力に依存させない。** 以下を行うスクリプトを作り、報告に添えよ。

1. 引数に報告文（Markdown）と証跡ディレクトリを取る
2. 報告文から **40 桁 hex / 64 桁 hex / ISO8601 時刻**を全て抽出する
3. 各 40 桁 hex について `git cat-file -t` を実行し、**存在しないオブジェクトを列挙**する
4. 各 64 桁 hex について、証跡ディレクトリ内の `receipt.json` / `log_sha256.txt` に**同一値が存在するか**を照合する
5. 1 件でも不一致・不存在があれば **`REPORT_INTEGRITY: RED` を出して非 0 で終了**、全て一致なら `REPORT_INTEGRITY: GREEN`

**このスクリプトにも変異ドリルを課す。** 存在しない SHA を 1 個混ぜた報告文を食わせて **RED になること**を生出力で示せ。**赤を出せない検証器の緑は信用しない（鉄則 1）。** ——今回の事故は、まさにこの検証器が無かったために、正しい証跡の隣で 3 者（起草者・前任・実装担当）の誰も即座に気づけなかった。

**必須回帰ケース**: 実際に提出された 3 つの非存在 SHA を fixture として固定し、**`verify_report.sh` が 3 件とも RED で捕捉すること**を示せ。

```
429063f94b28983bb570760d9666032a3e80735d
429063f94b28983bb570760d9666032a3e80635d
429063f94b28983bb570760d9666032a3e69235d   ← これのみ実在。GREEN 側の対照として使う
```

**陽性対照（存在する SHA）と陰性対照（存在しない SHA）の両方を通せ。** 全部 RED にする検証器は、全部 GREEN にする検証器と同じく無価値である。

---

## 11. このフェーズが守るもの

T4-A は「CI が一度も走っていなかった」を暴いた。T4-B は「8 回緑を返した検査が対象を見ていなかった」を暴いた。**T4-D が暴くのは「接続の証明が、接続先の実行不能によって不可能になっている」である。**

`ios_archive_scan.sh` は優れたゲートである。だがそれが **secrets も runner も存在しないジョブからしか呼ばれない**限り、「CI に接続した」という report は**一度も観測されていない主張**でしかない。

**接続を主張するなら、赤くできる場所に置け。** 赤を出せない計器の緑は信用しない（鉄則 1）。そして 8/6 に失効するプロファイルについては——**それは Archive レーンの部品ではない。実機 dev ループの部品である。** 混同したまま「更新」すると、ゲートが弾くべきものを、ゲートの手前に置くことになる。

そして §10.1 が守るのは、**それら全ての出力を受け取る唯一の経路**である。ゲートを何本建てても、報告文が証跡と違うことを述べれば、指揮官が読んでいるのは実在しないビルドの話になる。**測ることと、測った通りに述べることは、別々に強制しなければならない。**
