# F-3 着工指示書 — LLM フレーバ層のアリーナ結合

**正本**: `docs/SPEC_FLAVOR_LAYER.md` **v3**（本指示書は v3 の作業手順であり、規約の正本ではない）。
**統治**: `docs/AI_SKILLS.md` §17 血の教訓法典 / §19 改定手続 / FLR 統治手続（HARD STOP）。
**発行**: 2026-07-29・リードアーキテクト。**実装は指示書の順に行い、各 T の末尾で停止して指揮官の裁定を仰ぐこと。**

---

## 0. 着工時の実測ベースライン（監査役が自ら計測。信用せず再実行せよ）

| 項目 | コマンド | 実測値 |
|---|---|---|
| HEAD | `git log --oneline -1` | `ba53c24`・origin と同期・clean |
| python 全体 | `python3 -m pytest tests/ -q` | **770 passed / 2 skipped / 0 failed** |
| python 収集数 | `python3 -m pytest tests/ --collect-only -q \| tail -1` | **771 collected** |
| フレーバ契約 | `python3 -m pytest tests/test_flavor_layer_contract.py -q` | **10 passed** |
| フレーバ Rust | `cargo test --features flavor-layer --lib flavor` | **16 passed / 0 failed**（212 filtered out） |
| doctest フェンス | `cargo test --features flavor-layer --doc` | **30 passed / 0 failed**（**CI では一度も実行されていない** — T-7 参照） |
| ビルド | `cargo check --features flavor-layer --lib` | 成功（キャッシュ温で 6.52s） |
| clippy（lib） | `cargo clippy --features flavor-layer --lib` | **0 errors / 12 warnings**（既存・`src/flavor/` 着弾ゼロ） |
| clippy（全体） | `cargo clippy --features flavor-layer --all-targets` | **既存 RED・280 errors** |

> **clippy の 280 は既存 RED である。** `--features flavor-layer` の有無に関わらず 280 で同一であり、`src/flavor/` に着弾するものは**一件も無い**（内訳: lib test 側の `unwrap` on Result 125 / `should not be present in production code` 78 / `unwrap` on Option 65 / `indexing may panic` 12）。
>
> **したがって F-3 の受入で `--all-targets` の GREEN を主張してはならない。** 受入は次の 2 点で表明せよ:
> 1. `cargo clippy --features flavor-live --lib` が **error 0** ∧ warnings が **12 のまま** ∧ フレーバ経路への着弾が **0 件**
> 2. `cargo clippy --features flavor-live --all-targets` の error 数が **280 のまま** ∧ `src/flavor` `src/llm/flavor_gen.rs` `src/blackbox_arena/flavor_slot.rs` への着弾が **0 件**
>
> **【監査役の訂正・2026-07-29】** 本項は当初 `--lib` に「0 warnings / 0 errors」を要求していたが、**これは誤りであった。** `94d83ee` を stash して実測した baseline は **12 warnings**（すべて非フレーバ）であり、**書かれた日から達成不能な条件**だった。**両者とも差分（delta）で表明せよ** —— 絶対値のゼロを要求すると、達成不能な条件を満たすために無関係な既存コードへ手を入れる圧力が生まれる。それは第十律（最小介入）違反であり、diff を膨らませて監査を困難にする。
>
> T-1 で実装者はこの 12 を **0 と騙らず正直に報告した。** 正しい振る舞いである。**ゲートが実測と食い違ったら、実測が正しい。ゲートを直せ。**
>
> 裸の数字を報告するな。**コマンドと生出力の対**で報告せよ（F-1 で clippy の `0` がコマンド無しに報告され、`--lib` と `--all-targets` で食い違った前例がある）。

---

## 1. 不可侵 — 触れたら差し戻し

1. **封緘領域**: `blackbox_sim/` の校正・`src-tauri/src/db/`・`.github/workflows/blackbox-profile-write-gate.yml`。`CalibrationCertificate` の封印は R-11 により**絶対不可侵**
2. **凍結コーパス**: `src/flavor/corpus.rs` の既存配列は **1 バイトも改変不可**。**追加（append）は改変ではないので可**
3. **凍結 API**: `VerifiedFlavor::verify(&str, &FlavorPolicy)` / `FlavorPolicy::v1_empty()` の **signature** は不変。`for_template` は**追加**であって変更ではない
4. **`flavor/` の純粋性**: `flavor/` に `pocket-brain` 依存・Genesis fingerprint・Fact 型を**入れない**
5. **LAW-20 / no-llm-authority**: フレーバを 6D / profile / gap / tensor / 決定ログ / vault へ渡すことを永久禁止
6. **push は指揮官の明示指示があるまで行わない**

---

## 2. タスク

### T-1 — FLV-R-9: 予算の権威を繋ぐ（着工前監査で発見した未配線の是正）

**背景（実測）**: `TemplateId::max_chars()` = 48/72/64 は存在するが、スキャナは `policy.max_chars()` を読み、唯一のコンストラクタ `v1_empty()` は **256 固定**。`TemplateId::max_chars()` の呼出元は `FlavorRequest::max_chars()` のみで、その呼出元は**ゼロ**。**この状態で配線すると FLV-R-5 は全ゲート GREEN のまま死ぬ。**

**作業**:
1. `FlavorPolicy::for_template(TemplateId) -> FlavorPolicy` を追加（`version` は 1 のまま、`max_chars` は `template_id.max_chars()`）
2. `v1_empty()` の doc に**コーパス／計測専用**である旨と、生成経路からの呼出禁止を明記
3. 母集団 **P-1**（§3.1）を `policy.rs` の `#[cfg(test)]` に固定

**関所 A**: P-1 全件 GREEN。**特に P-1-7 / P-1-8**（同一入力・テンプレート違いで判定が逆転する対）が通ること。
**変異ドリル D-1**: `ArenaEventHeadline` の上限を 48 → 256 に書き換え、**P-1-7 が RED になることを示す**。復元して GREEN を示す。
**変異ドリル D-2**: `for_template` の本体を `Self::v1_empty()` に差し替え、**P-1-7 が RED になることを示す**。復元して GREEN を示す。

---

### T-2 — feature `flavor-live` の追加と既定漏れの閉塞（FLV-I-12）

```toml
flavor-live = ["flavor-layer", "pocket-brain", "blackbox-sim"]
```

**指揮官裁定（2026-07-29）**: `flavor-live` が `blackbox-sim` を引き込むことを**承認**する。上位 feature が下位を引き込むことは統合テストの都合上許容される。**絶対条件は単方向性の維持のみ** —— すなわち **`blackbox-sim` 側から `flavor-live` / `pocket-brain` を引き込まないこと**（FLV-I-08）。

**依存の向きを取り違えるな。** FLV-I-08 が禁ずるのは `blackbox-sim` **が** `pocket-brain` を引くこと。`flavor-live` が `blackbox-sim` を引くのは逆向きで、`blackbox-profile-write = ["blackbox-sim"]` の確立形に倣う。**この単方向性が裁定の絶対条件であるため、契約テストは逆辺を明示的に表明せよ**（下記）。

**作業**: `tests/test_flavor_layer_contract.py` に `test_flv_i_12_flavor_live_not_in_default` を追加。表明する事項:
- `flavor-live` が `default` に**無い**
- `blackbox-sim` の定義に `pocket-brain` / `flavor-layer` / `flavor-live` が**現れない**（逆辺の禁止）
- `flavor-layer` の定義に `pocket-brain` が現れない（既存 FLV-I-08 の維持）

**変異ドリル D-7**: `default` に `flavor-live` を追記して **RED を示す**。復元。

---

### T-3 — 生成アダプタ `llm/flavor_gen.rs`（FLV-R-8 / FLV-R-12）

**`llm/flavor/` を*ディレクトリ*として作るな**（`test_lib_rs_gates_flavor_module` が不在を表明している）。**単一ファイル**とせよ。

**作業**:
1. `FlavorRequest` → プロンプト文字列のレンダリング。**数値を prompt に入れない**（FLV-W-02）
2. 生成 → `UnverifiedFlavor` → `VerifiedFlavor::verify(raw, &FlavorPolicy::for_template(tid))`
3. **不通過は破棄。再試行・切り詰め・修正を実装しない**（A-2 / FLV-I-07）
4. プロンプトの「数値を書くな」節は**差し替え可能な定数**として分離する（§4 の 2 アーム計測でアーム B が除去する対象）

**関所 B**: `test_flv_i_07_no_retry_on_guard_failure` — `flavor_gen.rs` / `flavor_slot.rs` に再試行の構造（`loop` による再生成・`retry` 識別子・`verify` 失敗後の再 `generate` 呼出）が**存在しない**ことを走査で表明。
**関所 C**: `test_flv_i_15_generation_path_never_calls_v1_empty` — `llm/flavor_gen.rs` と `blackbox_arena/flavor_slot.rs` に `v1_empty` が現れないことを走査で表明。

---

### T-4 — アンビエント単一スロット `blackbox_arena/flavor_slot.rs`（FLV-R-10）

```
FlavorCorrelation { genesis_fingerprint: [u8; 8], tick: u32, template_id: TemplateId }
単一スロット: Option<(FlavorCorrelation, VerifiedFlavor)>
```

**規約**:
- **`AdvanceView` に相乗りさせない。** 相乗りはターン進行が生成完了を待つ経路を生む（A-4 違反）
- 配信は**非ブロッキング pull**（`bxs_take_flavor`）。不在時は即 `None`
- **busy 時は新要求を捨てる。キューを作らない**
- 照合は **Rust 側**。FE から相関トークンを受け取る設計にするな（権威を FE に置くことになる）
- **take セマンティクス**: 引き渡したらスロットを空にする（同じフレーバを次 tick で再配信しない）

**作業**: 母集団 **P-2**（§3.2）と **P-3**（§3.3）を `flavor_slot.rs` の `#[cfg(test)]` に固定。

**関所 D**: P-2 / P-3 全件 GREEN。
**変異ドリル D-3**: 相関照合を削除し、**P-2-2〜P-2-5 が RED になることを示す**。復元。
**変異ドリル D-4**: busy 時の破棄をキューイングに変え、**P-3-3 が RED になることを示す**。復元。

---

### T-5 — FE 配線と視覚的区別（FLV-I-05 / FLV-I-11）

**作業**:
1. フレーバは **Fact スロットと視覚的に区別された専用スロット**にのみ描画（減光・イタリック・接頭記号等 — §6 補償制御）
2. `dangerouslySetInnerHTML` 禁止
3. フレーバ文字列を数値へ変換する経路を作らない

**関所 E**: `test_flv_i_05_fe_never_parses_flavor_text` — 母集団 **P-4**（§3.4）の禁止パターンがフレーバ経路に現れないことを走査。
**関所 F**: `test_flv_i_11_flavor_slot_is_visually_distinct` — フレーバ描画要素が Fact 描画要素と**異なる CSS クラス／スタイル識別子**を持つことをリテラル一致で表明（LAW-23 に従い、期待文字列を実装前に洗い出すこと）。

---

### T-6 — A-1 削除可能性の証明（§13）— **F-3 完了条件の中心**

> 同一シードのキャンペーンを `flavor-live` 有 / 無で走らせ、`state_digest` の系列が**バイト単位で完全一致**する。

**規約**:
- **cargo の中で cargo を起動するな**（§9.2）。CI の独立したシェルステップとして駆動する
- **最終 digest ではなく Decide-time の全系列**を出せ（Phase 6-A step1 の教訓 — 最終だけ合わせると途中の分岐退化が黙殺される）
- **モデルをロードした状態でも同じ照合を行え。** 「LLM が動いていないから一致した」は A-1 の証明ではない

**作業**: 既存のリプレイ／校正ハーネスのどれを再利用したかを**実名で報告**すること。新規に書いた場合はその理由を述べよ。

---

### T-7 — CI ゲート `.github/workflows/flavor-gate.yml`（FLV-I-17）

**背景（実測）**: `grep -rln flavor_seal_probe` の結果は **AI_SKILLS.md / Cargo.toml / lib.rs / verified.rs の 4 件のみ**。workflow も shell も Makefile も pytest も存在せず、`.github/workflows/` に "flavor" の文字列は**ゼロ**。**§9.2 が要求する「CI の独立したステップ」は未実装であり、この層で最も強い保証がどの自動ゲートからも駆動されていない。** AI_SKILLS §20.1-1 の「CI/監査の `cargo rustc --cfg ...` で理由まで検証」は**一度きりの手動監査**を指していたにすぎない。

**指揮官裁定（2026-07-29）**: **既存の封印プローブもすべて本ゲートへレトロフィットせよ。** F-3 で新設する分だけでなく、F-1/F-2 時代に作られたものを漏れなく含める。**CI で実行されないガードは幻想にすぎない。**

#### T-7 の前に読め — レトロフィット対象の実測（監査役調べ）

`.github/workflows/` 内の `cargo test` は**全て `--lib`** であり、`--doc` は**どの workflow にも存在しない**。**`--lib` は doctest を実行対象から外すフラグである。** 一方:

```
$ cargo test --features flavor-layer --doc
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

内訳 = `flavor/verified.rs` の 11 フェンス + 11 伴走、`knowledge/fsm.rs` の 4 フェンス + 4 伴走。**本 repo の compile-fail フェンス 15 本は、書かれた日から一度も CI で実行されていない**（FLV-W-11）。

BXS の C-1（`--lib` を必ず付けよ）は正しい掟である。しかし**その掟が別のガード群を丸ごと無効化していた。** どちらの掟も単独では正しいため、掟を読むだけでは発見できない。**`--lib` を書く箇所には `--doc` を別ステップとして併記せよ。**

**作業**: `blackbox-profile-write-gate.yml` の確立形に倣い、job を分けて依存辺を監査証跡にする。

| job | 内容 |
|---|---|
| `seal-probe` | `cargo rustc --features flavor-layer --lib -- --cfg flavor_seal_probe_verified` が **E0451** を、`--cfg flavor_seal_probe_checked` が **E0603** を stderr に出すことを `grep -c` == 1 で表明。**コンパイルが失敗したこと**だけでなく**理由**を表明せよ。F-3 で新設するプローブも同じ形で追加 |
| `doctest-fences`（**レトロフィット・新設**） | `cargo test --features flavor-layer --doc`。**`--lib` を付けるな**（付けた瞬間このジョブは何も検査しない）。`test result: ok.` 行が **1 本**であることと、`passed` が **30 以上**であることを表明する。**件数を下限として固定せよ** —— フェンスが静かに消えたとき、`ok` だけを見ていると気付けない |
| `a1-deletability` | §13 の digest 恒等（`flavor-live` 有/無、モデル有/無の 4 構成） |
| `flavor-live-absent` | 既定 feature 構成で `flavor_gen` / `flavor_slot` が**コード上不在**であることを `never used` 走査で表明（`write-absent` と同型） |

**変異ドリル D-5**: `checked.rs` の `Checked` タプル欄を `pub(crate)` へ退行させ、**`seal-probe` job が E0603 を出さなくなる（＝ RED）ことを示す**。復元して GREEN を示す。
**変異ドリル D-8（新設）**: `verified.rs` のフェンス doctest を 1 本削除し、**`doctest-fences` job が下限 30 を割って RED になることを示す**。復元して GREEN を示す。**`ok` だけを条件にしていると、この変異は検出されない** —— それを実演せよ。

#### T-7 必須要件（指揮官裁定・2026-07-29）— テスト実行件数の下限固定

**T-1c の監査で残存リスクが実測された。** 契約テストは `#[test]` の剥離と `#[ignore]` を閉塞したが、**`#[cfg(test)]` をモジュールごと無効化する経路（例: `#[cfg(any())]`）では緑のまま通る**（実測: 契約テスト `11 passed` のまま）。属性を見る静的走査は、モジュール自体がコンパイル対象から外れる経路を覆えない。

**受け皿は件数の下限表明である。** `doctest-fences` に課した「passed ≥ 30」と**同型の下限**を、次に対しても CI で課せ:

| 対象 | 下限 |
|---|---|
| `cargo test --features flavor-layer --lib flavor` | **26 以上**（`test result: ok.` 行が 1 本 ∧ `0 ignored`） |
| `cargo test --features flavor-layer --doc` | **30 以上** |

**`0 ignored` の表明を省くな。** `#[ignore]` が付いたテストがあっても `test result:` は `ok.` と出る —— **`ok` は「全部走った」を意味しない。**

**変異ドリル D-9（新設）**: `flavor/policy.rs` の `#[cfg(test)]` を `#[cfg(any())]` に差し替え、**件数下限で RED になることを示す**。復元して GREEN を示す。この変異は python 契約テストでは検出できない —— **それも併せて実演し、「どちらのガードが何を覆うか」を報告せよ。**
**`seal-probe` には `--lib` を付けよ／`doctest-fences` には付けるな。** 両者は逆である。取り違えると片方が恒久 GREEN（何も検査しない）になる。

---

### T-8 — 破棄率の実測（§14）— **母集団を著述してはならない唯一の計測**

**2 アームで実施**（§14.2）:

| アーム | プロンプト |
|---|---|
| A | 「数値を書くな」の指示文**あり** |
| B | 当該指示文を**除去** |

**報告形式（§14.3 — 欠くと無効）**: `arm` / `model` / `N` / `accepted` / `discarded` / `findings`（`Invisible` `Markup` `UnicodeNumeral` `HanNumeral` `LexicalQuantity` `OverBudget` の内訳）/ `leaked`。

**安全性の主張は「両アームとも `leaked` == 0」である。** 破棄率はアーム間で当然異なってよく、**異なることこそが「指示文は効率手段である」の意味**である（FLV-R-12）。

**絶対禁止**: 破棄率を良くするためにテーブル・例外表・境界・コーパスを変更すること（LAW-25b の「さらに悪い変奏」）。**アーム B の破棄率が高いことはテーブルを緩める理由にならない。**

**N が小さいまま FLV-R-3 の判断材料として提出するな。** 比率は必ず N と内訳を同伴させよ。

---

## 3. 事前凍結された母集団（監査側が著述。実装前に凍結・LAW-25）

**実装者はこの母集団を改変してはならない。** 期待値が実装に合わないときは、実装を直すか、**指揮官へ差し戻せ**。母集団の書き換えで通すことを禁ずる。

> 破棄率（T-8）だけはこの原則の**例外**である。あれは誰も期待判定を著述していない母集団に対してのみ成立する計測であり、監査側が著述した瞬間に情報を失う（LAW-25b）。

### 3.1 P-1 — テンプレート単位予算（FLV-I-15）

フィラーは `"あ"` の反復とする。`あ` は L1（`is_numeric`）・L2（`HAN_NUMERALS`）・L3（`LEXICAL_QUANTITY`）・`AUDITED_V1` の**いずれにも属さず**、charset 段の不可視・markup にも該当しない（監査役が `tables.rs` / `charset.rs` を読んで確認済み）。ゆえに `OverBudget` 以外の `Finding` が混入しない。

| # | policy | 入力 | 期待 |
|---|---|---|---|
| P-1-1 | `for_template(ArenaEventHeadline)` | `"あ"×48` | **ACCEPT** |
| P-1-2 | `for_template(ArenaEventHeadline)` | `"あ"×49` | **REJECT** `OverBudget { chars: 49, max: 48 }` |
| P-1-3 | `for_template(ArenaEventAside)` | `"あ"×72` | **ACCEPT** |
| P-1-4 | `for_template(ArenaEventAside)` | `"あ"×73` | **REJECT** `OverBudget { chars: 73, max: 72 }` |
| P-1-5 | `for_template(SettlementRipple)` | `"あ"×64` | **ACCEPT** |
| P-1-6 | `for_template(SettlementRipple)` | `"あ"×65` | **REJECT** `OverBudget { chars: 65, max: 64 }` |
| **P-1-7** | **`for_template(ArenaEventHeadline)`** | **`"あ"×60`** | **REJECT** |
| **P-1-8** | **`for_template(ArenaEventAside)`** | **`"あ"×60`** | **ACCEPT** |
| P-1-9 | `v1_empty()` | `"あ"×200` | **ACCEPT**（計測用の器は意図的に緩い） |

> **【監査役の訂正・2026-07-29】P-1-7 / P-1-8 は「心臓」ではない。**
>
> 当初、本項は P-1-7/P-1-8 を「偶然通ることがあり得ない唯一の対」「この母集団の心臓」と記していた。**論理としては正しいが、検出器としては誤りである。**
>
> T-1 完了後の監査で、`for_template` を**フラット 60**（48 と 72 の間に置いた敵対的定数 —— まさにこの対が捕らえるはずの形）へ変異させて実測した:
>
> ```
> panicked at src/flavor/policy.rs:84:9:
> expected REJECT for len=49 under max=60
> ```
>
> **落ちたのは P-1-2 であり、P-1-7 は実行すらされなかった。** P-1-1〜P-1-6 が既に 3 つの異なる予算を釘付けしており、実際に発火するのは常に最も厳しい **P-1-2**（49 を 48 で REJECT）である —— 49 以上のあらゆるフラット定数を捕らえ、49 未満は P-1-1 が捕らえる。したがって **P-1-7 / P-1-8 は論理的に冗長**であり、価値は**意図の記録**に留まる。**削除はするな**（`for_template` がテンプレートを取り違える将来の変異に対する記録として残す）が、**検出力を担っていると考えるな。**
>
> P-1-9 は、`for_template` が `v1_empty` へ退行したことを `v1_empty` 側の緩さと対比して記録する対照である。

> **P-1 は 1 つのテスト関数に畳んではならない（T-1b で是正済み）。** Rust の `assert` は最初の不一致で中断するため、9 件を 1 関数に入れると**観測できるのは常に先頭の 1 件だけ**になり、残り 8 件は「効いている」ことが一度も示されない。これは SPEC §9.1-2（**攻撃 1 つにつきフェンス 1 つ。複数の欠如を 1 フェンスにまとめると、残った 1 つのエラーが新しく開いた穴を隠す**）を母集団に適用した話である。**ケースが独立に観測できるよう関数を分割せよ。** 分割していれば、変異ドリルのために一時テストを作って消す必要は生じない。

### 3.2 P-2 — 相関トークン（FLV-I-13）

セッションの現在値を `(F1, T5, Headline)` とする。

| # | スロットが保持する相関 | 期待 |
|---|---|---|
| P-2-1 | `(F1, T5, Headline)` | **配信** |
| P-2-2 | `(F1, T4, Headline)` | **破棄**（tick が古い — 遅延到着） |
| P-2-3 | `(F1, T6, Headline)` | **破棄**（tick が新しい — 並べ替え／時計ずれ） |
| P-2-4 | `(F2, T5, Headline)` | **破棄**（別キャンペーン） |
| P-2-5 | `(F1, T5, Aside)` | **破棄**（別テンプレート） |
| P-2-6 | 空 | `None` |
| P-2-7 | `(F1, T5, Headline)` を **2 回 pull** | 1 回目は配信、**2 回目は `None`**（take セマンティクス） |

> P-2-3 を落とすな。「古い方だけ弾けばよい」は誤りである —— 単一スロットは最新優先で上書きされるため、**現在 tick より新しい相関がスロットに載る状態は設計上あり得ない**。あり得ない状態が観測されたら、それは配線バグであって配信対象ではない。
> P-2-7 は「同じフレーバが次ターンにも表示される」失敗様式（FLV-W-10 の変奏）を閉じる。

### 3.3 P-3 — busy と単一スロット（FLV-I-14）

| # | 状況 | 期待 |
|---|---|---|
| P-3-1 | idle 時に要求 | 生成へ渡る |
| P-3-2 | 生成中に要求 | **新要求を破棄**。呼び出しは**即座に返る**（ブロックしない） |
| P-3-3 | 生成中に 100 連続要求 | **キュー長は常に 0**。無制限成長なし |
| P-3-4 | キャンペーン中止後に生成が完了 | 結果を破棄。panic しない |

### 3.4 P-4 — FE 禁止パターン（FLV-I-05）

フレーバ経路（フレーバ文字列を受け取る TS/TSX）に現れてはならない識別子・構文:

`Number(` / `parseFloat` / `parseInt` / `Math.` / 単項 `+` によるフレーバ変換 / `/\d/` 等の数字クラス正規表現 / `.match(` / `dangerouslySetInnerHTML`

> 走査は**フレーバ経路に限定**せよ。FE 全体を対象にすると既存の Fact 描画（数値を正当に扱う）が引っかかり、テストを緩める圧力が生まれる。**緩めた瞬間このガードは死ぬ。**

---

## 4. 報告の様式（LAW-22 — これを欠く報告は虚偽）

各 T の完了時に次を提出せよ。

1. **コマンドと生出力の対**（数字だけの報告は無効）
2. **変異ドリルの 4 点セット**: ①壊す変更の diff ②RED の生出力 ③復元 ④GREEN の生出力
3. **未実施項目の明示的申告**
4. `python3 -m pytest tests/ -q | tail -1`（770 からの増減を説明せよ）
5. `python3 -m pytest tests/ --collect-only -q | tail -1`（**消えたテストが無いこと**。テスト集合の差分で隠蔽が露見する）

   > **収集数 771 と実行結果 772（770 passed + 2 skipped）が 1 だけ食い違う理由を先に潰しておく。** `tests/test_ui_smoke.py:21` は `textual` 未導入による**モジュールレベルの import skip** であり、収集対象に数えられないまま実行結果には skip 1 件として現れる。もう一方の skip（`test_fsa_2026_07_13_02_stdio_llm_boundary.py:109` — Windows AppContainer）は通常の収集済みテストである。**この差は既存であり `ba53c24` で実測確認済み**（監査役が stash して HEAD で再計測）。開発機に `textual` を入れると収集数が動くので、**環境差を退行と誤認するな。**
6. clippy は §0 の 2 点形式（**両者とも差分。絶対値のゼロを主張するな**）で

**「完了」の語は検証ログの後にのみ置ける。ビルド成功は動作の証明ではない。**

---

## 5. 禁止事項（再掲・違反は差し戻し）

- ガード不通過時の**再試行**（A-2 / FLV-I-07）
- フレーバの**永続化**・決定ログ／`state_digest` への混入（FLV-R-2 / A-6 / §7）
- **射程拡大** —— アリーナ 1 面のみ（FLV-R-1）。CONSULT / 面接 / ES 添削 / 講評へ広げるには裁定が要る
- 破棄率のために**テーブル・例外表・境界を緩める**こと（LAW-25b）
- `flavor-live` を **default features** へ入れること
- 事前凍結された母集団 **P-1〜P-4 の書き換え**
- **push**（指揮官の明示指示があるまで）

---

## 6. 申し送り

- **指示と成果物の食い違いは監査で必ず露見する。** 指示に従えない技術的理由を見つけたら、**黙って実装を変えずに報告せよ**（F-2 T-3 の手続違反の再発防止）。判断が正しくても、報告しない限り手続違反である
- **未検証は「未検証」と書け。** これは従来どおり評価する
- Tier 3（GGUF 取込の実機検証）は出荷ブロッカーとして残置。`main` マージ前・リリースビルド前に必須（`AI_SKILLS` §5.1 — **取込 UI へ到達する方法が非自明なので必読**）
