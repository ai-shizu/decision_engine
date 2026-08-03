# M0 iOS初期化 実装指示書（Composer-Executable Blueprint）

> **読者:** 実働部隊（Composer / 自律型コーディングAI）。**一字一句従え。裁量による拡張・美学の再解釈・スコープの前借りを禁ずる。**
> **起草:** Fable（主任アーキテクト）。
> **親仕様:** `docs/ROADMAP_IOS_TAURI_V2.md`（Standalone Pivot / rev.3・承認済み）の **§5 Workstream A（A2 / A2.1 / A3 / A4）** を本タスクへ具体化したもの。
> **本タスクの位置づけ:** ロードマップの実働第一歩。「Tauri v2 iOSターゲットの初期化」と「iPhoneシミュレータ上での既存UIシェル起動」までを射程とする。**embedded Python・llama.cpp・SQLCipher・LLM・recovery archiveは本タスクの対象外（M2〜M6）。**
> **前提（指揮官ACK済み）:** G0-C.1 Eligibility＝**Option A（A17 Pro以降・8GB・3〜4B級）**承認。絶対孤立ADR / Pocket Brain rubric / Freeze Transition ADR承認。**UI.D ACK発行済み（UI凍結解除）。** ただし本タスクはUI表層に触れない（後述§1）。

---

## 0. 検証済みリポジトリ事実（推測禁止・起草時に実地検分した現状）

Composerはこれらを前提とせよ。**再確認は許すが、以下と異なる状態を見つけたら実装せず即HARD STOP報告すること。**

- **作業ルート:** `apps/desktop/`（フロント）＋ `apps/desktop/src-tauri/`（Rust/Tauri）。
- **Tauriは既にv2。** `@tauri-apps/api`/CLI が `^2`（[package.json:17,24](apps/desktop/package.json)）、config schema v2（[tauri.conf.json:2](apps/desktop/src-tauri/tauri.conf.json)）。**v1→v2移行や `tauri migrate` を実行するな。**
- **bundle identifier は `com.ai-shizu.pkb`（[tauri.conf.json:5](apps/desktop/src-tauri/tauri.conf.json)）。変更禁止。** `tauri ios init` はこの既存IDを使う。
- **mobile用crate-typeは既に設定済み**（[Cargo.toml:9-11](apps/desktop/src-tauri/Cargo.toml) = `["staticlib","cdylib","rlib"]`）。**触るな。**
- **`mobile_entry_point` は既に存在**（[lib.rs:22](apps/desktop/src-tauri/src/lib.rs) の `#[cfg_attr(mobile, tauri::mobile_entry_point)]`）。**新規追加不要。**
- **platform override patternが既に存在する。** [tauri.macos.conf.json](apps/desktop/src-tauri/tauri.macos.conf.json) が雛形。base config の PowerShell コマンドを `bash scripts/run-*.sh` へ差し替え、bundle を macOS 用に上書きしている。**iOS override はこれに倣う。**
- **`gen/apple` はまだ存在しない**（`src-tauri/gen/` 配下は `schemas/` のみ）。= iOS未初期化。
- **`scripts/run-dev.sh` / `run-build.sh` は既に存在する**（macOS host用bash版）。新規作成不要。
- **base の `beforeDevCommand` / `beforeBuildCommand` はPowerShell**（[tauri.conf.json:7,9](apps/desktop/src-tauri/tauri.conf.json)）。macOS host にPowerShellは通常無いため、iOS override で bash版へ差し替える。
- **base bundle は `targets:["nsis"]`＋`externalBin:["binaries/pkb-engine"]`**（[tauri.conf.json:33,36-38](apps/desktop/src-tauri/tauri.conf.json)）。これはWindows/desktop用。**base を書き換えず、iOS override で無効化する（§3 Phase B）。**
- **`setup()` が sidecar を起動し、失敗を伝播する。** [lib.rs:63-78](apps/desktop/src-tauri/src/lib.rs) は `manager.start(handle)` の結果を `?` で伝播（[lib.rs:66](apps/desktop/src-tauri/src/lib.rs)）し、続けて `WebviewWindowBuilder::from_config` で手動ウィンドウ生成（`create:false` 前提）。iOSにはPython sidecarが存在しないため、このまま起動すると `setup()` が Err を返し**アプリが起動しない**。ゆえに§3 Phase C の最小 cfg ガードが必須。
- **`build.rs` は全targetで engine placeholder を生成する**（[build.rs:14-36](apps/desktop/src-tauri/src/build.rs)）。iOS build で不要な `binaries/pkb-engine-*-apple-ios` stub を生むため、§3 Phase C で `ios` を除外する。
- **`paths.rs` の release用 `bundled_engine_name()` に iOS arm が無い**（[paths.rs:135-158](apps/desktop/src-tauri/src/paths.rs) は windows/macos/linux のみ）。ただしこれは `#[cfg(not(debug_assertions))]`＝**release専用**。本タスクの `tauri ios dev`（debug）ではコンパイルされない。**本タスクでは触るな**（release対応はM1のA3で別途扱う）。
- **capability は単一 `capabilities/default.json`**（`windows:["main"]`＋window close/minimize/maximize権限、[default.json](apps/desktop/src-tauri/capabilities/default.json)）。iOSにdesktop window制御は不適。§3 Phase C の条件付き対応を見よ。
- **既存テスト基盤:** boundary テストは純関数のみ（`npm run test:boundary`）。Rust は `cargo test -p pkb-desktop --lib`。**iOS初期化でこれらの結果を1件も変えてはならない。**

---

## 1. 絶対規律（交渉不可）

1. **既存デスクトップ版のロジック・UIを1バイトも壊すな。** desktop（Windows/macOS）の挙動はbyte-identicalに保つ。Rust変更はすべて `#[cfg(mobile)]` / `#[cfg(not(mobile))]` 等のcompile-time分岐で行い、desktop経路のトークン列を変えない。
2. **凍結ファイル（本タスクでは diff ゼロ）:**
   ```text
   apps/desktop/src/**                      # React/CSS/TS（App.css・index.html・components・src/lib すべて）
   apps/desktop/tests-runtime/**            # boundary テスト
   apps/desktop/src-tauri/src/commands.rs / engine.rs / knowledge/** / artifact_auth.rs / ipc_contract.rs / webview_policy.rs / os_sandbox.rs
   apps/desktop/src-tauri/src/paths.rs      # release cfg はM1。本タスクでは触るな
   src/** tests/** scripts/** config/**     # Python コア・憲法ガード・golden・ビルドスクリプト
   apps/desktop/package.json / package-lock.json / vite.config.ts / tsconfig*.json / Cargo.toml
   apps/desktop/src-tauri/tauri.conf.json   # base config（iOS override で対応するため base は不変）
   apps/desktop/src-tauri/tauri.macos.conf.json
   ```
   UI.D ACK は出ているが、**本タスクはUI表層・フォント同梱・Native Ceremonyに着手しない**（それらはM1/M5）。App.css・index.html を触るな。
3. **新規依存を追加するな。** npm も Cargo も。`tauri ios init` が `gen/apple` 内へ生成する Xcode/CocoaPods 構成は許容だが、`Cargo.toml` / `package.json` への手動 dep 追加は禁止。CocoaPods が iOS プロジェクト内に入れる Pods は Tauri 標準生成物の範囲に限る。
4. **ネットワーク・egressを一切足すな（絶対孤立）。** ATS例外、`NSAllowsArbitraryLoads*`、Local Network、Bonjour、socket、network entitlement、`NSLocalNetworkUsageDescription`、`NSCameraUsageDescription` を追加しない。dev server の localhost はTauri開発時のものであり、release configへ混ぜない。
5. **本タスクのスコープ外に踏み込むな。** embedded Python / NumPy / llama.cpp / Metal / SQLCipher / LLM / 384D search / recovery archive は **一切実装しない**（M2〜M6）。「UIシェルがシミュレータで描画される」以上のことをするな。
6. **`gen/apple` は disposable。** `tauri ios init` の生成物を手編集で「育てない」。手修正が必要に見えたらHARD STOP。
7. **git規律:** `git add -A` / `git add .` 禁止。`.claude/` を stage するな。push 禁止。`reset --hard` / `checkout --` / `revert` 禁止。当該タスクで触れたファイルだけを明示 stage する。

---

## 2. 前提環境の確認（実装前チェック）

本タスクは **macOS + Xcode 環境が必須**（Windows 上では `tauri ios init` は完遂できない）。次を確認し、欠けていればHARD STOP報告：

```bash
sw_vers                                  # macOS であること
xcodebuild -version                      # Xcode（iOS/iPadOS 26 SDK以上が望ましい。無ければ報告）
xcode-select -p                          # Command Line Tools のパス
pod --version                            # CocoaPods
rustc --version && cargo --version
rustup target list --installed | grep -E 'aarch64-apple-ios|x86_64-apple-ios|aarch64-apple-ios-sim'
node --version && npm --version
```

iOS Rust target が無ければ追加（依存追加ではなくtoolchain整備）：

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
```

---

## 3. 実行ステップ

### Phase A — iOSプロジェクト生成（純初期化・コード変更ゼロ）

作業ディレクトリは `apps/desktop/`。

```bash
cd apps/desktop
npm run tauri ios init
```

- 生成される `src-tauri/gen/apple/` を確認する。**手編集しない。**
- `tauri ios init` が `tauri.conf.json`（base）を書き換えた場合は**その差分を破棄せず一旦確認**し、base への不要な変更（dev URL埋め込み等）があればHARD STOP報告（§1-2でbaseは不変が原則）。
- この時点で `git status` を目視し、生成物が `gen/apple/` に限定されていることを確認する。

### Phase B — iOS platform override 作成（`tauri.macos.conf.json` に倣う）

`apps/desktop/src-tauri/tauri.ios.conf.json` を新規作成する。**baseを書き換えず、override で iOS 固有値だけを与える**：

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "build": {
    "beforeDevCommand": "bash scripts/run-dev.sh",
    "beforeBuildCommand": "bash scripts/run-build.sh"
  },
  "bundle": {
    "externalBin": [],
    "iOS": {
      "minimumSystemVersion": "17.0"
    }
  }
}
```

- `beforeDevCommand`/`beforeBuildCommand` を bash 版へ差し替えるのは、macOS host に PowerShell が無いため（macos override と同じ理由・同じ値）。
- `externalBin: []` で base の `binaries/pkb-engine` 参照を iOS ビルドから除外する（iOSに sidecar は存在しない）。
- `minimumSystemVersion: "17.0"` はロードマップA2の pin。
- **`UIDeviceFamily` を Info.plist へ手書きしない。** iPhone最適化（`TARGETED_DEVICE_FAMILY=1`）は生成設定側で扱う（本タスクでは既定のまま。iPad compatibility mode の検証はM1/M5）。
- **Option A の `UIRequiredDeviceCapabilities`（`iphone-performance-gaming-tier`）を本タスクで設定しない。** これはLLM同梱（M4）と同時に入れる。今入れるとシミュレータ起動を不必要に阻害しうる。

### Phase C — シミュレータ起動のための最小 compile-time ガード

**目的:** `#[cfg(mobile)]` で iOS 経路だけを分岐し、sidecar起動をスキップして WebView シェルを描画する。**desktop 経路は 1 バイトも変えない。**

**C-1. `build.rs`: iOS で engine placeholder を生成しない。**
[build.rs:36](apps/desktop/src-tauri/src/build.rs) の `ensure_engine_placeholder` 呼び出しを、iOSターゲットで**スキップ**する。既存の `target` 文字列判定（`build.rs` は `TARGET` env を保持済み）を使い、`target` が `ios` を含む場合のみ呼ばない。他 target（windows/macos/linux）の挙動は不変にすること。

**C-2. `lib.rs`: `setup()` の sidecar 起動と手動ウィンドウ生成を mobile で分岐。**
[lib.rs:63-79](apps/desktop/src-tauri/src/lib.rs) の `setup` クロージャを compile-time 分岐する。**desktop（`not(mobile)`）は現行コードをそのまま維持**し、`mobile` では **`manager.start()` を呼ばず、手動 `WebviewWindowBuilder::from_config` も行わない**（iOSはTauri mobile runtimeが config からwebviewを自動生成する）。

目標形（**参考スケルトン。Composerはコンパイル可能性を必ず自分で検証し、既存の型・変数名に厳密に合わせよ**）:

```rust
.setup(move |app| {
    #[cfg(not(mobile))]
    {
        let handle = app.handle().clone();
        let manager = Arc::clone(&engine);
        tauri::async_runtime::block_on(manager.start(handle)).map_err(std::io::Error::other)?;

        let window_config = app
            .config()
            .app
            .windows
            .first()
            .ok_or_else(|| std::io::Error::other("main window config is missing"))?;
        WebviewWindowBuilder::from_config(app.handle(), window_config)?
            .on_navigation(|url| webview_policy::current_navigation_allowed(url.as_str()))
            .on_new_window(|_, _| NewWindowResponse::Deny)
            .on_download(|_, event| !matches!(event, DownloadEvent::Requested { .. }))
            .build()?;
    }
    #[cfg(mobile)]
    {
        // iOS: Tauri mobile runtime が config から main webview を生成する。
        // sidecar は存在しないため start() を呼ばない（engine は "not ready" のまま）。
        let _ = &engine; // 未使用move変数の警告回避（既存の所有関係を壊さない範囲で）
    }
    Ok(())
})
```

- `engine` / `engine_for_exit` の `Arc` 所有関係、`RunEvent::Exit` での `shutdown()`（[lib.rs:83-87](apps/desktop/src-tauri/src/lib.rs)）は desktop で不変に保つこと。mobile 側で未使用となる import / 変数が出たら、`#[cfg]` か `let _ =` で**警告を潰すに留め、desktop の記述を削るな**。
- `main` window の `create:false`（[tauri.conf.json:19](apps/desktop/src-tauri/tauri.conf.json)）は desktop 前提。iOS で webview が二重生成/未生成になる場合は、**base を変えず** iOS override 側で `app.windows` の扱いを調整できるか検討し、**それでも解決しなければ HARD STOP**（勝手に base の `create` を書き換えない）。

**C-3. capability（条件付き）。**
`tauri ios init` が mobile 用 capability を生成した場合はそれを使う。生成されず、かつ既存 `capabilities/default.json` の window 権限（close/minimize/maximize）が iOS ビルド検証で失敗する場合のみ：
- `default.json` に `"platforms": ["macOS", "windows", "linux"]` を加えて desktop 限定にし、
- iOS 用に window 制御権限を含まない最小 capability（`core:event:*` と webview deny-create 程度）を別ファイルで追加する。
- **capability 検証が通っているなら触るな**（過剰な変更を避ける）。

---

## 4. 成功条件（Definition of Done）

すべて満たして初めて完了とする。実測ログを報告に添付せよ。

1. **desktop 非回帰（最優先）:**
   - `git diff` が §1-2 の凍結ファイルに対して**出力ゼロ**。
   - Rust が desktop でビルド可能（`cargo build -p pkb-desktop`）。
   - `npm run test:boundary` が現行と同じく全 GREEN。
   - `cargo test -p pkb-desktop --lib` が現行と同じく GREEN。
2. **iOSプロジェクト生成:** `src-tauri/gen/apple/` が存在し、手編集痕が無い。`tauri.ios.conf.json` が Phase B の内容で存在する。
3. **シミュレータ起動:**
   ```bash
   cd apps/desktop
   npm run tauri:ios-dev
   ```
   （**M0 限定の履歴手順** — 当時は featureless UI シェル到達が DoD。`pocket-brain` / `secure-vault` は未導入。）
   **現在の標準形（T4-B-2 以降）:**
   ```bash
   npm run tauri:ios-dev -- --features pocket-brain,secure-vault
   ```
   iPhoneシミュレータが起動し、**既存の7タブUIシェルが描画され、タブ切替（Alt/タップ）が動作し、クラッシュしない**こと。
4. **エンジン挙動の許容範囲:** 本タスクに embedded Python は無いため、engine 依存操作（consult・record 等の実データ処理）は **"not ready" / 未接続の既存挙動**で構わない。**engine を動かそうとするな**（それはM2）。DoDは「UIシェルが正常に立ち上がること」であって「エンジンが応答すること」ではない。
5. **証跡:** シミュレータのスクリーンショット（UIシェル描画状態）と、上記コマンドの実測出力を報告に添える。

---

## 5. やってはいけないこと（明示的禁止・スコープ前借り防止）

- embedded CPython / NumPy / `_pkb_native` / llama.cpp / Metal / SQLCipher / 384D search / GGUF の導入。→ **M2〜M4**
- recovery archive（`.pkbvault`）・journal・DeviceSigner の実装。→ **M3/M6**
- App.css・index.html・components の改変、BIZ UDGothic 同梱、Native Ceremony、Haptic 実装。→ **M5**
- `Cargo.toml` / `package.json` への依存追加、`paths.rs` の release cfg 変更、base `tauri.conf.json` の書き換え。
- ネットワーク/socket/Bonjour/camera/ATS 関連の一切の追加。
- `UIRequiredDeviceCapabilities` / `UIDeviceFamily` の手書き投入。
- `gen/apple` の手編集、`.claude/` の stage、`git add -A`、push。
- 上記いずれかが「必要」に見えた場合、**実装せず §6 の異常時HARD STOP**。

---

## 6. 停止規律（HARD STOP）とコミット

### 6.1 正常完了時
1. `git status --short` を目視し、**当該タスクで触れたファイルのみ**を stage する。想定 stage 対象：
   ```text
   apps/desktop/src-tauri/tauri.ios.conf.json      # 新規
   apps/desktop/src-tauri/gen/apple/**             # tauri ios init 生成物（Tauri既定の .gitignore に従う）
   apps/desktop/src-tauri/src/lib.rs               # Phase C-2（mobile cfg 分岐のみ）
   apps/desktop/src-tauri/src/build.rs             # Phase C-1（ios 除外のみ）
   apps/desktop/src-tauri/capabilities/**          # Phase C-3 を行った場合のみ
   ```
   `.claude/` を含めない。`git add -A`/`.` を使わない。
2. アトミックコミット（メッセージは指揮官指定を厳守）：
   ```
   chore(build): initialize tauri iOS standalone target

   Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
   ```
3. push せず**直ちに停止**し、指揮官へ報告する。報告必須項目：
   - §4 の各コマンド実測結果（PASS数含む）と凍結ファイル diff ゼロの証拠。
   - `git diff --stat` と `git log --oneline -1`。
   - シミュレータUIシェル描画のスクリーンショット。
   - Phase C で加えた cfg 分岐の要約（desktop 経路 byte-identical の宣言）。

### 6.2 異常時（即停止・実装せず報告）
- §0 の検証済み事実と異なるリポジトリ状態を発見した。
- `tauri ios init` が base `tauri.conf.json` や凍結ファイルを書き換える。
- シミュレータ起動に §3 に列挙した以外の blocker（追加の Rust 修正・依存・network設定）が必要になった。
- desktop の boundary / cargo test が1件でも RED になった（＝ desktop 経路を壊した兆候。テストを直すな。自分の diff を疑え）。
- macOS/Xcode/CocoaPods 環境が §2 を満たさない。

いずれも**現状を巻き戻さずそのまま報告**し、Fable の追加指示を待て。**ACK なく次のマイルストーン（M1 以降）へ進むな。**
