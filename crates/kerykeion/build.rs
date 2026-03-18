//! Build script for kerykeion: compiles Meshtastic protobuf definitions via prost-build.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_dir = "proto";
    let protos = [
        "proto/meshtastic/portnums.proto",
        "proto/meshtastic/telemetry.proto",
        "proto/meshtastic/channel.proto",
        "proto/meshtastic/config.proto",
        "proto/meshtastic/module_config.proto",
        "proto/meshtastic/mesh.proto",
        "proto/meshtastic/admin.proto",
        "proto/meshtastic/storeforward.proto",
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
