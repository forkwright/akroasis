//! Radio management CLI — detect, read, program, export, import.

pub mod detect;
pub mod errors;
pub mod export;
pub mod import;
pub mod program;
pub mod progress;
pub mod read;
#[cfg(feature = "hardware-serial")]
// kanon:ignore RUST/feature-gate-check -- declared in akroasis/Cargo.toml [features]
mod serial_hardware;

use std::path::PathBuf;

use clap::Subcommand;
use syntonia::{Channel, RadioConstraints};

use self::errors::RadioError;

/// Radio subcommands.
#[derive(Subcommand)]
pub enum RadioCommand {
    /// Detect connected radios
    Detect {
        /// Serial port to probe directly (e.g. /dev/ttyUSB0).
        #[arg(long)]
        port: Option<String>,

        /// Emit a machine-readable JSON report instead of human text.
        #[arg(long)]
        json: bool,
    },

    /// Read channels from radio
    Read {
        /// Serial port (e.g. /dev/ttyUSB0). Auto-detects if omitted.
        #[arg(long)]
        port: Option<String>,
    },

    /// Program radio with frequency plan
    Program {
        /// Serial port. Auto-detects if omitted.
        #[arg(long)]
        port: Option<String>,

        /// Frequency plan file (.toml or .json)
        #[arg(long)]
        plan: PathBuf,
    },

    /// Export channels from radio to file
    Export {
        /// Serial port. Auto-detects if omitted.
        #[arg(long)]
        port: Option<String>,

        /// Emit a machine-readable JSON completion report instead of human text.
        #[arg(long, requires = "output")]
        json: bool,

        /// Output format
        #[arg(long, value_enum)]
        format: ExportFormat,

        /// Output file (stdout if omitted)
        #[arg(long, short)]
        output: Option<PathBuf>,
    },

    /// Import and display a frequency plan from file
    Import {
        /// Emit a machine-readable JSON report instead of the human channel table.
        #[arg(long)]
        json: bool,

        /// Input file (.toml, .json, .csv, or .img)
        file: PathBuf,
    },
}

/// Supported export formats.
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum ExportFormat {
    Toml,
    Json,
    Csv,
    ChirpCsv,
}

impl ExportFormat {
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Csv => "csv",
            Self::ChirpCsv => "chirp-csv",
        }
    }
}

// ---------------------------------------------------------------------------
// Radio variant model
// ---------------------------------------------------------------------------

/// Known radio variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(feature = "hardware-serial"),
    allow(
        dead_code,
        reason = "radio variants used in test mocks; not all exercised in binary"
    )
)]
pub enum RadioVariant {
    Uv5r,
    BfF8hp,
    Uv5rmPlus,
}

impl RadioVariant {
    /// Human-readable display name.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Uv5r => "Baofeng UV-5R",
            Self::BfF8hp => "Baofeng BF-F8HP",
            Self::Uv5rmPlus => "Baofeng UV-5RM Plus",
        }
    }

    /// Returns radio-specific validation constraints.
    pub fn constraints(self) -> RadioConstraints {
        match self {
            Self::Uv5r | Self::Uv5rmPlus => syntonia::baofeng_uv5r_constraints(),
            Self::BfF8hp => syntonia::baofeng_f8hp_constraints(),
        }
    }

    /// Maximum number of channels for this variant.
    pub fn max_channels(self) -> u16 {
        self.constraints().max_channels
    }
}

impl std::fmt::Display for RadioVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

// ---------------------------------------------------------------------------
// Detected radio descriptor
// ---------------------------------------------------------------------------

/// A radio discovered during hardware detection.
#[derive(Debug, Clone)]
pub struct DetectedRadio {
    pub variant: RadioVariant,
    pub port: String,
    pub firmware: String,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Hardware abstraction (trait-based for testability)
// ---------------------------------------------------------------------------

/// Abstraction over radio hardware detection and connection.
pub trait Hardware {
    /// Enumerate all connected radio hardware.
    ///
    /// # Errors
    /// Returns [] on detection failure.
    fn detect_radios(&self) -> Result<Vec<DetectedRadio>, RadioError>;
    /// Return the radio on a specific port, if present.
    ///
    /// # Errors
    /// Returns [] if detection fails on the given port.
    fn detect_radio_on_port(&self, port: &str) -> Result<Option<DetectedRadio>, RadioError> {
        let mut radios = self.detect_radios()?;
        Ok(radios
            .iter()
            .position(|radio| radio.port == port)
            .map(|idx| radios.swap_remove(idx)))
    }

    /// Open a programming session on the given serial port.
    ///
    /// # Errors
    /// Returns [] if the port cannot be opened.
    fn open(&self, port: &str) -> Result<Box<dyn Session>, RadioError>;
}

/// An open connection to a radio.
pub trait Session {
    fn variant(&self) -> RadioVariant;
    /// Download the full EEPROM image from the radio.
    ///
    /// # Errors
    /// Returns [] on serial I/O or protocol errors.
    fn download_image(&mut self, on_block: &dyn Fn(u16, u16)) -> Result<Vec<u8>, RadioError>;
    /// Upload an EEPROM image to the radio.
    ///
    /// # Errors
    /// Returns [] on serial I/O or protocol errors.
    fn upload_image(&mut self, data: &[u8], on_block: &dyn Fn(u16, u16)) -> Result<(), RadioError>;
    /// Decode channel records from an EEPROM image.
    ///
    /// # Errors
    /// Returns [] if the image is malformed.
    fn decode_channels(&self, image: &[u8]) -> Result<Vec<Channel>, RadioError>;
    /// Encode channel records into an EEPROM image.
    ///
    /// # Errors
    /// Returns [] if encoding fails.
    fn encode_channels(&self, channels: &[Channel]) -> Result<Vec<u8>, RadioError>;
}

// ---------------------------------------------------------------------------
// Stub hardware (returns errors until P1-02..P1-06 are implemented)
// ---------------------------------------------------------------------------

#[cfg_attr(
    all(feature = "hardware-serial", not(test)),
    allow(
        dead_code,
        reason = "stub backend remains available for tests and no-hardware builds"
    )
)]
pub struct StubHardware;

impl Hardware for StubHardware {
    fn detect_radios(&self) -> Result<Vec<DetectedRadio>, RadioError> {
        Err(RadioError::HardwareNotAvailable)
    }

    fn open(&self, _port: &str) -> Result<Box<dyn Session>, RadioError> {
        Err(RadioError::HardwareNotAvailable)
    }
}

// ---------------------------------------------------------------------------
// Port resolution helper
// ---------------------------------------------------------------------------

/// Resolves the target radio from an explicit port or auto-detection.
///
/// # Errors
/// Returns [] when no radios are found,
/// [] when more than one is found,
/// or a hardware error on detection failure.
pub fn resolve_target(port: Option<&str>, hw: &dyn Hardware) -> Result<DetectedRadio, RadioError> {
    if let Some(port) = port {
        hw.detect_radio_on_port(port)?.map_or_else(
            || {
                // WHY: Port was given explicitly but detection didn't identify it.
                // Fall back to opening directly; the user may know the target.
                Ok(DetectedRadio {
                    variant: RadioVariant::Uv5r,
                    port: port.to_string(),
                    firmware: String::new(),
                    warnings: Vec::new(),
                })
            },
            Ok,
        )
    } else {
        let radios = hw.detect_radios()?;
        match radios.len() {
            0 => Err(RadioError::NoRadioDetected),
            1 => Ok(radios.into_iter().next().unwrap_or_else(|| {
                // SAFETY: We just verified len() == 1
                unreachable!()
            })),
            _ => Err(RadioError::MultipleRadiosDetected),
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Dispatches a radio subcommand.
///
/// # Errors
/// Returns [] if the command fails.
#[cfg(not(feature = "hardware-serial"))] // kanon:ignore RUST/feature-gate-check -- declared in akroasis/Cargo.toml [features]
pub fn dispatch(cmd: &RadioCommand, out: &mut dyn std::io::Write) -> Result<(), RadioError> {
    dispatch_with(cmd, &StubHardware, out)
}

/// Dispatches a radio subcommand with live serial detection enabled.
///
/// # Errors
/// Returns [] if the command fails.
#[cfg(feature = "hardware-serial")] // kanon:ignore RUST/feature-gate-check -- declared in akroasis/Cargo.toml [features]
pub fn dispatch(cmd: &RadioCommand, out: &mut dyn std::io::Write) -> Result<(), RadioError> {
    dispatch_with(cmd, &serial_hardware::SerialHardware, out)
}

/// Dispatches with a specific hardware backend (for testing).
///
/// # Errors
/// Returns [] if the command fails.
pub fn dispatch_with(
    cmd: &RadioCommand,
    hw: &dyn Hardware,
    out: &mut dyn std::io::Write,
) -> Result<(), RadioError> {
    match cmd {
        RadioCommand::Detect { port, json } => detect::run(port.as_deref(), hw, *json, out),
        RadioCommand::Read { port } => read::run(port.as_deref(), hw, out),
        RadioCommand::Program { port, plan } => program::run(port.as_deref(), plan, hw, out),
        RadioCommand::Export {
            port,
            json,
            format,
            output,
        } => export::run(port.as_deref(), *json, format, output.as_deref(), hw, out),
        RadioCommand::Import { json, file } => import::run(file, *json, out),
    }
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use clap::Parser;

    use super::*;

    // Wrapper to test subcommand parsing via clap.
    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: RadioCommand,
    }

    fn parse(args: &[&str]) -> RadioCommand {
        TestCli::parse_from(std::iter::once("test").chain(args.iter().copied())).command
    }

    #[test]
    fn parse_detect() {
        let cmd = parse(&["detect"]);
        match cmd {
            RadioCommand::Detect { port, json } => {
                assert!(port.is_none());
                assert!(!json);
            }
            _ => panic!("expected Detect"),
        }
    }

    #[test]
    fn parse_detect_json_flag() {
        let cmd = parse(&["detect", "--json"]);
        match cmd {
            RadioCommand::Detect { port, json } => {
                assert!(port.is_none());
                assert!(json);
            }
            _ => panic!("expected Detect"),
        }
    }

    #[test]
    fn parse_detect_with_port() {
        let cmd = parse(&["detect", "--port", "/dev/ttyUSB0"]);
        match cmd {
            RadioCommand::Detect { port, json } => {
                assert_eq!(port.as_deref(), Some("/dev/ttyUSB0"));
                assert!(!json);
            }
            _ => panic!("expected Detect"),
        }
    }

    #[test]
    fn parse_read_with_port() {
        let cmd = parse(&["read", "--port", "/dev/ttyUSB0"]);
        match cmd {
            RadioCommand::Read { port } => {
                assert_eq!(port.as_deref(), Some("/dev/ttyUSB0"));
            }
            _ => panic!("expected Read"),
        }
    }

    #[test]
    fn parse_program_with_both_args() {
        let cmd = parse(&[
            "program",
            "--port",
            "/dev/ttyUSB0",
            "--plan",
            "channels.toml",
        ]);
        match cmd {
            RadioCommand::Program { port, plan } => {
                assert_eq!(port.as_deref(), Some("/dev/ttyUSB0"));
                assert_eq!(plan, PathBuf::from("channels.toml"));
            }
            _ => panic!("expected Program"),
        }
    }

    #[test]
    fn parse_export_format_and_output() {
        let cmd = parse(&["export", "--format", "json", "--output", "out.json"]);
        match cmd {
            RadioCommand::Export {
                port,
                json,
                format,
                output,
            } => {
                assert!(port.is_none());
                assert!(!json);
                assert!(matches!(format, ExportFormat::Json));
                assert_eq!(output, Some(PathBuf::from("out.json")));
            }
            _ => panic!("expected Export"),
        }
    }

    #[test]
    fn parse_export_json_flag() {
        let cmd = parse(&["export", "--json", "--format", "csv", "--output", "out.csv"]);
        match cmd {
            RadioCommand::Export {
                port,
                json,
                format,
                output,
            } => {
                assert!(port.is_none());
                assert!(json);
                assert!(matches!(format, ExportFormat::Csv));
                assert_eq!(output, Some(PathBuf::from("out.csv")));
            }
            _ => panic!("expected Export"),
        }
    }

    #[test]
    fn parse_export_json_requires_output() {
        let result = TestCli::try_parse_from(["test", "export", "--json", "--format", "csv"]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_import_file() {
        let cmd = parse(&["import", "channels.csv"]);
        match cmd {
            RadioCommand::Import { json, file } => {
                assert!(!json);
                assert_eq!(file, PathBuf::from("channels.csv"));
            }
            _ => panic!("expected Import"),
        }
    }

    #[test]
    fn parse_import_json_flag() {
        let cmd = parse(&["import", "--json", "channels.csv"]);
        match cmd {
            RadioCommand::Import { json, file } => {
                assert!(json);
                assert_eq!(file, PathBuf::from("channels.csv"));
            }
            _ => panic!("expected Import"),
        }
    }

    #[test]
    fn parse_missing_required_args_fails() {
        // `program` requires --plan
        let result = TestCli::try_parse_from(["test", "program"]);
        assert!(result.is_err());
    }
}
