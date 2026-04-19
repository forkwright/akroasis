//! User-friendly error types for radio operations.

use snafu::Snafu;

/// Errors from radio CLI operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(super)))]
#[non_exhaustive]
pub enum RadioError {
    #[snafu(display(
        "No radio detected. Check that the radio is on and the programming cable is connected."
    ))]
    NoRadioDetected,

    #[snafu(display("Multiple radios detected. Use --port to specify which one."))]
    MultipleRadiosDetected,

    #[snafu(display(
        "Radio not responding. Try: power cycle the radio, reseat the cable, \
         check the volume knob isn't muting the mic connector."
    ))]
    SerialTimeout { port: String },

    #[snafu(display(
        "Cannot access {port}. Run: sudo usermod -aG dialout $USER (then log out and back in)"
    ))]
    PermissionDenied { port: String },

    #[snafu(display(
        "Radio didn't respond at 9600 baud. This might be a UV-17Pro or similar \
         radio requiring a different protocol."
    ))]
    WrongBaudRate { port: String },

    #[snafu(display(
        "SAFETY: Refused to write calibration data at address {addr:#06x}. \
         This protects your radio from being bricked."
    ))]
    ForbiddenAddress { addr: u16 },

    #[snafu(display(
        "File size doesn't match any known radio. Expected 6152 or 7176 bytes, got {size}."
    ))]
    ImageSizeMismatch { size: usize },

    #[snafu(display("failed to read {}: {source}", path.display()))]
    ReadFile {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[snafu(display("failed to write {}: {source}", path.display()))]
    WriteFile {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[snafu(display("unsupported file format '{ext}'. Supported: .toml, .json, .csv, .img"))]
    UnsupportedFormat { ext: String },

    #[snafu(display("plan validation failed: {message}"))]
    ValidationFailed { message: String },

    #[snafu(display("{message}"))]
    Plan { message: String },

    #[snafu(display("{source}"))]
    Syntonia { source: syntonia::Error },

    #[snafu(display("write aborted by user"))]
    WriteAborted,

    #[snafu(display("verification failed: {message}"))]
    VerificationFailed { message: String },

    #[snafu(display("CSV parse error at line {line}: {message}"))]
    CsvParse { line: usize, message: String },

    #[snafu(display("hardware support not yet available (syntonia protocol layer pending)"))]
    HardwareNotAvailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_radio_detected_message() {
        let err = RadioError::NoRadioDetected;
        let msg = err.to_string();
        assert!(msg.contains("No radio detected"));
        assert!(msg.contains("programming cable"));
    }

    #[test]
    fn permission_denied_includes_port_and_fix() {
        let err = RadioError::PermissionDenied {
            port: "/dev/ttyUSB0".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("/dev/ttyUSB0"));
        assert!(msg.contains("dialout"));
    }

    #[test]
    fn forbidden_address_shows_hex() {
        let err = RadioError::ForbiddenAddress { addr: 0x1EC0 };
        let msg = err.to_string();
        assert!(msg.contains("0x1ec0") || msg.contains("0x1EC0"));
        assert!(msg.contains("SAFETY"));
    }
}
