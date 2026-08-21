//! Routing decisions over arbitrary decoded packets.
//!
//! Routing acts on packets the mesh supplies, so a hostile neighbour chooses the
//! hop counts, ports and payload shapes this sees. The decision must be total:
//! every packet that decodes produces a verdict rather than a panic.

#![no_main]

use kerykeion::processor::RoutingProcessor;
use kerykeion::proto::MeshPacket;
use libfuzzer_sys::fuzz_target;
use prost::Message as _;

fuzz_target!(|data: &[u8]| {
    // Only well-formed packets reach routing; malformed bytes are the
    // message_parse target's subject, and feeding them here would spend the
    // budget re-testing the parser instead of the decision.
    if let Ok(packet) = MeshPacket::decode(data) {
        let _verdict = RoutingProcessor::process_routing(&packet);
    }
});
