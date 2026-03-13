// Reference implementation for SPSC lock-free queue.
// Standards: CPP.md > Audio Processing > SPSC Ring Buffer

#pragma once

#include <array>
#include <atomic>
#include <cstddef>
#include <type_traits>

#ifdef __cpp_lib_hardware_interference_size
    inline constexpr size_t CACHE_LINE = std::hardware_destructive_interference_size;
#else
    inline constexpr size_t CACHE_LINE = 64;
#endif

template <typename T, size_t Capacity>
class SpscQueue {
    static_assert((Capacity & (Capacity - 1)) == 0, "Capacity must be power of two");
    static_assert(std::is_trivially_copyable_v<T>);

    static constexpr size_t MASK = Capacity - 1;

    alignas(CACHE_LINE) std::atomic<size_t> head_{0};  // consumer writes
    alignas(CACHE_LINE) std::atomic<size_t> tail_{0};  // producer writes
    alignas(CACHE_LINE) std::array<T, Capacity> buffer_;

public:
    bool try_push(const T& item) noexcept {
        const size_t t = tail_.load(std::memory_order_relaxed);
        const size_t h = head_.load(std::memory_order_acquire);
        if (t - h >= Capacity) return false;
        buffer_[t & MASK] = item;
        tail_.store(t + 1, std::memory_order_release);
        return true;
    }

    bool try_pop(T& item) noexcept {
        const size_t h = head_.load(std::memory_order_relaxed);
        const size_t t = tail_.load(std::memory_order_acquire);
        if (h == t) return false;
        item = buffer_[h & MASK];
        head_.store(h + 1, std::memory_order_release);
        return true;
    }
};
