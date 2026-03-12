//! ἀκρόασις — attentive reception
//!
//! RF intelligence, mesh networking, and communications sovereignty.
//! Binary entrypoint for the Akroasis platform.

use clap::Parser;

#[derive(Parser)]
#[command(name = "akroasis", version, about = "ἀκρόασις — attentive reception")]
enum Cli {
    /// Radio management — programming, frequency plans, profiles
    Radio,
    /// Mesh networking — Meshtastic node management, topology, messaging
    Mesh,
    /// SDR reception — spectrum monitoring, signal analysis, demodulation
    Sdr,
    /// Signal intelligence — protocol decoding, activity monitoring, alerting
    Intel,
    /// Communications — encrypted messaging, email, protocol bridges
    Comms,
}

fn main() {
    let cli = Cli::parse();

    match cli {
        Cli::Radio => println!("syntonia — radio management (not yet implemented)"),
        Cli::Mesh => println!("kerykeion — mesh networking (not yet implemented)"),
        Cli::Sdr => println!("dektis — SDR reception (not yet implemented)"),
        Cli::Intel => println!("semaino — signal intelligence (not yet implemented)"),
        Cli::Comms => println!("kryphos — communications (not yet implemented)"),
    }
}
