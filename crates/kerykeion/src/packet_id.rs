//! Monotonic outbound packet-id sequencing for AES-CTR nonce uniqueness (#209).
//!
//! [`crate::message::MessageBuilder::build`] feeds `packet_id` straight into
//! `crypto::build_nonce` as the AES-CTR nonce material for the sender's PSK
//! (`crypto.rs`'s module doc has the exact byte layout). A value drawn
//! independently per packet gives no non-repetition guarantee — two packets
//! sharing a `(packet_id, from)` pair XOR their plaintexts together under
//! CTR's keystream reuse. [`PacketIdCounter`] replaces that with a value
//! that only ever increases for the lifetime of one instance.

use rand_core::{OsRng, RngCore as _};

use crate::Error;
use crate::error::PacketIdSpaceExhaustedSnafu;

/// Monotonic sequence generator for outbound `packet_id` / AES-CTR nonce values.
///
/// # Persistence — read before wiring this into a long-running radio
///
/// WARNING: [`Self::seed_random`] draws a fresh random starting point and
/// carries no memory of ids a prior instance issued. Calling it on every
/// process start reproduces the exact defect this type exists to close: a
/// sequence that is monotonic *within one run* but restarts at an
/// independent random point on every reboot gives the same non-guarantee
/// the bare `OsRng.next_u32()`-per-packet call did (#209), just redrawn once
/// per process instead of once per packet. Call `seed_random` only when no
/// persisted value exists for this identity/PSK — typically first-ever
/// provisioning. On every subsequent start, persist [`Self::current`] after
/// each [`Self::next`] and reconstruct via [`Self::resume`] instead.
#[derive(Debug)]
pub struct PacketIdCounter {
    /// The most recently issued id, or the resume point if none has been
    /// issued by this instance yet. `None` only before the very first
    /// `seed_random`/`resume` call — the public constructors never leave it
    /// unset, so [`Self::current`] is total once a counter exists.
    last_issued: u32,
}

impl PacketIdCounter {
    /// Start a fresh sequence from a random 32-bit seed.
    ///
    /// WARNING: valid only when no persisted counter value exists for this
    /// identity/PSK — see the type-level docs.
    #[must_use]
    pub fn seed_random() -> Self {
        Self {
            last_issued: OsRng.next_u32(),
        }
    }

    /// Resume a sequence from the last id a prior instance issued, as
    /// returned by [`Self::current`] and persisted by the caller (e.g. on
    /// process shutdown, or after every send if crash-safety across an
    /// unclean exit matters more than the write cost).
    ///
    /// The first subsequent [`Self::next`] call returns `last_used + 1`,
    /// never `last_used` itself or a fresh random value — this is what
    /// prevents a restart from repeating a nonce this key has already used.
    #[must_use]
    pub const fn resume(last_used: u32) -> Self {
        Self {
            last_issued: last_used,
        }
    }

    /// The most recently issued id. Callers persist this after every
    /// [`Self::next`] and pass it to [`Self::resume`] on the next start.
    #[must_use]
    pub const fn current(&self) -> u32 {
        self.last_issued
    }

    /// Issue the next id in the sequence.
    ///
    /// # Errors
    ///
    /// Returns [`Error::PacketIdSpaceExhausted`] instead of wrapping past
    /// `u32::MAX` back toward `0`. A silent wrap would revisit a value this
    /// instance (or, via `resume`, a prior one) may already have issued
    /// under the same key — reproducing #209 at the 32-bit boundary instead
    /// of the birthday bound the raw-random design failed at. Exhaustion
    /// means the PSK must rotate; retrying `next` cannot recover.
    pub fn next(&mut self) -> Result<u32, Error> {
        let next = self
            .last_issued
            .checked_add(1)
            .ok_or_else(|| PacketIdSpaceExhaustedSnafu.build())?;
        self.last_issued = next;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn next_is_strictly_increasing_and_never_repeats() {
        let mut counter = PacketIdCounter::resume(0);
        let mut seen = HashSet::new();
        let mut prev = 0u32;
        for i in 0..10_000u32 {
            #[expect(clippy::unwrap_used, reason = "test-only: far from u32::MAX")]
            let id = counter.next().unwrap();
            assert!(seen.insert(id), "packet id {id} repeated at iteration {i}");
            if i > 0 {
                assert_eq!(id, prev + 1, "counter must advance by exactly 1 per call");
            }
            prev = id;
        }
    }

    #[test]
    fn resume_continues_from_the_persisted_value_not_from_zero() {
        // WHY(#209): this is the restart-repro case named in the issue — a
        // counter that forgets what it already issued and restarts at zero
        // (or anywhere below the persisted point) can reissue an id, and
        // hence a nonce, this key has already used.
        let persisted_last_used = 4_242u32;
        let mut resumed = PacketIdCounter::resume(persisted_last_used);
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let first_after_restart = resumed.next().unwrap();
        assert_eq!(
            first_after_restart,
            persisted_last_used + 1,
            "resume must continue past the persisted value, never reissue it or reset"
        );
        assert_ne!(
            first_after_restart, 0,
            "resume must not behave like a fresh/unpersisted seed"
        );
    }

    #[test]
    fn next_refuses_to_wrap_past_u32_max() {
        // WHY(#209): the other failure mode named in the issue. Wrapping
        // silently back to 0 would reissue the lowest ids this instance
        // already used under the same key.
        let mut counter = PacketIdCounter::resume(u32::MAX - 1);
        #[expect(clippy::unwrap_used, reason = "test-only: one below the ceiling")]
        let one_below_ceiling = counter.next().unwrap();
        assert_eq!(one_below_ceiling, u32::MAX);
        assert!(
            matches!(counter.next(), Err(Error::PacketIdSpaceExhausted { .. })),
            "next() must refuse rather than wrap to a low, already-issued value"
        );
    }

    #[test]
    fn seed_random_two_instances_start_from_different_points() {
        // WHY: not a strong cryptographic proof (a collision is astronomically
        // unlikely, not impossible) — a weak smoke check that seed_random is
        // actually drawing from the RNG rather than a fixed constant.
        let a = PacketIdCounter::seed_random();
        let b = PacketIdCounter::seed_random();
        assert_ne!(a.current(), b.current());
    }
}
