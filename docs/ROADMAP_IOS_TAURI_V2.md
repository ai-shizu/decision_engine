# PKB iOS / App Store モバイル化・全体ロードマップ

- Status: Master roadmap（正式版）— Fable監査 全delta統合済み（F-A1〜A6 / F-P1〜P4 / T1・T2 / LAN降格）
- Date: 2026-07-17
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
6. **同期はSQLiteファイルコピーではなく、暗号化された正本DB＋署名付きoperation logとする。** raw/processedファイル群を直接同期しない。正本データだけを同期し、ベクトル・tensor・index・cacheは各端末で再生成する。
7. **主力同期は「無ポート・OS媒介」に固定する。** 正本DB/WALではなく、recipient-boundに暗号化した署名付きoplog差分<code>.pkbdelta</code>をexportし、ユーザーがAirDrop/Files等のOS所有チャネルで移送する。import commit後の受領証は数十byteのopaque QR Ackで返す。baseline appはbrowse/listen/connectせず、Bonjour、mDNS、Local Network permission、app-owned network portを持たない。
8. **OS/iCloud Backupも復旧経路にしない。** ThisDeviceOnly keyとの不整合を避けるため全PKB管理fileをbackup除外し、復旧はユーザー操作の署名・暗号化<code>.pkbvault</code>だけに固定する。
9. **LAN Syncはrelease主力から破棄する。** mDNS/SRV/端末名等の残余metadataを「沈黙する端末」と両立できないため、旧LAN案は非出荷のcontingency ADRへ降格する。即時baselineは<code>.pkbdelta</code>＋QR Ack、最終理想形の候補はM0で成立性をSpikeするUSB-mux/PeerTalk＋<code>127.0.0.1</code>限定の有線sessionである。
10. **美学はbundle artifactまで含む。** BIZ UDGothic Regular/BoldをOFL 1.1 noticeとともに物理同梱し、<code>UIAppFonts</code>へ固定する。app-owned UIKit/AppKit ceremonyは<code>#0a0a0a</code>/<code>#f2f2f2</code>、radius 0、限定inverse videoへ写像し、触覚は視覚的音量と同じsemantic grammarだけから発火する。

3名の専任体制とsecurity/App Store法務reviewを前提に、App Store 1.0の<code>Silent Transfer</code> baselineまで**50〜76週**、G0-B'を通過した別profile/versionの<code>Wired-enhanced</code>まで**58〜88週**（各25〜35%のcontingency込み）を計画帯とする。1名体制では100〜140週以上を見込む。baselineのTestFlight判定は2027年第2〜第4四半期、App Store提出判定は2027年第4四半期〜2028年第1四半期を目安とするが、M0 USB SpikeとM2/M3後に必ず再見積りし、日付より各exit gateを優先する。Wired-enhancedはbaseline提出をblockしない。

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

### 1.3 「暗号化SQLite同期」は既存DBの同期機能ではない

現行正本は単一SQLiteではない。<code>diary.md</code>、LINE履歴、calendar/finance/consultation JSON、probe/interview/knowledge、独自binary/mmap、各種processed artifactへ分散している。<code>sqlite3</code>はmacOS Calendar DBのread-only importにしか使われていない。

したがって必要なのは、次の順序である。

1. 正本データと派生データの分類。
2. Python側repository境界の導入。
3. SQLCipher正本ストアと厳密schemaの新設。
4. 既存ファイルからの検証付き一回移行。
5. operation logの生成。
6. その後にP2P同期。

DB/WALファイルの丸ごと転送は、双方向mergeにも破損耐性にもならないため禁止する。

### 1.4 app-owned transportと小型モデルは現行規律に衝突する

現行規律は本番TCP/IPをloopbackを含め禁止し、Pythonのnetwork importとRust raw socketも恒久テストで拒否している。したがって「LANだから既存規律内」と解釈してはならない。

文書driftもM0で解消する。<code>.cursorrules:19</code>には古いlocalhost llama HTTP許可が残る一方、より新しい<code>docs/AI_SKILLS.md:52-57</code>はloopbackを含むTCP/IPを禁止し、private prompt channelだけを許している。本計画は後者と現行実装を正とする。ゆえにApp Store 1.0 baselineはTCP/IP例外を追加せず、<code>.pkbdelta</code>をOS所有のshare/document-picker境界へ手渡すだけとする。AirDropが無線を使うか、Files providerがどこへ保存するかはOSとユーザーの選択であり、PKB自身がpeer探索・port listen・宛先接続を行うこととは区別する。

M0のUSB-mux Spikeだけは、iOS native ownerが<code>127.0.0.1</code>へ短命listenし、Mac側usbmuxが物理cable越しにforwardするG0-B'候補を調べる。これは既存規律内だと偽装せず、App Store feasibility、public/private API、sandbox、license、radio-zero evidenceを揃えた場合だけ別の明示例外として審議する。旧Bonjour/LAN案は実装・bundle・提出対象にしない。

また、現行model policyは原則7B・3.5GB以上、小型モデル禁止である。一方、iOS/iPadOSアプリの非圧縮build上限は4GBであり、7B量子化モデル、WebView、Python、NumPy、Metal、DBを同梱する前提は危険である。[Apple build size limits](https://developer.apple.com/help/app-store-connect/reference/app-uploads/maximum-build-file-sizes/)

この二点は実装上の小さな設定変更ではなく、正式な規律変更判断である。

## 2. G0: 実装開始前の承認ゲート

以下が署名・承認されるまで、該当実装へ入らない。

### G0-A: モバイル実行方式

**推奨承認案:** Pythonがorchestration/data/prompt/business ruleを所有し続け、iOSではembedded CPythonとして同一プロセス実行する。RustはTauri境界、artifact認証、native lifecycle、LLM/search FFIを所有する。C++は数値カーネルだけを所有する。

これは現行production IPC/sidecar/artifact規律のplatform-scoped改定である。承認時に<code>.cursorrules</code>、<code>docs/AI_SKILLS.md</code>、constitutional testsへ次を同時反映する。

- desktopのstdio sidecar process、private prompt channel、search daemon→1-shot→NumPy fallback、artifact inventoryは維持する。canonical signed storeを有効化するMac buildだけ、C3.2のfixed internal DeviceSigner reverse frameを同じprivate stdio transportへ追加し、external Tauri contractへ露出しない。
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

<code>Silent Transfer</code> baselineにはapp-owned network stackがないため、旧LAN案の「embedded CPythonとnetwork pluginが同一process」というhard conflictを主経路から除去できる。ただしM6-Uの有線候補を有効にすると、embedded CPythonと<code>127.0.0.1</code> listenerが同一iOS app processへ入る。OS entitlement/capabilityでPythonだけからloopback能力を剥がせないため、これは**G0-A/B' joint hard conflict**として別途明示受容する。Python audit hookやmodule削除をOS sandboxと呼んではならない。

baselineと有線候補の双方でattack surface closureとして次を要求し、有線候補ではこれに加えてG0-B'のloopback例外を審査する。

- iOS Python bundleから<code>_socket</code>/<code>socket</code>/DNS/SSL client、<code>ctypes</code>/cffi、user-controlled arbitrary <code>dlopen</code>、subprocess/multiprocessing、writable import pathを除外。唯一のbinary-extension load経路はread-only app bundle内のmanifest列挙済み・署名済みAppleFrameworkLoader frameworkとする。
- built-in/dynamic extension、stdlib、frameworkをallowlist化し、unknown moduleをhard fail。
- isolated <code>PyConfig</code>、environment無視、read-only <code>sys.path</code>、bytecode write無効。
- <code>_pkb_native</code>はexact operationだけで、raw pointer/selector/URL/host/pathを受けない。
- Cargo/native linked symbol、Python import closure、archive framework、WebView command setをrelease testで固定。
- Python audit hook/offline runtimeはdefense in depthとして残すが、隔離境界の証明には使わない。

この例外を受容できない、またはM0 Spikeが成立しない場合は**M6-UだけをNo-Go**とする。<code>.pkbdelta</code>＋QR Ackのbaseline、embedded Python、manual recoveryは影響を受けない。

### G0-B: Metadata Silence / transport裁定

**T1 baselineを即時採用する。** 同期の主経路はrecipient-boundに暗号化した署名付きoplog package <code>.pkbdelta</code>のexport/importと、import commit後に表示するopaque QR Ackである。転送はユーザーがAirDrop、Files document provider、外部media等のOS所有surfaceから選ぶ。PKBはpeer discovery、advertise、browse、listen、connect、DNS、mDNS、HTTP、WebSocketを一切行わない。

baselineのrelease invariant:

- <code>egress-live</code>はoff、旧<code>lan-sync</code>はoffかつcode/capability/permission/artifact 0、<code>wired-sync</code>もoff。
- <code>NSLocalNetworkUsageDescription</code>、<code>NSBonjourServices</code>、network client/server entitlement、Network.framework/Bonjour owner、network XPC agent、listener/browserをbundleしない。
- WebViewはpackage bytes、filesystem path、share target、recipient public key、Ack secretを取得しない。開始要求だけを送り、recipient alias/scopeを再表示するapp-owned native ceremonyとユーザーの明示confirm後にnative exporterがOS share/document pickerを提示する。
- 外部から見えるfilenameはCSPRNGによるsession-scoped opaque値、clear headerはmagic/version、random package tag、KEM/AEADに必要なrandom-looking material、padded ciphertext lengthだけとし、vault/actor/device/user/record名、head、count、wall timeを出さない。
- packageは宛先を誤って選んでも指定peer以外が復号できない。OS/providerには転送時刻、padded size、送受信の事実が見える可能性があり、AirDrop自体のdiscovery/device metadataやradio利用をPKBの「metadata 0」保証へ含めない。絶対radio silenceが必要な利用者は有線候補が承認されるまでAirDropを選ばない。
- QR Ackはcanonical dataやstable IDを含まず、version 1 byte＋single-use <code>ack_handle</code> 16 byte＋recipient actorのP-256 raw signature 64 byte、計81 byteのfixed binaryとする。camera frame、decoded bytes、signatureをlog/Photo Libraryへ保存しない。

M0でrelease artifactを次のいずれかへ固定する。Wired-enhancedを選ぶ場合もSilent Transfer機能は残し、有線が失敗した時にruntimeでLANへfallbackしない。

| Profile | Compile / entitlement | 必須milestone | 提出時の機能 |
|---|---|---|---|
| <code>Silent Transfer</code>（1.0 baseline） | <code>egress-live</code>/<code>lan-sync</code>/<code>wired-sync</code> off、app-owned network surface 0 | M6-T＋M6-R | <code>.pkbdelta</code>＋QR Ack、<code>.pkbvault</code>、manual enrollment/admin transfer |
| <code>Wired-enhanced</code>（条件付き別profile/version） | <code>egress-live</code>/<code>lan-sync</code> off、G0-B'承認済み<code>wired-sync</code>だけon | M6-T＋M6-U＋M6-R | baseline一式＋明示foreground USB session |

profile値はCargo/native feature allowlist、artifact manifest、capability、Info.plist、privacy、Review Notes、acceptance reportへ同じ値で記録する。IPA/Mach-O、Rust feature inventory、Tauri permission、native symbol/importをscanし、許可profile外のtransport owner/success pathをrelease gateで拒否する。

**G0-B' — USB-mux / PeerTalk例外候補:** M0でtime-boxed Spikeを行い、次を全部満たすevidenceが出た場合だけ<code>.cursorrules</code>、<code>AI_SKILLS.md</code>、constitutional testsへ有線限定例外を提案する。

1. iOS側はexplicit foreground session中のnative ownerだけがnumeric IPv4 <code>127.0.0.1</code>へbindする。<code>localhost</code>、<code>0.0.0.0</code>、<code>::</code>/<code>::1</code>、任意port入力、DNS、Bonjourを使わない。background/cable detach/timeoutで即closeする。
2. Mac側はpinned sourceのPeerTalk/usbmux adapterを狭い<code>pkb-wired-agent</code>へ隔離し、接続種別がUSBであることをfail-closedに検証する。usbmuxのnetwork接続、Wi-Fi pairing、任意device/port、fallback transportを拒否する。
3. Wi-Fi/Bluetooth/cellularをoffにしたminimum/latest iPhone、mandatory iPad compatibility mode、arm64/x86_64 Macで双方向転送でき、全radio interface packet captureとapp-owned non-loopback byteが0である。
4. cable/USB topologyはtransport locationの証拠に使えるがpeer identityの代わりにしない。既存のactor/membership signature、recipient-bound encryption、native user confirmationをそのまま要求する。
5. pinned PeerTalk commit、license、SBOM、memory safety audit、modern OS/device support、Mac sandbox/notarization、iOS App Store archive/private-symbol scanを通す。undocumented/private iOS API、承認不能なtemporary sandbox exception、jailbreak/device-management前提が必要ならNo-Goとする。PeerTalkの過去採用例は現在のApp Review適合性の証明にしない。
6. wired transportはD章と同じsealed <code>.pkbdelta</code>/Ack state machineをbounded frameへ載せるだけとし、第2のsync semantics、raw DB転送、平文frameを作らない。
7. iOS同一processのloopback能力をPythonだけからOS-levelに剥がせない残余リスクをG0-A/B'で明示受容する。受容できなければSpike成功でも出荷しない。

旧LAN/Bonjour案は**非出荷backup ADR**へ降格する。1.0/conditional wired profileのどちらにも<code>lan-sync</code>、Bonjour、LNP、network XPCを含めない。将来復活させるには新たなCommander裁定、metadata threat model、G0-B再承認、別binary/versionのApp Reviewが必要であり、M6-Uのfallbackとして自動採用しない。

desktop contract parityのため既存egress系Tauri command surfaceが必要な場合、command自体はexact schemaの**inert fixed-refusal adapter**として残す。<code>get_runtime_capabilities.egress_live=false</code>を返し、UI actionを提示せず、呼ばれても現行contractどおりの固定<code>EGRESS_LIVE_NOT_READY</code>だけを返す。host/path/payloadを解釈せず、native transportへ到達せず、Tauri capability/scopeでlive permissionを与えない。「commandが存在する」ことと「egress能力が存在する」ことを混同しない。

### G0-C: iOSモデルロール

最初に現行7Bを実機測定する。その結果により次のいずれかを正式決定する。

1. **7B維持:** archive実寸、peak memory、thermal、tokens/s、品質を通す。App Storeは任意のRAM帯をinstall eligibilityにできないため、Appleが正当に表現できる<code>UIRequiredDeviceCapabilities</code>/minimum OSで定まる全install対象iPhoneとiPadOS compatibility-mode端末でLLMを安全に動かす。RAM機種名の非公開allowlistや、eligible device上のsilent LLM-unavailableで逃げない。[UIRequiredDeviceCapabilities](https://developer.apple.com/documentation/bundleresources/information-property-list/uirequireddevicecapabilities)
2. **推奨:** 署名対象の<code>model_params.json</code>へ固定の<code>ios</code> roleを追加し、2〜4B候補を7B基準と比較する。日本語、PKB retrieval、gap safety、prompt injection、誤答率の閾値を満たしたモデルだけ許可する。
3. **No-Go:** 7Bも承認済み2〜4B roleもpackage/memory/thermal/quality gateを満たさない場合、「local LLM統合済みApp Store 1.0」は停止し、scope/G0-Cを再承認する。未検証小型modelやmodelなしbuildへ自動fallbackしない。

<code>PKB_ALLOW_SMALL_LLM=1</code>を製品上の逃げ道にしない。

App Store 1.0にはG0-Cを通過したproduction GGUFを**1件以上必ずbundle**し、mobile artifact inventory/constitutional testでrole/hash/signature/license/sizeを固定する。import-only/modelなしbuildは1.0候補にしない。optional manual model importを残す場合も、bundle model inventoryの代替にはしない。

### G0-D: 暗号・ストア・App Store

- SQLCipher edition/license、SBOM、再現buildを承認する。
- bundled GGUFのredistribution licenseと、CPython/NumPy/llama.cpp/embedding runtime/model/SQLCipherのlicense notice・attributionをartifact単位で承認する。
- DB key、device identity、delta recipient/enrollment key、任意のwired session key、contact identity/state-chainの関係をADR化する。
- 輸出コンプライアンス判断のownerを決める。
- <code>ITSAppUsesNonExemptEncryption=false</code>を推測で設定しない。
- PKB-managed iCloud sync/CloudKitだけでなく、**canonical DBを含むPKB管理fileをOS/iCloud Backupから除外する**。ThisDeviceOnlyのDB keyだけが移行せず暗号化DBだけが復元される破綻を許さない。復旧経路はM6-Rの暗号化manual archiveだけとし、Appleの[backup storage guidance](https://developer.apple.com/documentation/foundation/optimizing-your-app-s-data-for-icloud-backup)、[ThisDeviceOnly Keychain semantics](https://developer.apple.com/documentation/security/ksecattraccessiblewhenunlockedthisdeviceonly)、端末紛失時のdata-loss告知、App Review説明を承認する。

### G0-E: UI freeze解除

<code>docs/SPEC_UI_MONOCHROME_CYBERPUNK.md:248-255,302-312</code>はUI.D後の対象fileを1-byte freezeし、ACKまでHARD STOPとしている。mobile対応は<code>index.html</code>、React/CSS、Tauri設定へ触れるため、実装前に次を満たす。

- UI.D完了ACKが存在することを確認する。存在しなければACKを待つ。
- mobile/platform-security delta specを別途承認し、変更許可file、safe-area/tab/titlebar copy、既存token再利用、iOS UIKit・macOS AppKitのWebView外security ceremony、visual acceptanceを固定する。
- **F-A1 typography delta:** BIZ UDGothic Regular/Boldのunmodified TTFを公式sourceのexact tag/commit＋SHA-256で固定し、app bundleへ物理同梱する。iOS <code>UIAppFonts</code>へ拡張子を含むbundle-relative filenameを列挙し、OFL 1.1全文、copyright、source/version/hashをapp内legal viewと配布noticeへ含める。runtime download、CDN、On-Demand Resources、system-installed font依存を禁止する。[Apple UIAppFonts](https://developer.apple.com/documentation/BundleResources/Information-Property-List/UIAppFonts) / [BIZ UDGothic source and OFL](https://github.com/googlefonts/morisawa-biz-ud-gothic)
- 現行UI SPECの<code>@font-face</code>禁止は維持する。CSSは登録済みPostScript/family nameだけを既存font stackから参照する。WKWebViewが<code>UIAppFonts</code>登録fontを実機で解決できない場合、local <code>@font-face</code>を黙って追加せずG0-Eへ差し戻し、Fableの明示deltaを待つ。
- **F-A2 Native Ceremony Design Annex:** app-owned UIKit/AppKit routeをCSS tokenのnative写像で固定し、汎用system-default card/button/alertの寄せ集めを禁止する。OS所有のFace ID/Touch ID、permission、share sheet、Files pickerは偽装・再描画・私製biometric prompt化せずtrusted system exceptionとし、その直前・直後を承認済みnative ceremony frameで包む。
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
| M6-T | <code>.pkbdelta</code>/Ack protocol、new pure transfer reducer/effects、native share/Files/QR ceremony、local peer-cursor repository | record/business/prompt/LLM semantics、既存UI surface、DB/WAL直接経路 | package/fuzz/crash/metadata tests、protocol V1/hash freeze、M6-T ACK |
| M6-U | G0-B'承認済みnative wired adapter、fixed frame/parser、artifact feature manifestのみ | sync semantics、canonical schema、WebView、Python network surface、UI copy | USB-only/radio-zero/private-symbol/sandbox evidence、wired ABI freeze、M6-U ACK |
| M6-R | archive/enrollment/admin-transfer protocolと、そのexact native ceremony/pure effect surfaceだけ | 通常record/prompt/LLM/UI、M6-T protocol | restore/transfer/tamper/crash matrix、package version freeze、M6-R ACK |
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
- exactly-once effect消費はreducerが発行するeffect IDとstate transitionで決める。re-render、duplicate event、cancel/retryでhaptic、share、camera、native ceremonyを再発火させない。
- component、<code>platformEffects</code>、任意domain moduleから<code>visualViewport</code>、<code>matchMedia</code>、CSS <code>style.setProperty</code>、Tauri <code>invoke/listen/getCurrentWindow</code>、plugin APIへ直接到達することをgrep＋import-boundary testで拒否し、exact allowlistの<code>PlatformPort</code>実装だけに許す。
- modal、<code>alert</code>、<code>confirm</code>を使わない。
- inference、index rebuild、transfer/importはambientに表示し、textarea、scroll、他機能を封鎖しない。
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
- production CSPの<code>connect-src</code>をtransferのために開けない。
- App Store updateはOS/App Storeの責務とし、アプリ自身にupdaterを持たせない。
- canonical DB、model、derived、cacheを含むPKB管理fileはOS/iCloud Backupから除外する。日常差分移送はM6-Tのrecipient-bound <code>.pkbdelta</code>、復旧はM6-Rの署名・暗号化<code>.pkbvault</code>だけを使う。
- Files document pickerでユーザーが自らiCloud Drive providerを選ぶことは、PKB-managed iCloud syncとは区別する。system providerを不当に隠さない。
- AirDrop/share sheet/FilesはOS所有surfaceであり、PKBが相手をdiscover・connectする経路ではない。選択providerがradio/cloudを使う可能性とmetadata限界をユーザーへ示すが、任意share extensionへ渡ってもplaintextにならないrecipient-bound envelopeを必須とする。
- G0-B'未承認のbaselineにloopbackを含むsocket ownerを入れない。承認済みWired-enhancedだけがexact <code>127.0.0.1</code> listenerをcompileでき、LAN/WAN/DNS/mDNS fallbackを持たない。

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
    PY --> VAULT["SQLCipher vault.sqlite\ncanonical tables + signed oplog"]
    MOBILE --> IOS["Narrow native services\nKeychain / lifecycle / Files-share / QR / EventKit / haptics"]
    PY --> DELTA["Signed sealed .pkbdelta\nbounded oplog package"]
    DELTA --> SHARE["OS-owned share sheet / Files picker\nno app listener or peer discovery"]
    SHARE <-. "user-selected AirDrop / Files / media" .-> MAC["Enrolled Mac peer"]
    MOBILE --> ACK["Native QR Ack ceremony\nopaque one-time receipt"]
    MOBILE -. "G0-B' + wired-sync only" .-> WIRE["127.0.0.1 listener\nPeerTalk / usbmux over cable"]
    WIRE <-. "physical USB session" .-> MAC
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
- Files document picker / system share sheet
- AVFoundation QR scan（Ackのみ、Photo Libraryへ保存しない）
- EventKit
- haptics
- BIZ UDGothic registration / app-owned native ceremony styling
- app switcher privacy shield
- G0-B'を通過した別profileだけ、exact <code>127.0.0.1</code> listenerとUSB-mux session

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
- seed buildのApp Store Connect supported-device listを保存し、最低eligible iPadと提出時の代表的な最新iPadを物理端末matrixへ固定する。Simulatorはlayout/OS組合せの補助に限り、Metal memory/thermal/jetsam、Data Protection、Keychain、Files/share/QR/EventKit、font rendering、delta/archive/enrollmentは物理iPadで通す。
- 将来native iPad/Mac/Visionを有効化する場合はdevice family、native layout、memory/Metal、Files/share/QR/EventKit/transfer、privacy、screenshot、review matrixを追加する別milestoneとする。

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
| <code>os_sandbox.rs:806-814</code> iOS unsupported | mobileではsidecar sandbox codeをcompileしない。app-level sandboxとin-process closureを別物として監査し、Wired-enhancedだけG0-A/B'の残余loopback riskを明示 |
| <code>lib.rs:63-66</code> setup時にsidecar start | backend factoryへ置換 |
| <code>paths.rs:43-55</code> iOSをgeneric Unix扱い | Tauri app data/Application Supportへ |
| base <code>beforeDevCommand</code>/<code>beforeBuildCommand</code>がPowerShell | iOS overrideまたは依存追加なしのcross-platform commandへ |
| <code>create:false</code>＋manual <code>WebviewWindowBuilder</code> | iOS compile/run、navigation/download hook、window creationをM1で実機gate |

iOSへmacOS用の<code>allow-unsigned-executable-memory</code>、<code>disable-library-validation</code>等をコピーしない。CPython experimental JITを無効化する。static codeは最終署名対象executableへlinkし、dynamic frameworkは個別に署名してbundleする。

### A4. capability / CSP

- desktopとiOS capabilityを別ファイルにする。
- iOS capabilityはdesktop window minimize/maximize/closeを持たない。
- haptics（selection/impactのみ）、Files/share、QR Ack scanner、custom EventKitは各command/permissionを列挙する。baselineへLAN/Bonjour/network commandを含めず、Wired-enhancedだけexact wired START/STOP/TRANSFER permissionを別capabilityへ置く。
- capability auto-discoveryへ依存せず、configでIDを明示する。
- remote URL scopeは設定しない。
- production CSPはself/IPC-onlyを維持する。
- <code>NSAllowsArbitraryLoads</code>、<code>NSAllowsArbitraryLoadsInWebContent</code>を設定しない。
- privileged app commandを通常の<code>invoke_handler</code>登録だけで全windowへ開かない。<code>tauri_build::AppManifest::commands</code>へ登録し、Tauri ACLの対象にする。

Tauriのcapability、permission、scopeは公式の[capabilities](https://v2.tauri.app/security/capabilities/)、[permissions](https://v2.tauri.app/security/permissions/)、[scopes](https://v2.tauri.app/security/scope/)に照らして監査する。

Tauri ACLとApple OS authorizationは別層として管理する。

| 機能 | Tauri側 | Apple側 |
|---|---|---|
| <code>.pkbdelta</code> export/import | exact native prepare/present/import command。bytes/path/share targetはWebViewへ返さない | system share sheet / document picker、security-scoped URL |
| Wired session（G0-B' profileのみ） | exact START/STOP/TRANSFER。host/port/deviceを引数にしない | numeric <code>127.0.0.1</code>＋USB-muxだけ。Bonjour/LNP keyなし。M0で実機/App Review feasibilityを検証 |
| Calendar | custom EventKit command permission | <code>NSCalendarsFullAccessUsageDescription</code>、EventKit authorization |
| Files import | exact picker/import command | system document picker、security-scoped URL |
| Haptics | semantic grammarを実装するselection/impactだけ。notification/vibrate permissionなし | 通常は別のuser permissionなし |
| Keychain | raw keyを返さないnative command | access group / data-protection class |
| Security ceremony | delta export、enrollment/membership/admin/recoveryの開始だけ。署名結果やkeyを返さない | iOSの<code>NSFaceIDUsageDescription</code>、Keychain <code>SecAccessControlUserPresence</code> / LocalAuthentication |
| Camera（QR Ack baseline） | Ack専用custom scanner command。Photo Library/microphone permissionなし | <code>NSCameraUsageDescription</code>、明示操作時のruntime authorization |
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
| custom delta/share | system share/document pickerを提示する狭いnative plugin | WebViewへpackage bytes/path/provider resultを返さず、bounded staged importを使う |
| custom QR Ack | AVFoundationのQR symbologyだけを読む狭いnative scanner | camera明示操作、frame memory-only、single decoded payloadで即停止 |
| PeerTalk/usbmux | 古いthird-party transportで現行App Review適合は未証明 | M0 Spike限定。G0-B'通過時だけpinned sourceをWired-enhancedへ採用候補 |

filesystem pluginでtimestamp APIを使う場合は生成tree外のcanonical privacy manifestへ該当required reason（現行公式案内のFileTimestamp <code>C617.1</code>を含む）を記載し、generatorが<code>src-tauri/gen/apple/PrivacyInfo.xcprivacy</code>へ配置した結果をarchive実体で検証する。[Tauri filesystem plugin](https://v2.tauri.app/plugin/file-system/)

### A5. CI / release toolchain

- PRごとにfrontend boundary、Rust test、Python test、desktop smokeを実行。
- iOS simulatorでcompile/install/cold launch。
- release候補は物理iPhoneで実行。
- macOS companionは既存per-arch方針を維持し、arm64/x86_64で<code>.pkbdelta</code> writer/reader、OS share/import、QR Ack ceremonyのlogical fixtureをiOSと共有する。baselineにnetwork XPCを追加しない。G0-B'通過時だけ、固定usbmux endpointへ限定した<code>pkb-wired-agent</code>のper-arch署名・sandbox・公証・private-symbol scanを別artifactとして検証する。
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

1. archive上限と実機memoryを通る場合の署名済みbundle model。App Store 1.0の推奨baseline。
2. Files/AirDrop等でユーザーが明示importする署名済み<code>.pkbmodel</code> package。

model packageはGGUF、model role、architecture、quantization、exact size、SHA-256、publisher signatureを持つ。importはnative temp fileへstreamし、上限・symlink・signature・hashを検証後、atomic renameする。任意GGUFや未署名modelを「自己責任」で実行するtoggleは設けない。

model/runtimeはP2Pの通常sync対象にしない。

### B6. memory / thermal / lifecycle

初期測定点は保守的に置くが、値を製品仕様へ決め打ちしない。

- context 2048から測定開始。
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
- DB keyをpeerへ渡さない。
- SQLCipher keyをログ、UserDefaults、WebViewへ出さない。
- DB、WAL、SHM、temporary fileの全経路を暗号化・保護する。
- system SQLiteとSQLCipherの曖昧な二重linkを避ける。
- app background/protected data unavailable前にtransactionを完了しDBを閉じる。
- canonical DB、WAL/SHM、model、derived、cache、stagingを<code>NSURLIsExcludedFromBackupKey</code>相当でOS/iCloud Backupから除外し、archive実体とrestore drillで確認する。

SQLCipherはat-rest暗号化であり、通信暗号・認証・競合解決の代替ではない。

G0-Dの推奨baselineは、CPythonのiOS用<code>_sqlite3</code> extensionを固定SQLCipher buildへlinkし、serialized Python engine workerだけが単一connection queueを所有する方式とする。Rust、XPC service、WebViewはvaultを開かず、sync eventも検証後にPython workerへenqueueする。

- 1processのSQLite実装とPKB vault connection ownerを各1つに限定。
- SQLCipher version、compile options、page/KDF settingsをmigration receiptへ記録。
- <code>SQLITE_TEMP_STORE=2</code>等のrelease compile optionを実体から検査し、runtime <code>temp_store=MEMORY</code>を固定。
- arbitrary PRAGMA/SQLをfrontend、sync frame、native pluginから渡さない。
- sort/temp spillを誘発するqueryのrow/byte/time上限を定める。
- DB/WALだけでなくtemporary/sort spillも物理端末でplaintext scanする。

このlink/owner構成がM3 spikeを通らない場合はG0-Dへ戻り、Rust-owned single <code>VaultStore</code>案を規律変更として再審査する。実装途中で二つのownerを併存させない。

### C3. schema

最低限、次を持つ。

~~~text
schema_meta
vault_meta
vault_membership
canonical_record
sync_event
peer_cursor
delta_recipient
pending_delta_export
seen_delta_package
pending_delta_ack
delta_receipt
conflict_sibling
tombstone_ack
migration_receipt
~~~

<code>sync_event</code>の概念schema:

| field | rule |
|---|---|
| vault_id | random vault identity。全署名へdomain binding |
| actor_id | actor signing public key fingerprint。transport keyとは別 |
| actor_seq | actorごとの永続単調増加値 |
| event_id | actor_id + actor_seq |
| prev_actor_event_hash | actor直列chainの直前event hash |
| membership_seq / membership_head_hash | authorがevent生成時に参照した検証済みmembership snapshot。署名preimageへ含める |
| schema_version | exact supported value |
| entity_type / entity_id | allowlist / bounded |
| operation | allowlisted verb |
| lamport_time | causal ordering。actor IDでdeterministic tie-break |
| observed_wall_time | 表示/provenance専用。securityやwinner判定に使わない |
| parent/version | 因果・競合検出 |
| canonical_payload | exact canonical JSON bytes |
| payload_hash | SHA-256 |
| signature | domain-separated actor signature |

署名preimageは、少なくとも<code>PKB-SYNC-EVENT-V1</code>、vault ID、actor ID、sequence、previous hash、membership sequence/head hash、全header、payload hashをexact byte orderで含める。<code>actor_event_hash</code>は<code>PKB-ACTOR-EVENT-HASH-V1 || canonical unsigned event body</code>のSHA-256とし、signature bytesをhash chainへ含めない。署名は同じbody/hashを認証し、<code>prev_actor_event_hash</code>はこのdeterministic body hashを参照する。参照membership headはreceiverのcurrent chainと同一または検証済みprefixでなければならず、そのsnapshotでactorがactiveで、current retire cutoffにも違反しないことを確認する。actor sequence/hash chainのforkを見つけた場合は片方を選ばずactorをquarantineする。

event append、materialized table更新、inbound seen-package記録、dirty marker更新は単一transactionで行う。outbound peer cursorはこのtransactionでは進めず、署名済みAckの検証・receipt commitと同じ独立transactionでだけ前進させる。全frame/schema/membership/signature/hash/sequence検証が終わるまでcanonical stateへ副作用を起こさない。unknown schema、untrusted actor、gap、oversize、hash不一致はbatch全体をrollbackする。

### C3.1 replicated actor trust

delta recipientのenrollment identityと、過去eventを検証するvault actor trustを分離する。任意のwired transport handleはどちらのtrustにも昇格させない。

- 1.0は**single-admin**へ固定する。vault genesisは<code>vault_id</code>、Mac上の唯一のadmin authorization verification key、最初のactor verification keyを固定し、iPhoneはwriterとして追加する。
- admin authorization keyは通常data event用actor keyと分離し、membership操作時だけuser presenceを要求する。
- <code>actor_add</code>、<code>actor_retire</code>、key cutoff、<code>admin_transfer</code>を署名付きmembership eventとしてreplicateする。
- <code>actor_retire</code> cutoffはsequenceだけでなくexact <code>(actor_id, cutoff_actor_seq, cutoff_actor_event_hash)</code>とし、event未作成actorは定義済みzero-head sentinelを使う。adminは自分が検証済みのactor headだけをcutoffにでき、tuple全体をmembership署名preimageへ含める。
- membership eventは<code>membership_seq</code>、<code>prev_membership_hash</code>、operation、対象key/role/exact cutoff、enrollment receipt hashをdomain-separated preimageへ含め、現在の唯一のadmin authorization keyだけが発行できる。
- <code>membership_event_hash</code>も<code>PKB-MEMBERSHIP-EVENT-HASH-V1 || canonical unsigned membership body</code>のSHA-256とし、signature bytesを含めない。これにより署名前にordered batchの全previous-hashをdeterministically確定できる。
- <code>membership_seq</code>はgenesisから1ずつ増える全順序である。gap、同一sequence別hash、旧admin署名、offline競合branchはwinnerを自動選択せずvault membershipをquarantineする。
- <code>admin_transfer</code>は旧admin署名と新admin keyのuser-presence付きacceptanceを同じeventへbindingし、そのcommit後だけ新adminを有効にする。1.0のtransfer先は検証済みdesktop companionを持つreplacement Macだけとし、iPhoneは常にwriterでadmin keyを持たない。複数admin roleとiOS admin roleは1.0 schemaで拒否する。
- 新端末はgenesisから現在headまでのmembership chainを検証してからdata eventを受け入れる。retired actor chainはsequenceだけでなくcutoff hashまで一致したbranchだけを受け入れ、cutoff後event、同sequence別hash、cutoffへ到達しないalternate branchを拒否する。
- enrollment receiptはactor signing key hash、delta-envelope public-key hash、roleをbindingする。G0-B'を採用する場合だけ、別のwired transport transcriptへUSB session key/hashをbindingし、baseline identityへ混ぜない。
- actor private keyは各端末のSecure Enclave/Keychainから出さず、verification public keyだけをreplicateする。
- key喪失/端末交換時はsequenceを再利用せず、旧actorをretireして新actorを追加する。
- DB-only snapshot rollbackとpeer-visible forkを検出するため、actor-head vector hashとmembership headのanchorをKeychainにも保存する。

DBとKeychainは単一transactionにできないため、単純な「不一致=攻撃」にはしない。M3でDB-first checkpoint＋two-slot/pending anchorのcrash recoveryをADR化する。DBがKeychain anchorより先なら署名chainが旧anchorの正当な延長かを検証してanchorを前進できる。KeychainがDBより先、旧anchorの子でない、同sequence別hashならrollback/forkとしてrecoveryへ入る。全commit/anchor instruction boundaryでcrash testを行う。

このanchorはhardware monotonic counterまたは外部witnessではない。DB、Keychain anchor、全peer状態を整合した同じ旧snapshotへ同時に戻すcoherent rollbackは1.0では検出保証しない。保証対象はDB-only rollback、同sequence別hash、将来新しいheadを持つpeerと再会した時に可視化されるforkである。

1.0で同時active deviceをMac＋iPhoneの2台へ制限しても、端末交換で過去actorは残るためmembership trust chainは省略しない。

### C3.2 DeviceSigner / transaction ownership

SQLCipher connection、event/materialized table、membership、pending receiptのtransaction ownerは一貫してserialized Python repository workerである。Rust、AppKit/UIKit、share/QR/wired native adapterはvaultを開かず、native signerは署名とKeychain anchorだけを担当する。

logical seamは次へ固定する。

~~~text
Python SignedEventRepository
  -> reserve exact pending event in SQLCipher
  -> DeviceSigner.sign_reserved_event(...)
       iOS: closed _pkb_native call
       macOS desktop: private stdio reverse control frame to Tauri parent
  -> verify signature with public key
  -> final DB transaction: sync_event + materialized state + dirty marker
  -> DeviceSigner.finalize_anchor(...)
~~~

- Pythonはvalidated canonical payload、actor sequence/previous hash、membership reference、CIDからproposalを作り、まず<code>pending_signed_event</code>をDBへdurable commitする。まだmaterialized stateへ反映しない。
- native DeviceSignerはdomain/version/vault/actor/sequence/previous hash/membership head/operation/size/CID/reservation IDを独立にexact parseし、Keychainのcommitted actor headと一致する同一pending reservationだけを署名する。raw byte/stringを任意に署名するoracleを持たない。
- signerはsignatureとdeterministic actor event hashをtwo-slot Keychain pendingへ保存してから返す。response loss/retryでは再署名せず、同じreservationの保存済みsignatureだけを返す。
- Pythonはactor public keyでsignature/event hashを再検証し、<code>synchronous=FULL</code>相当のfinal transactionでevent append、materialized update、dirty marker、DB pending消去を一括commitする。その後だけnative signerへexact event hashのfinalizeを送り、committed anchorを前進する。
- restart時はDB pending/final eventとnative pending/committed anchorを照合する。DB pendingだけなら同じproposalを再開、native pendingだけならDB proposal hash一致時だけ再開、DB final＋native pendingならsignatureを再検証してanchor finalizeする。別proposal、sequence gap、同seq別hashはquarantineする。

macOS reverse control frameは既存private stdio channel内のreserved internal typeであり、WebView/Tauri command/event/XPCへ公開しない。EngineManagerのsingle reader/writer loopが通常responseとinternal sign request/replyをCIDでmultiplexし、command handlerからnested invokeしたり相互同期waitでdeadlockしたりしない。single-event reservation 1件、または承認済みmembership batch 1件だけをin-flightにし、両者の同時併存を拒否してqueue/size/timeout/cancelをboundedにする。Rust parentはfixed schema、CID、state、sizeを検証し、sidecar stdoutを任意native selectorへ変換しない。iOSの<code>_pkb_native</code>も同じlogical contractを実装する。

admin membership操作も同じownership ruleに従う。AppKit <code>pkb-security</code>はuser presence後にexact membership preimageを検証・署名するだけで、Python repositoryがsignatureをpublic-key再検証してmembership event/pending receiptを単一DB transactionでcommitし、その後native admin anchorをtwo-slot finalizeする。native UI/Rust/share/QR/wired adapterからSQLCipherを開かない。

membership signerはcount 1の通常操作に加え、replacement transfer専用の**bounded atomic batch（最大3 event）**を持つ。

- Pythonは全unsigned bodyとdeterministic membership hashを先に作り、<code>membership_batch_reservation</code>としてDBへdurable保存する。
- native signerはbatch count/operation allowlistを検証し、最初のsequence/previous hashがcommitted membership anchorへ一致し、後続がsequence +1かつ直前unsigned body hashへ連結することを順に再計算する。1.0で許す3件batchは<code>actor_add(new)</code>→<code>actor_retire(old exact cutoff)</code>→<code>admin_transfer</code>だけである。
- user presence後に全bodyへ署名し、全signature/body hash/final headを1つのtwo-slot native pending batchへ保存してから返す。partial signature setを返さず、retryは保存済みbatchだけを返す。
- Pythonは全signatureとinternal chainを再検証し、3 membership eventを1つのSQLCipher transactionでcommitする。その後final batch hash/headを指定してnative anchorを一度だけfinalizeする。
- crash/restart時はDB/nativeのbatch reservation、signature set、final DB rows、anchorを全体として照合し、partial prefixをactive membershipにしない。

M6-Tのdelta manifest/Ack署名も同じDB-first reservation原則へ従う。native signerにgeneric bytesやarbitrary digestを渡さない。

- outboundはPythonがpeer cursor、membership head、actor vector、event hash、recipient delta-key hash、package/Ack handleを<code>pending_delta_export</code>へcommitしてから、exact <code>sign_delta_manifest</code>を呼ぶ。Pythonがsignatureを再検証してsigned manifestをreservationへ確定するまでsealed fileを外へ渡さない。
- inboundはcanonical event transactionとactor/membership Keychain anchor finalize後、Pythonがexact committed head/vector/package hashを<code>pending_delta_ack</code>へreserveして<code>sign_delta_ack</code>を呼ぶ。Pythonが再検証・receipt commitした81-byte Ackだけをnative QR routeへ渡す。
- response loss/restartは同じreservationの保存済みsignatureを再利用し、別manifest/Ack、same handle/different preimage、retired actor、sequence/head regressionをquarantineする。
- share/Files/QR/wired helperはSQLCipher connectionもDeviceSigner oracleも取得せず、既に確定したopaque artifact handleとfixed success/error eventだけを扱う。

### C4. 同期対象分類

| 分類 | 例 | 方針 |
|---|---|---|
| 正本・同期 | user-entered diary/record、calendar/finance、imported message source、manual profile、永続化が既に許可されたprobe/interview debrief/report、明示保存したconsultation、curated ES/knowledge | oplogで同期 |
| 条件付き同期 | OS Calendarから明示importしたevent、明示保存したLLM transcript/document | provenanceとconsentを保持して非権威documentとして同期 |
| 派生・非同期 | embeddings、vector/LSM index、tensor、deep profile、retrieval manifest、compiled narrative、cache | 端末ごとに再生成 |
| runtime・非同期 | GGUF、llama/search runtime、Python/NumPy、artifact manifest、app binaries | production GGUF/runtimeはApp bundle、optional signed importは補助のみ |
| security・通常同期外 | DB key、device private key、peer receipt cursor、delta-envelope private key、wired consent、egress/policy | 端末localのみ。vault contact-alias keyはenrollment grant時だけwrapped provision |
| diagnostics・非同期 | logs、telemetry、crash data、temporary inference、quarantine | 同期しない |

「processed配下だから同期」「JSONだから同期」のようなpathベース判定にしない。entity schemaのallowlistだけで決める。

sync transactionはcanonical eventをcommitし、対象のderived-generation dirty markerを立てるだけにする。そのtransaction内でprofiler、embedding、index/tensor rebuild、LLMを起動しない。現行の「RECORD保存でprofilerを走らせない」「LLM/embeddingは初回利用まで遅延初期化」を維持し、明示rebuildまたは次回consult/profileのbounded foreground jobで再生成する。

dirty markerは単なるbooleanにせず、必要な<code>canonical_head_hash</code>、schema、<code>embedder_id</code>、generator versionから<code>required_generation_id</code>を確定する。derived artifact manifestにも同じ入力とartifact hash/count/dimensionを署名・記録し、query時にcurrent required IDと完全一致する世代だけを使う。不一致またはdirty中に旧LSM/index/tensorをsilent利用せず、固定<code>DERIVED_STALE</code> stateで該当機能だけをambient unavailableにする。

rebuildはData Protection＋backup除外済みの新しいstaging generationへ行い、bounded count/dimension/hash、source head、embedder/runtime signatureを検証した後、manifest pointerをatomic swapする。crash、cancel、low disk、validation失敗では旧generationをactiveへ戻さず、orphan stagingを次回起動時に削除してdirtyを維持する。canonical dataの閲覧・入力は継続できるが、retrieval/profile等のderived依存機能は正しい世代が完成するまでfixed unavailableとし、品質の低いfallbackへ落とさない。

LLM由来documentは<code>provenance=llm</code>の非権威資料であり、受諾操作だけでdeterministic profile/tensor/authority stateへ昇格させたり、future promptへ自動再注入したりしない。ユーザーが観測事実として編集・再入力する場合は別operationとprovenanceで記録する。

probe/interviewのmemory-only turn state、priority queue、未完sessionを永続化・同期しない。永続entityは<code>simulated</code>、speaker（self/contact）、source（subjective/objective）、provenanceをlosslessに保持し、simulated回答やLINE上のself発話を通常の観測事実へ誤分類しない。

### C4.1 cross-device identity

現行のcontact identityと一部state chainはroot key派生であり、端末ごとのroot keyが異なると同一入力から同じIDを再計算できない。この問題をplaintext/normalized handleの永続化やDB key共有で解決しない。

推奨方針:

- 既存<code>C-...</code> IDはcanonical opaque IDとして保持し、pair先で再計算しない。
- genesis時にcontact identity専用のrandom vault HMAC keyを作る。DB key、device root、actor/transport key、state-chain keyとは別物とする。
- migration時だけraw sourceをmemory上でnormalizeし、vault HMAC digest→既存opaque C-IDの対応を作る。plaintext/normalized handleはDB/oplog/logへ書かない。
- vault HMAC keyはnormal sync eventへ載せず、<code>.pkbgrant</code> enrollment transcriptにbindingしたencrypted key envelopeで新deviceへ一度だけprovisionし、ThisDeviceOnly Keychainへ保存する。
- iPhoneの新規importは同じHMAC digestでidentity directoryを照合し、存在するC-IDを再利用する。
- 両端で同時に未知contactを作った場合は両IDを保持し、ambient identity conflictで明示mergeする。
- device root key、DB key、actor/transport private keyは共有しない。
- retrieval/profile/state-chain等の派生artifactは受信せず、各端末のlocal keyで再生成する。
- operation logのcross-device authenticityはactor signatureで検証し、相手端末のlocal HMAC rootを要求しない。

revoked peerから既配布vault HMAC keyを回収できない。1.0は自動key rotationを約束せず、compromise時のidentity epoch migrationを別設計とする。全device/keyを失った場合の復旧には、明示作成した暗号化manual recovery packageが必要である。

この変更は<code>secure_identity.py</code>とstate-chain意味論に触れるため、G0-D ADRとgolden migration testなしに実装しない。

### C5. 既存データ移行

**M3 rehearsal:**

1. legacy inventoryをread-onlyで列挙し、file size/hash/record countをreceipt化。
2. strict parserで新DBへdry-run import。
3. record単位のcanonical hashを旧readerと新readerで比較。
4. shadow write期間に双方のlogical outputを比較。
5. deterministic genesis membership/oplogを含むstaging candidateを構築し、candidate rootをmigration receiptへ署名する。legacy sourceを変更・削除しない状態でtest-only cutover/revertを反復する。

**M6-R完了後のproduction cutover:**

6. app-managed legacy plaintextをcutover後にlogical deleteすることをinlineで明示し、承認されなければproduction cutoverへ進まない。外部のuser-selected source fileは削除対象にしない。
7. writerを止めてfinal legacy hashを再確認し、deterministic genesis membership/oplogを含む**最終staging candidate**を一度だけ構築する。
8. そのcandidate root、migration receipt、canonical event、membership chainをM6-R形式のimmutable encrypted migration archiveとしてユーザー指定先へ作成する。
9. 別の空staging vaultへrestoreし、candidate root/hash/count/membershipを独立検証する。export成功だけで先へ進まない。
10. 再生成せず、検証済みの同一staging candidateをatomicにcanonical storeへ切り替える。
11. legacy reader/writerをcompile/runtimeとも無効化し、app-managed legacy plaintext treeをlogical deleteする。<code>.bak</code>へのrenameやplaintext quarantineは作らない。
12. Application Support、Documents、tmp、staging、app-owned export pathをknown marker/canaryでscanし、active app-managed plaintext copy 0を確認する。
13. enrollment grant、<code>.pkbdelta</code> export、optional wired sessionはcutoverとplaintext cleanup完了後だけ許可する。

M3 exitはschema/repository/migration rehearsalの完了であり、production user dataの不可逆cutoverではない。production cutoverはM6-Rのwriter・reader・restore drill完了後、M7 release rehearsalでだけ許可する。これによりM3→M6-R→M7の一方向依存を維持する。

corrupt/ambiguous legacy dataをsilent repairしない。安全な固定errorと対象record IDを示し、元fileを変更しない。同期開始後に旧storeへdowngradeしない。

APFS/SSDのcopy-on-write block、OS snapshot、既存Time Machine/外部backup、ユーザーが保持する元sourceのphysical secure eraseはappから保証できない。保証するのはapp-managed live namespaceからのlogical removalと、以後plaintextを再生成しないことまでである。migration前のphysical remanenceをUI/privacy/threat modelへ明記し、必要ならOS側のFileVault・snapshot/backup管理をユーザーへ案内するが、PKB自身がそれらへアクセス・削除しない。

### C6. conflict policy

- append-only journal/consultation: event ID集合和。
- tags/links: add/remove identityを持つOR-set相当。
- 低リスクscalar setting: field allowlist＋causal Lamport＋actor IDのdeterministic LWW。wall clockをwinnerに使わない。
- diary/note本文の同時編集: silent LWW禁止。両siblingを保持。
- delete: tombstone。1.0ではmembership/ackにかかわらずoplogからphysical GCしない。
- security/enrollment secret/receipt cursor/wired consent/egress: 絶対に同期しない。
- conflict解決自体も新しい署名eventとして残す。

conflict UIはmodalにせず、status lineと専用のambient inboxで提示する。

1.0のdeleteはlogical deleteであり、暗号化されたevent history内の旧本文は復号可能なまま残る。UI/privacy policyでこれを明示する。将来hard deleteを約束する場合は、署名付きcompaction epochまたはrecord keyのcrypto-shreddingを別設計し、全peerのmembership/ack証明なしに実装しない。

## 8. Workstream D — ゼロトラスト無ポート同期

### D1. Portable Delta baseline（T1）

1. 事前にM6-Rの<code>.pkbjoin/.pkbgrant</code> ceremonyでwriter actor keyとrecipient-specific delta-envelope public keyをmembershipへbindingする。Macはadmin、iPhone/iPad compatibility instanceはwriterのままとする。
2. ユーザーが「差分を書き出す」を選ぶ。WebViewはopaque peer selection tokenとoperation codeだけをEffectとして出し、native security routeがverified local alias、方向、対象vault、前回Ack stateをWebView由来でない値から再表示する。
3. 明示confirm後、Python repositoryがpeerの最後に検証したAck cursorからmembership prefix＋actor have-vector＋不足event rangeを確定し、<code>pending_delta_export</code>へsnapshot/rootをdurable予約する。Ackがない、古い、失われた場合は安全側に重複eventを含める。
4. native exporterはrecipient-bound sealed <code>.pkbdelta</code>をbounded streaming生成し、opaque temporary fileをData Protection＋backup除外済みstagingへ置く。package bytes/path/recipient key/signatureをWebViewへ返さない。
5. iOSは<code>UIActivityViewController</code>または<code>UIDocumentPickerViewController</code>、macOSは対応するOS share/import surfaceを提示する。ユーザーがAirDrop/Files/外部media等を選ぶ。PKB自身は宛先をdiscover/connectせず、share extension/providerの種類をtrust判定へ使わない。[UIActivityViewController](https://developer.apple.com/documentation/uikit/uiactivityviewcontroller) / [UIDocumentPickerViewController](https://developer.apple.com/documentation/uikit/uidocumentpickerviewcontroller)
6. 受信側はfile association/document pickerからnative bounded streamへ取り込み、clear header上限→recipient decrypt→inner manifest→membership-first→actor eventの順で全検証する。検証完了前のcanonical DB副作用は0である。
7. Python repositoryだけがvalidated batchとlocal receiptを単一transactionでcommitし、derived dirty markerを立てる。native ownerはcommit resultをexact parseしてからone-time QR Ackを作り、Native Ceremony Design Annex準拠のscreenへ表示する。
8. 送信側がQRをnative scannerで読むと、<code>ack_handle</code>からoutstanding export reservationを引き、完全な署名preimageをlocal DBから再構築してrecipient actor signatureを検証し、local <code>peer_cursor</code>/<code>delta_receipt</code>だけを前進する。Ackはcanonical event削除やoplog GCの根拠にしない。

双方向同期は同じ一方向packageを2回使う。A→B import後、BはAへの不足eventとA→B Ackを次のB→A <code>.pkbdelta</code> inner manifestへpiggybackできる。AがB→Aをcommitした後の最終AckだけをQRでBへ返す。片方向だけなら受信直後のQR Ackで終了する。package/Ack紛失時は同じeventを再送し、event ID＋body hash＋signatureでidempotentに処理する。

camera拒否・故障・アクセシビリティ上の代替として、同じAck bytesをtiny recipient-bound <code>.pkback</code> fileとしてOS所有surfaceで戻せるようにする。通常導線はQRのままにし、manual codeへの短縮、clipboard、Photo Library scan、network callbackをfallbackにしない。

### D2. <code>.pkbdelta</code> envelope / metadata silence

<code>.pkbdelta</code>はAirDrop/Files/provider/外部mediaを信用しないstore-and-forward envelopeであり、SQLite/zipのコピーではない。各peerはactor/admin/DB/contact keyと分離した<code>delta_recipient_key</code>をnative Keychain/Secure Enclave境界で生成し、private keyをThisDeviceOnlyに保つ。public-key hashは<code>.pkbjoin/.pkbgrant</code>とadmin署名付きmembershipへbindingする。retired peer keyへの新規exportを拒否する。

G0-Dが承認した標準sealed-envelope方式とAEADだけを使い、独自ECDH/KDFを設計しない。clear outer headerは次だけに固定する。

~~~text
PKBDLT01 magic / exact version / crypto-suite ID
ephemeral public key / nonce / salt
random unlinkable package tag
chunk size / chunk count / padded ciphertext length
~~~

vault ID、sender/recipient actor、device/user名、membership head、actor vector、event count/range、record title、wall time、Ack handleはencrypted inner manifestへ置く。stable recipient hintをclearへ置かず、端末内のbounded active recipient keysだけを試す。filenameは96bit以上のCSPRNG basename＋<code>.pkbdelta</code>とし、Quick Look preview/Spotlight content metadataを生成しない。compressionは1.0で使わず、inner lengthをG0-D承認済みのbounded size bucketへpaddingする。拡張子、転送時刻、bucket size、OS/providerが付与するfile metadataは残余リスクとして明示する。

inner manifestは少なくとも次をcanonical bytesで持つ。

~~~text
PKB-DELTA-MANIFEST-V1
|| protocol / schema / crypto versions
|| vault_id
|| sender_actor_id / intended_recipient_actor_id
|| recipient_delta_key_hash
|| membership base/head + ordered missing prefix
|| sender actor-have-vector hash
|| bounded event ranges + ordered event hashes
|| package_id / ack_handle / expiry policy
|| padding bucket / ciphertext chunk commitments
~~~

exporting actorがclosed DeviceSigner operationでmanifestを署名し、innerへsignatureを含める。各eventのactor signatureも独立に再検証する。share providerが差替え・truncate・reorder・replayしても、outer上限、AEAD、manifest signature、membership/event chainのいずれかでDB副作用0のまま拒否する。未知vault、未enroll recipient、必要baseを持たない端末には固定<code>DELTA_BASE_REQUIRED</code>を返し、<code>.pkbvault</code> restoreへ誘導する。<code>.pkbdelta</code>をrecovery archiveとして解釈しない。

outbound stateを次に固定する。

~~~text
IDLE -> PREPARING -> RESERVED -> SIGNED -> SEALED
     -> HANDED_TO_OS -> WAITING_ACK
     -> ACK_PARSED -> ACK_VERIFIED -> ACK_COMMITTED
~~~

share cancel/failureはdelivery証明ではなくcursor不変で終了する。OSが「共有完了」を返しても<code>WAITING_ACK</code>と表示し、「同期完了」にしない。membership変更やrecipient retire後は未送信packageをinvalidateし、既に外へ渡った暗号文をremote revokeできるとは主張しない。

inbound stateを次に固定する。

~~~text
PICKED -> COPIED_TO_STAGING -> OUTER_VALIDATED -> DECRYPTED
       -> MEMBERSHIP_VALIDATED -> EVENTS_VALIDATED -> COMMITTING
       -> ANCHOR_FINALIZED -> ACK_RESERVED -> ACK_SIGNED -> ACK_READY
~~~

security-scoped provider URLはTOCTOU境界である。直接繰返し読まず、size/count上限付きでapp-owned・backup除外済みstagingへ一度copyし、hashを固定してprovider accessを閉じてからparseする。low disk、URL revoke、background、crashではpartial materialization 0とし、durable staging reservationから再開または安全に破棄する。exact duplicateはactive stateを変えず保存済みAckを再表示し、same package ID/different manifestまたはsame event ID/different bodyはfork/quarantineとする。

### D3. recipient identity / QR Ack

- actor signing keyはP-256 Secure Enclaveを優先し、代替はThisDeviceOnly Keychainとする。
- single-admin authorization keyはactor key・delta recipient keyから分離し、現在のadmin Macだけが保持する。membership operationへ<code>SecAccessControlUserPresence</code>を要求し、通常record appendやAck再表示ではadmin promptしない。
- delta recipient keyはdeviceごとのenvelope復号専用で、actor/admin署名やDB encryptionへ流用しない。private key、raw shared secret、content keyをPython/WebView/logへ返さない。
- application event、delta manifest、Ackはそれぞれ別domainでactor keyにより署名する。OS transfer channel、filename、cable locationをidentityとして信用しない。

Ack署名preimageを次へ固定する。

~~~text
PKB-DELTA-ACK-V1
|| vault_id
|| package_id
|| package_manifest_hash
|| committed_membership_head_hash
|| acknowledged_actor_vector_hash
|| recipient_actor_id
|| ack_handle
|| COMMITTED
~~~

QRにpreimageを載せない。wire payloadは<code>version(1) || ack_handle(16) || P-256 raw signature(64)</code>のexact 81 byteである。<code>ack_handle</code>はpackageごとのCSPRNG単回値でstable device/vault IDではない。senderはhandleからlocal pending exportを一意に引き、recipient actor verification keyと完全preimageを再構築して署名を検証する。unknown/ambiguous handle、wrong length/version、signature mismatch、別package、retired actor、replayでcursorを動かさない。

Ackはinbound event transactionとKeychain anchor finalizeが完了した後だけ、Pythonの<code>pending_delta_ack</code> reservation→native exact signer→Python public verification→receipt DB commitの順で作る。再表示は保存済み同一Ackだけを返し、任意bytesを署名するoperationを作らない。DeviceSignerへ追加できるのは次のclosed operationだけである。

- <code>sign_delta_manifest(reservation_id, exact_manifest_fields)</code>
- <code>sign_delta_ack(reservation_id, exact_ack_fields)</code>

compromised/malicious peerは自身のactor keyで「commit済み」と虚偽署名できるため、Ackは相手端末の物理storageを外部から証明するものではない。1.0ではAckを再送cursorの最適化と監査receiptにだけ使い、canonical oplogのphysical GCや唯一copy削除を行わない。

QR生成・読取はReact/WKWebView外の<code>pkb-security</code> native routeが所有する。iOSはUIKit＋AVFoundationのQR symbologyだけ、macOSは対応native camera routeを使う。[Apple QR symbology](https://developer.apple.com/documentation/avfoundation/avmetadataobject/objecttype/qr) / [NSCameraUsageDescription](https://developer.apple.com/documentation/bundleresources/information-property-list/nscamerausagedescription)

- exact version/lengthの単一QRだけを受け、URL、deep link、text commandとして解釈しない。
- capture sessionはユーザーが「Ackを読む」を押したforegroundだけで開始し、最初のvalid/terminal result、cancel、backgroundで即停止する。
- frame、preview image、decoded payloadをPhoto Library、pasteboard、DB、log、React stateへ保存しない。WebViewへ返すのは固定<code>ACK_ACCEPTED</code>/<code>ACK_REJECTED</code> eventだけ。
- camera permissionはその操作時だけ要求し、拒否時は<code>.pkback</code> file fallbackを同じnative ceremony内で案内する。microphone/Photo Library permissionは要求しない。
- native routeはG0-E Native Ceremony Design AnnexとBIZ UDGothic assetを使う。OS所有camera/Face ID permission sheet自体はtrusted system exceptionであり、偽装しない。

device retirement時はmembership cutoff、delta recipient key binding、local cursorを更新し、retired keyへの新規exportを拒否する。retire前にpeerへ渡ったplaintextまたは復号可能packageを回収できず、曖昧な「vault key rotation」やremote eraseを約束しない。Secure Enclave/Keychain境界は[Storing keys in Keychain](https://developer.apple.com/documentation/security/storing-keys-in-the-keychain)と[Secure Enclave P-256](https://developer.apple.com/documentation/cryptokit/secureenclave)に従う。

### D4. package / frame contract

復号成功後のpackage、OS provider、enrolled peerも信用しない。

- protocol/schema version
- package / import reservation ID
- outer/inner/chunk type
- monotonic counter
- payload length
- canonical payload hash
- 最大frame、batch件数、総byte数
- replay / seen-package window
- parse time / I/O / disk / memory budget

length検証前にallocateしない。chunkをstagingへ書く前にper-chunk上限を検査し、full allocationや<code>File.arrayBuffer()</code>を避ける。duplicate、replay、out-of-order、gap、unknown type、oversizeをhard failする。peer key、provider URL/path、payload、cryptographic error detailをWebViewへ返さない。G0-B' wired transportもこのexact frame contractを再利用し、別codecを持たない。

### D5. membership-first delta / resume

data deltaより先にmembershipを収束させる。exporterはlast verified Ack cursorをhintにしてもreceiver stateを推測せず、package内を次の順序へ固定する。importerは同じ順にvalidate/commitし、Ackは最後にしか作らない。

~~~text
membership base/head
 -> ordered missing membership prefix
 -> sequence / prev-hash / admin signature validation
 -> membership transaction
 -> sender actor have-vector (contiguous_seq, head_hash)
 -> declared bounded ranges
 -> bounded signed batch
 -> full validation
 -> one DB transaction
 -> Keychain anchor finalize
 -> signed QR/file Ack
~~~

- headが一致すればdata phaseへ進む。
- 一方が他方の検証済みprefixなら不足membership eventをsequence順に適用し、そのcommit後に同packageのactor rangeを検証する。receiverがexporterより先行する場合はpackageを破壊的に拒否せず、共通prefixを検証して既知duplicateをidempotent処理し、逆方向deltaへ不足分を載せる。
- 同一sequence別hash、どちらもprefixでないhead、旧admin/複数admin/iOS admin eventはmembership forkとしてquarantineし、data phaseへ進まない。
- data batchがunknown actorまたはpackage内にもないmembership headを参照した場合、DB副作用0の固定<code>MEMBERSHIP_REQUIRED</code>でpackageを拒否する。別network round-tripへ自動移行せず、新しいdelta exportを要求する。
- package内で<code>actor_add</code>/<code>retire</code>/<code>admin_transfer</code>をcommitしたら、後続data eventはそのordered new headを正しく参照する場合だけ同transaction planへ入れる。途中head、alternate branch、不明順序ならpackage全体をrollbackする。

membershipが収束した後だけ、packageのactorごとの<code>(contiguous_actor_seq, actor_event_head_hash)</code>、declared range、receiver local vectorを比較して不足eventを適用する。sequenceだけのhigh-water markは禁止する。

- same actor/same sequence/different head hashはmissing range 0として通さず、即fork quarantineする。
- sequenceが異なる場合、短い側のhead hashが長い側chainの同sequence ancestorと一致し、最初のmissing eventの<code>prev_actor_event_hash</code>がそこへ接続することを検証する。不一致ならforkである。
- actor-have-vector全体のcanonical hashをsigned delta manifestへbindingし、manifestとevent rangeが一致しなければimportしない。
- retired actorを初めて受け取るreceiverは、eventをbounded stagingへ置き、admin署名済みexact cutoff <code>(seq, hash)</code>まで一続きに到達してからmaterialized stateへcommitする。到達前はhistory incomplete、cutoff超過は常に拒否し、alternate branchをactiveへしない。

再送はpackage IDとevent IDでidempotentにする。単なる<code>INSERT OR IGNORE</code>だけに頼らず、duplicate package manifestおよびeventのbody hash/signatureが既存値と一致することを確認する。同じIDで別manifest/payloadなら攻撃/破損としてquarantineする。

SQLite DB/WAL/SHMを直接送らない。SQLite Session Extensionは将来の局所最適化候補に留め、1.0の因果・idempotency・business conflictはapp-owned oplogで扱う。[SQLite Session Extension](https://www.sqlite.org/sessionintro.html)

### D5.1 OS-owned transfer lifecycle

- export/import/QRは両端foregroundの明示操作だけ。silent periodic sync、push wakeup、provider polling、Macからの常時着信を採用しない。
- OS share sheetへpackageをhand offした時点はdeliveryでもcommitでもない。app復帰後は<code>WAITING_ACK</code>を静的status/determinate progressで示し、textarea/scroll/他機能をblockしない。
- backgroundへ移ると新規package作成、provider copy、QR captureを止める。短いbackground assertionは開始済みのSQLCipher transactionまたはKeychain anchor finalizeだけに使い、LLMや長いfile copyへ流用しない。
- inbound provider URLはforeground中にbounded staging copyを完了できない場合、security scopeを閉じて安全に中断する。partial stagingは暗号化・backup除外し、resume tokenにpath/filenameを含めない。
- share cancel、provider unavailable、camera denied、Ack lossは固定ambient stateへ変換する。OS/provider raw error、destination、path、device名を表示・logしない。
- temporary exported packageはAck受領または明示破棄後にapp-owned stagingからlogical deleteする。OS/provider/AirDrop受信先のcopyを削除できるとは主張しない。

### D6. M6-U USB-mux / PeerTalk有線session（T2、条件付き）

M0でSpikeし、G0-B'がprovisional Goになった場合だけM6-Uを開始する。これは<code>Silent Transfer</code>を置換する別sync coreではなく、同じsealed <code>.pkbdelta</code> packageとsigned Ackを物理cable上で往復させるtransport adapterである。[PeerTalk](https://github.com/rsms/peertalk) / [usbmuxd](https://github.com/libimobiledevice/usbmuxd)

~~~text
Mac app native ceremony
  -> fixed pkb-wired-agent (no vault/model/admin/actor key access)
  -> system usbmuxd AF_UNIX endpoint
  -> verified USB-only device handle / fixed forwarded port
  -> physical cable
  -> iOS native plugin bound to 127.0.0.1 only
  -> bounded sealed .pkbdelta frames
  -> Python-only validation / SQLCipher commit
  -> signed Ack over the same cable
~~~

G0-B'の実装契約:

- <code>wired-sync</code>はdefault offで、<code>egress-live</code>または旧<code>lan-sync</code>との同時compileを拒否する。Wired-enhanced artifact以外にはPeerTalk、usbmux symbols、loopback listener、wired commandを含めない。
- iOS native ownerだけが、両端の明示arm後、numeric IPv4 <code>127.0.0.1</code>＋compile-time固定portへ<code>AF_INET/SOCK_STREAM</code>でbind/listenする。backlog 1、session 1、bounded accept 1とし、host/port/deviceをWebView/Python/config/user inputから受けない。
- <code>localhost</code>、<code>0.0.0.0</code>、<code>::</code>/<code>::1</code>、DNS、Bonjour、MultipeerConnectivity、Network.framework discovery、URLSession、Wi-Fi/AWDL/cellular/tetheringを使わない。Local Network usage key/prompt、network client/server entitlementをbaselineから持ち込まない。
- Mac adapterはsystem usbmuxdのfixed AF_UNIX endpointだけへ接続し、libraryが報告するconnection typeをexact USBとして検証する。network device lookup、netmuxd、Wi-Fi sync、first-device自動選択をcompile/runtimeで拒否する。複数physical deviceは固定<code>WIRED_DEVICE_AMBIGUOUS</code>で停止する。
- system USB trust/pairingはPKB identityではない。両端のenrolled actor、recipient envelope key、membership head、native user confirmationを検証し、sealed/signed package以外を受けない。
- iOS listenerはbackground、cable detach、Mac/iPhoneのcancel、timeout、terminal error、Ack完了で即closeする。再接続は両端の新しい明示armを必要とし、常駐listenerを作らない。
- UDID、serial、USB product/device name、usbmux path/handleをDB、log、error、WebView、enrollment identityへ保存しない。必要なopaque handleはmemory-only session lifetimeに限定する。
- Mac helperを必要とする場合、AF_UNIX usbmux access＋bounded frame relayだけを持ち、vault/model filesystem、delta private key、actor/admin signer、任意socket/connect/sign/decrypt/key-export APIを持たない。親app/sidecarとfixed IPC schema、code-signing/team requirementで相互検証する。
- GPLのusbmuxd/libimobiledevice executableをbundle/spawnしない。PeerTalkはpinned source commitとMIT noticeをSBOMへ入れ、private/undocumented symbolやunsafe frame parserを監査する。

M0 Spikeは次をminimum/latest iPhone、mandatory iPad compatibility mode、arm64/x86_64 Macで実測する。

1. current pinned Xcode/SDK、iOS 17 deployment、Tauri mobile bridgeでclean build/archiveできる。
2. bind addressが<code>127.0.0.1</code>だけで、非loopback connect/listen、DNS/mDNS/Bonjour、Local Network promptが0。
3. Wi-Fi/Bluetooth/cellularをoffにしてbounded roundtripでき、onの場合もPKB-owned radio interface packetが0。
4. USB connection type以外、network lookup、複数device、malicious USB host、wrong actor/key/package、frame fuzzをfail-closeする。
5. unplug/background/cancel/crashの全chunk/DB/anchor boundaryでpartial materialization 0、listener bounded close、idempotent resume。
6. iOS archiveのprivate-symbol/static analyzer、App Review Guideline 2.5.1 dossier、Mac sandbox/Developer ID signing/notarizationを通す。
7. PeerTalkが要求し得る<code>/private/var/run/usbmuxd</code> accessまたはtemporary sandbox exceptionを法務/security reviewする。承認不能なtemporary exception、private API、jailbreak/device-management/MFi accessory前提が必要ならNo-Goとする。
8. exact allowlist外の<code>socket/bind/listen/connect</code> callsite 0、Python <code>_socket</code> import 0、WebView commandから任意transport到達0。

M6-U exitは上記evidenceに加え、2端末双方向delta、signed Ack、membership fork、cable removal、rate/memory/disk bounds、per-arch helper署名を通すこととする。いずれかが失敗しても<code>Silent Transfer</code> 1.0を止めず、LANへfallbackしない。

### D7. M6-R manual encrypted recovery/archive

iCloud/OS backupを使わない唯一のfull recovery経路として、<code>.pkbvault</code>を<code>.pkbdelta</code>とは別magic/reader/meaningのprotocolとして設計する。SQLite/zip fileの単純コピーにはせず、delta packageをfull recoveryの代用にしない。

- magic/version、bounded canonical manifest、KDF descriptor、chunk count/size、vault IDをexact headerへ固定。
- approved password/recovery-key KDF＋AEADでchunk単位暗号化し、exporting actorがmanifest/transcriptを署名。
- canonical event、membership chain、migration receipt、wrapped contact-alias keyだけを含める。
- model、derived index、cache、log、DB key、device private keyを含めない。
- importはnative stream→bounded staging→KDF/signature/hash/schema/membership全検証→新しいdevice-local SQLCipher DBへtransaction適用するが、直ちにactive vaultへしない。
- wrong password、corrupt/truncated archive、同一import transaction内のreplay、既存anchorからのhead regression、zip-bomb相当のsize/count、path traversal、symlink、low disk、全crash boundaryを拒否。
- 新端末restoreは新actorとして**read-only <code>PendingVault</code>**へ入り、canonical edit、sync/export、derived rebuildを禁止する。既存vault merge/replaceの意味をinline画面で明示し、silent overwriteしない。

current admin Macが残る場合、network/transfer channelに依存しないmanual enrollmentを次の2-package ceremonyで行う。

1. 新端末native security routeがwriter actor key、delta recipient key、one-time enrollment keyを生成し、<code>PKB-JOIN-V1</code>、vault ID、archive root、membership head、actor/delta-recipient/enrollment public-key hash、nonce、expiryをactor keyで署名したbounded <code>.pkbjoin</code>をexportする。
2. current admin MacのAppKit <code>pkb-security</code>が<code>.pkbjoin</code>をnative importし、archive root/head、nonce/expiry、actor signature、current membershipを検証する。WebViewはrequest bytesを受け取らない。
3. user presence後、Mac AppKit signerがactor key＋delta recipient key hash＋writer roleを含むexact <code>actor_add</code> preimageだけへ署名する。Python repositoryがadmin public keyで再検証し、membership/pending receiptをsingle DB transactionでcommitしてnative anchorをfinalizeした後、membership prefix、enrollment receipt、contact-alias key envelopeを新device enrollment keyへbindingした<code>.pkbgrant</code>をexportする。
4. 新端末はnonce、actor/delta-recipient/enrollment key、archive root、sequence/previous hash、admin signatureを検証し、Python repositoryだけがgrantをcommitする。その後native anchorをfinalizeし、<code>PendingVault</code>をactiveへatomic切替する。

<code>.pkbjoin</code>/<code>.pkbgrant</code>はcanonical record本文を含めず、version/size/countを固定し、G0-Dで承認した標準KDF・key agreement・AEAD envelopeを使う。wrong package、expiry、replay、別vault、別actor/key、古いmembership head、grant前crashをhard failする。このceremonyにBonjour、socket、network XPCは不要であり、<code>Silent Transfer</code> baselineだけでMac→iPhone enrollmentを完結できる。

current admin Macからreplacement Macへの1.0 admin transferもnetworkへ依存させない。新Macが<code>.pkbvault</code>をPendingVaultへrestoreした後、WebView外AppKit ceremonyで次の4-round package protocolを行う。

1. 新Macがwriter actor key、candidate admin authorization key、enrollment keyを生成し、PendingVaultのarchive/canonical root、membership head、actor-have-vectorを含むuser-presence付き<code>.pkbadminreq</code>をexportする。
2. 旧Macはrequestのroot/head/vectorがcurrent vaultとexact一致することを検証する。不一致なら最新<code>.pkbvault</code>の再importを要求し、transferを開始しない。一致後はmaintenance gateで旧actor DeviceSignerと全writerをfreezeし、予定するordered <code>actor_add(new)</code>→<code>actor_retire(old, exact seq+head hash)</code>→<code>admin_transfer</code>のfields、current root/head/vector、nonce、challenge IDをdomain-separatedに署名した**非authorizing** <code>.pkbadminchallenge</code>をexportする。このchallenge単体をmembership eventとして受理しない。
3. 新Macはnative routeでexact proposalを表示し、candidate admin keyのuser-presence付き<code>.pkbadminaccept</code>をchallenge hashへbindingしてexportする。まだadminとしてactiveにしない。
4. 旧Macはacceptanceを検証・再表示し、user presence後にAppKit signerが3つのexact membership preimageへcurrent admin keyで署名する。Python repositoryが全signature/cutoffを再検証し、3 eventとpending receiptをsingle DB transactionでcommitしてnative anchorをfinalizeする。challenge/acceptance hash、contact-alias key envelope、membership chainを含む<code>.pkbadmingrant</code>をexportし、新MacのPython repositoryがexact chain/root/vectorを検証・commitしてatomic activateする。旧admin keyと旧Mac actor keyはretireし、cutoff後eventを拒否する。

旧Macがfinal commit後・grant export前にcrashしても、DB-first pending recordから同一grantだけを再exportできるようにする。final署名前のrequest/challenge/acceptanceはmembershipを変更せず、期限切れ/abandon時に安全に破棄してwriter freezeを解除できる。別root/head/vector、別challenge、acceptance再利用、途中のcanonical/membership変更、iPhone target、両端user presence欠落を拒否する。全packageはcanonical本文を含まず、join/grantと同じbounded encrypted envelope規律に従う。

KDF/AEAD/key custody、recovery phrase、contact-key envelope、archive rollback semanticsはG0-D ADRで確定する。自動OS backupはfallbackではない。設計・テストが完了するまで「端末紛失から復旧可能」と約束しない。

全admin private keyを失ったdisaster restoreでは、旧vaultへ未署名<code>actor_add</code>するfallbackを禁止する。1.0はreplacement Mac上で**新vault ID/genesisを作り、旧archive root hashと最終検証済みmembership headをprovenanceとしてcanonical recordを再発行する**方式へ固定する。one-time recovery authorityや複数adminは将来の別protocolとし、1.0 archive readerは受理しない。

既存DB/Keychain anchorまたはcurrent admin/peer witnessがある場合はarchive head regression、unknown branch、同sequence別hashを拒否できる。一方、全device・Keychain anchor・peer witnessを失ったfresh disaster restoreでは、古いが正当に署名されたarchiveが最新版かを判別できない。created timeは表示metadataに留め、freshness保証に使わない。この場合は「最終と確認できないarchiveからの新vault」と明示し、1.0の非保証として扱う。

### D8. 旧LAN Syncの降格

旧Bonjour/Network.framework/LNP/TLS/network-XPC設計は、検討履歴を失わないための**非出荷contingency ADR**としてのみ保管する。このmaster roadmapのactive architecture、milestone、release profile、acceptance、DoDには含めない。

- source/buildへ<code>lan-sync</code> feature、<code>_pkb-sync._tcp</code>、Bonjour browser/listener、<code>NSLocalNetworkUsageDescription</code>/<code>NSBonjourServices</code>、network entitlement、<code>pkb-sync-agent</code>を追加しない。
- <code>.pkbdelta</code>失敗、AirDrop拒否、Files provider不在、M6-U No-Goを理由にLANへ自動fallbackしない。
- 将来再検討する場合は、Commanderによる新directive、G0-B再開、mDNS/SRV/hostname/traffic metadataの新threat model、現行規律原文の明示改定、別binary/version、App Reviewを必須とする。
- 「LAN内だから安全」「random service nameなら沈黙」「TLSならmetadataも隠れる」という表現を禁止する。

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
| <code>COMMIT</code> | local durable save、verified delta import/Ack commit。静的status＋白borderで確定を示す | medium impact 1回 |
| <code>CEREMONY_SEAL</code> | consent ON、enrollment/member/admin/recoveryの最終確定。承認済みconsent/confirmed seal領域だけinverse | heavy impact 1回 |
| <code>FAIL_CLOSED</code> | 初回のterminal warning/error。静止inverse errorが最大視覚音量 | rigidまたはmedium impact 1回。反復なし |

- button down/clickではなく、reducerがsemantic state transitionの成功/失敗を確定した一度だけ発火する。re-render、duplicate event、retry、Ack再表示、cancelで再発火しない。
- visual/ARIA/VoiceOverが先であり、触覚は代替にもsecurity commit証拠にもならない。hardware/system setting/plugin failureは固定no-op <code>HAPTIC_UNAVAILABLE</code> Eventとし、操作結果を変えない。
- keypress、token、文字入力、scroll、animation frame、progress tick、camera preview、backgroundでは常に<code>SILENT</code>。
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

<code>platformEffects</code>はdomain/product stateを一切保持しない。現在のplatform、viewport、capability、permission、pending share、listener、haptic debounce等をmodule variable/cache/closure singletonへ置かない。Effect input以外から判断せず、injected immutable function tableである<code>PlatformPort</code>へ委譲し、portのraw <code>unknown</code> resultをeffect固有parserへ通したEventだけを返す。

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
- direct haptics、share/document picker、camera、LocalAuthentication call
- <code>localStorage</code>/<code>sessionStorage</code>/<code>indexedDB</code>、module mutable cache/queue/timer/retry

現行componentに残る直接Tauri/window/listener importはM5のFreeze Transition ADRへexact列挙して移す。grepだけでalias importを逃さず、dependency-free boundary harnessでcomponent→port/plugin importを拒否する。同一Event列から得るstate＋Effect列のbyte-equivalence、malformed raw resultのfixed rejection、repeated renderでeffect再発火0をacceptanceにする。

### E7. Native Ceremony Design Annex（F-A2）

WebView外のapp-owned UIKit/AppKit surfaceは「標準native UIだから例外」ではない。次のtoken mappingを単一native style ownerへ固定し、enrollment、delta export/QR Ack、membership/admin transfer、archive restore、recoveryの全ceremonyで共有する。

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
- SAS/role/peer alias/operation/package root/commit stateはnative verified objectから再表示し、WebViewのcopy/transcriptを署名・承認対象にしない。raw key/signature/path/actor IDを表示しない。
- BIZ UDGothic、VoiceOver順序、safe area、UIFontMetrics/Dynamic Type、Bold Text、Increase Contrast、Reduce Motion、44pt targetを全routeで守る。新しいinfinite animationを持たない。
- 実confirm後のsemantic transitionだけE5 grammarを発火し、派手なsystem notification hapticを使わない。

Face ID/Touch ID/passcode、camera/calendar permission、OS share sheet、Files pickerはOS所有であり、appが色・radius・fontを安全に上書きできない**trusted system exception**である。これらを独自に模倣、overlay、偽glyph、私製biometric promptとして再描画しない。app-owned pre-auth screenをAnnexどおり表示し、固定・非PIIの<code>localizedReason</code>から本物のsystem sheetを起動し、戻り先のpost-auth receiptもAnnexどおりにする。対応OS/APIでAppleのembedded authentication viewを採用できる場合も標準biometric iconを保持し、その周囲だけをstyleする。[Local Authentication Embedded UI](https://developer.apple.com/documentation/localauthenticationembeddedui)

visual acceptanceはOS sheet領域をpixel comparisonから明示除外し、その前後のapp-owned routeだけをWeb/native token parity screenshot、R=G=B scan、corner-radius/blur/system-blue scan、VoiceOver/Dynamic Type実機testへ通す。

### E8. Silent Terminalのモバイル解釈

- grayscale paletteを維持。
- bundled BIZ UDGothicを第一fontとし、承認済みlocal fallbackだけを使う。web fontをfetchせず、<code>@font-face</code>を黙って追加しない。
- radius 0、1px border、白glow限定。
- selection/error/consent ONだけinverse video。
- inferenceは現行仕様で許可済みのanimationだけを再利用する。delta prepare/import/Ack/wired transferはstatic status/determinate progressを基本とし、新規spinner/pulse loopを追加しない。
- permission denied、conflict、thermal stopを赤/黄/緑で表さない。
- OS-mandated permission/share/Files/Face ID sheetはAnnex準拠のapp-owned pre/post routeから明示操作の文脈でだけ誘発し、その上へapp独自overlay/modalを重ねない。それ以外の状態説明はscreen内ambient regionで行う。
- **OLEDの公式採用（F-A3）:** iOSではapp背景の基底を<code>--bg-deep: #000000</code>へ落とし、<code>viewport-fit=cover</code>の全面でOLED発光ゼロ領域とUIの境界を消す。notch/Dynamic Island/home indicator周辺が漆黒に溶け、端末の物理輪郭ごとSilent Terminalになることを意匠として明文化する。black smear（純黒上の灰文字スクロール残像）と、静的1px白罫・inverse video領域の焼き付き傾向はOLED実機受入（§12.6）で確認し、必要な調整は既存灰調トークンの範囲でだけ行う。有彩色・blur・radiusによる回避を認めない。
- **Launch Screenとprivacy shield（F-A6）:** iOS Launch storyboardとapp switcher privacy shieldは黒地（<code>#000</code>/<code>#0a0a0a</code>）＋静的グリフ1つ（例: <code>§</code>）で規定する。汎用blur、spinner、ロゴアニメーション、有彩色を使わない。shieldは本文・provenance・research状態を完全に覆い、復帰時にアニメーションで剥がさない。

## 10. 段階ロードマップ

3名体制の想定役割:

- Platform/Release: Tauri、Rust、Xcode、signing、native plugin。
- Core/Data: Python、embedded runtime、SQLCipher、migration、sync reducer。
- UI/Quality: React/CSS、accessibility、E2E、device matrix。LLM性能検証は全員で共有。

| Milestone | 期間目安 | 依存 | 主な成果物 | Exit gate |
|---|---:|---|---|---|
| M0 規律・脅威モデル / USB Spike | 3〜5週 | なし | ADR G0-A〜E/B'、Metadata Silence ADR、Freeze Transition ADR、Native Ceremony Annex、Haptic Grammar、font/license manifest、data map、acceptance matrix、PeerTalk/usbmux実機Spike | baseline無ポート裁定、M6-U provisional Go/No-Go、全delta/ADRの承認者署名 |
| M1 v2/iOS shell | 3〜5週 | G0-A/E | exact version pin、ios init、iOS/iPadOS 17 pin、platform config/capability、生成物方針、BIZ UDGothic/UIAppFonts/OFL、fake backend | desktop regression 0、minimum/latest iPhone＋iPad compat cold launch、WKWebView/UIKit font proof |
| M2 embedded engine spike | 6〜8週 | M1、G0-A | CPython 3.13 XCFramework、AppleFrameworkLoader packaging、NumPy per-module frameworks、project再生成、代表facade operation | 物理端末、marker/origin/codesign archive scan、golden parity |
| M3 canonical vault | 8〜12週 | M2、G0-D | repository seam、SQLCipher schema、cross-platform DeviceSigner reservation、artifact manifest再生成、single-admin membership/anchor、migration rehearsal | signer/DB crash matrix、plaintext/temp漏洩0、scoped rollback drill |
| M4 native LLM/embed/search | 8〜12週 | M2、G0-C | llama/Metal、384D provider、C++ static kernel、artifact manifest再生成、signed model artifacts | memory/thermal/embedder/quality gate |
| M5 mobile UI / effect boundary | 7〜10週 | M1、G0-E | safe area、keyboard、touch、stateless platformEffects/PlatformPort、Haptic Grammar、Native Ceremony base、Files/share/QR/custom EventKit、iPad compatibility mode | grep/import boundary、VoiceOver、Dynamic Type、native/Web visual parity、G0-E再freeze ACK |
| M6-T Portable Delta | 8〜12週 | M3、G0-B/D/E | <code>.pkbdelta</code> sealed package、recipient key、membership-first import、OS share/Files、81-byte QR/<code>.pkback</code> Ack、peer cursor、Mac companion | tamper/replay/metadata/crash/convergence/camera/provider tests、app-owned port 0 |
| M6-U wired session（条件付き、baseline非blocking） | 8〜12週 | M6-T、G0-B' provisional Go | pinned PeerTalk/usbmux adapter、USB-only Mac helper、<code>127.0.0.1</code> listener、same package/Ack core | USB-only/radio-zero/private-symbol/sandbox/notary/App Review evidence |
| M6-R encrypted archive / manual enrollment | 6〜8週 | M3、G0-D/E | <code>.pkbvault</code>、PendingVault、join/grant＋admin-transfer packages、recovery key、streaming import/export | tamper/scoped-rollback/enrollment/transfer/crash/low-disk tests |
| M7 parity・hardening | 6〜9週 | M3〜M6-T＋M6-R、Wired-enhancedはM6-Uも | native dependency/font/license最終freeze、clean再生成、全command parity、lifecycle、fuzz、performance、freeze hash closure | 選択profileのrelease acceptance matrix全通過 |
| M8 TestFlight/App Store | 4〜6週 | M7 | privacy/export、review notes、archive、TestFlight | review-ready sign-off |
| M9 post-release | 継続 | M8 | local diagnostics export、security response、migration policy | network drift 0、signed releaseのみ |

M3/M4/M5はM2のGo後に並行化できるが、Freeze Transition ADRで同一file/symbolを同時解凍しない。M6-T/M6-Rはcanonical storeのcutover方式が確定するまで開始しない。<code>Silent Transfer</code> baselineはM6-Uを待たず50〜76週、Wired-enhancedはM0 provisional Go後も別profile/versionとして58〜88週を目安にする。M6-UがNo-GoでもM6-T/M6-R/security reviewを省略せず、M2/M3 exitで再baselineする。

### 最初の30日

1. G0 ADR、Metadata Silence ADR、Freeze Transition ADR、Native Ceremony Annex、Haptic Grammarを作成・承認。
2. exact current toolchain inventoryとv2 migration audit。
3. macOS release hostで<code>tauri ios init</code>。
4. generated Apple projectをdisposableにするnative dependency/template/privacy-manifest方針を固定。
5. base configからdesktop sidecar設定を分離。
6. fake in-process backendでiPhone simulatorに既存React shellを表示。
7. CPython 3.13＋NumPy iOS minimal framework spike。
8. 代表operationを3つ選び、3.12 desktopとのgolden corpusを凍結。
9. 7B modelのarchive/RAM budgetを机上計算し、実機benchmark planを固定。
10. canonical/derived/security data inventoryを確定。
11. <code>.pkbdelta</code> outer metadata、membership-first import、81-byte signed Ack、app-owned port 0のnegative test仕様を先にREDで追加。
12. PeerTalk/usbmuxをM0でtime-boxed Spikeし、USB-only connection type、<code>127.0.0.1</code> bind、radio packet 0、sandbox/private API/App Review feasibilityをGo/No-Go判定。
13. BIZ UDGothic Regular/Boldのsource/tag/hash/OFLを固定し、<code>UIAppFonts</code>＋WKWebView/UIKit実機proofを作る。
14. component/TitleBar/engineのdirect Tauri/DOM API importをinventoryし、M5 thaw allowlistとPlatformPort grep/import gateをRED化。
15. active <code>embedder_id</code>とembedding model/runtime artifactをinventory。

### 60〜90日

- embedded Pythonでhealth、record、consultation smoke。
- <code>_pkb_native</code>のLLM/search prototypeと384D embedding provider spike。
- SQLCipher schema、legacy dry-run import、migration receipt。
- safe area/titlebar/tablist/composer、bundled font、stateless effect boundary、native ceremony token mappingのmobile proof。
- <code>.pkbdelta</code> sealed writer/reader、OS share/Files、QR Ackのnetworkless prototype。M0 USB SpikeがGoならM6-U詳細設計をfreezeし、No-Goならartifactをbaselineから除去。
- 7Bと候補iOS roleの品質/性能比較。
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
- Guideline 2.1向けにfull-access review手順、必要なMac companion、sample enrollment/delta fixture、sample modelを用意する。
- Guideline 4.2向けに、on-device inference、encrypted vault、OS-native Files/share/QR Ack/EventKit/haptics/native ceremony、optional wired sessionが単なるrepackaged websiteを超えるnative utilityであることを実機evidence付きdossierにする。

### 11.2 privacy

- <code>PrivacyInfo.xcprivacy</code>をbundleへ含める。
- Tauri、Rust crates、Swift packages、CPython、NumPy、llama、SQLCipherのrequired-reason APIを棚卸し。
- App Privacyは「developerが受信するデータ」をApple定義に照らして正確に回答。
- P2P dataを「非収集」とする扱いは、developer/partnerが受信・アクセスしないというApple定義からの推論として法務確認する。
- App Store Connect metadataへPrivacy Policy URLを設定し、app内にも容易に到達できるlocal/offline policy viewとURLを置く。
- privacy policyにon-device保存、recipient-bound <code>.pkbdelta</code>、ユーザー選択のAirDrop/Files/provider、QR Ack、provider上の暗号文copy/時刻/padded size残余、logical delete、peer revokeの限界、developer非アクセスを記載する。Wired-enhancedではUSB-only loopback sessionとusbmux metadata限界を追加する。
- analytics/crash telemetry/広告IDを入れない。
- Xcode archive privacy reportを確認し、Apple指定third-party SDKのmanifest/binary signatureを検査する。OpenSSL等がtransitiveに入る場合も対象とし、可能ならApple system crypto/CommonCrypto構成を優先する。

Appleの[privacy manifest](https://developer.apple.com/documentation/bundleresources/adding-a-privacy-manifest-to-your-app-or-third-party-sdk)、[required-reason API](https://developer.apple.com/documentation/bundleresources/describing-use-of-required-reason-api)、[App Privacy](https://developer.apple.com/app-store/app-privacy-details/)をreleaseごとに再監査する。

### 11.3 export compliance

SQLCipher、TLS、署名、Keychain/Secure Enclaveを含むため、Appleのexport questionnaireと法務判断をrelease gateにする。[Export compliance overview](https://developer.apple.com/help/app-store-connect/manage-app-information/overview-of-export-compliance/)

OS暗号だけか、第三者暗号実装を含むか、配布国、免除条件を記録する。根拠文書なしに<code>ITSAppUsesNonExemptEncryption</code>を固定しない。

### 11.4 background / energy

- LLM background executionを宣言しない。
- baselineはforeground/backgroundを問わずnetwork listenerを持たない。Wired-enhancedのloopback listenerも明示foreground session外では存在しない。
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

- account/cloud/internet不要で、<code>Silent Transfer</code>はapp-owned network path/port/Local Network permissionを持たず、encrypted <code>.pkbdelta</code>をsystem share/Filesで移送してnative QRでAckすること。
- AirDrop/Filesはユーザーが選ぶOS-owned channelであり、share sheet完了をsync commitと扱わず、受信commit後のsigned Ackだけでcursorを進めること。review用のMac↔iPhone往復fixtureを用意する。
- <code>.pkbjoin/.pkbgrant</code>、<code>.pkbvault</code>、replacement-admin手順とreview用fixture。Wired-enhancedを提出する場合だけ、physical USB、<code>127.0.0.1</code>、no Bonjour/LNP/radio packet、両端arm手順を追加説明する。
- <code>UIDeviceFamily=[1]</code>のiPhone-optimized appである一方、iPadOS compatibility modeでも同等機能を提供することと、そのreview手順。
- modelの同梱/import方式。
- backgroundで推論、provider polling、QR camera、listenerを動かさないこと。
- ユーザーデータをdeveloperが受け取らないこと。

## 12. 検証・受入マトリクス

### 12.1 architecture / egress

- WebViewからHTTP/WebSocket/DNSが0。
- Pythonからnetwork import/socket creationが0。
- llama buildにserver/RPC/network symbolがない。
- <code>Silent Transfer</code> archive/app processに<code>socket/bind/listen/connect</code>、Network.framework、Bonjour、MultipeerConnectivity、network XPC owner/success pathが0で、runtime port scanとpacket captureもapp-owned network byte 0。
- 全App Store profileで旧<code>lan-sync</code>、<code>_pkb-sync._tcp</code>、LNP/Bonjour plist key、network client/server entitlement、<code>pkb-sync-agent</code>が0。
- baselineは<code>egress-live</code>/<code>lan-sync</code>/<code>wired-sync</code>がoffで、transport owner/symbol/capability/permission/success pathがIPAとartifact manifestに0。parity用egress commandはfixed refusalだけ。
- Wired-enhancedは<code>egress-live</code>/<code>lan-sync</code> off、<code>wired-sync</code>だけon。exact native callsite以外のsocket symbol 0、bind address <code>127.0.0.1</code>以外0、system usbmux AF_UNIX以外のMac connect 0、USB connection type以外拒否、radio interface packet 0。
- <code>wired-sync</code>と<code>egress-live</code>/<code>lan-sync</code>の同時buildが失敗し、baseline artifactへPeerTalk/usbmux/helperが混入しない。
- system share/Files pluginはpackage bytes/path/provider URLをWebViewへ返さず、任意network APIを所有しない。share cancel/successをdelivery/commit証明にしない。
- CSPにLAN/loopback host/IP/wildcardを追加していない。
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
- repeated render/duplicate/cancelでshare、QR、native ceremony、hapticの再発火0。observer START/STOP/unmount/backgroundでopaque handle leak 0。
- unknown key/type/oversizeを両backendで同じ固定errorにする。
- CID、token/status/final order、cancel semanticsをgolden比較。
- desktop Python 3.12.10とiOS Python 3.13.xのcanonical hash、ranking、profile、promptを比較。
- C++ executable/static kernelのtop-k parity。
- active 384D embedderのvector/normalization/top-k/downstream parityと<code>embedder_id</code>再構築を検証。
- canonical commitと<code>required_generation_id</code> dirty markerが同transactionで、stale manifest/旧index queryは固定<code>DERIVED_STALE</code>となり結果を返さない。
- rebuild stagingのsource-head/embedder/generator/hash/count/dimension検証、atomic manifest swap、各instruction crash/cancel/low-disk後のdirty維持とorphan cleanup。
- <code>simulated</code> true/false、self/contact、subjective/objectiveの対照corpusでmigration/sync後の分類とweightが一致。
- LLM provenance documentがauthority state/profile/future promptへ自動昇格しない。
- desktopの現行全Python/Rust/TypeScript suiteを維持し、件数はCI collectで動的記録する。
- iOS未実装commandをruntime surpriseにせず、release前に全parityを満たす。

### 12.3 storage / migration

- wrong key、corrupt page、truncated WAL、low diskでhard fail。
- DB/WAL/tempのplaintext scan。
- release SQLCipher compile optionsとruntime <code>temp_store=MEMORY</code>を実体検査。
- legacy malformed/duplicate/huge record。
- dry-runの副作用0。
- migration中crashの各instruction boundary。
- migration receipt/hash/count一致。
- final staging candidateのgenesis/oplog/membership root＝archive root＝independent restore root＝atomic cutover root。
- cutover後にlegacy reader/writerが無効で、app-managed legacy tree/<code>.bak</code>/plaintext quarantineが0。Application Support/Documents/tmp/staging/app-owned exportのcanary scanが0。
- APFS/SSD/snapshot/Time Machine/user-owned sourceのphysical erasure非保証がmigration receipt/UI/threat modelと一致。
- M6-R manual archive export/restoreとpeerからのrebuild drill。OS backupをrecovery pathとして扱わない。
- DB commit/Keychain anchor更新の全crash boundary、DB-ahead recovery、Keychain-ahead rollback拒否。
- DeviceSignerのDB reservation前後、native pending/sign前後、response loss、final DB commit前後、anchor finalize前後、restart/duplicateを網羅し、same pending signature再利用、sequence gap/fork/二重materialize 0。
- macOS private stdio reverse signerとiOS <code>_pkb_native</code>のexact logical fixture、arbitrary-sign request/unknown domain/size/CID/state拒否、WebView/share/QR/wired helperからのsigner到達0。
- admin membership signerはsignatureだけを返し、PythonだけがSQLCipher commitすることをconnection-owner/instrumentation testで確認。
- 3-event membership batchのDB/native reservation、各署名、response loss、atomic DB commit、anchor finalize全境界でcrashし、partial membership prefix 0、同じsignature batch再利用、final head一致。
- delta manifest/AckのDB reservation、native exact sign、Python public verification、sealed handoff、inbound commit、anchor finalize、Ack receipt commitの全crash boundary。same handle/different preimage拒否、81-byte QR以外拒否、share/QR/wired helperのSQLCipher connection 0。
- protected data unavailable時のDB close。

### 12.4 sync（Silent Transfer baseline）

- IPA/app processのport/listener/browser/connect/DNS/mDNS/Bonjour、Local Network keys/prompt、network entitlement/XPCが0。airplane mode、Wi-Fi off、Mac不在でもpackage export/import/QRが成立する。
- outer headerとrandom filenameにvault/actor/device/user/record名、head/vector/count/range/time、stable recipient hintが0。ciphertext lengthは承認bucket一致で、同じlogical payloadのpadding試験を通る。
- malicious Files provider/share extension、wrong destination、provider URL差替え/TOCTOU、truncate、chunk reorder、duplicate、replay、oversize、unknown version/suite、AEAD/signature/hash破損でplaintext leakage/DB副作用0。
- share cancel、OS share success、app termination、provider unavailableをdelivery/commitと扱わず、verified Ack前にpeer cursorを前進しない。
- outbound/inbound state machineの全instruction、chunk、DB transaction、Keychain anchor、temporary-file cleanup境界でcrash/restartし、partial materialization 0、same reservation/idempotent resume。
- wrong recipient key、retired recipient、unknown vault、missing baseはfixed error。<code>.pkbdelta</code>を<code>.pkbvault</code> readerへ渡して相互拒否する。
- membership base/head→ordered prefix validation→membership transaction→actor vector/range→data transaction→anchor→Ackの順序をmanifest/import traceで固定し、data-first/unknown-head packageのDB副作用0。
- data eventの署名済みmembership sequence/head reference、old-prefix actor validity、retire cutoff、package内membership変更後のordered new-head reference。
- actor-have-vectorの<code>(contiguous_seq, head_hash)</code>比較。同seq別hash即quarantine、seq差のancestor/first-prev連結、signed vector/declared range改変を拒否。
- retired actorのadmin署名済みexact cutoff <code>(actor_id, seq, head_hash)</code>、alternate branch、cutoff未到達のstaging非materialize、cutoff超過拒否。
- actor hash-chain fork/equivocation、DB-only snapshot rollback、peer-visible fork、unknown actor、membership chain欠落。DB＋Keychain＋全peerのcoherent rollbackは非保証としてreportへ明記する。
- single-admin membershipのsequence/previous-hash gap、同sequence分岐、actor add/retire、device replacement、third-actor relay、複数admin/旧admin/iOS-admin違反、new actor add→old actor exact-cutoff retire→replacement Mac admin transferの順序。
- <code>.pkbjoin/.pkbgrant</code>でactor key、delta recipient key、role、nonce、archive root/headがbindingされ、初期iPhone writerにadmin private keyが存在しない。admin transfer後は旧admin keyを拒否する。
- compromised WebViewからexport/import/Ack commandを呼んでも、native recipient/scope再表示＋明示confirmなしにpackageをOSへ渡せず、package bytes/path/key/signature/QR payloadを取得できない。share/QR/wired adapter単独にadmin/actor arbitrary-sign能力がない。
- QRはexact 81 byte、single-use handle、recipient actor signatureを検証する。wrong length/version/handle/package/actor/signature、screenshot replay、duplicate scanでcursor誤前進0。
- camera allow/deny/Settings変更/background/cancel、frame retention 0、Photo Library/microphone permission 0。camera unavailable/iPad compatibility modeではsame receiptの<code>.pkback</code> fallbackをnetworkなしで完遂する。
- A→B delta、B→A reverse delta＋piggyback Ack、final QR Ackの双方向flow。逆方向data 0、Ack loss、package loss、duplicate deliveryでも収束する。
- 同時編集、sibling conflict、tombstone、peer retire/revoke、event permutation/duplicate/interruptionのproperty test、rejection時DB副作用0。
- N端末での収束性。1.0で2 active端末だけを正式supportしてもprotocol testは3 actorと過去retired actorを含める。
- Ackはreceipt/cursorだけを更新し、oplog physical GC/唯一copy削除を起こさない。malicious recipientの虚偽Ackという限界を明記する。

### 12.4.1 wired session（Wired-enhancedのみ）

- minimum/latest iPhone、mandatory iPad compatibility mode、arm64/x86_64 Macで、Wi-Fi/Bluetooth/cellular offのUSB往復と、on時の全radio interface PKB packet 0。
- iOS bind/listenはnumeric <code>127.0.0.1</code>＋fixed portだけ。wildcard/IPv6/hostname/DNS/Bonjour/LNP prompt 0、backlog/session/accept各1、arbitrary host/port command 0。
- Macはsystem usbmux AF_UNIX＋USB connection typeだけ。network lookup/netmuxd/Wi-Fi sync/tethering/first-device fallbackを拒否し、複数deviceはfixed ambiguity error。
- cable attach/detach、background、両端cancel、timeout、crash、wrong USB host、loopback probe、frame fuzzの全境界でbounded close、partial DB effect 0、新しい両端armなしの再listen 0。
- PeerTalk pinned source/hash/MIT/SBOM、private-symbol 0、Mac sandbox/temporary-exception decision、Developer ID per-arch signing/notary、iOS archive/static analyzer/App Review dossierがGREEN。
- <code>pkb-wired-agent</code>が必要な場合、vault/model/key filesystem、actor/admin signer、arbitrary sign/decrypt/key export、IP network entitlementが0で、fixed parent IPC/team requirementを通る。
- same <code>.pkbdelta</code>/Ack bytesとmembership-first coreをportable/wired cross-fixtureで一致させ、第2のmerge/conflict/codecがない。

### 12.4.2 encrypted archive

- wrong recovery key/KDF params、tamper、truncation、同一transaction内のduplicate/replay、existing anchorからのhead regression。
- max header/chunk/count/total bytes、path traversal、symlink、decompression bomb相当。
- model/derived/log/device private keyがarchiveに含まれない。
- contact-alias key envelopeとmembership chainのbinding。
- staging/import/DB commit/atomic cutoverの全crash boundary。
- existing vaultへのmerge/replaceが明示選択どおりで、silent overwrite 0。
- import直後はread-only PendingVaultで、grant前のedit/sync/export/derived rebuildが0。
- <code>.pkbjoin/.pkbgrant</code>のwrong vault/actor/enrollment key、nonce再利用/期限切れ、old membership head、admin signature改変、grant前後の全crash boundary。
- replacement Macの<code>adminreq→adminchallenge→adminaccept→admingrant</code>全round、challenge単体の非authorizing性、両端user presence、PendingVault/current root＋membership＋actor-vector exact一致、途中write freeze、ordered new-actor add＋old-actor exact-cutoff retire＋admin transfer、iPhone target、replay、final commit前後のcrash/re-export。
- <code>Silent Transfer</code> buildでapp-owned network/XPCを使わずMac admin commit→manual package transfer→iPhoneおよびiPad compatibility mode activationを完遂。
- <code>Silent Transfer</code> buildでapp-owned network/XPCを使わず旧Mac→replacement Mac admin transferを完遂し、旧admin key、旧Mac actor cutoff後event、iOS admin eventを拒否。
- 全admin喪失時は旧vaultへのactor追加を拒否し、replacement Mac上のnew vault/genesis＋旧root provenanceだけを許可。
- existing anchor/admin witnessがあるvalid-old archiveはhead regressionとして拒否し、witnessのないfresh disaster restoreではfreshness非保証を明示してnew vaultへ分岐。

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
- text input/scrollがdelta prepare/import/Ack、optional wired transfer、inference中も利用可能。
- BIZ UDGothic Regular/Boldの日本語/Latin/full-width representative corpus、WKWebView/UIKit/AppKit同一family/metrics、font fallback 0、line-height 1.6、最大Dynamic Type横clip 0、network font request 0。
- Haptic Grammar全tokenのsemantic transition exactly-once、visual/ARIA併用、device/system haptics off、hardwareなし、background。notification/vibrate/multi-pulse permission/call 0。
- Native Ceremony Annexの全routeで<code>#0a0a0a</code>/<code>#f2f2f2</code>写像、R=G=B、radius/blur/system-blue/stock alert 0、BIZ UDGothic、VoiceOver、44pt。OS所有Face ID/permission/share/Files sheetはtrusted exceptionとして前後surfaceだけをvisual compareし、私製biometric模倣0。
- <code>platformEffects</code>のstateless/import/grep/re-render/observer cleanup gate。componentからVisualViewport/Tauri/plugin/native API直接到達0。
- iPad compatibility modeでFiles/share、QR/<code>.pkback</code>、<code>.pkbdelta</code>、EventKit、Keychain/Data Protection、backup exclusion、archive/enrollmentがiPhoneと実質同等。Wired-enhancedは物理USBも同等。
- 44×44が透明hit-area拡張で満たされ、視覚密度（1px罫・台帳行高・タブ寸法）がdesktop規定から肥大していない。
- OLED実機（notch/Dynamic Island世代）でblack smearの許容確認、静的1px白罫/inverse video長時間表示の焼き付き傾向確認、<code>#000</code>基底とsafe-area外周の連続性確認。
- Launch Screenとapp switcher privacy shieldがE8規定（黒地＋静的グリフ、blur/spinner/有彩色0）に一致し、shieldが本文を完全被覆する。
- monochrome/radius/border/glow/native-token parityのvisual regression。

## 13. Threat modelと限界

守る対象:

- 紛失・盗難端末上のat-rest data。
- malicious Files provider/share extension/外部mediaによる<code>.pkbdelta</code>の読取・改変・差替え・TOCTOUと、wrong recipientへの移送。
- replay、duplicate、malformed/oversize package/frame、偽sender/retired actor、Ack偽造・再利用。
- DB/operation log改竄、DB-only snapshot rollback、peer-visible actor fork/equivocation。
- WebView compromiseからの任意network/file/provider/path/model操作、package/QR bytes取得と、native user-presenceを経ないexport/membership/admin操作。
- Wired-enhancedではmalicious USB host、network-connected usbmux deviceへの誤接続、loopback session probe、cable interruption。
- log、snapshot、backupによる漏洩。

1.0で守れないもの:

- unlock済み端末またはOS自体が侵害された状態。
- 正式にenrollされ、復号済みデータを受け取ったpeerの悪用。
- 画面を物理カメラで撮影されること。
- OS share/providerが見る<code>.pkbdelta</code>拡張子、random filename、転送時刻、padding bucket size、送受信の事実。AirDropがOS levelで使う端末表示名、近接性、radio/discovery metadataはPKBの制御外であり、app-owned advertisement 0と端末全体のRF silenceを同一視しない。
- ユーザーが選んだiCloud Drive/third-party Files/AirDrop受信先/外部mediaに残るrecipient-bound暗号文copyの保持・削除。
- QR Ackを第三者が撮影すること。payloadは81-byte opaque signed receiptで本文/stable IDを持たずreplay-safeだが、受領操作の時刻・存在は視認され得る。
- malicious enrolled peerが自身のactor keyで虚偽Ackを署名すること。Ackを相手storageの証明やoplog GCへ使わないことで破壊的影響を防ぐ。
- Wired-enhancedにおいてsystem usbmux/OSが扱う一時的device handle/USB metadata、またはphysical cable相手が正当peerであるとの保証。actor/membership/envelope検証は省略しない。
- 端末紛失後のremote wipe。
- revoke前にpeerへ渡ったcopyの回収。
- logical delete後の暗号化event historyからのphysical erasure。
- Wired-enhancedのiOS同一process内で、embedded PythonだけをOS-level loopback sandboxへ隔離すること。Silent Transfer baselineにはapp-owned socket自体がない。
- DB、Keychain anchor、全peer状態を整合して同じ旧snapshotへ戻すcoherent rollbackの検出。外部witness/hardware monotonic counterを持たない1.0では保証しない。
- 全device/Keychain/admin/peer witness喪失後、古いが正当に署名された<code>.pkbvault</code>が最新版かをfresh disaster restoreだけで判定すること。
- migration前に書かれたAPFS/SSD block、OS snapshot、Time Machine/外部backup、ユーザー所有の元sourceをphysical secure eraseすること。app-managed live namespaceのlogical cleanupだけを保証する。
- compromised WebViewがallowlisted export/import ceremonyの開始要求を繰返してavailabilityを害すること。native routeはverified recipient/scopeを再表示し明示confirmなしにartifactを外へ渡さないため、開始要求だけをhuman intentやexfil authorizationにしない。

「zero trust」はOS transfer channel、peer、package、Ack、cableを常に検証し、recipient-bound encryption、membership、最小権限、署名eventを使うという意味であり、enrolled peerやuser-selected providerから既存copyを消せるという意味ではない。

iOSのPython import/native API closureは攻撃面を縮小するdefense in depthであり、macOS sidecar sandboxと同等のOS境界ではない。Silent Transferではnetwork capabilityをappから除去してこの差を主同期から切り離す。G0-A/B'の残余loopback riskを受容できなければM6-Uだけを出荷せず、baselineを維持する。

## 14. リスク順位と停止条件

| Risk | 影響 | 早期検証 | 停止/分岐 |
|---|---|---|---|
| CPython 3.13/NumPy iOS build | 全business core | M2実機spike | 失敗時は規律変更判断へ戻る |
| iOS in-process Python＋wired loopback | egress憲法 | M0 Spike、G0-A/B'、archive closure audit | 受容不可/証明失敗ならM6-UだけNo-Go、baseline維持 |
| 384D embedding provider drift | retrieval/parity | M2/M4 corpus＋top-k | silent fallback禁止、M4停止 |
| 7B package/memory/thermal | local LLM | M0〜M4先行benchmark | 7Bまたは承認済みiOS model roleが全eligible device gateを通らなければ1.0 No-Go。silent fallback禁止 |
| canonical store移行 | 全データ | record inventory＋shadow compare | loss/ambiguityが1件でも未処理ならcutover禁止 |
| <code>.pkbdelta</code> envelope/provider metadata | confidentiality / Silent Terminal | outer-header review、malicious-provider/TOCTOU/padding tests | stable ID/plaintext漏洩またはbounded streaming不能ならM6-T停止 |
| QR Ack/cursor defect | 再送・整合性 | 81-byte exact parser、signature/crash/replay tests | Ack前cursor前進/誤GCが1件でもあればrelease停止 |
| PeerTalk/usbmux/private API/sandbox | Wired-enhanced不可 | M0 current-device archive/App Review Spike | private API、USB識別不能、radio byte、承認不能exceptionならM6-U Kill。LAN fallback禁止 |
| actor trust/rollback設計 | sync integrity | single-admin membership/hash-chain/scoped rollback/crash tests | fork/anchor不整合でrelease停止、coherent rollbackは非保証を明記 |
| sync protocol defect | confidentiality/integrity | threat model＋property/fuzz | full resyncで隠さずrelease停止 |
| SQLCipher/license/export | release不可 | M0 legal/SBOM | 解決までarchive/upload禁止 |
| BIZ UDGothic/WKWebView resolution・OFL | 美学/法務 | M1 physical-device font proof＋license manifest | fallback、hash/license欠落ならG0-Eへ戻りM1停止。<code>@font-face</code>無断追加禁止 |
| stateful <code>platformEffects</code> / raw API drift | pure-function憲法 | import/grep/effect determinism gate | direct access/state/cache/timerが1件でもあればM5再freeze不可 |
| UI.D freeze/Annex未承認 | 規律違反 | G0-E＋Freeze Transition ACK | exact thaw外を変更せず、再freeze ACKまで次milestone停止 |
| App Store 2.5.2/4.2 | rejection | early review dossier | model/runtime説明・native valueを再設計 |
| iOS lifecycle/jetsam | data loss/crash | physical device matrix | supported device/paramsを狭める |
| desktop regression | 現行要塞を毀損 | backend compile-time isolation | desktop suite不合格ならmerge禁止 |

## 15. Definition of Done

App Store 1.0 baselineは15.1＋15.2＋15.3、将来のWired-enhanced profile/versionは15.1＋15.2＋15.4をすべて満たした時だけ「完了」とする。旧LAN SyncにはDoDを与えず、出荷候補にしない。

### 15.1 共通DoD

- 現行7 tabと全release対象commandがiOSで意味論一致。
- Pythonがbusiness/prompt/data ruleのsource of truthであり続ける。
- llama/searchがin-processで、子プロセス・localhost serverがない。
- active 384D embedderが署名済みで、desktop/iOSのquality/parity gateを通る。
- G0-Cを通過したproduction GGUFを1件以上bundleし、signed model policyを外れたmodelを実行しない。
- canonical dataがSQLCipher＋Data Protectionで暗号化され、plaintext WAL/tempがない。
- 全canonical event/membershipがcross-platform DeviceSigner reservationを通り、Python-only DB commit、signature再検証、two-slot anchor、arbitrary-sign拒否のcrash matrixを満たす。
- canonical DBを含む全PKB管理fileがOS/iCloud Backup対象外で、app-managed legacy plaintextのlogical cleanup、migration receipt、M6-R recovery/scoped rollback drillが通る。
- M6-R encrypted archiveのtamper/crash recovery gateが通る。
- single-admin membership、<code>.pkbjoin/.pkbgrant</code> enrollment、4-round replacement-Mac admin transfer、conflict、保証対象rollbackがnative user-presence/tamper/crash testを通る。
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

### 15.2 Portable Delta共通DoD

- recipient-bound <code>.pkbdelta</code>、manifest/event signatures、membership-first import、padding、malicious-provider/TOCTOU、crash/resume、conflict/revoke、保証対象rollbackがM6-T matrixを通る。
- outer header、filename、OS share metadataにapp-controlled PII/stable vault/device/actor IDが0。AirDrop/Files/providerの残余metadataと暗号文copy限界をUI/privacy/Review Notesへ記載する。
- share/provider successではなく、commit後のexact 81-byte recipient actor-signed QR/<code>.pkback</code> Ackだけでcursorを進める。Ack loss/replay/虚偽の限界がoplog GCやdata lossへつながらない。
- package bytes/path/key/signature/QR payloadがWebViewへ0で、native recipient/scope再表示＋明示confirmなしのexport 0。Mac/iPhone/iPad compatibility modeの双方向deltaが収束する。
- <code>.pkbvault</code>移送、<code>.pkbjoin/.pkbgrant</code>加入、replacement-Mac admin transferをapp-owned networkなしで完遂し、wrong/stale/replayed packageと全admin喪失時のnew-vault分岐を検証する。

### 15.3 Silent Transfer profile追加DoD

- <code>egress-live</code>/<code>lan-sync</code>/<code>wired-sync</code>がcompile offで、socket/listener/connect/DNS、Network.framework/Bonjour/LNP、network XPC、PeerTalk/usbmux helper、network entitlement、<code>NSLocalNetworkUsageDescription</code>/<code>NSBonjourServices</code>がarchiveに0。parity commandはinert fixed refusalだけ。
- runtime port scan、radio/non-radio packet capture、Mach-O/native symbol/capability/permission scanでapp-owned network byte/success pathが0。
- App Store説明は「自動LAN sync」と訴求せず、ユーザー操作のencrypted portable delta＋signed receiptとして正確に説明する。

### 15.4 Wired-enhanced profile追加DoD

- G0-A/B'の同一process loopback残余リスクと有線限定例外が正式承認され、<code>egress-live</code>/<code>lan-sync</code> off、<code>wired-sync</code>だけonである。
- iOSはnumeric <code>127.0.0.1</code>＋fixed port以外のlistener 0、Macはsystem usbmux AF_UNIX＋USB connection type以外0。Bonjour/LNP/DNS/radio packet/fallback transportが0。
- pinned PeerTalk source/license/SBOM/private-symbol、Mac helper least privilege/per-arch signing/sandbox/notary、current iOS archive/App Review evidenceがGREEN。
- 両端explicit arm、native ceremony、sealed portable package、signed Ack、unplug/background/crash/fuzz、bounded closeが全実機matrixを通り、M6-U failure時のLAN fallbackが0。

## 16. 明示的な非目標

- Android同時対応。
- native iPad universal target/layout最適化（iPadOS compatibility modeの同等機能・試験は必須）。
- React Native、Flutter、Electronへの再プラットフォーム。
- Tailwind、UI kit、外部state manager。
- PKB-managed cloud/OS backup・sync、web account、subscription backend。復旧はM6-R manual encrypted archiveだけ。
- LAN Sync、Bonjour/mDNS discovery、Local Network permission、app-owned Wi-Fi/AWDL/cellular connection。旧案は非出荷backup ADRだけ。
- background常時sync、push wakeup、provider polling、background listener。
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
- [Apple UIActivityViewController](https://developer.apple.com/documentation/uikit/uiactivityviewcontroller)
- [Apple UIDocumentPickerViewController](https://developer.apple.com/documentation/uikit/uidocumentpickerviewcontroller)
- [Apple AVFoundation QR symbology](https://developer.apple.com/documentation/avfoundation/avmetadataobject/objecttype/qr)
- [Apple camera authorization](https://developer.apple.com/documentation/avfoundation/requesting-authorization-to-capture-and-save-media)
- [Apple UIImpactFeedbackGenerator](https://developer.apple.com/documentation/uikit/uiimpactfeedbackgenerator)
- [Apple Local Authentication Embedded UI](https://developer.apple.com/documentation/localauthenticationembeddedui)
- [PeerTalk source](https://github.com/rsms/peertalk)
- [libimobiledevice usbmuxd](https://github.com/libimobiledevice/usbmuxd)
- [Apple App Sandbox temporary exceptions](https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/AppSandboxTemporaryExceptionEntitlements.html)
- [Apple XPC services](https://developer.apple.com/documentation/xpc/creating-xpc-services)
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

この文書は規律変更そのものではない。G0の承認、Metadata Silence/Freeze Transition ADR、Native Ceremony Annex、Haptic Grammar、font/license manifest、原文更新、RED constitutional test、UI ACKが揃うまで、embedded runtime、canonical store、iOS model role、Portable Delta、mobile UI deltaは提案状態に留まる。M6-UはさらにG0-B'とM0 USB Spike evidenceが揃うまで提案にも着手せず、旧LAN apertureは非出荷backupのままとする。
