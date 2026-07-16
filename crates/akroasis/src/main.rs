//! ἀκρόασις — attentive reception
//!
//! RF intelligence, communications sovereignty, and operational awareness.
//! 17 crates. 10 capability domains. One shared signal model.

mod cli;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "mesh table/status helpers await daemon integration, tracked in #264"
    )
)]
mod mesh;
mod radio;
mod vault;

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

    /// A vault operation failed.
    #[snafu(display("{source}"))]
    Vault { source: vault::VaultCliError },

    /// A mesh operation failed.
    #[snafu(display("{source}"))]
    Mesh { source: mesh::MeshError },

    /// An I/O operation failed.
    #[snafu(display("I/O error: {source}"))]
    Io { source: std::io::Error },
}

/// Application configuration loaded from TOML file and environment overrides.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
#[expect(
    dead_code,
    reason = "config fields reserved for future CLI options, tracked in #264"
)]
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

fn dispatch(command: &Command, out: &mut dyn std::io::Write) -> Result<(), Error> {
    match command {
        Command::Radio(args) => {
            radio::dispatch(&args.command, out).context(RadioSnafu)?;
        }
        Command::Mesh(args) => {
            mesh::dispatch(&args.command, out).context(MeshSnafu)?;
        }
        Command::Sdr => {
            writeln!(out, "dektis — SDR reception (not yet implemented)").context(IoSnafu)?;
        }
        Command::Proximity => writeln!(out, "engys — proximity intelligence (not yet implemented)")
            .context(IoSnafu)?,
        Command::Shield => {
            writeln!(out, "aspis — network defense (not yet implemented)").context(IoSnafu)?;
        }
        Command::Watch => {
            writeln!(out, "skopos — OSINT collection (not yet implemented)").context(IoSnafu)?;
        }
        Command::Test => {
            writeln!(out, "peira — offensive security (not yet implemented)").context(IoSnafu)?;
        }
        Command::Intel => {
            writeln!(
                out,
                "semaino + ichneutes — intelligence (not yet implemented)"
            )
            .context(IoSnafu)?;
        }
        Command::Auto => {
            writeln!(out, "praxis — automation (not yet implemented)").context(IoSnafu)?;
        }
        Command::Nav => {
            writeln!(out, "chorografia — navigation (not yet implemented)").context(IoSnafu)?;
        }
        Command::Know => {
            writeln!(out, "pinax — knowledge repository (not yet implemented)").context(IoSnafu)?;
        }
        Command::Vault(args) => {
            vault::dispatch(&args.command, out).context(VaultSnafu)?;
        }
        Command::Privacy => {
            writeln!(out, "lethe — privacy (not yet implemented)").context(IoSnafu)?;
        }
    }
    Ok(())
}

fn run() -> Result<(), Error> {
    let _config = load_config()?;
    let cli = Cli::parse();
    let mut stdout = std::io::stdout().lock();
    dispatch(&cli.command, &mut stdout)
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
