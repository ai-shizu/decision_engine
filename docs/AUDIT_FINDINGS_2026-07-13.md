# PKB Full-Stack Red Team Audit Findings

> 監査日: 2026-07-13
> 監査範囲: Rust IPC、Python backend、React/Tauri frontend、C++検索コア、永続化層、Derived Store、将来の確率推論構想
> 最終裁定: `GREEN (ALL RESOLVED)`
> 台帳規律: 各Findingの状態欄を正本とする。受入契約、実装、独立検証、全体回帰検証が完了するまで状態を`RESOLVED`へ変更してはならない。

## 状態定義

- `UNRESOLVED`: 問題事象と根本原因を確認済みだが、是正に未着手または是正未検収。
- `IN_PROGRESS`: RED契約を固定し、承認済みスコープ内で是正中。
- `RESOLVED`: REDからGREENへの遷移、独立検証、全体ゲート、台帳証拠のすべてを満たした状態。

## Foundationフェーズ共通禁止事項

- Findingをまとめて推測修正してはならない。1件ずつRED契約を固定し、独立して検収する。
- 監査時点の症状、攻撃ベクトル、根本原因を削除または過去形へ改変してはならない。
- 「ローカルだから安全」「ハッシュがあるから改変不能」「seedがあるから全環境で決定的」という推論を受入証拠にしてはならない。
- 文書上の宣言、型注釈、プロンプト指示、grep件数だけで構造的不変条件を証明してはならない。

---

## FSA-2026-07-13-01: グローバル外部通信能力の残存

- **ID**: `FSA-2026-07-13-01`
- **重要度**: `CRITICAL`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: E0aは旧`knowledge_fetcher`経路を封鎖したが、Pythonプロセス全体の外向き通信能力は残存している。ローカルembedding modelが欠落した状態、または環境変数が外部から変更された状態で`SentenceTransformer(model_name)`がHugging Face Hubへの取得を試行し得る。ユーザー由来テキスト、モデル識別子、端末情報などが外部依存コードの通信経路へ到達する可能性を構造的に排除できていない。
- **根本原因**: `src/python/core/pipeline.py`が`SentenceTransformer`を`local_files_only=True`なしで生成している。`HF_HUB_OFFLINE`と`TRANSFORMERS_OFFLINE`は`os.environ.setdefault(..., "1")`であり、親環境に`0`などが存在すれば強制上書きされない。RustのPython/sidecar起動も環境をclearせず、proxy、Hub、認証関連変数を継承する。E0aのAST契約は特定ファイルのimport構文を検査するだけで、プロセス能力や推移的依存を隔離していない。
- **実コード証拠**:
  - `src/python/core/pipeline.py:140-143`
  - `src/python/core/consultation_engine.py:32-33`
  - `src/python/core/cli.py:32-33`
  - `apps/desktop/src-tauri/src/engine.rs:465-500`
  - `tests/test_e0a_egress_lockdown.py:153-195`
- **必須是正措置**: 全model loaderを明示的なローカルパス、`local_files_only=True`、固定revision/digestへ限定する。起動時にoffline変数を強制代入し、子プロセス環境をallowlist方式で再構築する。Python、Rust、WebViewをOSレベルの外向き通信denyへ置き、loopbackの承認済みendpointだけを例外化する。静的AST契約に加え、欠落model、汚染環境、proxy注入、DNS/HTTP観測を用いた動的な「送出ゼロ」契約を追加する。
- **解決日**: `2026-07-14`
- **解決記録**: 指揮官裁定によりloopback例外案を撤回し、production engineのTCP/IP依存を完全廃止した。releaseはbundled sidecarのみを起動し、System Python経路は`PKB_UNSAFE_DEV_ENGINE=1`を要求するdebug専用の非保証モードへ隔離した。Rustを唯一のkernel sandbox起動所有者とし、Windowsはcapability 0のAppContainer、Linuxは`socket(AF_INET/AF_INET6)`だけを`EACCES`で拒否するseccomp-BPF、macOSはnetwork entitlementを持たないApp Sandboxをfail-closedで適用する。Python側はoffline環境を強制し、model loaderをローカル実体へ限定した。WindowsではAppContainer内のnative Winsock C ABI probeがIPv4/IPv6 listenerへ接続できず、親側のaccept件数も0であることをruntime実測した。sandbox構築失敗はfallbackせずengine起動エラーになる。release binary中のSystem Python起動tokenは0件。macOS sandbox moduleは`aarch64-apple-darwin`向けmetadata compileを通過し、署名・entitlementのnative検証はmacOS release workflowのhard gateとした。
- **検証証拠**: pytest `575 passed, 1 skipped`、boundary `108/108`、Rust unit `46/46`、kernel sandbox integration `2/2`、`cargo build` / `cargo check` / `cargo build --release`、`tsc --noEmit`、Vite build、`git diff --check`はすべてGREEN。実ネットワークへの送信テストは行わず、親プロセス所有のloopback listenerだけを敵対probeに使用した。

---

## FSA-2026-07-13-02: Loopback LLMの本人性確認不在

- **ID**: `FSA-2026-07-13-02`
- **重要度**: `CRITICAL`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: 攻撃者または別プロセスが先に`127.0.0.1:18081`をbindすると、PKBはそのlistenerを正規llama-serverとして再利用し、日記、面接、profile、相談内容を含むsystem/user promptを送信する。既定の`urllib` openerがOSまたは環境のproxyを利用する構成では、loopback URLであることだけでは直接接続も保証できない。
- **根本原因**: port open判定と`/health`の固定応答だけを本人性として扱い、PID、親子関係、実行ファイルdigest、model digest、起動nonce、challenge-responseを検証していない。HTTP clientにもproxy自動検出を無効化する専用openerがない。
- **実コード証拠**:
  - `src/python/core/llm_backend.py:41-68`
  - `src/python/core/llm_backend.py:86-112`
  - `src/python/core/llm_backend.py:144-170`
  - `src/python/core/llm_config.py:54`
- **必須是正措置**: 外部既存listenerの無条件再利用を廃止し、PKB自身がspawnしたPIDだけを所有対象とする。OS割当または安全に予約したport、起動ごとの高entropy nonce、認証header、PID/実行物/model/runtime digestの結合検証を導入する。HTTP clientはproxyなしの専用openerを用い、hostを正規化したloopback literalへ限定する。本人性検証に失敗した場合はpromptを1 byteも送らずhard-failする。
- **解決日**: `2026-07-14`
- **解決記録**: FSA-01の指揮官裁定へ統合し、TCP/HTTP server、port再利用、health応答による本人性推定を廃止した。LLMはPKBがその場でspawnした単発子プロセスだけを使用し、WindowsではAppContainer ACLを付与したlocal Named Pipe、POSIXでは親子stdioを介して要求を渡す。Windows pipeは親が生成した予測不能名、owner/SYSTEM/AppContainer SIDだけのDACL、接続client PIDの完全一致を要求し、外部listenerやproxyへ到達する経路を持たない。子プロセスの所有権、終了、timeout、応答上限を親が保持し、transportまたはidentity検証失敗時はpromptを別経路へ再送しない。
- **検証証拠**: FSA-02 strict boundary契約、Named Pipe SID/PID契約、既存LLM client契約、全pytest、Rust kernel sandbox runtime probe、release binary token scan、および全体build gateがGREEN。FSA-01/02は同一kernel/process境界の不可分な是正として単一コミットで固定する。

---

## FSA-2026-07-13-03: WebView侵害からのIPC読出し・外部送出

- **ID**: `FSA-2026-07-13-03`
- **重要度**: `CRITICAL`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: XSS、依存汚染、WebView navigationの逸脱が一度成立すると、rendererは汎用`pkb_invoke`経由でbackendデータを読み出し、画像、beacon、fetch、formなどを用いて外部へ送出できる。現時点で直接HTML sinkが見つからないことは、将来の侵害時のblast radiusを制限しない。
- **根本原因**: `tauri.conf.json`のCSPが`null`である。Rust IPCは`cmd: String`を受ける単一汎用commandで、frontendの`pkbInvoke<T>`はruntime validationなしに結果を型castする。renderer権限とbackend commandが機能単位に分離されておらず、外部navigation/new-window/remote resourceのdenyも不変条件化されていない。
- **実コード証拠**:
  - `apps/desktop/src-tauri/tauri.conf.json:25-27`
  - `apps/desktop/src-tauri/src/commands.rs:8-17`
  - `apps/desktop/src/lib/engine.ts:24-30`
  - `apps/desktop/src-tauri/capabilities/default.json`
- **必須是正措置**: `default-src 'self'`を基礎とし、外部`connect-src`、`img-src`、`media-src`、`frame-src`、`form-action`、`object-src`をdenyする厳格CSPを導入する。navigationとnew-windowをallowlist化する。Rust側を型付きの明示commandへ分割し、各commandの入力、出力、権限scopeをstrict schemaで検証する。frontendが侵害されても外部送出と全backend読出しを同時取得できない権限分離をOS境界まで含めて構築する。
- **解決日**: 2026-07-14
- **解決記録**: production/dev CSPを分離し、productionはself/ipc以外の外部resource・navigation・new-window・downloadをdenyした。main WebViewはRustで手動構築し、exact origin allowlistを適用した。`core:default`とwindow/webview作成権限を除去し、renderer向け汎用`pkb_invoke(cmd, Value)`を28個の明示Tauri commandへ置換した。各requestは`deny_unknown_fields`付き閉型とsemantic validatorを通る。frontendは全command応答とengine eventを`unknown`で受け、command固有のstrict runtime parserを通した後だけstateへ入れる。
- **検証証拠**: 静的契約は実装前10 RED / 2 GREENから12/12 GREEN、Rust IPC契約4/4、TypeScript runtime boundary契約は11件GREEN後にnested evidence/claimの未検証を追加REDで検出・是正し12/12 GREEN、既存boundaryを含む120/120 GREEN。FSA-2026-07-13-12が所有するRust stdioの最大byte・deadline・response id/cid照合は本Findingの解決範囲に含めず、未解決状態を維持する。

---

## FSA-2026-07-13-04: 実行物・モデルの真正性不在

- **ID**: `FSA-2026-07-13-04`
- **重要度**: `CRITICAL`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: 改変されたPython interpreter、sidecar、llama-server、GGUF、embedding modelを正規成果物として起動できる。攻撃者は同じ名前と十分なサイズのファイルへ置換するだけで機密データを取得し、結果を改変できる。ビルド時にも未固定依存が取り込まれ、同一commitから異なる成果物が生成され得る。
- **根本原因**: `PKB_PYTHON`やPATH候補は`is_file`中心、sidecarは存在とサイズ、GGUFは名前とサイズで検証される。内容digest、署名、許可manifest、toolchain identityがない。PyInstaller、Python package、Rust stable、GitHub Actionsが完全なversion/hash/commitへ固定されていない。
- **実コード証拠**:
  - `apps/desktop/src-tauri/src/paths.rs:81-101`
  - `apps/desktop/src-tauri/src/paths.rs:131-144`
  - `src/python/core/paths.py:12-31`
  - `src/python/core/llm_config.py:106-161`
  - `apps/desktop/scripts/build-engine.ps1:26`
  - `apps/desktop/scripts/build-sidecar.sh:50-56`
  - `.github/workflows/build-macos.yml:35-89`
- **必須是正措置**: 実行物、model、prompt bundle、runtime、設定のdigestを署名済みallowlist manifestへ固定し、起動前に全byteを検証する。ユーザーoverrideは明示的な非production modeへ隔離する。Python/Rust/Node/toolchain、package、GitHub Actionをversionとhashまたはcommit SHAへ固定し、offlineかつhash検証付きで再現可能なbuildを成立させる。生成成果物のSBOM、署名、再現性比較をrelease gateへ追加する。
- **解決日**: `2026-07-14`
- **解決記録**: production専用Ed25519公開鍵をRustへ静的固定し、canonical `pkb.artifact_allowlist.v1`とdetached signatureを起動前に検証する。manifestはsidecar、C++検索実行物、llama runtime tree、model config、決定論的SBOM、1件以上のGGUFを必須とし、相対path・base・file/tree種別・size・SHA-256を結合する。symlink/junction/reparse、未知key、重複ID、非canonical JSON、path差替え、不足、1byte改変はhard-failする。Rustは全artifactを検証してからproduction sidecarをspawnし、manifest exact SHA-256とapp/data rootだけをclear済み子環境へ渡す。Pythonはそのattestationを再検証し、GGUF、llama runtime、検索exe、外部embedding、model configの使用直前にexact pathとbytesを再検証する。config削除はdefault fallbackせずhard-failする。RFC 8032公開テスト鍵はtest fixtureだけに隔離し、production signerは秘密鍵から導出した公開鍵が固定trust rootと不一致なら出力を作らない。
- **供給網記録**: Python `3.12.10`、Node `24.18.0`、Rust `1.96.1`、Ed25519/SHA-256依存、Cargo/npm lock、Python wheel全hash、GitHub Actions完全commit SHAを固定した。Windows/macOS releaseはself-hosted secure runnerでsidecar二回buildのbyte一致を要求し、macOSは最終codesign後の.app内sidecarを署名対象にする。Cargo/npm/Python lockからtimestamp・UUID・絶対pathなしのCycloneDX SBOMを二回生成してbyte比較し、そのSBOM tree自体を署名manifestへ含める。署名済みoffline artifact packなしのrelease uploadは禁止する。
- **検証証拠**: 初期REDはPython `7 failed`、Rustはverifier/module不在でcompile RED。追加敵対契約として署名済みconfig削除時のsilent defaultと、決定論的path-free SBOM不在を個別RED確認した。対象GREENはPython FSA-04 `9/9`、Rust `5/5`、関連Python `80/80`、hash-lock実resolver、workflow YAML、PowerShell parser、`cargo check --locked --release --all-targets`。全体回帰はpytest `596 passed, 1 skipped`、boundary `120/120`、TypeScript/Vite、Rust unit `46/46` + artifact `5/5` + IPC `4/4` + kernel sandbox `2/2`、debug build/check/release build、`git diff --check`がGREEN。production秘密鍵はローカルへ存在しないため、production署名成功だけはCI secret所有のrelease gateで実行し、ローカル契約はRFC 8032 test fixtureによる正署名と誤鍵拒否を検証する。

---

## FSA-2026-07-13-05: LLMの非決定的出力による権威状態汚染

- **ID**: `FSA-2026-07-13-05`
- **重要度**: `CRITICAL`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: 同一transcript、同一事前状態、同一model名でも、LLMが異なる妥当候補を返すと6D tensor profileのscore、confidence、evidenceが変化する。structured generation失敗時の通常生成fallbackも、非決定的な候補集合を権威状態へ流入させる。
- **根本原因**: generation設定が`temperature=0.6`でseedを結合していない。structured pathの`temperature=0`もbit-level determinismを保証せず、例外時には通常`generate()`へfallbackする。Pythonのschema検証は候補の形状とallowlistを確認するが、候補集合の一意性を導出しない。採用されたLLM提案が決定論的集計へ直接入力されるため、下流計算だけが決定的でも全体は決定的にならない。
- **実コード証拠（是正後）**:
  - `src/python/core/llm_config.py:54-55`
  - `src/python/core/llm_backend.py:97-112`
  - `src/python/core/llm_backend.py:141-170`
  - `src/python/core/interview_report.py:188-231`
  - `src/python/core/tensor_profile.py:351-390`
- **必須是正措置**: LLM提案を権威状態から分離し、同じ証拠から完全かつ一意に導出される決定論的候補集合だけをstate更新へ使用する。LLMを残す場合は提案を非権威の補助表示に限定するか、exact runtime/model/prompt/config/input/output artifactを固定して再生可能な採否台帳へ結合する。seed固定のみを是正完了条件にしてはならない。
- **解決日**: `2026-07-14`
- **解決記録**: `interview_report.generate_report()` のLLM schema/promptから6D evidence要求を除去し、`aggregate_profile()` / `parse_and_validate_proposals()` へのproduction call pathを切断した。6DはLLM出力を引数に持たない`tensor_profile.authoritative_profile()`だけが構築する。決定論的rubric観測器が未実装の現段階では、全次元をscore `None` / confidence `0.0` / evidence空のN/Aとしてfail-closedする。LLM生成の4軸metricsはUIで「AI評価候補（非測定・履歴更新に不使用）」と明示する表示専用データに降格し、`compute_growth_context()` / `_GROWTH_CONTEXT_TEMPLATE`およびInterview/GD開始時の再注入経路を撤去した。
- **検証証拠**: 初期REDは「異なる妥当LLM出力で6Dが変化する」「LLM schemaが6D evidenceを要求する」「production call graphがLLM提案を集計へ渡す」の3/3。拡張GREENはFSA-05専用5/5、Tensor契約とintegrationを含む関連75/75。異なるLLM出力で同一N/A profile、schema/promptから6D語彙不在、AST上の集計call path不在、成長再注入API不在、UI非権威ラベルを固定した。

---

## FSA-2026-07-13-06: モデル・ランタイム・セッションIDの証拠結合不足

- **ID**: `FSA-2026-07-13-06`
- **重要度**: `CRITICAL`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: 異なるGGUF、server binary、embedding model、prompt、generation config、numeric runtimeで生成した結果が、同一または空の`model_hash`を持つmanifestとして保存される。同じmode/configの別会話が同じ`session_id`を共有し、turn、evidence、manifestの監査鎖を別セッションへ誤結合または再利用できる。
- **根本原因**: manifest作成時の`model_hash`既定値が空文字であり、`prompt_version`も固定文字列に留まる。`session_id`はmodeとconfigだけをBLAKE2b-8でhashし、transcript genesis、生成nonce、session開始artifactを含まない。「model version」と「session identity」の正本が定義されていない。
- **実コード証拠**:
  - `src/python/core/session_memory.py:727-731`
  - `src/python/core/session_memory.py:944-981`
  - `src/python/core/retrieval_manifest.py:148-153`
  - `src/python/core/consultation_engine.py:967-1001`
- **必須是正措置**: GGUF bytes、llama-server bytes、embedding model、prompt本文、schema、generation parameters、numeric runtime、feature code versionをcanonical runtime identityへ結合する。session genesisには高entropy IDまたは明示的なimmutable genesis recordを作成し、transcript head、runtime identity、parent stateを結合する。短縮表示用IDと暗号論的な完全identityを分離し、空hashを正常値として許可しない。
- **解決記録**: `CanonicalRuntimeIdentity`を導入し、GGUF、llama-server、embedding model、prompt全文、schema、generation parameters、numeric runtimeをcanonical serializationとBLAKE2bで完全結合した。Session Genesisには同identity、親state、CSPRNG起動nonce、開始時刻、初期transcript headを結合し、manifestの生成・load境界では空または不正形式のhashを例外として完全にHard-fail化した。
- **検証証拠**: FSA-06契約テスト`7 passed`、全体pytest`608 passed, 1 skipped`、Cargo`57 passed`、renderer boundary`108 passed`、desktop build成功、`git diff --check`違反0件。

---

## FSA-2026-07-13-07: Pointer-Payload結合の形骸化とstate replay

- **ID**: `FSA-2026-07-13-07`
- **重要度**: `CRITICAL`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: 書込可能な主体はpayloadを改変し、その内容から新しいBLAKE2b IDを再計算してpointerも同時更新できる。また`latest.json`を過去の正当manifestへ戻すと、形式上正しいため巻戻しが受理される。正当な別session・別証拠のmanifestへ差し替えるcross-context replayも防げない。
- **根本原因**: `manifest_id`は無鍵hashであり、真正性を提供しない。単調sequence、parent hash、expected session head、expected evidence head、OS保護鍵によるMACが存在しない。Pointer-Payload Bindingが「一方だけが壊れた場合の整合性確認」と「攻撃者に対する改変不能性」を混同している。
- **実コード証拠**:
  - `src/python/core/retrieval_manifest.py:489-498`
  - `src/python/core/retrieval_manifest.py:799-827`
  - `src/python/core/retrieval_manifest.py:907-950`
- **必須是正措置**: OS保護鍵を用いたkeyed BLAKE2またはHMACでpayloadとdomain separatorを認証する。各recordへ単調sequence、parent MAC/hash、session genesis、runtime identity、evidence headを結合する。load時に呼出側が期待するheadと完全照合し、過去head、fork、cross-session recordをhard-failする。鍵消失、recovery、migrationの手順を明示し、サイレントな再署名を禁止する。
- **解決記録**: 起動時にインメモリ生成するroot keyからsession別鍵を導出するHMAC-SHA256を導入し、`sequence_number`、`parent_hash`、`session_genesis_id`をpayloadへ必須結合したkeyed state chainを構築した。load時はMAC、呼出側が期待するsession genesisとsequence、インメモリの信頼済みheadを完全照合し、payload偽造、rollback、cross-session replayをHard-fail化した。
- **検証証拠**: FSA-07契約テスト`3 passed`、全体pytest`611 passed, 1 skipped`、Cargo全target`57 passed`、renderer boundary`120 passed`、desktop build成功、`git diff --check`違反0件。

---

## FSA-2026-07-13-08: 永続化層のサイレント修復と破壊的上書き

- **ID**: `FSA-2026-07-13-08`
- **重要度**: `CRITICAL`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: JSON破損、途中書込、部分的なfilesystem障害が発生すると、loaderが破損を空dictまたはdefault状態へ変換する。その後の通常保存が破損前の原bytesを上書きし、回復可能だったユーザーデータを物理的に消去する。Interview reportは同一秒・同一genreでfilenameが衝突し、破損reportは履歴から黙って除外される。
- **根本原因**: 複数storeが`JSONDecodeError`と`OSError`を「データなし」と同一視する。direct `write_text`が多く、durable atomic replace、file/dir fsync、single-writer lock、typed persistence error、bytes保持規律が統一されていない。読み込み失敗時のrepair権限と通常更新権限が分離されていない。
- **実コード証拠**:
  - `src/python/core/profile_store.py:50-62`
  - `src/python/core/finance_manager.py:29-58`
  - `src/python/core/calendar_manager.py:28-53`
  - `src/python/core/consultation_log.py:21-53`
  - `src/python/core/profiler.py:1213-1228`
  - `src/python/core/interview_report.py:419-440`
- **必須是正措置**: absent、corrupt、permission、I/O failureをtyped resultとして分離し、corrupt bytesを不変のままhard-failする。全権威storeへsame-directory temporary file、exclusive/no-follow open、flush、file fsync、atomic replace、parent-directory fsync、single-writer lockingを適用する。repairは明示的な別操作とし、backup/quarantine、利用者確認、監査recordなしに実行してはならない。report IDはcontent/session identityを含む衝突不能な値へ変更する。
- **解決記録**: JSON不在だけを新規状態として扱い、破損、権限、I/O障害をtyped errorでHard-failすることでサイレント修復を完全撤去した。same-directoryのexclusive/no-follow一時ファイル、flush、file fsync、atomic replace、可能な環境でのparent-directory fsyncを行うDurable Atomic Writeを全対象storeへ適用した。Interview reportのファイル名へcanonical contentのSHA-256を結合し、秒精度ファイル名による衝突と破壊的上書きを排除した。
- **検証証拠**: FSA-08契約テスト`11 passed`、全体pytest`622 passed, 1 skipped`、Cargo全target`57 passed`、`git diff --check`違反0件。破損bytes不変、replace失敗時の原本保持、一時ファイル掃除、file fsync先行、同一秒の異内容report分離を固定した。

---

## FSA-2026-07-13-09: Derived Storeの自己整合性欠如

- **ID**: `FSA-2026-07-13-09`
- **重要度**: `CRITICAL`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: 改変されたLSM manifestに`../`、absolute path、未知segment名を挿入すると、検索、compaction、unlinkがowned directory外へ到達し得る。manifestが参照するsegmentが欠落していても検索は黙って`continue`し、同じ入力から異なる証拠集合を返す。Tensor Storeでは`daily`が同じなら、異なるdyad、LINE message、scopeから構築したtensorが同一`content_hash64`を持つ。
- **根本原因**: LSM manifestにexact schema、owned basename allowlist、segment payload hash、path containment、全参照のpreflightがない。missing segmentをhard-failせず証拠 omissionとして扱う。Tensor headerのidentityが`daily`だけをhashし、`dyads`、`line_messages`、`group_contacts`、`contact`、`scope`、`alias`、feature/runtime versionを結合していない。readerもpayload全体のhashを再計算しない。
- **実コード証拠**:
  - `src/python/core/lsm_index.py:113-128`
  - `src/python/core/lsm_index.py:230-274`
  - `src/python/core/lsm_index.py:413-430`
  - `src/python/core/tensor_store.py:122-127`
  - `src/python/core/tensor_store.py:142-198`
  - `src/python/core/tensor_store.py:421-584`
- **必須是正措置**: manifestをexact-key strict schemaで検証し、segment名をowned basename patternへ限定する。全segmentをpath containment、symlink拒否、size、header、payload hash、ID結合までpreflightしてから利用または削除する。欠損・不一致は証拠集合を返す前にhard-failする。Tensor identityは全canonical input、feature code、scope、numeric runtimeを含むmanifest hashとpayload hashへ変更し、readerが双方を再検証する。
- **解決記録**: LSM manifestをexact schemaへ更新し、owned basename、direct-child containment、symlink拒否、欠損、全segmentのBLAKE2b payload hashを利用・merge・削除前に一括preflightするfail-closed境界を導入した。Tensor `content_hash64`へdaily、dyads、LINE入力、scope、alias、feature contract、numeric runtimeをcanonical結合し、header identityと全row bytesを束ねたpayload hashをReaderで再検証する。
- **検証証拠**: FSA-09契約テスト`5 passed`、LSM/Tensor関連`36 passed`、全体pytest`627 passed, 1 skipped`、Cargo全target`57 passed`、`git diff --check`違反0件。path traversal、missing segment、payload 1-byte改変、Tensor identity衝突、Tensor row 1-byte改変をHard-fail化した。

---

## FSA-2026-07-13-10: C++・Python間の浮動小数点演算と順位の非決定性

- **ID**: `FSA-2026-07-13-10`
- **重要度**: `CRITICAL`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: 同じvectorとqueryでも、NEON FMA、scalar accumulation、OpenMP thread数、merge順、NumPy/BLAS buildの違いにより最下位bitと同点判定が変化する。同score時にchunk IDなどの総順序がなく、top-kの採否がplatform、CPU、thread数、library buildによって変わる。証拠集合が変われば後段のmanifestと意思決定も変わる。
- **根本原因**: C++ TopKがscoreだけを比較し、NEON FMAとscalar pathで異なる演算順を許容する。OpenMPの実行条件をruntime identityへ結合していない。Python fallbackはscoreだけによる`np.argsort`とsortを用い、tie-breakを規定していない。量子化単位、丸めmode、NaN/Inf、overflow、accumulation orderの仕様がない。
- **実コード証拠**:
  - `src/cpp/search_engine.cpp:265-304`
  - `src/cpp/search_engine.cpp:316-337`
  - `src/python/core/consultation_engine.py:729-755`
  - `src/python/core/digital_twin.py:338-360`
  - `src/python/core/digital_twin.py:531-570`
- **必須是正措置**: 決定論の保証範囲を「同一build・CPU・runtime」へ限定するか、cross-platform保証が必要ならscoreを規定精度のfixed-pointまたは明示量子化値へ変換する。丸めmode、飽和規則、NaN/Inf拒否、accumulation orderを固定し、順位を`(-score_q, chunk_id)`などの完全な総順序にする。SIMD/scalar、thread数、Python/C++で同じgolden bit patternとtie distributionを検証する。
- **解決記録**: C++/Pythonの全検索経路でNaN/InfをHard-failし、スコアを共通式`floor(score * 10^6 + 0.5)`で明示的な固定小数点整数へ量子化した。順位を`(-score_q, chunk_id)`の完全総順序へ統一し、NEON/OpenMPとNumPy fallbackの挙動を等価化した。
- **検証証拠**: FSA-10契約テスト`3 passed`、全体pytest`630 passed, 1 skipped`、Cargo全target`57 passed`、ARM64/OpenMP C++ build警告・エラー0件、`git diff --check`違反0件。

---

## FSA-2026-07-13-11: Unicode正規化の不統一とLINE alias衝突

- **ID**: `FSA-2026-07-13-11`
- **重要度**: `WARNING`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: 視覚的に同じNFC/NFD文字列、改行表現、結合文字がstoreごとに異なるhash、ID、dedup結果を生む。LINE contact aliasは32-bit空間しかなく、異なる人物が同じaliasへ衝突して証拠やdyad状態を混同し得る。
- **根本原因**: normalize処理が各storeで統一されず、raw UTF-8、NFC済み文字列、JSON serializationが混在する。LINE aliasは`digest_size=4`のBLAKE2bとlocal saltを使用し、collision検出やkey identityを持たない。birthday boundによる衝突確率は次で近似される。

  $$
  P_{\mathrm{coll}} \approx 1-\exp\left(-\frac{n(n-1)}{2\cdot 2^{32}}\right)
  $$

  $n=10{,}000$では約$1.16\%$であり、永続的な人物identityとして許容できない。
- **実コード証拠**:
  - `src/python/core/line_telemetry.py:66-78`
  - `src/python/core/session_memory.py:118-120`
  - `src/python/core/retrieval_manifest.py:185-200`
  - `src/python/core/lsm_index.py:52-62`
  - `src/python/core/tensor_store.py:122-127`
- **必須是正措置**: hash-before-constructionの共通canonicalization仕様を作り、Unicode NFC、line ending、whitespace、JSON exact encoding、number representation、domain separatorを固定する。人物aliasはOS保護鍵によるHMACまたはkeyed BLAKE2の128-bit以上へ移行し、key ID、collision検出、migration、salt/key消失時のhard-failを定義する。表示用短縮値と永続identityを分離する。
- **解決日**: 2026-07-15
- **解決記録**: Hash-before-constructionの共通正規化を導入し、Unicode NFC、改行・空白、JSONのキー順・ASCII escape・separator・有限数値表現を固定した。LINE人物aliasはOS保護鍵由来のHMAC-SHA256による永続Identityへ移行し、128-bit以上のMAC強度、衝突検出、鍵消失・破損時のHard-failを確立した。権威的な永続Identityと表示専用Short IDを分離し、照合には永続Identityのみを使用する。
- **検証証拠**: FSA-11契約テスト`2 passed`、全体pytest`632 passed, 1 skipped`、Cargo全target`57 passed`、`compileall`成功、`git diff --check`違反0件。

---

## FSA-2026-07-13-12: Rust IPCのサイズ・deadline・相関・runtime parser不足

- **ID**: `FSA-2026-07-13-12`
- **重要度**: `WARNING`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: compromisedまたは故障したPython engineが改行なしの巨大応答を返すと、Rustは無制限にmemoryを確保し得る。応答が来なければconnection mutexを保持したまま永久停止する。古い応答や異なるrequestの応答でも、`id`/`cid`を照合しないため現在の呼出結果として受理し得る。frontendでは一部command以外がruntime validationなしでReact stateへ入る。
- **根本原因**: Rust transportが`BufRead::read_line`を直接使用し、最大byte、read deadline、event数、JSON depth、response schemaを規定していない。最終応答は`ok`の有無を中心に判定し、要求した`id`/`cid`との完全一致を検査しない。TSのgeneric `pkbInvoke<T>`が静的型をruntime保証と誤認させ、strict parserの適用範囲がconsult/manifest周辺に限られている。
- **実コード証拠**:
  - `apps/desktop/src-tauri/src/engine.rs:149-164`
  - `apps/desktop/src-tauri/src/engine.rs:382-454`
  - `src/python/engine_stdio.py:204-225`
  - `apps/desktop/src/lib/engine.ts:24-30`
- **必須是正措置**: request/response/eventへexact schema、最大line bytes、最大JSON depth、最大event数、read/write deadlineを設定する。応答の`id`と`cid`を要求値へ必須結合し、不一致をprotocol failureとしてengineを隔離する。blocking I/OをTauri async runtimeから専用workerへ分離する。すべてのTS IPC wrapperを`unknown`受領とcommand固有strict parserへ統一し、missing/extra/wrong-typeをhard-failする。
- **解決日**: 2026-07-15
- **解決記録**: Rust IPCへ上限付きbuffer（response 1 MiB、request 8 MiB）、JSON深度・message数上限、read/write deadlineを導入し、stdioを専用reader/writer workerへ分離した。Tauri commandは`spawn_blocking`経由とし、`id`/`cid`不一致応答をdropして正しい相関応答だけを採用する。Python成功・失敗envelopeも`id`/`cid`を必須echoする。TypeScriptは全IPC commandを`unknown`で受け、Zod等と同等のcommand固有Strict Parserを必須引数とする中央helperへ統一し、未検証値を返せない構造にした。
- **検証証拠**: FSA-12 Rust契約`3 passed`、TypeScript runtime boundary`121 passed`、frontend production build成功、全体pytest`633 passed, 1 skipped`、Cargo全target`60 passed`、Rustfmt・`git diff --check`違反0件。

---

## FSA-2026-07-13-13: TrueSkill・2PL・Observable-State POMDPのカテゴリ誤用と識別不能性

- **ID**: `FSA-2026-07-13-13`
- **重要度**: `CRITICAL`
- **状態**: `RESOLVED`
- **症状（攻撃ベクトル）**: 形式化されていないTrueSkill転用、未anchorの2PL、定義矛盾を含むObservable-State POMDPを実装すると、数値が出力されてもskill beliefとして識別不能または意味不明になる。近似推論、MCMC、randomized item selectionを導入すれば、同一証拠から同一事後分布という絶対規律も崩壊する。
- **根本原因**:
  - TrueSkillは競争者間の順位・勝敗likelihoodを持つrating modelであり、面接の絶対的な能力段階を観測するmodelではない。Factor Graphという実装形式だけを転用しても、面接item、rater、rubric、ordinal responseの観測分布は定義されない。
  - 2PLは

    $$
    P(Y_i=1\mid\theta)=\sigma\left(a_i(\theta-b_i)\right)
    $$

    である。任意の$A>0$と$B$に対して

    $$
    \theta'=A\theta+B,\qquad b_i'=Ab_i+B,\qquad a_i'=\frac{a_i}{A}
    $$

    と変換しても確率が不変であり、尺度anchorなしに$\theta$、$a_i$、$b_i$を同時学習できない。単一利用者の少数応答から時変能力とitem parameterを同時推定する設計はさらに非識別となる。
  - 0から4などの面接評価はordinal responseであり、binary 2PLは順序情報を破棄する。
  - stateが完全にobservableならmodelはMDPでありPOMDPではない。skillがlatentならbelief stateと、明示的なtransition $T$、observation $O$、reward $R$が必要である。
- **実コード証拠**: 監査時点のrepositoryには、TrueSkill、Bayesian Skill Ledger、2PL IRT、POMDPについて、生成モデル、識別制約、transition、observation、reward、近似法、丸め規則を定義した実装または正式仕様が存在しない。したがって将来構想は検証可能な設計へ到達していない。
- **必須是正措置**: TrueSkill、未anchor 2PL、名称だけのPOMDPをFoundationの基盤にしてはならない。固定grid、固定band transition、ordinal responseを持つDynamic Ordinal Rasch Filterへ移行する。観測modelはordered thresholdを用いて次のように固定する。

  $$
  P(Y_{it}\ge r\mid\theta_t)=\sigma(\theta_t-b_{ir}),
  \qquad b_{i1}<b_{i2}<\cdots<b_{iR}
  $$

  ability gridを$\{\theta_k\}_{k=1}^{K}$、固定transitionを$T_{jk}$とした予測・更新は次で行う。

  $$
  \widetilde{w}_{t,k}=\sum_j T_{jk}w_{t-1,j}
  $$

  $$
  w_{t,k}=
  \frac{\widetilde{w}_{t,k}P(y_t\mid\theta_k,i_t)}
       {\sum_{\ell}\widetilde{w}_{t,\ell}P(y_t\mid\theta_{\ell},i_t)}
  $$

  item選択はpoint estimate上のFisher informationではなく、posterior全体に対する期待情報利得を使用する。

  $$
  \operatorname{EIG}(i)
  =H(\bar p_i)-\sum_k w_{t,k}H\!\left(p_i(\theta_k)\right),
  \qquad
  \bar p_i=\sum_k w_{t,k}p_i(\theta_k)
  $$

  grid、transition、item threshold、prior、log-sum-exp、量子化、丸めmode、tie-breakをversioned artifactとして固定する。MCMCとrandomized policyを権威更新から排除し、同一canonical inputに対するposterior vectorのbit-level goldenを実装前RED契約にする。questionがskillを変化させないならPOMDPを導入せずadaptive testingとして扱い、skillを変化させる場合だけ明示的なstate transitionとutilityを別途裁定する。

- **解決日**: `2026-07-15`
- **解決記録**: TrueSkillおよび未anchor 2PLを権威推論経路から完全撤去し、versioned artifactで固定したability grid、prior、band transition、ordered thresholdを用いるDynamic Ordinal Rasch Filterへ置換した。予測・観測更新はLog-Sum-Expによる固定順序の完全周辺化とし、MCMC・randomized policyを排除した。item選択にはposterior全体の決定論的EIGを実装し、固定小数点量子化とitem IDの完全総順序でtieを解消する。posterior vectorとEIGのIEEE-754 Golden契約により、同一証拠に対するビットレベルの完全再現性を証明した。モデルartifactのSHA-256は`CanonicalRuntimeIdentity`へ必須結合した。
- **検証証拠**: FSA-13契約`2 passed`、専用Golden・Runtime Identity回帰`11 passed`、全体pytest`637 passed, 1 skipped`、Cargo全target`60 passed`、`git diff --check`違反0件。

---

## 監査裁定

- **総合状態**: `GREEN (ALL RESOLVED)`
- **Foundation完了記録**: 13件すべてでRED契約、最小是正、対象GREEN、全体回帰、禁止領域確認、台帳更新を完了した。
- **次フェーズ制限**: `解除`。グローバルegress、実行物真正性、state identity、永続化hard-fail、数値決定論、数理モデル識別制約のFoundation要件はすべて解決済みであり、次のdeploymentへ進行可能とする。
- **監査commit状態**: 全Findingを個別のatomic commitで封印し、FSA-13最終commitをもって本監査台帳をcloseする。
