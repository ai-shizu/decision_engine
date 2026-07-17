# SPEC — E0b STEP 2: Intent Builder v2 + HMAC Attestation (Composer-Executable Blueprint)

> **読者:** 自律型コーディング AI（Cursor Composer 等）。人間向け解説ではない。**一字一句従え。裁量なし。**
> **親仕様:** `docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md` §4 / `docs/SPEC_E0B_STEP1_ATTESTATION.md`（承認済み）。
> **前提:** STEP 0（憲法ガード 10/10）+ STEP 1（`canonicalize_for_match` / `PIISnapshot` 16/16）はロック済み。
> **本 STEP の目的:** 外部検索クエリ群へリプレイ不能メタデータ（session/nonce/gen/epoch/dict_hash）を結合し、
> 一時鍵 `K_spawn` で HMAC-SHA256 の「サニタイザ通過証明タグ」を付与する Python Intent Builder v2 を作る。
> **署名前フレーミングバイト列と HMAC タグは本ブループリントが確定済み（§2.B の KAT）。実装をこれに合わせよ。**
> **ACK 規律:** STEP 2.D 完了時に HARD STOP。指揮官 ACK まで Rust STEP 3 へ進むな。

---

## 0. 何を作るか

新規モジュール **`src/python/core/e0b_intent.py`**（basename が `*e0b*.py` → STEP 0 Scope E cage に自動収監）。
公開 API:

```text
attestation_framing(session_id, txn_nonce, sidecar_generation, policy_epoch, dict_hash, queries) -> bytes
    # §1.4 の順序で長さ前置バイト列を構築（純関数・暗号を含まない）
attest_tag(k_spawn_hex: str, framing: bytes) -> str
    # HMAC-SHA256( key = bytes.fromhex(k_spawn_hex)[32B], msg = framing ) の hexdigest（64 lowercase hex）
    # ★ STEP 2.C で call-count を検証するため独立関数として export する
canonicalize_outbound_query(q: str) -> str
    # 送出＝署名対象クエリの決定論的整形（NFC + 不可視除去 + 制御 reject）。casefold しない（検索語を保持）
build_attested_intent(abstract_queries, *, k_spawn, session_id, txn_nonce,
                      sidecar_generation, policy_epoch, dict_hash, sanitizer) -> dict
    # context 検証(fail-closed) → 各クエリ canonicalize → All-or-Nothing preflight(sanitizer)
    # → framing → attest_tag。戻り値 {"abstract_queries":[q*...], "attestation": tag}
load_spawn_key_from_env(env=os.environ) -> str
    # PKB_EGRESS_KEY を読む。不在/不正は ValueError（fail-closed）
```

`sanitizer` は **E0a の本物** `core.privacy_search.EgressSanitizer`（凍結・改変禁止・**import して使うのは可**）。

---

# セクション 1: アーキテクチャと制約 (Constraints & Blast Radius)

## 1.1 データフロー

```text
abstract_queries(list[str])
  → canonicalize_outbound_query 各件 → q*（送出＝署名対象と 1 byte も違わない列）
  → All-or-Nothing preflight: 全 q* を EgressSanitizer.validate_query に通す
       1 件でも EgressViolationError なら即 abort・attest_tag 未呼出
  → attestation_framing(session_id, txn_nonce, gen, epoch, dict_hash, q*) : bytes
  → attest_tag(K_spawn, framing) : HMAC-SHA256 hex
  → {"abstract_queries": q*, "attestation": tag}
```

## 1.2 絶対禁止事項（Composer がやりがちな手抜きの事前封鎖）

**★最重要の暗号トラップ — 同じ hex 文字列でも用途で扱いが真逆:**

| 値 | 役割 | 正しい変換 | 誤り（やりがち） |
|---|---|---|---|
| `K_spawn` (64hex) | HMAC の**鍵** | `bytes.fromhex(k_spawn)` → **32 raw bytes** | `k_spawn.encode()`（64 ASCII bytes になる = 別の鍵）|
| `session_id` (32hex) | **メッセージ**の一部 | `session_id.encode("utf-8")` → **32 ASCII bytes** | `bytes.fromhex()`（16 bytes になる = 別のメッセージ）|
| `txn_nonce` (64hex) | メッセージの一部 | `.encode("utf-8")` → 64 ASCII bytes | `bytes.fromhex()` |
| `dict_hash` (64hex) | メッセージの一部 | `.encode("utf-8")` → 64 ASCII bytes | `bytes.fromhex()` |

1. **鍵だけ decode、他の hex は encode。** これを取り違えると KAT の tag が合わない（= 機械検出される）。
2. **エンディアンを絶対に固定。** u64 は必ず `n.to_bytes(8, "big")`（または `struct.pack(">Q", n)`）。
   `sys.byteorder` 依存・`"little"`・`struct.pack("Q", …)`（native）・`"=Q"`/`"<Q"` は**禁止**。
3. **クエリ長は UTF-8 バイト長。** `len(q.encode("utf-8"))`。`len(q)`（スカラー数）を使うな。
   （`日本の就職` は 5 scalar / **15 bytes**、`😀` は 1 scalar / **4 bytes** — KAT-1/KAT-3 が機械検出する。）
4. **KAT の期待値をお前の実装から生成するな（＝偽 GREEN、最悪の脆弱性）。** §2.B の framing_hex / tag_hex は
   本ブループリントが Python 標準ライブラリで事前確定した固定値である。**実装をこの固定値へ合わせよ。逆は禁止。**
5. **`==` でタグ比較しない**（将来の検証時）。`hmac.compare_digest` を使う。本 STEP は生成側だが規律として明記。
6. **golden/JSON は `encoding="utf-8"` で開け。** 既定エンコーディング（Windows は cp932）で開くと CJK/emoji で
   `UnicodeDecodeError`。`open(path, encoding="utf-8")` を厳守。
7. **`privacy_search.py` を改変するな。** import して `EgressSanitizer` を**使う**のは正当（凍結＝変更禁止であって
   利用禁止ではない）。E0a のロジックを再実装・複製するな。
8. **preflight を通す前に framing/HMAC を作るな。** All-or-Nothing の意味が壊れる（§2.C）。
9. **subprocess/ctypes/network/動的 exec を使うな。** 本モジュールは Scope E cage 下にある。使うと STEP 0 が RED。
   使ってよい import: `hmac`, `hashlib`, `unicodedata`, `os`(env のみ), `dataclasses`, `typing`,
   `collections.abc`, `core.privacy_search`（EgressSanitizer/EgressViolationError）。

## 1.3 影響範囲 (Blast Radius)

**新規作成（唯一の product 追加）:**
```text
src/python/core/e0b_intent.py            # Intent Builder v2 + HMAC attestation
tests/test_e0b_intent_step2.py           # 本 STEP の全テスト
tests/golden/e0b_attestation_kat.json    # KAT: 固定入力 → framing_hex + tag_hex（§2.B の値をそのまま）
```

**改変を許可（STEP 2.A のみ・1 行）:**
```text
tests/test_e0b_constitutional_guard.py
  → SANCTIONED_E0B_MODULES に "e0b_intent.py" を追加するだけ。他は 1 文字も変えるな。
```

**触れてはならない:**
```text
src/python/core/privacy_search.py        # E0a — import 可・改変禁止
src/python/core/e0b_attestation.py       # STEP 1 — 変更しない（本 STEP は import しなくても完結する）
src/python/core/knowledge_fetcher.py / facade.py / engine_stdio.py   # 配線は STEP 5+
apps/desktop/src-tauri/**                 # Rust は STEP 3
上記「作成/限定改変」以外の全ファイル
```

## 1.4 フレーミングの絶対規則（指揮官確定・Rust STEP 3 が鏡像実装する）

```text
attestation_framing(session_id, txn_nonce, sidecar_generation, policy_epoch, dict_hash, queries) =
      session_id.encode("utf-8")                 # 固定 32 bytes（32 hex ASCII）
   || txn_nonce.encode("utf-8")                  # 固定 64 bytes（64 hex ASCII）
   || sidecar_generation.to_bytes(8, "big")      # u64 big-endian
   || policy_epoch.to_bytes(8, "big")            # u64 big-endian
   || dict_hash.encode("utf-8")                  # 固定 64 bytes（64 hex ASCII, STEP1 PIISnapshot.hash_hex）
   || for q in queries:  len(q.encode("utf-8")).to_bytes(8, "big") || q.encode("utf-8")
tag = HMAC-SHA256(key = bytes.fromhex(K_spawn), msg = framing).hexdigest()
```

> **単射性の根拠（重要）:** session_id / txn_nonce / dict_hash は**長さ前置されていない**。この連結が単射で
> あり続けるのは、これら 3 者が**厳密固定長**（32/64/64 hex）であり、クエリ部が長さ前置で自己区切りである
> ためだけである。したがって §2.A の**固定長検証は単なる書式チェックではなくセキュリティ不変条件**であり、
> 省略・緩和は禁止。（v3 §4.1 との差分は §4 に記載。本 STEP は上記の指揮官確定フレーミングを実装する。）

---

# セクション 2: 実行フェーズ計画 (TDD Loops)

各 STEP は **RED → 検証コマンド → GREEN（実装指示）→ 完了条件** で閉じる。

---

### STEP 2.A: Fail-Closed 契約（環境不在・不正状態）

1. **RED:** `tests/test_e0b_intent_step2.py` を新規作成し、
   `from core.e0b_intent import build_attested_intent, load_spawn_key_from_env, attest_tag,
   attestation_framing, canonicalize_outbound_query` を import（モジュール未作成 → **ImportError で RED**）。
   以下の fail-closed テストを実装（すべて `pytest.raises((ValueError, TypeError))`）:
   - **K_spawn 不正:** `None` / 空 / 63 文字 / 65 文字 / 大文字を含む(`"AA…"`) / 非 hex(`"zz…"`) /
     hex だが 62 桁（=31 bytes）。→ `build_attested_intent`（および `attest_tag`）が拒否。
   - **session_id 不正:** 31/33 桁・大文字・非 hex。**txn_nonce 不正:** 63/65 桁・大文字・非 hex。
     **dict_hash 不正:** 63/65 桁・大文字・非 hex。
   - **gen / epoch 不正:** 負数 / `2**64`（境界超）/ `bool`(True) / float / str。
   - **abstract_queries 不正:** 空 list / 5 件（`MAX_FETCH_QUERIES=4` 超）/ list でない / 要素が非 str /
     canonicalize 後に空になる要素（例 `"​"`）/ 4096 バイト超のクエリ。
   - **環境不在:** `load_spawn_key_from_env(env={})` が `ValueError`。
     `load_spawn_key_from_env(env={"PKB_EGRESS_KEY": "not-hex"})` が `ValueError`。
     `load_spawn_key_from_env(env={"PKB_EGRESS_KEY": "<64 lowercase hex>"})` は 64hex 文字列を返す。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_intent_step2.py -k failclosed -v
   ```
3. **GREEN（実装）:** `src/python/core/e0b_intent.py` を作成。定数
   `MAX_FETCH_QUERIES=4`, `MAX_QUERY_BYTES=4096`（E0a 準拠・再定義）。
   厳密 validator（`type(x) is str` / 正規表現不使用の hex 判定 `all(c in "0123456789abcdef" ...)` と長さ一致 /
   `type(n) is int and not isinstance(n,bool) and 0<=n<2**64`）を実装。`canonicalize_outbound_query`（§下記）実装。
   `build_attested_intent` は先頭で全 context を検証し、不正は `ValueError`/`TypeError`。
   **`canonicalize_outbound_query` 仕様（casefold しない・match 用とは別物）:**
   ```
   1. type(q) is not str → TypeError。 surrogate(U+D800..DFFF) を含む → ValueError。
   2. 不可視/Bidi/format を除去（STEP1 REMOVE_SET のうち制御以外:
      0x00AD,0x061C,0x180E,0x200B..0x200F,0x202A..0x202E,0x2060..0x2064,0x2066..0x2069,0xFEFF）。
   3. 除去後になお C0/C1 制御(0x00..0x1F,0x7F..0x9F, TAB/CR/LF 含む)が残れば ValueError（fail-closed・畳まない）。
   4. NFC 正規化（NFKC でも casefold でもない）。
   5. strip 後 空 → ValueError。 UTF-8 バイト長 > MAX_QUERY_BYTES → ValueError。
   ```
4. **完了条件:** failclosed 系全 PASS。**→ コミット。**

---

### STEP 2.B: 暗号論的フレーミングと HMAC 既知ベクトル（Cryptographic Golden RED）

**★以下の KAT は本ブループリントが Python 標準ライブラリで事前確定した固定値である。golden ファイルへ
そのまま格納し、実装がこの値を再現することを要求する。実装から生成し直すことを固く禁ずる。**

共通鍵・コンテキスト（全 KAT 共通）:
```
K_spawn    = 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f   (32 bytes)
session_id = 0123456789abcdeffedcba9876543210                                     (32 hex)
txn_nonce  = 00112233445566778899aabbccddeeffffeeddccbbaa99887766554433221100     (64 hex)
dict_hash  = f3b8fd0c8070d2127fd0b3daaacd12c25ab5e819cd3c7d17d11cd9fa632d34e0     (64 hex)
```

| name | gen | epoch | queries | framing 長 |
|---|---|---|---|---|
| KAT-1 | 7 | 3 | `["ai career", "日本の就職"]` | 216 B |
| KAT-2 | 1 | 1 | `["python async"]` | 196 B |
| KAT-3 | 7 | 3 | `["😀 test"]` | 193 B |

**KAT-1** — framing_hex:
```
30313233343536373839616263646566666564636261393837363534333231303030313132323333343435353636373738383939616162626363646465656666666665656464636362626161393938383737363635353434333332323131303000000000000000070000000000000003663362386664306338303730643231323766643062336461616163643132633235616235653831396364336337643137643131636439666136333264333465300000000000000009616920636172656572000000000000000fe697a5e69cace381aee5b0b1e881b7
```
KAT-1 tag_hex: `6c4390f16549039ca60fca51104a4a6c204252d6ce0583d4237eae27c328d9fa`

**KAT-2** — framing_hex:
```
3031323334353637383961626364656666656463626139383736353433323130303031313232333334343535363637373838393961616262636364646565666666666565646463636262616139393838373736363535343433333232313130300000000000000001000000000000000166336238666430633830373064323132376664306233646161616364313263323561623565383139636433633764313764313163643966613633326433346530000000000000000c707974686f6e206173796e63
```
KAT-2 tag_hex: `c36b1fe4c46564af2dfd5310c771b5bf95eb81daff071a2e8b9bb87aac284861`

**KAT-3** — framing_hex:
```
30313233343536373839616263646566666564636261393837363534333231303030313132323333343435353636373738383939616162626363646465656666666665656464636362626161393938383737363635353434333332323131303000000000000000070000000000000003663362386664306338303730643231323766643062336461616163643132633235616235653831396364336337643137643131636439666136333264333465300000000000000009f09f98802074657374
```
KAT-3 tag_hex: `dec75bd5da278801c3d75074e329a8349c483263f799519164f5dc17dd9ccee8`

1. **RED:** `tests/golden/e0b_attestation_kat.json` を作成し、上記 3 件を格納
   （各要素: `name, k_spawn, session_id, txn_nonce, sidecar_generation, policy_epoch, dict_hash, queries[],
   framing_hex, tag_hex`。**queries は JSON に UTF-8 リテラルで記述し、ファイルは UTF-8 保存**）。
   `test_e0b_intent_step2.py` に:
   - `test_framing_matches_golden`: 各 KAT で
     `attestation_framing(...).hex() == framing_hex`（バイト列一致）。
   - `test_tag_matches_golden`: 各 KAT で `attest_tag(k_spawn, bytes.fromhex(framing_hex)) == tag_hex`。
   - `test_tag_end_to_end`: 各 KAT で `attest_tag(k_spawn, attestation_framing(...)) == tag_hex`。
   - `test_query_byte_length_not_scalar`: KAT-1 の CJK と KAT-3 の emoji で framing の該当長さ前置が
     `000000000000000f`(15) / `0000000000000009`(9) であること（スカラー長 5/6 でないこと）。
   - **敏感性（1 bit で総崩れ）:** K_spawn の末尾 1 文字を変える / gen を 7→8 にする / query の 1 文字を変える
     と tag が golden と**異なる**こと（改竄検出の証明）。
   - **JSON エンコーディング:** golden を `open(..., encoding="utf-8")` で読むこと（cp932 罠の回避を明示）。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_intent_step2.py -k "framing or tag or byte_length" -v
   ```
3. **GREEN（実装）:** `attestation_framing`（§1.4 の順序・`to_bytes(8,"big")`・UTF-8 バイト長）と
   `attest_tag`（`hmac.new(bytes.fromhex(k_spawn), framing, hashlib.sha256).hexdigest()`）を実装。
   `attest_tag` は k_spawn の 64 lowercase hex 検証も行い、不正なら `ValueError`。
4. **完了条件:** KAT 系全 PASS（framing・tag ともに 3 件 byte-exact）。**→ コミット。**

---

### STEP 2.C: All-or-Nothing トランザクション（Preflight RED）

1. **RED:** `test_e0b_intent_step2.py` に追加。E0a の本物 `EgressSanitizer` を用いる:
   ```
   from core.privacy_search import EgressSanitizer, EgressViolationError
   ```
   - **1 件違反で全体 abort・署名ゼロ:** PII 用語（例 `["himitsu"]`）を持つ `EgressSanitizer` を作り、
     `abstract_queries = ["ai career", "himitsu project", "python async"]` を
     `build_attested_intent(...)` へ。`unittest.mock.patch("core.e0b_intent.attest_tag")` で attest_tag を
     mock 化し、**`EgressViolationError` が raise され、かつ `mock.call_count == 0`** を assert。
   - **違反位置に依らない:** 違反クエリを**末尾**に置いた場合も、先頭安全クエリが署名を発生させない
     （`call_count == 0`）こと。戻り値（部分結果）も生成されないこと。
   - **全件安全なら 1 回だけ署名:** 違反語を含まないクエリ列で、attest_tag が**厳密に 1 回**呼ばれる
     （`call_count == 1`）こと。mock を外した実経路で戻り値 `{"abstract_queries":[...], "attestation": tag}` の
     `attestation` が 64 lowercase hex であること。
   - **preflight は canonicalize 後の q* に対して走る:** 不可視で隠した PII（例 query に
     `"himi​tsu"` を混ぜる）が canonicalize_outbound（不可視除去）後に
     サニタイザ照合対象となり弾かれること（サニタイザ側 normalize と合わせ二重に防がれることの確認）。
   - **統合＝KAT 一致:** 「全 query を素通しする」サニタイザ（PII 用語なし）を使い、KAT-1 と同一の
     context・クエリで `build_attested_intent(...)` を呼ぶと、戻り値 `attestation == KAT-1 tag_hex`。
     （build 経路が framing/HMAC 原始関数と同じ golden へ収束することの証明。canonicalize は clean 入力に恒等。）
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_intent_step2.py -k "preflight or allornothing or integration" -v
   ```
3. **GREEN（実装）:** `build_attested_intent` を実装。順序厳守:
   ```
   context 検証(fail-closed) → q* = [canonicalize_outbound_query(q) for q in abstract_queries]
   → for q in q*: sanitizer.validate_query(q)   # EgressViolationError は捕捉せず伝播（abort）
   → framing = attestation_framing(session_id, txn_nonce, gen, epoch, dict_hash, q*)
   → tag = attest_tag(k_spawn, framing)
   → return {"abstract_queries": list(q*), "attestation": tag}
   ```
   preflight ループを framing/attest より**必ず前**に置く。例外を握り潰さない。
4. **完了条件:** preflight 系全 PASS（call_count 0/1 の証明含む）。**→ コミット。**

---

### STEP 2.D: 全回帰 + HARD STOP

1. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_intent_step2.py tests/test_e0b_attestation_step1.py tests/test_e0b_constitutional_guard.py -v
   python -m pytest tests/ -q
   ```
2. **完了条件:** 全 PASS（STEP 0/1 非回帰。STEP 0 は `e0b_intent.py` を Scope E cage で監視しつつ緑）。**→ HARD STOP。**

---

# セクション 3: コミットと停止規律 (Checkpoints & HARD STOP)

## 3.1 コミット規律

`git status --short` で目視し、当該 STEP の対象ファイルのみ stage（`git add -A`/`.` 禁止）。
**`.claude/` を stage しない。push 禁止。** revert/reset/`checkout --` 禁止。既存コミット改変禁止。

| STEP | stage 対象 | コミットメッセージ |
|---|---|---|
| 2.A | `tests/test_e0b_constitutional_guard.py`, `src/python/core/e0b_intent.py`, `tests/test_e0b_intent_step2.py` | `feat(e0b-step2): STEP 2.A — sanction e0b_intent in Scope E + fail-closed context validation` |
| 2.B | `src/python/core/e0b_intent.py`, `tests/test_e0b_intent_step2.py`, `tests/golden/e0b_attestation_kat.json` | `feat(e0b-step2): STEP 2.B — length-prefixed framing + HMAC-SHA256 known-answer vectors (byte-exact)` |
| 2.C | `src/python/core/e0b_intent.py`, `tests/test_e0b_intent_step2.py` | `feat(e0b-step2): STEP 2.C — all-or-nothing sanitizer preflight (HMAC call-count 0 on violation)` |

各コミット末尾:
```
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
```
（STEP 2.A で `e0b_intent.py` を作成した瞬間に STEP 0 の cage が実働するため、2.A の stage には
`test_e0b_constitutional_guard.py`（SANCTIONED 追加）と `e0b_intent.py` を同一コミットに含め、常に緑を保つ。
STEP 2.D はコード差分なしのため空コミットを作らない。）

## 3.2 HARD STOP（絶対命令）

STEP 2.D で全スイート GREEN・3 コミット済みに到達したら、**出力を停止しユーザー（指揮官）へ報告し ACK を待て。**
報告に必ず含めるもの:
1. `test_e0b_intent_step2.py` のテスト一覧と各 PASS/FAIL 実測値。
2. 検証コマンドの生ターミナル出力（STEP 0/1 非回帰の実数を含む）。
3. `git log --oneline`（2.A/2.B/2.C の 3 コミット、対象ファイルのみ）。
4. **生成された framing_hex（3 KAT）と HMAC tag（3 KAT）** を報告し、本ブループリント §2.B の固定値と
   **完全一致**していることを明記（指揮官が KAT を突合できるように）。
5. **Rust STEP 3 未着手の明言**（`facade.py`/`engine_stdio.py`/`src-tauri/**` 変更ゼロ）。

**ACK なしに STEP 3（Rust Dual-Run 独立再検査）へ進むな。**

## 3.3 HARD STOP（異常時）

- **KAT 不一致:** golden やテストを実装へ合わせて書き換える（偽 GREEN）ことは**厳禁**。疑うべき順序:
  ① 鍵を `bytes.fromhex` で decode したか（`encode` していないか）② session/nonce/dict_hash を `encode` したか
  （`fromhex` していないか）③ `to_bytes(8,"big")` か ④ クエリ長が UTF-8 バイト長か。それでも解けなければ
  実 framing_hex と golden を並べて提示し停止・裁定待ち。
- **STEP 0 が RED（cage 違反）:** `e0b_intent.py` が subprocess/ctypes/network/動的 exec を使用。除去して報告。
- **`privacy_search.py` 改変が必要に見えた:** 誤読。`EgressSanitizer` は import して使うだけ。停止して報告。

## 3.4 STEP 2 完了の定義 (DoD)

- `attestation_framing` が §1.4 の順序で 3 KAT を byte-exact 再現（UTF-8 バイト長・big-endian・鍵は非関与）。
- `attest_tag` が 3 KAT の HMAC-SHA256 tag を byte-exact 再現（鍵は `bytes.fromhex` の 32B）。
- fail-closed 契約（不正 K_spawn/session/nonce/dict_hash/gen/epoch/queries・env 不在）を全網羅。
- All-or-Nothing: 1 件違反で `EgressViolationError` かつ `attest_tag` call_count == 0、全件安全で == 1。
- `EgressSanitizer`（E0a 本物）を import 利用、`privacy_search.py` 差分ゼロ。product 追加は `e0b_intent.py` のみ。
- Scope E cage が `e0b_intent.py` を監視し STEP 0/1 は非回帰。
- Rust STEP 3 未着手・ACK 待ちで停止。

---

# セクション 4: v3 §4.1 との差分（指揮官確認事項・実装は本 STEP のフレーミングで確定）

本 STEP は**指揮官が STEP 2 指令で明示したフレーミング**（§1.4）を実装し、KAT もそれで確定した。
参考として、v3 §4.1 の attestation フレーミングとの差分を挙げる（**将来 STEP での reconciliation を要すれば別裁定**）:

1. **ドメイン分離定数** `b"PKB-E0B-EGRESS-V3"` を v3 は先頭に置くが、本 STEP のフレーミングには無い。
   将来プロトコル版数を分離したい場合は追加を検討（現状は単一プロトコルのため機能影響なし）。
2. **`provider_id`** を v3 は含む（cross-provider リプレイ束縛）。本 STEP は provider を署名に含めない。
   v1 provider は Wikipedia 単一のため現状リスクは低いが、Provider 追加時（E0c）に束縛の要否を再検討。
3. **`n_queries` カウント**を v3 は含む。本 STEP は含めないが、**クエリ部が長さ前置で自己区切り＋前段が
   固定長**のため単射性は保たれる（§1.4）。カウント追加は defense-in-depth の余地。
4. **session_id/txn_nonce/dict_hash の扱い:** v3 は raw bytes を長さ前置（`framed()`）。本 STEP は指揮官指定どおり
   **hex 文字列を UTF-8 エンコードし固定長で非長さ前置**連結。単射性は固定長検証（§2.A）に依存する。

いずれも「実装を止める」問題ではない。指揮官が v3 準拠へ寄せる判断をする場合のみ、別 RED・別コミットで反映する。
