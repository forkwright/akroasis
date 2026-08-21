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
        // A message the parser accepted must be representable again. A decode
        // that produces a value the encoder cannot handle is a parser that
        // admits more than the type models.
        let re_encoded = message.encode_to_vec();
        let round_tripped = FromRadio::decode(re_encoded.as_slice())
            .expect("a message this parser produced must decode again");
        assert_eq!(
            message, round_tripped,
            "re-encoding an accepted message must be lossless"
        );
    }

    // The outbound direction is parsed from untrusted input too, in the gateway
    // and replay paths. No round-trip assertion here: only the inbound type is
    // reconstructed from the wire in normal operation.
    let _ = ToRadio::decode(data);
});
