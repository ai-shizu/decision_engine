// ============================================================================
//  PKB Search Engine Core — Snapdragon X (Windows on ARM) 向け
// ============================================================================
//  - ARM NEON 組み込み関数 (vld1q_f32 / vfmaq_f32 / vdupq_n_f32) による
//    4-lane 同時内積計算
//  - AoSoA レイアウト: struct VectorBlock { float data[384][4]; int32_t chunk_ids[4]; }
//    → 1回のロードで「4本のベクトルの同一次元」を取得し、SIMDレーンを常に充填
//  - OpenMP: 各スレッドがローカル Top-K ヒープを保持し、最後に直列マージ
//    (共有ヒープへのロック競合を完全排除)
//  - ファイルマッピング: Windows は CreateFileMapping/MapViewOfFile、
//    POSIX は mmap(MAP_PRIVATE) によるゼロコピー読み込み
//
//  バイナリ仕様は src/python/pipeline.py の docstring と完全同期。
//
//  コンパイル (Windows on ARM / clang++):
//    clang++ -O3 -std=c++17 -march=armv8-a+simd -fopenmp ^
//        src/cpp/search_engine.cpp -o build/search_engine.exe
//  OpenMP ランタイムが無い環境では -fopenmp を外してもビルド可 (直列動作)。
//
//  実行 (1-shot / 互換維持):
//    build/search_engine.exe data/processed/vectors.bin data/processed/query.bin [top_k]
//
//  実行 (常駐デーモン / Target Bravo):
//    build/search_engine.exe --daemon <scratch_path>
//    - 制御プレーン: stdin から JSON 1行ずつ受信、stdout へ JSON 1行ずつ応答。
//      デーモンモードの stdout はプロトコル専用線 (Python engine_stdio と同原則)。
//      診断メッセージは全て stderr へ。
//    - データプレーン: <scratch_path> を両プロセスが mmap (レイアウトは
//      ScratchBuffer / src/python/core/search_daemon.py と 1 バイト単位で同期)。
//    - stdin の EOF (親 Python の死 = パイプ切断) で自己終了する。
// ============================================================================

#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <chrono>
#include <iostream>
#include <map>
#include <memory>
#include <stdexcept>
#include <string>
#include <type_traits>
#include <vector>

#if defined(__ARM_NEON) || defined(__ARM_NEON__)
  #include <arm_neon.h>
  #define PKB_HAS_NEON 1
#else
  #define PKB_HAS_NEON 0
#endif

#ifdef _OPENMP
  #include <omp.h>
#endif

#ifdef _WIN32
  #define WIN32_LEAN_AND_MEAN
  #include <windows.h>
#else
  #include <fcntl.h>
  #include <sys/mman.h>
  #include <sys/stat.h>
  #include <unistd.h>
#endif

// ---------------------------------------------------------------- レイアウト定義
constexpr uint32_t kDim   = 384;
constexpr uint32_t kLanes = 4;

#pragma pack(push, 1)
struct FileHeader {                 // 32 bytes — pipeline.py の "<8sIIIIII" と一致
    char     magic[8];              // "PKBVEC01"
    uint32_t dim;
    uint32_t lanes;
    uint32_t num_vectors;
    uint32_t num_blocks;
    uint32_t block_bytes;
    uint32_t reserved;
};

struct VectorBlock {                // 6160 bytes
    float   data[kDim][kLanes];     // dim-major / lane-minor (AoSoA)
    int32_t chunk_ids[kLanes];      // パディングレーンは -1
};
#pragma pack(pop)

static_assert(sizeof(FileHeader)  == 32,   "FileHeader layout mismatch");
static_assert(sizeof(VectorBlock) == 6160, "VectorBlock layout mismatch");

// ------------------------------------------- 共有 scratch レイアウト (Target Bravo)
// Python 側の唯一の対応物は src/python/core/search_daemon.py の struct 定義。
// 変更する時は両方を同時に変え、magic のバージョンを上げること (PKBVEC01 と同規則)。
// little-endian 前提。全フィールドが自然整列になるよう設計してあるが、
// 将来の改変での暗黙パディング混入を防ぐため pack(1) + offsetof で釘付けにする。
//
// 同期モデル (seqlock 簡易版):
//   Python が seq / top_k / query を書く → stdio で {"cmd":"search","seq":N} を送る
//   → C++ が scratch の seq とリクエストの seq の一致を検証して検索
//   → results / result_count を書いてから stdio 応答を返す。
//   stdio の 1 行往復そのものがメモリバリアを兼ねるため、ロックは不要。
constexpr uint32_t kScratchMaxK    = 64;
constexpr char     kScratchMagic[8] = {'P','K','B','S','C','R','0','1'};

#pragma pack(push, 1)
struct ScratchResult {              // 8 bytes — Python "<if"
    int32_t chunk_id;               // 未使用スロットは -1 (PKBVEC01 のパディング/墓標規約と同一)
    float   score;
};

struct ScratchBuffer {              // 2072 bytes — Python 側 SCRATCH_SIZE と一致
    char          magic[8];         // "PKBSCR01"
    uint64_t      seq;              // Python が書く。リクエスト行の seq と一致必須
    uint32_t      top_k;            // Python が書く (kScratchMaxK 超は clamp)
    uint32_t      result_count;     // C++ が書く (有効 results 数)
    float         query[kDim];      // Python が書く (f32 × 384)
    ScratchResult results[kScratchMaxK];  // C++ が書く
};
#pragma pack(pop)

static_assert(sizeof(ScratchResult) == 8,                        "ScratchResult layout mismatch");
static_assert(offsetof(ScratchBuffer, seq)          == 8,        "ScratchBuffer.seq offset");
static_assert(offsetof(ScratchBuffer, top_k)        == 16,       "ScratchBuffer.top_k offset");
static_assert(offsetof(ScratchBuffer, result_count) == 20,       "ScratchBuffer.result_count offset");
static_assert(offsetof(ScratchBuffer, query)        == 24,       "ScratchBuffer.query offset");
static_assert(offsetof(ScratchBuffer, results)      == 24 + kDim * 4,
              "ScratchBuffer.results offset");                   // 1560
static_assert(sizeof(ScratchBuffer) == 24 + kDim * 4 + kScratchMaxK * 8,
              "ScratchBuffer total size mismatch");              // 2072

// ---------------------------------------------------------------- Target Echo: PKBTEN01
// 日次×特徴量テンソル (mmap ゼロコピー共有)。docs/SPEC_ECHO_GENESIS.md §5.1/§5.1.1。
// Python 側の唯一の対応物は core/tensor_store.py。変更は両方同時 + magic バージョン更新。
//
// v1 (E1〜E4) では C++ はこの struct を読まない (numpy が同一ファイルを
// ゼロコピーで読む)。境界面を将来も動かさないため struct だけ今日凍結する。
// 全フィールド自然整列 (header 64B、row 136B ≡ 0 mod 8)。little-endian。
constexpr uint32_t kTenFeat = 32;
constexpr char     kTensorMagic[8] = {'P','K','B','T','E','N','0','1'};

#pragma pack(push, 1)
struct TensorHeader {            // 64 bytes — Python "<8sIIIIiIQ16s8x"
    char     magic[8];           // "PKBTEN01"
    uint32_t version;            // = 1
    uint32_t n_rows;             // 日数 (密。row i = epoch_day + i 日)
    uint32_t n_features;         // 使用レーン数 (<= kTenFeat)
    uint32_t row_stride;         // = sizeof(TensorRow) = 136 (読み手は必ずこれを使う)
    int32_t  epoch_day;          // row 0 の日付 (1970-01-01 からの日数, ローカル暦日)
    uint32_t flags;              // bit0: dyad スコープ / 他ビット予約 (0)
    uint64_t content_hash64;     // 全canonical input + feature/runtime identity
    uint8_t  payload_hash128[16];// header identity + 全row bytes のBLAKE2b-128
    uint8_t  reserved[8];        // 0 埋め
};

struct TensorRow {               // 136 bytes — Python "<iI32f"
    int32_t  day_index;          // 常に行番号と一致 (整合性検証用の冗長フィールド)
    uint32_t valid_mask;         // bit k = レーン k 観測済み。欠測は値 0.0f + bit 0
    float    f[kTenFeat];        // NaN 格納禁止 (I-18)
};
#pragma pack(pop)

static_assert(sizeof(TensorHeader) == 64,               "TensorHeader layout mismatch");
static_assert(offsetof(TensorHeader, version)       ==  8, "TensorHeader.version offset");
static_assert(offsetof(TensorHeader, n_rows)        == 12, "TensorHeader.n_rows offset");
static_assert(offsetof(TensorHeader, n_features)    == 16, "TensorHeader.n_features offset");
static_assert(offsetof(TensorHeader, row_stride)    == 20, "TensorHeader.row_stride offset");
static_assert(offsetof(TensorHeader, epoch_day)     == 24, "TensorHeader.epoch_day offset");
static_assert(offsetof(TensorHeader, flags)         == 28, "TensorHeader.flags offset");
static_assert(offsetof(TensorHeader, content_hash64)== 32, "TensorHeader.hash offset");
static_assert(offsetof(TensorHeader, payload_hash128)==40, "TensorHeader.payload hash offset");
static_assert(sizeof(TensorRow) == 136,                 "TensorRow layout mismatch");
static_assert(offsetof(TensorRow, f) == 8,              "TensorRow.f offset");
static_assert(sizeof(TensorRow) % 8 == 0,               "TensorRow 8-byte alignment");
// W-3: コンパイラ依存パディング/コピー挙動の排除 (bool/enum/ビットフィールド不使用の裏付け)
static_assert(std::is_standard_layout<TensorHeader>::value, "TensorHeader must be standard-layout");
static_assert(std::is_standard_layout<TensorRow>::value,    "TensorRow must be standard-layout");
static_assert(std::is_trivially_copyable<TensorRow>::value, "TensorRow must be memcpy-safe");

// ---------------------------------------------------------------- mmap 抽象化
#ifdef _WIN32
// デーモンの制御プレーンは UTF-8 JSON でパスを受け取るため、UTF-8 → UTF-16 変換を
// 試みてから CreateFileW で開く。変換不能 (= ANSI コードページの argv 等) は
// 従来通り CreateFileA へフォールバックする。
static HANDLE open_file_utf8(const char* path, DWORD access, DWORD share) {
    const int wlen = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS,
                                         path, -1, nullptr, 0);
    if (wlen > 0) {
        std::wstring wpath(static_cast<size_t>(wlen), L'\0');
        MultiByteToWideChar(CP_UTF8, 0, path, -1, &wpath[0], wlen);
        HANDLE h = CreateFileW(wpath.c_str(), access, share, nullptr,
                               OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
        if (h != INVALID_HANDLE_VALUE) return h;
    }
    return CreateFileA(path, access, share, nullptr,
                       OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
}
#endif

struct MappedFile {
    const uint8_t* data = nullptr;   // 読み取りビュー (常に有効)
    uint8_t*       wdata = nullptr;  // writable=true で open した時のみ非 null
    size_t         size = 0;
#ifdef _WIN32
    HANDLE hFile = INVALID_HANDLE_VALUE;
    HANDLE hMap  = nullptr;
#else
    int fd = -1;
#endif

    // writable=true は共有 scratch 専用 (MAP_SHARED / FILE_MAP_ALL_ACCESS —
    // Python 側 mmap との相互可視性が必須)。index の読みは従来通り read-only。
    bool open(const char* path, bool writable = false) {
#ifdef _WIN32
        // Python が同じファイルを write mmap で保持するため SHARE は READ|WRITE
        hFile = open_file_utf8(path,
                               writable ? (GENERIC_READ | GENERIC_WRITE) : GENERIC_READ,
                               FILE_SHARE_READ | FILE_SHARE_WRITE);
        if (hFile == INVALID_HANDLE_VALUE) return false;
        LARGE_INTEGER li{};
        if (!GetFileSizeEx(hFile, &li)) return false;
        size = static_cast<size_t>(li.QuadPart);
        hMap = CreateFileMappingA(hFile, nullptr,
                                  writable ? PAGE_READWRITE : PAGE_READONLY, 0, 0, nullptr);
        if (!hMap) return false;
        void* p = MapViewOfFile(hMap, writable ? FILE_MAP_ALL_ACCESS : FILE_MAP_READ, 0, 0, 0);
        if (!p) return false;
        data = static_cast<const uint8_t*>(p);
        if (writable) wdata = static_cast<uint8_t*>(p);
        return true;
#else
        fd = ::open(path, writable ? O_RDWR : O_RDONLY);
        if (fd < 0) return false;
        struct stat st{};
        if (fstat(fd, &st) != 0) return false;
        size = static_cast<size_t>(st.st_size);
        void* p = ::mmap(nullptr, size,
                         writable ? (PROT_READ | PROT_WRITE) : PROT_READ,
                         writable ? MAP_SHARED : MAP_PRIVATE, fd, 0);
        if (p == MAP_FAILED) return false;
  #ifdef POSIX_MADV_WILLNEED
        ::posix_madvise(p, size, POSIX_MADV_WILLNEED);
  #endif
        data = static_cast<const uint8_t*>(p);
        if (writable) wdata = static_cast<uint8_t*>(p);
        return true;
#endif
    }

    ~MappedFile() {
#ifdef _WIN32
        if (data)  UnmapViewOfFile(data);
        if (hMap)  CloseHandle(hMap);
        if (hFile != INVALID_HANDLE_VALUE) CloseHandle(hFile);
#else
        if (data)   ::munmap(const_cast<uint8_t*>(data), size);
        if (fd >= 0) ::close(fd);
#endif
    }
};

// ---------------------------------------------------------------- Top-K (固定長・挿入ソート)
struct Hit {
    float   score;
    int64_t score_q;
    int32_t chunk_id;
};

static constexpr double kScoreScale = 1000000.0;

static bool try_quantize_score(float score, int64_t* score_q) noexcept {
    if (!std::isfinite(score)) return false;
    const double scaled = static_cast<double>(score) * kScoreScale;
    const double rounded = std::floor(scaled + 0.5);
    if (rounded < -9223372036854775808.0 ||
        rounded >= 9223372036854775808.0) {
        return false;
    }
    *score_q = static_cast<int64_t>(rounded);
    return true;
}

static bool hit_is_better(const Hit& lhs, const Hit& rhs) noexcept {
    if (lhs.score_q != rhs.score_q) return lhs.score_q > rhs.score_q;
    return lhs.chunk_id < rhs.chunk_id;
}

struct TopK {
    std::vector<Hit> hits;   // score 降順を維持
    size_t k;

    explicit TopK(size_t k_) : k(k_) { hits.reserve(k_ + 1); }

    void push(float score, int64_t score_q, int32_t id) {
        if (k == 0) return;
        const Hit candidate{score, score_q, id};
        if (hits.size() >= k && !hit_is_better(candidate, hits.back())) return;
        const auto pos = std::lower_bound(
            hits.begin(), hits.end(), candidate, hit_is_better);
        hits.insert(pos, candidate);
        if (hits.size() > k) hits.pop_back();
    }

    void merge(const TopK& other) {
        for (const Hit& h : other.hits) push(h.score, h.score_q, h.chunk_id);
    }
};

// ---------------------------------------------------------------- コア: 1ブロック 4本同時内積
// 戻り値: lane0..3 の内積が入った float[4]
static inline void block_dot4(const VectorBlock& blk, const float* query, float out[4]) {
#if PKB_HAS_NEON
    // 4本の独立アキュムレータで FMA のレイテンシを隠蔽 (次元方向に4段アンロール)
    float32x4_t acc0 = vdupq_n_f32(0.f);
    float32x4_t acc1 = vdupq_n_f32(0.f);
    float32x4_t acc2 = vdupq_n_f32(0.f);
    float32x4_t acc3 = vdupq_n_f32(0.f);
    for (uint32_t d = 0; d < kDim; d += 4) {
        // data[d][0..3] = 4本のベクトルの次元 d → 1ロードで4レーン充填
        acc0 = vfmaq_f32(acc0, vld1q_f32(blk.data[d + 0]), vdupq_n_f32(query[d + 0]));
        acc1 = vfmaq_f32(acc1, vld1q_f32(blk.data[d + 1]), vdupq_n_f32(query[d + 1]));
        acc2 = vfmaq_f32(acc2, vld1q_f32(blk.data[d + 2]), vdupq_n_f32(query[d + 2]));
        acc3 = vfmaq_f32(acc3, vld1q_f32(blk.data[d + 3]), vdupq_n_f32(query[d + 3]));
    }
    vst1q_f32(out, vaddq_f32(vaddq_f32(acc0, acc1), vaddq_f32(acc2, acc3)));
#else
    // 非ARM環境向けスカラフォールバック (レイアウトは同一)
    float acc[4] = {0.f, 0.f, 0.f, 0.f};
    for (uint32_t d = 0; d < kDim; ++d)
        for (uint32_t l = 0; l < kLanes; ++l)
            acc[l] += blk.data[d][l] * query[d];
    std::memcpy(out, acc, sizeof(acc));
#endif
}

// ---------------------------------------------------------------- 検索本体
static TopK search(const VectorBlock* blocks, int64_t num_blocks,
                   const float* query, size_t k) {
    TopK global(k);
    std::atomic<bool> invalid_score{false};

#ifdef _OPENMP
    const int nthreads = omp_get_max_threads();
    std::vector<TopK> locals(nthreads, TopK(k));

    #pragma omp parallel
    {
        TopK& local = locals[omp_get_thread_num()];
        #pragma omp for schedule(static)
        for (int64_t b = 0; b < num_blocks; ++b) {
            float s[4];
            block_dot4(blocks[b], query, s);
            for (uint32_t l = 0; l < kLanes; ++l) {
                const int32_t id = blocks[b].chunk_ids[l];
                if (id < 0) continue;
                int64_t score_q = 0;
                if (!try_quantize_score(s[l], &score_q)) {
                    invalid_score.store(true, std::memory_order_relaxed);
                    continue;
                }
                local.push(s[l], score_q, id);
            }
        }
    }
    for (const TopK& l : locals) global.merge(l);   // 直列マージ (ロック不要)
#else
    for (int64_t b = 0; b < num_blocks; ++b) {
        float s[4];
        block_dot4(blocks[b], query, s);
        for (uint32_t l = 0; l < kLanes; ++l) {
            const int32_t id = blocks[b].chunk_ids[l];
            if (id < 0) continue;
            int64_t score_q = 0;
            if (!try_quantize_score(s[l], &score_q)) {
                invalid_score.store(true, std::memory_order_relaxed);
                continue;
            }
            global.push(s[l], score_q, id);
        }
    }
#endif
    if (invalid_score.load(std::memory_order_relaxed)) {
        throw std::runtime_error("non-finite or out-of-range search score");
    }
    return global;
}

// ---------------------------------------------------------------- index 検証 (1-shot / daemon 共用)
static bool validate_index(const MappedFile& mf, const FileHeader** hdr_out,
                           const VectorBlock** blocks_out) {
    if (mf.size < sizeof(FileHeader)) return false;
    const auto* hdr = reinterpret_cast<const FileHeader*>(mf.data);
    if (std::memcmp(hdr->magic, "PKBVEC01", 8) != 0 ||
        hdr->dim != kDim || hdr->lanes != kLanes ||
        hdr->block_bytes != sizeof(VectorBlock) ||
        mf.size != sizeof(FileHeader) + size_t(hdr->num_blocks) * sizeof(VectorBlock)) {
        return false;
    }
    *hdr_out    = hdr;
    *blocks_out = reinterpret_cast<const VectorBlock*>(mf.data + sizeof(FileHeader));
    return true;
}

// ============================================================================
//  デーモンモード (Target Bravo)
// ============================================================================
//  制御プレーンの JSON は自前クライアント (core/search_daemon.py) 専用のため、
//  意図的に「フラットなオブジェクト・既知キーのみ」の最小サブセットに限定する。
//  クライアント側は ensure_ascii=False + UTF-8 で送信する規約。

// ---- 応答用: 文字列を JSON エスケープ (エラーメッセージ用・ASCII 前提)
static std::string json_escape(const std::string& s) {
    std::string out;
    out.reserve(s.size() + 8);
    for (char c : s) {
        switch (c) {
            case '"':  out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n";  break;
            case '\r': out += "\\r";  break;
            case '\t': out += "\\t";  break;
            default:
                if (static_cast<unsigned char>(c) < 0x20) {
                    char buf[8];
                    std::snprintf(buf, sizeof(buf), "\\u%04x", c);
                    out += buf;
                } else {
                    out += c;
                }
        }
    }
    return out;
}

// ---- 受信用: `"key"` の直後の `:` に続く値の先頭位置を返す (-1 = キー無し)
static long long json_value_pos(const std::string& line, const std::string& key) {
    const std::string pat = "\"" + key + "\"";
    size_t from = 0;
    while (true) {
        const size_t p = line.find(pat, from);
        if (p == std::string::npos) return -1;
        size_t q = p + pat.size();
        while (q < line.size() && (line[q] == ' ' || line[q] == '\t')) ++q;
        if (q < line.size() && line[q] == ':') {
            ++q;
            while (q < line.size() && (line[q] == ' ' || line[q] == '\t')) ++q;
            return static_cast<long long>(q);
        }
        from = p + 1;   // 文字列値の中に同名の並びがあった場合は読み飛ばす
    }
}

static bool json_find_u64(const std::string& line, const std::string& key, uint64_t* out) {
    const long long pos = json_value_pos(line, key);
    if (pos < 0) return false;
    size_t q = static_cast<size_t>(pos);
    if (q >= line.size() || line[q] < '0' || line[q] > '9') return false;
    uint64_t v = 0;
    while (q < line.size() && line[q] >= '0' && line[q] <= '9')
        v = v * 10 + static_cast<uint64_t>(line[q++] - '0');
    *out = v;
    return true;
}

// \uXXXX (サロゲートペア含む) を UTF-8 へ復号して out へ追記
static bool append_unicode_escape(const std::string& s, size_t* i, std::string* out) {
    auto hex4 = [&](size_t p, uint32_t* v) -> bool {
        if (p + 4 > s.size()) return false;
        uint32_t r = 0;
        for (size_t k = p; k < p + 4; ++k) {
            const char c = s[k];
            r <<= 4;
            if (c >= '0' && c <= '9') r |= uint32_t(c - '0');
            else if (c >= 'a' && c <= 'f') r |= uint32_t(c - 'a' + 10);
            else if (c >= 'A' && c <= 'F') r |= uint32_t(c - 'A' + 10);
            else return false;
        }
        *v = r;
        return true;
    };
    uint32_t cp = 0;
    if (!hex4(*i, &cp)) return false;
    *i += 4;
    if (cp >= 0xD800 && cp <= 0xDBFF) {           // 上位サロゲート
        if (*i + 6 > s.size() || s[*i] != '\\' || s[*i + 1] != 'u') return false;
        uint32_t lo = 0;
        if (!hex4(*i + 2, &lo) || lo < 0xDC00 || lo > 0xDFFF) return false;
        *i += 6;
        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
    }
    if (cp < 0x80) {
        *out += char(cp);
    } else if (cp < 0x800) {
        *out += char(0xC0 | (cp >> 6));
        *out += char(0x80 | (cp & 0x3F));
    } else if (cp < 0x10000) {
        *out += char(0xE0 | (cp >> 12));
        *out += char(0x80 | ((cp >> 6) & 0x3F));
        *out += char(0x80 | (cp & 0x3F));
    } else {
        *out += char(0xF0 | (cp >> 18));
        *out += char(0x80 | ((cp >> 12) & 0x3F));
        *out += char(0x80 | ((cp >> 6) & 0x3F));
        *out += char(0x80 | (cp & 0x3F));
    }
    return true;
}

static bool json_find_string(const std::string& line, const std::string& key, std::string* out) {
    const long long pos = json_value_pos(line, key);
    if (pos < 0) return false;
    size_t q = static_cast<size_t>(pos);
    if (q >= line.size() || line[q] != '"') return false;
    ++q;
    std::string v;
    while (q < line.size()) {
        const char c = line[q];
        if (c == '"') { *out = v; return true; }
        if (c == '\\') {
            if (q + 1 >= line.size()) return false;
            const char e = line[q + 1];
            q += 2;
            switch (e) {
                case '"':  v += '"';  break;
                case '\\': v += '\\'; break;
                case '/':  v += '/';  break;
                case 'n':  v += '\n'; break;
                case 'r':  v += '\r'; break;
                case 't':  v += '\t'; break;
                case 'b':  v += '\b'; break;
                case 'f':  v += '\f'; break;
                case 'u':  if (!append_unicode_escape(line, &q, &v)) return false; break;
                default:   return false;
            }
        } else {
            v += c;
            ++q;
        }
    }
    return false;   // 閉じ引用符なし
}

// ---- 応答送出 (stdout はプロトコル専用線。必ず 1 行 + 即 flush)
static void respond_ok(uint64_t seq, uint32_t count, double elapsed_us) {
    std::printf("{\"ok\":true,\"seq\":%llu,\"count\":%u,\"elapsed_us\":%.1f}\n",
                static_cast<unsigned long long>(seq), count, elapsed_us);
    std::fflush(stdout);
}

static void respond_simple_ok() {
    std::printf("{\"ok\":true}\n");
    std::fflush(stdout);
}

static void respond_error(bool has_seq, uint64_t seq, const std::string& msg) {
    if (has_seq)
        std::printf("{\"ok\":false,\"seq\":%llu,\"error\":\"%s\"}\n",
                    static_cast<unsigned long long>(seq), json_escape(msg).c_str());
    else
        std::printf("{\"ok\":false,\"error\":\"%s\"}\n", json_escape(msg).c_str());
    std::fflush(stdout);
}

// ---- index キャッシュ: path → mmap 済みハンドル (一度だけマップして使い回す)
struct IndexHandle {
    MappedFile         mf;
    const FileHeader*  hdr    = nullptr;
    const VectorBlock* blocks = nullptr;
};

using IndexCache = std::map<std::string, std::unique_ptr<IndexHandle>>;

static IndexHandle* get_index(IndexCache& cache, const std::string& path, std::string* err) {
    auto it = cache.find(path);
    if (it != cache.end()) return it->second.get();
    auto h = std::make_unique<IndexHandle>();
    if (!h->mf.open(path.c_str())) {
        *err = "cannot map index: " + path;
        return nullptr;
    }
    if (!validate_index(h->mf, &h->hdr, &h->blocks)) {
        *err = "index header/layout mismatch: " + path;
        return nullptr;
    }
    IndexHandle* raw = h.get();
    cache.emplace(path, std::move(h));
    return raw;
}

// ---- search コマンド処理
static void handle_search(const std::string& line, ScratchBuffer* scr, IndexCache& cache) {
    uint64_t seq = 0;
    if (!json_find_u64(line, "seq", &seq)) {
        respond_error(false, 0, "missing seq");
        return;
    }
    std::string path;
    if (!json_find_string(line, "index", &path) || path.empty()) {
        respond_error(true, seq, "missing index");
        return;
    }
    // seqlock 検証: scratch の seq とリクエストの seq の不一致 = クライアント側の
    // 書き込み順序バグか別プロセスの割り込み。黙って古いクエリを検索しないこと。
    if (scr->seq != seq) {
        char buf[96];
        std::snprintf(buf, sizeof(buf), "seq mismatch (scratch=%llu, request=%llu)",
                      static_cast<unsigned long long>(scr->seq),
                      static_cast<unsigned long long>(seq));
        respond_error(true, seq, buf);
        return;
    }
    uint32_t k = scr->top_k;
    if (k == 0) {
        respond_error(true, seq, "top_k must be >= 1");
        return;
    }
    if (k > kScratchMaxK) k = kScratchMaxK;

    std::string err;
    IndexHandle* idx = get_index(cache, path, &err);
    if (!idx) {
        respond_error(true, seq, err);
        return;
    }

    // scratch 上の query は 16 バイト整列が保証されないため、整列済みローカルへコピー
    alignas(16) float query[kDim];
    std::memcpy(query, scr->query, sizeof(query));

    const auto t0 = std::chrono::steady_clock::now();
    TopK result(k);
    try {
        result = search(idx->blocks, idx->hdr->num_blocks, query, k);
    } catch (const std::exception& exc) {
        respond_error(true, seq, exc.what());
        return;
    }
    const auto t1 = std::chrono::steady_clock::now();

    for (uint32_t i = 0; i < kScratchMaxK; ++i)
        scr->results[i] = ScratchResult{-1, 0.f};
    const uint32_t count = static_cast<uint32_t>(result.hits.size());
    for (uint32_t i = 0; i < count; ++i)
        scr->results[i] = ScratchResult{result.hits[i].chunk_id, result.hits[i].score};
    // result_count は最後に書く。この後の stdio 応答が Python 側の読み取りバリア。
    scr->result_count = count;

    respond_ok(seq, count,
               std::chrono::duration<double, std::micro>(t1 - t0).count());
}

// ---- デーモン本体
static int run_daemon(const char* scratch_path) {
    MappedFile scratch;
    if (!scratch.open(scratch_path, /*writable=*/true)) {
        std::fprintf(stderr, "daemon: cannot map scratch '%s'\n", scratch_path);
        return 1;
    }
    if (scratch.size < sizeof(ScratchBuffer)) {
        std::fprintf(stderr, "daemon: scratch too small (%zu < %zu)\n",
                     scratch.size, sizeof(ScratchBuffer));
        return 1;
    }
    auto* scr = reinterpret_cast<ScratchBuffer*>(scratch.wdata);
    if (std::memcmp(scr->magic, kScratchMagic, sizeof(kScratchMagic)) != 0) {
        std::fprintf(stderr, "daemon: scratch magic mismatch (Python 側と非同期?)\n");
        return 1;
    }

    IndexCache cache;

    std::printf("{\"event\":\"ready\",\"max_k\":%u,\"scratch_bytes\":%zu,"
                "\"neon\":%s,\"openmp\":%s}\n",
                kScratchMaxK, sizeof(ScratchBuffer),
                PKB_HAS_NEON ? "true" : "false",
#ifdef _OPENMP
                "true"
#else
                "false"
#endif
    );
    std::fflush(stdout);

    // stdin の EOF = 親 Python の死 (パイプ切断) or 明示クローズ。
    // どちらでもループを抜けて自己終了する — ゾンビ化防止の安全装置。
    std::string line;
    while (std::getline(std::cin, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty()) continue;

        std::string cmd;
        if (!json_find_string(line, "cmd", &cmd)) {
            respond_error(false, 0, "missing cmd");
            continue;
        }
        if (cmd == "search") {
            handle_search(line, scr, cache);
        } else if (cmd == "remap") {
            // 指定 index のマッピングを即時解放 (次の search で遅延リマップ)。
            // Windows ではマップ中のファイルを再構築できないため、
            // Python 側はインデックス再構築の【前】にこれを送ること。
            std::string path;
            if (!json_find_string(line, "index", &path)) {
                respond_error(false, 0, "missing index");
            } else {
                cache.erase(path);
                respond_simple_ok();
            }
        } else if (cmd == "ping") {
            respond_simple_ok();
        } else if (cmd == "shutdown") {
            respond_simple_ok();
            break;
        } else {
            respond_error(false, 0, "unknown cmd: " + cmd);
        }
    }
    return 0;
}

// ---------------------------------------------------------------- エントリポイント
int main(int argc, char** argv) {
    if (argc > 1 && std::strcmp(argv[1], "--daemon") == 0) {
        if (argc < 3) {
            std::fprintf(stderr, "usage: search_engine --daemon <scratch_path>\n");
            return 1;
        }
        return run_daemon(argv[2]);
    }
    const char* vec_path   = argc > 1 ? argv[1] : "data/processed/vectors.bin";
    const char* query_path = argc > 2 ? argv[2] : "data/processed/query.bin";
    const size_t top_k     = argc > 3 ? static_cast<size_t>(std::stoul(argv[3])) : 5;

    // --- ベクトルDBをマップ
    MappedFile mf;
    if (!mf.open(vec_path)) {
        std::fprintf(stderr, "error: cannot map '%s'\n", vec_path);
        return 1;
    }
    const FileHeader* hdr = nullptr;
    const VectorBlock* blocks = nullptr;
    if (!validate_index(mf, &hdr, &blocks)) {
        std::fprintf(stderr, "error: header/layout mismatch (Python側と非同期?)\n");
        return 1;
    }

    // --- クエリベクトル読み込み (float32 × 384)
    alignas(16) float query[kDim];
    {
        std::FILE* f = std::fopen(query_path, "rb");
        if (!f || std::fread(query, sizeof(float), kDim, f) != kDim) {
            std::fprintf(stderr, "error: cannot read query '%s'\n", query_path);
            if (f) std::fclose(f);
            return 1;
        }
        std::fclose(f);
    }

    std::printf("PKB search engine  [NEON=%s, OpenMP=%s]\n",
                PKB_HAS_NEON ? "on" : "off",
#ifdef _OPENMP
                "on"
#else
                "off"
#endif
    );
    std::printf("db: %u vectors / %u blocks (%zu bytes mapped)\n",
                hdr->num_vectors, hdr->num_blocks, mf.size);

    // --- ウォームアップ + 計測
    constexpr int kIters = 100;
    TopK result(top_k);
    double us = 0.0;
    try {
        result = search(blocks, hdr->num_blocks, query, top_k);
        const auto t0 = std::chrono::steady_clock::now();
        for (int i = 0; i < kIters; ++i)
            result = search(blocks, hdr->num_blocks, query, top_k);
        const auto t1 = std::chrono::steady_clock::now();
        us = std::chrono::duration<double, std::micro>(t1 - t0).count() / kIters;
    } catch (const std::exception& exc) {
        std::fprintf(stderr, "error: search failed: %s\n", exc.what());
        return 1;
    }

    std::printf("latency: %.1f us/query (avg of %d)\n\n", us, kIters);
    std::printf("Top-%zu results (cosine):\n", top_k);
    for (size_t i = 0; i < result.hits.size(); ++i)
        std::printf("  #%zu  chunk_id=%-4d  score=%.9g\n",
                    i + 1, result.hits[i].chunk_id, result.hits[i].score);
    std::printf("\n(chunk_id は data/processed/metadata.json の chunks[].id に対応)\n");
    return 0;
}
