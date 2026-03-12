//! ἀκρόασις — attentive reception
//!
//! RF intelligence, communications sovereignty, and operational awareness.
//! 17 crates. 10 capability domains. One shared signal model.

use clap::Parser;

#[derive(Parser)]
#[command(name = "akroasis", version, about = "ἀκρόασις — attentive reception")]
enum Cli {
    /// Radio management — frequency plans, programming, vehicle telemetry
    Radio,
    /// Mesh networking — Meshtastic, topology, DTN, PACE communications
    Mesh,
    /// SDR reception — spectrum, demodulation, EW detection
    Sdr,
    /// Proximity — WiFi, BLE, Zigbee, NFC/RFID monitoring
    Proximity,
    /// Network defense — IDS/IPS, CAN bus, IoT security
    Shield,
    /// OSINT — feeds, recon, asset discovery, threat intel
    Watch,
    /// Offensive security — pentesting, vulnerability assessment
    Test,
    /// Signal intelligence — aggregation, correlation, focal points
    Intel,
    /// Automation — playbooks, PACE, state machines, triggers
    Auto,
    /// Navigation — vehicle/foot routing, military planning, maps
    Nav,
    /// Knowledge — offline references, frequency databases, manuals
    Know,
    /// Communications — encrypted messaging, key management
    Comms,
    /// Privacy — VPN, anonymization, OPSEC assessment
    Privacy,
    /// Serve the Akroasis daemon
    Serve,
}

fn main() {
    let cli = Cli::parse();

    match cli {
        Cli::Radio => println!("syntonia — radio management (not yet implemented)"),
        Cli::Mesh => println!("kerykeion — mesh networking (not yet implemented)"),
        Cli::Sdr => println!("dektis — SDR reception (not yet implemented)"),
        Cli::Proximity => println!("engys — proximity intelligence (not yet implemented)"),
        Cli::Shield => println!("aspis — network defense (not yet implemented)"),
        Cli::Watch => println!("skopos — OSINT collection (not yet implemented)"),
        Cli::Test => println!("peira — offensive security (not yet implemented)"),
        Cli::Intel => println!("semaino + ichneutes — intelligence (not yet implemented)"),
        Cli::Auto => println!("praxis — automation (not yet implemented)"),
        Cli::Nav => println!("chorografia — navigation (not yet implemented)"),
        Cli::Know => println!("pinax — knowledge repository (not yet implemented)"),
        Cli::Comms => println!("kryphos — communications (not yet implemented)"),
        Cli::Privacy => println!("lethe — privacy (not yet implemented)"),
        Cli::Serve => println!("daemon mode (not yet implemented)"),
    }
}
