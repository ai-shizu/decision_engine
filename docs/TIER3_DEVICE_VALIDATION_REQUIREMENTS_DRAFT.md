# Tier 3 実機検証 — 要件定義書（**ドラフト／たたき台**）

**状態**: **ドラフト。指揮官の裁定を経ていない。** 着工指示書ではない。
**起草**: リードアーキテクト（監査役）・2026-07-29
**基準コミット**: `3af443d`（F-3 完遂時点）
**親**: `docs/AI_SKILLS.md` §5.1（取込ゲート as-built）/ 第三律（Jetsam の法則）/ §1.1（iOS メモリ絶対の掟）

---

## 0. 起草にあたって実測した事実（推測ではない）

本ドラフトは以下の**コード実測**に基づく。数値・経路はすべて `3af443d` で確認した。

| # | 実測事実 | 出典 |
|---|---|---|
| 1 | **シミュレータは Metal を使っていない。** `effective_n_gpu_layers(requested, ios_sim) → 0`、`apply_context_device_policy` が `offload_kqv(false)` / `op_offload(false)` を強制 | `llm/service.rs:1374-1392` |
| 2 | 判定は `cfg!(all(target_os = "ios", target_abi = "sim"))` —— **実機ではこの分岐に入らない** | `llm/service.rs:1397, 1490` |
| 3 | 要求値は **`n_gpu_layers: 999`**（全層オフロード） | `llm/commands_consult.rs:92` |
| 4 | GGUF は**バンドルリソース**として同梱。`"../models/pocket-brain.gguf": "models/pocket-brain.gguf"` が **`tauri.conf.json` と `tauri.ios.conf.json` の両方**にある | 両 conf の `bundle.resources` |
| 5 | ロード優先順位は**バンドル優先** → AppData フォールバック | §5.1-3 / `model_path::resolve_loadable_model_path` |
| 6 | **iOS entitlements が空**（`<dict/>`）。`com.apple.developer.kernel.increased-memory-limit` は**無い** | `gen/apple/pkb-desktop_iOS/pkb-desktop_iOS.entitlements` |
| 7 | 計測器は既に在る: `phys_footprint_bytes()` / `os_proc_available_memory_bytes()` / `FootprintBand`（Comfort <0.70 / Elevated ≥0.70 / High ≥0.85 / Over ≥1.0） | `monitor/probe.rs`, `monitor/degradation.rs:52-79` |
| 8 | モデル実体: **1,117,320,736 バイト** / sha256 `6a1a2eb6…9407e` / 26 KV・**339 テンソル**・GGUF V3 | Tier 2 証跡（§5.1）＋ F-3 T-8 実測 |

---

## 1. 目的とスコープ

### 1.1 Tier 2 から持ち越されたリスクの再定義

§5.1 は Tier 3 の残置理由を「実機の Files は iCloud/他アプリ提供の実プロバイダを含む」としている。**これは正しいが、リスクの主要部分ではない。**

**実測が示す本当のリスクは、Tier 3 が独立した 4 つの未知を同時に持ち込むことである。**

| ID | 未知 | Tier 2 で検証されたか |
|---|---|---|
| **U-1** | 1.1GB のバンドルが実機にインストールでき、バンドル内リソースとして解決できるか | **否**（シミュレータはホスト FS） |
| **U-2** | Jetsam 制約下でモデルがロードできるか | **否**（シミュレータはホストのメモリを使う） |
| **U-3** | **Metal オフロードが正しい logits を出すか** | **否 —— この経路は一度も実行されていない** |
| **U-4** | security-scoped 取込が実プロバイダ（iCloud 等）で成立するか | 部分的（`group.com.apple.FileProvider.LocalStorage` のみ） |

> **U-3 が本フェーズの最大の未知である。** コードコメント自身がこう書いている ——
>
> > *The Simulator exposes a synthetic Metal device without the unified/shared memory capabilities llama.cpp expects for reliable quantized inference. Keep full Metal offload on physical iOS devices, but force Simulator builds onto the host CPU so corrupted logits cannot reach the stream.*
>
> **「壊れた logits がストリームに届かないように」シミュレータを CPU に落としている。** つまり実機の Metal 経路は、**設計者自身が慎重さを要すると判断した経路であり、かつ誰も実行していない。**

### 1.2 U-2 と U-3 は分離せよ（本ドラフトの中核提案）

**素朴な Tier 3 は「実機で動かす」の一手で U-1〜U-4 をまとめて試す。それは失敗したときに原因が分からない。**

F-3 で確立した作法を適用する —— **A-1-3（実モデル）と A-1-2b（作り置き）を分けたのと同じ操作である。**

実機上で **Metal を確実に排した経路**を用意し、次の順で進める:

```
G-2  実機 ＋ CPU オラクル構成         → U-2（メモリ）だけを試す
G-3  実機 ＋ n_gpu_layers=999（Metal）→ U-3（バックエンド）だけを試す
```

> **【訂正・2026-07-29】`n_gpu_layers = 0` だけでは CPU 経路にならない。**
>
> 当初本ドラフトは「`n_gpu_layers` を 0 に強制する」と書いた。**これは誤りである。** gptsol の指摘を受け、固定版ソースで確認した:
>
> ```cpp
> // llama-context.cpp:257-265（llama-cpp-sys-2 0.1.151 同梱）
> for (const auto & dev : model.devices) {
>     ggml_backend_t backend = ggml_backend_dev_init(dev.dev, nullptr);
>     ...
> }
> ```
>
> **コンテキストは `model.devices` の全デバイスについて backend を初期化する。`n_gpu_layers` は層の *割当* にしか効かず、backend の *初期化* を抑止しない。** したがって `n_gpu_layers = 0` のままでも Metal backend は起き、Metal コンテキストバッファが確保される —— **U-2 と U-3 が分離できていない。**
>
> **正しい CPU オラクル構成**（4 つすべてを併用する）:
>
> | 設定 | API（固定版に実在を確認済み） |
> |---|---|
> | `model.devices` を空にする | `LlamaModelParams::with_devices(&[])`（`params.rs:515`） |
> | 層を CPU に置く | `with_n_gpu_layers(0)` |
> | KQV を CPU に置く | `with_offload_kqv(false)` |
> | op offload を止める | `with_op_offload(false)` |
>
> **指揮官裁定 4 の `CORAXIS_FORCE_CPU=1` は、この 4 点セットを駆動するものとして実装せよ。** 層数だけを 0 にする実装は、**分離したつもりで分離していない**という本プロジェクトが最も嫌う形になる。

**G-2 が落ちればメモリの問題、G-3 だけが落ちれば Metal の問題**と一意に切り分かる。まとめて試すと「実機で動かない」しか分からない。

> **注意**: これは `effective_n_gpu_layers` の**恒久的な変更ではない**。実機を CPU に落とす既定を導入したら U-3 は永久に未検証のまま残る。**検証用の一時的な上書き経路**（環境変数・debug 専用 cfg 等）として設計し、**G-3 では必ず既定（999）に戻して測る。**

### 1.3 本フェーズの最終ゴール — 出荷ブロッカーの解除基準

> **実機上で、バンドル同梱の GGUF がロードされ、Metal オフロードで生成が成立し、フレーバがガードを通過して表示スロットへ到達し、その間のメモリ footprint が実測値として記録されること。**

**「クラッシュしなかった」は解除基準ではない。** 理由は §3.1 に述べる。

---

## 2. 物理制約とサンドボックス突破機構

### 2.1 メモリ — Jetsam は交渉しない（第三律）

**1.1GB のモデル ＋ WKWebView ＋ Metal バッファが、既定の Jetsam 上限に収まるかは未知である。**

**entitlements が空である**という実測が、ここで効く。`com.apple.developer.kernel.increased-memory-limit` は Jetsam 上限を引き上げる標準的な緩和策であり、**現在付与されていない。**

**本ドラフトの提案**: **付与を前提にするな。まず素の状態で測れ。**

理由は二つ。第一に、entitlement には provisioning profile / 開発者アカウント種別の制約があり得る（→ **gptsol タスク候補 Q-2**）。第二に、**素の状態での footprint が分からないまま上限を上げると、「必要だったのか」が永久に分からなくなる。** 数字を得てから判断する。

### 2.2 mmap とページの性質 — 計算の前提が変わりうる

llama.cpp は既定でモデルを **mmap** する。**バンドル内リソースを mmap した場合、そのページはファイルバックの clean ページ**であり、匿名の dirty ページとは Jetsam への計上が異なる可能性がある。

**これが成立するなら、1.1GB は phys_footprint に丸ごと乗らない。** 成立しないなら乗る。**この一点で Tier 3 の成否見通しが根本的に変わる。**

**推測で進めるべきではない。** → **gptsol タスク候補 Q-1**

### 2.3 バンドル戦略 — 現行構成を変えない

**現行のバンドル同梱（`bundle.resources`）を維持することを提案する。** 変更を提案しない理由:

- **§5.1-3 のロード優先順位（バンドル優先 → AppData）は Tier 1/2 で GREEN であり、実績のある経路である**
- On-Demand Resources / 起動後ダウンロードへの変更は、**第一律（完全オフライン）に触れる**。ネットワーク経由のモデル取得は §5.1 の射程外であり、封印庫（§18）の「`knowledge_fetcher` の外向き再開放」と同じ精神で扱うべきである
- 1.1GB がバンドルにあること自体は、**読み取り専用・署名済み・mmap 可能**という利点でもある

**ただし出荷時の制約は別途確認を要する** —— IPA サイズ、App Store の配信上限、`codesign` が 1GB 級リソースを扱えるか。→ **gptsol タスク候補 Q-3**

### 2.4 Rust レイヤーからの読み込み — 既存経路を変えない

`path().resolve("models/pocket-brain.gguf", BaseDirectory::Resource)` が実機で**シミュレータと同じパスを返すか**は未確認である（シミュレータは `.app/assets/models/`）。**解決したパスを実測でログへ出すことを G-1 の要件とする。**

**`validate_gguf_file(path)` が magic 検証を行っている**（`load_model` の冒頭）。実機でこれが通ることは、パス解決とファイル完全性の同時証明になる。

---

## 3. 検証規約（実機ゲート）

### 3.1 「クラッシュしなかった」は計測ではない

**F-3 で最も高価だった教訓をここに適用する。**

> **A-1-f の教訓**: モデルのロードに失敗すると `Unavailable` が返り、アームは静かに「何も生まれない系列」へ退化する。**それでも digest は一致する。**

実機で同じ形が起きる。**モデルのロードに失敗したアプリは、正常に動作したアプリと外形上区別できない。** どちらも落ちない。フレーバは既定動作（固定テンプレート）に落ち、UI はエラーを出さない —— **それは SPEC §6 の設計どおりの振る舞いである。**

**したがって各ゲートは「起きたこと」を肯定的に表明しなければならない。**

### 3.2 G-0 — まずログが見えることを確かめる（最初のゲート）

**LAW-09 の由来**: rustls の CryptoProvider 未登録による起動即 SIGABRT は、**panic メッセージが os_log の Info レベルに沈んで見えなかった**ため診断が遅れた。

**Tier 3 の証跡の全てはログに依存する。したがって最初に検証すべきは、ログが読めることそのものである。**

- 既知の文字列を意図的に出力させ、**実機から確実に回収できる経路を確立する**
- `eprintln!` / `log::error!` / `log::info!` の**どのレベルが実際に見えるか**を実測して記録する
- 回収手段（Xcode console / `xcrun devicectl` / Console.app）を実名で記録する

> **D-26 と同型のゲートである。** 検出器が動いていることを、検出対象を測る前に確かめる。**ログが見えないまま G-1 以降を回すと、すべての結果が「たぶん動いた」になる。**

### 3.3 各ゲートと肯定的表明

| ゲート | 検証対象 | **肯定的に表明すべきこと**（「エラーが無い」では不十分） |
|---|---|---|
| **G-0** | ログ回収 | 既知文字列が実機ログに現れる。**可視なログレベルを実名で記録** |
| **G-1** | U-1（バンドル解決） | `resolve_loadable_model_path` が返した**実パスをログに出す**。`validate_gguf_file` 通過。**バンドル由来であること**（AppData フォールバックでないこと） |
| **G-2** | U-2（メモリ／CPU） | **`339 tensors` は Release では観測できない**（llama.cpp の C++ ログが消えるため）—— モデル同一性は `model.*`（§11.1）で判定する。footprint は `phase.{baseline,model_loaded,ctx_created,inference,idle}` の OSLog 値で記録する（**`phys_footprint_bytes=` という文字列は出力されない**）。`FootprintBand` の到達段 |
| **G-3** | U-3（Metal） | **`offloaded 29/29 layers to GPU` も Release では観測できない**（§11.5）—— 代替はプリフィル時間差（Metal 約 1.02 秒 / CPU 約 34.8 秒 ≒ **34 倍**）による強い間接証拠。生成が**トークンを返す**。**出力が壊れていないこと**（§3.4・**指揮官の目視**） |
| **G-4** | フレーバ E2E | `attempts > 0` ∧ 表示スロットへ到達 ∧ **`leaked == 0`** |
| **G-5** | U-4（取込） | §5.1 の手順で取込 UI へ到達し、**実プロバイダ（iCloud 等）から**コピー成立。3 地点 SHA-256 一致 |

### 3.4 G-3 の特殊な難しさ — 壊れた logits は例外を出さない

**コードコメントが「corrupted logits」を懸念している。** 壊れた logits は panic せず、**もっともらしくない日本語**として現れる。そして**フレーバ層はそれをガードに掛けるので、数値が混じっていれば破棄される** —— つまり**破棄率の上昇として現れうる。**

**提案**: G-3 では**フレーバ層ではなく、破棄率計測ハーネス（`--measure-discard`）に相当する経路**で生の出力を採り、**人間が読んで日本語として成立しているかを確認する。** これは自動判定できない。**指揮官または監査役による目視を検証手続きに含める。**

> **ここは F-3 と性質が違う。** F-3 のガードは「数値が混じっていないこと」を機械的に判定できた。**「意味のある日本語であること」は判定器が存在しない。** LAW-20 の精神に従い、**決定論的観測器が無いことを認め、目視であると明記する。** 自動化したふりをするな。

### 3.5 母集団と再現性

**Tier 3 は「1 回成功した」で解除してよい**（§5.1 が「実機 1 回」と定めている）。ただし:

- **成功の証跡は生ログとして保存せよ。** 数値の要約だけでは、後から「何を見て解除したか」が検証できない
- **デバイス実体を記録せよ** —— 機種・iOS バージョン・空きストレージ・**ロード時の熱状態**。Jetsam 上限と Metal の挙動は機種依存である
- **1 機種の成功は 1 機種の成功である。** 最小サポート機種で測っていないなら、そう書け

---

## 4. 特務機関（gptsol）への依頼事項 — タスク候補

**いずれも「深い事前調査が必要で、局所的に閉じた技術パズル」である。** 監査役が推測で埋めるべきでないと判断した箇所。

### Q-1（最優先）— mmap されたバンドルリソースは Jetsam にどう計上されるか

> iOS において、アプリバンドル内のファイルを `mmap` した場合、そのページは `phys_footprint`（`proc_pid_rusage` / `TASK_VM_INFO`）および Jetsam の判定にどう計上されるか。ファイルバックの clean ページは、匿名 dirty ページと同等に扱われるか。llama.cpp の既定 mmap 経路（`use_mmap=true`）で 1.1GB のモデルをロードした場合、`os_proc_available_memory()` はどう変化するか。

**なぜ重要か**: **この答え一つで、1.1GB が Jetsam 予算に乗るか乗らないかが決まる。** Tier 3 の成否見通しが根本的に変わり、`increased-memory-limit` が必要かの判断もこれに依存する。**推測で進めれば、失敗したときに原因を誤診する。**

### Q-2 — `com.apple.developer.kernel.increased-memory-limit` の実際の要件と効果

> 当該 entitlement の付与に必要な条件（provisioning profile / 開発者アカウント種別 / App Store 審査上の扱い）。機種クラス別の実際の上限変化。`Extended Virtual Addressing` 等の関連 entitlement との関係。無料/個人開発者アカウントで実機デバッグ時に使えるか。

**なぜ重要か**: 現在 entitlements は空である。**必要になったときに「付けられない」と分かるのが最悪のタイミング**であり、事前に判明していれば設計（量子化レベル・`n_ctx`）の選択肢が残る。

### Q-3 — Tauri iOS における 1GB 級バンドルリソースの実際の扱い

> `tauri.conf.json` の `bundle.resources` に 1GB 超のファイルを置いたとき、iOS の実機ビルド（`tauri ios build` / Xcode archive）で (a) `.app` に正しくコピーされるか、(b) `codesign` が問題なく扱えるか、(c) `path().resolve(..., BaseDirectory::Resource)` が実機で返すパスはシミュレータと同一形か。IPA サイズ・App Store 配信の実際の上限（uncompressed slice / cellular download）。

**なぜ重要か**: G-1 の前提そのもの。**シミュレータで動いたパス解決が実機で同じとは限らない**（§5.1 は sim のパスのみ記録している）。

### Q-4 — llama-cpp-2 / llama.cpp の iOS 実機 Metal 経路

> `llama-cpp-2` 0.1.151（llama.cpp 相当版）を `aarch64-apple-ios`（実機）向けにビルドしたときの Metal シェーダ供給方式（`ggml-metal.metal` の埋め込み / `default.metallib` 生成 / 実行時コンパイル）。`n_gpu_layers=999` を指定した場合の `MTLDevice.recommendedMaxWorkingSetSize` に対する挙動 —— 超過時に失敗するか黙って CPU に落ちるか。iOS 実機固有の既知の不具合。

**なぜ重要か**: **G-3 の判定基準に直結する。** 「黙って CPU に落ちる」実装なら、`offloaded 0/29` を見て初めて分かる —— そして U-3 は未検証のまま「成功」に見える。**A-1-f と同型の退化がここにある。**

### Q-5 — 実機からの Rust ログ回収（G-0 の前提）

> `aarch64-apple-ios` 実機で、Rust の `eprintln!` / `stderr` / `log` crate 出力が os_log のどのサブシステム・レベルに落ちるか。Xcode を介さずに（`xcrun devicectl` 等で）確実に回収する手順。Release ビルドで消えるか。

**なぜ重要か**: **LAW-09 の再演を防ぐ。** panic が Info レベルに沈んで数日を失った前例がある。Tier 3 の証跡はすべてログ依存であり、**ログが見えないことは検証不能を意味する。**

### Q-6（優先度低・条件付き）— Metal 経路の出力健全性を機械判定する方法

> 量子化モデルの Metal 推論が「壊れた logits」を出しているかを、目視以外で検出する実務的な方法（同一プロンプト・同一 seed で CPU と Metal の logits を比較する等）。オフライン・単一デバイスで実施可能な形。

**なぜ重要か**: §3.4 のとおり、現状これは目視判定である。**機械判定が可能なら Tier 3 の証跡が一段強くなる。** ただし**存在しないなら「存在しない」と結論して目視で進む** —— 無理に自動化したふりをしないこと。

---

## 5. 禁止事項（本ドラフトの段階で既に確定しているもの）

- **アプリ内 HTTP / ストリームで GGUF を取得するコードの追加**（第一律 / §5.1 不変条件）
- **`effective_n_gpu_layers` の実機側を恒久的に 0 にすること** —— U-3 が永久に未検証になる。検証用の一時上書きのみ
- **`n_ctx` 拡大によるメモリ問題の「解決」**（第三律-4 —— Jetsam への前借り）
- **ObjC コールバック内での Mutex 取得・モデル Drop**（§1.1-1 / 第三律-1）
- **`--features pocket-brain,secure-vault` を付けないビルドでのロード失敗調査**（第三律-3 —— まず feature を疑え）
- **エラー文言へのパス・例外原文の露出**（§5.1 不変条件）
- **「クラッシュしなかった」を成功として報告すること**
- **目視判定を自動判定として報告すること**

---

## 6. 指揮官裁定（2026-07-29・確定）

| # | 論点 | 裁定 |
|---|---|---|
| 1 | gptsol 発注 | **Q-1 と Q-4 を最優先。この 2 件の回答を得るまで G-2 / G-3 の実装に着工しない** |
| 2 | 検証機種 | **ベースライン = RAM 4GB 機（iPhone 13 / A15 Bionic 相当）。** ここでの OOM 回避が第一関門 |
| 3 | `increased-memory-limit` | **付与なしで G-2 を回す。** OOM が発生し、**かつ Jetsam の仕様上不可避と証明された場合にのみ**付与を許可 |
| 4 | U-2/U-3 分離の上書き | **環境変数**（例 `CORAXIS_FORCE_CPU=1`）。**Xcode スキームから注入**する。ソースへのハードコード禁止 |
| 5 | G-3 の目視判定 | **指揮官が担当。** 自動判定するふりはしない（LAW-20） |
| 6 | バンドル戦略 | **現行の直接バンドルを維持。** On-Demand Resources による遅延ロードは第一律（完全オフライン）を侵す危険により**却下** |

---

## 6.1 追加実測 — コードが Q-1 の答えを「断言」している（未検証）

起草後の追加調査で、`llm/params.rs` に次の doc コメントを発見した。

```rust
pub struct LoadParams {
    /// Layers to offload to Metal. 999 = all.
    pub n_gpu_layers: u32,
    /// Keep weights as clean, file-backed pages (not counted against jetsam dirty).
    pub use_mmap: bool,
}
```

> **「not counted against jetsam dirty」—— これは Q-1 の答えそのものを、事実として書いている。そして裏付ける実測はどこにも存在しない。**

`use_mmap: true` は production の全経路（`commands_consult.rs:93` / `rag/commands_rag.rs:916` / `PocketBrainPanel.tsx:77` / `useForegroundRestore.ts:70` / `flavor_a1.rs:302`）で設定されており、**iOS のメモリ戦略全体がこの一文の正しさに乗っている。**

**これは本プロジェクトが繰り返し見つけてきた形である** —— 呼出元ゼロの `const fn`、CI が一度も実行しない doctest、そして**測っていない事実を書いたコメント**。

**Q-1 の位置づけを格上げする**: 未知の解明ではなく、**既に前提とされている命題の検証**である。**偽であれば、iOS のメモリ設計を見直す必要がある。**

---

## 7. 本ドラフトの立場

**F-3 では、モデルが一行も書かないうちに 7 種類の「緑なのに何も守っていない」を見つけられた。型と走査で先に塞げたからである。**

**Tier 3 にはそれがない。Jetsam に型はない。**

だからこそ、Tier 3 で守るべき規律は F-3 と同じではなく、**より単純で、より厳しい**:

> **測った数字だけを書く。測っていないものは「測っていない」と書く。**

「実機で動いた」と書けるのは、**バンドル由来のパスが解決され、339 テンソルがロードされ、29/29 層が Metal に乗り、生成がトークンを返し、footprint が数値として記録され、そのログが手元にある**ときだけである。

**そのどれか一つでも欠けたら、欠けたと書く。**

---

## 8. gptsol 回答の監査結果（2026-07-29・固定版ソースで検証）

**Q-1 / Q-4 の回答を受領し、監査役が `llama-cpp-2` / `llama-cpp-sys-2` **0.1.151 の実体**に対して検証した。** gptsol は llama-cpp-rs `7f0a0d9` / llama.cpp `9e3b928` を典拠として挙げたが、**我々の固定版のソースと一致していた。**

### 8.1 検証できた主張

| 主張 | 我々の固定版での確認 |
|---|---|
| no-copy 経路（`buffer_from_host_ptr` → `newBufferWithBytesNoCopy`） | **確認**。`llama-model.cpp:1500-1515` に `buffer_from_host_ptr_supported` と条件 `ml.use_mmap && use_mmap_buffer && buffer_from_host_ptr_supported && is_default_buft` |
| **重み用の第二の 1.04 GiB backing は作られない** | **確認**（上記経路の帰結） |
| 埋め込み MSL を実行時コンパイル・`default.metallib` 不要 | **確認**。`ggml-metal-device.m:125` に `"using embedded metal library"`、`:233 / :288` に `newLibraryWithSource:` |
| 部分 CPU fallback は存在しない | **整合**（`recommendedMaxWorkingSetSize` に応じて層数を減らすコードは無い） |
| ログ書式 3 行 | **確認**。`llama-model.cpp:1577`（`offloading output layer to GPU`）/ `:1580`（`offloading %d repeating layers to GPU`・引数は `n_repeating`）/ `:1585`（`offloaded %d/%d layers to GPU`） |
| `n_gpu_layers=0` では Metal backend が起きる | **確認**。`llama-context.cpp:257-265`（§1.2 の訂正を参照） |
| `with_devices` / デバイス列挙 API が使える | **確認**。`params.rs:515` / `lib.rs:499` |
| Q4_K_M の内訳 169+29+141 | **整合** —— 合計 339 が我々の実測テンソル数と一致 |

> **ログ書式は確認したが、中央行の値（gptsol の予測は `27`）は未確認である。** `n_repeating` の実値はモデル依存であり、**実機ログで確定する。ゲートの grep を `28` 前提で書くな。**

### 8.2 最重要の裁定 — 我々のコメントは半分正しく、半分未検証だった

> **no-copy は確認済み。だが「Jetsam に計上されない」は未確認。**

gptsol は XNU の `phys_footprint` が `iokit_mapped` を含み、IOKit mapping は backing が clean/external かに関わらず総量を計上すると指摘した。**すなわち no-copy であることは、課金されないことを意味しない。**

**`params.rs` のコメント「not counted against jetsam dirty」は、Metal を使う経路については依然として未証明である。** 保守的には **1.04 GiB が Metal 使用時に課金される前提で予算を組む**べきであり、覆せるのは実機差分のみ。

**`use_mmap: true` は維持する。** 二重 backing を避け、CPU-only 時の clean/reclaimable 性質を保つため。`use_mmap: false` にすると匿名 dirty となり **約 1.04 GiB の footprint 増加が期待値**である（改悪）。

### 8.3 決定的な計測 — **B − A の差分**

gptsol の提案する 4 variant を採用する。**各 variant は別の cold launch で実行する。**

| Variant | `use_mmap` | devices / layers | context offload |
|---|---|---|---|
| **A** | true | `with_devices(&[])`, GPU 0 | KQV false / op false |
| **B** | true | Metal 明示, 999 | true / true |
| C | false | `with_devices(&[])`, GPU 0 | false / false |
| D | false | Metal 明示, 999 | true / true |

**判定**:

- **A** で `external`/`resident` が約 +1.04 GiB ∧ `phys_footprint` ほぼ不変 → raw mmap の clean 会計を確認
- **B − A** が約 1.04 GiB 動く → **copy ではないが Metal/IOKit が alias 全量を課金**。コメントは偽
- **B − A** が小さい → 当該 OS / 機種ではコメントの実用的前提が成立
- **C − A** で `internal`/`footprint` が約 +1.04 GiB → `use_mmap=false` の予想どおり

> **`MTLDevice.currentAllocatedSize` の 1.04 GiB 増加は copy の証明にならない**（Metal の論理リソース会計であるため）。**この罠を踏むな。**

計測点: open 前 / model load 後 / context 作成後 / 最初の prefill 後 / 128-token decode 中 / 圧力・再アクセス後 / unload 後。

記録項目: `TASK_VM_INFO`（`phys_footprint` `resident_size` `internal` `external` `compressed` `limit_bytes_remaining`）/ `proc_pid_rusage`（`ri_phys_footprint` `ri_pageins`）/ `os_proc_available_memory()` / `mincore()` による GGUF mapping の resident pages / Metal（`maxBufferLength` `recommendedMaxWorkingSetSize` `currentAllocatedSize` no-copy view 数）/ 全 llama.cpp ログ。

### 8.4 G-3 の判定基準を強化する

**`offloaded 29/29` は「重み配置の計画」の指標であって、Metal で演算されたことの証明ではない。** 未対応 tensor/op は CPU に置かれ得るし、shader/context 初期化はその後に失敗し得る。

**G-3 の成功ゲートは以下を要求する**:

```
ggml_metal_library_init: using embedded metal library
ggml_metal_device_init: has unified memory    = true
ggml_metal_device_init: use shared buffers    = true
load_tensors: offloaded 29/29 layers to GPU
```

∧ コンテキスト作成成功 ∧ **prefill と batch-1 decode の双方で有限 logits**（NaN/Inf は即 fail）。

さらに debug 実行で `GGML_SCHED_DEBUG=1` の `## SPLIT ... MTL0`、最終権威として **Instruments Metal System Trace / GPU Capture**（固定版に `ggml_backend_metal_capture_next_compute()` が在り `/tmp/perf-metal-<pid>.gputrace` を生成できる）。

### 8.5 自動 logits 判定は実装可能（§3.4 の訂正）

**§3.4 は「判定器が存在しない」と書いたが、これは誤りである。** gptsol の提案する CPU/Metal logits 比較は、**単一オフライン実機で実装可能**である（`with_devices` が固定版に在ることを確認した）。

手続き: 同一の固定 token ID 列を teacher-force し、sampling を通さず **full-vocab F32 logits** を prefill（batch ≥ 32）と decode（batch 1）の複数地点で比較。NaN/Inf は即 fail。NMSE/RMSE・max absolute error・cosine similarity・top-1・top-k overlap・KL/JS を記録。**2 モデルを同時に保持せず、CPU を unload してから Metal をロードする。**

> **seed を揃えた生成文字列の比較は判定器にならない** —— 微小差が sampling 分岐を生むため。
>
> **whole-model に普遍的な閾値は存在しない。** upstream の backend-op test の NMSE `1e-7` を whole-model 閾値として盲用してはならない。CPU/CPU・Metal/Metal の反復揺らぎから校正せよ。
>
> **したがって「機械判定が可能」＝「目視が不要」ではない。** 指揮官の目視判定（裁定 5）は維持する。機械判定は**それを補強する第二の証跡**として位置づける。

### 8.6 Jetsam 実測の作法

**実際の Jetsam pass は Xcode / debugger から切り離した Release / ad hoc / TestFlight で実施し、`.ips` の reason を確認する。**

- **`per-process-limit`** → `increased-memory-limit` の検討対象（指揮官裁定 3 の「不可避と証明された場合」がこれに当たる）
- **`vm-pageshortage` / `fc-thrashing`** → **entitlement では根治しない**。全体圧力型であり、設計側の対処が必要

> **裁定 3 の判断根拠は、この `.ips` の reason 文字列である。** 「OOM した」だけでは entitlement の可否を決められない。

### 8.7 コメント修正の要件（実装タスクとして残す）

gptsol の指摘どおり、次の 2 箇所は**測っていないことを断言している**。修正を Tier 3 実装時の要件とする。

```rust
// params.rs — 現在（断言している）
/// Keep weights as clean, file-backed pages (not counted against jetsam dirty).

// 推奨（測っていないことを書かない）
/// Request llama.cpp's read-only mmap-backed weight path.
/// Clean file-backed pages normally stay outside task phys_footprint, but
/// Metal/IOKit accounting and residency must be verified on each device class.
```

```rust
// service.rs — 現在（因果を断言している）
/// The Simulator exposes a synthetic Metal device without the unified/shared
/// memory capabilities llama.cpp expects for reliable quantized inference.

// 推奨（未適格性確認であることを書く）
/// Simulator Metal output has not been qualified against the CPU reference.
/// Force CPU in Simulator; physical-device Metal is validated separately.
```

> **後者の因果関係（合成 Metal が量子化 logits を壊す）は、Apple / upstream の一次資料で確認できなかった。** 方針（Simulator を CPU に落とす）は安全側であり維持するが、**理由を断言している記述は弱めるべきである。**

### 8.8 残った未知

**gptsol が「不明」と明示した唯一の点**:

> **mmap を Metal からアクセスした場合、いつ・どの量だけ IOKit ledger に入るか。**

Apple の公開資料に記述が無く、**§8.3 の B − A 差分が唯一の確定手段である。**

**4GB の iPhone 13 で 1.04 GiB が Jetsam 課金されるかは、実測でしか分からない。** それが Tier 3 の中心的な問いとして残った。

---

## 9. 第 2 弾（Q-3 / Q-5 / Q-2 / Q-6）の監査結果 — **着工前に第 0 フェーズが必要**

**gptsol の第 2 弾により、実機着工を止めるべき事項が 2 件判明した。どちらも監査役が実測で確認した。**

### 9.1 【ブロッカー 1】Release ではログが全滅する — G-0 が成立しない

**連鎖を実測で確認した。**

| 環 | 確認 |
|---|---|
| `#[cfg_attr(mobile, tauri::mobile_entry_point)]` | **`lib.rs:155` に実在** |
| マクロが `run()` 前に `tauri::log_stdout()` を呼び fd 1/2 を pipe へ奪う | gptsol（Tauri 2.11.5 `Logger.swift` / `tauri-macros` mobile entry） |
| 我々の `StderrLogger` が **全 `log::` を `eprintln!` へ変換** | **`lib.rs:109-135` で確認** |
| Swift `Logger.enabled` の **Release 既定が `false`** → 破棄 | gptsol |

そして `lib.rs:120` のコメントはこう書いている:

> `idevicesyslog -p Coraxis` **already proven (this investigation) to capture** this process's raw stdout/stderr

**その実績は debug ビルドのものである。**

> **これは Tier 2 → Tier 3 とまったく同じ形である。** ある構成で検証したことを、別の構成でも成り立つと仮定している。**しかも今回は最も証跡が要る場面で起きている** —— Jetsam 実測はデバッガから切り離した Release でしか取れないのに、**そこでログが消える。**

**`method: debugging` は export / 署名の方法であって、Swift の `#if DEBUG` を有効にするものではない。**

**帰結**: **G-0 は現状の実装では Release で必ず落ちる。logger の改修が実機着工の前提である。**

### 9.2 【ブロッカー 2】Xcode 直ビルドは Tauri のリソース同期を迂回する

**gptsol が Q-3-c を確定させた**（Tauri CLI 2.11.4 / commit `8909f221…`）:

- `ios build` / `ios dev` は Xcode 処理の前に **`inject_resources` を一度呼ぶ**
- 最終処理は **`std::fs::copy` であり、存在・mtime・サイズ・内容による skip 分岐は無い**
- **したがって CLI 起動なら、ステージは毎回ソースの内容へ戻る**
- **しかし `tauri ios xcode-script` は Rust をビルドするだけで `inject_resources` を呼ばない**

**これが Jul 25 の IPA が正常で Jul 29 の `.xcarchive` がスタブだった理由である。**

**監査役の実測（除去前）**:

| 対象 | サイズ | SHA-256 |
|---|---:|---|
| ソース | 1,117,320,736 | `6a1a2eb6…9407e` |
| ステージ（`gen/apple/assets/models/`） | **8** | `af5570f5…`（8 ゼロバイトの SHA と一致） |
| **Jul 29 `.xcarchive`** | **8** | 同上 |
| Jul 25 `build/Payload/` | 1,117,320,736 | `6a1a2eb6…9407e` |

**指揮官裁定により、汚染された 2 者は削除済み**（`gen/apple/assets/models/` と `pkb-desktop_iOS.xcarchive`。いずれも `.gitignore` の `*.gguf` 配下で git は未汚染）。

**掟**: **Xcode 直接の Build / Archive を原則禁止する。** そのうえで **source → stage → archive の 3 点で SHA を止める**（§9.5）。**mtime を出荷ゲートに使うな** —— Xcode の `CpResource` は内容 SHA を依存判定に使わず、サイズ・mtime が同じなら内容が変わっても skip し得る。

### 9.3 entitlement は決定的な梃子にならない — **+250 MiB のみ**

gptsol が Apple 公式 IPSW の DeviceTree から `kern.max_task_pmem` / `kern.entitled_max_task_pmem` を抽出した。**iPhone 13 = `iPhone14,5` / `D17AP`**:

| iOS / build | 通常 | entitlement 付き |
|---|---:|---:|
| 17.5.1 / 21F90 | **2098 MiB** | **2348 MiB** |
| 18.4 / 22E240 | 2098 MiB | 2348 MiB |
| 26.6 / 23G71 | 2098 MiB | 2348 MiB |

**増分 250 MiB（+11.9%）のみ。**

> **裁定 3 の判断は、実質これで決まる。** 1.04 GiB のモデルに対し素の上限は 2098 MiB —— 残り約 1 GiB が Metal・WKWebView・KV・その他の取り分である。**entitlement が買えるのは 250 MiB にすぎない。**
>
> **足りないなら設計側で対処するしかない**（`n_batch` / `n_ubatch` 縮小、初期化の直列化、CPU/Metal コンテキストの同時生存排除、最後に量子化）。**`n_ctx` は縮小方向のみ許容**（拡大は Jetsam への前借りとして社内規約が禁じる）。

**`extended-virtual-addressing` は物理メモリも Jetsam 上限も増やさない。** しかも現行 XNU では Increased Memory Limit 自体が jumbo VA の条件に含まれるため、**重ねる実益は通常ない。** 検討条件は `mmap == MAP_FAILED` / `ENOMEM` / `KERN_NO_SPACE` / VA 断片化を観測した場合のみ。

**実行時判断には固定値でなく `os_proc_available_memory()` を使う。**

### 9.4 App Store — 現構成は上限内

- **uncompressed app size 上限 4 GB**、Mach-O の全 `__TEXT` 合計 500 MB。**現在値（主実行体 129,351,824 B ＋ モデル 1,117,320,736 B）は上限内**
- **cellular の 200 MB は警告であって提出拒否ではない。** 全 variant が超えるのは想定内。初回インストールは Wi-Fi 推奨と案内する
- **App Thinning では除外されない** —— device idiom / scale / asset catalog / ODR tag を持たない通常リソースであり、**全 device variant にそのまま含まれる**
- Bitcode は Xcode 14 以降 App Store が受理せず、本件の最適化要素ではない
- **正本のサイズは `.app` / `.xcarchive` / IPA の単純サイズではない。** Ad Hoc export の `App Thinning Size Report.txt`、最終的には App Store Connect の `App File Sizes`

### 9.5 【新設】第 0 フェーズ — 計器を直してから実機へ

**G-0 の前に、G-0 を成立させるための工事が要る。**

| タスク | 内容 |
|---|---|
| **P0-1** | **Rust ログを native OSLog へ直結**（Tauri の pipe 経由をやめる）。必須証跡は **Default 以上**（Info / Debug は Apple が十分に永続化しない）。数値は **`%{public}llu` 等の固定書式**で出す |
| **P0-2** | **3 点 SHA ゲート** —— source / stage / archive。`expected_size` と `expected_sha` を定数化し、シェルで止める。**mtime を使うな** |
| **P0-3** | **§5.1 手順 6 の改訂** —— 「本物を復元し SHA 一致を確認」の対象を **source だけでなく stage と archive にも広げる**。今回の残留はこの穴が生んだ |
| **P0-4** | 回収手順の dry-run —— `log collect` / `devicectl` の `systemCrashLogs` |

**プライバシー検閲の罠**: os_log は動的文字列を既定で `<private>` に潰す。**footprint のバイト数が潰れると計測不能になる。** 一方、現行の Tauri redirector は record 全体を `%{public}@` で出すため、**有効化すると panic 原文もパスも丸ごと public になる** —— これは §5.1 の不変条件（エラー文言にパス・例外原文を出すな）に反する。**native shim では「数値は固定書式の public、任意文字列は流さない」を守る。**

### 9.6 Q-6 の追加見解 — 閾値は「不明」と明示された

**gptsol は Q4_K_M・A15・固定版に対する CPU↔Metal logits の公開数値範囲を「存在しない」と明示した。** そのうえで校正手順を提示している:

```
W = CPU/CPU と Metal/Metal の最大反復距離
D = calibration corpus の正常 CPU↔Metal 距離群
T = max(D) + max( 0.25·max(D), 6·MAD(D), 3·W )
```

指標は単一ではなく **① NaN/Inf hard gate ② centered NMSE または centered cosine ③ JS divergence ④ margin-aware top-k / top-1** の組合せ。**upstream 演算子テストの `1e-7` を whole-model 出荷閾値へ流用してはならない**（第 1 弾から一貫）。

API は固定版に実在を確認済み —— `get_logits`（`context.rs:237`）/ `get_logits_ith`（`:280`）/ `n_vocab`（`model.rs:568`）。**`get_logits_ith` の `i` は元の batch token offset で、この wrapper は負値を扱えない**（C API の `-1` は使えない）。

**Q-6-d が重要**: logits 比較では**検出できない**破損様式が存在する（CPU/Metal 共通の loader/graph バグ、tokenizer / chat template、sampling パラメータ、stop / UTF-8 streaming、corpus 外の rare path、checkpoint 後の破損、near-tie の増幅）。**したがって目視判定（裁定 5）は維持する。** 機械判定は第二の証跡である。

---

## 10. 実機実測（2026-07-30）— **Q-1 に答えが出た**

**デバイス**: iPhone 17 Pro (iPhone18,1) / `Gggzns` / **Release ビルド・デタッチ起動**
**ビルド**: `9a7d4b1` + P0-5、`tauri ios build --features pocket-brain,secure-vault`
**モデル**: 1,117,320,736 バイト（**1,065.6 MiB**）/ sha256 `6a1a2eb6…9407e`（3 点 SHA ゲート OK）

### 10.1 G-0R — 計器は Release でも生きている（**GREEN**）

```
22:57:21.538 Df Coraxis[36716] [com.ai-shizu.pkb:app]    instrument.alive=2130600
22:57:22.687 Df Coraxis[36716] [com.ai-shizu.pkb:memory] phase.model_loaded=78923608
```

| 命題 | 証拠 |
|---|---|
| Release でも OSLog シムが届く | 両行が出た |
| プライバシー検閲を突破 | 数値が読める（`<private>` ではない） |
| Default 型で永続化 | `Df` プレフィクス |
| **P0-1 の仮説が正しかった** | **`[stderr]` 系が完全に消え、シム経由だけが残った** |

**debug では `[stderr] llama_model_loader…` が見え、Release では消え、シムは両方で生きている。** 対照実験として完結している —— **Tauri の Swift Logger の死角は実在し、OSLog 直結がそれを埋めた。**

### 10.2 フェーズ別 footprint（2 サイクルで再現）

`set_phase(Inference)` は **`ctx.decode(&mut batch)` の直後**（`service.rs:1508`）。**プリフィルが完了し、重みが実際に読まれた後**の測定値である。

| フェーズ | footprint | 増分 |
|---|---:|---:|
| `instrument.alive`（プロセス起動時） | **2.0 MiB** | — |
| `phase.baseline`（purge 後） | **48.9 MiB** | アプリ本体＋WebView |
| **`phase.model_loaded`** | **76.5 MiB** | **+27.6 MiB** |
| **`phase.ctx_created`** | **130.0 MiB** | **+53.5 MiB** |
| **`phase.inference`**（decode 完了後） | **159.9 MiB** | **+29.9 MiB** |
| `phase.idle`（生成完了後） | **163.2 MiB** | +3.3 MiB |

生ログ（2 サイクル目）:

```
23:07:02.839 phase.model_loaded=80201896
23:07:07.226 phase.ctx_created=136251608
23:07:08.271 phase.inference=167676168
23:07:08.732 phase.idle=171084040
```

### 10.3 **Q-1 の回答 — 重みは `phys_footprint` に計上されない**

> **1,065.6 MiB のモデルを読み切った後で、プロセス全体の footprint は 160 MiB。**

**モデルロードが footprint に乗せたのは +27.6 MiB —— モデルサイズの 2.6% である。** ページテーブル・`MTLBuffer` オブジェクト・メタデータ・非 mmap 部分として説明がつく量であり、**1 GiB のコピーでも 1 GiB の課金でもない。**

**`params.rs` の「Keep weights as clean, file-backed pages (not counted against jetsam dirty)」は、この機種・この構成において正しかった。** gptsol が「Apple の公開資料に記述が無く不明」とした一点に、実測が答えを出した。

**§6.1 で「未検証の断言」として指摘したコメントは、これをもって裏付けられた。** ただし**コメントの書き方は改めるべきである**（§8.7 の推奨文）—— 正しい命題であっても、**測る前に事実として書いてはならない**という手続きの問題は残る。

### 10.4 予想外の発見 — **主戦場は重みではなく KV / compute バッファ**

**コンテキスト生成（+53.5 MiB）が、モデルロード（+27.6 MiB）より高い。**

**したがって Tier 3 のメモリ予算を支配するのは重みではない。** gptsol が縮小の優先順位で `n_batch` / `n_ubatch` / `n_ctx` を量子化より上位に置いたのは正しく、**量子化を下げる必要は当面ない。**

4GB 機（iPhone 13・`kern.max_task_pmem` = 2098 MiB）へ外挿すると、**160 MiB は上限の約 8%。** entitlement の +250 MiB を要する場面には遠い。

### 10.5 まだ言えないこと

1. **この測定が Metal 経路のものである証拠が無い。** Release では llama.cpp の C++ ログが消えるため、**`offloaded 29/29 layers to GPU` を観測していない。** 実機では `effective_n_gpu_layers` が 999 を返す**はず**だが、観測していない以上「はず」である。**これは B − A 差分が答えるべき問いそのもの**
2. **測定機は iPhone 17 Pro であり、ベースラインの iPhone 13（4GB）ではない。** footprint の会計はカーネルの挙動なので「重みが課金されない」性質は機種に依らないはずだが、**絶対値と Jetsam 上限は機種ごとに異なる**

### 10.6 実験計画の更新（指揮官裁定・2026-07-30）

**§8.3 の 4 variant のうち C / D（`use_mmap=false`）はスキップする。** 重みが課金されないことが実測で判明した以上、`use_mmap=false` は**約 1 GiB の悪化が確実**であり、対照としての価値しか無い。

**残すのは A と B の分離のみ:**

| アーム | 構成 | 測るもの |
|---|---|---|
| **A** | `with_devices(&[])` ＋ `n_gpu_layers(0)` ＋ `offload_kqv(false)` ＋ `op_offload(false)` | Metal 抜きの footprint |
| **B** | 現行（Metal 999） | 実測済み: **160 MiB** |

**B − A が小さければ「Metal/IOKit も課金していない」、大きければ「Metal 経由で課金される」** —— 一問一答になる。

**A の実装は P0-6 の射程である**（`LoadParams` にデバイス選択の口が無く、現状では実行できない）。

---

## 11. G-1 / G-2 実測（2026-07-30〜31）— **B − A 差分が Q-1 を確定させた**

**デバイス**: iPhone 17 Pro / `Gggzns` / **Release ビルド・`devicectl` からのデタッチ起動**
**ビルド**: `2e3c085`（P0-6）/ 4 検査（新鮮さ・dev URL 0 件・プロファイル有効・3 点 SHA）通過済み

### 11.1 G-1 — Release でのモデル同一性（**GREEN**）

Release では llama.cpp の C++ ログが消えるため、`339 tensors` は観測できない。**P0-6 の `model.*` がその穴を埋めた。**

| ラベル | 実測 | 権威値との照合 |
|---|---:|---|
| `model.origin` | **1** | **バンドル由来**（2 なら AppData フォールバック＝失敗） |
| `model.n_layer` | **28** | GGUF `qwen2.block_count` = 28 ✓ |
| `model.meta_count` | **23** | GGUF KV 26 − 配列型 3 = 23 ✓ |
| `model.n_params` | 1,777,088,000 | — |
| `model.size` | 1,111,370,240 | ファイル 1,117,320,736 との差 5.7 MiB = ヘッダ＋メタ ✓ |
| `model.force_cpu` | 0 / 1 | **アームの記録**（下記） |

> **期待値の訂正**: 監査役は当初 `n_layer≈29` / `meta_count≈26` と書いたが**両方誤り**。`29` は `block_count + 1`（出力層）、`26` は配列型を含む GGUF ヘッダ値。**GGUF を直接パースして確定させた。** 詳細は `AI_SKILLS` §5.1。

### 11.2 G-2 — **B − A 差分**

A = CPU オラクル（`CORAXIS_FORCE_CPU=1`・4 点セット）、B = 既定 Metal（`n_gpu_layers=999`）。
**A は 2 サイクル（PID 37438）、B は 5 サイクル（PID 37249 / 37387）。**

| 地点 | **A（CPU）** | **B（Metal）** | **B − A** |
|---|---:|---:|---:|
| `model_loaded` | 74.33 MiB | 74.64 MiB | **+0.31 MiB** |
| `ctx_created#2` | 150.38 MiB | 174.98 MiB | **+24.60 MiB** |
| **`inference`** | **201.53 MiB** | **179.27 MiB** | **−22.26 MiB** |

**符号が逆転する。** コンテキスト生成では Metal が 25 MiB 多いが、**推論後は CPU の方が 22 MiB 多い。**

### 11.3 **Q-1 の最終回答**

> **`model_loaded` における B − A は 0.31 MiB。モデルのテンソルは 1,059.9 MiB ある。**
>
> **mmap されたファイルバックの重みは、Metal の有無にかかわらず `phys_footprint` に計上されない。**

gptsol は「no-copy は確認したが IOKit ledger への計上は**不明**。保守的には 1.04 GiB が Metal 使用時に課金される前提で予算を組め」と助言した。**差分実測がそれを否定した。** Metal が足すのは**コンテキスト側の約 25 MiB**であって、alias された重みの全量ではない。

**§6.1 で「未検証の断言」として指摘した `params.rs` のコメントは、Metal 経路まで含めて裏付けられた。** ただし**書き方の是正（§8.7）は維持する** —— 正しい命題であっても、測る前に事実として書いてはならない。

### 11.4 **CPU フォールバックはメモリ緩和策にならない**

プリフィル中の増分が理由を示す。

| アーム | `ctx_created#2` → `inference` | 所要時間 |
|---|---:|---:|
| **A（CPU）** | **+51.15 MiB** | **約 34.8 秒** |
| **B（Metal）** | **+4.29 MiB** | **約 1.02 秒** |

**CPU バックエンドは匿名の compute バッファをプリフィル中に 51 MiB 確保する。Metal は共有バッファ上で計算するため 4 MiB で済む。**

**gptsol が「unified memory なので CPU へ戻せば必ず総 footprint が減るとは限らない」と述べた挙動の実証である。**

> **設計判断**: **4GB 機に対して Metal は速度・メモリの双方で有利。** メモリ不足時に `n_gpu_layers` を下げる対処は**逆効果**であり、縮小すべきは `n_batch` / `n_ubatch` / `n_ctx` である（§10.4 と整合）。

### 11.5 U-3 への強い間接証拠

**プリフィル時間が 34.8 秒 対 1.02 秒 —— 約 34 倍。**

Release では `offloaded 29/29 layers to GPU` を観測できないが、**B が秘かに CPU で走っていたなら 35 秒かかるはずである。1 秒で終わっている以上、Metal が実際に計算している。**

**直接観測ではないが、U-3（Metal が本当に使われているか）に対する極めて強い証拠である。** 残る U-3 の問いは「**出力が壊れていないか**」（G-3・指揮官の目視）だけになった。

### 11.5.1 G-3 —— 目視判定の結果（**GREEN**・2026-07-30〜31）

**判定者**: 指揮官および前任の監査役（Opus）。**実施セッションで直接観測。**

| 観点 | 結果 |
|---|---|
| 日本語として成立しているか | **完全に成立** |
| 文字化け | **無し** |
| 無限ループ / 反復縮退 | **無し** |
| 意味の崩壊 | **無し** |
| A（CPU）との並置比較 | **明らかな破綻は無し** |

⇒ **U-3 は「Metal が計算している」（§11.5 の 34 倍）と「出力が壊れていない」（本項）の両面で満たされた。G-3 GREEN。**

> **これは人間判定であり、そう明記する。** §3.4 のとおり「意味のある日本語であること」の決定論的判定器は存在しない。**自動化したふりをしない**という LAW-20 の方針に従い、判定者・観測内容・時期を記録することをもって証跡とする。**機械判定（Q-6 の logits 比較）は閾値校正が未了であり、本判定を置き換えるものではない。**
>
> **手続き上の記録:** 本項は 2026-07-31 に**遡って**記録された。実施は前セッションだったが、**どのコミットにも証跡が無い状態が一時的に生じた** —— 引き継ぎ書のサマリは GREEN と記していたが、リポジトリ内には根拠が無かった。**§3.5 が「成功の証跡は生ログとして保存せよ。数値の要約だけでは、後から何を見て解除したかが検証できない」と定めているのは、まさにこの穴を塞ぐためである。目視ゲートであっても、観測した事実そのものを同じセッション内で記録せよ。**

### 11.6 ピーク値と Jetsam 余裕

| アーム | ピーク footprint |
|---|---:|
| A（CPU） | 204.1 MiB |
| B（Metal） | 189.7 MiB |

**iPhone 13（4GB・`kern.max_task_pmem` = 2098 MiB）に外挿して上限の 10% 未満。** entitlement の +250 MiB を要する場面には遠い。

### 11.7 手続き上の記録

- **`model.force_cpu` をロード前に出す設計（P0-6）が効いた。** 全 PID でどちらのアームかが記録され、**アームの取り違えが構造的に起きなかった**
- **一度、A アームが `inference` 未到達で終わった**（PID 37353）。クラッシュでも Jetsam でもなく（`systemCrashLogs` に今夜の記録なしを確認）、CPU 推論の遅さ（35 秒）ゆえに完了前に操作されたと見られる。**再実行で解消**
- **`log collect` は出力先が既存だと `File exists (17)` で失敗し、古いアーカイブがそのまま読まれる。** 一度これで「新しいデータが無い」と誤認しかけた。**回収のたびに出力名を変えること**

---

## 12. G-5 実測（2026-07-31）— **Tier 3 全ゲート突破**

**デバイス**: iPhone 17 Pro `Gggzns` / Release / `devicectl` デタッチ起動（PID 39180）
**ビルド**: `0e5db80`（G-5 修正）/ 4 検査（新鮮さ・dev URL 0 件・プロファイル有効・3 点 SHA）通過済み

### 12.1 判定 — **GREEN**

| §3.3 の要求 | 証拠 | 判定 |
|---|---|---|
| 取込 UI へ到達 | 起動時にモデル取込 UI が表示（指揮官確認） | ✅ |
| **実プロバイダから**コピー成立 | `provider=other`（**`tmp` ではない**）＋ **`scope_start` の失敗行が出ない** | ✅ |
| 3 地点 SHA-256 一致 | 端末実ファイルの SHA-256 = `6a1a2eb6…9407e` | ✅ |

```
09:22:39 import.diag step=source_shape detail=provider=other segments=10 ext=gguf len=134
09:22:39 import.step dest_ready
09:22:39 import.step confirm_size bytes=1117320736
09:22:40 model.origin=2  n_layer=28  n_params=1777088000  size=1111370240  meta_count=23  n_vocab=151936
```

### 12.2 **「出ない行」が肯定的証拠になった稀な例**

失敗実行（PID 39029）では `import.diag step=scope_start detail=Failed to start accessing security-scoped resource` が出ていた。成功実行では**その行が無い**。

**`startAccessingSecurityScopedResource` は、真に security-scoped な URL でしか成功しない。** アプリ自身が所有する URL では必ず失敗する —— それを直前の実行が実証している。したがって**沈黙は「アプリサンドボックス外の実プロバイダ由来である」ことを積極的に主張する。** 対照実験として完結している。

**限界:** `provider=other` はマーカー集合が提供元を同定できなかったことを意味する。**security-scoped 由来は証明されたが、「iCloud Drive から」と名指しで書ける証拠ではない。**

### 12.3 `origin=2` が偽 GREEN になり得た経路（回避済み）

**`model.origin=2` は「AppData から読んだ」しか意味しない。** 旧セッションの残留モデルがあれば、**取込を一切行わなくても `2` になる**（§5.1 手順 5 の罠）。`devicectl install` はコンテナを保持したまま上書きするため、これは現実的な危険だった。

**回避の論理**: 取込 UI は `resolve_loadable_model_path` が**バンドルと AppData の両方で失敗**したときにしか出ない。**したがって「取込 UI が先に出たこと」が、残留モデル不在の証明である。** UI 到達を先に確認したことで `origin=2` が結論たり得た。

### 12.4 サイズ一致で終わらせなかった理由

`validate_gguf_file` は**先頭 4 バイトの magic しか見ない**。`ENOSPC` で切り詰められたコピーもこれを通過し、「正常な取込」として受理される。`confirm_size bytes=1117320736` はその穴を塞ぐが、**サイズ一致は内容一致ではない。**

`devicectl device copy from --domain-type appDataContainer` で**端末上の実バイト列を吸い出して SHA-256 を取れる**ことが判明したため、状況証拠で終える理由が消えた。リポジトリ側と端末側が同一ハッシュである以上、**間のプロバイダはバイト列を無改変で運んだ**ことになり、連鎖は端から端まで証明される。

### 12.5 根本原因 — **ピッカーの `fileAccessMode` 既定が `copy`**

詳細は `AI_SKILLS` §5.1 as-built 項目 1。要点のみ:

- `plugin-dialog` の Swift が `asCopy: args.fileAccessMode == .scoped ? false : true` を評価する。**既定 `copy` では iOS が 1.1GB を自アプリ container tmp へ複製し、非 security-scoped な URL を返す**
- その URL への `startAccessingSecurityScopedResource` は**正しく失敗する**。FE がそれを致命扱いしていたため、**コピーに到達する前に取込が死んでいた**
- **`fileAccessMode: "scoped"` の明示**で解決。原本を動かさず、複製も生じない（`copy` 時の複製削除は API 契約上**呼び出し側の責任**であり、誰も消していなかった）

**Tier 2 はこの分岐を踏んでいない。** シミュレータでは成功する経路しか通らず、**「Tier 2 GREEN だから実機も通る」は成り立たなかった。これが Tier 3 を必須にしていた理由そのものである。**

### 12.6 手続き上の記録

- **計器が無ければ 1 回目で原因が分からなかった。** FE の `softImportError` が生エラーを `_raw` で受けて**捨てて**おり、失敗経路には Rust のログも無かった。**「当てずっぽうで直す」前に計器を入れる判断が、往復を 1 回で済ませた**
- **`import.fail` というラベルを 1 箇所だけ書いてしまい、コミット前レビューで発見した。** 回収コマンドの `grep -E 'import\.(step|diag)'` から**漏れる計器**であり、**計器そのものが検出されない**という同型の欠陥だった。ラベルは `import.diag` に統一
- **`xctrace` の `Offline` は CoreDevice の可用性を意味しない**（§5.1 の訂正を参照）。USB 実在は `ioreg -p IOUSB -l` で見よ。**`system_profiler SPUSBDataType` はこの環境で 0 行＝検出器として死んでいる**

### 12.7 Tier 3 の最終状態

| ゲート | 判定 |
|---|---|
| G-0R | **GREEN**（`9b7677c`） |
| G-1 | **GREEN**（`9b7677c`） |
| G-2 | **GREEN**（`9b7677c`） |
| G-3 | **GREEN**（§11.5.1・目視） |
| G-5 | **GREEN**（本節） |

**出荷ブロッカーとしての Tier 3 は解除された。**

> **【訂正・2026-07-31】本節は当初「G-4 は Tier 3 の射程外」と書いた。誤りである。** G-4 は §3.3 のゲート表に載っており、§1.3 の完了基準も「フレーバがガードを通過して表示スロットへ到達し」を含む。**射程内である。** 実測は §13。**自分が書いた完了宣言の中で、自分の表から 1 行を落としていた** —— 本フェーズが一貫して潰してきた欠陥の、最後の変奏である。

---

## 13. G-4 実測（2026-07-31）— **フレーバ E2E。Tier 3 全ゲート完了**

**デバイス**: iPhone 17 Pro `Gggzns` / Release / `devicectl` デタッチ起動（PID 42102）
**ビルド**: `82ceead` / `--features pocket-brain,secure-vault,flavor-live`
**操作**: INTERVIEW → `[ GD ]` → BlackboxArena でキャンペーン開始・ターンを 4 回 advance

### 13.1 判定 — **GREEN**

| §3.3 の要求 | 実測 | 判定 |
|---|---:|---|
| `attempts > 0` | **4** | ✅ |
| 表示スロットへ到達 | `accepted=4`（`held` へ着弾） | ✅ |
| `leaked == 0` | **0** | ✅ |

```
15:01:54.478 instrument.alive=1901176
15:01:54.740 model.force_cpu=0
15:01:55.563 model.n_layer=28 / n_params=1777088000 / size=1111370240 / meta_count=23 / n_vocab=151936
15:01:55.564 model.origin=1
15:05:04.298 flavor.attempts=1                     ← admit
15:05:04.565 flavor.attempts=1 accepted=1 discarded=0 unavailable=0 leaked=0 dropped_busy=0
15:05:09.147 flavor.attempts=2 …                   （以下 4 サイクル）
15:05:15.246 flavor.attempts=4 accepted=4 discarded=0 unavailable=0 leaked=0 dropped_busy=0
```

**`unavailable=0` が効いている。** `completion` が毎回 `Some` だった＝**実機 Metal 経路で LLM が実際に 4 回生成し、その全てがガードとベルト再スキャンを通過した。** モデル同一性は §11.1 の権威値と一致（`origin=1` バンドル・`force_cpu=0`）。

### 13.2 計器の設計が効いた点

admit と finish が対で観測できる:

```
15:05:04.298  flavor.attempts=1   ← begin_request
15:05:04.565  flavor.attempts=1   ← finish（267ms 後）
```

**この 2 行が無い版で 1 回の実機セッションを失っている。** 送出が `finish` にしか無かったため、「経路に入っていない」と「入ったが完了しなかった」が空ログとして同一に見えた。**沈黙が一意の意味を持つことは、計器の付加機能ではなく要件である。**

### 13.3 【残件 1】ガードは実機で一度も撃っていない（`discarded=0`）

**`leaked == 0` は、`discarded == 0` と併せて読むと見かけより弱い。**

- **証明された**: 実機生成 4 件がガードとベルトの双方でクリーン
- **証明されていない**: ガードが実機で**汚れた出力を撃墜できる**こと

**撃墜対象が一度も飛来していない以上、撃墜能力は実機で試されていない。** F-3 は host 上で 52 試行してこれを実証したが（指示文があってもモデルは数値を書く＝ FLV-R-12）、**実機での射撃はゼロ発である。**

**N=4 は小さすぎる。** 取得は安価で、同じ画面で GD のターンを 20〜30 回進めればよい。**G-4 の成立要件ではないが、「盾が実機で機能する」ことの直接証拠は未取得であると明記する。**

### 13.4 【残件 2】スロット到達済みのテキストが FE で描画されない

**指揮官の目視**: BOOKS / EVENTS / TURN LOG（T0〜T3）は完璧に描画され、ターン進行も確認できたが、**フレーバ文は画面上のどこにも現れなかった。**

FE は配線されている —— `bxs_take_flavor` は `blackboxArena.ts:129` に在り、`BlackboxArena.tsx:227` が `<BlackboxFlavorSlot text={flavorText} />` を描画する。**断絶は配線の欠落ではなく、取得の時刻である。**

`BlackboxArena.tsx:170` は `advance` 直後に `take()` を**一度だけ**呼び、再ポーリングしない。実測タイミングと突き合わせると:

| 時刻 | 出来事 |
|---|---|
| T+0ms | `advance` 復帰・kick 受理（`attempts=N`）。**FE が `take()` → `held` は空 → `null`** |
| T+267ms | 生成完了・`held = Some((corr@tick_N, flavor))` |
| 次ターン | `advance` で tick が N+1 へ。`take(current@tick_N+1)` → **相関不一致 → 破棄**（`take()` は戻さない） |

⇒ **全ての生成物が「直前の take には早すぎ、次の take には古すぎる」。取りこぼしは競合ではなく決定論的である。** 生成が 267ms を要する限り、単発 take では原理的に届かない。

> **これは §3.3 の文言（「表示スロットへ到達」）を破っていないが、G-4 の名称（「フレーバ E2E」）は破っている。** 鎖は最後の環でつながっていない。**指揮官裁定により FE 側の課題として分離**するが、**「実機でフレーバが表示された」と書いてよい証拠は存在しない**（LAW-22）。
>
> 修正の方向は 2 つ。(a) FE が `held` の着弾後に再ポーリングする、(b) `take()` が tick 不一致を破棄せず、直近 1 件を提示可能にする。**(b) は FLV-I-13 の相関設計（P-2-2…5 の「設計上あり得ない新しい tick / 古い・他キャンペーンの結果を捨てる」）に触れるため、安易に緩めてはならない。** 決めるのは測ってからである。

### 13.5 Tier 3 の最終状態

| ゲート | 判定 | 典拠 |
|---|---|---|
| G-0R | **GREEN** | §10.1 |
| G-1 | **GREEN** | §11.1 |
| G-2 | **GREEN** | §11.2 |
| G-3 | **GREEN** | §11.5.1（目視・LAW-20） |
| **G-4** | **GREEN** | **§13.1** |
| G-5 | **GREEN** | §12.1 |

**§3.3 のゲート表は全て緑。Tier 3（実機インフラ・モデルロード検証）は完了した（指揮官承認・2026-07-31）。**

**完了が意味しないこと**: 上記 2 残件（実機でのガード発砲・FE への貫通）は未取得であり、**どちらも「動いているはずだ」で埋めてはならない。** 本フェーズが繰り返し見つけた欠陥は、すべて「書かれているが、走っていない」だった。**最後の 2 つは「走っているが、届いていない」である。**
