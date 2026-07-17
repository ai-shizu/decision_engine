# PKB iOS / App Store モバイル化・全体ロードマップ

- Status: Master roadmap（正式版 rev.3）— Commander裁定によるStandalone Pivot（同期機構全パージ・絶対孤立・Pocket Brain LLM選定）
- Date: 2026-07-17（rev.1 Codex起草 → rev.2 Fable監査統合 → rev.3 Standalone Pivot）
- Target: Tauri v2 / iPhone-optimized UI＋iPadOS compatibility mode / App Store
- Source of truth: <code>.cursorrules</code>, <code>docs/AI_SKILLS.md</code>, <code>docs/SPEC_UI_MONOCHROME_CYBERPUNK.md</code>

## 0. 経営判断用サマリー

この計画は「既存Web UIをiOSの箱に入れる」計画ではない。現行PKBはすでにTauri v2であり、真の移行対象は、デスクトップの子プロセス型エンジンをiOSで成立する同一プロセス型エンジンへ置き換えることである。

最終方針は次のとおりとする。

1. **Tauri v2は再移行しない。** 現行v2実装を公式移行チェックリストで監査し、バージョンを固定したうえで、iOS生成物・設定・capabilityを追加する。
2. **React/CSS/UI契約は維持する。** React Native、Tailwind、Zustand/Redux/Jotai等、UIライブラリ、WebViewからのネットワークは導入しない。既存のplain React、pure reducer、strict parser、dumb viewをそのままモバイルへ伸ばす。
3. **Pythonの業務ロジック所有権を維持する。** iOSではCPythonをアプリ内に埋め込み、Rustから固定C APIで呼ぶ。PyInstaller sidecar、stdio子プロセス、REPL、動的パッケージ導入は持ち込まない。
4. **LLMは同一プロセス化する。** Rustの薄い安全ラッパーから、固定コミットのllama.cpp C APIを呼び、Metal/Accelerateを使う。server、RPC、CLI、モデル自動ダウンロードは製品へ含めない。
5. **検索C++は数値カーネルのまま静的リンクする。** 現在の384次元AoSoA dot/top-k責務を変えず、daemon/executableだけをin-process C ABIへ変える。
6. **正本は暗号化SQLCipher DB＋署名付きappend-only journalとする。** journalは同期のためではなく、改竄・DB巻き戻しの検出とrecovery archiveの完全性証明のために持つ。ベクトル・tensor・index・cacheは派生物としてjournal headへ束縛し、端末内で再生成する。
7. **同期は存在しない。iPhone単体で完結する絶対孤立要塞である。** アプリは自身の全生涯で1つのsocketも作らず、peer discovery、listen、connect、DNS、mDNS、share sheet経由のデータ移送を含む全同期機構を持たない。唯一の外部接点は、ユーザー明示操作のFiles document pickerによる暗号化recovery archive（<code>.pkbvault</code>）のexport/importと、user-file・署名済みmodelの明示importだけである。
8. **OS/iCloud Backupも復旧経路にしない。** ThisDeviceOnly keyとの不整合を避けるため全PKB管理fileをbackup除外し、復旧はユーザー操作の署名・暗号化<code>.pkbvault</code>だけに固定する。
9. **LAN Sync・USB-mux有線・QR受領証・<code>.pkbdelta</code>はCommander裁定（Standalone Pivot）により全パージした。** cameraを含む関連permission・feature・codeを一切残さない。復活には新directive、新threat model、別binary/versionを必須とする（D3パージ記録）。Mac母艦との継続的データ共有は本製品の目標から除外する。
10. **美学はbundle artifactまで含む。** BIZ UDGothic Regular/BoldをOFL 1.1 noticeとともに物理同梱し、<code>UIAppFonts</code>へ固定する。app-owned UIKit/AppKit ceremonyは<code>#0a0a0a</code>/<code>#f2f2f2</code>、radius 0、限定inverse videoへ写像し、触覚は視覚的音量と同じsemantic grammarだけから発火する。
11. **LLMは「Pocket Brain」ドクトリンで選定する。** 母艦を失った今、on-device modelが唯一の頭脳である。旧7B/3.5GB下限はiOS roleについて正式に改定し、jetsam実測に基づくmemory予算、日本語感情分析・家計簿JSON抽出のgolden rubric、GBNF grammar拘束decodingを通過した極小・高知性GGUFだけをG0-Cで採用する。

3名の専任体制とsecurity/App Store法務reviewを前提に、App Store 1.0の<code>Standalone</code>単一profileまで**40〜62週**（25〜35%のcontingency込み）を計画帯とする。同期workstreamの全パージにより旧計画から約10〜14週短縮された。1名体制では85〜120週以上を見込む。TestFlight判定は2027年第2〜第3四半期、App Store提出判定は2027年第3〜第4四半期を目安とするが、M0 Pocket Brain benchmarkとM2/M3後に必ず再見積りし、日付より各exit gateを優先する。

## 1. 先に解決する4つの誤解・衝突

### 1.1 現行はすでにTauri v2

「v1→v2」は未実施タスクではなく、**完了監査＋残存デスクトップ依存の除去**として扱う。

| 根拠 | 現状 |
|---|---|
| <code>apps/desktop/package.json:17,22</code> | <code>@tauri-apps/api</code> / CLI はv2 |
| <code>apps/desktop/package-lock.json:1194-1205</code> | API 2.11.1 / CLI 2.11.4 |
| <code>apps/desktop/src-tauri/Cargo.lock:3498-3501</code> | Tauri 2.11.5 |
| <code>apps/desktop/src-tauri/tauri.conf.json:2</code> | config schema v2 |
| <code>apps/desktop/src-tauri/src/lib.rs:22</code> | <code>mobile_entry_point</code> 済み |
| <code>apps/desktop/src-tauri/Cargo.toml:9-11</code> | mobile用crate type済み |

公式の[v1→v2移行ガイド](https://v2.tauri.app/start/migrate/from-tauri-1/)との照合は行うが、<code>tauri migrate</code>を無批判に再実行しない。現行のstrict command、capability、CSPを壊さないことを優先する。

### 1.2 iOSでは現行の三段子プロセスが成立しない

現行の主要経路は次である。

~~~text
React
  -> Tauri/Rust EngineManager
  -> PyInstaller Python sidecar
  -> llama CLI / C++ search daemon
~~~

App Store配布のTauri iOS appは、desktopのようにbundle済み任意executableをchild processとしてspawnできず、Tauri shell pluginのiOS supportもURL openに限定される。したがってLLMだけMetal化しても、Python facade、NumPy、検索、永続化、prompt policyが残り、機能全体は動かない。

推奨経路は次である。

~~~text
React / CSS
  -> exact Tauri commands/events
  -> Rust MobileEngine adapter
  -> embedded CPython core
       -> built-in _pkb_native API
            -> Rust safe wrapper
                 -> llama.cpp / Metal
                 -> existing C++ 384D kernel
       -> SQLCipher-backed Python repository
~~~

CPython 3.13以降はiOSのembedded modeを公式に文書化しており、インタプリタ、標準ライブラリ、アプリコードを自己完結したbundleへ含める方式を示している。[Using Python on iOS](https://docs.python.org/3.13/using/ios.html)

ただし現行releaseはPython 3.12.10を厳密固定している。このため、iOS用に勝手にPython版を変えるのではなく、3.13系への互換性・golden parity・NumPy iOS buildをM2のGo/No-Goにする。

### 1.3 暗号化正本ストアは新設であり、既存DBの流用ではない

現行正本（desktop）は単一SQLiteではない。<code>diary.md</code>、LINE履歴、calendar/finance/consultation JSON、probe/interview/knowledge、独自binary/mmap、各種processed artifactへ分散している。<code>sqlite3</code>はmacOS Calendar DBのread-only importにしか使われていない。iOSはこれを引き継がず、次の順序で自前の正本を新設する。

1. 正本データと派生データの分類。
2. Python側repository境界の導入。
3. SQLCipher正本ストアと厳密schemaの新設。
4. 端末上genesisとuser-fileの検証付き明示import。
5. append-only signed journalの生成（改竄検出・recovery archive完全性用）。

DB/WALファイルの丸ごとコピーはrecovery archiveの代替にならないため禁止する。

### 1.4 現行規律との衝突は「モデル下限」だけが残る

現行規律は本番TCP/IPをloopbackを含め禁止し、Pythonのnetwork importとRust raw socketも恒久テストで拒否している。Standalone Pivotによりアプリはsocket ownerを一切持たないため、**transport例外の審議自体が不要になった**。例外を管理するのではなく、socket/DNS/network symbolの不存在をsymbol/packet/permissionの三面scanで機械的に証明する（§12.4）。

文書driftはM0で解消する。<code>.cursorrules:19</code>には古いlocalhost llama HTTP許可が残る一方、より新しい<code>docs/AI_SKILLS.md:52-57</code>はloopbackを含むTCP/IPを禁止している。本計画は後者と現行実装を正とし、iOSはin-process C API直結のためHTTP経路自体が存在しない。

残る正式な規律変更判断は1点、model policyである。現行は原則7B・3.5GB以上・小型モデル禁止だが、iOS/iPadOSアプリの非圧縮build上限4GBと最低対応端末のjetsam実測の下で、この下限は物理的に成立しない。G0-C（Pocket Brain）でiOS専用roleの下限改定を正式に裁定する。[Apple build size limits](https://developer.apple.com/help/app-store-connect/reference/app-uploads/maximum-build-file-sizes/)

## 2. G0: 実装開始前の承認ゲート

以下が署名・承認されるまで、該当実装へ入らない。

### G0-A: モバイル実行方式

**推奨承認案:** Pythonがorchestration/data/prompt/business ruleを所有し続け、iOSではembedded CPythonとして同一プロセス実行する。RustはTauri境界、artifact認証、native lifecycle、LLM/search FFIを所有する。C++は数値カーネルだけを所有する。

これは現行production IPC/sidecar/artifact規律のplatform-scoped改定である。承認時に<code>.cursorrules</code>、<code>docs/AI_SKILLS.md</code>、constitutional testsへ次を同時反映する。

- desktopのstdio sidecar process、private prompt channel、search daemon→1-shot→NumPy fallback、artifact inventoryは維持する。desktop（Windows/macOS）へvault/DeviceSignerを移植しない——Workstream CはiOS専用である。
- iOSだけembedded CPython、in-process native LLM transport、static search→NumPy fallback、mobile artifact inventoryを許可する。
- Python <code>llm_backend</code>/<code>llm_transport</code>がprompt/generation policyのownerであり続け、Rust FFIへpolicyを移さない。
- <code>engine_stdio.py</code>をそのままembedded importせず、transport-neutral pure dispatchを抽出する。desktop stdout/JSON/event bytesはgoldenで固定する。
- desktop/mobileのcommand/response/event意味論を同一cross-platform fixturesで照合する。

不採用とする案:

- React Nativeによる再実装。
- Rustへの業務ロジックの無断全面移植。
- iOSでのPython/llama/search子プロセス。
- WebView内Python/WASMへの移植。
- 機能ごとの別contractや緩いJSON passthrough。

**Kill criterion:** 物理端末でembedded CPython＋NumPy＋代表facade operationを再現可能buildにできず、App Store向けarchive監査にも通せない場合はM2で停止する。その時点で初めて「Rustへ業務ロジックを移す規律変更」か「iOS機能範囲の変更」を再承認する。黙ってfallbackしない。

Standalone構成にはapp-owned network stackが一切ないため、旧LAN/有線案が抱えた「embedded CPythonとnetwork能力が同一process」というhard conflictは**構造ごと消滅した**。loopback listenerも存在しない。Python audit hookやmodule削除をOS sandboxと呼ばない規律は維持する。

attack surface closureとして次を要求する。

- iOS Python bundleから<code>_socket</code>/<code>socket</code>/DNS/SSL client、<code>ctypes</code>/cffi、user-controlled arbitrary <code>dlopen</code>、subprocess/multiprocessing、writable import pathを除外。唯一のbinary-extension load経路はread-only app bundle内のmanifest列挙済み・署名済みAppleFrameworkLoader frameworkとする。
- built-in/dynamic extension、stdlib、frameworkをallowlist化し、unknown moduleをhard fail。
- isolated <code>PyConfig</code>、environment無視、read-only <code>sys.path</code>、bytecode write無効。
- <code>_pkb_native</code>はexact operationだけで、raw pointer/selector/URL/host/pathを受けない。
- Cargo/native linked symbol、Python import closure、archive framework、WebView command setをrelease testで固定。
- Python audit hook/offline runtimeはdefense in depthとして残すが、隔離境界の証明には使わない。

このclosureはdefense in depthであり、G0-Aのexit条件へ含める。隔離境界の証明には使わない。

### G0-B: 絶対孤立（Absolute Isolation）裁定

**同期は設計から存在しない。** Commander裁定（Standalone Pivot）により、LAN Sync、USB-mux/PeerTalk有線session、QR受領証、<code>.pkbdelta</code>差分パッケージ、share sheet経由のデータ移送を全パージした。iOS appは自身の全生涯で1つのsocketも作らない。release profileは<code>Standalone</code>のただ1つである。

release invariant:

- <code>egress-live</code>はcompile off。旧<code>lan-sync</code>/<code>wired-sync</code> featureはcode・capability・permission・artifactごと存在しない（0件をscanで証明する。feature flagとして「残して無効化」もしない）。
- app processに<code>socket/bind/listen/connect</code> callsiteが**allowlistなしで0**。Network.framework、Bonjour、MultipeerConnectivity、URLSession data/upload/download、WebSocketのowner/symbol/success pathが0。
- <code>NSLocalNetworkUsageDescription</code>、<code>NSBonjourServices</code>、<code>NSCameraUsageDescription</code>、network client/server entitlement、network XPCをbundleしない。cameraはQR受領証の廃止によりpermissionごと消滅した。
- 外部接点はユーザー明示操作のFiles document pickerだけとする: (a) <code>.pkbvault</code> recovery export/import、(b) user-file（LINE履歴等）の明示import、(c) 署名済み<code>.pkbmodel</code>の明示import。いずれもsecurity-scoped URL→bounded staging→strict検証の一方向流路であり、pickerの提示はnative ownerが行い、WebViewはbytes/path/provider resultを受けない。
- 機内モード（全無線off）で全機能が恒久的に完全動作する。networkは「使わない」のではなく「使えない」ことを、native symbol scan・全interface packet capture・permission/entitlement検査の三面で証明する。
- desktop contract parityのための既存egress系Tauri commandはexact schemaの**inert fixed-refusal adapter**として残す。<code>get_runtime_capabilities.egress_live=false</code>を返し、UI actionを提示せず、呼ばれても固定<code>EGRESS_LIVE_NOT_READY</code>だけを返す。host/path/payloadを解釈せず、「commandが存在する」ことと「egress能力が存在する」ことを混同しない。

パージ済み設計の扱いはD3（パージ記録）に従う。復活には新directive、新threat model、該当G0の再開、現行規律原文の明示改定、別binary/versionのApp Reviewを必須とし、いかなるcode・permission・feature・Info.plist keyも「将来のため」に残さない。

### G0-C: Pocket Brain — iOS LLM選定ドクトリン

母艦を失った今、on-device modelがPKBの唯一の頭脳である。旧policy（原則7B・3.5GB以上）はiOSの物理と両立しないため、iOS専用roleについて正式に改定する。silent fallbackや「modelなしbuild」への逃げは引き続き禁止する。

#### G0-C.1 memory物理学とeligibility裁定

iOSはswapを持たず、per-processの**jetsam上限**（機種RAM依存・概ね総RAMの5〜6割）を超えたappを即killする。よって「archiveに入るか」ではなく「**dirty footprintが最低対応端末のjetsam上限に収まるか**」が生死を分ける。

- model weightsはapp bundle内GGUFをmmapし、llama.cppのMetal no-copy buffer経由で**clean file-backed pages**として常駐させる。clean pagesはOSがevict可能でjetsam計算上のdirty footprintに含まれない。dirty予算を消費するのはKV cache、activation/compute buffer、CPython/NumPy heap、WebViewである。
- <code>os_proc_available_memory</code>を起動時・model load前に読み、実測budgetでload可否を決める。機種名からの推測RAM表に依存しない。
- 設計目標: **dirty footprint合計 ≤ 最低対応端末のper-app上限の60%**（memory warning・OS変動への余裕）。M4で<code>phys_footprint</code>実測をacceptanceにする。

M0で次のeligibility裁定を行う（Commander署名必須）:

| Option | UIRequiredDeviceCapabilities | 最低RAM | model予算（Q4量子化file実寸目安） | 帰結 |
|---|---|---|---|---|
| **A（Fable推奨）** | <code>iphone-performance-gaming-tier</code>（A17 Pro以降） | 8GB | 2.0〜2.5GB → **3〜4B級** | 対象機種は狭いが、要塞の頭脳に足る知性。budgetに余裕 |
| B | 汎用（iOS 17 floor＝iPhone XS世代を含む） | 3〜4GB | ≤1.0GB → **1〜1.5B級** | 市場は広いが、感情分析・抽出の品質天井が低い |

App Storeは任意のRAM帯を直接install eligibilityにできないため、RAM下限の保証は文書化されたcapability keyだけで行う。非公開機種allowlistやeligible端末上のsilent LLM-unavailableは引き続き禁止する。[UIRequiredDeviceCapabilities](https://developer.apple.com/documentation/bundleresources/information-property-list/uirequireddevicecapabilities)

#### G0-C.2 任務適合ゲート（日本語感情分析・家計簿JSON抽出）

candidateはparameter数ではなく**golden rubric**で選ぶ。M0で次の2系statementを凍結する。

1. **感情分析rubric:** 日記・LINE文体の日本語corpusに対し、閉集合label（感情種別・極性・強度）＋根拠spanを出力する。desktop 7B referenceの出力を擬似正解とした一致率と、人手検収サンプルの双方に閾値を設定する。
2. **家計簿JSON抽出rubric:** 自然文/レシート様テキスト→exact schema（date/amount/currency/category/payee/memo）。**llama.cppのGBNF grammar拘束decodingを必須**とし、malformed JSONのretry loopではなく文法レベルでschema妥当性を保証する。数値・日付の正規化誤り（全角/半角、カンマ区切り金額、和暦/西暦）は意味誤りとして採点する。

共通gate: prompt injection耐性（PKB既存の対抗corpus）、gap safety（不明時に捏造せず固定unknownを返す）、streaming応答のtoken毎policy維持。

#### G0-C.3 candidate matrix（M0 benchmark対象・採用時にexact commit/hash固定）

| 級 | candidate | license | 位置づけ |
|---|---|---|---|
| 1〜2B | TinySwallow-1.5B-Instruct / Qwen3-1.7B / sarashina2.2-1B | Apache-2.0 / Apache-2.0 / MIT | Option B主力、Option Aの低負荷（Low Power Mode/thermal）profile |
| 3〜4B | Qwen3-4B / sarashina2.2-3B / Gemma 3 4B（QAT） / llm-jp-3-3.7b | Apache-2.0 / MIT / Gemma license要審査 / Apache-2.0 | **Option A主力候補** |

- 量子化はQ4_K_M baseline、予算が許せばQ5_K_M。日本語calibration corpusによるimportance matrix量子化を比較し、rubric劣化が閾値内のものだけ許可する。
- redistribution licenseとattributionはG0-Dで承認し、署名済みbundle artifactとして<code>model_params.json</code>のiOS roleへ固定する。
- 本表は候補であり確定ではない。M0 benchmarkの実測（rubric一致率、tokens/s、first-token latency、dirty footprint、thermal 20分連続）だけを採用根拠とする。

#### G0-C.4 決定と停止

1. **採用:** eligibility Option、primary model、量子化、context上限（初期2048、上限はKV cache実測で裁定）を単一ADRで固定する。
2. **No-Go:** 全candidateがrubric閾値を満たさない場合、「local LLM統合済み1.0」を停止しscopeを再承認する。未検証modelやrubric未達modelを黙って出荷しない。
3. <code>PKB_ALLOW_SMALL_LLM=1</code>等のenvironment迂回を製品に持ち込まない。改定はこのG0-C ADR自体の改訂だけで行う。

App Store 1.0にはG0-Cを通過したproduction GGUFを**ちょうど1件**bundleし、mobile artifact inventory/constitutional testでrole/hash/signature/license/sizeを固定する。import-only/modelなしbuildは1.0候補にしない。optional manual model importを残す場合も、bundle model inventoryの代替にはしない。

### G0-D: 暗号・ストア・App Store

- SQLCipher edition/license、SBOM、再現buildを承認する。
- bundled GGUFのredistribution licenseと、CPython/NumPy/llama.cpp/embedding runtime/model/SQLCipherのlicense notice・attributionをartifact単位で承認する。
- DB key、device identity、journal actor key、recovery archive KDF/recovery key、contact identity/state-chainの関係をADR化する。
- <code>.pkbvault</code>のKDF/AEAD/recovery-key custodyをG0-C採用modelのlicenseとともにartifact単位で承認する。
- 輸出コンプライアンス判断のownerを決める。
- <code>ITSAppUsesNonExemptEncryption=false</code>を推測で設定しない。
- PKB-managed iCloud sync/CloudKitだけでなく、**canonical DBを含むPKB管理fileをOS/iCloud Backupから除外する**。ThisDeviceOnlyのDB keyだけが移行せず暗号化DBだけが復元される破綻を許さない。復旧経路はM6の暗号化manual archiveだけとし、Appleの[backup storage guidance](https://developer.apple.com/documentation/foundation/optimizing-your-app-s-data-for-icloud-backup)、[ThisDeviceOnly Keychain semantics](https://developer.apple.com/documentation/security/ksecattraccessiblewhenunlockedthisdeviceonly)、端末紛失時のdata-loss告知、App Review説明を承認する。

### G0-E: UI freeze解除

<code>docs/SPEC_UI_MONOCHROME_CYBERPUNK.md:248-255,302-312</code>はUI.D後の対象fileを1-byte freezeし、ACKまでHARD STOPとしている。mobile対応は<code>index.html</code>、React/CSS、Tauri設定へ触れるため、実装前に次を満たす。

- UI.D完了ACKが存在することを確認する。存在しなければACKを待つ。
- mobile/platform-security delta specを別途承認し、変更許可file、safe-area/tab/titlebar copy、既存token再利用、iOS UIKit・macOS AppKitのWebView外security ceremony、visual acceptanceを固定する。
- **F-A1 typography delta:** BIZ UDGothic Regular/Boldのunmodified TTFを公式sourceのexact tag/commit＋SHA-256で固定し、app bundleへ物理同梱する。iOS <code>UIAppFonts</code>へ拡張子を含むbundle-relative filenameを列挙し、OFL 1.1全文、copyright、source/version/hashをapp内legal viewと配布noticeへ含める。runtime download、CDN、On-Demand Resources、system-installed font依存を禁止する。[Apple UIAppFonts](https://developer.apple.com/documentation/BundleResources/Information-Property-List/UIAppFonts) / [BIZ UDGothic source and OFL](https://github.com/googlefonts/morisawa-biz-ud-gothic)
- 現行UI SPECの<code>@font-face</code>禁止は維持する。CSSは登録済みPostScript/family nameだけを既存font stackから参照する。WKWebViewが<code>UIAppFonts</code>登録fontを実機で解決できない場合、local <code>@font-face</code>を黙って追加せずG0-Eへ差し戻し、Fableの明示deltaを待つ。
- **F-A2 Native Ceremony Design Annex:** app-owned UIKit/AppKit routeをCSS tokenのnative写像で固定し、汎用system-default card/button/alertの寄せ集めを禁止する。OS所有のFace ID/Touch ID、permission、Files pickerは偽装・再描画・私製biometric prompt化せずtrusted system exceptionとし、その直前・直後を承認済みnative ceremony frameで包む。
- **F-A4 Haptic Grammar:** raw vibration/notification種別の許可表ではなく、視覚的音量とsemantic state transitionに対応するE5の文法を承認する。OS標準のsuccess/warning/error notification pattern、multi-pulse、連続振動を製品へ含めない。
- **F-P1 effect boundary:** <code>platformEffects</code>はstateless runnerに限定し、pure reducerが返したclosed <code>Effect</code>だけを入力、strict parse済み<code>DomainEvent</code>だけを出力する。React state/store/queue/cache/retry/timer/subscription/API直接参照を持たせない。
- **F-P3 action語彙の事前凍結:** 新規UI action/<code>Effect</code>/<code>Event</code> union（viewport、content size category、haptic grammar、delta transfer状態等）をdelta specへ**実装前に列挙**し、dependency-free reducer harnessのテストを先にREDで書いてから実装する。列挙外のaction/Effect追加はfreeze違反として扱う（本計画がD章negative testで採るRED-first流儀のUI適用）。
- 現行仕様が許可するloop animation以外を追加しない。syncは原則static status/determinate progressで示す。
- M0でF-P2のFreeze Transition ADRを承認し、各milestoneのentry hash、exact file/symbol allowlist、解除期間、再freeze test、exit ACKを先に決める。

このロードマップ文書の作成はfreeze解除を意味しない。

### G0-E.1: Freeze Transition ADR（F-P2）

M0成果物として<code>ADR-FREEZE-TRANSITION-IOS-V1</code>を作る。各entry時点で対象treeのSHA-256 manifestを採取し、承認者、理由、許可file/symbol、禁止領域、必要test、exit hashを署名する。許可外diff、既存testの削除・skip・期待値弱体化、複数milestoneを跨ぐ開放状態は即停止である。再freeze ACKがないmilestoneから次のmilestoneへ進まない。

| Milestone | 一時解除する範囲 | 常に凍結する範囲 | 再freeze / exit evidence |
|---|---|---|---|
| M0 | roadmap、ADR、threat model、RED guard/test specificationのみ。product codeは解除しない | 全production code、既存test | signed baseline hash、G0-A〜E/B' decision、no-product-diff ACK |
| M1 | Tauri platform config、generated-project template、native shell、font asset/Info.plist/legal notice、fake backend adapter | Python business/prompt/data、既存pure reducers、既存UI copy | clean generation diff、font hash/PostScript/WKWebView proof、desktop regression、M1 hash ACK |
| M2 | transport-neutral engine seam、iOS bootstrap/import closure、CPython/NumPy packaging、new exact parserだけ | domain/business/prompt semantics、existing reducer/view、store schema | golden parity、archive/import/codesign scan、exact changed-symbol manifest、M2 ACK |
| M3 | Python repository seam、SQLCipher schema/migration、DeviceSigner reservation/anchorだけ | prompt/LLM policy、UI、conflict semanticsの未承認変更 | migration/signer crash matrix、schema/protocol version freeze、M3 ACK |
| M4 | signed model config、native LLM/embed/search adapterとFFIだけ | prompt owner、business rule、UI/reducer、canonical schema | quality/parity/memory/thermal evidence、native ABI/hash freeze、M4 ACK |
| M5 | G0-E allowlist内の<code>App.css</code>/<code>index.html</code>/表層component、新規pure mobile reducer/effect union/parser、single PlatformPort、native ceremony/font/haptic adapter | 既存<code>researchUiReducer</code>/<code>policyStore</code>/<code>parseKnowledgePolicy</code>意味論、Python/Rust business core、ユーザー可視copyの無断変更 | boundary/import/grep gate、visual/a11y matrix、native screenshot diff、G0-E再freeze ACK |
| M6 | <code>.pkbvault</code> recovery protocolと、そのexact native ceremony/pure effect surfaceだけ | 通常record/prompt/LLM semantics、既存UI surface、DB/WAL直接経路 | restore/tamper/rollback/crash matrix、package version freeze、M6 ACK |
| M7 | 原則解除なし。release blockerは1件ごとのmicro-ADR＋最小allowlist | それ以外すべて | empty-gen rebuild、full regression、dependency/protocol final freeze |
| M8 | App Store metadata、Review Notes、privacy/legal copy、承認済みlocal policy viewだけ | runtime/business/protocol/UI behavior | signed release candidate hash、review dossier ACK |
| M9 | signed migration/security response ADRで許可した最小範囲だけ | release branchの全artifact | forward/backward migration test、新version再freeze |

M5以前に既存componentから直接Tauri/window/listener APIを移す必要が見つかっても、便宜上先行編集しない。M5のexact thawへ列挙するか、別のsigned micro-ADRを起こす。

## 3. 絶対に維持する設計規律

### 3.1 UI

- plain React + CSSのみ。
- UI stateはReact非依存のpure reducerで管理し、dependency-free harnessでテストする。
- componentは表示と入力に限定する。
- low-level Tauri invoke/listen/window/plugin/DOM platform APIは単一<code>PlatformPort</code>へ集約し、<code>engine.ts</code>と<code>platformEffects</code>はtyped domain facadeとしてそこへ委譲する。全外部値を<code>unknown</code>からstrict runtime parserへ通す。
- <code>platformEffects</code>は**stateless**である。module/global mutable state、React hook/context、store、cache、queue、retry、timer、polling、debounce/throttle、subscription handle、storageを所有しない。pure reducerが返したdata-only closed discriminated union <code>PlatformEffect</code>だけを入力とし、関数、DOM object、Tauri handle、callback、任意host/pathを受けない。
- effect runnerはraw APIを直接触らず、注入済み<code>PlatformPort</code>へ一度委譲する。port/hostがobserver handleの生成・disposeだけを担当し、raw resultを<code>unknown</code>としてparseしたclosed <code>PlatformEvent</code>だけをrunnerが返す。raw exception、DOM value、plugin result、path、payloadをReact stateへ返さない。domain state、policy、dedupe、retryはport/hostへ置かずpure reducerが所有する。
- exactly-once effect消費はreducerが発行するeffect IDとstate transitionで決める。re-render、duplicate event、cancel/retryでhaptic、picker提示、native ceremonyを再発火させない。
- component、<code>platformEffects</code>、任意domain moduleから<code>visualViewport</code>、<code>matchMedia</code>、CSS <code>style.setProperty</code>、Tauri <code>invoke/listen/getCurrentWindow</code>、plugin APIへ直接到達することをgrep＋import-boundary testで拒否し、exact allowlistの<code>PlatformPort</code>実装だけに許す。
- modal、<code>alert</code>、<code>confirm</code>を使わない。
- inference、index rebuild、archive export/importはambientに表示し、textarea、scroll、他機能を封鎖しない。
- monochrome、角丸0、1px border、local monospace、inverse videoの限定使用を維持する。
- iOSではBIZ UDGothic Regular/Boldをsigned local artifactとして同梱し、WebViewとapp-owned native ceremonyの第一fontにする。<code>BIZ UDPGothic</code>との取り違え、runtime font取得、未承認subsetting/rename/format変換を拒否する。
- 色で状態を表現しない。hapticも視覚/ARIAの代替にしない。
- hapticはraw styleをcomponentから指定せず、E5のsemantic grammar tokenだけをEffectへ載せる。notification/vibrate/multi-pulseを使わない。
- 現行仕様が許可済みのactive-process animationだけを再利用し、新しい無限loopを増やさない。<code>prefers-reduced-motion</code>を守る。

### 3.2 境界

- frontendから任意URL、host、path、SQL、model argsを渡さない。
- command/request/response/eventのexact key、type、size、CIDを検証する。
- raw exception、filesystem path、IP、key、record本文をerror event/logへ出さない。
- platform差はbackend adapterとnative pluginへ閉じ込める。
- desktopの既存stdio boundaryは維持し、mobileで同じlogical contractを再現する。

### 3.3 オフライン性

- PKB自身のCloudKit/iCloud Drive sync、Firebase、外部DB、push、STUN/TURN、telemetry、crash upload、CDNを使わない。
- model/runtime/knowledgeを自動downloadしない。
- production CSPの<code>connect-src</code>を開けない。
- App Store updateはOS/App Storeの責務とし、アプリ自身にupdaterを持たせない。
- canonical DB、model、derived、cacheを含むPKB管理fileはOS/iCloud Backupから除外する。復旧はM6の署名・暗号化<code>.pkbvault</code>だけを使う。
- Files document pickerでユーザーが自らiCloud Drive providerを選ぶことは、PKB-managed iCloud syncとは区別する。system providerを不当に隠さない。export済みarchiveの保管はユーザーの責任領域であり、暗号化がその前提である。
- app-owned socket ownerはprofileを問わず存在しない。loopbackを含むlistener/connectを恒久テストで拒否する。share sheetは使わない（データ移送surfaceを持たない）。

## 4. 目標アーキテクチャ

~~~mermaid
flowchart TB
    UI["React + CSS\npure reducer / dumb view"] --> IPC["Exact Tauri commands & events\nstrict TS/Rust parsers"]
    IPC --> ROUTER{"Compile-time backend"}
    ROUTER -->|desktop| SIDE["Existing EngineManager\nPython sidecar over stdio"]
    ROUTER -->|iOS| MOBILE["MobileEngine\nin-process serialized worker"]
    MOBILE --> PY["Embedded CPython core\nfacade / prompt / reducers / repository"]
    PY --> NATIVE["_pkb_native fixed API"]
    NATIVE --> LLAMA["Rust llama wrapper\nllama.cpp C API + Metal"]
    NATIVE --> SEARCH["Existing C++ numerical kernel\n384D AoSoA dot/top-k"]
    PY --> VAULT["SQLCipher vault.sqlite\ncanonical tables + signed journal"]
    MOBILE --> IOS["Narrow native services\nKeychain / lifecycle / Files picker / EventKit / haptics"]
    PY --> RECOV["Signed encrypted .pkbvault\nrecovery archive"]
    RECOV <-. "user-explicit Files picker only\nno socket / no share sheet / no camera" .-> FILES["OS document picker\nuser-chosen destination"]
~~~

### 4.1 Engine backend seam

既存の<code>EngineConnection</code>/<code>EngineConnector</code>とclosed Tauri command群を再利用し、次のcompile-time分岐を置く。

~~~text
EngineBackend
  DesktopProcessBackend  [cfg(not(mobile))]
  IosEmbeddedBackend      [cfg(target_os = "ios")]
~~~

両backendは同じものを返す。

- health response
- operation response envelope
- correlation ID
- status/token/final event
- cancellation outcome
- fixed safe error code

frontendはbackend種別を知る必要がない。platform固有表示が必要な場合は、狭い<code>get_runtime_capabilities</code> responseをexact parserで受け、pure reducerへeventとして渡す。UA sniffingや<code>__TAURI_INTERNALS__</code>だけの判定を使わない。

### 4.2 Embedded Python

アプリbundleへ次を固定して含める。

- pinned CPython framework
- 必要最小の標準ライブラリ
- PKB core Python code
- iOS向けにbuild・署名したNumPy extension
- 固定built-in module <code>_pkb_native</code>

含めないもの:

- REPL、console、pip、venv
- runtime code download
- user Python execution
- PyInstaller bootloader
- subprocess/multiprocessing
- desktop TUI
- socket/DNS modulesの利用経路
- Documents/Application Support等のwritable locationからのmodule import
- user inputをcodeとして扱う<code>eval</code>/<code>exec</code>経路

embedded configurationではUTF-8 modeを固定し、bytecode書込みを無効化し、Python home/pathをread-only app bundleへ限定する。CPython公式iOS文書が示すApp Store scanner対策を確認し、不要stdlib moduleをallowlist方式で除外する。bundle内Python source/bytecode/frameworkも署名付きartifact inventoryへ含める。

#### 4.2.1 iOS binary extension packaging

CPython/NumPyのbinary extensionは「static linkすればよい」「<code>dlopen</code>を全消去すればよい」のどちらでもない。CPython 3.13の[Binary extension modules / Adding Python to an iOS project](https://docs.python.org/3.13/using/ios.html#binary-extension-modules)をcanonical packaging contractとする。

- dynamic <code>Python.xcframework</code>をEmbed & Signし、device <code>arm64-apple-ios</code>と承認済みsimulator sliceをartifact manifestへ固定する。
- target-specific stdlibとthird-party package treeをread-only bundleへcopyし、device buildからsimulator binary、simulator buildからdevice binaryを除外する。
- <code>lib-dynload</code>、NumPy、その他allowlisted packageの各<code>.so</code>を**1 module = 1 signed framework**へpost-processする。framework名はfull dotted import path、各frameworkは1 binary＋required Info.plistだけを持つ。
- 元<code>.so</code>位置をtext <code>.fwork</code> markerへ置換し、framework内に逆参照<code>.origin</code>を置く。両pathはapp bundle相対・canonical・manifest一致をmachine検証し、<code>..</code>、symlink、writable root、unknown frameworkを拒否する。
- binary importはCPythonのAppleFrameworkLoaderだけに限定する。Python/native APIから任意path/symbolを渡す<code>dlopen</code>、ctypes/cffi、runtime framework追加を公開しない。
- build phase自体を生成tree外のversioned sourceとしてA2.1のclean regenerationへ組み込み、全extension frameworkを個別codesignする。
- IPA scanでraw <code>.so</code> 0、manifest外<code>.fwork/.origin</code> 0、Python extension executableが<code>Frameworks</code>外に0、frameworkごとのbinary数=1、Info.plist/bundle ID/slice/codesign一致を要求する。

M2 exitはhealth importだけでなく、NumPy代表extensionのdevice/simulator import、<code>__file__</code>の<code>.fwork</code> location、<code>ModuleSpec.origin</code>のframework location、tampered marker/framework/signature拒否を通す。

Rustは1本のserialized engine worker上でinterpreterとGILを所有する。Tauri commandごとにbounded envelopeを渡し、Pythonのdispatcher/facadeを呼ぶ。Python callbackはtoken/statusをRust event sinkへ返す。解放・background・cancelの順序をstate machineで固定する。

1.0では<code>docs/AI_SKILLS.md</code>が固定する**desktop Python 3.12.10を変更しない**。iOSだけは公式embedded supportのためCPython 3.13.xをexact pinし、runtime versionは違ってもbusiness/prompt/data sourceとlogical contractを分岐させない。

1. 現行desktop 3.12.10でgolden corpusを凍結。
2. desktop release lane、PyInstaller、wheel/lock、artifact inventoryは3.12.10のまま維持。
3. diagnostic CIへ3.13 exact pinのshadow laneを追加し、iOS frameworkと同じsource/import closureを検査。
4. desktop 3.12.10とiOS 3.13.xで全pure/core/contract test、hash/canonical JSON/float/NumPy結果を比較し、差異0または明示承認済みplatform adapterだけを許す。
5. version compatibility shimへbusiness ruleを書かず、strict test付きのbootstrap/platform adapterへ閉じる。
6. 将来desktopを3.13へ上げる場合は別ADRとし、<code>AI_SKILLS.md</code>、exact lock/wheel、PyInstaller、artifact hash、全desktop acceptanceを同時更新する。このロードマップの黙示変更にしない。

現行import graphには<code>subprocess</code>、llama CLI、search daemon、macOS Calendar SQLite readerが含まれるため、M2ではこれらをprotocol＋platform adapterへ分離する。iOS import closureがdesktop process moduleを読み込まないことをstatic testで固定する。data root、model root、temporary rootはcurrent directoryや<code>HOME</code>から推測せず、RustがTauri path resolverで得たapp container内のvalidated bootstrap objectとしてPythonへ一度だけ渡す。

現行のidentity root secretもenvironment variableで注入しない。iOSではKeychain-backed native key providerを<code>_pkb_native</code>の閉じた操作としてPythonへ提供し、raw key bytesをWebView、UserDefaults、logへ返さない。

### 4.3 Native compute bridge

<code>_pkb_native</code>は汎用FFIを公開せず、次のような閉じた操作だけを持つ。

- verified model handleを開く/閉じる
- bounded promptからsessionを開始
- tokenを1つ進める
- token/batch境界でcancel
- 384D queryに対するbounded top-k
- native memory/thermal stateのsanitize済み取得

path、pointer、raw Metal option、thread countをfrontendから渡さない。model paramsは署名済みconfigが唯一のsource of truthである。

### 4.4 Native service boundary

Swift/Objective-C側はApple platform serviceだけを担当する。

- Keychain / Secure Enclave
- app lifecycle / protected data availability
- memory warning / thermal state / Low Power Mode
- Files document picker（recovery/user-file/model importとarchive export専用）
- EventKit
- haptics
- BIZ UDGothic registration / app-owned native ceremony styling
- app switcher privacy shield

business rule、prompt、conflict resolution、record classificationをnative UI codeへ書かない。Tauriの[mobile plugin bridge](https://v2.tauri.app/develop/plugins/develop-mobile/)を用い、permissionはoperation単位で狭くする。

## 5. Workstream A — Tauri v2 / iOS基盤

### A1. v2完了監査とversion pin

- packageの<code>^2</code>を検討し、release toolchainはAPI/CLI/Rust crateをexact pinする。
- v2 migration checklistで、core API、capability/permission、plugin、CSP、mobile entry pointを照合する。
- base configをplatform-neutralへ戻す。
- <code>targets:["nsis"]</code>を<code>tauri.windows.conf.json</code>へ移す。<code>externalBin</code>はWindows/macOS各overrideへ明示するか、review済み<code>--config</code> mergeで渡す。存在しないgeneric <code>tauri.desktop.conf.json</code>を前提にしない。
- <code>build.rs</code>のsidecar placeholder生成を<code>cfg(not(mobile))</code>へ限定する。
- release artifact verifierからmobileに存在しないsidecar/search executable requirementをplatform manifestへ分離する。

### A2. iOS project

macOS＋full Xcode＋CocoaPods＋Rust iOS targetsを固定したrelease環境で、公式手順に沿って<code>tauri ios init</code>を行う。[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) / [App Store distribution](https://v2.tauri.app/distribute/app-store/)

追加するもの:

- <code>tauri.ios.conf.json</code>
- <code>src-tauri/gen/apple</code>
- bundle ID / development team / build number
- <code>bundle.iOS.minimumSystemVersion: "17.0"</code>のexplicit pin
- iOS icons / launch assets
- iOS-specific Info.plist fragment
- pinned <code>BIZUDGothic-Regular.ttf</code>/<code>BIZUDGothic-Bold.ttf</code>、exact <code>UIAppFonts</code> array、OFL 1.1/copyright notice
- <code>PrivacyInfo.xcprivacy</code>
- iOS-only capability
- simulator/device build lanes

既存identifier <code>com.ai-shizu.pkb</code>はApp Store ConnectのApp IDと一致・所有可能か確認し、確定後は変更しない。

1.0のminimum OSはiOS/iPadOS 17.0へ固定する。これによりEventKitはiOS 17のfull-access APIと<code>NSCalendarsFullAccessUsageDescription</code>へ一本化する。minimumを将来17未満へ下げる変更は、availability-gated API、旧<code>NSCalendarsUsageDescription</code>、permission state、minimum/latest OS test matrixを同時に追加する別ADRなしに行わない。M1 exitはminimum/latest iOSのiPhoneと、minimum/latest iPadOS compatibility modeの両方でcold launch・permission denial・foreground復帰を通すこととする。[Tauri iOS configuration](https://v2.tauri.app/reference/config/#iosconfig) / [Calendar full-access usage description](https://developer.apple.com/documentation/bundleresources/information-property-list/nscalendarsfullaccessusagedescription)

1.0は**iPhone向けにUI最適化したiOS app**とする。generator/build settingの<code>TARGETED_DEVICE_FAMILY=1</code>をcanonical sourceへ固定し、<code>UIDeviceFamily</code>をInfo.plistへ手書きしない。archive実体が<code>UIDeviceFamily=[1]</code>で<code>2</code>を含まないことを検査する。[Apple UIDeviceFamily](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/iPhoneOSKeys.html#//apple_ref/doc/uid/TP40009251-SW11)

- <code>UIDeviceFamily=[1]</code>はnative iPad targetを外すだけで、iPadOS compatibility modeを外す設定ではない。Apple Developer Program License Agreement §3.3(E)に従い、compatibility modeでiPhoneと実質同等の全機能を提供し、device判定で妨げない。これは1.0の**mandatory supported surface**である。[Apple Developer Program License Agreement](https://developer.apple.com/jp/support/terms/apple-developer-program-license-agreement/) / [QA1780](https://developer.apple.com/library/archive/qa/qa1780/)
- native iPad layout/universal target、Mac Catalyst、visionOS targetは1.0へ含めない。
- App Store ConnectではiOS appのApple silicon Mac availabilityをoffにし、Apple Vision Pro availabilityもoffにする。既存desktop companionは別のmacOS artifactであり、iOS binaryをMacへ代用しない。[Apple silicon Mac availability](https://developer.apple.com/help/app-store-connect/manage-your-apps-availability/manage-availability-of-iphone-and-ipad-apps-on-macs-with-apple-silicon) / [Apple Vision Pro availability](https://developer.apple.com/help/app-store-connect/manage-your-apps-availability/manage-availability-of-iphone-and-ipad-apps-on-apple-vision-pro)
- seed buildのApp Store Connect supported-device listを保存し、最低eligible iPadと提出時の代表的な最新iPadを物理端末matrixへ固定する。Simulatorはlayout/OS組合せの補助に限り、Metal memory/thermal/jetsam、Data Protection、Keychain、Files/EventKit、font rendering、archive export/importは物理iPadで通す。
- 将来native iPad/Mac/Visionを有効化する場合はdevice family、native layout、memory/Metal、Files/EventKit/archive、privacy、screenshot、review matrixを追加する別milestoneとする。

開発・archive経路もM1で固定する。

- initialize: <code>npm run tauri ios init</code>
- simulator/device development: <code>npm run tauri ios dev</code>
- Xcode archive inspection: <code>npm run tauri ios build -- --open</code>
- release export: <code>npm run tauri ios build -- --export-method app-store-connect</code>

物理iPhoneのdev serverはproduction egressと混同しない。第一選択はXcode接続TUNとCLIの<code>--force-ip-prompt</code>を使い、plain dev serverをuntrusted LANへ公開しない。<code>TAURI_DEV_HOST</code>を使う場合はreview済み開発networkだけ、CLIを生存させた短命sessionだけに限定する。dev URL/dev CSP/ATS例外はrelease configへ一切mergeせず、archiveにdev URLやWebSocket endpointがないことを検査する。[Tauri mobile development](https://v2.tauri.app/develop/)

### A2.1 generated project / native framework規律

<code>src-tauri/gen/apple</code>は手編集で育てるsource of truthではなく、**再生成可能なdisposable artifact**として扱う。CPython、NumPy、llama.cpp、SQLCipher等の追加方法はM1でartifactごとに1つのdeterministic ownerへ固定し、同一artifactをSwift Package binary target、custom XcodeGen template、<code>bundle.iOS.frameworks</code>から重複追加しない。CPythonは<code>Python.xcframework</code>のEmbed & Signと4.2.1のversioned post-process build phaseを組にした公式方式をbaselineとする。

- version、URLではないlocal artifact path、checksum、slice、link mode、licenseを生成tree外のmanifest/configへ固定する。
- <code>PrivacyInfo.xcprivacy</code>、Info.plist fragment、entitlement、icon/assetも生成tree外にcanonical sourceを持たせ、generator/templateで配置する。
- BIZ UDGothic 2 font、OFL/copyright、asset manifestも生成tree外をcanonical sourceとし、Xcode resource membershipと<code>UIAppFonts</code>へ決定論的に配置する。Regular/Bold、PostScript name、file hash、license hashの欠落・余分・<code>BIZ UDPGothic</code>混入をclean regeneration後に拒否する。
- Tauriの<code>bundle.iOS.frameworks</code>変更は生成済みXcode projectへ自動反映されると仮定しない。M2はCPython/NumPy、M3はSQLCipher、M4はllama/embed/searchという**milestone-scoped artifact manifest**を確定し、追加・version変更のたびに公式手順でiOS projectをclean再生成する。
- 各再生成後に直前承認済みprojectとの差分をallowlist reviewし、bundle ID、minimum OS、signing、capability、privacy manifest、framework search/link settings、全device/simulator sliceをmachine checkする。
- M7で全native dependency closureを最終freezeし、空の生成treeからrelease projectを再構築する。Xcode UIでの一回限りの手修正をrelease手順へ残さない。
- milestone manifest、再生成diffまたはarchive実体が不一致なら、そのdependencyを導入するM2/M3/M4または最終M7のexitを閉じる。

### A3. platform isolation

現行の直接blockerを順に除く。

| 現行箇所 | 対処 |
|---|---|
| <code>tauri.conf.json:31-38</code> NSIS/externalBin | desktop overrideへ移動 |
| <code>build.rs:5-36</code> 全target sidecar | desktop cfgのみ |
| <code>paths.rs:135-164</code> iOS engine名なし | mobileではsidecar pathをcompileしない |
| <code>os_sandbox.rs:806-814</code> iOS unsupported | mobileではsidecar sandbox codeをcompileしない。app-level sandboxとin-process closureを別物として監査する |
| <code>lib.rs:63-66</code> setup時にsidecar start | backend factoryへ置換 |
| <code>paths.rs:43-55</code> iOSをgeneric Unix扱い | Tauri app data/Application Supportへ |
| base <code>beforeDevCommand</code>/<code>beforeBuildCommand</code>がPowerShell | iOS overrideまたは依存追加なしのcross-platform commandへ |
| <code>create:false</code>＋manual <code>WebviewWindowBuilder</code> | iOS compile/run、navigation/download hook、window creationをM1で実機gate |

iOSへmacOS用の<code>allow-unsigned-executable-memory</code>、<code>disable-library-validation</code>等をコピーしない。CPython experimental JITを無効化する。static codeは最終署名対象executableへlinkし、dynamic frameworkは個別に署名してbundleする。

### A4. capability / CSP

- desktopとiOS capabilityを別ファイルにする。
- iOS capabilityはdesktop window minimize/maximize/closeを持たない。
- haptics（selection/impactのみ）、Files（archive export/import・user-file/model import）、custom EventKitは各command/permissionを列挙する。network/LAN/Bonjour/camera/share commandはprofile自体に存在しない。
- capability auto-discoveryへ依存せず、configでIDを明示する。
- remote URL scopeは設定しない。
- production CSPはself/IPC-onlyを維持する。
- <code>NSAllowsArbitraryLoads</code>、<code>NSAllowsArbitraryLoadsInWebContent</code>を設定しない。
- privileged app commandを通常の<code>invoke_handler</code>登録だけで全windowへ開かない。<code>tauri_build::AppManifest::commands</code>へ登録し、Tauri ACLの対象にする。

Tauriのcapability、permission、scopeは公式の[capabilities](https://v2.tauri.app/security/capabilities/)、[permissions](https://v2.tauri.app/security/permissions/)、[scopes](https://v2.tauri.app/security/scope/)に照らして監査する。

Tauri ACLとApple OS authorizationは別層として管理する。

| 機能 | Tauri側 | Apple側 |
|---|---|---|
| <code>.pkbvault</code> export/import | exact native prepare/present command。bytes/pathはWebViewへ返さない | system document picker、security-scoped URL |
| Calendar | custom EventKit command permission | <code>NSCalendarsFullAccessUsageDescription</code>、EventKit authorization |
| Files import | exact picker/import command | system document picker、security-scoped URL |
| Haptics | semantic grammarを実装するselection/impactだけ。notification/vibrate permissionなし | 通常は別のuser permissionなし |
| Keychain | raw keyを返さないnative command | access group / data-protection class |
| Security ceremony | recovery export/import、consent等の開始だけ。署名結果やkeyを返さない | iOSの<code>NSFaceIDUsageDescription</code>、Keychain <code>SecAccessControlUserPresence</code> / LocalAuthentication |
| Bundled font | Tauri permissionなし | exact <code>UIAppFonts</code>＋signed bundle resource。system-wide font capabilityなし |

### A4.1 plugin compatibility matrix

| plugin/capability | iOS評価 | 方針 |
|---|---|---|
| shell/process | child-process代替にならない。shellのiOS supportはURL open中心 | LLM/Python/searchには不採用 |
| filesystem | iOS app領域に限定。FileTimestamp API使用時はprivacy manifest reasonが必要 | 大容量importはdocument picker＋native streamを優先 |
| haptics | iOS対応 | selection/impact permissionだけ。E5 grammar＋stateless effect runnerで採用し、notification/vibrateをbundle/capabilityから除外 |
| SQL | iOS対応でもgeneric JS SQLを開く | WebViewから使わず、Python-owned SQLCipher connectionを採用 |
| Stronghold | secret storeでありDB暗号化ではない | SQLCipher代替にしない |
| custom EventKit | 公式汎用pluginではなく自作mobile plugin | permission、strict response、PII-free errorを実装 |
| custom archive | document pickerを提示する狭いnative plugin | WebViewへpackage bytes/path/provider resultを返さず、bounded staged importを使う |

filesystem pluginでtimestamp APIを使う場合は生成tree外のcanonical privacy manifestへ該当required reason（現行公式案内のFileTimestamp <code>C617.1</code>を含む）を記載し、generatorが<code>src-tauri/gen/apple/PrivacyInfo.xcprivacy</code>へ配置した結果をarchive実体で検証する。[Tauri filesystem plugin](https://v2.tauri.app/plugin/file-system/)

### A5. CI / release toolchain

- PRごとにfrontend boundary、Rust test、Python test、desktop smokeを実行。
- iOS simulatorでcompile/install/cold launch。
- release候補は物理iPhoneで実行。
- desktop（Windows/macOS）製品は既存のrelease方針を独立に維持する。iOSとのdata-transfer fixtureは存在しない。共有するのはPython core/prompt/reducerのcross-platform logical fixtureだけである。
- archive実体でBIZ UDGothic 2 file、OFL/copyright、<code>UIAppFonts</code>、PostScript name、hash、target membershipを検査し、WKWebView/UIKit/AppKitの日本語/Latin混植glyph corpusをminimum/latest実機で比較する。
- pinned Xcode/Rust/Node/CPython/llama/SQLCipher/NumPyとSBOMを記録。
- App Store archiveを二回buildし、許容差分を監査。
- 2026年7月時点では、2026年4月以降のiOS/iPadOS uploadにiOS/iPadOS 26 SDK以上が必要である。これはminimum deployment targetとは別のbuild SDK要件である。compatible Xcodeをpinし、提出時にもApple公式ページで要件を再確認する。[Submitting apps](https://developer.apple.com/jp/app-store/submitting/)

## 6. Workstream B — ローカルLLM / 384D embedding / Metal / 検索

### B1. llama.cppのin-process化

固定コミットからiOS arm64用static libraryまたは管理下XCFrameworkをbuildする。llama.cppは[iPhone sampleとXCFramework build例](https://github.com/ggml-org/llama.cpp/blob/master/examples/llama.swiftui/README.md)を提供しているが、upstream生成物をそのまま取得せず、自前CIで再現する。

製品build方針:

- C ABIだけに依存し、C++ ABIをRustへ漏らさない。
- Metal/Accelerateを有効化。
- server、RPC、CLI、OpenMP、network downloaderを無効化。
- Metal libraryを署名済みbundleへ固定。
- source commit、compiler、flags、slice、SHA-256をartifact manifestへ記録。
- simulatorはCPU smoke、物理端末はMetal acceptance。

Rust wrapperはunsafeを1moduleへ閉じ、model/context/sessionの単一所有、cancel flag、token boundary、最大入力/出力をsafe APIで保証する。

### B2. Python orchestrationとの接続

既存<code>LlamaStdioBackend</code>のprompt redaction、config、artifact検証、streaming semanticsを保ちつつ、iOS adapterだけをnative iteratorへ差し替える。

~~~text
Python prompt/orchestration
  -> _pkb_native.begin()
  -> _pkb_native.next_token()
  -> Python status/token policy
  -> exact pkb-engine-event
~~~

prompt bytes、token、回答本文をOS logへ送らない。Rustとllama間にsocket、localhost server、temporary prompt fileを置かない。

model weightsのresident reuseは許すが、各facade operationはfresh <code>llama_context</code>を原則とし、KV cache、prompt history、session stateをrequest/mode間で共有しない。upstream APIによる完全resetを代替にする場合は、全KV/session buffer消去をpinned versionとgoldenで証明する。cancel、error、background後もcontextを破棄する。

### B3. 384D embedding provider

derived vectorを各端末で再生成する以上、query/document embeddingの同一性はLLMと独立したP0要件である。現行<code>pipeline.build_embedder()</code>は、ローカルSentenceTransformerと<code>hashed-ngram-384</code> fallbackの二経路を持つ。iOSでPyTorch/tokenizersが無いことを理由にsilent fallbackしてはならない。

M2/M4で次を行う。

1. release/dataごとに実際の<code>embedder_id</code>、model/tokenizer hash、384D normalizationをinventoryする。
2. 現行fallbackが正式sourceなら、同じPython実装をiOSでbit-level golden比較し、意味品質を改めて承認する。
3. SentenceTransformerが正式sourceなら、固定model/tokenizerをoffline native runtimeまたはrelease時変換済みCore ML packageで動かすspikeを行う。PyTorch一式の無条件bundleはしない。
4. desktop referenceに対し、同一日本語corpusのvector誤差だけでなくtop-k順序、tie、downstream consultation/profilerを比較する。
5. 採用providerはdesktop/iOS双方のartifact manifestへ固定し、model hash、tokenizer hash、runtime/conversion version、dimensionを<code>embedder_id</code>へbindingする。
6. <code>embedder_id</code>変更時は全LSM/vector indexを破棄してcanonical sourceから再構築し、異空間vectorを混在させない。

embedding model/runtimeも自動downloadせず、署名済みbundle artifactを原則とする。候補runtime、model conversion、license、required-reason API、archive sizeがgateを通らなければ、検索品質を落とすfallbackではなくM4を停止する。

### B4. search kernel

現行search daemonのprocess/mmap transportをiOSへ持ち込まない。384D AoSoA dot/top-kのC++数値kernelをstatic libraryとしてbuildし、Rustの固定C ABI経由で呼ぶ。ranking、filter、business ruleはPythonに残す。

desktop executableとiOS static libraryは同一golden vectorsで、score order、tie-break、NaN/Inf rejection、top-kをbit/許容誤差単位で比較する。

<code>PKBVEC01</code>のbyte layout、Python <code>pipeline.py</code>の<code>to_aosoa</code>/<code>write_binary</code>、desktopの<code>PKBSCR01</code> transportは変更しない。iOS static wrapperはvalidated 384D bufferとbounded top-kだけを受け、file format解釈やmerge ruleを新たにC++へ入れない。

### B5. model package

自動downloadは採用しない。二つの供給形態だけを許す。

1. G0-C（Pocket Brain）を通過した署名済みbundle model。App Store 1.0のbaseline。
2. Files document pickerでユーザーが明示importする署名済み<code>.pkbmodel</code> package。

model packageはGGUF、model role、architecture、quantization、exact size、SHA-256、publisher signatureを持つ。importはnative temp fileへstreamし、上限・symlink・signature・hashを検証後、atomic renameする。任意GGUFや未署名modelを「自己責任」で実行するtoggleは設けない。

model/runtimeはrecovery archive（<code>.pkbvault</code>）へ含めない。

### B6. memory / thermal / lifecycle

初期測定点は保守的に置くが、値を製品仕様へ決め打ちしない。

- context 2048から測定開始。
- G0-C.1のmemoryドクトリンに従う: weightsはmmap clean file-backed pages（Metal no-copy buffer）、dirty footprintは<code>phys_footprint</code>で実測し、最低対応端末のper-app上限の60%以下を維持する。<code>os_proc_available_memory</code>をmodel load前チェックに使う。
- JSON抽出modeはGBNF grammar拘束decodingを固定し、prompt/grammarは署名済みconfigから読む。
- sequence/session/modelは同時1個。
- lazy load。
- bounded batch/ubatch/output。
- idle、memory warning、protected-data unavailableで解放。
- Low Power Modeは低負荷profile。
- thermal seriousは現sessionの出力を短縮し、次sessionを低負荷profileで作る。context生成後のbatch変更はpinned llama APIで安全性を実証するまで行わない。
- thermal criticalは部分結果を保全し、session/modelを解放。
- app switcher snapshotでは本文をprivacy shieldで隠す。shieldの意匠はE8（F-A6）に従い、黒地＋静的グリフ1つとし、汎用blur・ロゴ演出を使わない。

iOSのMetal/background制約上、1.0の推論はforeground限定とする。[Preparing Metal apps for background](https://developer.apple.com/documentation/metal/preparing-your-metal-app-to-run-in-the-background)

foreground停止state machine:

1. <code>willResignActive</code>で新規decode/Metal submissionをatomicに閉じる。
2. 現decodeをbounded token/ubatch安全点まで進める。
3. pinned llama.cpp backendのsynchronization APIを使い、既存command bufferがschedule/completeしたことを確認する。
4. GPU利用終了後だけcontext/model resourceを解放する。
5. <code>didEnterBackground</code>後のcommand buffer create/commitを0にする。
6. 短いDB checkpointを終え、background taskを終了する。

ubatch上限はmemoryだけでなく最大deactivation latencyから決める。同期APIや停止順序を物理端末で証明できなければreleaseしない。<code>MTLCommandBufferError.notPermitted</code>はacceptanceで0件とする。

測定する値:

- cold/warm model load
- first-token latency
- tokens/s
- peak resident memory
- Metal allocated bytes
- context別KV cache
- repeated load/unload leak
- request間KV/prompt isolation
- energy impact / battery
- thermal transition
- memory warning / jetsam

最低対応端末で1回でも再現可能jetsamを起こす構成はreleaseしない。

## 7. Workstream C — 暗号化canonical store

### C1. Python repository seam

既存manager/facadeから直接file writeを剥がし、Python側にstrictなrepository protocolを置く。

~~~text
CanonicalRepository
  LegacyFileRepository   [migration/reference only]
  SqlCipherRepository    [new source of truth]
~~~

domain operation、validation、canonicalizationはPythonに残す。repositoryは任意SQL文字列を受けず、<code>append_diary_entry</code>、<code>upsert_calendar_event</code>等の閉じたoperationだけを持つ。

### C2. SQLCipher

<code>vault.sqlite</code>をSQLCipherで暗号化し、iOS Data Protectionも重ねる。[SQLCipher design](https://www.zetetic.net/sqlcipher/design/)

- DB keyは端末ごとにrandom生成。
- KeychainのThisDeviceOnly / unlocked時のみ利用可能なclassへ保存。
- DB keyを端末の外へ出さない（archiveへも含めない）。
- SQLCipher keyをログ、UserDefaults、WebViewへ出さない。
- DB、WAL、SHM、temporary fileの全経路を暗号化・保護する。
- system SQLiteとSQLCipherの曖昧な二重linkを避ける。
- app background/protected data unavailable前にtransactionを完了しDBを閉じる。
- canonical DB、WAL/SHM、model、derived、cache、stagingを<code>NSURLIsExcludedFromBackupKey</code>相当でOS/iCloud Backupから除外し、archive実体とrestore drillで確認する。

SQLCipherはat-rest暗号化であり、通信暗号・認証・競合解決の代替ではない。

G0-Dの推奨baselineは、CPythonのiOS用<code>_sqlite3</code> extensionを固定SQLCipher buildへlinkし、serialized Python engine workerだけが単一connection queueを所有する方式とする。Rust、native adapter、WebViewはvaultを開かない。

- 1processのSQLite実装とPKB vault connection ownerを各1つに限定。
- SQLCipher version、compile options、page/KDF settingsをmigration receiptへ記録。
- <code>SQLITE_TEMP_STORE=2</code>等のrelease compile optionを実体から検査し、runtime <code>temp_store=MEMORY</code>を固定。
- arbitrary PRAGMA/SQLをfrontend、native pluginから渡さない。
- sort/temp spillを誘発するqueryのrow/byte/time上限を定める。
- DB/WALだけでなくtemporary/sort spillも物理端末でplaintext scanする。

このlink/owner構成がM3 spikeを通らない場合はG0-Dへ戻り、Rust-owned single <code>VaultStore</code>案を規律変更として再審査する。実装途中で二つのownerを併存させない。

### C3. schema

最低限、次を持つ。

~~~text
schema_meta
vault_meta
canonical_record
journal_event
pending_signed_event
import_receipt
recovery_receipt
~~~

<code>journal_event</code>（旧<code>sync_event</code>の単一actor後継）の概念schema:

| field | rule |
|---|---|
| vault_id | random vault identity。全署名へdomain binding |
| actor_id | 本端末のjournal signing public key fingerprint |
| actor_seq | 永続単調増加値 |
| event_id | actor_id + actor_seq |
| prev_actor_event_hash | 直列chainの直前event hash |
| schema_version | exact supported value |
| entity_type / entity_id | allowlist / bounded |
| operation | allowlisted verb |
| observed_wall_time | 表示/provenance専用。securityへ使わない |
| canonical_payload | exact canonical JSON bytes |
| payload_hash | SHA-256 |
| signature | domain-separated actor signature |

journalは同期のためではなく、(a) 改竄・DB-only巻き戻しの検出（Keychain anchor照合）、(b) <code>.pkbvault</code> recovery archiveの完全性証明、(c) 監査可能な編集履歴のために持つ。署名preimageは少なくとも<code>PKB-JOURNAL-EVENT-V1</code>、vault ID、actor ID、sequence、previous hash、全header、payload hashをexact byte orderで含める。<code>actor_event_hash</code>は<code>PKB-ACTOR-EVENT-HASH-V1 || canonical unsigned event body</code>のSHA-256とし、signature bytesをhash chainへ含めない。chainのfork・gap・同sequence別hashを見つけた場合は自動修復せずvaultをquarantineし、固定errorとrecovery restore案内で報告する。

event append、materialized table更新、dirty marker更新は単一transactionで行う。全schema/signature/hash/sequence検証が終わるまでcanonical stateへ副作用を起こさない。unknown schema、gap、oversize、hash不一致はbatch全体をrollbackする。

### C3.1 単一actor identityとanchor

Standalone構成のtrust対象は本端末の単一actorだけである。membership chain、admin key、<code>actor_add</code>/<code>actor_retire</code>/<code>admin_transfer</code>、enrollment receiptはパージした（D3）。

- genesisで<code>vault_id</code>と本端末のjournal actor signing keyを固定する。actor keyはP-256 Secure Enclaveを優先し、代替はThisDeviceOnly Keychainとする。private keyを端末の外へ出さない。
- DB-only snapshot rollbackを検出するため、actor headのanchorをKeychain two-slotへ保存する。
- DBとKeychainは単一transactionにできないため、単純な「不一致=攻撃」にはしない。M3でDB-first checkpoint＋two-slot/pending anchorのcrash recoveryをADR化する。DBがKeychain anchorより先なら署名chainが旧anchorの正当な延長かを検証してanchorを前進できる。KeychainがDBより先、旧anchorの子でない、同sequence別hashならrollback/forkとしてrecoveryへ入る。全commit/anchor instruction boundaryでcrash testを行う。
- このanchorはhardware monotonic counterでも外部witnessでもない。DBとKeychain anchorを整合した同じ旧snapshotへ同時に戻すcoherent rollbackは1.0では検出保証しない。保証対象はDB-only rollbackと同sequence別hashである。
- 端末交換・復旧はM6のrecovery archive restoreだけで行う。restore先端末は新actor keyで**新epoch**を開始し、旧chainのroot/head hashをprovenanceとして新epochの最初のeventへ記録する。旧actor keyのsequence再利用・偽装継続を拒否する。

### C3.2 DeviceSigner / transaction ownership

SQLCipher connection、journal/materialized table、pending receiptのtransaction ownerは一貫してserialized Python repository workerである。Rust、UIKit、native adapterはvaultを開かず、native signerは署名とKeychain anchorだけを担当する。

logical seamは次へ固定する。

~~~text
Python SignedEventRepository
  -> reserve exact pending event in SQLCipher
  -> DeviceSigner.sign_reserved_event(...)   [iOS: closed _pkb_native call]
  -> verify signature with public key
  -> final DB transaction: journal_event + materialized state + dirty marker
  -> DeviceSigner.finalize_anchor(...)
~~~

- Pythonはvalidated canonical payload、actor sequence/previous hash、CIDからproposalを作り、まず<code>pending_signed_event</code>をDBへdurable commitする。まだmaterialized stateへ反映しない。
- native DeviceSignerはdomain/version/vault/actor/sequence/previous hash/operation/size/CID/reservation IDを独立にexact parseし、Keychainのcommitted actor headと一致する同一pending reservationだけを署名する。raw byte/stringを任意に署名するoracleを持たない。
- signerはsignatureとdeterministic actor event hashをtwo-slot Keychain pendingへ保存してから返す。response loss/retryでは再署名せず、同じreservationの保存済みsignatureだけを返す。
- Pythonはactor public keyでsignature/event hashを再検証し、<code>synchronous=FULL</code>相当のfinal transactionでevent append、materialized update、dirty marker、DB pending消去を一括commitする。その後だけnative signerへexact event hashのfinalizeを送り、committed anchorを前進する。
- restart時はDB pending/final eventとnative pending/committed anchorを照合する。DB pendingだけなら同じproposalを再開、native pendingだけならDB proposal hash一致時だけ再開、DB final＋native pendingならsignatureを再検証してanchor finalizeする。別proposal、sequence gap、同seq別hashはquarantineする。

DeviceSignerが持つclosed operationは<code>sign_reserved_event</code>、<code>finalize_anchor</code>、そしてM6の<code>sign_recovery_manifest(reservation_id, exact_manifest_fields)</code>のちょうど3つである。recovery manifest署名も同じDB-first reservation原則に従い、Pythonがsignatureを再検証してsigned manifestをreservationへ確定するまでarchive fileを外へ渡さない。generic bytesやarbitrary digestを署名するAPIをどのplatformにも作らない。

desktop（Windows/macOS）は現行file-based storeを継続し、vault/DeviceSignerを移植しない。Files/native adapterはSQLCipher connectionもDeviceSigner oracleも取得せず、既に確定したopaque artifact handleとfixed success/error eventだけを扱う。

### C4. 永続対象分類

| 分類 | 例 | 方針 |
|---|---|---|
| 正本 | user-entered diary/record、calendar/finance、imported message source、manual profile、永続化が既に許可されたprobe/interview debrief/report、明示保存したconsultation、curated ES/knowledge | signed journal＋materialized tableへ記録。recovery archiveへ含める |
| 条件付き正本 | OS Calendarから明示importしたevent、明示保存したLLM transcript/document | provenanceとconsentを保持した非権威documentとして記録。recovery archiveへ含める |
| 派生 | embeddings、vector/LSM index、tensor、deep profile、retrieval manifest、compiled narrative、cache | journal headへ束縛して端末内で再生成。archiveへ含めない |
| runtime | GGUF、llama/search runtime、Python/NumPy、artifact manifest、app binaries | 署名済みbundle artifact。archiveへ含めない |
| security | DB key、device private key、actor key、recovery KDF素材 | 端末localのみ。archiveへも含めない（recovery keyはユーザー保管） |
| diagnostics | logs、temporary inference、quarantine | 保全対象外。telemetryはそもそも存在しない |

「processed配下だから正本」「JSONだから正本」のようなpathベース判定にしない。entity schemaのallowlistだけで決める。

record transactionはcanonical eventをcommitし、対象のderived-generation dirty markerを立てるだけにする。そのtransaction内でprofiler、embedding、index/tensor rebuild、LLMを起動しない。現行の「RECORD保存でprofilerを走らせない」「LLM/embeddingは初回利用まで遅延初期化」を維持し、明示rebuildまたは次回consult/profileのbounded foreground jobで再生成する。

dirty markerは単なるbooleanにせず、必要な<code>canonical_head_hash</code>、schema、<code>embedder_id</code>、generator versionから<code>required_generation_id</code>を確定する。derived artifact manifestにも同じ入力とartifact hash/count/dimensionを署名・記録し、query時にcurrent required IDと完全一致する世代だけを使う。不一致またはdirty中に旧LSM/index/tensorをsilent利用せず、固定<code>DERIVED_STALE</code> stateで該当機能だけをambient unavailableにする。

rebuildはData Protection＋backup除外済みの新しいstaging generationへ行い、bounded count/dimension/hash、source head、embedder/runtime signatureを検証した後、manifest pointerをatomic swapする。crash、cancel、low disk、validation失敗では旧generationをactiveへ戻さず、orphan stagingを次回起動時に削除してdirtyを維持する。canonical dataの閲覧・入力は継続できるが、retrieval/profile等のderived依存機能は正しい世代が完成するまでfixed unavailableとし、品質の低いfallbackへ落とさない。

LLM由来documentは<code>provenance=llm</code>の非権威資料であり、受諾操作だけでdeterministic profile/tensor/authority stateへ昇格させたり、future promptへ自動再注入したりしない。ユーザーが観測事実として編集・再入力する場合は別operationとprovenanceで記録する。

probe/interviewのmemory-only turn state、priority queue、未完sessionを永続化しない。永続entityは<code>simulated</code>、speaker（self/contact）、source（subjective/objective）、provenanceをlosslessに保持し、simulated回答やLINE上のself発話を通常の観測事実へ誤分類しない。

### C4.1 contact identity（端末local）

現行のcontact identityと一部state chainはroot key派生である。Standalone構成では他端末との同一性要件が消滅したため、cross-device HMAC directory、wrapped key provision、identity mergeは設計ごとパージした（D3）。

- 既存<code>C-...</code> IDはcanonical opaque IDとして保持する。
- identity導出は本端末のroot keyだけで完結し、plaintext/normalized handleをDB/journal/logへ書かない。
- recovery archive restore後の新epochでも、archive内canonical recordのopaque IDをそのまま再利用し、再計算しない。
- <code>secure_identity.py</code>とstate-chain意味論の変更はG0-D ADRとgolden testなしに行わない。

### C5. 初期化とuser-file import

iOS appはdesktop dataを引き継がない。初回起動でgenesis（vault ID・actor key・空journal）を作り、以後の正本はすべて端末上で生まれる。desktop（Windows）製品は独立に現行構成で継続し、両者間のデータ移送は非目標である（D3）。

user-fileの明示import（LINE履歴txt、calendar/finance JSON等）は同期ではなく、次の規律に従う。

1. Files document pickerによる明示選択だけを入口とし、security-scoped URL→bounded staging copy→strict parser→journal transactionの一方向流路とする。
2. importはdry-run検証（malformed/duplicate/huge record）を先に行い、副作用0で結果を提示してからcommitする。
3. <code>import_receipt</code>へsource hash/size/record countを記録する。元fileを変更・削除しない。
4. corrupt/ambiguous dataをsilent repairしない。安全な固定errorと対象record IDを示す。
5. speaker/source/provenanceをimport時にlosslessに保持し、simulated回答やLINE上のself発話を通常の観測事実へ誤分類しない。

security-scoped provider URLはTOCTOU境界である。直接繰返し読まず、size/count上限付きでapp-owned・backup除外済みstagingへ一度copyし、hashを固定してprovider accessを閉じてからparseする。low disk、URL revoke、background、crashではpartial materialization 0とし、durable staging reservationから再開または安全に破棄する。

### C6. delete / journal semantics

単一端末・単一actorのため、装置間conflictは存在しない。旧conflict policy（sibling保持、OR-set、deterministic LWW）はパージした（D3）。

- delete: tombstone。1.0ではjournalからphysical GCしない。
- 1.0のdeleteはlogical deleteであり、暗号化されたjournal history内の旧本文は復号可能なまま残る。UI/privacy policyでこれを明示する。将来hard deleteを約束する場合は、署名付きcompaction epochまたはrecord keyのcrypto-shreddingを別設計する。
- journal chainの検証失敗（fork/gap/hash不一致）は自動修復せずquarantineし、固定errorとrecovery archiveからのrestore案内をambientに提示する。modal/alertを使わない。

## 8. Workstream D — 絶対孤立とRecovery Archive

### D1. 絶対孤立invariant

G0-Bのrelease invariantを実装・検証するworkstream。socket/DNS/Network.framework/Bonjour/camera symbolの静的scan、runtime port scan、全interface packet capture、permission/entitlement検査を§12.4のacceptanceへ落とし、機内モード完全動作を恒久テスト化する。証明するのは「同期が失敗する」ことではなく「**同期という概念がbinaryに存在しない**」ことである。

- negative testはM0でREDから書く: socket/DNS/camera/network symbol 0、app-owned network byte 0、network系Info.plist key 0。
- desktop dev環境のdev URL/HMR WebSocketがrelease archiveへ混入しないことを同じgateで検査する。
- WebViewはpickerの提示要求とsanitize済み結果eventだけを扱い、file bytes/path/provider URLへ到達しない。

### D2. <code>.pkbvault</code> recovery archive（単一actor）

iCloud/OS backupを使わない唯一のfull recovery経路として、<code>.pkbvault</code>を設計する。SQLite/zip fileの単純コピーにはしない。

- magic/version、bounded canonical manifest、KDF descriptor、chunk count/size、vault IDをexact headerへ固定。
- approved password/recovery-key KDF＋AEADでchunk単位暗号化し、exporting actorが<code>sign_recovery_manifest</code>（C3.2）でmanifestを署名する。
- canonical event（journal全chain）、materialized stateの再構築に必要なschema情報、import receiptだけを含める。
- model、derived index、cache、log、DB key、device private key、actor private keyを含めない。
- exportはユーザー明示操作のFiles document pickerで保存先を選ぶ。保存先providerがiCloud Drive等である可能性はユーザーの選択であり、暗号化がその前提である。export先path/destinationをlogへ残さない。
- importはnative stream→bounded staging→KDF/signature/hash/schema全検証→新しいdevice-local SQLCipher DBへtransaction適用するが、直ちにactive vaultへしない。**read-only <code>PendingVault</code>**で内容を確認させ、明示confirm後にatomic切替する。
- 復元先端末は新actor key・新epochで継続し（C3.1）、旧chain root/headをprovenanceとして記録する。旧actor sequenceの再利用・偽装継続を拒否する。
- wrong password、corrupt/truncated archive、replay、既存vaultより古いheadへのregression、zip-bomb相当のsize/count、path traversal、symlink、low disk、全crash boundaryを拒否する。
- 既存vaultへのmerge/replaceの意味をinline画面で明示し、silent overwriteしない。
- KDF/AEAD/recovery-key custody、archive rollback semanticsはG0-D ADRで確定する。設計・テストが完了するまで「端末紛失から復旧可能」と約束しない。

**単一端末集中リスクの明示:** 本設計では、最新のexport以降のデータは端末喪失とともに失われる。appはexportの鮮度（最終export日時のみ・宛先なし）をambient statusで常時表示し、periodic reminderを既存setting規律内で提供する。ただしauto-export、background export、cloud escrowは行わない。exportするか否かは常にユーザーの明示決定である。

全archive・Keychain anchorを失ったfresh disaster restoreでは、古いが正当に署名されたarchiveが最新版かを判別できない。created timeは表示metadataに留め、freshness保証に使わない。「最終と確認できないarchiveからの復元」と明示し、1.0の非保証として扱う。

### D3. パージ記録（Purge Record）

Commander裁定（Standalone Pivot）で次を設計ごとパージした。検討履歴はgit historyと非出荷contingency ADRにのみ残る。

- 旧LAN Sync一式（Bonjour/Network.framework/LNP/TLS/network XPC/<code>_pkb-sync._tcp</code>）。
- USB-mux/PeerTalk有線session（<code>wired-sync</code> feature、<code>127.0.0.1</code> listener、<code>pkb-wired-agent</code>）。
- <code>.pkbdelta</code>差分パッケージ、QR/<code>.pkback</code>受領証、camera permission（<code>NSCameraUsageDescription</code>）。
- multi-actor membership chain、admin authorization key、enrollment（<code>.pkbjoin</code>/<code>.pkbgrant</code>）、4-round admin transfer。
- cross-device contact identity directory（vault HMAC key provision）。
- desktop⇄iOS間のあらゆる継続的データ移送。

再導入の最低条件: Commanderの新directive、新threat model、該当G0の再開、現行規律原文の明示改定、別binary/versionのApp Review。source/build/capability/permission/Info.plistへ「将来のため」のcode・key・featureを一切残さず、§12.4のscanで0件を維持する。「standaloneだから安全」という表現で将来の同期追加を正当化することも禁止する——追加は常に新しい脅威モデルである。

## 9. Workstream E — モバイルUI / UX

### E1. viewport / safe area / keyboard

<code>index.html</code>を次の意図へ更新する。

~~~html
<meta
  name="viewport"
  content="width=device-width, initial-scale=1, viewport-fit=cover"
/>
~~~

CSS:

- <code>100vh</code> fallbackの後に<code>100dvh</code>を使う。
- top/right/bottom/leftへ<code>env(safe-area-inset-*)</code>を加える。
- composerとbottom actionはhome indicatorを避ける。
- software keyboard時はraw <code>visualViewport</code>を唯一の<code>PlatformPort</code>で観測する。pure reducerの<code>OBSERVE_VISUAL_VIEWPORT</code> Effect→strict <code>VIEWPORT_CHANGED</code> Event→<code>SET_VIEWPORT_CSS_VARS</code> Effectの順にし、component/<code>platformEffects</code>から直接触らない。CSS custom propertyへの書込は**表示専用・一方向の唯一の認可経路**であり、JS側から読み戻してstate/判断へ使わない（F-P4。真実はreducer stateだけとする）。
- fixed 420px chatをsmall viewportで解除する。
- orientation change、split view、Dynamic Typeでcontentを切らない。

safe-area実装はWebKitの[Designing Websites for iPhone X](https://webkit.org/blog/7929/designing-websites-for-iphone-x/)を基準にする。

WKWebView/CSS monospaceがUIKit Dynamic Typeへ自動追従するとは仮定しない。実機proofで追従しなければ、native <code>PlatformPort</code>が<code>UIContentSizeCategory</code>とBold Text変更を購読し、allowlisted category/scaleだけをstrict eventへ変換する。pure reducerがCSS variable更新Effectを返し、stateless <code>platformEffects</code>がportへ委譲する。起動時・runtime変更・最大accessibility sizeでreflow/横clipなしを検証する。

### E1.1 Bundled Japanese Monospace（F-A1）

iOSのinstalled-font inventoryや将来のOS変更へ美学を委ねず、projectがversion/metricsを固定できるBIZ UDGothicを同梱する。採用assetは公式Morisawa/Google Fonts sourceのunmodified Regular/Bold TTFだけとし、<code>BIZ UDPGothic</code>、variable/unknown build、subsetting、rename、format conversionを混ぜない。

- exact release tag/commit、<code>BIZUDGothic-Regular.ttf</code>/<code>BIZUDGothic-Bold.ttf</code> SHA-256、PostScript/family name、size、OFL/copyright hashをsigned mobile artifact manifestへ含める。
- Xcode Resources target membershipと<code>UIAppFonts</code> arrayをgeneratorから作り、missing/extra/duplicate filenameをhard failする。system-wide font install entitlementは使わない。[Adding a custom font to your app](https://developer.apple.com/documentation/uikit/adding-a-custom-font-to-your-app)
- Web CSSは既存<code>--font-ui</code>/<code>--font-mono</code> stackの先頭を登録済み<code>"BIZ UDGothic"</code>へ固定し、remote/local <code>@font-face</code>を新設しない。UIKit/AppKit ceremonyもresolved PostScript nameを使い、fallbackを黙認しない。
- M1実機でWKWebView、UIKit、iPad compatibility modeが同じRegular/Boldを解決することをCoreText/PostScript照会＋representative Japanese/Latin/full-width glyph metric golden＋screenshotで証明する。WebKitから解決不能ならG0-E No-Goへ戻す。
- Dynamic Type/UIFontMetrics相当のscale、Bold Text、最大accessibility categoryでも全角grid、line-height 1.6、no horizontal clip、VoiceOver reading orderを維持する。
- OFL 1.1全文、copyright、source/version/hashをbundle内license resourceとoffline legal viewへ置き、font network requestをpacket/URL protocol testで0にする。

### E2. platform chrome

現行<code>TitleBar</code>はTauri存在だけでdesktop controlsを描くため、iOSでも誤表示する。runtime capabilityからplatformを判定し、iOSではTitleBar全体を描画しない。desktop window control permissionをiOS capabilityへ入れない。

### E3. navigation

7 tabとARIA manual activation semanticsは維持する。

- small viewportでは1行horizontal-scroll tablist。
- active tabをeffectでviewへ入れる。
- label/order/keyboard navigationをdesktopと共有。
- hamburger、drawer、modal navigationを新設しない。
- inactive panel unmount方針を維持し、memoryを抑える。

### E4. touch / forms

- interactive targetは最低44×44 CSS px。**44pxは透明なhit-area拡張（padding・擬似要素・hit-slop）で満たし、視覚box・罫線・文字サイズの肥大でledger密度を崩さない（F-A5）。** visual boxとhit boxの分離を標準とし、1px罫の台帳様式・タブ寸法をdesktop規定から太らせない。
- hoverだけのaffordanceを禁止。
- <code>pointer: coarse</code>でspacingを調整。
- textareaにinputMode、enterKeyHint、autoCapitalize等を用途別に設定。
- selection、caret、copy/paste、external keyboard、VoiceOverを実機確認。
- date/time rolling controlはtap targetを拡張し、gesture追加時もpure reducerへactionを送るだけにする。
- large file importは<code>File.arrayBuffer()</code>＋JSON IPCの全量多重copyをやめ、native document picker→bounded stream→verified temp fileとする。
- macOS Calendar DB直読みの代わりに、iOSはEventKitを使うcustom Tauri mobile pluginを実装する。<code>NSCalendarsFullAccessUsageDescription</code>とruntime authorizationを分離し、permissionとimportは明示操作時だけ。[Calendar usage description](https://developer.apple.com/documentation/bundleresources/information-property-list/nscalendarsfullaccessusagedescription)

### E5. Haptic Grammar（F-A4）

触覚は「許可event一覧」ではなく、Silent Terminalの視覚的音量と同じsemantic transitionを表す文法とする。pure reducerはraw <code>light/medium/heavy</code>やplugin methodではなく、次のclosed grammar tokenだけをEffectへ出す。公式Tauri haptics pluginを採る場合も、selection/impact permissionだけをexact pinする。[Tauri Haptics](https://v2.tauri.app/plugin/haptics/) / [UIImpactFeedbackGenerator](https://developer.apple.com/documentation/uikit/uiimpactfeedbackgenerator)

| Grammar token | 意味 / 同時に存在すべき視覚音量 | Native mapping |
|---|---|---|
| <code>SILENT</code> | ambient progress、focus/hover、typing、scroll、streaming、spinner、background、automatic state | 触覚なし |
| <code>NAV_TICK</code> | tab/segmented selectionが実際に変わり、局所active inverse/白borderが移る | selection、またはlight impact 1回 |
| <code>COMMIT</code> | local durable save、verified user-file import commit。静的status＋白borderで確定を示す | medium impact 1回 |
| <code>CEREMONY_SEAL</code> | consent ON、recovery export/import・restore確定。承認済みconsent/confirmed seal領域だけinverse | heavy impact 1回 |
| <code>FAIL_CLOSED</code> | 初回のterminal warning/error。静止inverse errorが最大視覚音量 | rigidまたはmedium impact 1回。反復なし |

- button down/clickではなく、reducerがsemantic state transitionの成功/失敗を確定した一度だけ発火する。re-render、duplicate event、retry、cancelで再発火しない。
- visual/ARIA/VoiceOverが先であり、触覚は代替にもsecurity commit証拠にもならない。hardware/system setting/plugin failureは固定no-op <code>HAPTIC_UNAVAILABLE</code> Eventとし、操作結果を変えない。
- keypress、token、文字入力、scroll、animation frame、progress tick、backgroundでは常に<code>SILENT</code>。
- <code>notificationFeedback</code>、<code>UINotificationFeedbackGenerator</code>、<code>vibrate</code>、success/warning/errorのOS notification pattern、multi-pulse、連続/独自波形を禁止し、対応plugin permission/symbolをarchiveから除外する。
- device-local disable toggleを既存setting規律に従って持てるが、新state libraryや<code>platformEffects</code>内storageを作らない。

### E6. Stateless <code>platformEffects</code> contract（F-P1）

public contractは概念上次へ固定する。

~~~text
pureReducer(state, DomainEvent)
  -> { nextState, effects: readonly PlatformEffect[] }

platformEffects.execute(readonly PlatformEffect)
  -> Promise<readonly ParsedPlatformEvent[]>
~~~

<code>platformEffects</code>はdomain/product stateを一切保持しない。現在のplatform、viewport、capability、permission、pending export、haptic debounce等をmodule variable/cache/closure singletonへ置かない。Effect input以外から判断せず、injected immutable function tableである<code>PlatformPort</code>へ委譲し、portのraw <code>unknown</code> resultをeffect固有parserへ通したEventだけを返す。

long-lived observationは次のresource ownershipへ分離する。

~~~text
Reducer -> START_VIEWPORT_OBSERVER(effect_id)
Runner  -> PlatformPort.startObserver(effect_id)
Host    -> owns opaque dispose handle only
Port    -> raw value -> strict parser -> VIEWPORT_CHANGED
Reducer -> STOP_VIEWPORT_OBSERVER(effect_id)
Host    -> dispose exact handle
~~~

host/portはhandle lifetimeだけを持ち、domain state、retry、dedupe、policy、event orderingを決めない。component unmount、background、cancelでSTOP Effectを必ず出し、listener leakを0にする。

grep＋import graph gateはexact <code>PlatformPort</code>実装以外の次を0件にする。

- <code>visualViewport</code>、<code>matchMedia</code>、<code>document.documentElement</code>/<code>style.setProperty</code>
- <code>@tauri-apps/api</code>、<code>@tauri-apps/plugin-*</code>、raw <code>invoke/listen/getCurrentWindow</code>
- direct haptics、document picker、LocalAuthentication call
- <code>localStorage</code>/<code>sessionStorage</code>/<code>indexedDB</code>、module mutable cache/queue/timer/retry

現行componentに残る直接Tauri/window/listener importはM5のFreeze Transition ADRへexact列挙して移す。grepだけでalias importを逃さず、dependency-free boundary harnessでcomponent→port/plugin importを拒否する。同一Event列から得るstate＋Effect列のbyte-equivalence、malformed raw resultのfixed rejection、repeated renderでeffect再発火0をacceptanceにする。

### E7. Native Ceremony Design Annex（F-A2）

WebView外のapp-owned UIKit/AppKit surfaceは「標準native UIだから例外」ではない。次のtoken mappingを単一native style ownerへ固定し、consent、archive export/restore、recoveryの全ceremonyで共有する。

| Web token / rule | UIKit / AppKit mapping |
|---|---|
| <code>--bg: #0a0a0a</code> | root ceremony background、traitにより有彩色へ変えない |
| <code>--bg-deep: #000000</code> | verified detail / secure input region |
| <code>--text: #f2f2f2</code> | primary text / 1px active border |
| <code>--text-muted: #8c8c8c</code> | secondary/provenance text、AA floor維持 |
| <code>--border: #2e2e2e</code> | 1 physical-pixel hairline |
| <code>--accent: #ffffff</code> | current focus/actionだけ。system blue禁止 |
| radius 0 | 全app-owned button/card/field/route。pill、sheet card、capsuleなし |
| inverse video | 選択、静止error、consent/confirmed sealだけ。白地<code>#0a0a0a</code>字 |
| typography | bundled BIZ UDGothic Regular/Bold、Dynamic Type scale、line-height相当1.6 |

- <code>UIAlertController</code>、<code>NSAlert</code>、stock rounded button/card、system-blue tint、material/blur、gradient、有彩色、drop shadowをapp-owned ceremonyへ使わない。iOSは専用full-screen native route、macOSは専用AppKit window/routeを使う。
- operation/archive root/commit stateはnative verified objectから再表示し、WebViewのcopy/transcriptを署名・承認対象にしない。raw key/signature/path/actor IDを表示しない。
- BIZ UDGothic、VoiceOver順序、safe area、UIFontMetrics/Dynamic Type、Bold Text、Increase Contrast、Reduce Motion、44pt targetを全routeで守る。新しいinfinite animationを持たない。
- 実confirm後のsemantic transitionだけE5 grammarを発火し、派手なsystem notification hapticを使わない。

Face ID/Touch ID/passcode、calendar permission、Files pickerはOS所有であり、appが色・radius・fontを安全に上書きできない**trusted system exception**である。これらを独自に模倣、overlay、偽glyph、私製biometric promptとして再描画しない。app-owned pre-auth screenをAnnexどおり表示し、固定・非PIIの<code>localizedReason</code>から本物のsystem sheetを起動し、戻り先のpost-auth receiptもAnnexどおりにする。対応OS/APIでAppleのembedded authentication viewを採用できる場合も標準biometric iconを保持し、その周囲だけをstyleする。[Local Authentication Embedded UI](https://developer.apple.com/documentation/localauthenticationembeddedui)

visual acceptanceはOS sheet領域をpixel comparisonから明示除外し、その前後のapp-owned routeだけをWeb/native token parity screenshot、R=G=B scan、corner-radius/blur/system-blue scan、VoiceOver/Dynamic Type実機testへ通す。

### E8. Silent Terminalのモバイル解釈

- grayscale paletteを維持。
- bundled BIZ UDGothicを第一fontとし、承認済みlocal fallbackだけを使う。web fontをfetchせず、<code>@font-face</code>を黙って追加しない。
- radius 0、1px border、白glow限定。
- selection/error/consent ONだけinverse video。
- inferenceは現行仕様で許可済みのanimationだけを再利用する。archive export/import、index rebuildはstatic status/determinate progressを基本とし、新規spinner/pulse loopを追加しない。
- permission denied、conflict、thermal stopを赤/黄/緑で表さない。
- OS-mandated permission/Files/Face ID sheetはAnnex準拠のapp-owned pre/post routeから明示操作の文脈でだけ誘発し、その上へapp独自overlay/modalを重ねない。それ以外の状態説明はscreen内ambient regionで行う。
- **OLEDの公式採用（F-A3）:** iOSではapp背景の基底を<code>--bg-deep: #000000</code>へ落とし、<code>viewport-fit=cover</code>の全面でOLED発光ゼロ領域とUIの境界を消す。notch/Dynamic Island/home indicator周辺が漆黒に溶け、端末の物理輪郭ごとSilent Terminalになることを意匠として明文化する。black smear（純黒上の灰文字スクロール残像）と、静的1px白罫・inverse video領域の焼き付き傾向はOLED実機受入（§12.6）で確認し、必要な調整は既存灰調トークンの範囲でだけ行う。有彩色・blur・radiusによる回避を認めない。
- **Launch Screenとprivacy shield（F-A6）:** iOS Launch storyboardとapp switcher privacy shieldは黒地（<code>#000</code>/<code>#0a0a0a</code>）＋静的グリフ1つ（例: <code>§</code>）で規定する。汎用blur、spinner、ロゴアニメーション、有彩色を使わない。shieldは本文・provenance・research状態を完全に覆い、復帰時にアニメーションで剥がさない。

## 10. 段階ロードマップ

3名体制の想定役割:

- Platform/Release: Tauri、Rust、Xcode、signing、native plugin。
- Core/Data: Python、embedded runtime、SQLCipher、journal/anchor、user-file import、recovery archive。
- UI/Quality: React/CSS、accessibility、E2E、device matrix。LLM性能検証は全員で共有。

| Milestone | 期間目安 | 依存 | 主な成果物 | Exit gate |
|---|---:|---|---|---|
| M0 規律・Pocket Brain選定 | 3〜5週 | なし | ADR G0-A〜E、絶対孤立ADR、Freeze Transition ADR、Native Ceremony Annex、Haptic Grammar、font/license manifest、data map、acceptance matrix、LLM候補benchmark（rubric golden凍結・実機footprint一次実測・eligibility裁定材料） | Standalone裁定、eligibility Option/model shortlistの承認者署名 |
| M1 v2/iOS shell | 3〜5週 | G0-A/E | exact version pin、ios init、iOS/iPadOS 17 pin、platform config/capability、生成物方針、BIZ UDGothic/UIAppFonts/OFL、fake backend | desktop regression 0、minimum/latest iPhone＋iPad compat cold launch、WKWebView/UIKit font proof |
| M2 embedded engine spike | 6〜8週 | M1、G0-A | CPython 3.13 XCFramework、AppleFrameworkLoader packaging、NumPy per-module frameworks、project再生成、代表facade operation | 物理端末、marker/origin/codesign archive scan、golden parity |
| M3 canonical vault | 6〜10週 | M2、G0-D | repository seam、SQLCipher schema、単一actor journal/anchor、DeviceSigner reservation、artifact manifest再生成、user-file import dry-run | signer/DB crash matrix、plaintext/temp漏洩0、rollback drill |
| M4 Pocket Brain統合 | 8〜12週 | M2、G0-C | llama/Metal in-process、GBNF grammar decoding、採用GGUF署名bundle、384D provider、C++ static kernel、artifact manifest再生成 | rubric一致率gate、<code>phys_footprint</code>/thermal acceptance、embedder/quality gate |
| M5 mobile UI / effect boundary | 7〜10週 | M1、G0-E | safe area、keyboard、touch、stateless platformEffects/PlatformPort、Haptic Grammar、Native Ceremony base、Files/custom EventKit、iPad compatibility mode | grep/import boundary、VoiceOver、Dynamic Type、native/Web visual parity、G0-E再freeze ACK |
| M6 recovery archive | 4〜6週 | M3、G0-D/E | <code>.pkbvault</code> export/import、PendingVault、新actor epoch、recovery key、streaming import/export、export鮮度ambient status | tamper/rollback/crash/low-disk tests、restore drill |
| M7 parity・hardening | 5〜8週 | M3〜M6 | native dependency/font/license最終freeze、clean再生成、全command parity、lifecycle、fuzz、performance、freeze hash closure | Standalone release acceptance matrix全通過 |
| M8 TestFlight/App Store | 4〜6週 | M7 | privacy/export、review notes、archive、TestFlight | review-ready sign-off |
| M9 post-release | 継続 | M8 | local diagnostics export、security response、migration policy | network drift 0、signed releaseのみ |

M3/M4/M5はM2のGo後に並行化できるが、Freeze Transition ADRで同一file/symbolを同時解凍しない。M6はcanonical storeの方式確定後に開始する。<code>Standalone</code>単一profileで**40〜62週**（25〜35% contingency込み）を目安とし、M0 Pocket Brain benchmarkとM2/M3 exitで再baselineする。

### 最初の30日

1. G0 ADR、絶対孤立ADR、Freeze Transition ADR、Native Ceremony Annex、Haptic Grammarを作成・承認。
2. exact current toolchain inventoryとv2 migration audit。
3. macOS release hostで<code>tauri ios init</code>。
4. generated Apple projectをdisposableにするnative dependency/template/privacy-manifest方針を固定。
5. base configからdesktop sidecar設定を分離。
6. fake in-process backendでiPhone simulatorに既存React shellを表示。
7. CPython 3.13＋NumPy iOS minimal framework spike。
8. 代表operationを3つ選び、3.12 desktopとのgolden corpusを凍結。
9. Pocket Brain: 感情分析/家計簿JSON rubric goldenを凍結し、candidate GGUF（1〜2B級/3〜4B級）をdesktop harnessで一次選抜。
10. 実機（最低対応候補機とA17 Pro級）でllama.cpp Metalのdirty footprint/tokens/s/thermalを一次実測し、eligibility裁定（G0-C.1 Option A/B）の材料を確定。
11. canonical/derived/security data inventoryを確定。
12. socket/DNS/camera/network symbol 0・app-owned network byte 0・network系plist key 0のnegative test仕様を先にREDで追加。
13. BIZ UDGothic Regular/Boldのsource/tag/hash/OFLを固定し、<code>UIAppFonts</code>＋WKWebView/UIKit実機proofを作る。
14. component/TitleBar/engineのdirect Tauri/DOM API importをinventoryし、M5 thaw allowlistとPlatformPort grep/import gateをRED化。
15. active <code>embedder_id</code>とembedding model/runtime artifactをinventory。

### 60〜90日

- embedded Pythonでhealth、record、consultation smoke。
- <code>_pkb_native</code>のLLM/search prototypeと384D embedding provider spike。
- SQLCipher schema、genesis/journal/anchor prototype、user-file import dry-run。
- safe area/titlebar/tablist/composer、bundled font、stateless effect boundary、native ceremony token mappingのmobile proof。
- <code>.pkbvault</code> writer/reader、PendingVault、Files picker export/importのprototype（当然networkless）。
- Pocket Brain二次選抜: 実機rubric再計測、量子化比較（Q4_K_M/Q5_K_M/imatrix）、GBNF拘束decodingのschema妥当率100%確認、prompt injection耐性。
- App privacy/export/license reviewの初回判定。

## 11. App Store / TestFlight提出計画

### 11.1 self-contained

App Review Guideline 2.5.2に合わせ、全executable codeを署名済みapp bundleへ含める。[App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/)

- CPython/libPythonとNumPy/<code>_sqlite3</code>等のPython binary extensionは公式AppleFrameworkLoader形式のdynamic frameworkとして個別署名する。llama/search/SQLCipher等の内部native libraryは最終appまたは対応extension frameworkへstatic linkし、独立実行物を作らない。
- BIZ UDGothic Regular/Bold、OFL 1.1/copyright、asset manifestを自己完結bundleへ含め、<code>UIAppFonts</code>とarchive hashを一致させる。fontをnetwork/On-Demand Resourcesから取得しない。
- runtime plugin、pip、script、server binaryをdownload/executeしない。
- GGUFは固定architecture/operator、JIT/custom executable opなしのbounded model dataとして申告し、role/hash/signatureを検証する。この扱いを一方的な確定事実とせず、Review dossierと実審査で確認する。
- optional model importを含める場合は目的と制限をReview Notesへ説明する。
- account/cloudなしでcapture/review/importを評価でき、local LLMはbundle済みproduction modelで再現できる状態にする。
- Guideline 2.1向けにfull-access review手順とsample data/modelを用意する。Mac companionは不要である。
- Guideline 4.2向けに、on-device inference（GBNF拘束の構造化抽出を含む）、encrypted vault、OS-native Files/EventKit/haptics/native ceremonyが単なるrepackaged websiteを超えるnative utilityであることを実機evidence付きdossierにする。

### 11.2 privacy

- <code>PrivacyInfo.xcprivacy</code>をbundleへ含める。
- Tauri、Rust crates、Swift packages、CPython、NumPy、llama、SQLCipherのrequired-reason APIを棚卸し。
- App Privacyは「developerが受信するデータ」をApple定義に照らして正確に回答。
- 同期・P2P機構は存在せず、ユーザーdataはdeveloperへ一切送信されない。「収集なし」回答の根拠をApple定義に照らして法務確認する。
- App Store Connect metadataへPrivacy Policy URLを設定し、app内にも容易に到達できるlocal/offline policy viewとURLを置く。
- privacy policyにon-device保存、暗号化recovery archiveとユーザー選択の保管先（iCloud Drive等を選べばそのproviderへ暗号文が置かれること）、logical deleteの限界、単一端末集中リスク（export鮮度）、developer非アクセスを記載する。
- analytics/crash telemetry/広告IDを入れない。
- Xcode archive privacy reportを確認し、Apple指定third-party SDKのmanifest/binary signatureを検査する。OpenSSL等がtransitiveに入る場合も対象とし、可能ならApple system crypto/CommonCrypto構成を優先する。

Appleの[privacy manifest](https://developer.apple.com/documentation/bundleresources/adding-a-privacy-manifest-to-your-app-or-third-party-sdk)、[required-reason API](https://developer.apple.com/documentation/bundleresources/describing-use-of-required-reason-api)、[App Privacy](https://developer.apple.com/app-store/app-privacy-details/)をreleaseごとに再監査する。

### 11.3 export compliance

SQLCipher、TLS、署名、Keychain/Secure Enclaveを含むため、Appleのexport questionnaireと法務判断をrelease gateにする。[Export compliance overview](https://developer.apple.com/help/app-store-connect/manage-app-information/overview-of-export-compliance/)

OS暗号だけか、第三者暗号実装を含むか、配布国、免除条件を記録する。根拠文書なしに<code>ITSAppUsesNonExemptEncryption</code>を固定しない。

### 11.4 background / energy

- LLM background executionを宣言しない。
- appはforeground/backgroundを問わずnetwork listener/connectionを一切持たない。
- sync/DBの短い終了処理をLLM継続へ流用しない。
- thermal/energy test evidenceを残す。
- unsupported background mode entitlementを追加しない。

### 11.5 signing / upload

1. Apple Developer Program membership、App Store Connect app record、App ID、bundle identifier、teamを確定。
2. Privacy Policy URL、Support URL、age rating、accessibility declarations、product metadataを確定。
3. Automatic signingまたは管理されたApp Store profileを固定。
4. archiveのentitlements、nested code/framework signatures、privacy manifest、slices、<code>UIDeviceFamily=[1]</code>を検査。
5. TauriのApp Store export methodでIPAを生成。
6. seed buildをuploadし、supported-device listに未検証surfaceがないことを確認。
7. App Store ConnectでApple silicon MacとApple Vision Pro availabilityをoffにしてrecord化。
8. TestFlight internal。
9. 物理端末acceptance。
10. TestFlight external向けBeta App Review informationとfull-access resourcesを提出。
11. App Review Notes、privacy、export、screenshotsを確定し、phased release可否を判断。

公式手順は[Tauri iOS signing](https://v2.tauri.app/distribute/sign/ios/)と[Tauri App Store distribution](https://v2.tauri.app/distribute/app-store/)を基準にする。

Review Notesには次を明示する。

- account/cloud/internet不要で、app-owned network path/port/permissionが一切存在しないこと（機内モードで全機能が動作する）。
- 復旧はユーザー明示操作のFiles document pickerによる暗号化<code>.pkbvault</code> export/importだけであること。review用のexport→restore fixtureを用意する。
- <code>UIDeviceFamily=[1]</code>のiPhone-optimized appである一方、iPadOS compatibility modeでも同等機能を提供することと、そのreview手順。
- （eligibility Option A採用時）<code>iphone-performance-gaming-tier</code>を要求する理由がon-device LLMのmemory要件であること。
- modelの同梱方式とoptional署名済み<code>.pkbmodel</code> import。
- backgroundで推論・listenerを動かさないこと。
- ユーザーデータをdeveloperが受け取らないこと。

## 12. 検証・受入マトリクス

### 12.1 architecture / egress

- WebViewからHTTP/WebSocket/DNSが0。
- Pythonからnetwork import/socket creationが0。
- llama buildにserver/RPC/network symbolがない。
- app process全体で<code>socket/bind/listen/connect</code> callsiteが0（allowlist自体が存在しない）。Network.framework、Bonjour、MultipeerConnectivity、URLSession data/upload/download、WebSocketのowner/symbol/success pathが0。runtime port scanと全interface packet captureでapp-owned network byteが0。
- 旧<code>lan-sync</code>/<code>wired-sync</code>/delta/QR関連のfeature、symbol、command、capability、permission、Info.plist keyが0。<code>NSCameraUsageDescription</code>、<code>NSLocalNetworkUsageDescription</code>、<code>NSBonjourServices</code>、network entitlement、network XPC、PeerTalk/usbmux残滓が0。
- <code>egress-live</code>はoffで、transport owner/symbol/capability/permission/success pathがIPAとartifact manifestに0。parity用egress commandはfixed refusalだけ。
- Files pluginはarchive/user-file/model importとarchive exportだけを提示し、bytes/path/provider URLをWebViewへ返さず、任意network APIを所有しない。picker操作の成功をcommit証明にしない。
- CSPにhost/IP/wildcardを追加していない。
- release archiveにdev URL、HMR WebSocket、dev CSP、ATS exceptionがない。
- iOS Python archiveにsocket/DNS/SSL client、ctypes/cffi、user-controlled arbitrary <code>dlopen</code>、subprocess、writable import pathがない。binary importはmanifest列挙済みAppleFrameworkLoader frameworkだけ。
- Python built-in/extension/import/framework allowlistとnative linked-symbol scanが一致する。
- dynamic-loader symbol/callsite allowlistはpinned libPython/AppleFrameworkLoaderだけで、Rust app、<code>_pkb_native</code>、NumPy package Python codeから任意loader APIを呼べない。
- raw <code>.so</code>、manifest外<code>.fwork/.origin</code>、Frameworks外Python extension executableが0。per-module frameworkのbinary数/Info.plist/slice/codesignとmarker↔origin往復が一致する。
- IPAにmanifest一致のBIZ UDGothic Regular/Bold 2 fileとOFL/copyrightがあり、<code>UIAppFonts</code> filename、PostScript name、SHA-256が一致する。<code>BIZ UDPGothic</code>、remote font、font-install entitlement、余分/missing fontは0。
- iCloud/CloudKit entitlement、ubiquity containerがない。
- canonical DB/WAL/SHM/staging、model、derived、cacheの全PKB管理fileがOS/iCloud Backup対象外である。ThisDeviceOnly keyだけが欠落したorphan DB restoreを作らない。
- <code>src-tauri/gen/apple</code>のclean再生成後に差分allowlist、minimum iOS、framework slice/link、Info.plist、entitlement、privacy manifestが一致する。
- IPAの<code>UIDeviceFamily</code>が<code>[1]</code>だけで、seed build supported-device listがiPhone＋mandatory iPad compatibility surfaceを正確に記録し、Apple silicon Mac/Vision opt-out recordと一致する。

### 12.2 contract / parity

- 全既存Tauri commandのexact schemaがdesktop/iOSで一致。
- <code>get_runtime_capabilities.egress_live=false</code>、egress UI action非提示、既存egress commandの固定<code>EGRESS_LIVE_NOT_READY</code> responseをdesktop/mobile fixtureで一致させ、network owner到達0。
- pure reducerの同一state/Event列からstate＋<code>PlatformEffect</code>列がbyte-equivalentで、runner出力はstrict parse済みclosed <code>PlatformEvent</code>だけ。malformed raw result、unknown key/type、raw exception/path/payloadをfixed rejectする。
- component/<code>platformEffects</code>/domain moduleから<code>visualViewport</code>、Tauri/plugin/window/storage APIへのdirect access 0。exact <code>PlatformPort</code>以外のimport graph/grepが0で、runnerにmodule mutable state、hook、timer、cache、queue、retry、subscription handleがない。
- repeated render/duplicate/cancelでpicker提示、native ceremony、hapticの再発火0。observer START/STOP/unmount/backgroundでopaque handle leak 0。
- unknown key/type/oversizeを両backendで同じ固定errorにする。
- CID、token/status/final order、cancel semanticsをgolden比較。
- desktop Python 3.12.10とiOS Python 3.13.xのcanonical hash、ranking、profile、promptを比較。
- C++ executable/static kernelのtop-k parity。
- active 384D embedderのvector/normalization/top-k/downstream parityと<code>embedder_id</code>再構築を検証。
- canonical commitと<code>required_generation_id</code> dirty markerが同transactionで、stale manifest/旧index queryは固定<code>DERIVED_STALE</code>となり結果を返さない。
- rebuild stagingのsource-head/embedder/generator/hash/count/dimension検証、atomic manifest swap、各instruction crash/cancel/low-disk後のdirty維持とorphan cleanup。
- <code>simulated</code> true/false、self/contact、subjective/objectiveの対照corpusでimport/restore後の分類とweightが一致。
- LLM provenance documentがauthority state/profile/future promptへ自動昇格しない。
- desktopの現行全Python/Rust/TypeScript suiteを維持し、件数はCI collectで動的記録する。
- iOS未実装commandをruntime surpriseにせず、release前に全parityを満たす。

### 12.3 storage / migration

- wrong key、corrupt page、truncated WAL、low diskでhard fail。
- DB/WAL/tempのplaintext scan。
- release SQLCipher compile optionsとruntime <code>temp_store=MEMORY</code>を実体検査。
- user-file importのmalformed/duplicate/huge record。
- import dry-runの副作用0とdry-run→confirm→commitの順序固定。
- import中crashの各instruction boundary。
- import receipt/hash/count一致、元file不変。
- genesis/journal head root＝archive root＝independent restore rootの一致。
- Application Support/Documents/tmp/staging/app-owned export pathのplaintext canary scanが0。
- M6 manual archive export/restore drill。OS backupをrecovery pathとして扱わない。
- DB commit/Keychain anchor更新の全crash boundary、DB-ahead recovery、Keychain-ahead rollback拒否。
- DeviceSignerのDB reservation前後、native pending/sign前後、response loss、final DB commit前後、anchor finalize前後、restart/duplicateを網羅し、same pending signature再利用、sequence gap/fork/二重materialize 0。
- iOS <code>_pkb_native</code> DeviceSignerのexact logical fixture、arbitrary-sign request/unknown domain/size/CID/state拒否、WebView/native adapterからのsigner到達0。
- native signerはsignatureだけを返し、PythonだけがSQLCipher commitすることをconnection-owner/instrumentation testで確認。
- recovery manifest署名のDB reservation、native exact sign、Python public verification、export write、import commit、anchor finalizeの全crash boundary。same reservation signature再利用、arbitrary digest拒否、native adapterのSQLCipher connection 0。
- protected data unavailable時のDB close。

### 12.4 isolation / recovery（Standalone）

絶対孤立:

- 機内モード（全無線off）で全機能（記録、consult、profile、user-file import、archive export/import、model load/推論）が完走する。
- IPA/app processのport/listener/connect/DNS/mDNS/Bonjour、camera、Local Network keys/prompt、network entitlement/XPCが0。
- Wi-Fi/Bluetooth/cellular on時も、全interface packet captureでapp-owned network byteが0。
- release archiveにdev URL、HMR WebSocket、dev CSP、ATS exceptionがない。

journal / anchor:

- journal chainのfork/gap/同sequence別hash/hash不一致でquarantineし、自動修復・silent継続をしない。
- DB-only snapshot rollbackをKeychain anchor照合で検出する。DB-ahead recovery、Keychain-ahead rollback拒否、全commit/anchor境界crash。
- DB＋Keychainのcoherent rollbackは非保証としてtest reportへ明記する。

user-file import:

- malicious provider/URL差替え/TOCTOU、truncate、oversize、malformed/duplicate/huge recordでDB副作用0。
- dry-run→confirm→commitの順序固定、import receiptのhash/count一致、元file不変。

<code>.pkbvault</code> recovery:

- wrong recovery key/KDF params、tamper、truncation、replay、既存vaultより古いheadへのregression、zip-bomb相当size/count、path traversal、symlink、low disk、全crash boundaryを拒否。
- model/derived/log/DB key/device private key/actor private keyがarchiveに含まれない。
- staging/import/DB commit/atomic切替の全crash boundaryでpartial materialization 0。
- import直後はread-only PendingVaultで、confirm前のedit/export/derived rebuildが0。
- 復元後の新actor epochが旧chain root/headをprovenanceとして記録し、旧actor sequenceを再利用しない。
- 既存vaultへのmerge/replaceが明示選択どおりで、silent overwrite 0。
- export鮮度ambient statusが正確で、export完了をpicker操作の成功ではなくwrite後の再読込検証で判定する。
- 全archive/Keychain喪失時のfresh disaster restoreでfreshness非保証が明示される。

### 12.5 LLM / lifecycle

- 最低eligible iPhone、最低eligible iPadOS compatibility-mode端末、提出時の代表的な最新iPhone/iPadまでの物理端末matrix。
- cold/warm、最大prompt/output、繰返しload/unload。
- cancel、lock、background/foreground、memory warning。
- Low Power Mode、thermal nominal/fair/serious/critical。
- <code>willResignActive</code>後の新規Metal submission 0、<code>didEnterBackground</code>後のcommand buffer commit 0。
- <code>MTLCommandBufferError.notPermitted</code> 0、bounded deactivation latency。
- model破損、symlink、wrong role、signature/hash/size不一致。
- app switcher privacy shield。
- prompt/tokenがOS log/crash dataにない。
- sentinel prompt Aの秘密がrequest/mode Bへ出ないことをnormal/cancel/error/background各経路で検証。
- CPU simulator artifactをdevice archiveへ混入させない。

### 12.6 UI / accessibility

- notch/Dynamic Island/home indicator全safe area。
- portrait/landscape、small/large iPhone、iPadOS compatibility-mode windowの全許可orientation/scale。
- software keyboard、hardware keyboard、dictation。
- VoiceOver、Dynamic Type、Bold Text、Increase Contrast。
- UIContentSizeCategoryの起動時/runtime変更、最大accessibility category、横clip 0。
- Reduce Motion。
- 44×44 target、hover依存0。
- tablist manual activation/ARIA。
- text input/scrollがarchive export/import、index rebuild、inference中も利用可能。
- BIZ UDGothic Regular/Boldの日本語/Latin/full-width representative corpus、WKWebView/UIKit/AppKit同一family/metrics、font fallback 0、line-height 1.6、最大Dynamic Type横clip 0、network font request 0。
- Haptic Grammar全tokenのsemantic transition exactly-once、visual/ARIA併用、device/system haptics off、hardwareなし、background。notification/vibrate/multi-pulse permission/call 0。
- Native Ceremony Annexの全routeで<code>#0a0a0a</code>/<code>#f2f2f2</code>写像、R=G=B、radius/blur/system-blue/stock alert 0、BIZ UDGothic、VoiceOver、44pt。OS所有Face ID/permission/Files sheetはtrusted exceptionとして前後surfaceだけをvisual compareし、私製biometric模倣0。
- <code>platformEffects</code>のstateless/import/grep/re-render/observer cleanup gate。componentからVisualViewport/Tauri/plugin/native API直接到達0。
- iPad compatibility modeでFiles、EventKit、Keychain/Data Protection、backup exclusion、archive export/import、user-file importがiPhoneと実質同等。
- 44×44が透明hit-area拡張で満たされ、視覚密度（1px罫・台帳行高・タブ寸法）がdesktop規定から肥大していない。
- OLED実機（notch/Dynamic Island世代）でblack smearの許容確認、静的1px白罫/inverse video長時間表示の焼き付き傾向確認、<code>#000</code>基底とsafe-area外周の連続性確認。
- Launch Screenとapp switcher privacy shieldがE8規定（黒地＋静的グリフ、blur/spinner/有彩色0）に一致し、shieldが本文を完全被覆する。
- monochrome/radius/border/glow/native-token parityのvisual regression。

## 13. Threat modelと限界

守る対象:

- 紛失・盗難端末上のat-rest data（SQLCipher＋Data Protection＋ThisDeviceOnly key）。
- malicious Files provider/外部mediaによる<code>.pkbvault</code>・user-file importの読取・改変・差替え・TOCTOU。
- replay、malformed/oversize archive/import file、旧headへのregression。
- DB/journal改竄とDB-only snapshot rollback（signed chain＋Keychain anchor照合）。
- WebView compromiseからの任意network/file/provider/path/model操作、archive bytes取得と、native user-presenceを経ないexport/recovery操作。
- log、snapshot、backupによる漏洩。

1.0で守れないもの:

- **単一端末集中。** 最新export以降のデータは端末喪失と同時に失われる。appは鮮度表示とreminderまでを担い、exportの実行と暗号化archiveの保管はユーザーの責任である。
- ユーザーが選んだ保管先（iCloud Drive/third-party provider/外部media）に置かれた暗号化archiveの保持・削除・複製。
- unlock済み端末またはOS自体が侵害された状態。
- 画面を物理カメラで撮影されること。
- 端末紛失後のremote wipe。
- logical delete後の暗号化journal historyからのphysical erasure。
- DBとKeychain anchorを整合して同じ旧snapshotへ同時に戻すcoherent rollbackの検出。外部witness/hardware monotonic counterを持たない1.0では保証しない。
- 全archive・Keychain anchor喪失後、古いが正当に署名された<code>.pkbvault</code>が最新版かをfresh disaster restoreだけで判定すること。
- compromised WebViewがallowlisted export/import ceremonyの開始要求を繰返してavailabilityを害すること。native routeはverified scopeを再表示し明示confirmなしにartifactを外へ渡さないため、開始要求だけをhuman intentやexfil authorizationにしない。

「zero trust」はOS provider、archive、import file、自DBのjournal chainを常に検証し、最小権限・署名event・native user presenceを使うという意味であり、ユーザーの保管先から暗号文を消せるという意味ではない。

Standalone構成ではnetwork capabilityがappに存在しないため、「embedded PythonをOS-levelでnetworkから隔離できない」という旧残余リスクは主要threatから消滅した。iOSのPython import/native API closureは攻撃面を縮小するdefense in depthとして維持するが、隔離境界の証明には使わない。

## 14. リスク順位と停止条件

| Risk | 影響 | 早期検証 | 停止/分岐 |
|---|---|---|---|
| CPython 3.13/NumPy iOS build | 全business core | M2実機spike | 失敗時は規律変更判断へ戻る |
| app-owned socket混入drift | 絶対孤立の毀損 | symbol/packet/permission三面scanのCI恒久化 | 1件でも検出したらrelease停止 |
| 384D embedding provider drift | retrieval/parity | M2/M4 corpus＋top-k | silent fallback禁止、M4停止 |
| canonical store初期化/user-file import | 全データ | import dry-run＋receipt検証 | silent repair禁止。loss/ambiguityは固定errorで停止 |
| Pocket Brain品質（感情/JSON rubric未達） | 製品価値の核 | M0一次選抜＋M4実機rubric | 全candidate未達なら1.0停止・scope再承認。silent出荷禁止 |
| jetsam/dirty footprint | 安定性 | M0一次実測＋M4 <code>phys_footprint</code> acceptance | 最低対応端末で再現可能jetsam 1件でもrelease不可 |
| eligibility裁定（Option Aの市場縮小） | 事業判断 | M0でCommander署名 | 裁定なしにM4へ進まない |
| journal/anchor rollback設計 | data integrity | hash-chain/rollback/crash tests | fork/anchor不整合でrelease停止、coherent rollbackは非保証を明記 |
| recovery archive defect | 復旧経路の喪失 | M6 tamper/crash/restore drill | restore drill不合格ならrelease停止。「復旧可能」を先に訴求しない |
| SQLCipher/license/export | release不可 | M0 legal/SBOM | 解決までarchive/upload禁止 |
| BIZ UDGothic/WKWebView resolution・OFL | 美学/法務 | M1 physical-device font proof＋license manifest | fallback、hash/license欠落ならG0-Eへ戻りM1停止。<code>@font-face</code>無断追加禁止 |
| stateful <code>platformEffects</code> / raw API drift | pure-function憲法 | import/grep/effect determinism gate | direct access/state/cache/timerが1件でもあればM5再freeze不可 |
| UI.D freeze/Annex未承認 | 規律違反 | G0-E＋Freeze Transition ACK | exact thaw外を変更せず、再freeze ACKまで次milestone停止 |
| App Store 2.5.2/4.2 | rejection | early review dossier | model/runtime説明・native valueを再設計 |
| iOS lifecycle/jetsam | data loss/crash | physical device matrix | supported device/paramsを狭める |
| desktop regression | 現行要塞を毀損 | backend compile-time isolation | desktop suite不合格ならmerge禁止 |

## 15. Definition of Done

App Store 1.0は<code>Standalone</code>単一profileであり、15.1（共通）＋15.2（絶対孤立）＋15.3（recovery）をすべて満たした時だけ「完了」とする。パージ済み同期機構にはDoDを与えず、出荷候補にしない。

### 15.1 共通DoD

- 現行7 tabと全release対象commandがiOSで意味論一致。
- Pythonがbusiness/prompt/data ruleのsource of truthであり続ける。
- llama/searchがin-processで、子プロセス・localhost serverがない。
- active 384D embedderが署名済みで、desktop/iOSのquality/parity gateを通る。
- G0-C（Pocket Brain）を通過したproduction GGUFをちょうど1件bundleし、signed model policyを外れたmodelを実行しない。
- Pocket Brain rubric（日本語感情分析・家計簿JSON抽出）の一致率gateと、GBNF grammar拘束decodingのschema妥当率100%を実機で通る。
- dirty footprintが最低対応端末のper-app上限の60%以下で、再現可能jetsamが0。
- canonical dataがSQLCipher＋Data Protectionで暗号化され、plaintext WAL/tempがない。
- 全canonical eventがDeviceSigner reservationを通り、Python-only DB commit、signature再検証、two-slot anchor、arbitrary-sign拒否のcrash matrixを満たす。
- canonical DBを含む全PKB管理fileがOS/iCloud Backup対象外で、import receipt、M6 recovery/rollback drillが通る。
- M6 encrypted archiveのtamper/crash recovery gateが通る。
- recovery export/import ceremonyと保証対象rollback（DB-only rollback検出）がnative user-presence/tamper/crash testを通る。
- PKB-managed iCloud/CloudKit sync、CDN、telemetry、remote DBがない。
- safe area、keyboard、touch、VoiceOver、Dynamic Type、reduced motionが実機で通る。
- BIZ UDGothic Regular/Boldがexact hash/OFL/<code>UIAppFonts</code>でbundleされ、WKWebView/UIKit/AppKit/iPad compatibility glyph/metric gateをfallback 0で通る。
- pure reducer→closed Effect→stateless <code>platformEffects</code>→single <code>PlatformPort</code>→strict Eventの境界が決定論/import/grep/listener cleanup gateを通り、component direct platform APIとstateful runnerが0。
- Haptic Grammarのselection/impactだけがsemantic transitionでexactly-once発火し、notification/vibrate/multi-pulse 0。Native Ceremony Annexのtoken/a11y/OS trusted-exception testが全security routeで通る。
- archiveの<code>UIDeviceFamily=[1]</code>、iPhone＋iPad compatibility supported-device list/matrix、Apple silicon Mac/Vision availability offが提出recordと一致する。
- monochrome Silent Terminal美学をWebViewとapp-owned native surfaceの両方で維持する。
- G0-E成果物とFreeze Transition ADRが承認され、全milestoneのexact thaw diff、exit hash、再freeze ACKが完了する。
- privacy manifest、privacy policy、export compliance、SBOM、signingが承認済み。
- TestFlight acceptance後、App Review向けの基本操作がaccount/cloudなしで確認でき、local LLMもproduction bundle modelで再現可能。
- desktop release regressionが0。

### 15.2 絶対孤立DoD

- <code>egress-live</code>がcompile offで、socket/listener/connect/DNS、Network.framework/Bonjour/LNP、camera、network entitlement、network XPC、delta/QR/wired残滓がarchiveに0。parity commandはinert fixed refusalだけ。
- runtime port scan、全interface packet capture、Mach-O/native symbol/capability/permission scanでapp-owned network byte/success pathが0。
- 機内モードで全機能（記録、consult、推論、import、archive export/import）が完走する。
- App Store説明・privacy回答が「完全offline・standalone」を正確に表現し、いかなる同期も訴求しない。

### 15.3 Recovery DoD

- <code>.pkbvault</code> export/import、PendingVault、新actor epoch、tamper/rollback/crash matrixが実機で通る。
- 単一端末集中リスク（export鮮度）がUI/privacy policy/Review Notesへ明示される。
- export鮮度ambient statusとreminderが機能し、auto-export・background export・cloud escrowが存在しない。

## 16. 明示的な非目標

- Android同時対応。
- native iPad universal target/layout最適化（iPadOS compatibility modeの同等機能・試験は必須）。
- React Native、Flutter、Electronへの再プラットフォーム。
- Tailwind、UI kit、外部state manager。
- PKB-managed cloud/OS backup・sync、web account、subscription backend。復旧はM6 manual encrypted archiveだけ。
- あらゆる同期・データ移送機構: LAN Sync、Bonjour/mDNS、USB-mux/PeerTalk有線、QR受領証、<code>.pkbdelta</code>、share sheet移送、multi-device運用、Mac companion連携。再導入はD3の条件だけ。
- desktop⇄iOS間のデータ移行・共有（desktop製品は独立に継続する）。
- camera permission、AVFoundation capture。
- background常時sync、push wakeup、provider polling、background listener、auto-export。
- remote/web font、runtime font download、On-Demand Resources font、未承認<code>@font-face</code>、BIZ UDGothicの無断subsetting/rename。
- Face ID/Touch ID/passcode/OS permission sheetの私製模倣・overlay・re-skin、app-owned stock alert/rounded ceremony。
- notification/vibrate/multi-pulse haptic、raw haptic styleをcomponentから指定すること。
- state/cache/queue/timer/retry/subscriptionを所有する<code>platformEffects</code>、componentからVisualViewport/Tauri/plugin/native APIへ直接到達すること。
- arbitrary GGUF/plugin/script実行。
- On-Demand Resources/Background Assetsによるmodel自動配布。
- remote LLM fallback。
- DB file/WALの直接複製。
- 全entityへの汎用CRDT導入。
- inference結果を理由にUI全体をbusy lockすること。
- 色彩・角丸・modalを使った「モバイルらしさ」の追加。

## 17. 主要一次資料

- [Tauri v1 to v2 migration](https://v2.tauri.app/start/migrate/from-tauri-1/)
- [Tauri iOS prerequisites](https://v2.tauri.app/start/prerequisites/)
- [Tauri CLI](https://v2.tauri.app/reference/cli/)
- [Tauri mobile development](https://v2.tauri.app/develop/)
- [Tauri platform configuration files](https://v2.tauri.app/develop/configuration-files/)
- [Tauri mobile plugin development](https://v2.tauri.app/develop/plugins/develop-mobile/)
- [Tauri configuration](https://v2.tauri.app/reference/config/)
- [Tauri capabilities](https://v2.tauri.app/security/capabilities/)
- [Tauri shell plugin](https://v2.tauri.app/plugin/shell/)
- [Tauri filesystem plugin](https://v2.tauri.app/plugin/file-system/)
- [Tauri haptics plugin](https://v2.tauri.app/plugin/haptics/)
- [Tauri SQL plugin](https://v2.tauri.app/plugin/sql/)
- [Tauri Stronghold plugin](https://v2.tauri.app/plugin/stronghold/)
- [Tauri App Store distribution](https://v2.tauri.app/distribute/app-store/)
- [CPython: Using Python on iOS](https://docs.python.org/3.13/using/ios.html)
- [PEP 730: Adding iOS as a supported platform](https://peps.python.org/pep-0730/)
- [llama.cpp iPhone/XCFramework sample](https://github.com/ggml-org/llama.cpp/blob/master/examples/llama.swiftui/README.md)
- [Apple App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/)
- [Apple UIAppFonts](https://developer.apple.com/documentation/BundleResources/Information-Property-List/UIAppFonts)
- [Apple: Adding a custom font to your app](https://developer.apple.com/documentation/uikit/adding-a-custom-font-to-your-app)
- [Morisawa / Google Fonts BIZ UDGothic and OFL 1.1](https://github.com/googlefonts/morisawa-biz-ud-gothic)
- [Apple UIDocumentPickerViewController](https://developer.apple.com/documentation/uikit/uidocumentpickerviewcontroller)
- [Apple UIImpactFeedbackGenerator](https://developer.apple.com/documentation/uikit/uiimpactfeedbackgenerator)
- [Apple Local Authentication Embedded UI](https://developer.apple.com/documentation/localauthenticationembeddedui)
- [Apple UIRequiredDeviceCapabilities](https://developer.apple.com/documentation/bundleresources/information-property-list/uirequireddevicecapabilities)
- [Apple os_proc_available_memory](https://developer.apple.com/documentation/os/os_proc_available_memory)
- [Apple: Reducing your app's memory use](https://developer.apple.com/documentation/xcode/reducing-your-app-s-memory-use)
- [llama.cpp GBNF grammars](https://github.com/ggml-org/llama.cpp/blob/master/grammars/README.md)
- [Apple Keychain](https://developer.apple.com/documentation/security/storing-keys-in-the-keychain)
- [Apple Secure Enclave](https://developer.apple.com/documentation/cryptokit/secureenclave)
- [Apple Privacy manifests](https://developer.apple.com/documentation/bundleresources/adding-a-privacy-manifest-to-your-app-or-third-party-sdk)
- [Apple third-party SDK requirements](https://developer.apple.com/support/third-party-SDK-requirements/)
- [Apple App Privacy management](https://developer.apple.com/help/app-store-connect/manage-app-information/manage-app-privacy/)
- [Apple Export compliance](https://developer.apple.com/help/app-store-connect/manage-app-information/overview-of-export-compliance/)
- [Apple maximum build sizes](https://developer.apple.com/help/app-store-connect/reference/app-uploads/maximum-build-file-sizes/)
- [Apple current submission SDK requirements](https://developer.apple.com/jp/app-store/submitting/)
- [WebKit safe-area guidance](https://webkit.org/blog/7929/designing-websites-for-iphone-x/)
- [SQLCipher design](https://www.zetetic.net/sqlcipher/design/)
- [SQLite Session Extension](https://www.sqlite.org/sessionintro.html)

---

この文書は規律変更そのものではない。G0の承認、絶対孤立ADR、Freeze Transition ADR、Native Ceremony Annex、Haptic Grammar、font/license manifest、Pocket Brain選定ADR、原文更新、RED constitutional test、UI ACKが揃うまで、embedded runtime、canonical store、iOS model role、recovery archive、mobile UI deltaは提案状態に留まる。パージ済み同期機構（LAN/USB-mux/QR/<code>.pkbdelta</code>）はD3の再導入条件なしに、いかなる形でも再着手しない。
