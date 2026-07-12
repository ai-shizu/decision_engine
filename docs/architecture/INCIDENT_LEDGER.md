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

## INCIDENT: `INC-PHASE4A-05`
* **DATE**: 2026-07-12
* **MODULE**: Phase 4-A (RetrievalManifest persistence failure containment)
* **SYMPTOM (症状)**:
  * `_bounded_context()` が `save_retrieval_manifest()` の store integrity / I/O 失敗を隔離せず伝播させ、面接・GD・講評・debrief の主機能が観測ストア破損で恒久停止した。
  * 通常 `mode="consult"` は `_bounded_context()` を使わない（監査原文の影響範囲は過大だった）。
* **ROOT CAUSE (エージェントの思考エラー)**:
  * INC-03/04 の hard-fail を「全呼び出し経路で裸伝播」と誤読し、feature caller 側の typed isolation 空白を残した。
  * 生成 Manifest の validation failure と、永続ストア破損・I/O 失敗を同一の停止条件として扱った。
* **ARCHITECTURAL RULING (絶対裁定)**:
  * 永続化層自身（`save_retrieval_manifest` / `load_latest_retrieval_manifest`）は hard-fail を維持する。corrupt store の修復・削除・上書き・`NO_MANIFEST` 化を禁じる。
  * feature caller（`_bounded_context`）は typed `RetrievalManifestPersistenceError` だけを隔離できる。隔離は修復ではなく、検証済み in-memory context の利用継続だけを許可する。
  * 生成直後の `validate_manifest()` 失敗と、未知例外（`RuntimeError` 等）は隔離禁止・即時伝播。
  * 隔離時は固定警告を status callback と非機密 stderr へ各1回出す。query / path / manifest / 元例外文言を埋め込まない。
  * latest 更新前に確定した有効 immutable orphan は削除しない。pointer は旧値のまま残してよい。
* **PREVENTION INSTRUCTION (今後のメタ・プロンプトに組み込むべき防衛命令)**:
  * hard-fail 契約と feature 可用性を混同するな。永続層は厳格、caller は typed persistence error のみ隔離。
  * `except Exception` / bare except で persistence を握り潰すな。未知例外は伝播せよ。
  * 破損ストアを「直してから続行」するな。警告して検証済み context だけで進め。

---

## INCIDENT: `INC-PHASE4A-06`
* **DATE**: 2026-07-12
* **MODULE**: Phase 4-A (bounded retrieval manifest retention)
* **SYMPTOM (症状)**:
  * `retrieval_manifests/<hex32>.json` がターン毎に無制限蓄積し、保持上限・削除方針が文書にもコードにも無かった。
* **ROOT CAUSE (エージェントの思考エラー)**:
  * INC-03/04 の「immutable」を「無期限保存」と誤読し、完全性(非改変)と保持寿命を混同した。
  * retention を未裁定のまま latest 読出だけを実装し、ライフサイクル空白を残した。
* **ARCHITECTURAL RULING (絶対裁定)**:
  * immutable は**保持中の非改変性**であり、無期限保存を意味しない。
  * 保持は `latest` が指す immutable + 直近の valid owned history に限定する (`RETRIEVAL_MANIFEST_RETENTION_LIMIT=256`)。
  * owned は `<32-char lowercase hex>.json` のみ。`latest.json` / `.tmp` / non-hex / 未知ファイルは削除しない。
  * latest 指す payload は常に保護。残りは `(st_mtime_ns, filename)` 降順で選択。mtime は削除候補順だけで、integrity・ID・履歴意味論の根拠にしない。
  * retained file の書換えは禁止。上限超過時の **valid 旧ファイル削除だけ**を許可する。
  * corrupt / ID 不一致 / symlink は retention 対象でも削除も修復もせず hard-fail（削除ゼロ）。
  * cleanup は新 immutable と latest pointer の commit **成功後**のみ。latest 更新失敗時は cleanup 禁止（有効 orphan 保持契約を維持）。
  * unlink 部分失敗は valid 旧履歴の部分削除を許容するが、latest と新 immutable はロールバックしない。次回成功 save で再度 prune 可能。
  * load 経路で prune / 修復 / 削除を行わない。startup cleanup / background thread / UI 設定を追加しない。
* **PREVENTION INSTRUCTION (今後のメタ・プロンプトに組み込むべき防衛命令)**:
  * 「消さない」と「無制限に残す」を同一視するな。非改変と有限保持は両立する。
  * prune を latest 更新前に置くな。corrupt を掃除のついでに消すな。
  * retention failure を理由に latest を旧値へ戻すな。typed persistence isolation で主機能を継続せよ。

---

## INCIDENT: `INC-ENGINE-LOG-01`
* **DATE**: 2026-07-12
* **MODULE**: Tauri sidecar / `engine.rs` + `engine_stdio.py` (Finding 12)
* **SYMPTOM (症状)**:
  * 子エンジン stderr を `logs/engine.log` へ無制限 append。ローテーションなし。
  * 失敗リクエスト毎に traceback・例外値・ユーザー由来文字列が永続ログへ残る経路があった。
* **ROOT CAUSE (エージェントの思考エラー)**:
  * 「stderr をファイルへ逃がせば stdout JSON は守れる」で止まり、永続ログの内容規律と有限保持を設計しなかった。
  * library print の stderr 退避と、診断の永続化を同一パイプに直結した。
* **ARCHITECTURAL RULING (絶対裁定)**:
  * `engine.log` は raw stderr dump ではない。永続化は exact allowlist 行のみ:
    `[PKB_DIAG_V1] REQUEST_FAILED`（LF、または終端のみの CRLF→LF。marker 内部の CR は拒否。prefix/suffix 不可）。
  * 有限保持: 各ファイル最大 `1_048_576` bytes、`engine.log` + `.1` + `.2`（合計最大 3 MiB）。各ファイル先頭に `[PKB_ENGINE_LOG_V1]` header（サイズ算入）。
  * **安全判定は header だけでは足りない。** header 以降の全行が allowlist 行（`[PKB_DIAG_V1] REQUEST_FAILED\n`）であること。1行でも逸脱すれば unsafe → reset（内容の backup 継承禁止）。
  * **存在・種別の裁定は `symlink_metadata` のみ。** `Path::exists()` 禁止（dangling symlink を欠落と誤認し `File::create` がリンク先を作る罠）。symlink / 非regular は Remove→V1 新規作成。
  * traceback・exception class/message・path・cmd・id・params・query・本文・JSON断片を永続化しない。
  * legacy/unsafe な owned log（header欠落・本文allowlist逸脱・上限超過・非regular・symlink・読取不能）は新ログへコピーも backup 継承もしない。未知ファイルは削除しない。
  * logger open/write/rotate 失敗時: 永続化だけ fail-closed。stderr pipe は drain 継続。raw stderr fallback 禁止。engine 起動をログ都合で止めない。
  * stdout は JSON Lines 専用契約を維持。IPC 失敗応答の既存 `error` 文字列 shape は維持（Finding 13 = UI表示規律は別裁定）。
* **PREVENTION INSTRUCTION (今後のメタ・プロンプトに組み込むべき防衛命令)**:
  * 子 stderr を File に直結するな。allowlist collector + bounded rotation を通せ。
  * 診断に例外オブジェクトを文字列連結するな。固定行だけを書け。
  * Finding 13（UI へ生例外）と本裁定を混同するな。

---

## INCIDENT: `INC-UI-ERROR-01`
* **DATE**: 2026-07-12
* **MODULE**: Desktop React tabs (Finding 13) / `uiErrorMessages.ts`
* **SYMPTOM (症状)**:
  * 7コンポーネント22箇所で `String(err)` により backend の `"{ExceptionType}: {message}"` が UI へ表示されていた。
  * Phase 4-A Context Observatory のみ固定文言規律で、同一アプリ内のエラー表示が二重基準だった。
* **ROOT CAUSE (エージェントの思考エラー)**:
  * transport の生エラーと UI 表示を同一経路とみなし、catch 値をそのまま state へ運んだ。
  * Finding 2 の replay/NoReplay とユーザー向け回復案内を接続しなかった。
* **ARCHITECTURAL RULING (絶対裁定)**:
  * UI catch は例外値を state / message / log / DOM / aria / console へ渡さない。
  * 表示は operation-keyed 固定文言のみ (`uiErrorMessages.ts` exact 21 keys)。error 引数・解析・分類・substring/regex/class 判定禁止。
  * Finding 2 `replay_policy` と整合: retry-safe は即時再試行案内可、NoReplay 相当は verify-first（「必要な場合だけ再実行」）。error 文字列を見て retry-safe へ昇格するな。
  * 自動再試行・polling・timer retry・hidden resend・reload 禁止。利用者が明示ボタンで再実行する構造を維持。
  * progress=`role="status"`、error=`role="alert"` + `error-text`。error kind を内容から推測するな。
  * Import error 行に filename / path / backend message を連結するな。
  * Record save error は 3 秒 auto-hide しない（成功通知の auto-hide は維持）。
  * Finding 12 境界: backend/IPC/`engine_stdio`/`engine.ts`/Rust の raw error は変更しない。UI だけ無菌化する。
* **PREVENTION INSTRUCTION (今後のメタ・プロンプトに組み込むべき防衛命令)**:
  * 新規 catch に `String(err)` / `err.message` を書くな。先に UiErrorCode を追加せよ。
  * IPC schema を「UI を綺麗にするため」に変えるな。表示層で固定文言へ置換せよ。

---

## INCIDENT: `INC-LLM-CLIENT-01`
* **DATE**: 2026-07-12
* **MODULE**: llama-server HTTP client (Finding 14) / `core/llm_backend.py`
* **SYMPTOM (症状)**:
  * `consultation_engine.py` と `cli.py` がそれぞれ `LlamaServerBackend` を持ち、`/health`・`/v1/chat/completions`・process lifecycle を二重実装していた。
  * CLI 側だけ `temperature=0.6` / `max_tokens=900` / `-c 8192` を直書きし、`llm_config.generation_params()` と drift し得た。
* **ROOT CAUSE (エージェントの思考エラー)**:
  * デスクトップ移行後もレガシー CLI を「消すか丸ごと委譲するか」の二項で考え、transport だけを共有する抽出を怠った。
  * 生成パラメータの単一真実（`llm_config`）を HTTP クライアント実装の単一所有と混同し、クライアント重複を放置した。
* **ARCHITECTURAL RULING (絶対裁定)**:
  * llama-server client の唯一所有者は `core/llm_backend.py`（`class LlamaServerBackend`・loopback probe・spawn/stop・`/health`・`/v1/chat/completions`・SSE・structured・KV slot 連携）。
  * model / port / server command / ctx / generation / startup timeout / slot-save capability の正本は引き続き `llm_config.py`（`config/model_params.json`）。生成パラメータをクライアントへ複製するな。
  * `consultation_engine` と `cli` は shared class を import するだけ。独自 urllib/socket HTTP を書くな。host は固定 `127.0.0.1`。remote / cloud / requests/httpx 禁止。
  * CLI 入口（`app.py` / `cli.py`）・固有 retrieval/prompt・`LlamaCliBackend`・`RuleBasedBackend`・`--show-prompt` / `--top-k` は維持。古いという理由だけで CLI を削除するな。
  * CLI fallback（`LlamaCliBackend`）も `generation_params()` と `LLAMA_CTX` を使え。`0.6` / `900` / `8192` 直書き禁止。相談用途の model は `find_gguf(role="consult")`。
  * process ownership: 自分が spawn した process のみ terminate/kill。外部所有 server 再利用時は `proc is None` のまま `stop()` で殺すな。timeout 時は self-owned のみ stop。
  * 契約テストは networkless fake transport のみ（`tests/test_llm_client_single_owner.py`）。実 model / 実 server / 外部 network を起動するな。
* **PREVENTION INSTRUCTION (今後のメタ・プロンプトに組み込むべき防衛命令)**:
  * `/health` や `/v1/chat/completions` を `llm_backend.py` 以外へ書くな。新規呼び出しは shared class を import せよ。
  * CLI を触るときは「入口互換」と「transport 所有」を分離せよ。prompt/retrieval 統合や CLI 削除を Finding 14 の延長でやるな。

---
