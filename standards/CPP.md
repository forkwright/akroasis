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

### CMake Presets

Standardize build configurations in `CMakePresets.json` (checked in) with `CMakeUserPresets.json` (gitignored) for local overrides. Presets should cover dev (debug + sanitizers), release (LTO), and tsan (separate thread sanitizer build).

Full implementation: `reference/cmake-presets.json`

---

## Naming

| Element | Convention | Example |
|---------|-----------|---------|
| Files | `snake_case.cpp` / `snake_case.hpp` | `audio_processor.cpp` |
| Classes / Structs | `PascalCase` | `AudioProcessor`, `BufferPool` |
| Functions / Methods | `snake_case` | `process_buffer`, `load_config` |
| Variables | `snake_case` | `sample_rate`, `buffer_size` |
| Constants | `UPPER_SNAKE_CASE` | `MAX_CHANNELS`, `DEFAULT_RATE` |
| Namespaces | `snake_case` | `akroasis::audio` |
| Macros | `UPPER_SNAKE_CASE` (avoid macros) | `AKROASIS_ASSERT` |
| Template parameters | `PascalCase` | `typename Allocator` |
| Private members | `snake_case_` (trailing underscore) | `buffer_size_`, `sample_rate_` |

---

## C++23 Adoption

### Adopt Now

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

### Track for C++26

| Feature | Status | Why it matters |
|---------|--------|---------------|
| `std::execution` (senders/receivers) | In C++26 | Structured async — replaces ad-hoc thread pools |
| Compile-time reflection | In C++26 | Serialization, enum-to-string, introspection without macros |
| Contracts | In C++26 | `pre`, `post`, `contract_assert` — replaces `assert()` macros |
| `std::hazard_pointer` | In C++26 | Safe memory reclamation for lock-free structures |
| `std::inplace_vector<T, N>` | In C++26 | Fixed-capacity, no-allocation vector — ideal for audio |
| Pattern matching | Not in C++26 | Still in design; track P2688 |
| C++20 modules | Experimental in CMake 3.28+ | Use for new internal code; keep headers for external APIs |

### `std::expected` Patterns

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

## Type System

### RAII Everywhere

Resources managed by constructors and destructors. No manual `new`/`delete`. No raw owning pointers.

```cpp
// Right
auto processor = std::make_unique<AudioProcessor>(config);

// Wrong
auto* processor = new AudioProcessor(config);
```

### Smart Pointer Rules

| Type | Use |
|------|-----|
| `std::unique_ptr<T>` | Single ownership (default) |
| `std::shared_ptr<T>` | Shared ownership (justify in comment why unique isn't enough) |
| `std::weak_ptr<T>` | Non-owning observer of shared |
| `T*` / `T&` | Non-owning access (never ownership transfer) |

### `const` by Default

- `const` on everything that doesn't need mutation
- `constexpr` for compile-time computation
- `consteval` for functions that must run at compile time
- Parameters by `const&` unless trivially copyable

### `std::span` for Non-Owning Views

Replaces the `(T* ptr, size_t count)` anti-pattern:

```cpp
// Before — no bounds information
void process(const float* data, size_t len);

// After — non-owning view with known extent
void process(std::span<const float> data);
```

### `std::string_view` Rules

Non-owning reference to string data. Dangling is the primary risk.

- Use for function parameters (replaces `const std::string&` when null-termination isn't needed)
- Never return `string_view` to local data
- Never store as a class member unless lifetime is guaranteed by design
- When in doubt, use `std::string` for owned data

### `std::variant` Over Unions

```cpp
// Right
using AudioFormat = std::variant<PcmFormat, FlacFormat, OpusFormat>;

// Wrong
union AudioFormat { PcmFormat pcm; FlacFormat flac; };
```

Visit with `std::visit` and explicit variant handling — no wildcard visitors for variants under your control.

---

## Error Handling

### No Exceptions in Hot Paths

`std::expected<T, E>` for audio processing and performance-critical code. Exceptions acceptable in initialization, configuration, and cold paths.

### Error Enum Per Module

```cpp
enum class AudioError {
    InvalidFormat,
    BufferTooSmall,
    DeviceUnavailable,
};

std::expected<AudioBuffer, AudioError> decode(std::span<const uint8_t> data);
```

Callers can match on error kind, not parse strings.

### Exception Boundaries

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

### SAFETY Comments

Same structured comment tags as STANDARDS.md. `// SAFETY:` before any:
- Raw pointer dereference
- `reinterpret_cast`
- Inline assembly
- C library calls with manual lifetime management
- Any `extern "C"` boundary function

---

## Concurrency

### Thread Safety Defaults

- `std::scoped_lock` for multi-mutex locking (deadlock-free)
- `std::jthread` over `std::thread` (auto-joins, cooperative cancellation via `stop_token`)
- `std::shared_mutex` for read-heavy, write-rare data (config, presets)
- Never hold locks across I/O or long operations

### `std::atomic` and Memory Ordering

Default to `memory_order_seq_cst`. Relax only with a documented proof of correctness.

| Order | Use |
|-------|-----|
| `seq_cst` | Default. Full ordering. |
| `acquire` | On loads — "I'm reading data someone published" |
| `release` | On stores — "I'm publishing data for others" |
| `acq_rel` | Read-modify-write ops (CAS, fetch_add) |
| `relaxed` | Standalone counters, statistics where ordering doesn't matter |
| `consume` | Deprecated in practice — compilers promote to acquire |

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

### `std::jthread` and Cooperative Cancellation

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
// Destructor calls request_stop() then join() — no manual shutdown
```

### Lock-Free Where Mandatory

Lock-free is mandatory for the audio thread (cannot tolerate priority inversion). Default to mutexes everywhere else — lock-free is not faster in general, it's lower-latency under contention.

### False Sharing Prevention

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

### Condition Variables

Every `wait()` must use a predicate — spurious wakeups are real:

```cpp
// Right
cv.wait(lock, [&] { return !queue.empty(); });

// Wrong — may return with queue still empty
cv.wait(lock);
```

Never wait on a condition variable on the real-time audio thread. Audio thread polls lock-free queues only.

---

## Audio Processing

### The Real-Time Contract

The audio callback runs on a deadline-driven thread (typically 1–10ms budget). Violations cause audible glitches. Every function reachable from the audio callback must be bounded-time and non-blocking.

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

### Audio Callback Signature

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

### SPSC Ring Buffer

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
- Monotonically increasing indices (never wrap — mask on access)
- Cache-line separation for head/tail (prevents false sharing)
- `trivially_copyable` constraint (memcpy-safe in buffer)

### Inter-Thread Communication Patterns

| Pattern | Use case | Data loss? |
|---------|----------|------------|
| SPSC queue | Event streams (MIDI, commands) | No (bounded backpressure) |
| Double buffer | Latest state snapshot | Yes (latest wins) |
| Triple buffer | Latest state, non-blocking both sides | Yes (latest wins) |
| SeqLock | Small read-heavy config | No (retry on conflict) |
| `std::atomic<T>` | Single values (gain, flag) | N/A |

### SeqLock for Parameter Updates

Single writer (UI thread), multiple readers (audio thread). Writer increments a sequence counter (odd = writing), reader retries on torn read — wait-free in the common case. Use for small, frequently-read parameter structs.

Full implementation: `reference/seqlock.hpp`

### Buffer Management

Pre-allocate everything at init time. Lock-free checkout/return via SPSC queues. If the pool is exhausted, output silence — never block.

Full implementation: `reference/audio-buffer-pool.hpp`

### Memory Allocation for Audio

- All audio-thread memory page-locked with `mlock()` / `mlockall()` (prevents page faults)
- Arena allocators reset per callback for scratch memory
- `std::pmr::monotonic_buffer_resource` backed by pre-allocated buffer for zero-allocation STL usage
- `alignas(32)` or `alignas(64)` on all audio buffers (SIMD alignment)

```cpp
alignas(64) static char audio_arena[1024 * 1024];
std::pmr::monotonic_buffer_resource audio_resource(audio_arena, sizeof(audio_arena));

// In audio callback — allocations come from pre-allocated buffer
std::pmr::vector<float> temp(&audio_resource);
temp.resize(512);  // no malloc
```

### Sample Format Handling

Internal processing always `float` (32-bit). Integer formats only at I/O boundaries.

```cpp
inline float int16_to_float(int16_t s) noexcept {
    return static_cast<float>(s) / 32768.0f;
}

inline int16_t float_to_int16(float f) noexcept {
    return static_cast<int16_t>(std::clamp(f, -1.0f, 1.0f) * 32767.0f);
}
```

Use deinterleaved (planar) buffers for processing — each channel contiguous for SIMD. Interleave/deinterleave at I/O boundaries.

### SIMD

Use `__restrict__` on all buffer pointer parameters to enable auto-vectorization:

```cpp
void mix(float* __restrict__ dst, const float* __restrict__ src,
         float gain, int n) noexcept {
    for (int i = 0; i < n; ++i)
        dst[i] += src[i] * gain;
}
```

Compiler flags: `-O2` minimum, `-march=native` for build machine, `-ffast-math` acceptable for audio (exact IEEE compliance less important than throughput). Use `-Rpass=loop-vectorize` (Clang) to verify vectorization.

For explicit SIMD, use intrinsics with compile-time dispatch (`__SSE2__`, `__AVX__`, `__AVX512F__`). `std::experimental::simd` (Parallelism TS) is not yet standardized — track but don't adopt.

### Thread Priority

Set real-time priority on the audio thread:
- **Linux:** `SCHED_FIFO` via `pthread_setschedparam` (requires `CAP_SYS_NICE` or `rtprio` in limits.conf)
- **macOS:** `THREAD_TIME_CONSTRAINT_POLICY` via `thread_policy_set`
- **Windows:** MMCSS `AvSetMmThreadCharacteristicsW(L"Pro Audio", &taskIndex)`

Pin audio thread to a performance core for cache locality (`pthread_setaffinity_np` on Linux).

---

## Rust FFI

### Boundary Rules

- Only POD types cross the boundary: integers, floats, raw pointers, `#[repr(C)]` structs
- No exceptions across `extern "C"` — catch everything on C++ side
- No C++ types (`std::string`, `std::vector`, `std::unique_ptr`) in `extern "C"` signatures
- Return integer status codes or `std::expected`-style enums for errors
- Document ownership transfer in function names: `_create`/`_destroy`, `_take`/`_borrow`

### Opaque Pointer Pattern

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
// C++ implementation — catch exceptions at the boundary
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

Rust side wraps in RAII immediately — see Rust standards for the `Drop` guard pattern.

### Error Passing

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

### Data Passing

- **Slices:** pointer + length pair. Use `std::span` internally, decompose at the `extern "C"` boundary.
- **Strings:** pointer + length (preferred) or null-terminated `const char*`. Validate UTF-8 on Rust side.
- **Callbacks:** `extern "C"` function pointer + `void* user_data` context.

### `cxx` vs Raw FFI

| Use `cxx` when | Use raw FFI when |
|-----------------|-------------------|
| You control both sides | Pre-existing C API |
| Bidirectional calls | One-directional, simple signatures |
| Want type safety at bridge | Need `#[no_mangle]` exports for C consumer |
| Passing C++ types (string, vector, unique_ptr) | Minimal dependencies |

### Build Integration

- **Rust is primary:** `cc` crate or `cxx-build` in `build.rs` to compile C++ sources
- **CMake is primary:** `corrosion` to import Rust crates as CMake targets
- **Linking:** `cc` crate handles `stdc++`/`c++` linking automatically. For manual: `cargo:rustc-link-lib=dylib=stdc++`

---

## Memory Safety

### `std::span` and Bounds Checking

```cpp
// Debug mode — enable bounds checking (pick one for your stdlib)
// libstdc++ (GCC):
target_compile_definitions(mylib PRIVATE $<$<CONFIG:Debug>:_GLIBCXX_ASSERTIONS>)

// libc++ (Clang) — hardened mode:
target_compile_definitions(mylib PRIVATE
    $<$<CONFIG:Debug>:_LIBCPP_HARDENING_MODE=_LIBCPP_HARDENING_MODE_DEBUG>
    $<$<CONFIG:Release>:_LIBCPP_HARDENING_MODE=_LIBCPP_HARDENING_MODE_FAST>
)
```

`_LIBCPP_HARDENING_MODE_FAST` in production (~0-2% overhead) is the single highest-value memory safety measure after smart pointers. Traps on out-of-bounds and null deref.

### Static Analysis: clang-tidy

Project `.clang-tidy` config enabling high-signal check groups: `bugprone-*` (use-after-move, dangling), `performance-*` (unnecessary copies), `concurrency-*` (thread safety), `modernize-*` (nullptr, override, range-for), `cert-*`, `cppcoreguidelines-*`. Promote critical checks to `WarningsAsErrors`.

Full implementation: `reference/clang-tidy.yml`

### GSL Utilities

From `microsoft/GSL` — use selectively:
- `gsl::not_null<T*>` — pointer guaranteed non-null at construction
- `gsl::narrow<T>` — checked narrowing cast (throws if value doesn't fit)
- `gsl::finally` — scope guard for cleanup

---

## Testing

### Framework and Structure

- **Framework:** GoogleTest or Catch2
- **Names:** `TEST(AudioProcessor, ReturnsEmptyWhenNoInput)`, not `Test1`
- **Property tests:** `rapidcheck` for round-trip, algebraic, and invariant properties
- **Fuzz targets:** libFuzzer for codec, parser, and deserialization code

### Sanitizer Builds

| Build | Flags | Detects |
|-------|-------|---------|
| ASan + UBSan | `-fsanitize=address,undefined -fno-sanitize-recover=all -fno-omit-frame-pointer -O1` | Buffer overflow, use-after-free, leaks, signed overflow, null deref, misalignment |
| TSan | `-fsanitize=thread -O1 -g` | Data races (separate build — cannot combine with ASan) |
| MSan | `-fsanitize=memory -fsanitize-memory-track-origins=2` | Uninitialized reads (Clang-only, requires instrumented deps) |
| Coverage | `-fprofile-instr-generate -fcoverage-mapping -O0 -g` | Coverage report (separate from sanitizers) |

UBSan has ~5-20% overhead — enable in development builds always. `-fno-sanitize-recover=all` makes it abort on first UB instead of continuing (critical for CI).

### Sanitizer CMake Integration

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

## Build System

### Target-Based CMake

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

### Dependency Management

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

### Android NDK Cross-Compilation

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

1. **Raw `new`/`delete`** — use smart pointers and RAII
2. **C-style casts** — use named casts with `// SAFETY:` on `reinterpret_cast`
3. **Macros for constants** — use `constexpr`
4. **`using namespace std` in headers** — pollutes every includer's namespace
5. **Exceptions in audio callback** — use `std::expected` or error codes; mark `noexcept`
6. **`std::shared_ptr` by default** — `unique_ptr` unless shared ownership is proven necessary
7. **Missing `const`** — const everything that doesn't mutate
8. **`std::thread` over `std::jthread`** — jthread auto-joins and supports `stop_token`
9. **Manual memory in FFI** — wrap in RAII immediately at the boundary
10. **Missing sanitizers in test builds** — ASan + UBSan minimum in CI
11. **`std::mutex::lock()` on audio thread** — causes priority inversion; use lock-free structures
12. **`std::vector`/`std::string` in audio callback** — may allocate; use pre-allocated buffers
13. **`std::function` in hot paths** — may heap-allocate for large captures
14. **`std::shared_ptr` in audio callback** — atomic refcount contention
15. **`volatile` for synchronization** — not a synchronization primitive; use `std::atomic`
16. **`memory_order_seq_cst` in SPSC queues** — `acquire`/`release` is sufficient and correct
17. **Modulo for ring buffer indexing** — use power-of-2 capacity with bitmask
18. **Shared cache line for producer/consumer indices** — false sharing kills throughput
19. **`compare_exchange_strong` in CAS loops** — `weak` is faster on ARM
20. **`std::condition_variable::wait()` without predicate** — spurious wakeups cause bugs
