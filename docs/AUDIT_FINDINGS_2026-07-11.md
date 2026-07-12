# PKB 読み取り専用監査 Findings

> 監査日: 2026-07-11
> 状態: 未解決バックログ。項目順に個別対応する。
> 出典: Fable5 による読み取り専用静的監査。`CONFIRMED` と `POTENTIAL` の区分を保持する。
監査範囲を広げます。まずRust層(IPC契約の対向側)、paths.pyのimport副作用、フロント各タブの規律違反パターン、TUIのAPIドリフトを並行調査します。

lookaheadは非対応の可能性があるため、単純パターンで再確認します。

### [P2] interview_report.v1 の tensor_profile フィールドがフロントエンド契約から欠落し、実測6Dプロファイルが到達不能

- 状態: `RESOLVED`(レビュー是正1〜4適用済み)
- 再開理由: 独立レビューにより4件の欠陥が確認された。(1) `report.config`が検証キーからの再構築ではなくbare castで受理されている、(2) Romance parserが固定tendency/action allowlistおよびnull↔非null分岐の整合性を検証していない、(3) Romance payloadがromance_analysisモードで許可されるだけでrequiredにならない、(4) 恒久pytestがdispatch件数とPython 2ファイルのSHA-256という時点依存値を固定しており意味契約を検証していない、(5) 新規追加CSSに`1px`/`480px`のリテラル値が残存。以下は前回のRESOLVED記録であり、削除せず保持する。
- 解決日(前回): 2026-07-11
- 根本原因: Python契約拡張(`interview_report.py`への`tensor_profile`添付)がTypeScript型契約・runtime境界・UIへ伝播しなかった。`engine.ts::consult()`が`pkbInvoke("consult", ...)`の型引数なし呼び出し(実質`unknown`をそのまま構造的に信頼する経路)で応答を受けており、`InterviewReport`型に`tensor_profile`が存在しないため、実測データがランタイムに届いても静的に参照不能だった。
- 変更ファイル:
  - 新規: `apps/desktop/src/lib/parseConsultResponse.ts`(consult応答全体のstrict runtime parser。tensor_profileのschema・固定6軸順・dimension_id/calculus_axis対応・score/confidence範囲とround2・evidence exact key・indicator allowlist・重複禁止・first-five-dimensions candidate限定・score成立条件とconfidence会計の再計算照合を実装。Python `core/tensor_profile.py`/`core/interview_report.py`/`core/romance_analysis.py`の鏡像)
  - 新規: `apps/desktop/src/lib/tensorProfileView.ts`(検証済み構造体→表示用データの純関数変換。ProfileTab定数は非import)
  - 新規: `apps/desktop/src/components/TensorProfilePanel.tsx`(TENSOR_PROFILE_6D / SESSION MEASUREMENT パネル。evidence本文・speaker・turn・indicator非表示)
  - 変更: `apps/desktop/src/lib/types.ts`(`TensorDimensionId`/`TensorEvidenceV1`/`TensorDimensionV1`/`TensorProfileReportV1`追加、`InterviewReport.tensor_profile`をrequired化)
  - 変更: `apps/desktop/src/lib/engine.ts`(`consult()`を`pkbInvoke<unknown>` → `parseConsultResponse`経路へ変更。`RomanceAnalysisResult`型はparseConsultResponse.tsから再export し既存import元を維持)
  - 変更: `apps/desktop/src/components/InterviewTab.tsx`(`TensorProfilePanel`をMISSION_RESULT内、metrics/evidenceの後・latencyの前にマウント。interview_sim/gd_sim共通の単一render経路)
  - 変更: `apps/desktop/src/App.css`(`.tensor-profile-panel`等新設、`.tensor-radar-legend`に狭幅1列化のmedia query追加。新規hex色・literal px・正のletter-spacing・animationなし)
  - 新規: `apps/desktop/tests-runtime/consult_tensor_boundary.test.ts`(RED先行、33件)
  - 新規: `apps/desktop/tests-runtime/gen_tensor_report_fixture.py`(越境golden。実際の`aggregate_profile`/`report_tensor_field`を呼び出し)
  - 変更: `apps/desktop/tsconfig.boundary.json`(新規ファイルをinclude追加)
  - 新規: `apps/desktop/scripts/run-boundary-tests.ps1`(既存42件+新規33件を単一入口で実行、finally節で生成物削除、失敗時exit 1)
  - 新規: `tests/test_tensor_profile_ui_contract.py`(静的source-inspection、11件。core 2ファイルのSHA-256ピン留め含む)
  - 変更: `docs/AUDIT_FINDINGS_2026-07-11.md`(本エントリ)
- 検証: 実行した全コマンドと件数:
  - `pytest tests/test_engine_tensor_profiling_backend_contract.py tests/test_tensor_profile_ui_contract.py -q` → 38 passed
  - `pytest tests/test_engine_tensor_profiling_backend_contract.py tests/test_tensor_profile_ui_contract.py tests/test_engine_tensor_profiling_ui_contract.py -q` → 48 passed
  - `apps/desktop/scripts/run-boundary-tests.ps1` → manifest_boundary 42/42、consult_tensor_boundary 33/33(越境golden T-33含む)、`BOUNDARY_EXIT=0`
  - `npx tsc --noEmit`(apps/desktop) → exit 0
  - `npm run build`(apps/desktop) → 67 modules transformed、881ms、exit 0
  - `git diff --check` → exit 0(クリーン)
  - `git status --short data` → 空(クリーン)
  - `git diff -- package.json package-lock.json apps/desktop/package.json apps/desktop/package-lock.json` → 空(差分なし)
  - `git diff --stat` で `src/python/core/**`、`engine_stdio.py`、`ProfileTab.tsx`、`MbtiGradientBars.tsx`、`ContextObservatory*.tsx`、`manifest.ts`/`manifestFetchState.ts`/`parseManifest.ts` の無変更を個別確認
- 結果: 全GREEN。ただし `pytest tests/ -q`(全ディレクトリ)実行時、`tests/test_oracle.py::test_stdio_dispatch_ignores_unknown_params` が1件失敗した。これは本finding対象外の既存欠陥であり、単体実行でも同一失敗を再現し、`engine_stdio.py`/`core/facade.py`/`test_oracle.py`のいずれも本session内でgit diff皆無(byte-identical)であることを確認した — Phase 3-Bの`_FakeFacade`モックに`last_romance_analysis`が欠けている、本findingと無関係な既存回帰。HANDOFF §10の凍結GREENゲート対象7スイートにも`test_oracle.py`は含まれない。
- commit: `UNCOMMITTED`
- 残存リスク(前回時点): `tests/test_oracle.py::test_stdio_dispatch_ignores_unknown_params` は本finding着手前から存在する無関係な既存失敗であり、本作業では対象外のため未修正のまま残る(別findingとして扱うべき)。それ以外に確認された残存リスクなし。

#### レビュー是正1〜4 追補

- 解決日: 2026-07-11(同日追補)
- 根本原因(追補分): (1) `report.config`はexactキー検証を経ずに`raw.config as InterviewReport["config"]`でbare castされていた — 「型」の検証はしても「値」を検証キーから再構築していなかった。(2)(3) Romance parserはPython `romance_analysis.py`の`ROMANCE_ANALYSIS_SCHEMA`(固定tendency/action enum)と`validate_result()`のnull↔insufficient結合規則を鏡像化せず、`schema`+`affinity_score`範囲のみで「shapeが正しければ受理」していた。また`romance_analysis`キーの許可判定(`hasRomance && !romanceAllowed`)はキー不在側の必須化を欠いていた。(4) 恒久pytestがdispatch件数26・core 2ファイルのSHA-256という時点依存値をハードコードし、当該ファイルが将来正当な理由で変更されるたびに無関係な理由で赤化する、または逆に本来の意味契約(禁止領域無変更・意味的no-direct-IPC)を検証しないまま緑化する構造だった。(5) CSS新設時に既存コードベースの`thin`/`rem`規約(`1px solid`は境界線の慣用表現として容認されるが、レスポンシブbreakpointは他箇所に前例なし)を確認せず`480px`を書いた。
- 変更ファイル(追補分・許可範囲外は無変更):
  - `docs/AUDIT_FINDINGS_2026-07-11.md`(本追補)
  - `apps/desktop/tests-runtime/consult_tensor_boundary.test.ts`(T-34〜T-54、21件追加。T-05の旧仮fixtureをPython固定allowlistの有効値へ更新。T-01〜T-04・T-06〜T-33の検証内容は不変)
  - `tests/test_tensor_profile_ui_contract.py`(`test_no_new_engine_command_registered`・`test_python_core_tensor_and_interview_report_untouched`を削除。`test_snapshot_specific_guards_are_absent`・`test_tensor_panel_uses_existing_report_without_direct_ipc`・`test_tensor_profile_css_blocks_have_no_px`・`test_tensor_profile_summary_uses_thin_border`・`test_tensor_radar_narrow_query_uses_rem`を追加。CSS抽出はbrace-depthカウントの小ヘルパー`_css_block()`を使用、全文regexは不使用)
  - `apps/desktop/src/lib/parseConsultResponse.ts`:
    - `parseInterviewConfig(raw, path)`新設。許可キー`industry/genre/difficulty/stance/customTheme`の部分集合のみを受理し、各fieldをraw参照から個別に読み出して新規objectへ書き出す。string系3fieldは空文字を許可(UI・Python契約双方の正常系)。difficulty/stanceはtype predicateで絞り込み、config再構築経路のcastを排除。
    - Romance側: `ROMANCE_SCHEMA`/`ROMANCE_ALLOWED_TENDENCIES`/`ROMANCE_ALLOWED_ACTIONS`/`ROMANCE_INSUFFICIENT_TENDENCY`/`ROMANCE_INSUFFICIENT_ACTION`をliteral tupleとして定義し、`isRomanceTendency`/`isRomanceAction`type predicateでruntime allowlist検証。`affinity_score===null ⇔ tendency/actionがinsufficient文言`の結合を強制。
    - `ConsultResponse`をmode別discriminated unionへ変更(`consult`/`es_review`は`report`/`romance_analysis`とも`never`、`interview_sim`/`gd_sim`は`report`のみoptional、`romance_analysis`branchは`romance_analysis`をrequired)。`romanceRequired && !hasRomance`を追加しromance_analysisモードでのpayload欠落を拒否。
  - `apps/desktop/src/App.css`: `.tensor-profile-summary-row`の`border-bottom`を`1px solid`→`thin solid`、`.tensor-radar-legend`のmedia queryを`max-width: 480px`→`max-width: 30rem`。新規tensor-profile面以外(`.score-bar`・`.tensor-radar-legend-row`等の既存CSS)は無変更のまま(px使用を維持)。
  - `types.ts`・`engine.ts`・`tsconfig.boundary.json`・`run-boundary-tests.ps1`・React components(`InterviewTab.tsx`等)・Python・package filesは本追補ラウンドで**無変更**(前回RESOLVED時点の内容のまま)。
- 検証(追補分): 実行した全コマンドと件数:
  - `pytest tests/test_engine_tensor_profiling_backend_contract.py tests/test_tensor_profile_ui_contract.py tests/test_engine_tensor_profiling_ui_contract.py -q` → **51 passed**(最終レビューで無関係なlegacy CSSの`px`存続を要求する負例テスト1件を削除後)
  - `apps/desktop/scripts/run-boundary-tests.ps1` → manifest_boundary 42/42、consult_tensor_boundary **54/54**(合計**96/96**)、`BOUNDARY_EXIT=0`
  - `npx tsc --noEmit`(apps/desktop) → exit 0(discriminated union化後もConsultTab.tsx/RomanceAnalysisPanel.tsx/InterviewTab.tsxは無変更で型適合)
  - `npm run build`(apps/desktop) → 67 modules transformed、888ms、exit 0
  - `git diff --check` → exit 0
  - `git status --short data` → 空
  - `git diff -- package.json package-lock.json apps/desktop/package.json apps/desktop/package-lock.json` → 空
  - `git diff -- src/python/core src/python/engine_stdio.py apps/desktop/src/components/ProfileTab.tsx apps/desktop/src/components/MbtiGradientBars.tsx apps/desktop/src/components/ContextObservatory.tsx apps/desktop/src/components/ContextObservatoryContainer.tsx apps/desktop/src/lib/manifest.ts apps/desktop/src/lib/manifestFetchState.ts apps/desktop/src/lib/parseManifest.ts` → 空(禁止領域無変更)
- 結果:
  ```text
  対象ゲート: 全GREEN
  全tests/には既知のスコープ外失敗1件(tests/test_oracle.py::test_stdio_dispatch_ignores_unknown_params。前回記録のとおり本findingと無関係、byte-identical、対象外)
  commit: UNCOMMITTED
  ```
- 残存リスク(追補後): Romance parserの「非null scoreと特定tendency/actionの因果的一致」(`validate_result()`の`expected_tendency`/`expected_action`引数が担う検証)は、フロントエンドが元metricsを持たないため検証範囲外のまま(意図的なスコープ外— 固定allowlist・null↔insufficient結合という構造的整合性は検証済み)。`tests/test_oracle.py`の既存失敗1件は引き続き別findingとして残存。それ以外に確認された残存リスクなし。

#### 最終レビュー是正 (2026-07-12)

- `parseInterviewConfig()`のdifficulty/stanceを明示的type predicateで絞り込み、config再構築経路に残っていた`as InterviewConfig[...]`を除去。
- 無関係なlegacy CSSに`px`が残ることを恒久条件にしていた`test_unrelated_legacy_css_px_does_not_affect_scope`を削除。新規tensor-profile selectorと`30rem` media queryだけを検査する意味契約は維持。
- 記録訂正: boundary T-05はRomance固定allowlist導入に伴い有効fixtureへ更新されており、「T-01〜T-33は無変更」ではない。T-34〜T-54の21件追加に加え、T-05 fixture更新として記録を訂正。
- 最終検証: 対象Python **51 passed**、manifest boundary 42/42 + consult tensor boundary 54/54 = **96/96 GREEN**、`tsc --noEmit` PASS、Vite build PASS (67 modules)、`git diff --check`・data・package差分なし。
- 状態: `RESOLVED`。commit: `UNCOMMITTED`。
- 確度: `CONFIRMED`
- 分類: 2(言語間契約不一致)、6(UIから利用できないバックエンド機能)、18(黙って破棄されるフィールド)
- 問題事象: バックエンドは面接講評の組み立てで毎回 `report["tensor_profile"]`(schema `tensor_profile.6d.v1`、28件の契約テストで検収済み)を添付してIPCで送出しているが、フロントエンドの型 `InterviewReport` は当該フィールドを宣言しておらず、いかなるコンポーネントも描画しない。実測6Dプロファイルはランタイムでフロントまで届いた後、参照されずに消える。
- 発生条件: 面接シミュレーション講評が生成される全ケース(常時)。
- 実害または潜在的影響: Phase 2 の主要成果である実測6Dテンソルプロファイルをユーザーが一度も閲覧できない。型契約が実IPCペイロードのサブセットであるため、将来この構造を扱う実装は契約から実データ形状を知り得ない。
- 根本原因: バックエンド契約(Python + 契約テスト)拡張時に、TypeScript 側の型契約と描画層が追従しなかった。
- 証拠:
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\interview_report.py:397` — `report["tensor_profile"] = report_tensor_field(profile)`(全講評に添付)
  - `C:\Users\badger\Documents\cursur\decision_engine\tests\test_engine_tensor_profiling_backend_contract.py:427` および `:505-506` — IPC応答内 `report["tensor_profile"]["dimensions"]` を契約として検証
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\lib\types.ts:183-191` — `InterviewReport` に `tensor_profile` フィールドなし
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\components\InterviewTab.tsx:806-825` — 描画は `report.metrics` / `report.latency` のみ
- 契約または文書との矛盾: `types.ts:178-182` の W-37 注記は「UIはこの構造体をstdio応答からそのまま受け取る」と述べるが、実際の応答構造の一部が型に存在しない。
- 静的監査だけでは確定できない点: なし(送出・型欠落・非描画はすべて静的に確定)。

### [P2] Rust IPC層のパイプ断自動リトライが非冪等コマンドを二重実行し得る

- 状態: `RESOLVED`
- 解決日: `2026-07-12`
- 確度: `CONFIRMED`
- 分類: 1(潜在的回帰)、3(IPC境界の欠陥)
- 問題事象: `EngineManager::invoke()` はパイプエラー検知時にエンジンを再起動し、**同一コマンドを同一パラメータで自動再送**する。Python側がリクエストを処理し副作用(ファイル追記・ログ保存・ノード追加)を完了した後、応答行がRustへ届く前にプロセスが死んだ場合、再送によって副作用が二重実行される。
- 発生条件: 副作用を持つコマンド(`import.line` のLINE履歴追記、`probe.answer` のappend-only HistoricalNode追加、`consult` の相談ログ保存、`calendar.sync` のマージ)の処理完了後〜応答送達前に、エンジンプロセスのクラッシュ/パイプ断が発生した場合。
- 実害または潜在的影響: append-only設計のPROBE台帳への同一回答の二重記録、相談ログの重複エントリ、カレンダーの重複マージ。LINE追記はprofiler側の多重集合デデュープ(AI_SKILLS §14)で影響が緩和される可能性があるが、他の経路に同等の防御は確認できない。
- 根本原因(監査時点 / 解決対象):
  - pipe errorを「未処理」と仮定した無条件再送
  - string error分類(`is_pipe_error`)
  - 未使用`_allow_restart`
  - commandごとの副作用性を輸送層が区別していなかった
- 解決方式:
  - typed `TransportFailure` / `ProtocolFailure` / `InvokeError`
  - 26 commandの明示的policy表(`replay_policy`)
  - unknown commandは`NoReplay`(`_` arm)
  - `NoReplay`: restartのみ、再送せず結果不明エラー(`MSG_OUTCOME_UNKNOWN`)
  - retry-safe(`RetryOnceAfterRestart`): restart後に最大1回だけ再送
  - Remote/NotReadyはfail-fast(restartなし)
  - `invoke_sync`を単発attempt primitive化(3引数、`_allow_restart`削除)
  - 3回目attemptを構造的に排除(`invoke_blocking`に再帰・ループなし)
- command分類:
  - RetryOnceAfterRestart:
    `health`, `settings.get`, `record.load`, `calendar.event_dates`, `import.stats`, `es.view`, `import.classify`, `oracle.payload`, `twin.forecast`, `profile.source_code`, `probe.status`, `context.manifest.latest`
  - NoReplay:
    `settings.save_fixed`, `record.save`, `consult`, `knowledge.fetch_pending`, `calendar.sync`, `import.line`, `import.document`, `settings.run_profiler`, `narrative.compile`, `oracle.report`, `tensor.rebuild`, `probe.next`, `probe.answer`, `shutdown`
    (および未知コマンドすべて)
- 変更ファイル:
  - `apps/desktop/src-tauri/src/engine.rs`
  - `apps/desktop/src-tauri/src/engine_tests.rs`
  - `apps/desktop/src-tauri/build.rs`
  - `docs/AUDIT_FINDINGS_2026-07-11.md`
- Windowsテスト環境是正(STEP 3.5):
  - libユニットテストexeへのCommon-Controls v6 manifest埋め込み(`cargo::rustc-link-arg=/MANIFEST:EMBED` + `MANIFESTDEPENDENCY`)
  - `WindowsAttributes::new_without_app_manifest()`によるbin側Tauri app manifest重複防止(アイコン・バージョン等の`resource.lib`は維持)
  - `STATUS_ENTRYPOINT_NOT_FOUND`(0xC0000139)解消
- 検証:
  - Rust `cargo test` → **27 passed / 0 failed**(lib unit; bin/doc 0)
  - `cargo build` / `cargo check` → GREEN(許容warning: 未使用`restart`のみ)
  - `npx tsc --noEmit`(apps/desktop) → GREEN
  - `npm run build`(apps/desktop) → Vite GREEN(67 modules)
  - `git diff --check` → GREEN
  - Cargo.toml / Cargo.lock / package*.json → 無差分
  - `git status --short data` → 空
  - 禁止パターン(`restart_then_unconditional_resend` / `unconditional resend` / `is_pipe_error` / `_allow_restart`) → engine.rs 内 0件
- commit: `UNCOMMITTED`
- 残存リスク:
  - NoReplayの通信断では処理結果が意図的に不明となる(`MSG_OUTCOME_UNKNOWN`)
  - exactly-onceや永続dedup ledgerは導入していない
  - 利用者は状態確認後に手動再実行する必要がある
- 契約または文書との矛盾(監査時点): AI_SKILLS §14(IMP-2)は取込APIの冪等性を規律とするが、輸送層の再送はどの文書にも規定がない。
- 静的監査だけでは確定できない点(監査時点): 「処理完了後・応答前」のクラッシュウィンドウが実際に到達可能か、および `merge_calendar` 等の下流デデュープ有無は、障害注入テストなしに確定できない。→ 本解決で27件のfault-injectionユニットテストにより輸送層の再送制御は実行検証済み。下流デデュープの網羅は本finding範囲外。
- 証拠(監査時点の証拠 — 行番号は当時のコード位置。現行実装では置換済み):
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src-tauri\src\engine.rs:135-143` — pipe error → `restart()` → `invoke_sync(cmd, params, cid, false)` 再送
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src-tauri\src\engine.rs:42-48` — リトライ対象エラーの判定(処理済み/未処理を区別しない)
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src-tauri\src\engine.rs:151` — `_allow_restart` 未使用
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\facade.py:387` — `import.line` の無条件追記

### [P2] Manifest永続化失敗が consult 全体を停止させる(観測計装が主機能の単一障害点)

- 状態: `RESOLVED`
- 解決日: `2026-07-12`
- 確度: `CONFIRMED`
- 分類: 3(永続化境界)、1(潜在的回帰)
- 問題事象(監査時点の原文): `_bounded_context()` はコンテキスト構築の直後、プロンプト生成・LLM呼び出しの前に `save_retrieval_manifest(manifest)` を例外隔離なしで呼ぶ。保存経路は既存immutableファイルとのID衝突・内容不一致、pointer異常、ディスクI/O失敗で `ValueError` を送出するため、観測ストアの破損が相談機能そのものを恒久的に失敗させる。
- 監査訂正: 通常の `mode="consult"` は `_bounded_context()` を使用しない。実影響範囲は `interview_sim`・`gd_sim`・両者の講評・`debrief` に限られる。
- 発生条件: `data/processed/retrieval_manifests/` 内のファイル破損・部分書込・手動改変、またはディスク障害。
- 実害または潜在的影響: 面接/GD/講評/debrief が例外で失敗し続け、UIからの復旧手段が存在しない(ストアの手動削除が必要)。計装(observability)の障害が本体機能の可用性を毀損する。
- 根本原因: INC-PHASE4A-03/04 の裁定はmanifest層のhard-failを正しく強制しているが、その障害の波及範囲(feature caller を巻き込むか否か)はどの文書でも裁定されておらず、呼び出し側に typed isolation がなかった。
- 解決方式:
  - `RetrievalManifestPersistenceError` (typed; `ValueError` サブクラス)
  - `save_retrieval_manifest`: 入力 validate は storage `try` 外。書込前に既存 latest pointer+payload を厳格検証。atomic `.tmp`+`os.replace`。`OSError`/`ValueError` のみ固定文言へ変換 (`raise ... from exc`)
  - `_bounded_context`: validate → working_memory 更新 → save。typed persistence error のみ隔離。固定 stderr + status 警告各1回。検証済み context を返して継続
  - `context.manifest.latest` / load 経路の corrupt hard-fail は不変
  - corrupt store の修復・削除・上書き・`NO_MANIFEST` 化は行わない
- 警告経路:
  - status: `コンテキスト監査記録を保存できませんでした。相談処理は継続します。`
  - stderr: `[PKB] retrieval manifest persistence failed; continuing without manifest update`
- fault-injection結果:
  - corrupt immutable / corrupt pointer でも `_bounded_context` 継続、store bytes 不変、`.tmp` 残骸なし
  - latest replace 失敗時は有効 immutable orphan 残存・pointer 旧値維持
  - validate 失敗・`RuntimeError` は伝播
  - 正常保存時は警告ゼロで latest 更新
- 変更ファイル:
  - `src/python/core/retrieval_manifest.py`
  - `src/python/core/consultation_engine.py`
  - `tests/test_manifest_persistence_isolation.py` (新規)
  - `tests/test_retrieval_manifest.py`
  - `docs/architecture/INCIDENT_LEDGER.md` (INC-PHASE4A-05)
  - `docs/AI_SKILLS.md` (§16.5)
  - `docs/AUDIT_FINDINGS_2026-07-11.md`
- 検証:
  - `pytest tests/test_manifest_persistence_isolation.py tests/test_retrieval_manifest.py` → 93 passed
  - `pytest tests/test_context_manifest_ipc.py tests/test_integration.py tests/test_engine_tensor_profiling_backend_contract.py tests/test_feature_romance_backend_contract.py` → 107 passed
  - `py_compile` retrieval_manifest / consultation_engine → OK
  - boundary suites → 42+54=96 GREEN
  - `tsc --noEmit` / Vite build → GREEN
  - `cargo test` 27/27 / `cargo check` → GREEN
  - `git diff --check` / data / package / Cargo → 無差分(本finding範囲外の既存差分は温存)
- commit: `UNCOMMITTED`
- 残存リスク:
  - Manifest保存に失敗したターンはContext Observatoryへ記録されない
  - latest更新失敗時に有効immutable orphanが残り得る
  - 自動修復・再試行・削除は意図的に行わない
- 証拠(監査時点の証拠 — 行番号は当時のコード位置):
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\consultation_engine.py:1162-1163` — `validate_manifest(manifest)` / `save_retrieval_manifest(manifest)` を裸で呼ぶ
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\retrieval_manifest.py:794-797` — 既存ファイル読込・照合(不一致で `ValueError`)
- 契約または文書との矛盾(監査時点): 矛盾ではなく裁定の空白。INC-04は「修復せず `ValueError`」を命じるが、feature caller の障害隔離については無規定 → INC-PHASE4A-05 で裁定。
- 静的監査だけでは確定できない点(監査時点): 破損ストア下での実際の失敗挙動と `.tmp` 経路 → fault-injection で実行検証済み。

### [P2] docs/CONTEXT.md が全面的に陳腐化したまま公式参照連鎖に残存

- 状態: `RESOLVED`
- 解決日: `2026-07-12`
- 確度: `CONFIRMED`
- 分類: 9(文書と実装の矛盾)、10(完了済み項目の未完了記載)、16(現在状態の陳腐化)、15(軽量モデルが誤解する手順)
- 問題事象(監査時点の原文): 2026-07-06版のまま更新されておらず、現在のシステム形状と多数の点で矛盾する。AI_SKILLS の冒頭が「迷ったら本書 → HANDOFF → CONTEXT.md の順に参照せよ」と公式参照先に指定しているため、エージェントが古い状態を現在として誤認する経路が生きている。
- 発生条件: 新任エージェント・軽量モデルが参照連鎖に従って CONTEXT.md を読んだ場合(常時)。
- 実害または潜在的影響: 「4タブUI」「stdioコマンド10種」を前提にした設計・監査ミス。実装済み機能(streaming表示、status進捗、カレンダーマーク)を「次に実装すべきタスク」として二重実装するリスク。
- 根本原因: Phase B以降のas-built更新がHANDOFF/AI_SKILLSに集約され、CONTEXT.mdだけ更新プロセスから脱落した。volatile snapshot（タブ数・IPC件数・未実装TODO・モデル容量・Generated日付）を正本として扱わせる文書設計だった。
- 解決方式:
  - `docs/CONTEXT.md` を一時点スナップショットから **安定アーキテクチャ索引** へ全面再構築
  - 文書所有権を固定（AI_SKILLS=規律 / HANDOFF=現在地 / CONTEXT=安定索引 / INCIDENT_LEDGER=裁定 / 実コード=最終正本）
  - IPC コマンド一覧・固定件数・次タスク節・Generated日付・モデル容量等の陳腐化情報を削除
  - UI surface 表を `App.tsx` の `TABS` 順序と一致させ、document contract で動的照合
  - README「詳細ドキュメント」と AI_SKILLS 冒頭の参照連鎖を役割別案内へ修正
  - 新規 `tests/test_context_document_contract.py`（15件）で再発防止
- 削除した陳腐化情報(代表):
  - 「React 4タブ UI」「4 タブシェル」「stdioコマンド10種」および固定コマンド表
  - 「次に実装すべきタスク」節（streaming / status callback / カレンダーマークを未実装扱い）
  - `Generated: 2026-07-06`、モデル容量、テスト件数、hash 固定値
- 変更ファイル:
  - `docs/CONTEXT.md`
  - `docs/AI_SKILLS.md`（冒頭の参照案内のみ）
  - `README.md`（「詳細ドキュメント」節のみ）
  - `tests/test_context_document_contract.py`（新規）
  - `docs/AUDIT_FINDINGS_2026-07-11.md`（本エントリ）
- 検証:
  - `pytest tests/test_context_document_contract.py` → 15 passed
  - `pytest tests/test_ui_smoke.py` → GREEN
  - boundary 96/96、`tsc --noEmit`、Vite build → GREEN
  - `cargo test` 27/27、`cargo check` → GREEN
  - `git diff --check` / data / package / Cargo → クリーン（本finding範囲）
- commit: `UNCOMMITTED`
- 残存リスク:
  - タブ構造や所有者変更時は CONTEXT 更新が必要（document contract が検知）
  - command 一覧は意図的に複製せず実コード（`engine_stdio.py`）参照
  - HANDOFF 自体の陳腐化や競合命令は別 finding
- 証拠(監査時点の証拠 — 行番号・内容は当時の CONTEXT.md):
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\CONTEXT.md:30` および `:232` — 「React 4タブ UI」「4 タブシェル」 vs `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\App.tsx:27-35` — TABS 7件
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\CONTEXT.md:75-88` — stdioコマンド表10行 vs `C:\Users\badger\Documents\cursur\decision_engine\src\python\engine_stdio.py:56-195` — dispatch 26コマンド
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\CONTEXT.md:286` — 「CONSULT 回答の streaming 表示」を未実装として列挙 vs `C:\Users\badger\Documents\cursur\decision_engine\src\python\engine_stdio.py:100-102`(chunkイベント)および `apps\desktop\src\components\ConsultTab.tsx:79-81`(受信描画)
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\CONTEXT.md:285` — 「家計簿入力日・相談日のカレンダーマーク」未実装として列挙 vs `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\facade.py:272-280`(diary/finance/consult日付の集約)および `apps\desktop\src\components\RecordTab.tsx:61`(利用)
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\CONTEXT.md:295` — 「CONSULT stdio 経由の status コールバック」未実装として列挙 vs `engine_stdio.py:100-102`
  - 参照連鎖の指定: `C:\Users\badger\Documents\cursur\decision_engine\docs\AI_SKILLS.md:5`
- 契約または文書との矛盾(監査時点): 上記の各行が実装と直接矛盾。
- 静的監査だけでは確定できない点: なし。

### [P2] 完了の定義(DoD)が境界42テストを含まず、その実行手順がリポジトリのどこにも存在しない

- 状態: `RESOLVED`
- 解決日: `2026-07-12`
- 着手日: `2026-07-12`
- 確度: `CONFIRMED`
- 分類: 8(テスト欠落・見せかけのGREEN)、16(テスト件数の陳腐化)、15(軽量モデルが誤解する手順)
- 問題事象(監査時点の原文): AI_SKILLS §3.5 の「完了の定義」はpytest(件数記載が「161件」のまま)・tsc・cargoのみを列挙し、Phase 4-Aで追加された `tests-runtime/manifest_boundary.test.ts`(42件)を含まない。さらにこのスイートの実行手順(境界tsconfigでのコンパイル → golden fixture生成 → `"type": "module"` 環境下でのCommonJS出力実行の回避手順)は追跡ファイルのどこにも記録されておらず、`tsconfig.boundary.json` 単体からは再現できない。メインの `tsc --noEmit` は `include: ["src"]` のため `tests-runtime/` を型検査すらしない。
- 監査後状況: Finding 1で `apps/desktop/scripts/run-boundary-tests.ps1` は追加済み（監査時点の「手順がどこにも存在しない」は解消）。ただし DoD未統合・ユーザー固有絶対パス依存・suite手書き列挙が残っていた。監査時点の42件と、現在runnerが実行する複数suite（manifest + consult/tensor）を混同しない。
- 発生条件: 将来のエージェントが `parseManifest.ts` / `manifestFetchState.ts` を変更し、文書化されたDoDに従って検証した場合。
- 実害または潜在的影響: runtime境界の防御(INC-PHASE4A系の再発防止装置)を一度も実行せずに「全GREEN」を宣言できる。文書化された手順に従うほど検証が漏れる、構造的な見せかけのGREEN。
- 根本原因: STEP 5でテスト基盤を新設した際、DoD(AI_SKILLS §3.5)とHANDOFF §10のゲート一覧へ統合されなかった。実行ノウハウがセッション会話にのみ存在する。
- 解決方式:
  - `package.json` に正式入口 `test:boundary` を追加
  - runner をユーザー固有絶対パスから脱却（`-Python` → `PKB_PYTHON` → `$LOCALAPPDATA` 候補 → `Get-Command python`）
  - `ensure-node-path.ps1` 利用、`finally` で `.boundary-tests-out` 削除
  - `tsconfig.boundary.json` を `tests-runtime/**/*.test.ts` glob 化
  - runner がコンパイル済み `*.test.js` を動的列挙（0件 hard-fail）
  - AI_SKILLS §3.5 から固定件数 `161件` を削除し、cwd 付き `npm.cmd run test:boundary` を DoD へ統合
  - HANDOFF §10 に正式 boundary 入口を追加（旧件数・「最新」表現は別findingのため温存）
  - `tests/test_dod_boundary_contract.py` で再発防止
  - STEP 6.5: DoD 全pytestが検出した `_FakeFacade.last_romance_analysis` 追従漏れを test-only 1行で是正（production / `engine_stdio.py` 無変更）
- 変更ファイル:
  - `apps/desktop/scripts/run-boundary-tests.ps1`
  - `apps/desktop/tsconfig.boundary.json`
  - `apps/desktop/package.json`
  - `docs/AI_SKILLS.md`（§3.5のみ）
  - `docs/HANDOFF.md`（§10のみ）
  - `tests/test_dod_boundary_contract.py`（新規）
  - `tests/test_oracle.py`（STEP 6.5: `_FakeFacade` に `last_romance_analysis = staticmethod(lambda: None)` 1行）
  - `docs/AUDIT_FINDINGS_2026-07-11.md`（本エントリ）
- 検証(実測・件数は今回の結果であり DoD 恒久契約値ではない):
  - 変更前: `test_stdio_dispatch_ignores_unknown_params` → `AttributeError: last_romance_analysis` RED
  - `pytest tests/test_oracle.py` → 7 passed
  - `pytest tests/` → **420 passed**
  - `npm.cmd run test:boundary` → manifest 42 + consult/tensor 54 全GREEN、`.boundary-tests-out` 残存なし
  - `tsc --noEmit` / Vite build → GREEN
  - `cargo test` 27/27 / `cargo check` → GREEN
  - `git diff --check` / data / package-lock / Cargo → クリーン
  - `engine_stdio.py` 無差分
- commit: `UNCOMMITTED`
- 残存リスク:
  - 新 boundary suite 追加時は glob が拾うが、fixture generator 命名規則外は runner の gen 列挙に追従が必要
  - HANDOFF §10 の古い件数・「最新」表現の全面改訂は別 finding
  - 他の test double が Phase 追加に追従漏れした場合、全pytest DoD が再び検出する（意図どおり）
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\AI_SKILLS.md:186` — 「全スイート (161件。正順/逆順 GREEN)」(現行は7スイートだけで170件)
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\AI_SKILLS.md:184-190` — DoDコマンド列に境界スイートなし
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\tsconfig.json:23` — `"include": ["src"]`(tests-runtime対象外)
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\package.json:5` — `"type": "module"`(CommonJS出力の素朴な `node` 実行と衝突。回避手順は未記録)
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\HANDOFF.md:397-445` — §10 GREENゲート一覧に境界スイートの実行コマンドなし
- 契約または文書との矛盾(監査時点): AI_SKILLS §16.6「実装より先にRED契約を書け」等の規律が、肝心の境界スイートをDoDから漏らしたままでは強制不能。
- 静的監査だけでは確定できない点: なし(手順の不在・件数の齟齬は静的に確定)。

### [P3] AI_SKILLS §12 ヘッダ「実装は全て未着手」が同一節内の as-built 記録と自己矛盾

- 状態: RESOLVED
- 着手日: `2026-07-12`
- 解決日: `2026-07-12`
- 確度: `CONFIRMED`
- 分類: 9(skillsと実装の矛盾)、10(完了済み項目の未完了記載)
- 問題事象: §12見出しは「Target Echo — 設計のみ完了(実装は全て未着手)」と宣言するが、同節内にE0/E1/E2の完遂as-built記録が存在し、実コードにも `tensor_store.py`・`oracle.py`・facade の oracle/twin/tensor API・ProfileTab の配線が存在する。
- 独立確認(2026-07-12): E0〜E4の実装ファイル・対応テスト・as-built見出しが存在する。E5のみが3000ms計測ゲート未達により意図的未着手。`SPEC_ECHO_GENESIS.md`冒頭にも「E0〜E5全て未実装」の旧宣言が残る。
- 発生条件: §0の読み込みプロトコルに従い見出しで節の要否を判断する読者(特に軽量モデル)。
- 実害または潜在的影響: Echoを「未着手」と誤認した再設計・再実装・重複SPEC作成。
- 根本原因: E0以降のas-built追記時にヘッダ文言が更新されなかった。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\AI_SKILLS.md:607` — ヘッダ「実装は全て未着手」
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\AI_SKILLS.md:637-663` — 「E0/E1 完遂 (2026-07-07) — as-built」以下
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\facade.py:142-207` — oracle_payload / oracle_report / twin_forecast / tensor_rebuild 実装
- 契約または文書との矛盾: 同一ファイル内の見出しと本文が矛盾。
- 静的監査だけでは確定できない点: なし。
- 解決(2026-07-12):
  - E0〜E4実装anchor: `tensor_store.py`(`TensorStore`/`build_tensor`)、`coupling.py`(`coupling_matrix`)、`digital_twin.py`(`TwinParams`/`fit_twin`/`simulate`)、`oracle.py`(`_assert_sterile`/`build_oracle_payload`)。対応テスト4件存在。
  - E5: 3000ms計測ゲート未達のため未着手・着手禁止(実装漏れではない)。
  - AI_SKILLS §12: 見出しを「E0〜E4完遂 / E5は計測ゲート未達のため未着手」へ是正。導入を設計規律+as-built所有・実コード正本・SPEC凍結参照・再実装禁止・E5着手禁止に改め、状態表を追加。既存as-built本文は無変更。
  - SPEC: Rev.7で文書同期を記録。冒頭の実装状態宣言をE0〜E4実装・検証済み / E5未実装かつ着手禁止へ更新。設計本文・数式・定数は無変更。
  - RED分布: `test_echo_document_status_contract.py` で4失敗(見出し・§12「実装は全て未着手」・SPEC「全て未実装」・両文書状態一致) / 5通過(as-built見出し・E5ゲート・AST・テスト存在・部分制約)。是正後9/9 GREEN。
  - 変更ファイル: `docs/AI_SKILLS.md`(§12見出し・導入・状態表のみ)、`docs/SPEC_ECHO_GENESIS.md`(Rev.7+冒頭状態宣言)、`tests/test_echo_document_status_contract.py`(新規)、本Finding。
  - ゲート: `py_compile` OK / echo契約9 / echo単体31 / context+dod契約26 / `pytest tests/` 429 / boundary 54+42 / `tsc --noEmit` / `npm run build` / `cargo test` 27 / `cargo check` / `git diff --check` / `.boundary-tests-out`なし。`data/`・package-lock・Cargoに本finding由来差分なし。
  - production無変更。
  - commit: `UNCOMMITTED`
  - 残存制約: dyad unratable stub、未定義config laneのmask=0、E5着手禁止(3000msゲート)。

### [P3] HANDOFF「AI_SKILLSを全文読む」と AI_SKILLS §0「全文読了は禁止」の命令衝突

- 状態: RESOLVED
- 着手日: `2026-07-12`
- 解決日: `2026-07-12`
- 確度: `CONFIRMED`
- 分類: 15(競合する命令)、9(文書間矛盾)
- 問題事象: HANDOFF §1 の必須手順2は「`docs/AI_SKILLS.md`を全文読む」と命じるが、AI_SKILLS §0(2026-07-08改訂)は「とりあえず全文」読み込みを明示的に禁止し、タスク種別ごとの節選択を命じる。
- 独立再監査(2026-07-12): `SPEC_ECHO_GENESIS.md` と `SPEC_CHARLIE_DELTA.md` 冒頭の読者向け前提命令にも同型の「AI_SKILLSを全文読め」が残存。一方 `AI_SKILLS` §0・`CONTEXT.md`・`THE_ARCHITECTS_MANIFESTO.md`・`SPEC_FOXTROT_UI.md` は既にタスク別ルーティングへ整合。
- 発生条件: 新任エージェントがHANDOFF §1に従って開始した場合(常時)。
- 実害または潜在的影響: どちらに従っても他方に違反する。HANDOFF自身の「競合時はAI_SKILLS優先」則で解決可能だが、軽量モデルには矛盾命令として残る(コンテキスト浪費または規律違反の二択に見える)。
- 根本原因: AI_SKILLS §0改訂(全文読了撤回)にHANDOFF §1の文言が追従していない。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\HANDOFF.md:48` — 「`docs/AI_SKILLS.md`を全文読む。」
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\AI_SKILLS.md:9-31` — §0「全文読了の強制を撤回」「『とりあえず全文』は禁止する」
- 契約または文書との矛盾: 両必読文書間の直接矛盾。
- 静的監査だけでは確定できない点: なし。
- 解決(2026-07-12):
  - AI_SKILLS §0を唯一の読み込みルーターとして維持(本書無変更)。
  - HANDOFF §1手順2: §0プロトコル → §1必読 → 表から該当節選択 → 横断時は複数行 → 「とりあえず全文」禁止 → 競合時AI_SKILLS優先。手順1(本書全文)は維持。表の複製なし。§10無変更。
  - Echo SPEC Rev.8: 前提命令を§0ルーティング後に§1・§12・本SPECへ。全文読了命令を除去。状態宣言・設計本文無変更。
  - Charlie/Delta SPEC Rev.7: 前提命令を§0ルーティング後に§1と§9〜§11の選択へ。全文読了命令を除去。実装状態宣言・設計本文無変更。
  - Foxtrot SPEC: 既存ルーティング維持・無差分。
  - RED分布: 契約テストでHANDOFF全文命令3件 + SPEC全文命令/ルーティング不足3件が失敗(抽出器の§11誤切りはテスト側で`(?!\d)`修正)。是正後12/12 GREEN。
  - 変更ファイル: `docs/HANDOFF.md`(§1手順2のみ・本finding分)、`docs/SPEC_ECHO_GENESIS.md`(Rev.8+前提命令)、`docs/SPEC_CHARLIE_DELTA.md`(Rev.7+前提命令)、`tests/test_document_reading_protocol_contract.py`(新規)、本Finding。
  - ゲート: py_compile OK / 契約12 / 関連契約35 / `pytest tests/` 441 / boundary 54+42 / tsc / build / cargo test 27 / cargo check / `git diff --check` / `.boundary-tests-out`なし。本finding由来のproduction・data・lock・Cargo差分なし。
  - production無変更。
  - commit: `UNCOMMITTED`
  - 残存事項: HANDOFF §10の陳腐化はFinding 8として未変更。

### [P3] HANDOFF §10「最新GREENゲート」が STEP 5 以前のスナップショットのまま「最新」を称する

- 状態: RESOLVED
- 着手日: `2026-07-12`
- 解決日: `2026-07-12`
- 確度: `CONFIRMED`
- 分類: 16(現在状態の陳腐化)
- 問題事象: §10は「最新再検証」として「Vite build: PASS、60 modules transformed」を記録するが、STEP 5・マウント後のビルドは61モジュール以上であり、また同節のゲート一覧に境界42テストが含まれない。§0/§11はas-built同期済みのため、同一文書内で鮮度が混在する。
- 独立確認(2026-07-12): Finding 5で `npm.cmd run test:boundary` 入口は§10へ追加済みだが、旧スナップショット(件数・絶対パス・module数・時間・「最新」宣言)は意図的に本Findingへ残されていた。解決方針は現在値への更新ではなく、所有権の再設計(§10は正本案内のみ、DoD正本は AI_SKILLS §3.5)。
- 発生条件: §10をゲート再現の基準として使う読者。
- 実害または潜在的影響: ゲート再現時の期待値齟齬。境界スイートを回さない検証をゲート達成と誤認。
- 根本原因: docs同期コミット(69fa99d)がヘッダ・§2・§11・§13を更新した一方、§10の検証スナップショットを更新しなかった。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\HANDOFF.md:441` — 「60 modules transformed、766ms」
  - `C:\Users\badger\Documents\cursur\decision_engine\docs\HANDOFF.md:397-410` — ゲート対象一覧(境界スイートなし)
- 契約または文書との矛盾: 同文書内の「As-Built完了」宣言(§0)と§10の旧スナップショットの不整合。
- 静的監査だけでは確定できない点: 現時点の正確なモジュール数(ビルド実行が禁止のため、直近セッション記録の61/63との差は確定不能)。
- 解決(2026-07-12):
  - §10を「検証ゲートの正本と実行規律」へ全面置換。volatile snapshot(件数・module数・時間・PASS一覧・「最新」宣言・ユーザー固有パス・限定スイート一覧)を削除。
  - DoDの唯一の正本は `docs/AI_SKILLS.md` §3.5。本節は正本案内・実行規律・boundary入口のみ所有。
  - Finding 5の `npm.cmd run test:boundary`（cwd=`apps/desktop`）を維持。`tsc --noEmit` は tests-runtime 代替不可と明記。
  - 削除した陳腐化情報: 170 passed / 1.32s、7スイート固定一覧、`C:\Users\...` pytest、60 modules / 766ms、TypeScript・Vite・diff・data・packageの静的PASS、「最高アーキテクト補佐が実環境で再実行」「最新再検証」「最新GREENゲート」。
  - RED分布: 契約9失敗(見出し・スナップショット宣言・§3.5正本欠落・DoD複製・絶対パス・数値・ファイル一覧・PASS結果・監査案内) / 5通過(boundary入口・cwd・tsc注記・pytest規律・Finding5回帰)。是正後14/14 GREEN。
  - 変更ファイル: `docs/HANDOFF.md`(§10のみ・本finding分)、`tests/test_handoff_gate_contract.py`(新規)、本Finding。
  - ゲート実測(台帳記録。HANDOFFへ転記しない): handoff契約14 / 関連契約38 / `pytest tests/` 455 / boundary 54+42 / tsc / build / cargo test 27 / cargo check / `git diff --check` / `.boundary-tests-out`なし。本finding由来のproduction・data・lock・Cargo差分なし。
  - production無変更。
  - commit: `UNCOMMITTED`
  - 残存事項: HANDOFF内のPhase 4-A as-built履歴(§0/§11等)は「最新結果」ではなく当時の記録として維持。

### [P3] PROFILEタブに恒久モックパネルが2件存在(6Dレーダー / MBTI)

- 状態: RESOLVED
- 着手日: `2026-07-12`
- 解決日: `2026-07-12`
- 確度: `CONFIRMED`
- 分類: 7(実データに配線されていないUI)、20(backend契約を持たない表示)
- 問題事象: PROFILEタブは (a) 固定定数 `TENSOR_RADAR_PREVIEW` によるレーダーチャート(「PHASE 1 PREVIEW / NOT MEASURED」)と (b) 固定値のMBTIバー(「PREVIEW / NOT MEASURED」「固定モック表示です」)を常設表示する。両者ともモックであることは明示されており意図的である(Phase 1 / 3-A のプレビュー裁定)。ただし (a) については、同一6軸の実測データが現在は毎講評で生成・搬送されている(P2第1項)ため、「幾何検証用の固定プレビュー」という当初の存在理由が陳腐化している。(b) にはbackend契約・測定機能が一切存在しない。
- 独立確認(2026-07-12): 6D実測はFinding 1によりInterview/GDの`MISSION_RESULT`→`TensorProfilePanel`へsession-scoped配線済み。PROFILE用のgenre横断「最新成績表」取得契約は存在しない。MBTI測定契約も存在しない。裁定: 新規backend/IPC/永続化を作らず、両モックを撤去する。
- 発生条件: PROFILEタブ表示時(常時)。
- 実害または潜在的影響: 個人分析ダッシュボード上に架空の数値が常設され、実測値と誤読される余地(ラベルで緩和されている)。実測データ到達後もモックが「本番の顔」を占有し続ける。
- 根本原因: Phase 1/3-A のプレビュー成果物が、Phase 2 で実データが生まれた後も差し替え裁定を受けていない。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\components\ProfileTab.tsx:16-59` — 固定 `TENSOR_RADAR_PREVIEW`
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\components\ProfileTab.tsx:365-376` — 「PHASE 1 PREVIEW / NOT MEASURED」常設描画
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\components\MbtiGradientBars.tsx:8-15,32-36` — 固定MBTI値と明示ラベル
- 契約または文書との矛盾: モック表示自体は文書化された意図的挙動。矛盾はプレビューの存在理由(Phase 1「未測定のため」)と現状(測定データが存在し破棄されている)の間にある。
- 静的監査だけでは確定できない点: なし。
- 解決(2026-07-12):
  - PROFILEから `TENSOR_RADAR_PREVIEW` / `TENSOR_PROFILE_6D` section / `MbtiGradientBars` を撤去。代替パネル・placeholder・架空値・ローカル保存なし。
  - 削除ファイル: `apps/desktop/src/components/MbtiGradientBars.tsx`
  - `TensorRadarChart`: `preview` prop / `PREVIEW_LABEL` / preview描画を削除。実測用clamp・null=N/A・SVG/legend/tooltip/keyboard維持。
  - CSS dead surface除去: `.tensor-radar-section` / `.tensor-radar-preview-label` / `.mbti-preview-*`。generic radar CSS維持。
  - 実測6D経路維持: InterviewTab `MISSION_RESULT` → `report.tensor_profile` → `TensorProfilePanel`（InterviewTab/TensorProfilePanel/backend/IPC無変更）。
  - MBTI非推定・新規IPCなし。
  - RED分布: 契約9失敗(モック残存)/3通過(実測経路・panel・radar CSS)。是正後 profile mock 12 + 関連契約群 41 GREEN。
  - 文書: SPEC_ENGINE_TENSOR_PROFILING 現行優先裁定、SPEC_PHASE3_UX_ROMANCE MBTI非表示、AI_SKILLS Phase 3-A Finding 9追補。
  - ゲート実測: 契約41 / `pytest tests/` 467 / boundary 54+42 / tsc / build / cargo test 27 / cargo check / `git diff --check` / Finding 9対象モック文字列（PHASE 1 PREVIEW / NOT MEASURED、PREVIEW / NOT MEASURED、MBTI preview）0件 / `.boundary-tests-out`なし。本finding由来のdata・lock・Cargo差分なし。
  - 注記(記録訂正): 「frontend preview文字列0件」は広すぎる。`ContextObservatory.tsx` の無関係な `STATIC PREVIEW / NO IPC` は残存しており本findingの対象外・変更禁止。RESOLVED判定・実装には影響なし。
  - production backend無変更。
  - commit: `UNCOMMITTED`

### [P3] ContextObservatoryContainer が in-flight 中の再クリックを禁止せず、W-49 並行禁止規律と不整合

- 状態: RESOLVED
- 着手日: `2026-07-12`
- 解決日: `2026-07-12`
- 確度: `CONFIRMED`
- 分類: 13(状態表示・操作効率)、9(規律と実装の矛盾)
- 問題事象: 取得ボタンは `phase === "loading"` の間も `disabled` にならず、連打で複数の `pkbInvoke` を発行できる。コードベースの規律(W-49)は「エンジン多重化が land するまで、pkbInvoke は並行前提で使うな」と明記し、他タブは busy フラグでボタンを殺している。stale応答はreducerのseqガードで破棄されるため表示は壊れないが、後続リクエストがRust側ロックで直列待ちになり無駄な実行が積まれる。
- 独立確認(2026-07-12): seqガードは応答整合性のみを保証し request admission control ではない。`disabled`だけではReact再描画前の同一tick連打を防げない。裁定: 即時更新`useRef`再入拒否 + `phase === "loading"`のbutton無効化の二重防壁で解決する。
- 発生条件: 読み込み中にユーザーがボタンを再クリックした場合。
- 実害または潜在的影響: 無駄なIPC/ファイルI/Oの直列実行。規律の不統一による将来の模倣リスク。
- 根本原因: STEP 5 実装指令が明示ボタン・seqガードを規定した一方、busy中のボタン無効化を規定しなかった。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\components\ContextObservatoryContainer.tsx:76-78` — `disabled` なしのボタン
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\lib\useCorrelationId.ts:10-14` — W-49 並行使用禁止の明記
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\components\ProfileTab.tsx:157` — 対照: `disabled={busy !== null}`
- 契約または文書との矛盾: W-49(コード内規律注記)との不整合。
- 静的監査だけでは確定できない点: 連打時の実挙動(直列待ちの体感遅延)は実行確認が必要。
- 解決(2026-07-12):
  - 二重防壁: `inFlightRef` 再入拒否 + `isLoading`(`phase === "loading"`)の `disabled` / `aria-busy`。
  - 処理順序: `if (inFlightRef.current) return` → `inFlightRef.current = true` → `++seqRef` → `REQUEST_START` → `latestContextManifest()` → `finally { inFlightRef.current = false }`。
  - 第2操作は queue/retry/debounce せず即無視。success/failure 双方で finally 解除。
  - 再試行可能: empty/error→`読み込み`、ready→`再読み込み`、loading→`読み込み中…`。
  - seq/stale ガード維持。reducer・parser・engine・dumb view 無変更。新規 IPC/cancel/queue なし。
  - RED分布: 8失敗(ref/finally/disabled/label/aria-busy) / 4通過(単一IPC・ラベル・polling禁止・reducer契約)。是正後12/12 GREEN。
  - 変更ファイル(本finding): `ContextObservatoryContainer.tsx`、`tests/test_context_observatory_single_flight_contract.py`、AI_SKILLS §16.4追補、HANDOFF §11 Container追補、本Finding。
  - ゲート: single-flight契約12 / context+handoff契約29 / `pytest tests/` 479 / boundary 54+42 / tsc / build / cargo test 27 / cargo check / `git diff --check` / `.boundary-tests-out`なし。本finding由来のbackend・data・lock・Cargo差分なし。
  - commit: `UNCOMMITTED`

### [P3] retrieval_manifests の不変ファイルがターン毎に無制限蓄積(保持方針なし)

- 状態: RESOLVED
- 着手日: `2026-07-12`
- 解決日: `2026-07-12`
- 確度: `CONFIRMED`
- 分類: 11(性能・ストレージ上の無駄)、5(読み手のいない生成物)
- 問題事象: consult系の全ターン(相談・面接・GDの各ターン)で `{manifest_id}.json` が1件ずつ永続化されるが、削除・世代管理・保持上限のコードおよび文書上の方針が存在しない。読み出しはlatest 1件のみ(immutableコピーはINC-04のすり替え検証に使われるため存在自体は設計意図)。
- 裁定(2026-07-12): `RETRIEVAL_MANIFEST_RETENTION_LIMIT=256`。owned=`<hex32>.json`のみ。latest指すimmutableを常時保護。残りは`(st_mtime_ns, filename)`降順で保持。mtimeは削除候補選択のみ(integrity根拠にしない)。corrupt/ID不一致/symlinkは削除も修復もせずhard-fail。unknown/non-hexは無視・非削除。cleanupは新immutable+latest commit成功後のみ。latest更新失敗時はcleanup禁止。
- 発生条件: 通常利用の継続(長期セッション・多ターン面接で加速)。
- 実害または潜在的影響: `data/processed/retrieval_manifests/` の単調増加。数千ターン規模でファイル数がディレクトリ走査・バックアップ・同期のコストに波及。
- 根本原因: Phase 4-A は完全性(immutability・結合検証)を裁定したが、保持(retention)を裁定していない。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\consultation_engine.py:1163` — 全ターンで保存
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\retrieval_manifest.py:789-834` — 保存・latest読出のみ(削除・列挙・上限なし)
- 契約または文書との矛盾: 矛盾なし(裁定の空白)。
- 静的監査だけでは確定できない点: 実運用でのファイル増加率と実害の発生時期(実データ読取が禁止のため測定不能)。
- 解決(2026-07-12):
  - `_prune_retrieval_manifests(latest_manifest_id=...)` を追加。`save_retrieval_manifest` 順序 = 検証→mkdir→latest integrity→immutable→latest atomic replace→prune。
  - 保持上限 256。選択: latest 保護 + `(mtime_ns, filename)` 降順。全 owned を preflight deserialize/validate/ID一致後にのみ unlink。
  - 障害: corrupt/ID mismatch/symlink → 削除ゼロ hard-fail。latest replace 失敗 → prune 非実行。unlink OSError → 固定 PersistenceError。load 経路は prune なし。
  - INC-PHASE4A-06 append、AI_SKILLS §16.5.1 追加。
  - RED→GREEN: `test_manifest_retention.py` + isolation追補 + `test_retrieval_manifest.py` で 108 passed / 1 skipped。
  - ゲート: `pytest tests/` 494 (+1 skip) / boundary 54+42 / tsc / build / cargo 27 / `git diff --check` / `.boundary-tests-out`なし。本finding由来の frontend・lock・Cargo・実data差分なし。
  - 残存リスク: corrupt historical が残ると以降の save は prune preflight で PersistenceError（修復せず、typed isolation で主機能継続）。unlink 部分失敗後は次回成功 save で再 prune。
  - commit: `UNCOMMITTED`

### [P3] engine.log が無制限追記され、全失敗リクエストのトレースバックを蓄積(ローテーションなし)

- 状態: RESOLVED
- 確度: CONFIRMED
- 解決日: `2026-07-12`
- commit: `UNCOMMITTED`
- 着手日: `2026-07-12`
- 再開理由(2026-07-12 検収): (1) V1 header のみで安全判定し header 後の秘密文字列を保持し得る、(2) `Path::exists()` が dangling symlink を欠落と誤認し `File::create` がリンク先を作成し得る、(3) matcher が行中 `\r` を無条件無視し exact match が崩れる、(4) 台帳 boundary 記録が 42 で不正確（正は 96/96）。敵対契約3件を RED 追加して是正する。
- 分類: 11(不要I/Oの蓄積)、17(privacy上危険なログ)
- 問題事象: Rustはエンジンのstderrを `logs/engine.log` へ `append` で開き、ローテーション・サイズ上限が存在しない。Python側は `sys.stdout = sys.stderr` により全ライブラリのprintがログへ流れ、さらに失敗した全リクエストで完全なトレースバックを出力する。例外引数にユーザー由来文字列(入力値・ファイル名等)が含まれる場合、それが無期限にログへ残る。
- 裁定(2026-07-12): 永続化は exact allowlist `[PKB_DIAG_V1] REQUEST_FAILED` のみ。1 MiB × current+2 backup。V1 header必須。legacy raw 非継承。logger failure時はdrain＋永続化fail-closed。stdout JSON Lines・IPC error shape（Finding 13対象）は維持。
- 発生条件: 長期利用、および例外を発生させる操作の反復。
- 実害または潜在的影響: ログの単調肥大。個人データ断片のログ残留(AI_SKILLS §1-2「ログ・エラーメッセージに日記本文やLINE本文をそのまま出すな」への抵触リスク)。
- 根本原因: ログ出力先の集約は設計されたが、ライフサイクル(ローテーション・保持)と内容規律(トレースバック内の値)の管理が未設計。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src-tauri\src\engine.rs:225-233` — `create(true).append(true)`、ローテーションなし
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\engine_stdio.py:36` — `sys.stdout = sys.stderr`
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\engine_stdio.py:219` — 失敗毎の `traceback.print_exc`
- 契約または文書との矛盾: AI_SKILLS §1-2 のログ規律と潜在的に抵触(実際に本文が例外へ乗る経路の有無は動的確認事項)。
- 静的監査だけでは確定できない点: 例外メッセージに実際に個人データが乗る具体経路の網羅(全raise箇所の値追跡は実行なしでは不完全)。
- git status (着手時): Finding 1〜11 の未commit差分を保持。巻き戻し禁止。
- 解決追補(2026-07-12・一次・不完全):
  - Python: `traceback` / `print_exc` 削除。失敗時 stderr へ固定診断 `[PKB_DIAG_V1] REQUEST_FAILED` を1回のみ。IPC `error` shape は維持（Finding 13未着手）。
  - Rust: 子 stderr を `Stdio::piped()` + collector。exact allowlist のみ永続化。CRLF→LF。1 MiB × `engine.log`+`.1`+`.2`。V1 header。legacy raw 非継承。logger I/O 不能時は drain＋永続化 fail-closed（raw fallback なし）。
  - RED→GREEN: `tests/test_engine_log_safety.py` 5件; Rust elog_* 13件 + Finding 2 既存 27件 = cargo 40 passed。
  - ゲート実測: pytest 499 passed / 1 skipped; boundary **consult/tensor 54 + manifest 42 = 96/96**（一次台帳の「boundary 42」は誤記）；tsc; Vite build; cargo test/build/check; `git diff --check` GREEN。
  - 残存方針: logger I/O 不能時はログを捨てて engine 継続。Finding 13（UI 生例外）は未着手。
  - 検収欠陥: header-only 安全判定 / dangling symlink / 行中 CR 無視 — 下記是正ラウンドへ。
- 是正追補(2026-07-12・検収欠陥解消):
  - 安全判定: header 以降の全行を allowlist 検証。`[PKB_ENGINE_LOG_V1]\nSECRET...` は unsafe → reset（保持・backup 継承しない）。
  - 存在裁定: `symlink_metadata` のみ。`Path::exists()` を Finding 12 区間から除去。Symlink≠Absent（`open_plan`）。dangling 実symlink は権限がある環境で runtime 検証、無権限時は open_plan + source 契約 + directory 置換で担保。
  - matcher: CR は marker 完了後の CRLF 終端のみ許可。行中 CR は discard。
  - 追加 RED→GREEN: `elog_header_plus_secret_body_is_reset_not_kept` / `elog_cr_inside_marker_rejected_crlf_end_ok` / `elog_open_plan_symlink_never_equals_absent` / `elog_finding12_source_has_no_path_exists` / `elog_directory_named_engine_log_replaced_with_v1_file`（+ dangling runtime）。cargo **46 passed**。Python 5/5。boundary **96/96**。
  - Finding 13: 未着手。commit/stage/push: なし。

### [P3] 全7タブが生のバックエンド例外文字列を `String(err)` でUI表示(エラー表示規律の不統一)

- 状態: RESOLVED
- 確度: CONFIRMED
- 解決日: `2026-07-12`
- commit: `UNCOMMITTED`
- 着手日: `2026-07-12`
- 分類: 13(エラー回復・状態表示)、17(例外メッセージ)
- 問題事象: 7コンポーネント22箇所で、stdio境界の `"{ExceptionType}: {message}"` 形式の生文字列がそのまま画面に描画される。Phase 4-A の `ContextObservatoryContainer` は「固定文言のみ表示・例外文言を運ばない」規律で実装されており、同一アプリ内でエラー表示の規律が二重基準になっている。
- 裁定(2026-07-12): UI catch は例外値を一切表示せず、operation-keyed 固定文言 (`uiErrorMessages.ts` 21キー) のみ。error 解析・分類禁止。Finding 2 `replay_policy` と retry-safe/verify-first を一致。自動再試行禁止。Finding 12 (backend/IPC) は変更しない。
- 発生条件: 任意のIPC失敗時。
- 実害または潜在的影響: ユーザー向けエラーが内部実装用語(`ValueError` 等)で表示される。例外メッセージにユーザー入力断片が含まれる経路では、それが画面へエコーされる(本人のみのローカルUIのため流出ではないが、Phase 4-Aが遮断した経路と同型)。
- 根本原因: Phase 4-A で導入されたエラー無菌化規律が既存タブへ遡及適用されていない(適用範囲の裁定なし)。
- 証拠(監査時点):
  - `String(err)` 22箇所: `ProbeTab.tsx`(4)、`RecordTab.tsx`(2)、`InterviewTab.tsx`(2)、`ConsultTab.tsx`(2)、`ImportTab.tsx`(5)、`ProfileTab.tsx`(4)、`SettingsTab.tsx`(3) — 各 `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\components\` 配下
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\engine_stdio.py:220` — `f"{type(exc).__name__}: {exc}"`
  - 対照: `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\lib\manifestFetchState.ts:8-9` — 固定文言定数
- 契約または文書との矛盾: AI_SKILLS §16.4(error時に例外文言を運ばせない)は Phase 4-A 境界のみを対象と読めるため、既存タブは形式上違反ではないが規律が分裂している。
- 静的監査だけでは確定できない点: 各例外経路のメッセージに実際に含まれ得る値の網羅。
- git status (着手時): Finding 1〜12 の未commit差分を保持。巻き戻し禁止。
- 解決追補(2026-07-12):
  - `uiErrorMessages.ts` exact 21 keys（retry-safe 5 / verify-first 16）。helper は code のみ受け取る。
  - 22 catch を固定キーへ置換。`String(err)` 0件。Import catch に filename 非連結。
  - statusKind 分離（Interview/Consult/Settings profiler）。error=`alert`、progress=`status`。Record error は 3s auto-hide しない。
  - RED→GREEN: `tests/test_ui_error_sanitization_contract.py` 13件; boundary ui_error 12 + 既存 54+42 = **108/108**。
  - Finding 12 / backend / IPC / engine.ts / Rust / Python production: 無変更。
  - 自動再試行なし。Finding 14 未着手。commit/stage/push なし。

### [P3] core/cli.py が llama HTTP クライアントを重複実装(レガシーCLI経路の drift リスク)

- 状態: RESOLVED
- 着手日: `2026-07-12`
- 解決日: `2026-07-12`
- 確度: `CONFIRMED`
- 分類: 5(重複)、9(単一真実則との緊張)
- 問題事象: `core/cli.py`(`src/python/app.py` から到達するレガシーCLI)が、`consultation_engine.py` と並行して health チェックおよび `/v1/chat/completions` クライアントを独自実装している。LLM関連の単一真実は `llm_config.py` と規定されているが、クライアント実装が二系統あるため片側だけの変更で挙動が乖離し得る。
- 裁定(2026-07-12): llama-server client の唯一所有者は `core/llm_backend.py`。config は `llm_config.py`。CLI入口・固有prompt/retrieval/LlamaCliBackend は維持。CLI削除禁止。generation literal (0.6/900/8192) を cli から除去。consult role 統一。
- 発生条件: cli.py 経由の利用、またはLLMクライアント仕様(エンドポイント・パラメータ)変更時。
- 実害または潜在的影響: 生成パラメータ・タイムアウト・エラー処理の乖離。メンテナンス時の修正漏れ。
- 根本原因: デスクトップ/TUI移行後もレガシーCLIが削除・委譲されずに残存。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\cli.py:223-242` — 独自 health/chat-completions クライアント
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\consultation_engine.py:595-660` — 同等機能の本流実装
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\app.py:11` — `from core.cli import main`(到達経路)
- 契約または文書との矛盾: AI_SKILLS §5「モデル名・量子化・サーバー引数・生成パラメータをコードにハードコードするな」(単一真実則)との緊張。cli.py 内のパラメータ取得元が llm_config か直書きかまでは未追跡。
- 静的監査だけでは確定できない点: cli.py が現在も動作するか(facade/コアAPIの変遷に対する互換性)は実行確認が必要。
- baseline(着手時): `py_compile` CLI/CE OK、`python src/python/app.py --help` exit 0、`data/` 無差分。Finding 1〜13 未commit差分保持。
- 解決追補(2026-07-12):
  - 新規 `core/llm_backend.py` が唯一の `LlamaServerBackend`（health / chat / SSE / structured / KV / lifecycle）。
  - `consultation_engine` / `cli` は shared import。CLI の urllib/socket 撤去。`find_gguf(role="consult")`。`LlamaCliBackend` は `generation_params()` + `LLAMA_CTX`。
  - 契約: `tests/test_llm_client_single_owner.py` 12件 GREEN（networkless）。全 pytest 524 passed / 1 skipped。
  - INC-LLM-CLIENT-01 / AI_SKILLS §5.7–9。CLI入口維持。Finding 15 未着手。commit/stage/push なし。

### [P3] engine.ts の readFileAsText が未使用エクスポート(デッドコード)

- 状態: RESOLVED
- 着手日: `2026-07-12`
- 解決日: `2026-07-12`
- commit: `UNCOMMITTED`
- 確度: `CONFIRMED`
- 分類: 5(デッドコード)
- 問題事象: `readFileAsText` はエクスポートされているが、コンポーネント・lib内のどこからも参照されていない(実際のファイル読込は `readTextLenient` に統一済み)。
- 裁定: 未使用`readFileAsText`を削除し、`readTextLenient`を唯一の持ち込みテキスト読込経路として維持。代替 alias / wrapper / re-export 禁止。`textDecode.ts` 変更禁止。
- 発生条件: 常時(静的事実)。
- 実害または潜在的影響: 実害は軽微。エンコーディング耐性のない `file.text()` 直呼びであるため、将来誤って再利用されると cp932 系ファイルで文字化けする(周辺規律 `readTextLenient` の迂回口)。
- 根本原因: `readTextLenient` 導入時に旧APIが削除されなかった。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\lib\engine.ts:189-191` — 定義
  - `apps\desktop\src\components` 配下の参照0件(grep確認)
- 契約または文書との矛盾: AI_SKILLS §2.2(持ち込みファイルは lenient 読込を使え)の迂回口として残存。
- 静的監査だけでは確定できない点: なし。
- baseline(着手時): production 参照は `engine.ts` 定義のみ。直接 `.text()` は当該関数内のみ。`readTextLenient` import/呼び出し/export 維持。`tsc --noEmit` / `npm run build` GREEN。Finding 1〜14 未commit差分保持。
- 解決追補(2026-07-12):
  - `engine.ts` から `readFileAsText` を完全削除。alias / replacement wrapper / re-export なし。
  - production 内旧識別子 0件。`engine.ts` 内直接 `.text()` 0件。
  - `readTextLenient` の既存正規経路（import + 呼び出し + `textDecode.ts` export）維持。`textDecode.ts` 無変更。
  - RED: 契約1 FAIL / 契約2 FAIL / 契約3 PASS（2 failed, 1 passed）。GREEN: 新規契約 3 passed。
  - 関連: `test_document_reading_protocol_contract` + `test_ui_smoke` 含む 16 passed。全 pytest 527 passed, 1 skipped。
  - boundary **108/108 GREEN**（consult_tensor 54 + manifest 42 + ui_error 12）。tsc / Vite / cargo test·build·check GREEN。`git diff --check` GREEN。`.boundary-tests-out` 残存なし。
  - production 変更は `engine.ts` の当該関数削除のみ。Finding 16 未着手。commit/stage/push なし。

### [P3] .gitignore に境界テストの生成物ディレクトリが未登録

- 状態: RESOLVED
- 着手日: `2026-07-12`
- 解決日: `2026-07-12`
- commit: `UNCOMMITTED`
- 確度: `CONFIRMED`
- 分類: 3(境界運用)、16(手順の未整備)
- 問題事象: 境界テストハーネスのコンパイル出力先 `apps/desktop/.boundary-tests-out/`(golden fixture・一時 package.json を含む)が .gitignore に登録されておらず、手動削除だけに依存している。
- 裁定:
  - `/apps/desktop/.boundary-tests-out/` だけを ignore（ルート相対 exact 1行）
  - `apps/desktop/tests-runtime/**` は追跡対象として維持（ignore 禁止）
  - runner `finally` cleanup は第一防壁として維持し、ignore は中断・異常終了時の第二防壁とする
- 発生条件: 境界テスト実行後にクリーンアップを忘れた/中断した場合。
- 実害または潜在的影響: git status の汚染。`git add apps/desktop/...` 系の一括ステージで生成物(合成fixture含む)が誤コミットされるリスク。
- 根本原因: STEP 5 指令が「実行後削除」を人為手順としてのみ規定し、リポジトリ側の防御(ignore)を設けなかった。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\.gitignore` — `boundary-tests-out` / `tests-runtime` に一致する行なし(grep 0件)。注: 当時の grep 対象に `tests-runtime` を含めたが、これは ignore 対象ではなく source asset の誤除外有無を確認する意図である。
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\tsconfig.boundary.json` — `"outDir": ".boundary-tests-out"`
- 契約または文書との矛盾: 「git status に現れないこと」を検証項目とする運用(HANDOFF §10系)と、ignoreによる構造的保証の不在。
- 静的監査だけでは確定できない点: なし。
- baseline(着手時): artifact / tests-runtime とも `git check-ignore --no-index` 非0。`.gitignore` 無差分・対象規則なし。`test_dod_boundary_contract` 11 passed。Finding 1〜15 未commit差分保持。
- 解決追補(2026-07-12):
  - `.gitignore` の `# Tauri desktop app` 区間へ exact rule `/apps/desktop/.boundary-tests-out/` を1行追加。runner cleanup 維持。
  - artifact 代表パス3種（`golden.json` / `package.json` / `tests-runtime/probe.test.js`）が repo `.gitignore:41` 由来で ignore。
  - `apps/desktop/tests-runtime/**` 非ignore。類似名 `.boundary-tests-out-backup/` 非ignore。
  - RED: 契約1 FAIL / 契約2 FAIL / 契約3 PASS / 契約4 PASS（2 failed, 2 passed）。GREEN: 新規契約 4 passed。関連 `test_dod_boundary_contract` 含む 15 passed。
  - boundary **108/108 GREEN**（consult_tensor 54 + manifest 42 + ui_error 12）、`BOUNDARY_EXIT=0`、runner 後 `.boundary-tests-out` 不在。
  - 全 pytest 531 passed, 1 skipped。`test_ui_smoke` 1 passed。tsc / Vite / cargo test·build·check GREEN。`git diff --check` GREEN。
  - runner / tsconfig / package / lock / Cargo / production 無変更（Finding 16 起因）。Finding 17 未着手。commit/stage/push なし。

### [P3] メインタブナビゲーションに ARIA タブ意味論がない

- 状態: RESOLVED
- 着手日: `2026-07-12`
- 解決日: `2026-07-12`
- commit: `UNCOMMITTED`
- 確度: `CONFIRMED`
- 分類: 14(アクセシビリティ)
- 問題事象: タブ切替は素の `<button>` 列で実装され、選択状態は CSS クラス `active` のみで表現される。`role="tablist"` / `aria-selected` / `aria-current` 等が存在せず、スクリーンリーダーは現在タブを判別できない。リポジトリ内の他コンポーネント(MbtiGradientBars の `role="img"` / `aria-label`)はARIA配慮を実装しており、水準が不統一。
- 裁定:
  - WAI-ARIA Tabs Pattern の manual activation
  - empty tabpanel shell は DOM に置き、active component のみ mount（F-11 両立）
  - ArrowLeft/Right/Home/End は focus のみ（`setTab` 禁止）
  - Enter/Space（native button）/ click で activation
  - Alt+1..7 維持
- 発生条件: 支援技術での利用時(常時)。
- 実害または潜在的影響: 非視覚利用での現在位置喪失。Alt+1〜7のキーボード経路は存在するため操作自体は可能。
- 根本原因: タブシェル実装時にARIA意味論が仕様化されなかった。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\App.tsx:100-111` — className切替のみのボタン列
  - 対照: `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\components\MbtiGradientBars.tsx:31,57-58`（注: Finding 9 で後に削除済み。監査時点の対照証拠として保持）
- 契約または文書との矛盾: AI_SKILLS §3.4「キーボード操作を守れ」はキーボードのみ規定しており、ARIAは無規定(規律の空白)。
- 静的監査だけでは確定できない点: 実際の支援技術での読み上げ挙動。
- baseline(着手時): ARIA tab semantics 0件、Arrow/Home/End 0件、Alt+1..7 と click `setTab` 存在、条件レンダリング unmount、`.tabs button:focus-visible` なし。関連契約22 passed、tsc/build GREEN。Finding 1〜16 未commit差分保持。
- 解決追補(2026-07-12):
  - WAI-ARIA APG manual activation 採用。`tablist`/`tab`/`tabpanel`、`aria-selected`、決定論的 ID 相互参照、roving `tabIndex`。
  - Left/Right wrap・Home/End は focus only。Enter/Space/click で activation。Alt+1..7 維持。`handleTabKeyDown` 内 `setTab` なし。
  - empty tabpanel shell（`hidden`）+ active のみ `renderMainTab(id)` mount で F-11 両立。`aria-current` なし。focus-visible（既存 token）。
  - RED: 契約1〜7・10・12 FAIL / 8・9・11 PASS（9 failed, 3 passed）。GREEN: 新規12 passed。関連40 passed。
  - 全 pytest 543 passed, 1 skipped。boundary **108/108**（consult_tensor 54 + manifest 42 + ui_error 12）。tsc / Vite / cargo test·build·check GREEN。`git diff --check` GREEN。
  - backend/IPC/package/data/Cargo 無変更（Finding 17 起因）。実際の screen reader 製品での読み上げ確認は未実施（残存検証）。Finding 18 未着手。commit/stage/push なし。
- 検収是正(2026-07-12):
  - `renderMainTab` の switch 末尾に `const _exhaustive: never = id` + `throw` を追加。`MainTab` 拡張時に tsc が失敗する exhaustiveness を保証。
  - 契約 `test_09b_render_main_tab_exhaustive_never_guard` を追加（RED→GREEN）。アクセシビリティ契約 **13 passed**。関連41 passed。全 pytest / boundary 108/108 / tsc / Vite / cargo / `git diff --check` 再GREEN。commit/stage/push なし。

### [P3] import.stats が呼び出し毎に日記・LINE履歴の全文を読み込む

- 状態: `RESOLVED`
- 着手日: `2026-07-12`
- 解決日: `2026-07-12`
- commit: `UNCOMMITTED`
- 確度: `CONFIRMED`
- 分類: 11(不要I/O)
- 問題事象: IMPORTタブの統計表示(`import.stats`)は、件数算出のために `diary.md` と `line_history.txt` の全文を毎回読み込み・走査する(`_diary_entry_count` は全文regex、`_line_export_count` は全文count)。キャッシュ・mtimeゲートはない。取込直後の再表示でも全文再読込となる。
- 裁定:
  - UTF-8 逐次走査（`read_text` 禁止）
  - process-local fingerprint cache（`st_dev`/`st_ino`/`st_size`/`st_mtime_ns`、最大2件）
  - known-write 前 invalidate（`save_record` / `_append_line_text` / `import_line_history`）
  - pre/post fingerprint 一致時だけ cache 保存
  - `st_ino == 0`（identity 不明）時は cache せず再走査
  - calendar/finance は対象外
- 発生条件: IMPORTタブ表示・各取込完了後の統計更新のたび。LINE履歴が大きい(数十MB級エクスポートの蓄積)ほど顕在化。
- 実害または潜在的影響: タブ表示のもたつき、無駄なディスクI/O。破損はない。
- 根本原因: 軽量stat設計(F2裁定)が「stdlibのみ」を満たす一方、ファイルサイズ増加時の再計算コストを考慮していない。
- 証拠(監査時点):
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\facade.py:289-295` — 全文読込によるカウント2関数
  - `C:\Users\badger\Documents\cursur\decision_engine\src\python\core\facade.py:312-346` — `data_source_stats` が毎回呼ぶ
  - `C:\Users\badger\Documents\cursur\decision_engine\apps\desktop\src\components\ImportTab.tsx:122` — マウント時・操作後の `refreshStats`
- 契約または文書との矛盾: AI_SKILLS §3.2-2「ファイルI/Oをループに入れるな」の精神との緊張(ループではないが反復呼び出し経路)。
- 静的監査だけでは確定できない点: 実データ規模での実測遅延(実データ読取禁止のため測定不能)。ただし反復全文読込自体は実装・Sandbox 契約で確認可能。
- baseline(着手時): diary/LINE とも `read_text()`。cache/fingerprint/invalidation なし。`test_import_stats` 15 passed。`data/` 無差分。Finding 1〜17 未commit差分保持。
- 解決追補:
  - `_diary_entry_count` / `_line_export_count` を `path.open("r", encoding="utf-8")` による逐次走査へ変更。`read_text` 不使用、`errors=` fallback 不使用(`test_09_streaming_scanners_no_read_text` がソース文字列を検査)。
  - `_source_fingerprint(stat) -> (st_dev, st_ino, st_size, st_mtime_ns) | None`。`st_ino == 0` は `None`(identity 不明として cache 無効化)。`st_ctime`/`st_ctime_ns` は不使用。
  - `_SOURCE_COUNT_CACHE: dict[Path, (fingerprint, count)]`。所有対象は `_CACHEABLE_SOURCE_PATHS = frozenset((DIARY_MD, LINE_HISTORY))` の最大2件のみ(`test_10_bounded_process_local_cache`)。sidecar/JSON/pickle/sqlite/Timer/Thread/ttl は存在しない(ソース文字列検査で確認)。
  - `_cached_source_count()`: scan前後で `path.stat()` を再取得し、fingerprint 不一致(scan中の変更)・scanner例外・ファイル消失のいずれでも cache へ書き込まない。scanner例外は握り潰さず伝播する(`test_05`/`test_06`)。
  - known-write invalidation を3経路に配線: `save_record()` の日記更新前(`_invalidate_source_count(DIARY_MD)`)、`_append_line_text()` の追記open前、`import_line_history()` の追記open前(いずれも `_invalidate_source_count(LINE_HISTORY)`)。
  - `data_source_stats()`: diary/line のみ `_cached_source_count` を経由。calendar/finance/es/knowledge は従来どおり毎回直接計算(`test_12_calendar_finance_not_cached`)。出力shape(`exists`/`count`/`mtime`)は不変(`test_11_output_shape_preserved`)。IPC(`engine_stdio.py` の `import.stats` dispatch)・frontend(`ImportTab.tsx` の `refreshStats()` 頻度)は無変更。
  - `docs/AI_SKILLS.md`(反復ファイル集計の禁止規律を §3.2 へ追記)・`docs/SPEC_FOXTROT_UI.md`(§2.2.1 裁定4への「Finding 18 追補」を追記)を同期。
- 検証結果:
  - `tests/test_import_stats_cache.py`: 12 passed(atomic replacement すり替え・scan中変更・scanner例外・delete/recreate・known-write invalidation・bounded cache・output shape・calendar/finance対象外の全契約)
  - `tests/test_import_stats_cache.py tests/test_import_stats.py tests/test_integration.py tests/test_ui_smoke.py`: 70 passed
  - `pytest tests/ -q`: 556 passed, 1 skipped(Finding 18起因の新規失敗なし)
  - `apps/desktop`: `npm run test:boundary`(manifest 42/42 + ui_error 12/12)・`tsc --noEmit`・`npm run build`(67 modules)いずれも GREEN
  - `apps/desktop/src-tauri`: `cargo test`(46 passed)・`cargo build`・`cargo check` いずれも GREEN(Finding 2 由来のdead-code警告のみ、Finding 18とは無関係)
  - `git diff --check`: exit 0。`git status --short data`: 空。`.boundary-tests-out` 残留なし
  - 変更ファイル(Finding 18分): `src/python/core/facade.py`、`tests/test_import_stats_cache.py`(新規)、`docs/AI_SKILLS.md`、`docs/SPEC_FOXTROT_UI.md`、本台帳。禁止領域(`tests/test_import_stats.py`・`calendar_manager.py`・`engine_stdio.py`・`apps/desktop/**`・`src/cpp/**`・`data/**`・`.claude/**`・Finding 1〜17/19以降)は無変更
  - 実データによるbenchmarkは未実施(禁止どおり)。反復全文読込の除去はscan呼出回数の直接観測(spy)で構造的に証明。
- 残存リスク:
  - 外部ツールが同一 file identity・同一 size・同一 `mtime_ns` を偽装して内容だけ変更した場合、process restart または既知 write 経路での invalidation まで stale となり得る(通常の編集・atomic replacement・内部書込は検出/invalidate 済み)。
  - identity 取得不能環境(`st_ino == 0`)では cache を無効化し正確性優先で毎回再走査するため、当該環境では性能改善の効果が得られない。
  - Finding 19以降は未着手。commit/stage/push は未実施。
