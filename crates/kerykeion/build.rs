//! Build script for kerykeion: compiles Meshtastic protobuf definitions via prost-build.

fn main() -> std::io::Result<()> {
    let proto_dir = "proto";
    // Grouped by what this crate DOES with each message rather than by the wire protocol's own
    // subsystem split, which is why there are six files here and not nine. Four of the previous
    // files had no consumer in this crate at all and are gone rather than compiled and unused.
    let protos = [
        "proto/packet.proto",
        "proto/node_inventory.proto",
        "proto/routing_diagnostics.proto",
        "proto/session.proto",
        "proto/admin.proto",
        "proto/mqtt_gateway.proto",
    ];

    prost_build::Config::new()
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .compile_protos(&protos, &[proto_dir])?;

    for proto in &protos {
        println!("cargo:rerun-if-changed={proto}");
    }
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}
