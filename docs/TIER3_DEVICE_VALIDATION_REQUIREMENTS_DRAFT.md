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

実機上で `n_gpu_layers` を **0 に強制できるデバッグ経路**を用意し、次の順で進める:

```
G-2  実機 ＋ n_gpu_layers=0（CPU）   → U-2（メモリ）だけを試す
G-3  実機 ＋ n_gpu_layers=999（Metal）→ U-3（バックエンド）だけを試す
```

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
| **G-2** | U-2（メモリ／CPU） | `llama_model_loader: … 339 tensors`。**ロード前後の `phys_footprint_bytes()` と `os_proc_available_memory_bytes()` を数値で記録**。`FootprintBand` の到達段 |
| **G-3** | U-3（Metal） | **`offloaded 29/29 layers to GPU`**（`0/29` なら CPU に落ちている＝ U-3 未検証）。生成が**トークンを返す**。**出力が壊れていないこと**（§3.4） |
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
