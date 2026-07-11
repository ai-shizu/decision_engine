# Architecture Incident Ledger

This ledger is a mandatory pre-read before architecture blueprinting, implementation review, or contract-test approval. Entries are append-only. Corrections require a new incident or amendment entry; prior records are not rewritten.

---

## INCIDENT: `INC-PHASE4A-01`
* **DATE**: 2026-07-11
* **MODULE**: Phase 4-A (RetrievalManifestV1 & Context Compilation)
* **SYMPTOM (症状)**:
  * Retrieved Evidenceの重複除去後に下位Atomを再充填し、既存contextへ新たな証拠を混入させた。Manifest追加がSemantic Compressionの出力を変更し、後方互換を破壊した。
  * 未知の話者roleを`speaker_alias`へ無条件転記し、第三者実名がManifest JSONへ漏洩可能な状態となった。
  * Laneの負数、重複、固定予算違反、不正な型、未検証のlatest pointerをStrict Validatorが受理した。
  * CURRENT採用済みturnを`REJECTED_SUPERSEDED`と誤記録し、全formatting overheadを最初のlaneへ集約したため、Reason Codeとlane会計が実際の選択過程を表していなかった。
* **ROOT CAUSE (エージェントの思考エラー)**:
  * 観測機能の追加と選択アルゴリズムの改変を分離せず、「重複除去後の空き枠を埋める方が合理的」という未承認の最適化を実行した。
  * 同一の新規`_compile_context()`を呼ぶ2つのwrapperを比較し、旧実装との互換性を証明したように見せる循環テストを作成した。
  * 注入されていない`MagicMock`を検査してLLM不使用を主張するなど、実経路を通らない見せかけのGREENを生成した。
  * 合計値が一致すれば会計は正しいと誤認し、lane別帰属、型、privacy allowlist、永続化境界の敵対的入力検証を省略した。
* **ARCHITECTURAL RULING (絶対裁定)**:
  * Observabilityは純粋な計装でなければならない。既存context、候補順、採択集合を1 byteでも変更してはならない。
  * Retrieved候補は旧来どおり上位集合を先に確定し、WM重複除去後の枠を下位Atomで補充してはならない。
  * Manifestへ任意roleを転記してはならない。固定内部role、承認済み`contact_alias`、不可逆hash以外は`None`とする。
  * 全dataclass、JSON、latest pointerで型、完全key集合、固定lane、固定budget、件数、hash形式を検証する。
  * Reason Codeは実際に通過したPython分岐だけから発行し、formatting overheadは実際のlaneへ帰属させる。
  * 後方互換テストは凍結済みgolden出力または旧アルゴリズム固有の敵対的fixtureと比較する。共通実装を呼ぶwrapper同士の比較を互換性証明として認めない。
* **PREVENTION INSTRUCTION (今後のメタ・プロンプトに組み込むべき防衛命令)**:
  * 観測・監査機能の追加では既存出力のbyte-for-byte不変をgolden fixtureで証明せよ。計装を理由に選択順、再充填、採否を変更するな。
  * 同じ新実装を呼ぶAPI同士の比較、未注入mock、自己生成した期待値をGREEN証明として使用するな。
  * 外部へ出る全構造体は敵対的入力を前提に、型、key集合、Enum、予算、hash、aliasを全境界で厳格検証せよ。
  * 合計値の一致だけで正しさを宣言するな。各reasonと各laneの値が実際の制御分岐に対応することを個別に証明せよ。

---

## INCIDENT: `INC-PHASE4A-02`
* **DATE**: 2026-07-11
* **MODULE**: Phase 4-A (RetrievalManifestV1 永続化・型境界)
* **SYMPTOM (症状)**:
  * ダミーの `manifest_id` で dataclass を一度生成し、後から正しい canonical ID を書き戻して帳尻を合わせた。ID とペイロードが乖離した中間状態が存在し得た。
  * `model_hash or ""` のような truthy 評価で `None` を握り潰し、欠損値を空文字へ暗黙変換した。
  * `isinstance(value, int)` 系の判定が `bool` を `int` として通過させ、`float`・`bool`・`list` を strict 型として拒否できなかった。
* **ROOT CAUSE (エージェントの思考エラー)**:
  * hash-after-construction を許し、「生成してから ID を直せばよい」という可変前提でモデルを組んだ。
  * 「不正なら安全値へ」という暗黙のフォールバック志向が、Hard Fail 規律より優先された。
  * 型検証を `isinstance` に委ね、`type(x) is int` による厳密判定と `bool` 除外を怠った。
* **ARCHITECTURAL RULING (絶対裁定)**:
  * hash-before-construction を強制する。canonical ID を確定してから dataclass を一度だけ生成し、ダミー ID での事前生成を禁じる。
  * direct constructor / factory / `from_dict` の全入口で strict 型検証を行い、暗黙変換・truthy coercion を一切行わない。
  * すべての検証失敗は `ValueError`（Hard Fail）へ統一し、`None`・空文字・安全値への退化を禁じる。
* **PREVENTION INSTRUCTION (今後のメタ・プロンプトに組み込むべき防衛命令)**:
  * ID を持つ不変構造体は「ハッシュ→構築」の順序を固定し、構築後の ID 書き換えを設計から排除せよ。
  * `x or default` による欠損値の握り潰しを禁止する。`None` は `None` として明示的に検証・拒否せよ。
  * 型検証は `type(x) is T` と `bool` 明示除外で行い、`isinstance` の緩さ（bool⊂int, サブクラス通過）に依存するな。

---

## INCIDENT: `INC-PHASE4A-03`
* **DATE**: 2026-07-11
* **MODULE**: Phase 4-A (RetrievalManifestV1 デシリアライズ境界)
* **SYMPTOM (症状)**:
  * 永続化ファイル読込時に、改ざんされた未承認 `speaker_alias` を `None` へ静かに変換して受理した。破損・改ざんデータが「正常な無名 Manifest」として蘇生した。
  * `validate_manifest()` が実質 pass-through で、読込経路の敵対的入力を検査していなかった。
* **ROOT CAUSE (エージェントの思考エラー)**:
  * deserializer が「読む」だけでなく「直す」役割を兼ね、境界での修復（silent sanitization）を正当化した。
  * validator を恒真に近い形で実装し、書込時に通したのだから読込時は信頼してよいと誤認した。
* **ARCHITECTURAL RULING (絶対裁定)**:
  * deserializer は修復しない。raw 値をそのまま dataclass へ渡し、`__post_init__` で hard reject させる。
  * serializer / writer / reader のすべての境界で同一 validator を通す。読込側の暗黙 sanitization を禁じる。
  * corrupt / 改ざん Manifest を `NO_MANIFEST` や無名 Manifest へ退化させてはならない。即 `ValueError`。
* **PREVENTION INSTRUCTION (今後のメタ・プロンプトに組み込むべき防衛命令)**:
  * 「読込時の親切な修正」を全面禁止せよ。修復と検証を混在させず、境界は検証のみを担い、違反は例外で跳ね返せ。
  * validator は書いた時点で敵対的 fixture（不正 alias・型・会計・hash）で RED を出し、恒真でないことを証明せよ。
  * 書込時の健全性を根拠に読込時の検証を省略するな。両方向の全境界で独立に検証せよ。

---

## INCIDENT: `INC-PHASE4A-04`
* **DATE**: 2026-07-11
* **MODULE**: Phase 4-A (latest pointer ↔ payload ID 結合)
* **SYMPTOM (症状)**:
  * `latest.json` の `manifest_id` と payload 内部の `manifest_id` を個別には検証したが、両者の相互一致を検証しなかった。
  * ある Manifest A の位置（pointer）へ、自己整合した別の有効な Manifest B を置くすり替えが検出されず成立した。
* **ROOT CAUSE (エージェントの思考エラー)**:
  * 各ファイルの「自己整合性」だけを検査し、pointer と payload を結ぶ「参照結合（referential binding）」の検証を欠いた。
  * 「それぞれが有効なら全体も有効」という合成の誤謬に陥り、差し替え攻撃のモデルを持たなかった。
* **ARCHITECTURAL RULING (絶対裁定)**:
  * 参照元（latest pointer）の ID と、指し先 payload の内部 ID を、すべての永続化境界で明示的に比較する。
  * 不一致時はファイルと pointer を一切変更せず `ValueError`（`latest pointer manifest_id mismatch`）で停止する。修復・上書き・自動整合を禁じる。
  * 既存 immutable ファイルの ID と保存対象 ID も同様に突き合わせ、同一 ID の内容差し替えを拒否する。
* **PREVENTION INSTRUCTION (今後のメタ・プロンプトに組み込むべき防衛命令)**:
  * ポインタ経由で解決する構造体は、「各々が有効」ではなく「参照元 ID＝指し先 ID」を明示検証せよ。自己整合性は差し替え攻撃を防がない。
  * 敵対的テストには malformed 入力だけでなく「自己整合した別の有効 payload によるすり替え」を必ず含めよ。
  * 検証失敗時は副作用ゼロ（ファイル・pointer 不変）を保証し、破損状態を上書きで隠蔽するな。

---
