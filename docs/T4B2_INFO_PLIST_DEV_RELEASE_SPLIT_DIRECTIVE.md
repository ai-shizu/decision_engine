# T4-B-2 実装指示書 — dev / release 分岐による絶対孤立違反の解消

> **宛先**: Grok 4.5（実装担当）
> **起草**: リードアーキテクト（Opus 5）・2026-08-01
> **前提**: T4-B ゲート（`7cd262e`）が現行アーカイブで **exit 24** を返している
> **完了条件**: **Release ビルドに対し `scripts/ios_archive_scan.sh` が GREEN（exit 0）**

---

## 0. なぜ今直すのか

**赤いまま放置された計器は、やがて警告としての意味を失い、無視されるようになる**（指揮官裁定）。

そしてもう一つ。**いま直せば、直ったことを証明できる。** T4-B には S-4 の逆向きドリル（キーを除いたコピーで緑になる）が既にあり、ゲートが「緑を出す能力」も実証済みである。**出荷直前に直すと、直ったことを確かめる手段が「もう一度全部手で測る」に戻る** —— そしてその手作業は、8 回連続で緑を返しながら本件を見逃していた。

---

## 1. 汚染源は **2 つ**ある。片方だけ直しても緑にならない

起草者が実測した。**推測ではない。**

### 汚染源 A — `apps/desktop/src-tauri/Info.ios.plist`（67 行）

**S-4（plist キー 5 件）と S-5 の plist 側ヒットの発生源。**

```
16: NSAppTransportSecurity
18:   NSAllowsLocalNetworking
20:   NSAllowsArbitraryLoadsInWebContent
22:   NSExceptionDomains
32:     10.0.0.0/8   … 169.254.0.0/16 / 172.16.0.0/12 / 192.168.0.0/16
53:     localhost
62: NSLocalNetworkUsageDescription
```

`tauri.ios.conf.json:34` の `bundle.iOS.infoPlist` が**無条件**でこれを指している。dev / release の分岐は存在しない。

### 汚染源 B — `apps/desktop/src-tauri/tauri.ios.conf.json` の `app.security.devCsp`

**S-5 のバイナリ側ヒットの発生源。** `devCsp` は名前のとおり dev でしか*適用*されないが、**文字列は Release バイナリに埋め込まれる。** 実測:

```
$ grep -ac "ipc.localhost"       Coraxis   → 1
$ grep -ac "connect-src"         Coraxis   → 1
$ grep -ac "ws://127.0.0.1:1421" Coraxis   → 1
```

`build.devUrl: "http://localhost:1420"` も同ファイルにある。

> **A だけを直すと S-4 は緑になるが、S-5 はバイナリ側で赤のまま残る。** 完了条件は「ゲート全体が緑」である。

---

## 2. 壊してはならないもの — 実機 HMR 開発ループ

**このループは高価な実測の積み重ねで確立された**（`AI_SKILLS`・過去セッションの記録）。**壊した状態で「完了」と報告してはならない。**

構成要素:

- Vite dev server（`:1420` / HMR `:1421`）
- `devCsp` の `http:` / `ws:` 許可
- **`Info.plist` の Local Network キー群** —— **実機で「ローカルネットワーク」許可ダイアログを出すために必要**
- `NSLocalNetworkUsageDescription` —— これが無いと iOS は許可を求めず、接続が黙って失敗する

**つまり汚染源 A は、dev では必要である。** 消すのではなく、**dev にだけ残す**のが本タスクである。

---

## 3. 要件

### R-1 Release の完全浄化

Release ビルドの成果物から以下を**完全に排除**すること。

- `NSAppTransportSecurity` とその配下すべて（`NSAllowsLocalNetworking` / `NSAllowsArbitraryLoadsInWebContent` / `NSExceptionDomains`）
- `NSLocalNetworkUsageDescription`
- CIDR 文字列（`10.0.0.0/8` / `169.254.0.0/16` / `172.16.0.0/12` / `192.168.0.0/16`）
- `localhost:1420` / `localhost:1421` / `127.0.0.1:1420` / `127.0.0.1:1421`
- `devCsp` 由来の断片（`ipc.localhost` / `connect-src` の dev 用値）

### R-2 dev ループの維持

`tauri ios dev` 経路では上記が**従来どおり注入**されること。

### R-3 機構の選択は実装者に委ねる。ただし禁じ手がある

Tauri v2 の設定マージ（`--config` による追加設定ファイル）等、**Tauri CLI の枠内で完結する機構**を用いよ。

**禁止:**

- **Xcode 側のスクリプトフック・手編集に依存する機構。** `AI_SKILLS` §5.1「ビルド経路の掟」が Xcode 直の Build / Archive を禁じている。`tauri ios build` / `ios dev` を通らない仕掛けは、**`inject_resources` を迂回した Jul 29 のスタブ混入と同型の事故を再生産する**
- **生成物（`gen/apple/**`）を直接書き換える機構。** あれは disposable である
- **Release 側に「空の ATS 辞書」を残すような部分的解除。** キーの**不在**が要件であり、無害な値の存在ではない

### R-4 ゲートを緩めるな

`scripts/ios_archive_scan.sh` は**無改変**とする。**S-4 / S-5 の検査条件を緩めて緑にすることを禁じる。** 緑は成果物が変わったことによってのみ得られる。

---

## 4. 検証（これが本タスクの主成果物である）

### V-1 Release が緑になること【必須】

```bash
# Release をビルドし直し、ゲートを回す
npm run tauri ios build -- --features pocket-brain,secure-vault,flavor-live
bash scripts/ios_archive_scan.sh
```

**期待: `GATE: GREEN` / exit 0。** S-4 と S-5 の生出力を含めて提出せよ。

> **ビルドし直さずに緑を報告するな。** 現在アーカイブに載っているのは修正前の成果物である。

### V-2 dev 経路にキーが残っていることの証明【必須】

`tauri ios dev` 経路で解決される設定に、§2 の要素が**含まれている**ことを示せ。**成果物または解決後設定の実出力**で示すこと —— 「分岐を書いたので入るはずだ」は証明ではない。

### V-3 ゲートの陽性能力が失われていないことの確認【必須】

R-1 の対象を**1 つだけ**一時的に戻し、**ゲートが再び赤くなる**ことを示せ（4 点セット: 変異 → RED 生出力 → 復元 → GREEN 生出力）。

> **緑になったゲートは、赤を出せることを再証明するまで信用できない。** 成果物を変えて緑にしたのか、検査が対象を見失って緑になったのかは、**赤を出させてみるまで区別がつかない。**

### V-4 実機 HMR の実動作 —— **貴殿の範囲外**

実機での HMR 接続確認は指揮官が行う。**貴殿は V-2 までを証明し、「実機未確認」と明記せよ。** 実機で確かめていないものを「維持されている」と書くな。

---

## 5. 受入条件（起草者はこれで監査する）

| # | 条件 |
|---|---|
| D-1 | **Release リビルド後、`ios_archive_scan.sh` が exit 0**（生出力） |
| D-2 | S-4 が GREEN（plist キー 0） |
| D-3 | S-5 が GREEN（**バイナリ側・plist 側とも 0**） |
| D-4 | S-1/S-2/S-3/S-6/S-7 が GREEN のまま（退行なし） |
| D-5 | V-2: dev 経路にキーが残る実出力 |
| D-6 | V-3: 逆向き変異ドリル 4 点セット |
| D-7 | `scripts/ios_archive_scan.sh` **無改変**（`git diff` 0） |
| D-8 | `.github/workflows/` 5 本・`gguf_three_point_sha_gate.sh` 無改変 |
| D-9 | 3 点 SHA ゲートが引き続き ALL OK（**スタブ混入なし**） |
| D-10 | 実機 HMR 未確認である旨の明記 |

---

## 6. 報告様式

**コマンドと生出力の対で。数値だけの要約は無効。**

1. 変更したファイルと差分
2. **リビルド後のゲート実行の生出力全文**（`SCAN SCOPE` と陽性対照を含む）
3. V-2 の実出力
4. V-3 の 4 点セット
5. **実施しなかったこと・確かめなかったことの明記**

---

## 7. このタスクが閉じるもの

ロードマップ §15.2 は「network 系 plist key 0」を DoD に据え、§14 は「1 件でも検出したら release 停止」と定める。**その違反が、検査されないまま出荷経路に載っていた。**

T4-B はそれを可視化した。**T4-B-2 はそれを消す。**

そして最後に一度だけ、消えたことをゲート自身に確認させる —— **緑を報告する資格は、赤を出せることを示した検査器にしかない。**
