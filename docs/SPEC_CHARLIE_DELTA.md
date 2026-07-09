# PKB 完全設計仕様書 — Target Charlie / Target Delta
# (次期実装者への引継ぎ仕様。発行: 2026-07-07 / 起草: Target Bravo 完遂セッション)
# Rev.2 (2026-07-07): Target Delta-LINE (対人プロトコル・テレメトリ) を §3.9 として統合。
#   不変条件 I-14〜I-16、罠 T-11〜T-13、Architect's Note 9〜11 を追加。
# Rev.3 (2026-07-07): Charlie C1 (LSM化) + Delta-LINE DL1/DL2 (テレメトリ + 一人称×二人称
#   の衝突 + Bounty) 完遂。詳細は AI_SKILLS.md §10/§11 (as-built)。本書 §3.9.1 に
#   実装時の訂正コメントあり (extract_conversation_sessions は流用不可だった)。
# Rev.4 (2026-07-07): D3 (Puppeteer + Narrative Compiler) 完遂。詳細は AI_SKILLS.md
#   §11.4/§11.5 (as-built)。Narrative Compiler は HumanSourceCode/HistoricalNode
#   (D1/D2 未着手) の代わりに deep_profile.gap_analysis.gaps のみを素材にする
#   スコープ限定版 — Target Charlie / Delta (DL1/DL2/D3) はこれで完遂、
#   残るは Delta の D1/D2 サブトラック (5軸・PROBE・HistoricalNode) のみ。
# Rev.5 (2026-07-09): D1/D2 着工前 Phase 0 (SPEC照合)。下記「実装状態の宣言」の
#   陳腐化 (Charlie/Delta-LINE/D3 を「未実装」と誤記) を現況へ更新。§3 冒頭に
#   D1 の5軸データソース監査 (どの軸が既存算出の包み込みで済み、どの軸が新規
#   上流計算を要するか) を追記。設計本文 (§3.0-3.8) は不変 — 照合と現況注記のみ。
# Rev.6 (2026-07-09): D1 詳細設計を §3.2.1 として統合 (§3.2 骨格疑似コードの実装
#   確定版)。5軸データフロー・共通 Axis コントラクト・新規2軸 (locus_of_control
#   の ATTRIBUTION_LEXICON+否定ガード / unlearning_rate の突合窓 W_MAX+逆数式)
#   の決定論アルゴリズムを確定。憲法ガード test_source_code.py の RED 項目に直結。

> **読者への前提命令**: 本書を読む前に `docs/AI_SKILLS.md` を全文読め (第0原則)。
> 本書と AI_SKILLS.md が矛盾した場合、**不変条件については AI_SKILLS.md が正**、
> 未実装機能の設計については本書が正。
>
> **実装状態の宣言 (再実装禁止リスト・Rev.5 2026-07-09 更新)**:
> - Target Alpha (KVプレフィックス・ピニング) — **実装済み** (`core/kv_cache.py`, AI_SKILLS §8)
> - Target Bravo Step 1/2 (mmapゼロコピーIPC + 本体配線) — **実装済み** (commit `ffb1fbd`, AI_SKILLS §9)
> - Target Charlie (LSM化 + llama-server資産活用) — **実装済み** (`core/lsm_index.py`, AI_SKILLS §10)
> - Target Delta-LINE (対人プロトコル・テレメトリ DL1/DL2) — **実装済み** (`core/line_telemetry.py`, AI_SKILLS §11)
> - Target Delta D3 (PUPPETEER + NARRATIVE COMPILER) — **実装済み** (`core/question_bank.py` / `core/narrative_compiler.py`, AI_SKILLS §11.4/§11.5)
> - Target Delta **D1/D2** (5軸 HumanSourceCode / PROBE ファネル / HistoricalNode) — **本書 §3.1-3.8 が設計。未実装** (次の着工対象)
> - Target Echo (oracle / tensor / digital_twin = E4 profile/oracle API) — **実装済み** (`core/oracle.py` 他, SPEC_ECHO_GENESIS)
> - Foxtrot (UI Rev.11 まで) — **実装済み** (SPEC_FOXTROT_UI, F6 PROBE タブのみ D2 待ちで未着手)
>
> §1 は as-built リファレンスである。§1 のコードを書き直すな。§2, §3 を実装するとき
> §1 のインターフェースを「呼ぶ」ことだけが許される。

---

# §1【Target Bravo Step 2】as-built リファレンス (実装済み・変更禁止)

## 1.1 システムシーケンス (検索ホットパス)

```
Python (ConsultationEngine)                      C++ (search_engine --daemon)
──────────────────────────────                   ─────────────────────────────
search_index(bin, meta, qvec, k)
  ├─ _get_search_daemon()
  │    └─ 初回のみ: scratch 生成 + spawn ───────▶ scratch を mmap (writable)
  │       ◀──────────────────────────────────── {"event":"ready","max_k":64,...}
  ├─ scratch へ mmap 書き:
  │    query[384] ← qvec, top_k ← k, seq ← n     (seq は必ず最後に書く)
  ├─ stdio 1行: {"cmd":"search","seq":n,
  │              "index":"<bin絶対パス>"} ──────▶ scratch.seq == n を照合 (seqlock)
  │                                              ├─ bin 未マップなら mmap (以後キャッシュ)
  │                                              ├─ NEON/OpenMP Top-K
  │                                              └─ results/result_count を scratch へ
  │  ◀─────────────────────────── {"ok":true,"seq":n,"count":m,"elapsed_us":…}
  ├─ scratch から results 読み (chunk_id >= 0 のみ採用)
  └─ metadata.json と突合 → list[dict]
```

**stdio の 1 行往復がメモリバリアを兼ねる。ロックは存在しないし、追加してはならない**
(SearchDaemonClient 内部の threading.Lock は「クライアント側の直列化」であり別物)。

## 1.2 フォールバック連鎖の状態機械

```
        起動成功                search で SearchDaemonError
 [未起動] ────▶ [daemon稼働] ──────────────────────▶ [failed(恒久)]
    │ start()失敗 (exe不在等)                              │
    └────────────────────────────────────────────────────┘
 failed 状態: 以後このプロセスでは daemon を respawn しない (spawn storm 防止)
 例外: shutdown() は failed を立てない (正常終了後の再利用は再起動可 — 意図的非対称)

 検索経路: daemon → (hits空 or daemon不可) → 1-shot exe → (hits空 or exe不在) → NumPy
 NumPy 分岐 = exe 不在環境の生命線。三段のどれも削除禁止。
```

## 1.3 インデックス再構築シーケンス (Windows errno 22 の決定論的回避)

```
sync_diary_index(force) / sync_knowledge_index(force)
  ├─ 鮮度判定 (mtime) → 再構築不要なら return False
  ├─ _release_index_mapping(bin)                    ★再構築の直前・必須
  │    ├─ daemon 無し → no-op
  │    ├─ daemon.remap(bin) 送信 ────▶ C++ が該当 bin を即時アンマップ
  │    └─ remap 失敗 → _drop_search_daemon()  (プロセス死でアンマップを保証)
  ├─ pipeline.build_index(...)  ← ここで bin を "wb" 上書き。マップ残存なら
  │                               Windows で PermissionError だった (解消済み)
  └─ 再構築後の再マップ操作は不要 — 次の search が遅延リマップする
```

## 1.4 公開インターフェース (シグネチャ凍結)

```python
# core/search_daemon.py
class SearchDaemonError(RuntimeError): ...
class SearchDaemonClient:
    def __init__(self, exe: Path|None=None, scratch_path: Path|None=None, spawn=None)
    def start(self) -> None                    # 失敗は SearchDaemonError
    def search(self, bin_path, qvec, top_k) -> list[tuple[int, float]]
    def remap(self, bin_path) -> None          # 即時アンマップ + 遅延リマップ予約
    def ping(self) -> bool
    def close(self) -> None                    # shutdown→Terminate→Kill + scratch unlink
    max_k: int                                 # ready 行の max_k (≤64)

# core/consultation_engine.py (ConsultationEngine)
def _get_search_daemon(self) -> SearchDaemonClient | None
def _drop_search_daemon(self) -> None
def _release_index_mapping(self, bin_path: Path) -> None
def search_index(self, bin_path, meta_path, qvec, top_k=3) -> list[dict]
```

```
共有 scratch (magic "PKBSCR01", 2072 bytes, little-endian, 全フィールド自然整列):
  off 0   char[8]  magic          off 20  u32      result_count (C++が書く)
  off 8   u64      seq            off 24  f32[384] query
  off 16  u32      top_k          off 1560 (i32,f32)[64] results (未使用=-1)
対応物は search_engine.cpp::ScratchBuffer と search_daemon.py の struct 定義の 2 つだけ。
変更は両方同時 + magic バージョン更新。
```

ゴミファイル排除は三重: (1) close() の unlink (atexit 登録)、(2) C++ の stdin EOF
自己終了、(3) 起動時の `_ce_shared_scratch_*.bin` sweep (生存プロセスの scratch は
OS がオープン中のため Windows では削除に失敗し自然にスキップ)。

---

# §2【Target Charlie】LSM 化と llama-server 遊休資産の活用

## 2.1 PKBVEC01 の LSM 化 — 「pkbseg.v1」設計

### 2.1.0 設計の核心 (Architect's Override — §4.1 参照)

ベースライン仕様は「PKBVEC01 の magic 更新」を示唆していたが、**却下する**。
セグメントファイルの中身は既存 PKBVEC01 のまま 1 バイトも変えない。バージョニングは
**マニフェスト (segments.json) 側**で行う。理由:
1. C++ のブロックレイアウト・検証・ホットループが完全無変更で流用できる
   (`validate_index` はセグメント単体に対してそのまま通る)。
2. pipeline.py の write_binary も無変更 (出力先パスが変わるだけ)。
3. 「1 バイト単位同期」義務のある境界面を増やさない。

もう 1 つの核心: **コンパクションは再埋め込みしない**。ベクトルは既に f32 で
セグメント内に存在する。コンパクション = 生存レーンのバイトコピー + マニフェスト
更新であり、O(バイト数)・埋め込みモデル不要・完全決定論である。

### 2.1.1 ファイル構成

```
data/processed/
  segments.json                 ← マニフェスト (唯一の真実)
  vectors.seg-000001.bin        ← PKBVEC01 形式そのまま (不変・追記されない)
  vectors.seg-000002.bin
  ...
  metadata.json                 ← 従来通り chunk_id → 本文 (全セグメント共通の台帳)
  vectors.bin                   ← 【互換】非LSMパス用に残す (§2.1.7)
```

### 2.1.2 マニフェストスキーマ

```json
{
  "format": "pkbseg.v1",
  "embedder_id": "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2#384",
  "next_chunk_id": 3181,
  "segments": [
    {"file": "vectors.seg-000001.bin", "live": 194, "dead": 14}
  ],
  "days": {
    "2026-07-02": {
      "chunk_id": 3141,
      "segment": "vectors.seg-000007.bin",
      "tomb_offset": 123456,
      "content_hash": "blake2b:9f2c..."
    }
  }
}
```

- `content_hash` = BLAKE2b(その日の diary_text + line全文 + calendar_events の
  正規化JSON + transactions の正規化JSON + consultations の正規化JSON)。
  **その日を構成する全ソースを含めること。** 1 ソースでも漏らすと、そのソースだけの
  編集が再埋め込みをすり抜ける (サイレント陳腐化 — 罠 T-3)。正規化 =
  `json.dumps(obj, ensure_ascii=False, sort_keys=True)`。
- `tomb_offset` = そのチャンクの `chunk_ids[lane]` (int32) のセグメントファイル内
  バイトオフセット。**追記時に計算して記録する** (墓標打ちを O(1) の 4 バイト
  in-place 書きにするため)。
  計算式: `32 + block_index*6160 + 384*4*4 + lane*4`。

### 2.1.3 差分同期フロー (O(全履歴) → O(変更日数))

```python
def sync_diary_index_lsm(self, force=False) -> bool:
    manifest = load_manifest()                    # 無ければ空マニフェスト
    if manifest["embedder_id"] != current_embedder_id():
        return self._full_rebuild_lsm()           # 埋め込み空間が変わったら全再構築 (不変条件 I-6)
    daily = load_daily_contexts()                 # 既存関数。日付キー
    changed = []
    for day in daily:
        h = content_hash(day)
        rec = manifest["days"].get(day["date"])
        if rec is None or rec["content_hash"] != h:
            changed.append((day, h))
    deleted = set(manifest["days"]) - {d["date"] for d in daily}
    if not changed and not deleted and not force:
        return False

    # 1. 新セグメント書き出し (embedder は変更分にのみ触れる)
    if changed:
        vecs = self.embedder.encode([render(day) for day, _ in changed])  # ★唯一の重い処理
        seg_path, entries = write_segment(vecs, changed, manifest["next_chunk_id"])
        append_metadata_entries(entries)          # metadata.json へ追記
    # 2. マニフェスト更新 (アトミック: .tmp へ書いて os.replace)
    #    days の付け替え / next_chunk_id / segments 追加を 1 回の replace で確定
    atomic_replace_manifest(manifest)
    # 3. 旧版チャンクへ墓標打ち (in-place 4バイト書き: int32 → -1)
    for date in [d for d, _ in changed if d["date"] in old_days] + list(deleted):
        tombstone(old_segment_path, old_tomb_offset)
        bump_dead_count(...)
    # 4. ★墓標を打ったセグメント全てに daemon.remap() を送る (罠 T-1)
    for seg in touched_segments:
        self._release_index_mapping(seg)
    # 5. コンパクション判定 (§2.1.5)
    maybe_compact(manifest)
    return True
```

**順序が生命線である**: 新セグメント確定 (1,2) → 墓標 (3) → remap (4)。
クラッシュ耐性: (2) と (3) の間で死ぬと同一日付が 2 チャンク生存する。これは
**検索側の日付デデュープ (§2.1.4) で吸収**する — マニフェストを 2 回書いて
「トランザクション」を偽装する複雑化はするな。

### 2.1.4 検索: マルチセグメント Top-K

**マイルストーン C1 (C++ 無変更)**: Python 側でセグメントごとに
`daemon.search(seg, qvec, k)` を呼び、結果を Python でマージする。
1 セグメントあたりの IPC は実測 ~10µs なので 8 セグメントでも 1ms 未満。
1-shot / NumPy フォールバックも「セグメントごとに実行してマージ」で同型に組める。

**マイルストーン C2 (最適化・C1 の計測後にのみ着手)**: 制御プレーンに
`{"cmd":"search","seq":n,"segments":[p1,p2,...]}` を追加し、C++ 内で既存
`TopK::merge` を流用してセグメント横断マージする。**C1 の実測レイテンシが
1ms を超えない限り C2 を実装するな** (計測なしの最適化は禁止)。

検索後の共通後処理 (どの経路でも):
```python
# 日付デデュープ: 同一日付の重複ヒットは score 最大の 1 件のみ残す
# (クラッシュ窓 §2.1.3 と、墓標書き遅延の両方をここで吸収する)
best = {}
for h in hits:
    d = h["date"]
    if d not in best or h["score"] > best[d]["score"]:
        best[d] = h
```

### 2.1.5 コンパクション

発火条件 (OR): `len(segments) > 8` / いずれかのセグメントで `dead / (live+dead) > 0.5`。

```
compact(manifest):
  1. 対象セグメント群の生存レーンを列挙 (manifest["days"] が真実。ファイル側の
     chunk_id は参照しない — 墓標書きが遅延していても manifest が正)
  2. 新セグメントへ f32[384] とチャンクIDを 4-lane AoSoA で再パック (バイトコピーのみ。
     numpy 可 — これは pipeline 側の処理であり stdlib 縛りの core 決定論ロジックではない)
  3. manifest 更新 (days の segment/tomb_offset 付け替え) → os.replace
  4. 旧セグメントに daemon.remap() → その後 unlink
     (Windows: remap 前に unlink すると PermissionError — 順序厳守)
  5. metadata.json から死んだ chunk_id のエントリを削除 (台帳の肥大化防止)
```

### 2.1.6 墓標 (Tombstone) の意味論

`chunk_id = -1` は既存 C++ ホットループの `if (id >= 0)` で **追加分岐ゼロ**で
スキップされる。パディング規約の墓標転用はこの 1 点のために予約されていた。
**-1 以外の負値に意味を与えるな** (将来の拡張余地として温存)。

### 2.1.7 互換性とフォールバック

- `segments.json` が存在しない環境では従来の単一 `vectors.bin` パスで動作を継続する
  (`sync_diary_index` 冒頭で分岐)。移行は初回 LSM 同期時に「既存 vectors.bin を
  seg-000001 として採用しマニフェストを合成」— 再埋め込みなしで移行完了。
- knowledge index は当面 LSM 化しない (ファイル数が少なく更新頻度が低い)。
  日記側で実証してから同型展開する。

## 2.2 llama-server 遊休資産の活用

### 2.2.1 (a) 埋め込みの GGUF 化 — torch 依存の排除

目的: PyInstaller sidecar から sentence-transformers/torch (数百MB + 起動ペナルティ)
を追放し、既にバンドル済みの llama-server に埋め込みも担わせる。

```
構成:
  config/model_params.json に role "embed" を追加:
    {"preferred": ["multilingual-e5-small*.gguf"], "port_env": "PKB_EMBED_PORT",
     "server_args": ["--embeddings"], "dim": 384}
  ※ multilingual-e5-small = 384 次元 (PKBVEC01 の dim と一致。これが選定理由。
     bge-m3 等の 1024 次元モデルは PKBVEC01 を壊すため選定禁止)

  core/embed_client.py (新規):
    class GgufEmbedder:
        name: str                        # embedder_id に使う
        def encode(self, texts: list[str], **_) -> np.ndarray   # (N, 384)
    - POST http://127.0.0.1:{port}/v1/embeddings (OAI互換) を第一候補、
      404 なら旧 /embedding へフォールバック (server_supports_slot_save と同じ
      「プローブして恒久記憶」パターンを踏襲)
    - e5 系は "query: " / "passage: " プレフィックスが必須。クエリ埋め込みと
      文書埋め込みでプレフィックスを分ける。この知識は GgufEmbedder の内部に
      閉じ込め、呼び出し側に漏らすな (罠 T-4)
    - 返却ベクトルは必ず自前で l2_normalize (サーバー側の正規化有無に依存しない)

  フォールバック連鎖 (build_embedder を三段化 — Bravo と同じ思想):
    GgufEmbedder (embed用llama-server) → SentenceTransformer → HashedNgramEmbedder
```

**絶対条件**: embed 用サーバーは consult 用サーバーと**別プロセス・別ポート**。
同一プロセスに同居させると、埋め込みリクエストがスロットの KV 状態と干渉し、
Target Alpha のプレフィックス・ピニングを静かに壊す (§8 の「静かに死ぬ系」)。
ライフサイクルは LlamaServerBackend と同じ Terminate→Kill + atexit を流用。

### 2.2.2 埋め込み空間の同一性 (最重要不変条件 I-6)

**embedder を切り替えた瞬間、既存の全ベクトルはゴミになる。** 空間の異なる
ベクトル同士の内積は「エラーの出ない乱数」であり、検索品質がサイレントに死ぬ。
- `embedder_id` (モデル名 + 次元) をマニフェストに永続化。
- 現在の embedder と不一致なら**全再構築を強制** (LSM の差分最適化を無効化してよい
  唯一のケース)。
- クエリ埋め込みと文書埋め込みは常に同一 embedder。片方だけ GGUF 化して
  「動いているように見える」状態が最悪 (罠 T-5)。

### 2.2.3 (b) KV プレフィックス・ピニング — 実装済み。触るな

Target Alpha として `core/kv_cache.py` に実装済み (AI_SKILLS §8)。本書での追加は
1 点のみ: **embed 用サーバーには slot save/restore を絶対に適用しない**
(`--slot-save-path` は consult サーバー専用。embed サーバーの起動引数に混ぜるな)。

---

# §3【Target Delta】次世代自己分析エンジン — PROBE / PUPPETEER / NARRATIVE COMPILER

## 3.0-Rec Phase 0 照合ノート (Rev.5 2026-07-09 — D1 着工前の現況固め)

D1/D2 着工前に §3.1 の 5軸 (HumanSourceCode) を現行コードへ突合した実測結果。
D1 は「純粋に既存算出を包む」のではなく、【軸により実装コストが大きく異なる】。
D1 詳細設計はこの監査を前提にスコープを引く:

- **即包み込み可 (既存算出が score/confidence 付きで存在)**:
  - `interpersonal` 3軸 (friction_response / latency_asymmetry / protocol_plasticity)
    — `line_telemetry.py` (DL2) が既に軸形で算出済み。D1 は EvidenceRef 化して
    HumanSourceCode.interpersonal へ載せるだけ。
- **材料あり (下地関数は在るが 5軸形への合成は新規)**:
  - `decision_threshold` — `gap_analysis.analyze_procrastination()` の declarations /
    宣言→実行遅延 + 相談回数から合成。
  - `reward_bias` — `gap_analysis.hyperbolic_discount()` + finance の短期/長期支出比。
  - `friction_energy_ledger` — `gap_analysis.analyze_life_balance()` (stabilizer) の
    対人摩擦後の生産性シグナル増減を軸化。
- **【新規上流計算が必要 — D1 の隠れ工数・要注意】**:
  - `locus_of_control` — 日記の帰属語彙 (自責/他責/運) 頻度比。**現状この語彙分類器は
    未実装** (profiler は value_hierarchy 止まり)。D1 で決定論的な帰属語彙カウンタを
    新設する必要がある。
  - `unlearning_rate` — 矛盾提示後の行動語彙変化週数の逆数。**追跡計算は未実装**。
    consultation_log / gap 矛盾イベントと行動語彙の時系列突合を新設する必要がある。

依存充足 (D1/D2 が「呼ぶ」下部): `gap_analysis` / `line_telemetry` (Bounty/DyadStats) /
`narrative_compiler` / `oracle`(E4) はすべて実装済み。テストは Rev.11 P2 で確立した
conftest Sandbox 上で走る (§3 の旧テストノートはこれに従って更新すること)。

## 3.0 統治原則 (gap_analysis から継承する憲法)

1. **発見は決定論、言語化のみ LLM。** 5 軸スコア・矛盾検出・質問選択・MBTI 投影は
   すべて stdlib 決定論アルゴリズム。LLM に「人間の分析」を任せた瞬間、実行ごとに
   人格が変わる占い機に退化する。
2. **全ての主張に定量証拠。** 証拠 (金額・件数・引用・日付) を添付できない指摘は
   出力する資格がない。
3. **UI はブラックボックス、ディスクはホワイトボックス。** ゲーミフィケーションの
   演出として UI は理由を隠すが、`deep_profile.json` には全計算根拠を記録する。
   ユーザーが JSON を開けば全てが監査できる — knowledge_fetcher の「全クエリ監査
   可能」と同じ倫理線。**ディスク側まで隠蔽する変更は原則違反。**
4. **建前人格の隔離を継承。** PROBE の回答は本人の私的内省 (主観軸・重み 1.0)。
   面接シミュレーター内の発話は従来通り simulated=True (重み 0.1)。
5. **第三者は被写体ではなく刺激。** 本システムの評価関数は常に「本人」である。
   LINE 全ログ解析 (§3.9) において、他者の発話は本人の応答行動を測るための刺激
   (stimulus) としてのみ使い、**第三者の人格プロファイルを構築・永続化しない**。
   派生ストア・LLM プロンプトに第三者の実名を出さない (不変条件 I-15)。

## 3.1 データモデル (dataclass 定義 — 永続化は JSON、`to_dict/from_dict` を実装)

```python
# core/source_code.py (新規) — 「人間のソースコード」5軸
from dataclasses import dataclass, field

@dataclass
class EvidenceRef:
    kind: str          # "diary" | "consult" | "finance" | "calendar" | "line" | "probe"
    date: str          # ISO (ローカル日付。todayIso 同等の JST 安全関数を使う)
    quote: str         # 原文引用 (最大 120 字。ログに全文を垂れ流すな — 個人データ規律)
    value: float | None = None   # 金額・遅延日数などの定量値

@dataclass
class Axis:
    score: float       # 0.0-1.0 (軸ごとに意味を定義。下記)
    confidence: float  # 0.0-1.0 = min(1.0, 観測イベント数 / 飽和値)
    evidence: list[EvidenceRef] = field(default_factory=list)   # 上位5件のみ保持
    updated: str = ""  # ISO date

@dataclass
class InterpersonalProtocol:      # §3.9 Delta-LINE が算出する対人プロトコル 3 軸
    # F. 摩擦応答様式: 摩擦イベントへの応答分布から。0=修復先行 1=回避先行
    friction_response: Axis
    # L. レイテンシ非対称: 自分の返信中央値 vs 相手の返信中央値の対数比の dyad 中央値。
    #    0.5=対称。<0.5=自分が速い(投資超過) >0.5=相手が速い(被投資)
    latency_asymmetry: Axis
    # P. プロトコル可塑性: dyad 間の文体変動 (敬語率・文長・スタンプ率) の正規化分散。
    #    0=単一プロトコル 1=相手ごとに人格を切替
    protocol_plasticity: Axis

@dataclass
class HumanSourceCode:
    # 1. 意思決定閾値: 宣言→実行の遅延分布 + 実行前相談回数から。0=即断 1=証拠を無限に要求
    decision_threshold: Axis
    # 2. 報酬系の偏り: 双曲割引 V の分布 + 支出の短期快楽/長期投資比。0=遅延報酬型 1=即時報酬型
    reward_bias: Axis
    # 3. 統制の所在: 日記の帰属語彙 (自責/他責/運) の頻度比。0=外的 1=内的
    locus_of_control: Axis
    # 4. アンラーニング速度: 講評/consultで矛盾を突きつけられた後、行動語彙が変化するまでの週数の逆数
    unlearning_rate: Axis
    # 5. 摩擦収支: 対人摩擦イベント後の生産性シグナル増減 (stabilizer機構の対人版)
    friction_energy_ledger: Axis
    # 6群. 対人プロトコル (Delta-LINE §3.9)。1〜5 が「本人単独」の記述、6群が
    #      「対人系での実測挙動」の記述。データソースが異なるため Axis 群として分離
    interpersonal: InterpersonalProtocol | None = None

# core/line_telemetry.py (新規) — §3.9 の派生ストア単位
@dataclass
class DyadStats:                   # 1:1 トークルームごとの決定論的集計。実名は保存しない
    contact_alias: str             # "C-3f9a" = blake2b(実名, key=ローカルsalt) 先頭8桁
    exchanges: int                 # 応答ペア数。min-sample ゲート (I-15/T-11) の母数
    user_reply_median_min: float   # 深夜窓 (23:00-08:00) 到着分と >48h は除外 (T-12)
    peer_reply_median_min: float
    initiation_ratio: float        # 会話 (バースト) の開始者が本人である割合
    user_msg_share: float          # 本人の発話量シェア (文字数ベース)
    formality_index: float         # 敬語マーカー率 (protocol_plasticity の素材)
    friction_events: int           # 多重シグナル判定 (§3.9.2) を通過した件数のみ
    friction_responses: dict = field(default_factory=dict)
                                   # {"avoid": n, "appease": n, "escalate": n, "repair": n}

# core/probe_engine.py (新規)
@dataclass
class HistoricalNode:
    id: str                    # "hn-<blake2b短縮>"
    date_range: str            # "2023-04..2024-03" 等
    fact_text: str             # 事実の記述
    source: str                # "probe" | "diary" | "import"
    is_trusted: bool = True
    superseded_by: str | None = None   # 訂正は新ノード作成 + このリンク。編集・削除は禁止
    weight: float = 1.0        # 共倒れ減衰の対象 (0.05 まで落ち得る。0 にはしない)
    disputed_with: str | None = None   # 減衰ペアの相互リンク (§3.3)

@dataclass
class ProbeSession:
    date: str
    stage: str                 # "FACT" → "CONTEXT" → "EMOTION" → "MEANING" の単方向ファネル
    target_axis: str           # 5軸のうち confidence 最低の軸 (質問選択の決定論的根拠)
    questions_asked: list[str]
    nodes_created: list[str]   # HistoricalNode.id

@dataclass
class Bounty:                  # 「矛盾」= 突くべき賞金首
    id: str
    axis: str                  # 関連する5軸
    theme: str                 # THEME_TAXONOMY のテーマ名
    subjective_claim: EvidenceRef
    objective_counter: EvidenceRef
    tension: float             # ギャップスコア差分 (gap_analysis の gap 値を流用)
    status: str                # "open" | "queued" | "probed" | "confirmed" | "resolved"
    bank_question_id: str | None = None   # ★無菌化は「選択」であり「生成」ではない (§3.4)

# core/narrative_compiler.py (新規)
@dataclass
class NarrativeClaim:
    text: str
    node_refs: list[str]       # ≥1 の HistoricalNode.id。空 = コンパイルエラー (幻覚防止)
@dataclass
class NarrativeDraft:
    es_text: str
    claims: list[NarrativeClaim]
    recruiters_eye: str        # 戦略のメタ解説 (どの評価関数を最大化したか)
    compiled_from: str         # source_code + gap_analysis のハッシュ (再現性の刻印)
```

永続化先: `data/processed/probe_store.json` (HistoricalNode / ProbeSession / Bounty)、
`data/processed/line_telemetry.json` (DyadStats + salt。実名→alias の対応表は**保存しない** —
salt さえあれば実名から alias を再導出できるが、alias から実名は戻せない一方向性)。
`deep_profile.v7` に `source_code` キーを追加 (5軸 + interpersonal 3軸 + 計算根拠)。
スキーマを変えたら `_gap_section()` と `update_user_profile()` の追従を確認
(AI_SKILLS §6.5 と同じ規律)。

## 3.2 5軸の決定論的計算 (疑似コード)

```python
def compute_source_code(daily, probe_store, prev: HumanSourceCode) -> HumanSourceCode:
    # 全軸共通: 観測イベント < 5 なら score を更新せず confidence のみ記録
    # (3日分のデータで人格を断定するのは分析ではなく偏見 — gap_analysis §6.2-4 と同じ)

    # 1. decision_threshold
    delays = [d for d in declaration_to_execution_delays(daily)]   # gap_analysis の宣言検出を流用
    consults_before_act = count_consult_then_act_pairs(daily)
    score = sigmoid(median(delays) / 7.0) * 0.7 + min(consults_before_act / 5, 1.0) * 0.3

    # 2. reward_bias — 双曲割引 V=1/(1+0.3D) の平均 (低い=先延ばし=即時報酬優位) と
    #    支出比 (娯楽・消費テーマ支出 / 学習・自己投資テーマ支出) の合成
    # 3. locus_of_control — ATTRIBUTION_LEXICON (自責: 「自分の準備不足」「私が」 /
    #    他責: 「〜のせい」「運が悪」「環境が」) の頻度比。probe の EMOTION 段回答も母集団に含む
    # 4. unlearning_rate — 講評/consultの指摘テーマについて、指摘日以降 k 週以内に
    #    行動ログ (カレンダー/LINE) の関連アクションが初出するかを追跡。1/k の平均
    # 5. friction_energy_ledger — FRICTION_MARKERS (衝突・言い争い・気まずい) を含む日の
    #    事後2日間の生産性シグナル差分 (stabilizer の対人版。符号がそのまま収支)
```

語彙表 (`ATTRIBUTION_LEXICON`, `FRICTION_MARKERS`) は gap_analysis の
`GUILT_MARKERS`/`PRODUCTIVITY_MARKERS` と同じ形式・同じファイル配置規約で定義する。

> **実装確定版は §3.2.1。** 上記 §3.2 の疑似コードは骨格 (illustrative)。
> 実装は下記 §3.2.1 (実データの `analyze_procrastination`/`line_telemetry`
> 出力へ結線した確定式) に従う。式が食い違う箇所 (例: decision_threshold) は
> §3.2.1 を正とする。

## 3.2.1 D1 実装詳細設計 (Rev.6 — §3.2 骨格の実装確定版)

設計典拠: §3.1 データモデル / 憲法 §3.0 (決定論・証拠必須・感情推定排除・建前隔離)。
Phase 0 照合 (§3.0-Rec) の実測に基づき、各軸を実在の上流関数へ結線する。

### A. 全軸共通コントラクト (`Axis`)
- `Axis = {score: float|None, confidence: 0.0-1.0, evidence: [EvidenceRef]≤5, updated: ISO}`。
- **score ∈ [0,1]**、意味は §3.1 定義。**データ不足なら score=None** (line_telemetry/gap
  の「断定を避ける」規約。捏造の 0 を出さない)。
- **confidence = min(1.0, n_obs / SAT)** (観測イベント数の飽和。SAT は軸別)。
- **evidence 必須**: score≠None の軸は EvidenceRef を最低1件。quote は **120字上限** (§3.1)。
- **建前隔離**: 主観テキスト源は `build_subjective_corpus` の
  **weight ≥ GENUINE_DOC_MIN_WEIGHT(0.5)** のみ (面接 simulated の重み0.1 は除外)。
- **決定論**: 全軸 stdlib のパターンマッチ・算術のみ。**LLM 呼び出しゼロ** (感情推定排除の
  構造的担保 → guard `test_no_emotion_inference`)。`clamp01(x)=max(0,min(1,x))`。

### B. 5軸データフロー (軸1・2・5・interpersonal)

- **軸1 decision_threshold** (0=即断 / 1=証拠を無限要求):
  - 源: `analyze_procrastination()` の per-task `avoidance_index` (=1−平均双曲割引値) +
    `consultation_log` の実行前・同テーマ相談件数 `preconsult`。
  - score = `clamp01(0.6·mean(avoidance_index) + 0.4·min(1, mean_preconsult/5))`。
  - confidence = `min(1, declarations/8)`。evidence = 宣言 quote + 遅延 records。
- **軸2 reward_bias** (0=遅延報酬 / 1=即時報酬):
  - 源: finance 支出分類 + 実行双曲割引値中央値 `V̄`。
  - 支出分類 lexicon (新設・辞書式): `SPEND_IMMEDIATE`={外食,娯楽,課金,衝動,コンビニ…} /
    `SPEND_INVEST`={書籍,受講,資格,貯蓄,投資…} を finance カテゴリ名にマッチ。
    `imm = 短期支出/(短期+長期)`。
  - score = `clamp01(0.5·imm + 0.5·(1−V̄))`。confidence = `min(1, n_tx/20)`。
- **軸5 friction_energy_ledger** (0=摩擦で消耗 / 1=摩擦を糧に):
  - 源: `FRICTION_MARKERS`(衝突・言い争い・気まずい…) を含む日の事後±2日窓へ
    `analyze_life_balance` の `_productivity_hits` 差分 `Δp = after − before` を適用。
  - score = `clamp01(0.5 + 0.5·tanh(Δp/2))` (Δp>0 = 摩擦を糧に = 1寄り)。
  - confidence = `min(1, n_friction_events/5)`。
- **6群 interpersonal** (friction_response / latency_asymmetry / protocol_plasticity):
  - 源: `line_telemetry` DL2 が **既に score/confidence 付き Axis 形で算出済み**。
    D1 は **そのまま採用** (EvidenceRef 化 = contact_alias 経由のみ、実名なし I-15)。

### C. 【新規】軸3 locus_of_control — 帰属語彙 (0=外的 / 1=内的)

分類辞書 (`TASK_LEXICON` idiom を踏襲。`dict[str, list[str]]`・stdlib のみ):
```python
ATTRIBUTION_LEXICON = {
  "internal": ["自分のせい","自分が悪","努力不足","準備不足","甘かった","力不足",
               "反省","次はこうする","やるべきだった","改善する","詰めが甘"],
  "external": ["のせい","せいで","環境が","会社が","周りが","上司が","理不尽",
               "仕方ない","どうしようもない","巻き込まれ"],
  "chance":   ["運が悪","運次第","たまたま","偶然","ツイてな","巡り合わせ","不運"],
}
NEGATORS = ["ない","じゃない","ではない","わけがない","とは思わない"]  # 否定ガード
```
算出 (genuine-weight 日記/相談テキストのみ):
1. 各群 kw を `re.finditer` で走査。ヒットごと前後30字の snippet を取り、**snippet 内に
   NEGATOR が近接する打消しは除外** (「自分のせいじゃない」を internal に誤計上しない
   — procrastination の DONE_MARKER 近接判定と同 idiom)。
2. `n_int, n_ext, n_chance` を集計。`N = n_int + n_ext + n_chance`。
3. **score = n_int / N** (N>0)。外的+運は分母のみ (0側へ引く)、内的が 1側。`N==0 → None`。
4. confidence = `min(1, N/15)`。evidence = internal/external 各最頻 snippet を
   EvidenceRef(kind="diary", quote≤120字) で最大5件。
- 憲法適合: 純パターンマッチ = 言語的物理量のカウント。**感情の「推定」ではない**
  (本人が書いた帰属語の実測頻度)。LLM 不使用。simulated 除外で建前隔離。

### D. 【新規】軸4 unlearning_rate — 語彙変化速度 (矛盾提示後の変化の速さ / 高=速い)

矛盾提示イベント源 (決定論・日付付き):
- (a) `analyze_procrastination()` の **flagged task** (type=task_avoidance) の宣言日 `T`。
- (b) `line_telemetry` の **Bounty status ∈ {"probed","confirmed"}** の付与日。
  (初版は (a) のみで着工可。(b) は confidence 増強として後付け可 — 下記オープン項目2)

時系列突合ウィンドウ & 計算:
```
W_MAX = 8 週 (56日)                       # 追跡窓。超える変化は「学習し直さなかった」
event ごとに:
  first_change = T 以降 W_MAX 以内で、そのテーマの「行動語彙変化」が初出した日
    行動語彙変化 = DONE_MARKERS / calendar 実行の初出 (procrastination の executions を再利用)
  weeks = (first_change - T).days / 7      # 変化あり
  weeks = W_MAX (=8)                        # 窓内に変化なし (減衰の底)
  rate_event = 1 / (1 + weeks)              # 即週=1.0、8週=0.111、単調減少
axis_score = mean(rate_event)               # 全 event 平均
confidence = min(1, n_events / 5)
```
- evidence: EvidenceRef ペア (矛盾提示 quote → 変化(実行)証跡 quote) を最大5件。
- 憲法適合: 日付差と DONE_MARKER 初出 = 完全に決定論。LLM 不使用。「感じ方」でなく
  「行動語彙が変わるまでの週数」という物理量。

### E. `get_source_code()` 出力 (`profile.source_code` API / §3.8)
```
{ "schema":"human_source_code.v1", "updated": ISO,
  "axes": { decision_threshold, reward_bias, locus_of_control, unlearning_rate,
            friction_energy_ledger, friction_response, latency_asymmetry, protocol_plasticity },
  "mbti_projection": "EsTj",     # ※下記
  "progress": mean(confidence) 全軸 }
```
- 永続化: `deep_profile.json` の `"source_code"` セクション (ディスク=ホワイトボックス
  §3.0-3。全計算根拠 = evidence/生カウントを JSON に残す)。§3.1 の永続化規約に合流。
- **MBTI 投影**: 5軸→MBTI 4文字の決定論的符号写像。**confidence<0.5 の文字は小文字**で
  「未確定の揺らぎ」を表現 (§2.5 "EsTj")。正確な軸→文字マッピング表はガード RED 後の
  実装で確定 (本節は「決定論・小文字=低confidence」の規約のみ固定 — オープン項目1)。

### F. 憲法適合チェックリスト (→ `test_source_code.py` の RED 項目に1対1対応)
| 憲法 | 構造的担保 | ガードテスト |
|---|---|---|
| 決定論 | 全軸 stdlib・LLM 無 | `test_source_code_deterministic` / `test_no_emotion_inference` |
| 証拠必須 | score≠None ⇒ evidence≥1 | `test_every_axis_has_evidence` |
| 感情推定排除 | D1 はカウント/日付差のみ (EMOTION 段は D2) | `test_no_emotion_inference` (backend.generate 未呼) |
| 第三者秘匿 | interpersonal は contact_alias 経由のみ | `test_no_third_party_realname` |
| 個人データ規律 | quote 120字上限 | `test_quote_length_cap` |
| 建前隔離 | weight≥0.5 のみ採用 | (corpus weight フィルタ) |

### G. オープン項目 (ガード RED の妨げにはならない・実装時に確定)
1. 軸→MBTI 文字マッピング表 (本節は規約のみ固定)。
2. 軸4 の矛盾提示イベント源: 初版 (a) flagged task のみ / (b) Bounty は後付け増強。
3. 軸5 の `tanh(Δp/2)` 除数 (2) は暫定。実 `_productivity_hits` スケールで調整。

## 3.3 クロス・バリデーションと「共倒れ減衰」(疑似コード)

```python
def cross_validate(node: HistoricalNode, engine) -> None:
    qvec = engine.embed(node.fact_text)
    hits = engine.search_daily(qvec, top_k=5)          # ★Bravo デーモンが 109µs で返す。
    support = [h for h in hits if is_consistent(h, node)]       #  Delta の検索多用は
    contra  = [h for h in hits if is_contradictory(h, node)]    #  Bravo が前提インフラ
    if support:
        node.weight = min(1.0, node.weight + 0.1 * len(support))
    elif contra:
        # 共倒れ減衰: どちらが正しいか機械は裁定しない。両方の検索重みを 0.05 へ。
        # 削除は絶対にしない (人間の記憶の矛盾は、それ自体が最上級のプロファイル情報)
        node.weight = 0.05
        for h in contra: set_chunk_search_weight(h["id"], 0.05)
        node.disputed_with = contra[0]["id"]           # 相互リンク (復権の布石)
        register_bounty(node, contra[0])               # ★矛盾は Puppeteer の賞金首になる
    # 復権: 後日の第三証拠 (新しい日記/probe) が片方を支持したら、その側のみ weight を戻す。
    # 減衰は可逆、削除は不可逆 — だから削除を禁止する。
```

`is_consistent/is_contradictory` は決定論 (テーマ一致 + 否定語彙 + 数値矛盾)。
LLM に裁定させるな。

## 3.4 The Puppeteer — 無菌化注入 (最重要: 情報隔離との両立)

**設計上の危機**: interview_sim の議論フェーズへの gap 注入は `_assert_no_gap_leak`
が禁じる聖域である (AI_SKILLS §7.1)。Puppeteer は一見これに抵触する。

**解決 — 「生成ではなく選択」による構成的無菌化**:

```python
# core/question_bank.py (新規): 完全に一般的な面接質問の静的バンク。
# 個人情報を 1 文字も含まない。axis × theme でタグ付け。
QUESTION_BANK = {
  "qb-017": {"text": "計画通りに進まなかった経験と、その時どう軌道修正したかを教えてください",
             "tags": {"axis": "decision_threshold", "theme": "キャリア・仕事の将来"}},
  ...  # 5軸 × 主要テーマで最低 40 問。文言はどの候補者にも成立する普遍質問のみ
}

def puppeteer_select(bounties) -> list[str]:
    # open な Bounty を tension 降順に走査し、axis+theme が一致するバンク質問 ID を
    # 決定論的に選択 (INTERVIEW_CASE_BANK と同じ巡回規則。乱数禁止)
    # → _interview_state["priority_queue"] へ質問 ID を積む (メモリ内のみ・永続化禁止)
```

- 面接官プロンプトに渡るのは**バンクの質問テキストだけ**。なぜその質問が選ばれたかは
  面接官コンテキストに存在しない。本番の面接官が「たまたま痛いところを突く」確率を
  人為的に 1.0 へ吊り上げる装置であり、面接官が本人を知っている状況は作らない。
- **LLM による質問の書き換え (無菌化リライト) は禁止。** リライト LLM は gap の
  言い回しを漏らし得る。静的バンクからの選択は構成的に漏洩ゼロを保証する。
- `_assert_no_gap_leak` は削除せず**強化**する: 注入された質問が QUESTION_BANK に
  実在することを assert するホワイトリスト検査を追加 (回帰ガードの進化)。
- 講評フェーズ (gap 統合が合法な唯一の場) で初めて「あの質問は Bounty #x を突く
  ものだった。回答は矛盾を解消したか」を開示・評価する。隔離→統合の既存非対称
  構造にそのまま乗る。

## 3.5 NARRATIVE COMPILER

```
入力: HumanSourceCode + gap_analysis (true_gakuchika 含む) + trusted HistoricalNodes + ES ドメイン
処理:
  1. 決定論パス: 使用可能な素材の選定 — weight ≥ 0.5 の trusted ノードのみ。
     true_gakuchika が発掘した「真の熱量テーマ」を主軸に採用 (建前テーマは自動降格)
  2. LLM パス: 素材からの文章生成。プロンプトに「素材に無い事実の創作禁止。
     各段落末尾に [hn-xxxx] 参照タグを付けよ」を明記
  3. 決定論パス (検収): 生成文の各段落から参照タグを抽出し、NarrativeClaim へ変換。
     タグ欠落 or 存在しないノード ID → その段落を破棄して再生成 (最大2回、以後は欠落段落を
     除いた短縮版を採用)。幻覚はスタイル問題ではなくコンパイルエラーとして扱う
  4. Recruiter's Eye: 「なぜこの構成か」— どの軸の弱点を隠さず先回りしたか、どの
     定量証拠が評価関数のどの項 (再現性・主体性・規模) を撃つかのメタ解説を別枠生成
出力: NarrativeDraft → data/es/draft_*.md (es_review モードでそのまま添削可能な形式)
```

**es_review との関係**: 生成したドラフトを es_review に掛けるとき gap_insights を
注入しない既存規律は不変。COMPILER は gap を知って書き、REVIEWER は知らずに叩く。
この分業が「書類単体の強度」の検証可能性を守る。

## 3.6 ゲーミフィケーション

```python
# プロファイリング進行度 (0-100%)
raw = ( 0.5 * mean(axis.confidence for axis in five_axes)
      + 0.3 * resolved_bounties / max(total_bounties, 1)
      + 0.2 * probe_coverage )                    # 人生年表の probe 済み期間比
progress_display = max(previous_display, int(raw * 100))   # ★表示は単調非減少 (高水位標)
# 内部値 raw は下がり得る (confidence 低下)。下がった事実は deep_profile に記録するが
# UI には見せない。動機維持のための演出であり、データ改竄ではない (原則3)。

# 動的 MBTI 投影 — 疑似コード
def project_mbti(sc: HumanSourceCode, recent_variance: dict) -> str:
    letters = [
        "E" if sc.friction_energy_ledger.score > 0.5 else "I",   # 摩擦収支が正=対人で充電
        "S" if sc.decision_threshold.score > 0.5 else "N",       # 高閾値=具体的証拠を要求
        "T" if sc.locus_of_control.score > 0.5 else "F",         # 内的統制=論理帰属
        "J" if sc.reward_bias.score < 0.5 else "P",              # 遅延報酬型=計画的
    ]
    # 「揺らぎ」: 直近14日の行動ログ分散が閾値超の文字には小文字表示 (例: "EsTj")
    # → 確定していない感覚が回答意欲を駆動する
    return "".join(l.lower() if recent_variance[l] > VAR_THRESHOLD else l for l in letters)
```

MBTI 投影は**表示専用アーティファクト**である。deep_profile の分析入力に戻すな
(出力を入力に戻すと自己強化ループで軸スコアが発振する — 罠 T-8)。心理測定学的に
MBTI は妥当性が低いが、ここでの役割は測定ではなく「ラベルの揺らぎで内省を誘発する
UI 装置」であり、その限りで採用する。

## 3.7 PROBE のファネル状態機械と実行制御

```
FACT ──▶ CONTEXT ──▶ EMOTION ──▶ MEANING   (単方向。逆行禁止。スキップは MEANING のみ可)
 │「大学2年の春、何をしていましたか」        (事実 = 心理的コスト最小の入口)
 │        │「それは誰の発案でしたか」
 │        │        │「その時、正直どう感じましたか」
 │        │        │        │「いま振り返るとそれは何だったと思いますか」
 └─ 回答は即 HistoricalNode(is_trusted=True) 化。EMOTION/MEANING の回答のみ
    主観コーパス (重み1.0) へ合流。FACT/CONTEXT は事実台帳であり感情分析に使わない
```

- **セッション制御 (Architect's Override §4.6)**: 1 日 1 セッション上限・1 セッション
  最大 5 問。連投は回答品質を下げるだけでなく、疲弊した回答がノイズとして 5 軸を
  汚染する。さらに**発火タイミングを stabilizer_effect 検出直後の日に優先配置**する
  — 充電後の生産性上昇窓は自己開示の心理的帯域が最も広い。追い込むパラメータは
  頻度ではなくタイミングである。
- PROBE の質問選択も決定論: confidence 最低の軸 → その軸の未 probe 期間 → ファネル
  段階、の辞書式順序。

## 3.8 配線 (責務分離の遵守)

```
facade.py に追加する API (全て既存パターンに従う):
  probe_next_question() -> dict        # {question, stage, session_id, progress}
  probe_answer(session_id, text) -> dict
  get_source_code() -> dict            # UI 表示用 (5軸 + MBTI投影 + progress)
  compile_narrative(es_domain) -> dict
engine_stdio.py の dispatch に "probe.next" / "probe.answer" / "profile.source_code" /
"narrative.compile" を追加 → apps/desktop/src/lib/engine.ts にラッパー。
UI: INTERVIEW タブに Puppeteer は不可視 (priority_queue はバックエンド内部)。
    SETTINGS or 新 PROFILE タブに進行度・MBTI・5軸レーダーを表示。
重い処理 (compile_narrative) は必ず status イベントで進捗を流す (§3.4 UI 規律)。
```

## 3.9 Target Delta-LINE — 対人プロトコル・テレメトリ (人間関係の物理的衝突ログ)

### 3.9.1 データソースと「第3チャネル」宣言 (不変条件 I-16 の本体)

LINE 全ログ (`data/raw/line_history.txt`) を、本人の対人挙動を実測する
**テレメトリ**として解析する。

> **【実装時の訂正】** 本節は当初「パーサは新設しない — `data_merger.
> _session_from_buffer` が既にセッション単位で `is_self` 帰属・contact・
> latency・stimulus/response 分離を構造化しており、その出力を消費するだけ」
> としていたが、これは誤りだった。実装して初めて判明した事実:
> `extract_conversation_sessions()` は「相手が発言 → 本人が返信」の**一方向
> のみ**を対象にした状態機械であり (`if active is None: if not msg["is_self"]:
> ...`)、本人がバーストを開始した会話は構造的に捨てられる。これは consult
> 文脈で表示する「相手への応答速度」を測るための設計であり、Delta-LINE が
> 必要とする `initiation_ratio` (誰が会話を始めるか) の計測には使えない。
> `core/line_telemetry.py` は独自の対称なバースト抽出 (`_bursts_by_contact`)
> を実装している。日付/時刻パース関数 (`normalize_date`/`_parse_dt`) のみ
> `data_merger` から再利用し、セッション状態機械そのものは再利用しない。
> **教訓**: 「既存関数の名前と役割が近い」だけで再利用可能と判断するな。
> 対称性・方向性など、暗黙の設計前提まで確認してから流用を決めろ。

**「ホワイトリスト廃止」の正確な意味**: 従来 `line_self_text` (本人発話のみ) に
限定していたのは gap_analysis の主観/客観コーパスであり、**その定義は 1 文字も
変えない**。全ログ (他者発話を含む) を読むのは本書で新設する第 3 チャネル
「対人テレメトリ」だけである。3 チャネルの憲法:

```
チャネル1 主観コーパス  : 日記 + 相談 Query           (他者発話・本人LINE発話とも不出)
チャネル2 客観コーパス  : 家計簿 + 予定 + line_self_text (他者発話は不出)
チャネル3 対人テレメトリ : LINE 全ログ (本人+他者)      ← Delta-LINE。集計値のみが下流へ
```

チャネル 3 から下流 (5軸・gap・Bounty) へ流れてよいのは**決定論的集計値と
仮名化済み証拠引用のみ**。他者の生発話をチャネル 1/2 に混ぜる変更は
`test_subjective_objective_separation` の破壊であり、実装ではなく変更が間違っている。

再計算トリガは **`import.line` のみ** (profiler 自動実行の既存規約 I-3 に相乗り)。
RECORD 保存・consult でテレメトリを再計算するな。

### 3.9.2 対人プロトコル 3 軸の決定論的定義

会話の単位 = バースト (メッセージ間隔 ≤30 分の連続塊)。応答レイテンシ = 相手バースト
終端 → 本人の次バースト先頭の経過分。dyad = 1:1 トークルーム。**グループチャットは
v1 では全指標から除外する** (多者間の帰属曖昧性は T-11 の温床。個別に設計してから)。

```python
# F. friction_response — 摩擦イベントの検出は多重シグナル (2-of-3) 必須:
#   (a) FRICTION_MARKERS がバースト内のどちらかの発話に出現
#   (b) 本人の応答レイテンシが当該 dyad 中央値の 3 倍超
#   (c) スレッド死 (活動中 dyad で 72h 無通信) または直後バーストに謝罪マーカー
#   単一キーワードでの判定は禁止 (皮肉・冗談・引用の誤検出 — T-11)。
#   イベント成立時、本人の次アクションを分類:
#     avoid    = 応答なし/スレッド死         appease = 謝罪マーカー (非自責文脈)
#     escalate = 否定語彙の増量返信          repair  = 疑問形+関係修復語彙の返信
#   score = (avoid + 0.5*appease) / total   … 0=修復先行 1=回避先行
#   ※ friction_energy_ledger (第5軸) へのイベント供給源も本判定に一本化する。
#     日記マーカー単独より LINE 実測の方が摩擦イベントの検出精度が高い —
#     二重検出したら LINE 側を正とする (同一日の重複カウント禁止)

# L. latency_asymmetry
#   dyad ごとに log(peer_median / user_median) を取り、exchanges ≥ 20 の dyad のみで
#   中央値 → sigmoid で 0-1 へ。深夜窓 (23:00-08:00 到着) のサンプルと 48h 超は除外、
#   48h 超はレイテンシではなくスレッド死として F 軸へ回す (T-12)。

# P. protocol_plasticity
#   dyad ごとの formality_index (敬語マーカー率)・平均文長・スタンプ/絵文字率の
#   3 次元ベクトルを作り、dyad 間分散を正規化。exchanges ≥ 20 の dyad が 3 未満なら
#   confidence 不足として score 未更新 (Axis.confidence の既存機構に乗せる)。
```

全語彙表 (`FRICTION_MARKERS`, `APOLOGY_MARKERS`, `FORMALITY_MARKERS`) は
gap_analysis の語彙表と同一形式・同一配置。**感情推定に LLM を使うな** —
言語化フェーズ以外に LLM が入った瞬間、テレメトリは再現不能になる (統治原則 1)。

### 3.9.3 高次プロファイリング: 一人称 × 二人称の衝突 (`social_positioning_gap`)

自己認識 (日記・相談での自己役割宣言) と、他者との実測相互作用が示す立ち位置を
突合し、既存の二項対立 (intention_gap / blind_spot — AI_SKILLS §6.1) の対人版として
ギャップを検出する。

```python
def analyze_social_positioning(daily, dyads: list[DyadStats]) -> list[dict]:
    gaps = []
    # 主観側: SELF_ROLE_LEXICON (聞き役/調整役/ムードメーカー/慕われている/頼られる 等)
    #         の日記・相談内シェアを役割ごとに集計
    # 客観側: dyad 集計の中央値 (initiation_ratio, user_msg_share, latency_asymmetry)
    #
    # 例1 (intention_gap 型): 「聞き役」自認シェア ≥ 0.3 なのに user_msg_share > 0.6
    #   → 会話占有率が自己像と矛盾。証拠 = シェア数値 + 日記引用 + dyad 数
    # 例2 (blind_spot 型): 全 dyad の initiation_ratio 中央値 > 0.8 かつ
    #   関係維持コストへの言及が主観コーパスに 0 件
    #   → 関係維持の一方的負担に無自覚。証拠 = 比率 + 対象 dyad 数 (alias のみ)
    # 例3 (intention_gap 型): 「良好な関係」言及 dyad で latency_asymmetry > 0.75
    #   (自分だけが即応) → 投資の非対称に無自覚
    #
    # 各ギャップは type="social_positioning_gap" で gaps リストへ合流。
    # format_gap_table のラベル表に同名エントリを追加 (型名の片側変更禁止 — §6.4 規則)。
    # tension 上位は register_bounty() で Bounty 化 → Puppeteer の資源循環へ (§3.4)。
    return gaps
```

検出閾値・最小標本数はすべて定数として `line_telemetry.py` 冒頭に集約し、
テスト (`test_line_telemetry.py`) の対照群 (「主観と客観が一致している dyad は
フラグしない」) で退化を防ぐ — stabilizer/知性化テストと同じ対照群規律。

### 3.9.4 隔離とデータ最小化 (I-14 / I-15 の本体)

```
LINE 全ログ ──▶ [チャネル3: 決定論集計] ──▶ DyadStats (alias化済み)
                                             ├──▶ 5軸/interpersonal (数値のみ)
                                             ├──▶ social_positioning_gap (数値+仮名引用≤120字)
                                             └──▶ Bounty ──▶ Puppeteer
                                                              │ (選択のみ・生成禁止)
     面接官コンテキストに渡るもの ◀── QUESTION_BANK の実在質問テキストのみ ◀──┘
```

- **結論の遮断 (I-14)**: テレメトリ・interpersonal 軸・social_positioning_gap・
  Bounty の内容は、面接官/GD 議論フェーズ・es_review のコンテキストに**一切**出さない。
  `_assert_no_gap_leak` の検査対象キーに `source_code` / `interpersonal` /
  `line_telemetry` / `bounties` を追加する (回帰ガードの拡張。§3.4 のホワイトリスト
  検査と併せて二重化)。講評フェーズのみ統合可 — 既存の非対称構造に完全に乗る。
- **無菌化された反証 (I-11 の適用)**: LINE 由来の Bounty も QUESTION_BANK からの
  決定論的**選択**のみ。「LINEでこういう事実があったので〜」という質問文の生成は、
  リライト LLM を経由しても禁止 (§3.4 の構成的無菌化と同一原理)。バンクには
  axis タグ `friction_response` / `latency_asymmetry` / `protocol_plasticity` と
  テーマ「対人関係・チームワーク」の質問を追加すること (タグ体系と
  format_gap_table ラベルの同期は必須)。
- **第三者最小化 (I-15)**: 実名は line_telemetry.json に保存しない (salt 付き一方向
  alias)。証拠引用は 120 字上限・仮名化済み・本人発話を優先し、他者発話の引用は
  ギャップ成立に不可欠な場合のみ。第三者ごとの性格記述・評価を生成するプロンプト/
  出力パスを作らない。DailyContext 本文 (検索インデックス) には従来通りセッション
  全文が入る — 除外するのは**分析チャネルと派生ストア**だけ (「記録と分析の分離」
  §1 の適用例。記録としては本物、分析の被写体は本人のみ)。

---

# §4【Architect's Note】ベースライン仕様への上書きと、その論拠

1. **PKBVEC01 の magic を上げない (§2.1.0)。** ベースラインは「レイアウト変更 +
   magic 更新」を想定していたが、セグメント内部を PKBVEC01 のまま凍結しマニフェストで
   バージョニングする方が、C++/pipeline の「1 バイト同期」境界面を一切動かさずに
   LSM を得られる。動かさない境界面はバグを生まない。
2. **コンパクション = バイトコピー (§2.1.0)。** LSM の常識 (SSTable マージ) を
   そのまま持ち込むと「再埋め込み」と誤解しやすいが、ベクトルは不変データである。
   埋め込みモデルをロードしないコンパクションは、深夜のバックグラウンド実行すら
   不要なほど軽い (数 MB のコピー)。
3. **墓標の in-place 書きには remap を必ず随伴させる (§2.1.3, 罠 T-1)。** POSIX の
   MAP_PRIVATE はフォールト済みページに対して後続のファイル書きを反映しない。
   「Windows では見えるのに macOS では古い chunk_id が生き続ける」という OS 依存の
   静かな死を、余計な賢さ (MAP_SHARED 化) ではなく明示的な remap で殺す。
   MAP_SHARED 化を却下した理由: 1 機能のためにクラッシュセマンティクスを全プラット
   フォームで変えるのは釣り合わない。
4. **Puppeteer の無菌化は「リライト」ではなく「静的バンクからの選択」(§3.4)。**
   ベースラインの「一般的な質問に無菌化して生成」を却下。LLM リライトは統計的に
   漏れる。選択は構成的に漏れない。さらに `_assert_no_gap_leak` にホワイトリスト
   検査を追加し、回帰ガードを弱めるどころか強化する。これが本書で最も重要な上書き。
5. **共倒れ減衰は可逆・削除は不可逆、ゆえに削除禁止 (§3.3)。** ベースラインの
   Weight=0.05 を採用した上で、`disputed_with` 相互リンクと第三証拠による復権
   プロトコルを追加。人間の記憶の矛盾は消すべきノイズではなく、それ自体が
   最も情報量の多いプロファイルデータである (矛盾 → Bounty → Puppeteer という
   資源循環がこの思想の実装)。
6. **「追い込む」パラメータは頻度ではなくタイミング (§3.7)。** ベースラインには
   なかった独自追加: PROBE セッションを stabilizer_effect 検出直後に配置する。
   行動データが「今この人は回復済みで帯域がある」と示す瞬間にだけ深い質問を打つ。
   疲弊時の連打は倫理的に悪いだけでなく、データとしてもノイズである — 倫理と
   データ品質がここでは同じ方向を向く。
7. **進行度は表示のみ高水位標 (§3.6)。** 内部値の低下を隠すのは UI 演出として許すが、
   ディスク上の raw 値は正直に記録する。「UI ブラックボックス / ディスク ホワイト
   ボックス」の分離線を明文化した (原則 3)。この線を越えてディスク側を偽装する
   変更は、本システムの存在理由 (自己欺瞞の破壊) への自己矛盾である。
8. **Delta は Bravo の上に建つ (§3.3)。** cross_validate は probe 回答 1 件ごとに
   ベクトル検索を撃つ。旧経路 (27.5ms + プロセス生成) なら設計不可能だった多段
   検証が、109µs の Bravo により「全回答を常時裏取りする」設計として成立する。
   インフラの高速化は機能の解禁である — これが Bravo を先に完遂した理由の追認。
9. **「ホワイトリスト廃止」をチャネル新設として実装する (§3.9.1)。** ベースラインの
   字義通り「制約を外して全ログをコーパスに流し込む」実装を却下。他者の発話は
   本人の自己申告でも本人の行動でもない — 既存 2 コーパスのどちらに混ぜても
   定義が壊れ、gap 検出の意味論が崩壊する。正しい形は第 3 チャネル (テレメトリ)
   の新設であり、既存チャネルの定義凍結 (I-16) とセットで初めて安全になる。
   「データを増やす」と「チャネルを増やす」は別の操作である。
10. **第三者は刺激であって被写体ではない (§3.0 原則 5 / I-15)。** 全ログ解析は
   技術的には相手の人格プロファイリングも可能にするが、作らない。理由は倫理だけ
   ではない: 本システムの評価関数は本人であり、第三者モデルの精度に投資する
   計算資源・プロンプト予算・保存領域はすべて本人プロファイルから盗んだものに
   なる。salt 付き一方向 alias は「実装者が後から誘惑に負けても実名に戻せない」
   構造的な退路の遮断である。
11. **社会的立ち位置の代理変数はレイテンシ非対称である (§3.9.2)。** ベースラインの
   「他者から見た立ち位置」を LLM の感情分析で推定する誘惑を却下。返信速度の
   非対称・会話開始比率・発話量シェアは、嘘をつかない物理量であり決定論で取れる。
   「相手がどう思っているか」を推定するのではなく「関係に誰がコストを払っているか」
   を実測する — 測れないものを推定せず、測れるものだけで殴るのが本システムの流儀。

---

# §5【不変条件の言語化】実装者が絶対に破ってはならない制約と、予測される罠

## 5.1 継承不変条件 (AI_SKILLS より。違反 = 即レビュー落ち)

- I-0: 完全オフライン。Delta の全機能は 127.0.0.1 とローカルファイルのみで完結する。
- I-1: core/ の決定論ロジックは stdlib のみ。発見=決定論 / 言語化=LLM の分業。
- I-2: Python エンジンの stdout はプロトコル専用線。全ファイル I/O に encoding="utf-8"。
- I-3: 遅延初期化 (embed サーバーも初回埋め込みまで起動しない)。RECORD 保存で
  profiler/probe を走らせない。
- I-4: シミュレーター由来ログは append_consultation(..., simulated=True)。
  PROBE の回答は simulated **ではない** (私的内省 = 主観軸・重み 1.0)。
- I-5: テストは決定論・実データ非接触・依存注入 (fake daemon / fake LLM / 一時
  PKB_PROJECT_ROOT)。exe 必須テストはゲート付き SKIP。

## 5.2 本書で新設する不変条件

- I-6: **埋め込み空間の同一性。** manifest.embedder_id と現 embedder の不一致 =
  全再構築。異空間ベクトルの混合検索を許すコードパスを 1 本も作らない。
- I-7: **マニフェストの更新は os.replace のアトミック 1 回。** 部分更新された
  segments.json が存在してはならない。
- I-8: **墓標は -1 のみ。** 他の負値に意味を与えない。墓標書き込みバッチの直後に
  必ず該当セグメントへ remap。
- I-9: **セグメント/インデックスの削除・上書きは「remap → 操作」の順。** 逆順は
  Windows で PermissionError (Bravo で実証済みの時限爆弾)。
- I-10: **HistoricalNode は append-only。** 訂正は superseded_by リンク付き新ノード。
  編集・削除 API を作らない。weight=0 も禁止 (最小 0.05)。
- I-11: **Puppeteer の注入質問は QUESTION_BANK 実在 ID のみ** (ホワイトリスト assert)。
  LLM による質問リライトでの「無菌化」は情報漏洩バグとみなす。
- I-12: **MBTI 投影・進行度は分析入力に戻さない** (表示専用。フィードバックループ
  遮断)。
- I-13: **講評フェーズ以外で Bounty の存在を面接官コンテキストに出さない。**
- I-14: **結論の遮断 (聖域の拡張)。** Delta-LINE 由来の一切の結論 (interpersonal 軸・
  DyadStats・social_positioning_gap・Bounty 内容・テレメトリ生値) を、面接官/GD
  議論フェーズ・es_review のコンテキストに出さない。面接官に到達してよいのは
  QUESTION_BANK の実在質問テキストのみ (I-11 との二重ガード)。`_assert_no_gap_leak`
  の検査キーに `source_code` / `interpersonal` / `line_telemetry` / `bounties` を
  追加し、この拡張をテストで固定してから機能を書く (ガードが先、機能が後)。
- I-15: **第三者データ最小化。** 派生ストア・LLM プロンプト・ログに第三者の実名を
  出さない (salt 付き一方向 alias)。第三者の人格プロファイルを構築・永続化する
  コードパスを作らない。証拠引用は 120 字上限・本人発話優先。分析の被写体は常に
  本人である (§3.0 原則 5)。
- I-16: **チャネル分離の凍結。** LINE 全ログを読むのは対人テレメトリ (第3チャネル)
  のみ。gap_analysis の主観コーパス (日記+相談) / 客観コーパス (家計簿+予定+
  line_self_text) の定義は 1 文字も変えない。他者発話をどちらかのコーパスへ流す
  変更は `test_subjective_objective_separation` の破壊 = 退化と認定する。

## 5.3 予測される罠 (エッジケースへの警告)

- **T-1 (OS依存の静かな死)**: POSIX MAP_PRIVATE は in-place 墓標書きを反映しない。
  remap 随伴を怠ると「macOS でだけ削除済み日記が検索に出る」。テストで再現しづらい
  ため、実装時に remap 呼び出しの有無を assert するユニットテストを書け。
- **T-2 (クラッシュ窓の重複)**: マニフェスト確定と墓標書きの間のクラッシュで同一
  日付が 2 チャンク生存。検索側の日付デデュープ (§2.1.4) が最後の砦 — デデュープを
  「非効率だから」と削るな。
- **T-3 (content_hash のソース漏れ)**: 家計簿だけ編集した日が再埋め込みされない。
  ハッシュ対象は DailyContext を構成する**全**ソース。data_merger にソースを追加
  したら content_hash にも追加 (grep 用マーカーコメントを両所に置け)。
- **T-4 (e5 プレフィックス)**: "query: "/"passage: " を付け忘れても動く (精度だけ
  落ちる)。GgufEmbedder 内部に閉じ込め、単体テストでプレフィックス付与を assert。
- **T-5 (片側だけの embedder 移行)**: クエリだけ GGUF / 文書は旧モデル、は
  「動くが常に無関係な結果」。embedder_id の照合はクエリ側の embed() にも入れる。
- **T-6 (scratch max_k=64 とセグメント)**: C1 (Python マージ) はセグメントごとに
  ≤64 件なので安全。C2 (C++ マージ) 実装時も応答は Top-64 で足りる — scratch
  レイアウトを拡張する必要はない。拡張したくなったらそれは top_k の使いすぎ。
- **T-7 (probe 回答の建前混入)**: 就活が近い時期の PROBE 回答は建前化しやすい。
  対策はフラグではなく cross_validate — 裏付けの無い「盛った」回答は自動的に
  weight が上がらない。信頼は検索が与える。手動の trusted フラグ操作 UI を作るな。
- **T-8 (自己強化発振)**: MBTI/進行度を主観コーパスや 5 軸計算に戻すと、投影が
  投影を強化して発振する。I-12 の理由。レビューでは「project_mbti の戻り値が
  どこへ流れるか」を必ず追跡せよ。
- **T-9 (LLM 検収の無限ループ)**: NARRATIVE COMPILER の再生成は最大 2 回で打ち切り、
  欠落段落を除いた短縮版を採用。「もう 1 回だけ」のリトライ増加は TTFT と電力の
  死 (7B ローカルモデルの再生成は数十秒単位)。
- **T-10 (probe_store の肥大)**: EvidenceRef.quote は 120 字上限、Axis.evidence は
  上位 5 件のみ。日記全文の複製を probe_store に溜めるな (個人データの分散保管は
  漏洩面積の拡大)。
- **T-11 (文脈誤読による行動アルゴリズムの歪曲)**: LINE の言語は高文脈である —
  皮肉・内輪ネタ・引用リプ・スタンプ連打を字義通りに読むと、仲の良い dyad ほど
  「摩擦だらけ」に見える (関係が深いほど否定語彙の遊びが増えるため、単一キーワード
  検出は**親密度と摩擦を正相関で誤認する**最悪の系統誤差を持つ)。回避策は 4 層:
  (1) 摩擦判定は 2-of-3 多重シグナル必須 — 語彙だけでは絶対に成立させない、
  (2) dyad ごとの相対化 — 閾値は全体定数ではなく当該 dyad の中央値基準、
  (3) グループチャットの v1 全面除外、(4) LLM 感情推定の禁止 (決定論マーカーのみ)。
  テストには「否定語彙が多いが即レス・スレッド継続する dyad はフラグしない」
  対照群を必ず置け。
- **T-12 (レイテンシの交絡因子)**: 深夜到着への翌朝返信・通知バッチ・授業/勤務時間は
  「遅さ」ではない。23:00-08:00 到着分の除外と 48h 超のスレッド死への転換 (§3.9.2) を
  実装から落とすと、latency_asymmetry は生活リズムの測定器に堕ちる。さらに
  **LINE に写らない関係** (対面中心の親友・家族) は「疎遠」に見える — テレメトリは
  LINE 上の関係の記述であって人間関係全体の記述ではない。この限定を LLM 言語化
  プロンプトに必ず注記させろ (「LINE上の観測範囲では」の限定句を削るな)。
- **T-13 (第三者データの分散漏洩)**: 実装が進むと「デバッグ用に実名を一時ログへ」
  「エラーメッセージに発話全文を」が必ず起きる。line_telemetry 系の例外・ログ出力は
  alias と件数のみを含む専用フォーマッタを 1 箇所に用意し、生テキストを触る変数を
  ログ関数へ渡すコードをレビューで機械的に落とせ (AI_SKILLS §1「ログに LINE 本文を
  出すな」の Delta-LINE 適用)。テストで「例外メッセージに実名・発話が含まれない」
  ことを assert する。

## 5.4 実装順序 (依存関係に基づく強制順序)

```
C1: LSM (Pythonマージ)  ──┐        テストゲート: 既存5スイート + test_lsm (新規)
C2: C++ セグメントマージ ──┤        ゲート: benchmark.py 前後比較 (C1 実測が遅い場合のみ)
C3: GGUF 埋め込み        ──┘        ゲート: 同一テキストの新旧 embedder 類似度検収
D1: 5軸 + probe_store    (C 系と独立に着手可)   ゲート: test_source_code (決定論)
DL1: LINE テレメトリ (DyadStats + 3軸)  (D1 と並行可。data_merger 出力のみに依存)
     ゲート: test_line_telemetry — T-11/T-12 の対照群 (親密dyad非フラグ・深夜除外) 必須
DL2: social_positioning_gap + Bounty 合流  (DL1 + D1 に依存)
     ゲート: _assert_no_gap_leak 拡張版 (I-14 のキー追加) を先に書いて RED を確認
D2: PROBE ファネル + cross_validate (D1 + Bravo に依存)
D3: Puppeteer            (D2 + DL2 + QUESTION_BANK に依存)  ゲート: ホワイトリスト assert
D4: NARRATIVE COMPILER   (D1-D3 の成果を消費)
D5: ゲーミフィケーション UI (最後。表示だけなので全機能の後)
各段で AI_SKILLS.md に不変条件を追記してから次へ進むこと (第0原則)。
DL 系の追加規律: I-14 のガード (テスト) は DL2 の機能コードより先にコミット順で
存在しなければならない。「ガードが先、機能が後」— 聖域の拡張は防壁の拡張から始まる。
```

---
*本仕様書は Target Bravo 完遂セッションの最終成果物である。実装者へ: 疑ったら
計測しろ。計測できないなら、それはまだ設計が終わっていない。*
