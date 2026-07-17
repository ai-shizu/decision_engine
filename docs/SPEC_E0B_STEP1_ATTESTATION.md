# SPEC — E0b STEP 1: Attestation v2 Foundation (Composer-Executable Blueprint)

> **読者:** 自律型コーディング AI（Cursor Composer 等）。人間向け解説ではない。**一字一句従え。裁量なし。**
> **親仕様:** `docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md` §3.2/§3.3/§4.1（承認・ロック済み）。
> **前提:** STEP 0（憲法ガード）は 10/10 GREEN でロック済み。無菌状態は機械契約で保護されている。
> **本 STEP の目的:** Rust の Dual-Run 独立再検査（将来 STEP 3）が成立する土台を作る。
> Python が生成する「正規化文字列」と「スナップショットハッシュ」は、**将来の Rust 実装と 1 バイトの
> 狂いもなく一致（byte-exact）**しなければならない。本 STEP は Python 側 + 越境 golden の確定まで。
> **ACK 規律:** STEP 1.D 完了時に HARD STOP。指揮官 ACK まで STEP 2 へ進むな。

---

## 0. 何を作るか（2 つの成果物）

1. **`canonicalize_for_match(s: str) -> str`** — 不可視/Bidi/制御を除去し NFKC+casefold する決定論的正規化。
   E0a `privacy_search._normalize_for_matching`（NFKC+casefold のみ・**凍結**）より強力な**別関数**。
2. **`PIISnapshot`** — 保護対象エンティティ列 + 単調 revision から、順序非依存・長さ前置単射エンコーディング
   による SHA-256 を持つ**不変**オブジェクト。

両者は新規モジュール **`src/python/core/e0b_attestation.py`** に実装する（モジュール名は厳守。理由は §1.3）。

---

# セクション 1: アーキテクチャと制約 (Constraints & Blast Radius)

## 1.1 データフロー（STEP 1 の位置）

```text
[将来] 保護対象エンティティ(profile/contact/alias/旧名/住所/略称)
   → build_pii_dictionary_snapshot()  ← 本 STEP ではまだ配線しない（供給元一本化は別 STEP）
   → PIISnapshot.build(raw_terms, revision)
        → 各 raw_term を canonicalize_for_match() で正規化 → dedup → UTF-8 バイト順 sort
        → 長さ前置単射プリイメージ → SHA-256 → hash_hex
   → [将来 STEP 3] Rust が同一アルゴリズムで再計算し byte-exact 一致を検証（越境 golden で今ロック）
```

本 STEP は**純粋な文字列 + ハッシュ処理**のみ。ネットワーク・IPC・ファイル書込・subprocess は一切ない。

## 1.2 絶対禁止事項（Composer がやりがちな手抜きの事前封鎖）

**★最重要の言語間トラップ（Python↔Rust）— これを外すと Dual-Run が崩壊する:**

1. **長さは「UTF-8 バイト長」であって「スカラー数」ではない。** framing の長さ前置は必ず
   `len(t.encode("utf-8"))`。`len(t)`（Python のスカラー数）を使うな。Rust は `t.len()`（= バイト長）を使う。
   - `len("日本") == 2` だが byte 長は **6**。`len("😀") == 1` だが byte 長は **4**。ここを間違えると
     ASCII テストは通るが CJK/emoji で Rust と衝突する。§2 STEP 1.C の CJK/emoji プリイメージで必ず検証。
2. **`str.casefold()` を使え。`str.lower()` を使うな。** `lower()` は ß/İ/ς 等で誤る。casefold は
   Unicode 完全ケースフォールド（locale 非依存）で Rust `caseless` と一致する。
3. **除去集合に `unicodedata.category()` を使うな。** カテゴリ判定は Python の Unicode 版と Rust クレートの
   版がずれると結果が変わり、byte-exact が壊れる。**§2 で与える明示的な frozen コードポイント集合**を
   ハードコードせよ（Python も Rust も同一の固定テーブル）。
4. **正規化形は厳密に NFKC。** NFC/NFD と混同するな。ケースフォールド後の**末尾 NFKC 再適用**（冪等化）を
   省くな（§2 のアルゴリズム順を 1 手も変えるな）。
5. **正規表現を使うな。** 決定論性と ReDoS 回避のため、除去は明示的コードポイント集合の内包表記で行う。
6. **golden の期待値をお前の実装から生成するな（= 偽 GREEN）。** golden は本ブループリントが仕様から
   手で確定した値である。**実装を golden に合わせる**のであって、その逆は禁止。
7. **サロゲートを黙って通すな。** Python `str` は lone surrogate(U+D800–U+DFFF) を保持できるが Rust `&str` は
   構造的に保持できない。parity 保証のため canonicalize は surrogate を含む入力を **ValueError で fail-closed**。
8. **正規化後に空になる term を黙って捨てるな。** 保護エンティティが消えると防御が漏れる → **ValueError**。
9. **`u64_be` は正確に 8 バイト big-endian。** `int.to_bytes(8, "big")`。`struct` の桁・エンディアン間違いに注意。
10. **不変性を本物にせよ。** frozen dataclass + tuple フィールド。mutation が例外を上げることをテストで証明。
11. **`privacy_search.py` を import も改変もするな。** 定数値が同じでも新規モジュールに再定義（独立性）。
12. **ハッシュへ入れる前に UTF-8 バイト順で sort、canonical 後に dedup。** dict/set の反復順に依存するな。

## 1.3 影響範囲 (Blast Radius)

**新規作成（唯一の product 追加）:**
```text
src/python/core/e0b_attestation.py            # canonicalize_for_match + PIISnapshot
tests/test_e0b_attestation_step1.py           # 本 STEP の全テスト
tests/golden/e0b_canonicalize_golden.jsonl    # 越境 golden（入出力を UTF-8 hex で記述）
tests/golden/e0b_pii_snapshot_golden.json     # プリイメージ hex + 期待 hash
```

**改変を許可（STEP 1.A のみ・限定）:**
```text
tests/test_e0b_constitutional_guard.py
  → 関数 test_e0b_scoped_modules_absent_or_caged **だけ**を許可リスト形式へ昇格（§2 STEP 1.A）。
    他の関数・他行は 1 文字も変えるな。これは STEP 0 が設計した「予約ケージ発動」であり正当な手続き。
```

**モジュール名は `e0b_attestation.py` で厳守する理由:** basename が STEP 0 の Scope E グロブ `*e0b*.py` に
一致し、`DENY_E0B`（subprocess/ctypes/multiprocessing/network/動的 exec 禁止）の檻へ**自動的に収監**される。
したがって本モジュールはそれらを一切使ってはならない（使うと STEP 0 スイートが RED になる = 正しい防御作動）。
使ってよいのは `unicodedata` / `hashlib` / `dataclasses` / `typing` / `collections.abc` のみ。

**触れてはならない:**
```text
src/python/core/privacy_search.py             # E0a Sanitizer — 凍結
src/python/core/knowledge_fetcher.py          # E0a stub — 凍結
src/python/core/facade.py / engine_stdio.py   # 配線は別 STEP
apps/desktop/src-tauri/**                      # Rust は STEP 3。ここでは書かない
上記「作成/限定改変」以外の全ファイル
```

---

# セクション 2: 実行フェーズ計画 (TDD Loops)

各 STEP は **RED → 検証コマンド → GREEN（実装指示）→ 完了条件** で閉じる。golden はすべて
**UTF-8 バイト列の hex** で記述する（golden ファイル自身の正規化曖昧さを排除するため）。

---

### STEP 1.A: Scope E ケージ発動（予約檻を許可リスト形式へ昇格）

新モジュール `e0b_attestation.py` は STEP 0 の Scope E グロブに一致し、現状の
`assert scoped == []`（clause a）を破る。これを**サンクション済み許可リスト**へ昇格させる。

1. **RED（実測手順）:** まず現状を確認。`e0b_attestation.py` はまだ無いので STEP 0 スイートは 10 passed のはず。
   これから作るモジュールを見越して、`tests/test_e0b_constitutional_guard.py` の
   `test_e0b_scoped_modules_absent_or_caged` **のみ**を次のとおり変更する:
   - モジュール直下に `SANCTIONED_E0B_MODULES = frozenset({"e0b_attestation.py"})` を追加（テストファイル内）。
   - clause (a) の `assert scoped == []` を、**未サンクションの e0b_* モジュールが存在しない**ことの assert へ置換:
     `unexpected = [p for p in scoped if p.name not in SANCTIONED_E0B_MODULES]` → `assert unexpected == []`。
   - clause (b) の cage ループはそのまま維持（サンクション済みモジュールが出現したら DENY_E0B を必ず適用）。
   - docstring に「STEP 1.A: e0b_attestation.py をサンクションし、Scope E cage を実働させた」と明記。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_constitutional_guard.py -v
   ```
3. **GREEN（実装）:** product コードなし。モジュール未作成でも `scoped` は空 → `unexpected == []` で PASS。
   許可リスト形式が「ゼロ件」に対して後方互換であることを実測する。
4. **完了条件:** STEP 0 スイートが依然 **10 passed**。cage は許可リストへ昇格済みで、未サンクションの
   e0b_* が今後現れたら即 RED になる（ratchet 維持）。**→ コミット（§3）。**

---

### STEP 1.B: `canonicalize_for_match` — 敵対的 Unicode 越境（Adversarial Unicode RED）

**確定アルゴリズム（順序を 1 手も変えるな。Rust STEP 3 はこれを鏡像実装する）:**
```text
canonicalize_for_match(s):
  1. s に surrogate (U+D800..U+DFFF) が 1 つでもあれば ValueError（fail-closed）。
  2. REMOVE_SET の各コードポイントを除去（明示 frozenset・下記）。
  3. NFKC 正規化。
  4. casefold。
  5. NFKC 正規化（冪等化ブラケット）。
  6. WHITESPACE_SET の各文字を U+0020 へ写像 → 連続する U+0020 を 1 個へ畳む → 先頭末尾の U+0020 を除去。
  戻り値: 上記の str（空文字列もあり得る。ここでは reject しない — reject は PIISnapshot 側）。

REMOVE_SET（明示コードポイント・カテゴリ判定禁止）:
  0x00..0x08, 0x0E..0x1F, 0x7F..0x9F,     # C0(空白系を除く)/DEL/C1
  0x00AD,                                  # SOFT HYPHEN
  0x061C,                                  # ARABIC LETTER MARK
  0x180E,                                  # MONGOLIAN VOWEL SEPARATOR
  0x200B, 0x200C, 0x200D, 0x200E, 0x200F,  # ZWSP ZWNJ ZWJ LRM RLM
  0x202A, 0x202B, 0x202C, 0x202D, 0x202E,  # LRE RLE PDF LRO RLO
  0x2060, 0x2061, 0x2062, 0x2063, 0x2064,  # WJ + 不可視演算子
  0x2066, 0x2067, 0x2068, 0x2069,          # LRI RLI FSI PDI
  0xFEFF                                    # BOM / ZWNBSP

WHITESPACE_SET（U+0020 へ写像する空白系）:
  0x09, 0x0A, 0x0B, 0x0C, 0x0D,            # TAB LF VT FF CR（tab/改行による回避を封鎖）
  0x20, 0x2028, 0x2029                     # SPACE, LINE SEP, PARA SEP
  （NFKC が他の Zs を U+0020 へ畳むため、明示集合はこれで十分。カテゴリ判定は使うな）
```

1. **RED:** `tests/test_e0b_attestation_step1.py` を新規作成し、
   `from core.e0b_attestation import canonicalize_for_match` を import（この時点でモジュール未作成 →
   **ImportError で RED**）。さらに以下の敵対テストを実装:
   - **golden 駆動テスト:** `tests/golden/e0b_canonicalize_golden.jsonl` を読み、各行 `{name, input_hex,
     expected_hex}` について `canonicalize_for_match(bytes.fromhex(input_hex).decode("utf-8")).encode("utf-8")
     .hex() == expected_hex` を assert。golden は下表を**そのまま**作成（期待値は本ブループリントが仕様から
     確定済み。実装から生成するな）:

     | name | input（説明） | input_hex | expected 文字列 | expected_hex |
     |---|---|---|---|---|
     | zwsp_evasion | `Atsuki`+U+200B+` Ishizu` | `417473756b69e2808b20497368697a75` | `atsuki ishizu` | `61747375 6b692069 73686979 7a75`→`617473756b69206973686979 7a75` |
     | plain_ref | `Atsuki Ishizu` | `417473756b6920497368697a75` | `atsuki ishizu` | `617473756b69206973686979 7a75` |
     | rlo_prefix | U+202E+`Atsuki` | `e2808e...`※下記注 | `atsuki` | `617473756b69` |
     | soft_hyphen | `Atsu`+U+00AD+`ki` | `41747375c2ad6b69` | `atsuki` | `617473756b69` |
     | fullwidth | `ＡＴＳＵＫＩ`(FF21..) | `efbca1efbcb4efbcb3efbcb5efbcabefbca9` | `atsuki` | `617473756b69` |
     | ligature_fi | `ﬁle`(U+FB01) | `efac816c65` | `file` | `66696c65` |
     | combining_nfc | `e`+U+0301+`` | `65cc81` | `é` | `c3a9` |
     | precomposed | `é`(U+00E9) | `c3a9` | `é` | `c3a9` |
     | dotted_I | `İ`(U+0130) | `c4b0` | `i`+U+0307 | `69cc87` |
     | superscript2 | `²`(U+00B2) | `c2b2` | `2` | `32` |
     | halfwidth_kana | `ｱｲｳ`(FF71..) | `efbdb1efbdb2efbdb3` | `アイウ` | `e382a2e382a4e382a6` |
     | tab_evasion | `John`+TAB+`Smith` | `4a6f686e094 8...`※ | `john smith` | `6a6f686e20736d697468` |
     | bidi_isolate | U+2066+`secret`+U+2069 | `e281a673656372657426...`※ | `secret` | `736563726574` |
     | double_space | `Atsuki  Ishizu`(2×SP) | `417473756b692020497368697a75` | `atsuki ishizu` | `617473756b69206973686979 7a75` |
     | empty_after | U+200B+U+FEFF | `e2808befbbbf` | ``(空) | `` |

     ※ input_hex に不明確な箇所があれば、**Composer は該当 Unicode 文字を UTF-8 エンコードして hex 化した
     正確な値を用いよ**（U+202E=`e2808e`ではなく正しくは `e280ae`; U+2066=`e281a6`; U+2069=`e281a9`;
     TAB=`09`）。上表の expected 文字列が正典であり、input を正しく UTF-8 hex 化すること。曖昧なら
     expected 文字列側を信頼し、input はその評価対象を UTF-8 で表したものとして生成せよ。
   - **surrogate fail-closed:** `canonicalize_for_match("\ud800")` が `ValueError` を上げること。
   - **冪等性:** 上表の全 input と全 expected について
     `canonicalize_for_match(canonicalize_for_match(x)) == canonicalize_for_match(x)`。
   - **カナリア（陰性）:** 既に正規な ASCII `"john smith"` が不変で返ること（過剰除去がないこと）。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_attestation_step1.py -k canonical -v
   ```
   （RED 段階では collection error / ImportError を実測。）
3. **GREEN（実装）:** `src/python/core/e0b_attestation.py` を新規作成し、上記アルゴリズムどおりに
   `canonicalize_for_match` を実装。REMOVE_SET / WHITESPACE_SET は明示 frozenset。`re` 不使用。
   `unicodedata.normalize("NFKC", ...)` と `str.casefold()` のみ使用。surrogate 検査を最初に置く。
4. **完了条件:** canonical 系テスト全 PASS。かつ **STEP 0 スイートが依然 10 passed**
   （新モジュールが Scope E cage を通過 = subprocess/ctypes/network/動的 exec 不使用の証明）。**→ コミット。**

---

### STEP 1.C: `PIISnapshot` — 単射性・不変性（Snapshot RED）

**確定エンコーディング（Rust STEP 3 が鏡像実装する）:**
```text
preimage(terms_canonical_dedup_sorted, revision) =
    b"PKB-PII-DICT-V1"                       # 15 byte 固定ドメイン分離（長さ前置しない固定定数）
 || revision.to_bytes(8, "big")
 || for t in terms:  len(t.encode("utf-8")).to_bytes(8, "big") || t.encode("utf-8")
hash_hex = sha256(preimage).hexdigest()      # 64 lowercase hex
term 列は: 各 raw_term を canonicalize_for_match → 空は ValueError → dedup → UTF-8 バイト順に sort。
PIISnapshot は frozen: fields = (revision:int, terms:tuple[str,...], hash_hex:str)。
PIISnapshot.build(raw_terms, revision, *, previous=None):
  - revision は非負 int、previous 指定時は revision > previous.revision（単調）else ValueError。
  - canonical term の UTF-8 長 > 4096 は ValueError（E0a MAX_PII_TERM_BYTES 準拠・再定義）。
  - term 数 > 100_000 は ValueError（MAX_PII_TERMS 準拠）。
```

1. **RED:** `test_e0b_attestation_step1.py` に追加:
   - **プリイメージ hex（手確定・単射性）:** `tests/golden/e0b_pii_snapshot_golden.json` を作成し、下記を
     そのまま格納。テストは `preimage(...)` の**バイト列**が golden の `preimage_hex` と一致し、かつ
     `snapshot.hash_hex == sha256(bytes.fromhex(preimage_hex)).hexdigest()` を assert:

     | name | terms(canonical,sorted) | revision | preimage_hex |
     |---|---|---|---|
     | ab_c | `["ab","c"]` | 1 | `504b422d5049492d444943542d5631` + `0000000000000001` + `0000000000000002`+`6162` + `0000000000000001`+`63` |
     | a_bc | `["a","bc"]` | 1 | `504b422d5049492d444943542d5631` + `0000000000000001` + `0000000000000001`+`61` + `0000000000000002`+`6263` |
     | cjk | `["日"]` | 1 | `504b422d5049492d444943542d5631` + `0000000000000001` + `0000000000000003`+`e697a5` |
     | emoji | `["😀"]` | 1 | `504b422d5049492d444943542d5631` + `0000000000000001` + `0000000000000004`+`f09f9880` |
     | rev2 | `["a"]` | 2 | `504b422d5049492d444943542d5631` + `0000000000000002` + `0000000000000001`+`61` |
     | empty | `[]` | 1 | `504b422d5049492d444943542d5631` + `0000000000000001` |

     （`preimage_hex` は連結値。空白は可読性のためで、実ファイルでは連結した 1 本の hex 文字列。）
   - **単射性（衝突不能）:** `ab_c` と `a_bc` のプリイメージ・hash が**異なる**こと
     （長さ前置が無ければ両者は `abc` に潰れて衝突する — これが防げていることの証明）。
   - **バイト長 vs スカラー長:** `cjk`/`emoji` のプリイメージが**バイト長**（3/4）を使っていること。
     もしスカラー長（1/1）を使うと hex がずれるので golden 不一致で落ちる（トラップ #1 の機械検出）。
   - **順序非依存:** `build(["c","ab"], 1)` と `build(["ab","c"], 1)` の `hash_hex` が一致。
   - **canonical+dedup 統合:** `build(["Atsuki​ Ishizu"], 1)` と
     `build(["ATSUKI ISHIZU", "atsuki  ishizu"], 1)` の `hash_hex` が一致（ゼロ幅回避が辞書段で閉じる証明）。
   - **revision が hash に効く:** `rev2` と、同 terms `["a"]` の revision=1 で `hash_hex` が異なる。
   - **境界/例外:** 空 term（`build(["​"], 1)` → ValueError）; 非単調（previous.revision=5, revision=3
     → ValueError）; 負 revision → ValueError; 4096 バイト超 canonical term → ValueError; 空スナップショット
     `build([], 1)` は `empty` golden の hash を持つ有効オブジェクト。
   - **不変性:** frozen — `snapshot.revision = 9` が例外（`dataclasses.FrozenInstanceError`）; `terms` が
     `tuple` 型; 返却 tuple の要素改変不能。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_attestation_step1.py -k snapshot -v
   ```
3. **GREEN（実装）:** `e0b_attestation.py` に `PIISnapshot`（frozen dataclass）と `build` classmethod、
   `preimage` 純関数を実装。`hashlib.sha256`、`int.to_bytes(8,"big")`、`sorted(...)`（UTF-8 バイト順 =
   Python str 既定 sort）を使用。dedup は canonical 後に順序安定で行う。
4. **完了条件:** snapshot 系テスト全 PASS。**→ コミット。**

---

### STEP 1.D: 全回帰 + HARD STOP

1. **RED（最終実測）:** 全スイートを実行し RED/GREEN 実数を取得。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_attestation_step1.py tests/test_e0b_constitutional_guard.py -v
   python -m pytest tests/ -q
   ```
3. **GREEN:** なし。
4. **完了条件:** 上記が全 PASS（STEP 0 も含め非回帰）。既存 Python スイートに退行なし。**→ §3 HARD STOP。**

---

# セクション 3: コミットと停止規律 (Checkpoints & HARD STOP)

## 3.1 コミット規律（各 STEP GREEN 直後・次へ進む前に必須）

`git status --short` で差分を目視し、**当該 STEP の対象ファイルのみ**を stage する。`git add -A`/`git add .` 禁止。
**`.claude/` を絶対に stage しない。push 禁止。** revert/reset/`checkout --` 禁止。既存コミット改変禁止。

| STEP | stage 対象 | コミットメッセージ |
|---|---|---|
| 1.A | `tests/test_e0b_constitutional_guard.py` | `test(e0b-step1): STEP 1.A — activate Scope E cage via sanctioned e0b_attestation allowlist` |
| 1.B | `src/python/core/e0b_attestation.py`, `tests/test_e0b_attestation_step1.py`, `tests/golden/e0b_canonicalize_golden.jsonl` | `feat(e0b-step1): STEP 1.B — canonicalize_for_match with adversarial Unicode golden (byte-exact)` |
| 1.C | `src/python/core/e0b_attestation.py`, `tests/test_e0b_attestation_step1.py`, `tests/golden/e0b_pii_snapshot_golden.json` | `feat(e0b-step1): STEP 1.C — PIISnapshot injective length-prefixed SHA-256 + immutability` |

各コミット末尾に:
```
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
```
（STEP 1.D はコード差分なしのため空コミットを作らない。）

## 3.2 HARD STOP（絶対命令）

STEP 1.D で全スイート GREEN・全 STEP コミット済みに到達したら、**出力を停止しユーザー（指揮官）へ報告し、
ACK を待て。** 以下を報告:
1. `tests/test_e0b_attestation_step1.py` のテスト一覧と各 PASS/FAIL 実測値。
2. 検証コマンドの生ターミナル出力（RED/GREEN 実数、STEP 0 非回帰の 10 passed 含む）。
3. `git log --oneline`（1.A/1.B/1.C の 3 コミット。対象ファイルのみを含むこと）。
4. 新規 golden 2 ファイルの内容要約（Rust STEP 3 が消費する越境契約であることの明示）。
5. **STEP 2 未着手の明言**（`facade.py`/`engine_stdio.py`/Rust への変更ゼロ）。

**ACK なしに STEP 2（Attestation 署名・intent.build IPC）へ進むな。**

## 3.3 HARD STOP（異常時）

- **予期せぬ RED で golden 不一致:** テストや golden を実装に合わせて書き換える（偽 GREEN 化）ことは
  **禁止**。実装のアルゴリズム順・byte 長・エンディアン・casefold・除去集合を疑い、それでも解けなければ
  差分（期待 hex vs 実 hex）を提示して停止し裁定を待て。
- **STEP 0 スイートが RED（cage 違反）:** `e0b_attestation.py` が subprocess/ctypes/network/動的 exec を
  使っている。設計違反なので当該実装を除去して停止し報告せよ（cage を緩めるな）。
- **`privacy_search.py` 改変が必要に見えた:** 設計誤読。E0b は独立モジュールで完結する。停止して報告。

## 3.4 STEP 1 完了の定義 (DoD)

- `canonicalize_for_match` が敵対 Unicode golden（ゼロ幅/Bidi/結合/全角/合字/casefold/tab）を byte-exact で通過。
- surrogate fail-closed / 冪等性が証明済み。
- `PIISnapshot` が長さ前置単射エンコーディング・順序非依存・単調 revision・不変性・境界例外を満たす。
- 越境 golden 2 種が commit 済み（Rust STEP 3 の byte-exact 契約を固定）。
- Scope E cage が `e0b_attestation.py` を実働監視し、STEP 0 は 10 passed を維持。
- `privacy_search.py` ほか凍結資産に差分ゼロ。product 追加は `e0b_attestation.py` のみ。
- STEP 2 未着手・ACK 待ちで停止。
