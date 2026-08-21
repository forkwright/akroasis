//! Protobuf message parsing against arbitrary bytes.
//!
//! Both directions of the session protocol are decoded from bytes that arrived
//! over the radio. Decoding must fail cleanly rather than panic, and anything
//! that decodes must survive being re-encoded.

#![no_main]

use kerykeion::proto::{FromRadio, ToRadio};
use libfuzzer_sys::fuzz_target;
use prost::Message as _;

fuzz_target!(|data: &[u8]| {
    if let Ok(message) = FromRadio::decode(data) {
        // A message the parser accepted must be representable again, and
        // encoding must reach a fixed point: a second pass has to produce the
        // same bytes as the first. A parser that admits more than the type
        // models shows up here as a field that survives one round and not two.
        //
        // WHY compare bytes rather than the decoded values: these messages carry
        // f32 fields, and `NaN != NaN`. Comparing values reports a byte-perfect
        // round trip of a NaN as a failure — which this target did, on
        // `NodeInfo.snr` (`22 05 25 d4 8d ff ff`, kept as a corpus seed), within
        // a minute of first running. Bytes are the property that was meant.
        let once = message.encode_to_vec();
        let again = FromRadio::decode(once.as_slice()).map(|m| m.encode_to_vec());
        assert_eq!(
            again.ok().as_deref(),
            Some(once.as_slice()),
            "a message this parser produced must decode again, to the same bytes"
        );
    }

    // The outbound direction is parsed from untrusted input too, in the gateway
    // and replay paths. No round-trip assertion here: only the inbound type is
    // reconstructed from the wire in normal operation, so the point is that
    // decoding neither panics nor hangs. `drop` rather than `let _ =` because
    // the discard is the intent, and a silent `let _` on a Result reads as an
    // oversight wherever it appears.
    drop(ToRadio::decode(data));
});
