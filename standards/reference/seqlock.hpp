// Reference implementation for SeqLock pattern.
// Standards: CPP.md > Audio Processing > SeqLock for Parameter Updates

#pragma once

#include <atomic>
#include <cstdint>

// AudioParams is a placeholder — replace with your actual parameter struct.
struct AudioParams {
    float gain;
    float pan;
    // Add fields as needed. Must be trivially copyable.
};

class SeqLock {
    std::atomic<uint32_t> seq_{0};
    AudioParams data_;

public:
    void write(const AudioParams& p) {
        seq_.store(seq_.load(std::memory_order_relaxed) + 1, std::memory_order_release);
        data_ = p;
        std::atomic_thread_fence(std::memory_order_release);
        seq_.store(seq_.load(std::memory_order_relaxed) + 1, std::memory_order_release);
    }

    AudioParams read() const noexcept {
        AudioParams result;
        uint32_t s;
        do {
            s = seq_.load(std::memory_order_acquire);
            if (s & 1) continue;
            result = data_;
            std::atomic_thread_fence(std::memory_order_acquire);
        } while (seq_.load(std::memory_order_acquire) != s);
        return result;
    }
};
