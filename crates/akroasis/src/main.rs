//! ἀκρόασις — attentive reception
//!
//! RF intelligence, communications sovereignty, and operational awareness.
//! 17 crates. 10 capability domains. One shared signal model.

mod cli;
mod radio;

use std::path::PathBuf;

use clap::Parser;
use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::Deserialize;
use snafu::{ResultExt, Snafu};

use cli::{Cli, Command};

/// Top-level application errors.
#[derive(Debug, Snafu)]
enum Error {
    /// Failed to load configuration.
    #[snafu(display("configuration error: {source}"))]
    Config {
        /// Boxed to keep the error variant small.
        #[snafu(source(from(figment::Error, Box::new)))]
        source: Box<figment::Error>,
    },

    /// A radio operation failed.
    #[snafu(display("{source}"))]
    Radio { source: radio::errors::RadioError },
}

/// Application configuration loaded from TOML file and environment overrides.
#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct Config {
    /// Path to the configuration file (default: `~/.config/akroasis/config.toml`).
    config_path: Option<PathBuf>,
}

fn default_config_path() -> PathBuf {
    std::env::var("HOME")
        .map_or_else(|_| PathBuf::from("."), PathBuf::from)
        .join(".config/akroasis/config.toml")
}

fn load_config() -> Result<Config, Error> {
    Figment::new()
        .join(Toml::file(default_config_path()))
        .join(Env::prefixed("AKROASIS_"))
        .extract()
        .context(ConfigSnafu)
}

fn dispatch(command: &Command) -> Result<(), Error> {
    match command {
        Command::Radio(args) => {
            radio::dispatch(&args.command).context(RadioSnafu)?;
        }
        Command::Mesh => println!("kerykeion — mesh networking (not yet implemented)"),
        Command::Sdr => println!("dektis — SDR reception (not yet implemented)"),
        Command::Proximity => println!("engys — proximity intelligence (not yet implemented)"),
        Command::Shield => println!("aspis — network defense (not yet implemented)"),
        Command::Watch => println!("skopos — OSINT collection (not yet implemented)"),
        Command::Test => println!("peira — offensive security (not yet implemented)"),
        Command::Intel => {
            println!("semaino + ichneutes — intelligence (not yet implemented)");
        }
        Command::Auto => println!("praxis — automation (not yet implemented)"),
        Command::Nav => println!("chorografia — navigation (not yet implemented)"),
        Command::Know => println!("pinax — knowledge repository (not yet implemented)"),
        Command::Comms => println!("kryphos — communications (not yet implemented)"),
        Command::Privacy => println!("lethe — privacy (not yet implemented)"),
        Command::Serve => println!("daemon mode (not yet implemented)"),
    }
    Ok(())
}

fn run() -> Result<(), Error> {
    let _config = load_config()?;
    let cli = Cli::parse();
    dispatch(&cli.command)
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("AKROASIS_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
