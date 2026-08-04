# T4-D — 指揮官アクション手順書（実装担当は実行しない）

> **宛先**: 指揮官（Commander）
> **起草**: 実装担当（Grok 4.5）・根拠 `docs/T4D_IOS_ARCHIVE_CI_DIRECTIVE.md` §3.3
> **禁則**: 実装担当による Apple ID / 支払い / アカウント設定 / secrets 登録の実行は P-5 違反

本ドキュメントは **C-1〜C-5** のみを扱う。実装レーン（D-2 以降）の代替ではない。

基準となる失効時刻（実測・指示書 §0.2）:

```
UUID           = bc9298a3-5ad3-40ae-b225-751814cff088
ExpirationDate = 2026-08-06 13:38:43 UTC
種別           = Development（get-task-allow=true / Personal Team 7 日の疑い）
```

**このプロファイルは Archive / Distribution レーンに差し込んではならない。**
更新しても Development のままなら、`ios_provisioning_probe.sh` と `check_get_task_allow.py` が正しく RED を返す。

---

## C-1 — 実機 dev ループの再署名

| 項目 | 内容 |
|---|---|
| **期限** | **2026-08-06 13:38:43Z まで**（失効後は実機アプリ起動不能） |
| **目的** | Tier3 実機・HMR・実機 CONSULT 検証を継続する |
| **費用** | 無償 Apple ID で可 |
| **注意** | 再署名後も **再び約 7 日で失効**する。Archive 用 Distribution にはならない |

### 手順

1. 対象 iPhone を Mac にケーブル接続し、信頼する。
2. Xcode で `apps/desktop` の iOS ワークスペース／プロジェクトを開く（既存の `tauri ios dev` / Xcode Run 経路）。
3. Signing & Capabilities で Team を選択し、**Automatically manage signing**（Development）で実機向けに再署名する。
4. 実機へ Run / インストールし、起動できることを確認する。
5. 再署名後、実装担当に依頼してよい検証（指揮官が実行しなくてよい）:
   ```bash
   bash scripts/ios_provisioning_probe.sh \
     "<再署名後の .app または embedded.mobileprovision>"
   ```
   - 期待: `profile_class=DEVELOPMENT` / `get_task_allow=true` / **exit 23**（Release 禁止の証明）
   - `seconds_remaining` と `expiration_utc` を記録し、次の失効日をカレンダーに入れる
6. **やってはならないこと**: この Development プロファイルを `IOS_MOBILE_PROVISION` として CI 署名 Archive に登録すること。

---

## C-2 — Apple Developer Program 加入状況の確認

| 項目 | 内容 |
|---|---|
| **期限** | **T4-D 期間中（直ちに）** |
| **目的** | `validity_days_total=7` が無償 Personal Team であるかの確定 |
| **判定** | 有償なら Development / Distribution プロファイルは通常 **365 日** |

### 手順

1. [Apple Developer Account](https://developer.apple.com/account) に、リポジトリ署名に使う Apple ID でログインする。
2. Membership を開き、次を記録する（スクリーンショット可・secrets は貼らない）:
   - Program 名（Apple Developer Program 有償 / 無償）
   - Team ID（期待: `4SZG59FV5K` との一致／不一致）
   - 有効期限
3. 実装担当へ結果を一文で返す:
   - `C-2=PAID` または `C-2=UNPAID_PERSONAL_TEAM`
4. **`C-2=UNPAID_PERSONAL_TEAM` の場合**: CI 署名 Archive レーンは T4-D では **武装できない**。  
   記録様式: `BLOCKED_EXTERNAL_PREREQUISITE: Apple Developer Program unpaid — Distribution profile cannot be issued`  
   **「後で動く予定」と書かないこと。**

---

## C-3 — Distribution 証明書 + Ad Hoc / App Store プロファイル発行

| 項目 | 内容 |
|---|---|
| **期限** | Archive レーン武装の前提（C-2 有償確定後） |
| **前提** | **C-2 が有償でなければ発行不可** |

### 手順（C-2=PAID のときのみ）

1. Certificates で **Apple Distribution** 証明書を作成／既存を確認する。
2. Identifiers で `com.ai-shizu.pkb` の App ID を確認する。
3. Profiles で次のいずれかを発行する（Archive 方針に合わせる）:
   - **App Store Connect** 配布用（App Store / TestFlight）
   - 必要なら **Ad Hoc**（指定デバイス）
4. 発行した `.mobileprovision` をローカルで検査する（実装担当に委譲可）:
   ```bash
   bash scripts/ios_provisioning_probe.sh /path/to/distribution.mobileprovision
   ```
   - 期待: `profile_class=APPSTORE` または `ADHOC` / `ENTERPRISE`
   - 期待: `get_task_allow=false` または `ABSENT`
   - 期待: **exit 0**（exit 23 なら Development が混入している）
5. **Development（`iOS Team Provisioning Profile` / `get-task-allow=true`）を C-4 の `IOS_MOBILE_PROVISION` に使わない。**

---

## C-4 — Secrets ×4 / Variable ×1 / Environment `ios-release` の登録

| 項目 | 内容 |
|---|---|
| **期限** | Archive レーン武装の前提（C-3 完了後） |
| **リポジトリ** | PUBLIC — 値を Issue / PR / chat に貼るな |

### 登録対象

| 種別 | 名前 | 内容 |
|---|---|---|
| Environment | `ios-release` | GitHub Environment を新規作成。保護ルールは指揮官裁量 |
| Secret | `IOS_CERTIFICATE` | Distribution 証明書（p12 等・既存 Tauri 手動署名規約に従う） |
| Secret | `IOS_CERTIFICATE_PASSWORD` | 上記パスワード |
| Secret | `IOS_MOBILE_PROVISION` | **C-3 の Distribution プロファイル**（Development 禁止） |
| Secret | `APPLE_DEVELOPMENT_TEAM` | Team ID |
| Variable | `PKB_IOS_GGUF_ASSET_ROOT` | 実 GGUF 読み取り専用ルート（マシンローカル絶対パス） |

### 手順

1. GitHub → Settings → Environments → `ios-release` を作成。
2. 同 Environment に Secrets ×4 を登録（リポジトリ secrets ではなく Environment 推奨）。
3. Repository variables に `PKB_IOS_GGUF_ASSET_ROOT` を登録。
4. 登録後、値そのものは共有せず、**名前の存在**のみ実装担当へ通知する。
5. 武装前の期待: `ios-signed-archive.yml` の preflight（D-4）が欠落 0 件になること。

---

## C-5 — self-hosted runner の用意（ephemeral 必須）

| 項目 | 内容 |
|---|---|
| **期限** | Archive レーン武装の前提 |
| **ラベル** | `[self-hosted, macOS, ARM64, pkb-ios-release]` |
| **必須** | **ephemeral**（ジョブ終了で破棄）。PUBLIC リポジトリのため常設 runner は攻撃面になる |

### 手順（概要）

1. 署名用 Mac（ARM64）を用意する。GGUF asset root と Xcode / 証明書キーチェーンにアクセスできること。
2. GitHub Actions の **ephemeral** self-hosted runner を登録する（公式ドキュメントの ephemeral 手順に従う）。
3. ラベルに `macOS` / `ARM64` / `pkb-ios-release` を付与する。
4. **`pull_request` / `pull_request_target` からこの runner を呼ばない**（現行 yml も禁止のまま維持）。
5. fork PR が self-hosted に届かないことを確認する。

---

## 依存関係（要約）

```
C-1（実機 8/6 まで） ── 独立。無償でも実施可。Archive とは別物。
C-2（有償確認） ─── 否 → Archive 武装は BLOCKED_EXTERNAL_PREREQUISITE
                 └─ 是 → C-3 → C-4 → C-5 → はじめて Archive レーン武装可
```

## 実装担当が実施しないこと（再掲）

- Apple ID ログイン、支払い、Program 加入操作
- secrets / variables / Environment / runner の作成・登録
- Development プロファイルの「更新」をもって Archive 準備完了と称すること

## C-2 実行結果（指揮官確定）

| 項目 | 値 |
|---|---|
| 結果 | `C-2=PAID` |
| 意味 | Apple Developer Program 有償。C-3 以降の Distribution 発行が可能 |
| 記録日 | 2026-08-04（指揮官通達） |
| 実装担当 | 未実行（本記録は通達の反映のみ） |

