# C++

> Additive to STANDARDS.md. Read that first. Everything here is C++-specific.
>
> Target: C++23 (minimum C++20). Use cases: audio processing, Rust FFI boundaries, performance-critical paths.
>
> **Key decisions:** C++23 (min C++20), CMake 3.28+ presets, std::expected errors, RAII, jthread, SPSC lock-free for audio, ASan+UBSan in dev, Rust FFI via opaque pointers.

---

## Toolchain

- **Standard:** C++23 (minimum C++20)
- **Compiler:** GCC 14+ / Clang 18+ / MSVC 17.9+
- **Build system:** CMake 3.28+ with presets
- **Generator:** Ninja (required for module support)
- **Linter:** `clang-tidy` with project `.clang-tidy` config
- **Formatter:** `clang-format`
- **Sanitizers:** ASan + UBSan in debug, TSan as separate build
- **Build/validate:**
  ```bash
  cmake --preset dev && cmake --build --preset dev
  ctest --preset dev
  run-clang-tidy -p build/
 ```

### CMake presets

Standardize build configurations in `CMakePresets.json` (checked in) with `CMakeUserPresets.json` (gitignored) for local overrides. Presets should cover dev (debug + sanitizers), release (LTO), and tsan (separate thread sanitizer build).

Full implementation: `reference/cmake-presets.json`

---

## Naming

See STANDARDS.md § Naming for universal conventions.

| Element | Convention | Example |
|---------|-----------|---------|
| Files | `snake_case.cpp` / `snake_case.hpp` | `audio_processor.cpp` |
| Namespaces | `snake_case` | `akroasis::audio` |
| Macros | `UPPER_SNAKE_CASE` (avoid macros) | `AKROASIS_ASSERT` |
| Template parameters | `PascalCase` | `typename Allocator` |
| Private members | `snake_case_` (trailing underscore) | `buffer_size_`, `sample_rate_` |

---

## C++23 adoption

### Adopt now

| Feature | Use | Compiler support |
|---------|-----|-----------------|
| `std::expected<T, E>` | Error handling without exceptions | GCC 13+, Clang 16+, MSVC 17.5+ |
| `std::print` / `std::println` | Type-safe formatted output | GCC 14+, Clang 18+, MSVC 17.7+ |
| Deducing `this` | CRTP replacement, recursive lambdas | GCC 14+, Clang 18+, MSVC 17.8+ |
| `std::unreachable()` | Marks provably unreachable code paths | GCC 13+, Clang 15+, MSVC 17.5+ |
| `if consteval` | Distinguish compile-time vs runtime context | GCC 12+, Clang 14+, MSVC 17.3+ |
| `std::to_underlying` | Safe enum-to-integer conversion | GCC 13+, Clang 13+, MSVC 17.3+ |
| `std::byteswap` | Endian conversion | GCC 13+, Clang 14+, MSVC 17.3+ |
| `std::out_ptr` / `std::inout_ptr` | Smart pointer ↔ C API bridge | GCC 14+, Clang 18+, MSVC 17.9+ |
| `static operator()` / `static operator[]` | Stateless functors without `this` overhead | GCC 13+, Clang 16+, MSVC 17.5+ |
| Multidimensional `operator[]` | `buffer[channel][frame]` syntax | GCC 13+, Clang 15+, MSVC 17.5+ |
| `std::flat_map` / `std::flat_set` | Cache-friendly sorted containers | GCC 15+, Clang 18+, MSVC 17.10+ |

### Track for c++26

| Feature | Status | Why it matters |
|---------|--------|---------------|
| `std::execution` (senders/receivers) | In C++26 | Structured async: replaces ad-hoc thread pools |
| Compile-time reflection | In C++26 | Serialization, enum-to-string, introspection without macros |
| Contracts | In C++26 | `pre`, `post`, `contract_assert`: replaces `assert()` macros |
| `std::hazard_pointer` | In C++26 | Safe memory reclamation for lock-free structures |
| `std::inplace_vector<T, N>` | In C++26 | Fixed-capacity, no-allocation vector: ideal for audio |
| Pattern matching | Not in C++26 | Still in design; track P2688 |
| C++20 modules | Experimental in CMake 3.28+ | Use for new internal code; keep headers for external APIs |

### `std::expected` patterns

The primary error handling mechanism for non-throwing code paths.

```cpp
enum class AudioError {
    InvalidFormat,
    BufferTooSmall,
    DeviceUnavailable,
};

std::expected<AudioBuffer, AudioError> decode_frame(std::span<const uint8_t> data) {
    if (data.size() < HEADER_SIZE)
        return std::unexpected(AudioError::BufferTooSmall);
    // ...
    return AudioBuffer{/*...*/};
}

// Monadic chaining (C++23)
auto result = decode_frame(data)
    .and_then(apply_gain)
    .transform(normalize)
    .or_else([](AudioError e) -> std::expected<AudioBuffer, AudioError> {
        log_error(e);
        return std::unexpected(e);
    });
```

### `std::out_ptr` for FFI

Bridges C-style out-parameters with smart pointers:

```cpp
extern "C" int32_t engine_create(Engine** out);
extern "C" void engine_destroy(Engine*);

auto engine = std::unique_ptr<Engine, decltype(&engine_destroy)>(nullptr, engine_destroy);
engine_create(std::out_ptr(engine));
// engine now owns the handle, auto-destroys
```

---

## Type system

### RAII everywhere

Resources managed by constructors and destructors. No manual `new`/`delete`. No raw owning pointers.

```cpp
// Right
auto processor = std::make_unique<AudioProcessor>(config);

// Wrong
auto* processor = new AudioProcessor(config);
```

### Smart pointer rules

| Type | Use |
|------|-----|
| `std::unique_ptr<T>` | Single ownership (default) |
| `std::shared_ptr<T>` | Shared ownership (justify in comment why unique isn't enough) |
| `std::weak_ptr<T>` | Non-owning observer of shared |
| `T*` / `T&` | Non-owning access (never ownership transfer) |

### `const` by default

- `const` on everything that doesn't need mutation
- `constexpr` for compile-time computation
- `consteval` for functions that must run at compile time
- Parameters by `const&` unless ly copyable

### `std::span` for non-Owning views

Replaces the `(T* ptr, size_t count)` anti-pattern:

```cpp
// Before: no bounds information
void process(const float* data, size_t len);

// After: non-owning view with known extent
void process(std::span<const float> data);
```

### `std::string_view` rules

Non-owning reference to string data. Dangling is the primary risk.

- Use for function parameters (replaces `const std::string&` when null-termination isn't needed)
- Never return `string_view` to local data
- Never store as a class member unless lifetime is guaranteed by design
- When in doubt, use `std::string` for owned data

### `std::variant` over unions

```cpp
// Right
using AudioFormat = std::variant<PcmFormat, FlacFormat, OpusFormat>;

// Wrong
union AudioFormat { PcmFormat pcm; FlacFormat flac; };
```

Visit with `std::visit` and explicit variant handling: no wildcard visitors for variants under your control.

---

## Error handling

See STANDARDS.md § Error Handling for universal principles.

### No exceptions in hot paths

`std::expected<T, E>` for audio processing and performance-critical code. Exceptions acceptable in initialization, configuration, and cold paths.

### Error enum per module

```cpp
enum class AudioError {
    InvalidFormat,
    BufferTooSmall,
    DeviceUnavailable,
};

std::expected<AudioBuffer, AudioError> decode(std::span<const uint8_t> data);
```

### Exception boundaries

Code that calls into exception-throwing libraries must catch at the boundary:

```cpp
std::expected<Config, std::string> load_config(std::string_view path) noexcept {
    try {
        return parse_yaml(path);
    } catch (const std::exception& e) {
        return std::unexpected(std::string(e.what()));
    }
}
```

### SAFETY comments

Same structured comment tags as STANDARDS.md. `// SAFETY:` before any:
- Raw pointer dereference
- `reinterpret_cast`
- Inline assembly
- C library calls with manual lifetime management
- Any `extern "C"` boundary function

---

## Concurrency

### Thread safety defaults

- `std::scoped_lock` for multi-mutex locking (deadlock-free)
- `std::jthread` over `std::thread` (auto-joins, cooperative cancellation via `stop_token`)
- `std::shared_mutex` for read-heavy, write-rare data (config, presets)
- Never hold locks across I/O or long operations

### `std::atomic` and memory ordering

Default to `memory_order_seq_cst`. Relax only with a documented proof of correctness.

| Order | Use |
|-------|-----|
| `seq_cst` | Default. Full ordering. |
| `acquire` | On loads: "I'm reading data someone published" |
| `release` | On stores: "I'm publishing data for others" |
| `acq_rel` | Read-modify-write ops (CAS, fetch_add) |
| `relaxed` | Standalone counters, statistics where ordering doesn't matter |
| `consume` | Deprecated in practice: compilers promote to acquire |

The acquire-release pattern is the workhorse of lock-free code:

```cpp
std::atomic<bool> data_ready{false};
int payload = 0;

// Producer
payload = 42;
data_ready.store(true, std::memory_order_release);   // publishes payload

// Consumer
while (!data_ready.load(std::memory_order_acquire)) {}
assert(payload == 42);                                // guaranteed visible
```

### `std::jthread` and cooperative cancellation

```cpp
process_thread_ = std::jthread([this](std::stop_token token) {
    while (!token.stop_requested()) {
        if (auto buffer = input_queue_.try_pop()) {
            process(*buffer);
        } else {
            std::this_thread::yield();
        }
    }
});
// Destructor calls request_stop() then join(): no manual shutdown
```

### Lock-Free where mandatory

Lock-free is mandatory for the audio thread (cannot tolerate priority inversion). Default to mutexes everywhere else: lock-free is not faster in general, it's lower-latency under contention.

### False sharing prevention

Independently-written atomics must be on separate cache lines:

```cpp
#ifdef __cpp_lib_hardware_interference_size
    inline constexpr size_t CACHE_LINE = std::hardware_destructive_interference_size;
#else
    inline constexpr size_t CACHE_LINE = 64;
#endif

struct Counters {
    alignas(CACHE_LINE) std::atomic<uint64_t> produced{0};
    alignas(CACHE_LINE) std::atomic<uint64_t> consumed{0};
};
```

### Condition variables

Every `wait()` must use a predicate: spurious wakeups are real:

```cpp
// Right
cv.wait(lock, [&] { return !queue.empty(); });

// Wrong: may return with queue still empty
cv.wait(lock);
```

Never wait on a condition variable on the real-time audio thread. Audio thread polls lock-free queues only.

---

## Audio processing

### The real-Time contract

The audio callback runs on a deadline-driven thread (typically 1-10ms budget). Violations cause audible glitches. Every function reachable from the audio callback must be bounded-time and non-blocking.

**Forbidden in the audio callback:**
- Heap allocation/deallocation (`new`, `delete`, `malloc`, `free`, `vector::push_back` that reallocates)
- Blocking locks (`std::mutex::lock`, `std::condition_variable::wait`)
- Exceptions (unwinding allocates)
- System calls (file I/O, console output, logging to disk)
- Thread creation/join
- Any STL container mutation that may allocate

**Permitted:**
- Arithmetic on stack variables and pre-allocated buffers
- Lock-free atomic operations
- Pre-allocated lookup tables
- SPSC queue operations
- SIMD intrinsics on aligned buffers
- `noexcept` functions with bounded execution time

### Audio callback signature

```cpp
void process_block(float** output, const float** input,
                   int num_channels, int num_frames) noexcept {
    for (int ch = 0; ch < num_channels; ++ch) {
        const float* in = input[ch];
        float* out = output[ch];
        for (int i = 0; i < num_frames; ++i) {
            out[i] = process_sample(in[i]);
        }
    }
}
```

Key: `noexcept`, no allocations, no branching on dynamic state that could trigger allocation.

### SPSC ring buffer

The fundamental primitive for passing data between real-time and non-real-time threads. One writer, one reader, no locks.

```cpp
template <typename T, size_t Capacity>
class SpscQueue {
public:
    bool try_push(const T& item) noexcept;
    bool try_pop(T& item) noexcept;
};
```

Full implementation: `reference/spsc-queue.hpp`

Design rules:
- Power-of-2 capacity (bitmask indexing, no modulo)
- Monotonically increasing indices (never wrap: mask on access)
- Cache-line separation for head/tail (prevents false sharing)
- `ly_copyable` constraint (memcpy-safe in buffer)

### Inter-Thread communication patterns

| Pattern | Use case | Data loss? |
|---------|----------|------------|
| SPSC queue | Event streams (MIDI, commands) | No (bounded backpressure) |
| Double buffer | Latest state snapshot | Yes (latest wins) |
| Triple buffer | Latest state, non-blocking both sides | Yes (latest wins) |
| SeqLock | Small read-heavy config | No (retry on conflict) |
| `std::atomic<T>` | Single values (gain, flag) | N/A |

### SeqLock for parameter updates

Single writer (UI thread), multiple readers (audio thread). Writer increments a sequence counter (odd = writing), reader retries on torn read: wait-free in the common case. Use for small, frequently-read parameter structs.

Full implementation: `reference/seqlock.hpp`

### Buffer management

Pre-allocate everything at init time. Lock-free checkout/return via SPSC queues. If the pool is exhausted, output silence: never block.

Full implementation: `reference/audio-buffer-pool.hpp`

### Memory allocation for audio

- All audio-thread memory page-locked with `mlock()` / `mlockall()` (prevents page faults)
- Arena allocators reset per callback for scratch memory
- `std::pmr::monotonic_buffer_resource` backed by pre-allocated buffer for zero-allocation STL usage
- `alignas(32)` or `alignas(64)` on all audio buffers (SIMD alignment)

```cpp
alignas(64) static char audio_arena[1024 * 1024];
std::pmr::monotonic_buffer_resource audio_resource(audio_arena, sizeof(audio_arena));

// In audio callback: allocations come from pre-allocated buffer
std::pmr::vector<float> temp(&audio_resource);
temp.resize(512);  // no malloc
```

### Sample format handling

Internal processing always `float` (32-bit). Integer formats only at I/O boundaries.

```cpp
inline float int16_to_float(int16_t s) noexcept {
    return static_cast<float>(s) / 32768.0f;
}

inline int16_t float_to_int16(float f) noexcept {
    return static_cast<int16_t>(std::clamp(f, -1.0f, 1.0f) * 32767.0f);
}
```

Use deinterleaved (planar) buffers for processing: each channel contiguous for SIMD. Interleave/deinterleave at I/O boundaries.

### SIMD (audio-specific)

Use `__restrict__` on all buffer pointer parameters to enable auto-vectorization:

```cpp
void mix(float* __restrict__ dst, const float* __restrict__ src,
         float gain, int n) noexcept {
    for (int i = 0; i < n; ++i)
        dst[i] += src[i] * gain;
}
```

Compiler flags: `-O2` minimum, `-march=native` for build machine, `-ffast-math` acceptable for audio (exact IEEE compliance less important than throughput). Use `-Rpass=loop-vectorize` (Clang) to verify vectorization.

See § SIMD intrinsics below for the full intrinsics guide (dispatch, alignment, portable wrappers).

### Thread priority

Set real-time priority on the audio thread:
- **Linux:** `SCHED_FIFO` via `pthread_setschedparam` (requires `CAP_SYS_NICE` or `rtprio` in limits.conf)
- **macOS:** `THREAD_TIME_CONSTRAINT_POLICY` via `thread_policy_set`
- **Windows:** MMCSS `AvSetMmThreadCharacteristicsW(L"Pro Audio", &taskIndex)`

Pin audio thread to a performance core for cache locality (`pthread_setaffinity_np` on Linux).

---

## Rust FFI

### Boundary rules

- Only POD types cross the boundary: integers, floats, raw pointers, `#[repr(C)]` structs
- No exceptions across `extern "C"`: catch everything on C++ side
- No C++ types (`std::string`, `std::vector`, `std::unique_ptr`) in `extern "C"` signatures
- Return integer status codes or `std::expected`-style enums for errors
- Document ownership transfer in function names: `_create`/`_destroy`, `_take`/`_borrow`

### Opaque pointer pattern

Expose C++ objects to Rust as opaque handles with create/destroy pairs:

```cpp
// C++ header (extern "C")
typedef struct AudioEngine AudioEngine;

AudioEngine* audio_engine_create(uint32_t sample_rate, uint32_t channels);
void audio_engine_destroy(AudioEngine* engine);
int32_t audio_engine_process(AudioEngine* engine,
                             const float* input, float* output, size_t frames);
```

```cpp
// C++ implementation: catch exceptions at the boundary
extern "C" AudioEngine* audio_engine_create(uint32_t sample_rate, uint32_t channels) {
    try {
        return new AudioEngineImpl(sample_rate, channels);
    } catch (...) {
        return nullptr;
    }
}

extern "C" void audio_engine_destroy(AudioEngine* engine) {
    delete static_cast<AudioEngineImpl*>(engine);
}

extern "C" int32_t audio_engine_process(AudioEngine* engine,
                                         const float* input, float* output,
                                         size_t frames) {
    try {
        static_cast<AudioEngineImpl*>(engine)->process(input, output, frames);
        return 0;
    } catch (...) {
        return -1;
    }
}
```

Rust side wraps in RAII immediately: see Rust standards for the `Drop` guard pattern.

### Error passing

```cpp
enum FfiStatus : int32_t {
    Ok = 0,
    InvalidArgument = 1,
    IoError = 2,
    InternalError = 3,
    BufferTooSmall = 4,
};
```

For rich errors, use a thread-local last-error pattern or caller-provided buffer:

```cpp
extern "C" FfiStatus ffi_do_work(int32_t input,
                                  char* err_buf, size_t err_buf_len) {
    try {
        do_work_inner(input);
        return FfiStatus::Ok;
    } catch (const std::exception& e) {
        if (err_buf && err_buf_len > 0) {
            std::strncpy(err_buf, e.what(), err_buf_len - 1);
            err_buf[err_buf_len - 1] = '\0';
        }
        return FfiStatus::InternalError;
    }
}
```

### Data passing

- **Slices:** pointer + length pair. Use `std::span` internally, decompose at the `extern "C"` boundary.
- **Strings:** pointer + length (preferred) or null-terminated `const char*`. Validate UTF-8 on Rust side.
- **Callbacks:** `extern "C"` function pointer + `void* user_data` context.

### `cxx` vs raw FFI

| Use `cxx` when | Use raw FFI when |
|-----------------|-------------------|
| You control both sides | Pre-existing C API |
| Bidirectional calls | One-directional, simple signatures |
| Want type safety at bridge | Need `#[no_mangle]` exports for C consumer |
| Passing C++ types (string, vector, unique_ptr) | Minimal dependencies |

### Build integration

- **Rust is primary:** `cc` crate or `cxx-build` in `build.rs` to compile C++ sources
- **CMake is primary:** `corrosion` to import Rust crates as CMake targets
- **Linking:** `cc` crate handles `stdc++`/`c++` linking automatically. For manual: `cargo:rustc-link-lib=dylib=stdc++`

---

## SIMD intrinsics

### Strategy: auto-vectorization first, intrinsics when proven necessary

Write scalar loops with `__restrict__` and let the compiler vectorize. Drop to explicit intrinsics only when the compiler's output is measured as insufficient. This ordering exists because intrinsics are architecture-specific, fragile across compiler versions, and opaque to the optimizer.

```cpp
// WHY: __restrict__ promises no aliasing, enabling the compiler to emit
// SIMD without manual intrinsics. Verify with -Rpass=loop-vectorize.
void apply_gain(float* __restrict__ out, const float* __restrict__ in,
                float gain, int n) noexcept {
    for (int i = 0; i < n; ++i)
        out[i] = in[i] * gain;
}
```

### Compile-time ISA dispatch

Use preprocessor guards to select the widest available instruction set at compile time. Each ISA tier gets its own translation unit compiled with the appropriate `-m` flag. The dispatch header selects the right implementation.

```cpp
// simd_dispatch.hpp
#if defined(__AVX512F__)
    #include "dsp_avx512.hpp"
#elif defined(__AVX2__)
    #include "dsp_avx2.hpp"
#elif defined(__SSE2__)
    #include "dsp_sse2.hpp"
#elif defined(__ARM_NEON)
    #include "dsp_neon.hpp"
#else
    #include "dsp_scalar.hpp"
#endif
```

```cmake
# WHY: Each TU compiled with its own ISA flags so intrinsics resolve.
# The dispatch header selects the right symbol at compile time.
add_library(dsp_sse2 OBJECT src/dsp_sse2.cpp)
target_compile_options(dsp_sse2 PRIVATE -msse2)

add_library(dsp_avx2 OBJECT src/dsp_avx2.cpp)
target_compile_options(dsp_avx2 PRIVATE -mavx2 -mfma)

add_library(dsp_avx512 OBJECT src/dsp_avx512.cpp)
target_compile_options(dsp_avx512 PRIVATE -mavx512f)
```

### Runtime CPU dispatch (CPUID)

When a single binary must run on heterogeneous hardware (distributed builds, plugin hosts), use runtime dispatch via CPUID. Resolve once at startup, cache the function pointer.

```cpp
#include <cpuid.h>

namespace simd {

// WHY: Resolved once at init, then called via pointer for zero per-call overhead.
using GainFn = void(*)(float* __restrict__, const float* __restrict__, float, int);

GainFn resolve_apply_gain() noexcept {
    unsigned int eax, ebx, ecx, edx;
    __get_cpuid(1, &eax, &ebx, &ecx, &edx);

    // WHY: Check AVX bit (ECX bit 28) + OSXSAVE (ECX bit 27) together.
    // AVX present without OS support means SIGILL on first use.
    if ((ecx & (1 << 28)) && (ecx & (1 << 27))) {
        __get_cpuid_count(7, 0, &eax, &ebx, &ecx, &edx);
        if (ebx & (1 << 5))  // AVX2
            return apply_gain_avx2;
    }
    return apply_gain_sse2;
}

// WHY: Static init resolves dispatch once; every subsequent call is a direct pointer.
static const GainFn apply_gain = resolve_apply_gain();

} // namespace simd
```

GCC and Clang also support `__attribute__((target_clones("avx2", "sse2", "default")))` for automatic multi-versioning, but it adds a hidden indirect call per invocation and interacts poorly with LTO. Prefer explicit dispatch for hot paths.

### Alignment

SIMD loads and stores require aligned memory. Misaligned access is silently slow on x86 (up to 2x penalty) and a fault on some ARM cores.

```cpp
// WHY: alignas(32) guarantees AVX-aligned loads without runtime checks.
// 64-byte alignment covers AVX-512 and cache-line alignment simultaneously.
alignas(64) float buffer[1024];

// Heap allocation: std::aligned_alloc (C++17)
// WHY: operator new does not guarantee SIMD alignment beyond alignof(max_align_t),
// which is typically 16 bytes — insufficient for AVX (32) or AVX-512 (64).
auto* buf = static_cast<float*>(std::aligned_alloc(64, 1024 * sizeof(float)));
// SAFETY: aligned_alloc requires size to be a multiple of alignment.
// 1024 * 4 = 4096, which is a multiple of 64.
```

For containers, use `std::pmr` with a properly aligned buffer resource (see § Custom memory allocators) or wrap `std::aligned_alloc` in an allocator adaptor.

### Portable intrinsic wrappers

Wrap raw intrinsics behind a thin abstraction that normalizes across SSE/AVX/NEON. Keep the wrapper in a single header per operation (e.g., `simd_gain.hpp`). The wrapper is not an abstraction layer — it is a portability shim. It must compile to the same instructions as raw intrinsics.

```cpp
// WHY: Thin wrapper gives one call site per DSP operation, with the ISA
// detail isolated to a single header. Changing ISA targets does not touch callers.
namespace simd {

#if defined(__AVX2__)
inline void gain_block(float* __restrict__ out, const float* __restrict__ in,
                       float g, int n) noexcept {
    __m256 vgain = _mm256_set1_ps(g);
    // WHY: Process 8 floats per iteration (256 bits / 32 bits).
    int i = 0;
    for (; i + 7 < n; i += 8) {
        __m256 v = _mm256_load_ps(in + i);   // requires 32-byte alignment
        _mm256_store_ps(out + i, _mm256_mul_ps(v, vgain));
    }
    // PERF: Scalar tail for non-multiple-of-8 lengths.
    for (; i < n; ++i)
        out[i] = in[i] * g;
}
#elif defined(__SSE2__)
inline void gain_block(float* __restrict__ out, const float* __restrict__ in,
                       float g, int n) noexcept {
    __m128 vgain = _mm_set1_ps(g);
    int i = 0;
    for (; i + 3 < n; i += 4) {
        __m128 v = _mm_load_ps(in + i);
        _mm_store_ps(out + i, _mm_mul_ps(v, vgain));
    }
    for (; i < n; ++i)
        out[i] = in[i] * g;
}
#elif defined(__ARM_NEON)
inline void gain_block(float* __restrict__ out, const float* __restrict__ in,
                       float g, int n) noexcept {
    float32x4_t vgain = vdupq_n_f32(g);
    int i = 0;
    for (; i + 3 < n; i += 4) {
        float32x4_t v = vld1q_f32(in + i);
        vst1q_f32(out + i, vmulq_f32(v, vgain));
    }
    for (; i < n; ++i)
        out[i] = in[i] * g;
}
#else
inline void gain_block(float* __restrict__ out, const float* __restrict__ in,
                       float g, int n) noexcept {
    for (int i = 0; i < n; ++i)
        out[i] = in[i] * g;
}
#endif

} // namespace simd
```

### Verification

Every SIMD path must be verified two ways:

1. **Compiler output.** Use `-Rpass=loop-vectorize -Rpass-missed=loop-vectorize` (Clang) or `-fopt-info-vec-all` (GCC) to confirm the compiler vectorized auto-vectorizable loops and to catch regressions.
2. **Benchmarks.** Measure the SIMD path against the scalar fallback on the target hardware. If the SIMD path is not measurably faster, delete it: it adds maintenance cost with no benefit.

### SIMD rules

1. **Alignment.** `alignas(32)` minimum for AVX, `alignas(64)` for AVX-512. All audio buffers aligned at allocation time.
2. **Tail handling.** Every SIMD loop must handle input lengths that are not a multiple of the vector width. Scalar tail loop is correct and sufficient.
3. **No `_mm_loadu_ps` in hot paths.** Unaligned loads are slower. Guarantee alignment at allocation time, then use aligned loads.
4. **`-ffast-math` scope.** Apply only to DSP translation units, not globally. It breaks `NaN` propagation, `inf` comparisons, and IEEE compliance.
5. **`std::experimental::simd`** (Parallelism TS v2) is not yet standardized. Track P1928, do not adopt until it lands in a shipping standard.

---

## Custom memory allocators

### When to use a custom allocator

The default allocator (`operator new` / `malloc`) is general-purpose: thread-safe, fragmentation-resistant, and predictable. It is the right choice for most code. Use a custom allocator only when profiling shows one of these problems:

1. **Allocation frequency in hot paths.** Thousands of small allocations per frame (audio callback, render loop, network packet processing). The general allocator's per-call overhead dominates.
2. **Fragmentation in long-running processes.** Variable-size allocations over hours/days fragment the heap, increasing RSS and cache misses.
3. **Deterministic latency.** Real-time threads cannot tolerate the general allocator's worst-case lock contention or mmap syscalls.

If none of these apply, do not introduce a custom allocator.

### `std::pmr` (polymorphic memory resources)

`std::pmr` is the standard allocator customization point since C++17. All custom allocation strategies should use `std::pmr::memory_resource` as the base.

```cpp
#include <memory_resource>
#include <vector>

// WHY: monotonic_buffer_resource never deallocates individual objects.
// It resets the entire buffer at once, giving O(1) "deallocation" cost.
// Ideal for per-frame scratch memory that is discarded as a batch.
alignas(64) static char scratch_buf[64 * 1024];
std::pmr::monotonic_buffer_resource scratch{scratch_buf, sizeof(scratch_buf)};

void process_frame() {
    scratch.release();  // reset to start of buffer: O(1), no destructors
    std::pmr::vector<float> temp{&scratch};
    temp.resize(512);   // no malloc: carved from scratch_buf
    // ... use temp ...
}   // temp destructor is a no-op: monotonic resource ignores deallocate()
```

### Arena allocator (monotonic)

The primary pattern for real-time and high-throughput code. Allocates by bumping a pointer. Individual deallocations are no-ops. The entire arena resets at a defined boundary (frame, request, callback).

```cpp
// WHY: Arena reset per audio callback means zero per-object deallocation cost.
// All scratch memory is reclaimed in one operation between callbacks.
class FrameArena {
public:
    explicit FrameArena(std::span<char> backing)
        : resource_(backing.data(), backing.size()) {}

    std::pmr::memory_resource* resource() noexcept { return &resource_; }

    // WHY: Called once between frames. Not thread-safe by design:
    // the arena belongs to a single thread (the audio thread).
    void reset() noexcept { resource_.release(); }

private:
    std::pmr::monotonic_buffer_resource resource_;
};

// Usage in audio callback:
alignas(64) static char arena_storage[256 * 1024];
static FrameArena frame_arena{arena_storage};

void audio_callback(float** out, const float** in,
                    int channels, int frames) noexcept {
    frame_arena.reset();
    std::pmr::vector<float> work_buf{frame_arena.resource()};
    work_buf.resize(frames);  // no syscall, no lock
    // ...
}
```

### Pool allocator (fixed-size blocks)

For allocating many objects of the same size (nodes, events, messages). Pre-allocates a slab of N blocks, returns them via a free list. O(1) alloc and free, zero fragmentation.

```cpp
// WHY: Pool avoids fragmentation for fixed-size objects and gives O(1)
// alloc/free without locks when used from a single thread.
template <typename T, size_t Capacity>
class FixedPool {
    static_assert(sizeof(T) >= sizeof(void*),
        "Pool element must be at least pointer-sized for free-list linkage");

public:
    FixedPool() noexcept {
        // WHY: Build the free list at construction. Each free slot stores
        // a pointer to the next free slot, overlaid on the object storage.
        for (size_t i = 0; i + 1 < Capacity; ++i)
            *reinterpret_cast<void**>(&storage_[i * sizeof(T)]) =
                &storage_[(i + 1) * sizeof(T)];
        *reinterpret_cast<void**>(&storage_[(Capacity - 1) * sizeof(T)]) = nullptr;
        free_head_ = &storage_[0];
    }

    void* allocate() noexcept {
        if (!free_head_) return nullptr;
        void* block = free_head_;
        free_head_ = *static_cast<void**>(free_head_);
        return block;
    }

    void deallocate(void* ptr) noexcept {
        *static_cast<void**>(ptr) = free_head_;
        free_head_ = ptr;
    }

private:
    alignas(T) char storage_[sizeof(T) * Capacity];
    void* free_head_ = nullptr;
};
```

### Wrapping a custom allocator as `std::pmr::memory_resource`

Any custom allocation strategy must integrate with `std::pmr` so that standard containers (`pmr::vector`, `pmr::string`, `pmr::unordered_map`) work transparently.

```cpp
// WHY: Inheriting from memory_resource lets standard pmr containers use
// the pool without any container-side changes.
template <typename T, size_t Capacity>
class PoolResource : public std::pmr::memory_resource {
    FixedPool<T, Capacity> pool_;

protected:
    void* do_allocate(size_t bytes, size_t alignment) override {
        if (bytes > sizeof(T) || alignment > alignof(T))
            throw std::bad_alloc{};
        void* p = pool_.allocate();
        if (!p) throw std::bad_alloc{};
        return p;
    }

    void do_deallocate(void* p, size_t, size_t) noexcept override {
        pool_.deallocate(p);
    }

    bool do_is_equal(const memory_resource& other) const noexcept override {
        return this == &other;
    }
};
```

### Allocator selection guide

| Allocator | Pattern | Fragmentation | Thread safety | Use case |
|-----------|---------|---------------|---------------|----------|
| Default (`new`) | General | Managed | Thread-safe | Everything unless profiling says otherwise |
| `monotonic_buffer_resource` | Bump pointer, batch reset | None (resets entire arena) | Single-thread | Per-frame scratch, request-scoped temp data |
| `unsynchronized_pool_resource` | Free-list pools by size class | Low | Single-thread | Many variable-size allocs on one thread |
| `synchronized_pool_resource` | Same with mutex | Low | Thread-safe | Multi-threaded pool (not real-time) |
| Custom `FixedPool<T, N>` | Fixed-size free list | None | Single-thread | MIDI events, graph nodes, message queues |

### Rules

1. **Profile first.** Never introduce a custom allocator without measurements showing the default is the bottleneck.
2. **`std::pmr` as the interface.** Custom allocators implement `std::pmr::memory_resource` so containers are not coupled to the allocator strategy.
3. **Single-owner arenas.** Arenas belong to one thread. No shared arenas between threads: that reintroduces the synchronization cost you were trying to avoid.
4. **Pre-allocate backing memory.** Arena and pool backing buffers are allocated (and `mlock`ed for audio) at init time, not in hot paths.
5. **Reset, don't deallocate.** For batch-lifetime memory (per-frame, per-request), use `monotonic_buffer_resource::release()` instead of per-object deallocation.
6. **Overflow policy is explicit.** Document what happens when the arena or pool is exhausted: throw, return null, fall back to upstream resource, or output silence (audio thread).

---

## Thread-local storage

### When to use `thread_local`

Thread-local storage (TLS) gives each thread its own instance of a variable, eliminating synchronization overhead. Use it when:

1. **Per-thread caches or scratch buffers.** Avoids contention on a shared cache while keeping the data warm in the thread's L1/L2 cache.
2. **Per-thread statistics and accumulators.** Aggregate at reporting time, not on every increment.
3. **Last-error patterns for FFI.** Thread-local error state avoids passing error context through every call in a C API.

Do not use TLS as a substitute for proper parameter passing. If a function needs data, take it as a parameter. TLS is for cross-cutting concerns where threading the value through every call site would be disproportionate.

### Initialization and lifetime

`thread_local` variables are initialized on first access in each thread (lazy init). Destruction occurs when the thread exits, in reverse order of construction.

```cpp
// WHY: Per-thread buffer avoids contention on a shared scratch buffer.
// Each audio worker thread gets its own 64KB scratch space.
thread_local alignas(64) char tls_scratch[64 * 1024];

// WHY: Wrapping in a function avoids the static-initialization-order fiasco
// across translation units. The thread_local is initialized on first call.
std::pmr::monotonic_buffer_resource& thread_scratch_resource() {
    thread_local std::pmr::monotonic_buffer_resource resource{
        tls_scratch, sizeof(tls_scratch)
    };
    return resource;
}
```

### Destructor ordering

`thread_local` destructors run when the thread exits, but the order across translation units is unspecified. If one TLS object depends on another, place them in the same TU or use a function-local `thread_local` to control initialization order.

```cpp
// WHY: Logger depends on scratch buffer. Function-local thread_local
// guarantees scratch is constructed before logger within each thread.
struct ThreadContext {
    std::pmr::monotonic_buffer_resource scratch{buf_, sizeof(buf_)};
    ThreadLocalLogger logger{&scratch};

private:
    alignas(64) char buf_[32 * 1024];
};

ThreadContext& thread_ctx() {
    // WHY: Single thread_local object controls destruction order.
    // scratch outlives logger because members destruct in reverse declaration order.
    thread_local ThreadContext ctx;
    return ctx;
}
```

### Last-error pattern for FFI

Thread-local error state is the standard pattern for rich error messages across `extern "C"` boundaries where only an integer status code can be returned.

```cpp
// WHY: C APIs return int status codes but callers often need a human-readable
// message. Thread-local storage avoids the need for caller-provided buffers
// or global error state that races across threads.
namespace {
thread_local std::string tls_last_error;
}

extern "C" int32_t engine_process(Engine* e, const float* in,
                                   float* out, size_t frames) noexcept {
    try {
        e->process(in, out, frames);
        return 0;
    } catch (const std::exception& ex) {
        tls_last_error = ex.what();
        return -1;
    }
}

extern "C" const char* engine_last_error() noexcept {
    // WHY: Returns pointer to thread_local string. Valid until the next
    // call to any engine_* function on the same thread.
    return tls_last_error.c_str();
}
```

### Per-thread accumulators

Accumulate statistics per-thread, then aggregate when needed. Eliminates atomic contention on every increment.

```cpp
struct ThreadStats {
    uint64_t samples_processed = 0;
    uint64_t xruns = 0;
    uint64_t alloc_bytes = 0;
};

// WHY: No atomics needed — each thread writes only its own instance.
thread_local ThreadStats tls_stats;

// Called from worker thread hot path: zero synchronization cost
void record_samples(uint64_t n) noexcept {
    tls_stats.samples_processed += n;
}

// WHY: Aggregation reads all threads' stats, but only at reporting boundaries
// (typically once per second). The brief lock contention here is acceptable
// because it is outside the hot path.
class StatsAggregator {
public:
    void register_thread() {
        std::scoped_lock lock{mu_};
        thread_stats_.push_back(&tls_stats);
    }

    ThreadStats aggregate() const {
        std::scoped_lock lock{mu_};
        ThreadStats total{};
        for (const auto* ts : thread_stats_) {
            total.samples_processed += ts->samples_processed;
            total.xruns += ts->xruns;
            total.alloc_bytes += ts->alloc_bytes;
        }
        return total;
    }

private:
    mutable std::mutex mu_;
    std::vector<ThreadStats*> thread_stats_;
};
```

### TLS and thread pools

`thread_local` destructors run when the thread exits, not when a task finishes. In thread pools (where threads are long-lived and recycled), this means:

1. **State leaks between tasks.** A `thread_local` accumulator from task A is still present when task B runs on the same thread. Reset TLS state explicitly at task boundaries if isolation is required.
2. **Destructors run late.** TLS resources are freed only when the pool thread is destroyed (typically at shutdown). For resources that should be freed earlier, use task-scoped RAII, not TLS.
3. **Registration lifetime.** If you register TLS pointers in an aggregator (as above), deregister them when the thread exits or the aggregator will hold dangling pointers.

```cpp
// WHY: RAII guard ensures the thread deregisters from the aggregator
// when the pool thread exits, preventing dangling pointer reads.
struct ThreadRegistration {
    explicit ThreadRegistration(StatsAggregator& agg) : agg_(agg) {
        agg_.register_thread();
    }
    ~ThreadRegistration() { agg_.deregister_thread(); }

    ThreadRegistration(const ThreadRegistration&) = delete;
    ThreadRegistration& operator=(const ThreadRegistration&) = delete;

private:
    StatsAggregator& agg_;
};

// In thread pool worker init:
thread_local ThreadRegistration reg{global_aggregator};
```

### Rules

1. **No TLS on the real-time audio thread.** First access triggers lazy initialization, which may allocate. Pre-allocate all audio-thread state at init time and pass it explicitly.
2. **Function-local `thread_local` over namespace-scope.** Avoids static-initialization-order issues and makes the initialization point explicit.
3. **Reset at task boundaries in thread pools.** TLS state persists across tasks on the same thread. If tasks require isolation, reset explicitly.
4. **Deregister on thread exit.** If TLS addresses are stored externally (aggregators, registries), use an RAII guard to deregister on destruction.
5. **Do not use TLS to avoid parameter passing.** If a function needs data, take it as a parameter. TLS is for cross-cutting concerns (error state, statistics, scratch buffers) where threading the parameter would be disproportionate.

---

## Memory safety

### `std::span` and bounds checking

```cpp
// Debug mode: enable bounds checking (pick one for your stdlib)
// libstdc++ (GCC):
target_compile_definitions(mylib PRIVATE $<$<CONFIG:Debug>:_GLIBCXX_ASSERTIONS>)

// libc++ (Clang): hardened mode:
target_compile_definitions(mylib PRIVATE
    $<$<CONFIG:Debug>:_LIBCPP_HARDENING_MODE=_LIBCPP_HARDENING_MODE_DEBUG>
    $<$<CONFIG:Release>:_LIBCPP_HARDENING_MODE=_LIBCPP_HARDENING_MODE_FAST>
)
```

`_LIBCPP_HARDENING_MODE_FAST` in production (~0-2% overhead) is the single highest-value memory safety measure after smart pointers. Traps on out-of-bounds and null deref.

### Static analysis: clang-tidy

Project `.clang-tidy` config enabling high-signal check groups: `bugprone-*` (use-after-move, dangling), `performance-*` (unnecessary copies), `concurrency-*` (thread safety), `modernize-*` (nullptr, override, range-for), `cert-*`, `cppcoreguidelines-*`. Promote critical checks to `WarningsAsErrors`.

Full implementation: `reference/clang-tidy.yml`

### GSL utilities

From `microsoft/GSL` (use selectively):
- `gsl::not_null<T*>`: pointer guaranteed non-null at construction
- `gsl::narrow<T>`: checked narrowing cast (throws if value doesn't fit)
- `gsl::finally`: scope guard for cleanup

---

## Testing

See TESTING.md for all testing principles (naming, isolation, coverage, test data, property testing).

C++-specific framework choices and tooling:

- **Framework:** GoogleTest or Catch2
- **Property tests:** `rapidcheck`
- **Fuzz targets:** libFuzzer for codec, parser, and deserialization code

### Sanitizer builds

| Build | Flags | Detects |
|-------|-------|---------|
| ASan + UBSan | `-fsanitize=address,undefined -fno-sanitize-recover=all -fno-omit-frame-pointer -O1` | Buffer overflow, use-after-free, leaks, signed overflow, null deref, misalignment |
| TSan | `-fsanitize=thread -O1 -g` | Data races (separate build: cannot combine with ASan) |
| MSan | `-fsanitize=memory -fsanitize-memory-track-origins=2` | Uninitialized reads (Clang-only, requires instrumented deps) |
| Coverage | `-fprofile-instr-generate -fcoverage-mapping -O0 -g` | Coverage report (separate from sanitizers) |

UBSan has ~5-20% overhead: enable in development builds always. `-fno-sanitize-recover=all` makes it abort on first UB instead of continuing (critical for CI).

### Sanitizer cMake integration

Use an `enable_sanitizers(target)` function that guards against ASan+TSan combination and applies flags via `target_compile_options` + `target_link_options`.

Full implementation: `reference/sanitizers.cmake`

### Fuzzing

```cpp
extern "C" int LLVMFuzzerTestOneInput(const uint8_t* data, size_t size) {
    MyCodec codec;
    codec.decode(std::span{data, size});
    return 0;
}
```

Compile: `clang++ -fsanitize=fuzzer,address,undefined -O1 -g fuzz_target.cpp -o fuzz_target`

CI runs fuzz targets for a fixed time (60s) to catch regressions. Dedicated fuzzing infrastructure runs continuously.

---

## Build system

### Target-Based cMake

Everything scoped to targets. No global `include_directories()`, `add_definitions()`, or `link_libraries()`.

```cmake
add_library(mylib src/foo.cpp src/bar.cpp)
target_include_directories(mylib
  PUBLIC  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
          $<INSTALL_INTERFACE:include>
  PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/src
)
target_compile_features(mylib PUBLIC cxx_std_23)
target_link_libraries(mylib PUBLIC fmt::fmt PRIVATE spdlog::spdlog)
```

### Dependency management

**FetchContent** for small/critical deps you need sanitizer-instrumented:

```cmake
include(FetchContent)
FetchContent_Declare(fmt
  GIT_REPOSITORY https://github.com/fmtlib/fmt.git
  GIT_TAG        11.1.4
  FIND_PACKAGE_ARGS
)
FetchContent_MakeAvailable(fmt)
```

**vcpkg** for the bulk of third-party deps with binary caching on CI. Manifest mode (`vcpkg.json`).

**Policy:** FetchContent for deps that must be compiled with your sanitizer/SIMD flags. vcpkg for everything else.

### Android NDK cross-Compilation

```cmake
# In CMakePresets.json
{
  "name": "android-arm64",
  "inherits": "default",
  "cacheVariables": {
    "CMAKE_SYSTEM_NAME": "Android",
    "CMAKE_ANDROID_ARCH_ABI": "arm64-v8a",
    "CMAKE_ANDROID_NDK": "$env{ANDROID_NDK_HOME}",
    "CMAKE_ANDROID_API": "26",
    "CMAKE_ANDROID_STL": "c++_shared"
  }
}
```

NDK r23+: CMake's built-in Android support (`CMAKE_SYSTEM_NAME=Android`) preferred over NDK's own toolchain file for CMake 3.21+.

---

## Dependencies

- Standard library first
- Header-only preferred for small utilities
- FetchContent or vcpkg over system-installed
- No Boost unless std lacks an equivalent

### Banned

| Pattern | Replacement |
|---------|-------------|
| Boost.Optional | `std::optional` |
| Boost.Expected | `std::expected` (C++23) |
| Boost.Variant | `std::variant` |
| `volatile` for synchronization | `std::atomic` |
| C-style casts | `static_cast`, `reinterpret_cast` (with `// SAFETY:`) |
| `#define` constants | `constexpr` |
| `printf` | `std::print` (C++23) |
| `std::thread` | `std::jthread` |

---

## Anti-Patterns

1. **Raw `new`/`delete`**: use smart pointers and RAII
2. **C-style casts**: use named casts with `// SAFETY:` on `reinterpret_cast`
3. **Macros for constants**: use `constexpr`
4. **`using namespace std` in headers**: pollutes every includer's namespace
5. **Exceptions in audio callback**: use `std::expected` or error codes; mark `noexcept`
6. **`std::shared_ptr` by default**: `unique_ptr` unless shared ownership is proven necessary
7. **Missing `const`**: const everything that doesn't mutate
8. **`std::thread` over `std::jthread`**: jthread auto-joins and supports `stop_token`
9. **Manual memory in FFI**: wrap in RAII immediately at the boundary
10. **Missing sanitizers in test builds**: ASan + UBSan minimum in CI
11. **`std::mutex::lock()` on audio thread**: causes priority inversion; use lock-free structures
12. **`std::vector`/`std::string` in audio callback**: may allocate; use pre-allocated buffers
13. **`std::function` in hot paths**: may heap-allocate for large captures
14. **`std::shared_ptr` in audio callback**: atomic refcount contention
15. **`volatile` for synchronization**: not a synchronization primitive; use `std::atomic`
16. **`memory_order_seq_cst` in SPSC queues**: `acquire`/`release` is sufficient and correct
17. **Modulo for ring buffer indexing**: use power-of-2 capacity with bitmask
18. **Shared cache line for producer/consumer indices**: false sharing kills throughput
19. **`compare_exchange_strong` in CAS loops**: `weak` is faster on ARM
20. **`std::condition_variable::wait()` without predicate**: spurious wakeups cause bugs
