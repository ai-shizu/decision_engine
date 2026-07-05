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
//  実行:
//    build/search_engine.exe data/processed/vectors.bin data/processed/query.bin [top_k]
// ============================================================================

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <algorithm>
#include <chrono>
#include <string>
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

// ---------------------------------------------------------------- mmap 抽象化
struct MappedFile {
    const uint8_t* data = nullptr;
    size_t         size = 0;
#ifdef _WIN32
    HANDLE hFile = INVALID_HANDLE_VALUE;
    HANDLE hMap  = nullptr;
#else
    int fd = -1;
#endif

    bool open(const char* path) {
#ifdef _WIN32
        hFile = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, nullptr,
                            OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
        if (hFile == INVALID_HANDLE_VALUE) return false;
        LARGE_INTEGER li{};
        if (!GetFileSizeEx(hFile, &li)) return false;
        size = static_cast<size_t>(li.QuadPart);
        hMap = CreateFileMappingA(hFile, nullptr, PAGE_READONLY, 0, 0, nullptr);
        if (!hMap) return false;
        data = static_cast<const uint8_t*>(MapViewOfFile(hMap, FILE_MAP_READ, 0, 0, 0));
        return data != nullptr;
#else
        fd = ::open(path, O_RDONLY);
        if (fd < 0) return false;
        struct stat st{};
        if (fstat(fd, &st) != 0) return false;
        size = static_cast<size_t>(st.st_size);
        void* p = ::mmap(nullptr, size, PROT_READ, MAP_PRIVATE, fd, 0);
        if (p == MAP_FAILED) return false;
  #ifdef POSIX_MADV_WILLNEED
        ::posix_madvise(p, size, POSIX_MADV_WILLNEED);
  #endif
        data = static_cast<const uint8_t*>(p);
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
    int32_t chunk_id;
};

struct TopK {
    std::vector<Hit> hits;   // score 降順を維持
    size_t k;

    explicit TopK(size_t k_) : k(k_) { hits.reserve(k_ + 1); }

    float threshold() const {
        return hits.size() < k ? -1e30f : hits.back().score;
    }

    void push(float score, int32_t id) {
        if (hits.size() >= k && score <= hits.back().score) return;
        auto pos = std::upper_bound(hits.begin(), hits.end(), score,
                                    [](float s, const Hit& h) { return s > h.score; });
        hits.insert(pos, {score, id});
        if (hits.size() > k) hits.pop_back();
    }

    void merge(const TopK& other) {
        for (const Hit& h : other.hits) push(h.score, h.chunk_id);
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
                if (id >= 0 && s[l] > local.threshold()) local.push(s[l], id);
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
            if (id >= 0 && s[l] > global.threshold()) global.push(s[l], id);
        }
    }
#endif
    return global;
}

// ---------------------------------------------------------------- エントリポイント
int main(int argc, char** argv) {
    const char* vec_path   = argc > 1 ? argv[1] : "data/processed/vectors.bin";
    const char* query_path = argc > 2 ? argv[2] : "data/processed/query.bin";
    const size_t top_k     = argc > 3 ? static_cast<size_t>(std::stoul(argv[3])) : 5;

    // --- ベクトルDBをマップ
    MappedFile mf;
    if (!mf.open(vec_path)) {
        std::fprintf(stderr, "error: cannot map '%s'\n", vec_path);
        return 1;
    }
    if (mf.size < sizeof(FileHeader)) {
        std::fprintf(stderr, "error: file too small\n");
        return 1;
    }
    const auto* hdr = reinterpret_cast<const FileHeader*>(mf.data);
    if (std::memcmp(hdr->magic, "PKBVEC01", 8) != 0 ||
        hdr->dim != kDim || hdr->lanes != kLanes ||
        hdr->block_bytes != sizeof(VectorBlock) ||
        mf.size != sizeof(FileHeader) + size_t(hdr->num_blocks) * sizeof(VectorBlock)) {
        std::fprintf(stderr, "error: header/layout mismatch (Python側と非同期?)\n");
        return 1;
    }
    const auto* blocks = reinterpret_cast<const VectorBlock*>(mf.data + sizeof(FileHeader));

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
    TopK result = search(blocks, hdr->num_blocks, query, top_k);
    constexpr int kIters = 100;
    const auto t0 = std::chrono::steady_clock::now();
    for (int i = 0; i < kIters; ++i)
        result = search(blocks, hdr->num_blocks, query, top_k);
    const auto t1 = std::chrono::steady_clock::now();
    const double us =
        std::chrono::duration<double, std::micro>(t1 - t0).count() / kIters;

    std::printf("latency: %.1f us/query (avg of %d)\n\n", us, kIters);
    std::printf("Top-%zu results (cosine):\n", top_k);
    for (size_t i = 0; i < result.hits.size(); ++i)
        std::printf("  #%zu  chunk_id=%-4d  score=%.4f\n",
                    i + 1, result.hits[i].chunk_id, result.hits[i].score);
    std::printf("\n(chunk_id は data/processed/metadata.json の chunks[].id に対応)\n");
    return 0;
}
