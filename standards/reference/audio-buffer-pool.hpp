// Reference implementation for lock-free audio buffer pool.
// Standards: CPP.md > Audio Processing > Buffer Management

#pragma once

#include <cstddef>
#include <vector>

// Assumes SpscQueue is available (see spsc-queue.hpp).
// Assumes AudioBuffer has a constructor(channels, frames) and a clear() method.

class AudioBufferPool {
    SpscQueue<AudioBuffer*, 64> free_list_;
    std::vector<AudioBuffer> storage_;

public:
    AudioBufferPool(int pool_size, int channels, int frames)
        : storage_(pool_size, AudioBuffer(channels, frames)) {
        for (auto& buf : storage_)
            free_list_.try_push(&buf);
    }

    AudioBuffer* acquire() noexcept {
        AudioBuffer* buf = nullptr;
        free_list_.try_pop(buf);
        return buf;  // nullptr if exhausted — output silence, never block
    }

    void release(AudioBuffer* buf) noexcept {
        buf->clear();
        free_list_.try_push(buf);
    }
};
