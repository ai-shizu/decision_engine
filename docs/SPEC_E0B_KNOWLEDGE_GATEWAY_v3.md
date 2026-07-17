# SPEC — Feature E0b v3: Zero-Trust External Knowledge Gateway (Audit-Grounded)

> **状態:** ブループリント v3（第 2 次独立監査 REJECT・全 14 findings 対応・指揮官 ACK 待ち・実装未着手）
> **置換対象:** v1 / v2（本書が唯一の正本。v1・v2 は歴史的参照のみ）
> **本改訂の性格:** v2 は監査指摘の方向性（dual-run 等）を先取りしていたが、**実コードの挙動を未検証のまま
> 「解決済み」と主張していた**。v3 は監査が指摘した実コード事実を本職が直接検証した上で接地する。
> **検証済みの実コード事実（監査主張の裏取り、いずれも TRUE）:**
> - [engine.rs:447](../apps/desktop/src-tauri/src/engine.rs) `invoke()` は `spawn_blocking` + メッセージ毎 `recv_timeout(IPC_IO_DEADLINE)` → **キャンセル不能・heartbeat 延長可能**（F8/F9 の前提は正しい）
> - [consultation_engine.py:412](../src/python/core/consultation_engine.py) `load_knowledge_chunks()` は `^##\s+` でファイルを**別チャンクへ分割**し、[:940](../src/python/core/consultation_engine.py) は `knowledge_hits` を**直接プロンプト連結**（F11 は正しい。ファイル先頭ヘッダは本文と分離する）
> - [engine_stdio.py:232](../src/python/engine_stdio.py) は `f"{type(exc).__name__}: {exc}"` を応答へ格納（F14 は正しい。例外値が UI へ到達し得る）
> - [privacy_search.py:35](../src/python/core/privacy_search.py) `_normalize_for_matching` は NFKC+casefold のみで**ゼロ幅・Bidi を除去しない**（F5/F12 の回避は成立する）

---

## 0. v3 の 2 つの中核転換

1. **HMAC の意味論を「実行証明」から完全に切り離す（F1 の根本受容）。**
   CPython 内制御フロー証明はエンクレーブなしに不可能。egress 可否は **Rust 境界の独立再検査**が単独で決める。
   attestation は「Txn 同一性・鮮度・辞書 revision 束縛・完全性」のみを担い、**PII 判定には一切寄与しない**。
   → 「PII クエリから signer を直接呼ぶ」攻撃は、Rust が同じクエリを自前で拒否するため egress ゼロ（証明不要）。

2. **canonicalization を egress パイプラインの単一の起点に置く（F5/F6/F12/F13 の統合解）。**
   クエリは E0b 入口で**ただ一度** canonical 化（不可視除去 → NFC → 制御文字 fail-closed）され、以後
   **署名・PII 判定・provider 送出のすべてが同一 byte 列**を使う。「検証した列と fetch した列が違う」を構造排除。

---

## 1. 全 14 findings → 解決マップ

| # | Sev | 指摘 | v3 解決の要点 | 節 |
|---|---|---|---|---|
| F1 | CRIT | HMAC は Sanitizer 実行を証明しない | egress 可否を **Rust 独立再検査**へ全面移管。HMAC は PII 判定に非関与。signer call-site を承認経路に限定する静的 call-graph 契約 + 「HMAC 既知ベクトルだけでは本性質を証明できない」を監査契約へ明記 | §3 |
| F2 | CRIT | epoch 内無制限リプレイ | 署名対象へ `txn_nonce`(single-use) `session_id` `provider` `policy_epoch` `dict_hash` `sidecar_generation` を包含。Rust FSM が nonce を消尽 | §4,§5 |
| F3 | CRIT | 3 フェーズを束ねる identity 不在 | `research_id` = nonce を全 phase・integrate params・provenance へ刻印。typestate FSM で不正遷移をコンパイル不能化 | §5 |
| F4 | CRIT | 「部分 ingest なし」が永続化設計と矛盾 | query 単位 `ingest_external_results` を**廃止**。1 research = **単一 record を 1 回 atomic write**。commit-after-response 喪失は idempotent 上書き（research_id 冪等キー）で 0/全件保証 | §6,§8 |
| F5 | HIGH | PII 辞書の正本・更新・表記揺れ未定義 + ゼロ幅回避 | canonical 辞書 snapshot（正本・monotonic revision・生成元一本化） + **match-canonicalization に不可視/Bidi 除去を追加**（Rust 側は E0a を待たず単独で厳格判定） | §3,§7 |
| F6 | HIGH | 越境フレーミング `len` 単位・鍵表現の曖昧 | `len`=UTF-8 バイト長と明定。鍵=hex decode 後 raw 32 byte。**署名前 framed-bytes 自体の固定値 golden**。NFC 固定・lone surrogate/NUL/CRLF fail-closed | §4 |
| F7 | HIGH | 一時鍵「child/env のみ」は非現実 | 鍵複製の現実を明記し zeroize/epoch 束縛/親 env override 拒否/**LLM 子への env scrubbing**/RNG・spawn 失敗時 fail-closed を契約化 | §4.3 |
| F8 | HIGH | deadline/cancel/single-flight 非線形化（120s vs 60s、heartbeat 延長、spawn_blocking 継続） | 予算を phase 別に再定義し整合。E0b の 2 IPC は**非 emit 契約**で heartbeat 延長を封鎖。fetch のみ真にキャンセル可能。integrate 前 point-of-no-return。single-flight は atomic CAS | §9 |
| F9 | HIGH | transport trait 同期 vs reqwest async・`stream` feature 欠落 | trait を **async 化**、`reqwest` に `stream` feature 追加、`cargo check --locked` 契約、本番型が同一 cap 経路を通るコンパイル契約 | §7.4 |
| F10 | HIGH | 1MiB cap と serde の主張不成立（gzip/depth skip/array） | **圧縮全面不使用**(identity 強制+feature off)、serde 前の**独立 depth pre-scan**、**bounded visitor** で要素 4 個目で即エラー（全 allocate 前に拒否） | §7 |
| F11 | HIGH | 固定ヘッダ/fence は隔離にならない・恒久汚染 | 隔離 namespace（`load_knowledge_chunks` 非対象）+ **専用 loader/render 経路** + render 時 special-token 物理無効化 + `##`・fence・link・image を構成不能化。**過大主張を撤回し残余リスクを明示** | §8 |
| F12 | HIGH | Unicode 除去・tag-strip・Python 再検証が非対称 | Rust/Python の match-canonicalization を**単一定義へ統一**。tag-strip→entity-decode の順序逆転（decode 先行）+ double-encoding 対策 + 越境 golden。**Rust 禁止 raw 値を Python も独立拒否** | §7,§8 |
| F13 | HIGH | URL trait/DNS/HTTP status 境界不足 | `build_url` を host 内包の単一構築へ（連結・追加 param 不能）。**送信直前の scheme/host/port/path/param 検査**。decoded param==signed query byte-exact。200+JSON のみ受理。DNS は §9 の resolve→deny→pin | §7.5,§10 |
| F14 | MED | エラー値秘匿・constant-time 未証明（engine_stdio が例外値を漏洩） | E0b 全 validator は**値を含まない固定 sentinel 例外**のみ raw。`hmac.compare_digest`/Rust `subtle` の静的契約。全失敗点への sentinel 注入で query/title/snippet/path/URL 完全不在を証明 | §11 |

v2 で否定されなかった骨格（Rust 主導・Python 網能力ゼロ・title+snippet のみ・Provider 抽象・既定 OFF・明示ボタン・深度 1・実網テスト禁止）は継承する。

---

## 2. 改訂後シーケンス（予算とキャンセル境界を明示）

```mermaid
sequenceDiagram
    autonumber
    participant UI as React UI
    participant ORCH as Rust Orchestrator<br/>(async, tokio::select! biased)
    participant FSM as ResearchSlot FSM<br/>(単一スロット・typestate)
    participant GW as NetGateway<br/>(dual-run + DNS guard, async)
    participant PY as Python sidecar (網能力ゼロ)
    participant EXT as External JSON API

    UI->>ORCH: research(query) [明示ボタン]
    ORCH->>FSM: CAS(Idle→Building) 失敗=Busy refusal
    ORCH->>ORCH: canonicalize(query) 1回 → q*  (以後 q* を不変使用)
    ORCH->>FSM: Txn{research_id=nonce32, gen=spawn_gen, deadline_abs}
    Note over ORCH,PY: [phase A: intent] 非cancel・IPC_IO_DEADLINE(30s)単発・非emit契約
    ORCH->>PY: knowledge.intent.build {query:q*, txn_nonce, sidecar_generation}
    PY->>PY: dict snapshot(rev R) → strict-preflight(q*派生の抽象query) → attest
    PY-->>ORCH: {abstract_queries(canonical), attestation, pii_revision, generation}
    ORCH->>GW: verify_and_check
    GW->>GW: ①tag constant-time ②nonce一致&state==Building ③gen一致 ④rev==latest<br/>⑤Rust独立 aho-corasick 再検査(不可視除去済) — 1失敗=ABORTED, egress 0
    ORCH->>FSM: Building→Fetching (nonce 消尽)
    Note over ORCH,EXT: [phase B: fetch] tokio::select!{cancel | deadline | fetch_all}<br/>arm drop = reqwest future drop = socket close
    loop 各 abstract_query 逐次 (≤4, fetch_budget 内)
        GW->>GW: resolve(host)→全IP deny-table濾過→合格IPへ pin (ClientBuilder::resolve)
        GW->>EXT: GET https/443固定, redirect:none, Accept-Encoding:identity
        EXT-->>GW: 200 application/json ≤1MiB (stream計数, 超過即drop)
        GW->>GW: content-encoding検査→depth pre-scan(≤32)→bounded serde(≤3)→抽出→§8無効化
    end
    ORCH->>ORCH: 残余予算 ≥ IPC_IO_DEADLINE を検査 (point of no return)
    ORCH->>FSM: Fetching→Integrating
    Note over ORCH,PY: [phase C: integrate] 非cancel・単発・非emit・冪等
    ORCH->>PY: knowledge.integrate {research_id, provider, policy_epoch, fetched_at, dict_hash, results_by_query}
    PY->>PY: strict再検証 → 単一 record を durable_atomic_write (research_id 冪等)
    PY-->>ORCH: {results_persisted}
    ORCH->>FSM: Integrating→Completed(terminal)
    ORCH-->>UI: ResearchReceipt (runtime parser)
    Note over FSM: 任意時点の cancel/deadline/検証失敗 = ABORTED(reason)[terminal];<br/>遅着 IPC は turnstile 照合で破棄; spawn_blocking スレッドは<br/>継続し得るが副作用は phase C の冪等単一 write のみに限局
```

---

## 3. Dual-Run 独立検査 — egress 可否の単一決定者（F1 / F5 / F12 解決）

### 3.1 設計原理（監査契約へ明記する命題）

> **監査契約 A1:** egress の PII 安全性は **Rust の独立再検査が単独で保証する**。
> Python preflight・HMAC・signer call-site の正しさは**この保証の前提ではない**（多層防御の上積みに過ぎない）。
> したがって「HMAC 既知ベクトル一致」テストは F1 の反証にならない。F1 の GREEN 条件は
> **§3.3 の Rust 再検査が、Python を完全にバイパスした攻撃入力に対しても PII を拒否すること**である。

### 3.2 canonical PII 辞書 snapshot（正本・F5）

```text
生成元 : Python core の単一 supplier に一本化（profile/contact/alias/旧名/住所/プロジェクト略称の
         既存抽出を集約する 1 関数 build_pii_dictionary_snapshot()。抽出元・alias 規則をここで確定）
正本   : data/processed/pii_dictionary/latest.json
形式   : {"schema":"pkb.pii_dictionary.v1","revision":<monotonic int>,
          "match_terms":[... match-canonical 済み・重複除去・ソート ...],
          "hash":"<64hex>"}
hash   : SHA-256( b"PKB-PII-DICT-V1" || u64_be(revision) || Σ(u64_be(len_bytes(t)) || utf8(t)) )
更新   : entity 追加/削除で revision+1・全置換（durable_atomic_write_text）。部分更新なし。
         更新は「同一 sidecar 内で次 Txn から有効」（§3.4 の drift 検査が旧 attestation を無効化）
```

### 3.3 match-canonicalization（Rust/Python 単一定義・F5/F12 の統合）

E0a `_normalize_for_matching`（NFKC+casefold のみ）は**変更しない**（凍結）。E0b は**より厳格な**専用関数を新設し、
**辞書用語の生成・Python 側 E0b preflight・Rust 側再検査の 3 箇所で同一適用**する。

```text
match_canonical(s):
  1. Unicode 正規化 NFKC
  2. casefold
  3. 除去: 全 Cf(format) = ZWSP/ZWNJ/ZWJ(U+200B..200D), U+200E/200F, U+202A..202E,
           U+2066..2069(Bidi isolates), U+061C(ALM), U+00AD(SHY), U+FEFF(BOM), U+180E
  4. 除去: C0/C1 制御文字（U+0000..001F, U+007F..009F）
  5. 空白 run（Zs 全種 + TAB）を単一 U+0020 へ畳む
  6. NFC 再正規化（3–5 後の結合安定化）
※ 出力は「照合専用」。outbound クエリの byte 列（§0-2 の q* / abstract_query）は別に保持し変えない。
```

- Rust 実装: `unicode-normalization`(NFKC/NFC) + `caseless` + テーブル駆動 Cf/Cc 除去。
- 等価性は**敵対 Unicode 越境 golden**（合字・トルコ i・全角/半角・NFC/NFD・結合文字・上記全不可視）で証明（§12 STEP 1）。
- **非対称でも安全:** 判定は AND ゲート。仮に片側が取りこぼしても他方が拒否すれば egress は起きない（拒絶の和集合）。

### 3.4 Rust 再検査手順（AND、1 失敗 = `ABORTED`・HTTP 接続ゼロ）

1. attestation tag constant-time 検証（§4）。
2. `txn_nonce` が現スロット Txn と一致 & `state == Building`（未消費）。
3. `sidecar_generation` 一致（§4.3 epoch 束縛）。
4. attested `dict_hash` == Rust が今読んだ latest.json の再計算 hash == 現行 revision（**drift 検査**）。
5. 全 abstract_query を `match_canonical` → `aho-corasick`（線形・非 backtrack）走査。terminal hit = `PiiRejected`。
   エラーに query・用語・位置を含めない（E0a `EgressViolationError` 規律）。

---

## 4. Attestation v2 — Txn 束縛・完全性のみ（F2 / F6 / F7 / M1 解決）

### 4.1 署名対象（全フィールド長さ前置・単射・`len`=UTF-8 バイト長で固定）

```text
msg = b"PKB-E0B-EGRESS-V3"
   || framed(session_id)          # 16B raw
   || framed(txn_nonce)           # 32B raw
   || framed(sidecar_generation)  # u64_be 固定 8B（framed だが len は常に 8）
   || framed(policy_epoch)        # u64_be 8B
   || framed(provider_id)         # ascii "wikipedia"
   || framed(dict_hash)           # 32B raw
   || u64_be(n_queries)
   || Σ framed(q*_i)              # canonical・順序保存・NFC 済み UTF-8
where framed(x) = u64_be(byte_len(x)) || x        # len は **UTF-8 バイト長**（Rust .len() と一致）
tag = HMAC-SHA256(K_spawn_raw32, msg)              # 出力 32B → 64 lowercase hex で IPC 授受
```

- **F6 明定:** `byte_len` は UTF-8 バイト長（`"日本"`→6, `"😀"`→4）。鍵は env の 64 hex を**decode した raw 32 byte**。
  scalar 数・code unit 数は使わない。**署名前 `msg` 自体の固定値 golden**を Python/Rust 双方で比較する
  （tag だけの一致では framing 差異を検出できないため）。
- **F6 fail-closed:** q* は NFC 済みのみ。lone surrogate・NUL(U+0000)・生 CR/LF を含む入力は canonicalize で reject。
  JSON literal 文字と `\uXXXX` は同一 scalar へ decode 後に比較（IPC JSON パーサ通過後の str が正本）。
- **F13 連動:** provider へ送る検索 param を 1 回 decode したものが `q*_i` と byte-exact 一致することを契約（§7.5）。

### 4.2 識別子ライフサイクル

| 項目 | 生成 | 経路 | 寿命 |
|---|---|---|---|
| `K_spawn` 32B | Rust spawn 毎 CSPRNG（`getrandom`） | env `PKB_EGRESS_KEY`(64hex) | sidecar 生存中。Rust `zeroize`、非永続・非ログ |
| `session_id` 16B | Rust spawn 毎 CSPRNG | env `PKB_EGRESS_SESSION`(32hex) | 同上 |
| `sidecar_generation` u64 | Rust、boot 毎 +1（`EngineManager` の generation counter を新設） | intent/integrate params | 再 boot で無効 |
| `txn_nonce` 32B | Rust、Txn 毎 CSPRNG | params `txn_nonce` / `research_id` | single-use・TTL 120s・FSM 消尽 |
| `policy_epoch` u64 | Rust、policy 変更毎 +1 | params | policy 変更で旧タグ失効 |
| `dict_hash` | Python snapshot 生成時 | latest.json + attestation 包含 | revision 更新で失効 |

**リプレイ全滅（F2）:** 同一 Txn 再送→nonce 消費済み拒否 / 別 cid・別 Txn→nonce 不一致 / 再 boot→gen・鍵・session
不一致 / policy 変更後→epoch 不一致 / 辞書更新後→dict_hash 不一致。「未保護時のタグを保護後に再利用」は
**dict_hash + drift 検査**で死ぬ。`ReplayPolicy::NoReplay` とは独立の消費機構である（監査契約 A2）。

### 4.3 鍵の現実と契約（F7 — 過大主張の撤回）

> **正直な限界:** 鍵は Rust 保持領域・hex String・Command 環境・Windows UTF-16 env block・Python `os.environ`・
> HMAC 処理中 bytes・crash dump 候補に**複製され得る**。「child/env のみ」は達成できない。
> 本設計は「同一ユーザー権限のフル侵害」を守らない（それは AppContainer L0 の管轄）。守るのは**配線ミス・
> epoch 混同・鍵残留・子孫継承**である。

契約（憲法ガード §12 STEP 0 / STEP 4 で証明）:

- 鍵・session・generation は spawn 毎に**厳密に 1 回**生成（debug/release 全 spawn 経路）。K1 は gen1 のみ、K2 は gen2 のみで valid。
- RNG 失敗・spawn 失敗・ready 失敗・shutdown 後に**利用可能な鍵が残らない**（fail-closed、intent.build は鍵不在で ValueError）。
- 親プロセスの同名 env は生成鍵を**上書きできない**（Rust が子 `Command` env に明示 set し、継承値を上書き）。
- **LLM 子・孫プロセスへ継承させない:** Python が llama.cpp 子を spawn する際、`PKB_EGRESS_*` を子 env から
  scrub する（AI_SKILLS §8 の「prompt 本文を argv/環境へ残さない」規律の拡張として明文化）。
- Rust は使用後に鍵バッファを `zeroize`。stdout/stderr/error/UI/log/file に鍵・タグを出さない。

---

## 5. トランザクション・ステートマシン（F3 / M1 解決）

```text
Rust: static SLOT: Mutex<Option<ResearchTxn>>            # 単一 = 並行度 1
状態(typestate 構造体、所有権移動で skip 不能):
  Idle → Building → Fetching → Integrating → Completed(terminal)
                 ↘ (任意失敗) ↘            ↘
                        ABORTED(reason)(terminal)
遷移は orchestrator 単一パスのみ。Txn<Building> → Txn<Fetching> の消費で不正遷移をコンパイル不能化。
research_id = txn_nonce(hex)。intent/integrate params・provenance record に刻印。
integrate は Fetching→Integrating の一意遷移でのみ構築（構築 = typestate 消費 → 二重 integrate 不能）。
順序正本は Rust FSM のみ。Python は状態を持たず research_id 形式検証 + 刻印のみ（順序判断を委譲されない）。
ABORTED reason: PolicyOff|Busy|AttestationMismatch|NonceConsumed|GenerationMismatch|DictionaryDrift
              |PiiRejected|Deadline|Cancelled|WireViolation|DnsDenied|StatusRejected|TransportFailure
TTL: created_at+120s で reaper が ABORTED(Deadline)。reaper は state 遷移のみ、資源解放は drop に委譲。
```

**要求テスト（監査 F3）:** 全不正遷移行列 / Integrate-before-Fetch・二重 Fetch・二重 Integrate・完了後遅延メッセージ
/ attested `[A,B]` に対する `[B,A]/[A]/[A,C]` の全 integrate 拒否（Rust が abstract_queries を保持し integrate 時に
research_id で束ねるため、Python へ渡る results_by_query の query は fetch 済み集合と Rust 側で照合）/ IPC の
`id/cid` が正しくても research identity・generation 不一致なら HTTP・永続化ともゼロ。

---

## 6. 永続化の原子性 — 0 件 or 全件（F4 解決）

- **query 単位 `ingest_external_results` を廃止**（v1/v2 の設計矛盾を除去）。
- 1 research = **単一 provenance record**（§8.2）。Python integrate は
  「全 results を strict 検証 → メモリ上で 1 record 構築 → `durable_atomic_write_text`（temp+fsync+rename）1 回」。
  検証途中失敗は fs に 1 byte も触れない（disk full は rename 前に発生 → 部分ファイルは temp のみ、公開されない）。
- **commit 後・応答喪失（F4 後段）:** record ファイル名を `ext_{research_id}.json` とし、research_id を**冪等キー**にする。
  同一 research_id の再 integrate は同一 byte 内容の上書き（内容一致を検証、不一致は `ValueError`= すり替え検出、
  INC-PHASE4A-04 様式）。→ 再操作で重複・二重反映が起きない。
- **receipt と実ファイルの乖離防止:** receipt の `results_persisted` は record 書込成功後にのみ Python が返す。
  Rust は receipt 受領 = Completed。応答喪失時は既存 `MSG_OUTCOME_UNKNOWN` へ収斂し、再操作は冪等上書きで安全。

**要求テスト（F4）:** temp/fsync/rename 各境界 + 最終 response 直前への fault injection / 再起動後が「0 or 全件」以外に
ならない / commit 後応答喪失→再操作で重複・上書き無し / 途中失敗時 receipt と実ファイル集合が乖離しない。

---

## 7. Wire / Transport / URL 境界（F9 / F10 / F13 解決）

### 7.1 圧縮全面不使用（F10 gzip）

- `reqwest` は **gzip/brotli/deflate feature を有効化しない**。リクエストに `Accept-Encoding: identity` 明示送出。
- 応答 `Content-Encoding` が `identity`/不在**以外**なら body を読まず `WireViolation`。
  → `bytes_stream()` が数える wire バイト == 決済バイト。decompression bomb はクラスごと消滅。

### 7.2 サイズ cap（F10 chunked/length 詐称）

- `bytes_stream()` chunk 逐次計数、`MAX_API_RESPONSE_BYTES = 1 MiB` 超過で**その chunk を append せず即 drop**。
- Content-Length は参考 pre-check のみ（信用しない）。chunked・length 詐称・無限ストリームは同一経路で死ぬ。
  trickle は fetch total timeout（§9）が上界。

### 7.3 depth と配列（F10 serde skip / array）

- **serde 前**に raw bytes 上で線形 pre-scan: UTF-8 検証 + `{`/`[` の深さ ≤ `MAX_JSON_DEPTH_WIRE=32`
  （文字列リテラル内・エスケープを正しく skip）。→ 未知フィールド内の 128 超 nesting も serde の `IgnoredAny`
  経路に依存せず**独立に**拒否。
- 抽出は **bounded visitor**: `results` 配列は 4 個目の要素で即 `Err`（全件 allocate/deserialize しない）。
  同様に `MAX_FETCH_QUERIES=4` を超える上位構造も visitor で早期拒否。

### 7.4 transport trait は async・依存整合（F9）

```rust
#[async_trait::async_trait]
pub(crate) trait HttpTransport: Send + Sync {
    async fn get_json_capped(&self, url: &Url, host_pin: IpAddr, cap: usize)
        -> Result<Vec<u8>, GatewayError>;   // cap 超過/非200/非JSON/encoding違反は Err
}
```

- `Cargo.toml`: `reqwest = { version="0.12", default-features=false, features=["rustls-tls","stream"] }`
  （gzip/cookies/native-tls/socks なし。**`stream` を明示追加** — v2 の依存指定は `bytes_stream()` を
  ビルドできなかった。監査 F9 は正しい）。`async-trait` を追加。
- **本番型 `ReqwestTransport` が fake と同一 cap 経路を通る**ことをコンパイル契約（orchestrator は
  `Arc<dyn HttpTransport>` 越しにしか fetch しない）。`cargo check --all-targets --locked` を STEP 0 ゲート。

### 7.5 URL 構築 — host 内包・送信直前検査（F13）

```rust
pub(crate) trait SearchProvider {
    fn id(&self) -> &'static str;                 // "wikipedia"
    fn build_request(&self, q_star: &str) -> ProviderRequest;  // host も内部定数から。呼出側は host を渡せない
}
pub(crate) struct ProviderRequest { pub url: Url, pub host: &'static str }  // url.host は build 内で固定
```

- `build_request` は固定 host・固定 path・固定 param 集合から `Url::query_pairs_mut` で構築（文字列連結禁止）。
  可変部は検索 param 1 個のみ = `q_star`。
- **送信直前アサート**（全入力に対し不変条件を実行時検査、違反 = `WireViolation`）:
  `scheme==https && host==allowlist && port==443 && userinfo.is_empty() && fragment.is_none() &&
   path==固定 && query キー集合==固定`。
- **byte-exact 契約:** 送出 URL の検索 param を 1 回 percent-decode したものが署名対象 `q*_i` と byte 一致。
- HTTP は **200 のみ**受理。`Content-Type` が `application/json`（±`; charset=utf-8`）以外は body を読まず `StatusRejected`。
  3xx は `redirect::Policy::none()` によりそもそも追従しない上、200 以外一律拒否。

---

## 8. コンテキスト隔離 v3 — 実 loader の挙動に接地（F11 / F12 / M3 解決）

> **v2 の過大主張を撤回する。** 監査 F11 のとおり、[consultation_engine.py:412](../src/python/core/consultation_engine.py)
> の `load_knowledge_chunks()` は `^##` でファイルを分割し、[:940](../src/python/core/consultation_engine.py) は
> hit 本文を直接連結する。**「固定ヘッダが常に付随」「被害上限は無害な誤検索 1 回」は成立しない。**

### 8.1 隔離 namespace（既存 loader から構造的に不可視）

- 保存先 `data/knowledge/external/ext_{research_id}.json`。`load_knowledge_chunks()` は KNOWLEDGE_DIR 直下の
  `.md`/`.txt` を**非再帰**（`iterdir()`）走査するため、**subdir + `.json` の二重の理由で当該 loader の対象外**。
  → 外部データが既存経路へ**暗黙混入する事故が構造的に起きない**（誤混入の物理排除）。

### 8.2 専用取得・専用 render 経路（唯一のプロンプト流入口）

外部データがプロンプトへ入るのは **`render_external_evidence()` ただ 1 関数**に限定する（ingest 時ではなく
**毎 render 時**に無効化を適用 — loader 改修や保存物改竄で前提が崩れる事故を単一責務へ集約して防ぐ）。

```text
load_external_evidence(query_vec) → external hit（専用 index / 専用 lane。既存 diary/knowledge lane と別）
render_external_evidence(hits) → プロンプト片。以下を毎回適用:
  1. special-token 物理無効化: model tokenizer の special 表（正本: config/model_params.json と同座で凍結）
     由来の固定パターン（<|…|>, <s></s>, [INST][/INST], <<SYS>> 等）の ASCII トリガ字を全角等価へ写像
     (< → ＜, | → ｜)。→ tokenizer レベルで role 昇格トークンが構成不能。regex 不使用の単一パス。
  2. 構造無効化: 行頭 #, 全 backtick/tilde, Markdown link/image 記法([](),![]()) , 生 HTML(<...>),
     既知 prompt delimiter を、各 1 code point の可視代替字へ写像 or 除去。→ fence・heading・link・image
     を snippet 内から構成不能化（title 内 ` による fence 閉じも封鎖）。
  3. 各 hit を「1 render 単位」として provenance と**同一テキスト塊**に束ねる（##/改行分割で警告と本文が
     分離しない構造。render 出力は build_dynamic_suffix の最後尾 lane にのみ配置）。
```

- **プロンプト構成順序:** 既存契約 `build_static_prefix()`（信頼・静的）→ `build_dynamic_suffix()`（動的）を継承し、
  external evidence は dynamic suffix の**最後尾**に固定。static prefix への混入は `test_prompt_static_dynamic_split`
  系で凍結。固定ヘッダ文言は**補助**であり主柱に数えない（監査 F11 の指摘を受けた格下げ）。

### 8.3 汚染フィードバックループの切断（自己増殖の物理排除）

- `knowledge.intent.build` の抽象クエリ生成文脈に external-origin データを**一切含めない**。
  → 「汚染 → 誘導クエリ → 再汚染」の自己増殖ループが構造的に存在しない。
- 深度 1（明示ボタン・fetch フック不存在）維持。歪んだクエリも §3.4 Rust 再検査が出自を問わず走る。

### 8.4 残余リスクの明示（誠実性 — 監査 F11 への正面回答）

> **証明できること:** 外部データは (a) さらなる egress を起こせない（深度 1 + intent 文脈除外 + PII 二重検査）、
> (b) tokenizer レベルの role 昇格トークンを構成できない（§8.2-1）、(c) URL を LLM 文脈へ持ち込まない（PII 持出
> 経路の遮断）、(d) ユーザーが per-research 単位で削除できる（§8.5）。
> **証明できないこと:** 「もっともらしい平文の誤誘導テキストを LLM が読んで判断を歪められない」ことは
> **証明しない**。これは全 RAG に共通の根源的限界であり、本設計は「無害化 + 非 egress 帰結 + 非自己増殖 +
> 可削除」で被害を限定するが、**「無害な誤検索 1 回」という v2 の上限主張は撤回する**。

### 8.5 per-research 削除 + 実 integration test（F11 要求）

- SETTINGS に外部 record 一覧・個別削除。ファイル粒度 = research_id 粒度 = 単一 unlink。索引は次回 sync で追随。
- **byte-level integration test:** 保存 → 専用 loader → index → retrieve → `render_external_evidence` → 最終
  prompt までを貫通し、(1) 各チャンクに external 属性が失われず付随、(2) snippet から fence 外構造/link/image が
  生成不能、(3) special token が最終 prompt に出現しない、を byte で検証。

### 8.6 Unicode 除去・tag-strip の対称化（F12）

- 除去集合に **U+061C, U+2028/U+2029, U+FEFF, U+200B–200F, U+202A–202E, U+2066–2069, U+00AD, U+180E** を含める
  （§3.3 の match-canonical と同一の Cf/Cc 集合を、抽出後正規化にも適用）。
- **順序逆転の修正:** entity decode を**先**に行い、その後に tag-strip・不可視除去（`&lt;span&gt;` → decode →
  `<span>` → strip の 1 パスで残渣を出さない）。double-encoding・decimal/hex numeric entity の C0/C1/Bidi も対象。
- **Python 独立拒否（F12 後段）:** Rust が除去/truncate するはずの raw 値を `knowledge.integrate` へ直接渡しても、
  **Python validator が独立に**制御文字・不可視・空文字化 title/snippet を拒否する（Rust 前段が唯一の砦ではない）。
  Rust 正規化出力 → Python validator 受理の越境 golden で対称性を証明。

---

## 9. 非同期安全・deadline 線形化（F8 解決）

### 9.1 予算の再定義（120s vs 60s 矛盾の解消）

```text
phase A intent  : 非cancel・単発 IPC。上界 = IPC_IO_DEADLINE(30s)。**非emit契約**で heartbeat 延長不能(§9.3)
phase B fetch   : tokio::select! 配下。上界 = FETCH_BUDGET(30s、逐次 ≤4 本・各 HTTP_TOTAL 15s を包含)
phase C integrate: 非cancel・単発 IPC。上界 = IPC_IO_DEADLINE(30s)。非emit契約
研究全体の wall-clock 上界 = 30 + 30 + 30 = 90s。
RESEARCH_TTL = 120s（reaper・余裕込み）。**v1/v2 の RESEARCH_DEADLINE=60s は撤回**（矛盾していた）。
```

### 9.2 キャンセル構造（fetch のみ真にキャンセル可能）

```rust
let outcome = tokio::select! {
    biased;
    _ = cancel.cancelled()               => return txn.abort(Cancelled),
    _ = tokio::time::sleep_until(fetch_deadline) => return txn.abort(Deadline),
    r = gateway.fetch_all(&intent)       => r,   // arm drop = reqwest future drop = socket close
};
```

- **drop 意味論の明文化と証明:** hyper/reqwest は response/body future の drop で接続を abort・socket を即 close。
  fake transport に「drop されたら flag を立てる」プローブを持たせ、cancel/deadline で即発火することを
  tokio paused-time で RED 証明（§12 STEP 5）。detached `spawn`・`spawn_blocking` は net_gateway 内で憲法禁止。

### 9.3 既存 spawn_blocking IPC への対処（F8/F9 の実挙動受容）

> [engine.rs:447](../apps/desktop/src-tauri/src/engine.rs) の `invoke()` は `spawn_blocking` + メッセージ毎
> `recv_timeout`。**この blocking スレッドは外側 future の cancel で停止しない。**これを設計前提として受容する。

- **phase A/C を非 cancel と認め、副作用を phase C の冪等単一 write に限局**する（§6）。cancel が phase C の
  blocking スレッド実行中に来ても、副作用は「1 回の冪等 record write」のみ = 遅延しても 0/全件不変。
- **heartbeat 延長封鎖:** `knowledge.intent.build` / `knowledge.integrate` は**中間 event を emit しない契約**
  （status/chunk を出さない）。→ `recv_line` は最終応答 1 回で返り、per-message timeout は実質 single-shot 30s。
  29 秒 heartbeat で延長する攻撃面を閉じる（emit するのは consult 等の streaming コマンドのみ、という既存分離を利用）。
- **turnstile:** blocking から返った時点で FSM 照合。Txn が terminal なら結果を同一関数内で破棄
  （`manifestFetchState.ts` stale-seq ガードの Rust 版）。
- **point of no return:** phase C 発行前に `残余 wall-clock ≥ IPC_IO_DEADLINE` を検査。不足なら integrate を
  **発行せず** `ABORTED(Deadline)`。「送ったが間に合わない」を構造排除。

### 9.4 single-flight の原子性（F8）

- `SLOT` への遷移は `Idle→Building` の **atomic CAS**（Mutex 内 compare-and-set）。占有中の新要求は `Busy` refusal。
- panic/cancel/timeout でも `Txn` drop 時に SLOT を `Idle` へ戻す（RAII guard）。次要求が必ず開始可能。
- policy OFF 判定 → SLOT CAS の **lock 順序を固定**（policy を先、SLOT を後）。policy OFF が成功を返した後に
  新規接続がゼロであることをテスト。

**要求テスト（F8）:** 単一 fake-clock で全 phase + mutex 待ちの deadline / 29 秒間隔 correlated event でも 90s 内停止
/ 各 phase cancel 後に HTTP・integrate・restart が遅延実行されない / barrier 同期 100 同時要求で実行厳密 1 件 /
各地点 error・panic・timeout 後に次要求開始可能 / policy OFF 成功後に新規接続ゼロ / kill 失敗・wait 停止注入でも
restart 無期限停止せず。

---

## 10. DNS / Network Guard（F7-net / F13-dns / M4 — v2 §9 を継承・強化）

```text
query 毎: host=固定定数 → resolver.lookup(host)（hickory-resolver、seam）
        → 全解決 addr を deny_table 濾過（1つでも非 global = ABORTED(DnsDenied), 合格分だけ使う事はしない）
        → ClientBuilder::resolve(host, vetted_addr) で pin（reqwest は再解決しない）→ 接続
deny_table(IPv4/IPv6, v4-mapped/NAT64 は unwrap 後に再判定):
  IPv4: 0/8,10/8,100.64/10(CGNAT),127/8,169.254/16,172.16/12,192.0.0/24,192.0.2/24,192.168/16,
        198.18/15,198.51.100/24,203.0.113/24,224/4,240/4,255.255.255.255/32
  IPv6: ::/128,::1,::ffff:0:0/96(→v4 再判定),64:ff9b::/96(NAT64→埋込v4 再判定),fc00::/7(ULA),
        fe80::/10,2001:db8::/32,ff00::/8
```

- **TOCTOU/rebinding:** 判定した addr **そのもの**へ pin → 「検査時 global・接続時 private」再解決が不能。
- **要求テスト:** deny_table 全域 fixture（境界 CIDR/v4-mapped/NAT64 埋込）/ 「一部 private なら全体 abort」/
  resolve→pin 後に fake resolver が答えを変えても接続先不変 / IP literal・suffix host・trailing dot・punycode・
  別 path provider で transport call ゼロ / proxy 汚染・redirect 先 dial ゼロ。

---

## 11. エラー秘匿・constant-time（F14 解決）

> [engine_stdio.py:232](../src/python/engine_stdio.py) は例外文字列を応答へ入れる。**E0b は値を例外に載せない。**

- E0b の**全 validator は値を含まない固定 sentinel 例外**のみ raise（例: `E0bRejected("E0B_VALIDATION_REJECTED")`）。
  query/title/snippet/path/URL/用語/位置/例外値を message に**一切**埋めない（E0a `EgressViolationError` 規律の全面適用）。
  → engine_stdio の generic handler が `f"{exc}"` を入れても、中身が固定 sentinel なので漏洩ゼロ。
- reqwest error・永続化 error は Rust/Python の各層で**固定 typed error へ写像してから**返す（原文字列を素通ししない）。
- constant-time: tag 比較は Python `hmac.compare_digest` / Rust `subtle::ConstantTimeEq`（`==` 禁止を静的契約で担保）。
- **要求テスト:** 全失敗点へ固有 sentinel を含む query/title/snippet/path/URL を注入し stdout/stderr/log/error/UI から
  完全不在を確認 / equal-length タグ比較が constant-time primitive のみ通る静的契約 / タグ先頭・末尾 1byte 変異で
  同一 typed error・transport ゼロ / 63・65 文字・大文字・空白・非 hex・Unicode homoglyph の形式拒否 /
  commit 前失敗と commit 後 response 喪失を区別する state-aware outcome。

---

## 12. 実装フェーズ計画 v3（RED/GREEN・全 finding にテストを紐付け）

ゲート: `python -m pytest` / `cargo test` / `cargo check --all-targets --locked` /
`npm.cmd run test:boundary` / `tsc --noEmit` / `vite build`。実網テスト全面禁止（transport/resolver/clock は注入）。

```text
STEP 0  憲法ガード & ビルド契約
        - E0a stub 凍結回帰 / 新規 Python の network import 禁止(AST)
        - net_gateway/orchestrator に spawn_blocking・detached spawn・fs 書込が無い静的検査
        - signer call-site が承認 preflight 経路限定の call-graph 契約 [F1]
        - `==` によるタグ比較不在の静的契約 [F14] / cargo check --locked が stream/async-trait 込みで通る [F9]
        - 鍵/session/gen env 不在で intent.build fail-closed [F7]
STEP 1  match-canonicalization + PII 辞書 snapshot（Python core, Rust と共有 golden 先行確定）[F5,F12]
        - 不可視/Bidi 除去・NFKC/casefold/NFC・空白畳み / 敵対 Unicode 越境 vector / monotonic revision・hash 照合
STEP 2  Python intent.build v2 [F1,F2,F6,F7]
        - framed-bytes 固定 golden(tag だけでない) / nonce/session/gen/epoch/dict_hash リプレイ vector 群
        - strict-preflight all-or-nothing / MAX_FETCH_QUERIES 超過 reject / signer 直接呼びを Rust が拒否する越境試験
STEP 3  Rust dual-run 再検査 + attestation 検証 [F1,F2,F5]
        - Python バイパス入力の PII 拒否(F1 GREEN 条件) / aho-corasick と Python 判定の対照 + 拒絶の和集合
        - drift abort / hash 照合失敗 fail-closed / framed-bytes 越境一致
STEP 4  Rust TxnStateMachine(typestate) + generation counter [F3,F7]
        - 全不正遷移コンパイル不能の doc-test / nonce single-use / TTL / 二重 research refusal / epoch 束縛
STEP 5  Rust async 安全 [F8,F9]
        - paused-time で cancel/deadline→drop プローブ即発火 / turnstile 遅着破棄 / point-of-no-return 境界
        - 29s heartbeat 非延長(非 emit 契約) / single-flight atomic CAS・RAII 解放 / 予算 90s 線形化
STEP 6  Rust wire + DNS guard + Provider + URL [F10,F13,F7-net]
        - identity 強制 / cap(cap-1,cap,cap+1) / content-type / depth pre-scan(127/128/129/512, 文字列内括弧)
        - bounded visitor(results 0/1/3/4/10000, peak alloc) / deny_table 全域 / resolve→pin TOCTOU
        - build_url 不変条件(全入力で scheme/host/port/path/param) / decoded param==signed query / 200+JSON のみ
STEP 7  Python integrate v2 + 隔離 namespace + render [F4,F11,F12,M3]
        - 単一 record 原子 write / 検証失敗 fs 無接触 / research_id 冪等上書き(すり替え検出)
        - exact-key・origin 列挙・64hex・制御文字/不可視/空文字化拒否(Rust 前段非依存)
        - render_external_evidence special-token 無効化 golden / 保存→loader→index→retrieve→prompt byte 貫通
        - intent 文脈からの external 除外(フィードバック遮断) / static-dynamic split 維持
STEP 8  orchestrator + Tauri command + policy(epoch) + LLM 子 env scrub + UI + receipt parser + 削除 UI [F7,F8,F11]
        - policy OFF refusal→接続ゼロ / all-or-nothing / net_gateway 非 export 静的確認 / 越境 golden
        - PKB_EGRESS_* が llama.cpp 子 env に継承されない検査
STEP 9  エラー秘匿の全失敗点検証 [F14] + docs 同期(AI_SKILLS §5/§7.2, CONTEXT.md, HANDOFF, INCIDENT_LEDGER)
        - 全 sentinel 注入 → 5 出力面から完全不在 / typed error 写像 / constant-time primitive 契約
```

依存追加（Rust）: `reqwest`(rustls-tls, **stream**), `async-trait`, `hmac`, `subtle`, `getrandom`, `zeroize`(既存),
`aho-corasick`, `unicode-normalization`, `caseless`, `hickory-resolver`（+ 既存 `sha2`）。Python/TS: ゼロ。

---

## 13. 監査への手続き的応答（2 点）

1. **前回監査の「全 14 項目」= 本レポートの F1–F14** と解する（v2 §10 で要請した残 7 項目が本監査で開示された）。
   14 findings すべてに §1 の対応表・§12 のテストを紐付けた。取りこぼしがあれば finding 番号でご指摘を請う。
2. **v2 の過大主張を明示撤回した箇所:** ①「HMAC が sanitizer 実行を証明」（→ dual-run 単独保証・§3.1 監査契約 A1）、
   ②「固定ヘッダで被害上限は誤検索 1 回」（→ 実 loader 挙動に接地・残余リスク明示・§8.4）、
   ③「RESEARCH_DEADLINE 60s」（→ 90s 線形予算へ・§9.1）、④ transport 同期 trait / gzip feature（→ async・stream・§7.4）。

---

## 14. 指揮官裁定が必要な未決事項

1. **Pydantic 逸脱**（repo 様式の strict validator を推奨・機能等価・依存ゼロ。v1 から継続）。
2. **E0a `privacy_search.py` 非改変の確認** — E0b は match-canonicalization を**新規モジュール**で追加し E0a を
   触らない設計。E0a 側 preflight 強化を望む場合は別 Finding・別裁定を要する。
3. **RetrievalManifest への `SourceType.EXTERNAL` 追加** — 凍結 schema の改版を伴うため独立裁定。
4. **AI_SKILLS §5「外向き通信の例外なし」条項の改訂文言** — Rust gateway を唯一の裁可例外とする憲法級変更。
5. **`config/model_params.json` の tokenizer special-token 表の正本化** — §8.2-1 の無効化が依存する凍結対象。
6. Wikipedia allowlist host 集合（`ja` 単独 or `{ja,en}`）/ research ボタン UI 配置（CONSULT 推奨）。

---

## 15. HARD STOP 宣言

本書の出力をもって v3 ブループリント策定を完了とする。**指揮官の ACK があるまで、STEP 0（RED テスト）を含む
一切のプロダクション/テストコードの実装に着手しない。**
