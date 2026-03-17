//! CLI definition — top-level subcommand tree.

use clap::{Parser, Subcommand};

use crate::radio::RadioCommand;
use crate::vault::VaultCommand;

#[allow(clippy::doc_markdown)]
#[derive(Parser)]
#[command(name = "akroasis", version, about = "ἀκρόασις — attentive reception")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Subcommand)]
pub enum Command {
    /// Radio management — frequency plans, programming, vehicle telemetry
    Radio(RadioArgs),
    /// Mesh networking — Meshtastic, topology, DTN, PACE communications
    Mesh,
    /// SDR reception — spectrum, demodulation, EW detection
    Sdr,
    /// Proximity — `WiFi`, BLE, Zigbee, NFC/RFID monitoring
    Proximity,
    /// Network defense — IDS/IPS, CAN bus, `IoT` security
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
    /// Credential vault — store, retrieve, rotate, and revoke secrets
    Vault(VaultArgs),
    /// Privacy — VPN, anonymization, OPSEC assessment
    Privacy,
    /// Serve the Akroasis daemon
    Serve,
}

/// Radio subcommand arguments.
#[derive(clap::Args)]
pub struct RadioArgs {
    #[command(subcommand)]
    pub command: RadioCommand,
}

/// Vault subcommand arguments.
#[derive(clap::Args)]
pub struct VaultArgs {
    #[command(subcommand)]
    pub command: VaultCommand,
}
