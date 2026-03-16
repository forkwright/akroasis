// WHY: Generates Rust types from vendored Meshtastic .proto files at build time.
// Pinned to Meshtastic protobufs v2.7.20.
#![allow(missing_docs)]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = prost_build::Config::new();

    config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");

    // WHY: Prost already derives Hash + Eq on enums via ::prost::Enumeration.
    // Only add serde derives via the blanket "." attribute above.

    let protos = &[
        "proto/meshtastic/mesh.proto",
        "proto/meshtastic/portnums.proto",
        "proto/meshtastic/config.proto",
        "proto/meshtastic/module_config.proto",
        "proto/meshtastic/channel.proto",
        "proto/meshtastic/admin.proto",
        "proto/meshtastic/telemetry.proto",
        "proto/meshtastic/storeforward.proto",
    ];

    config.compile_protos(protos, &["proto/"])?;

    println!("cargo:rerun-if-changed=proto/meshtastic");
    Ok(())
}
