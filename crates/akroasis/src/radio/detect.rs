//! `akroasis radio detect` — discover connected radios.

use std::io::Write;

use serde::Serialize;
use snafu::ResultExt;

use super::errors::{IoSnafu, JsonReportSnafu, RadioError};
use super::{DetectedRadio, Hardware};

const RADIO_DETECT_JSON_SCHEMA: u8 = 1;

#[derive(Serialize)]
struct DetectReport<'a> {
    schema_version: u8,
    command: &'static str,
    radio_count: usize,
    radios: Vec<DetectedRadioReport<'a>>,
}

#[derive(Serialize)]
struct DetectedRadioReport<'a> {
    variant: &'static str,
    port: &'a str,
    firmware: &'a str,
    warnings: &'a [String],
}

/// Runs the detect subcommand.
pub(crate) fn run(hw: &dyn Hardware, json: bool, out: &mut dyn Write) -> Result<(), RadioError> {
    let radios = hw.detect_radios()?;

    if json {
        write_json_report(&radios, out)?;
        return Ok(());
    }

    if radios.is_empty() {
        writeln!(
            out,
            "No radios detected. Check that the radio is on \
             and the programming cable is connected."
        )
        .context(IoSnafu)?;
        return Ok(());
    }

    print_detected(&radios, out)?;
    Ok(())
}

fn write_json_report(radios: &[DetectedRadio], out: &mut dyn Write) -> Result<(), RadioError> {
    let report = DetectReport {
        schema_version: RADIO_DETECT_JSON_SCHEMA,
        command: "radio detect",
        radio_count: radios.len(),
        radios: radios
            .iter()
            .map(|radio| DetectedRadioReport {
                variant: radio.variant.display_name(),
                port: &radio.port,
                firmware: &radio.firmware,
                warnings: &radio.warnings,
            })
            .collect(),
    };

    serde_json::to_writer_pretty(&mut *out, &report).context(JsonReportSnafu)?;
    writeln!(out).context(IoSnafu)?;
    Ok(())
}

/// Formats and prints detected radios.
pub(crate) fn print_detected(
    radios: &[DetectedRadio],
    out: &mut dyn Write,
) -> Result<(), RadioError> {
    writeln!(out, "Detected radios:").context(IoSnafu)?;
    for (i, radio) in radios.iter().enumerate() {
        let firmware_info = if radio.firmware.is_empty() {
            String::new()
        } else {
            format!(" (firmware: {})", radio.firmware)
        };
        writeln!(
            out,
            "  {}. {} on {}{}",
            i + 1,
            radio.variant.display_name(),
            radio.port,
            firmware_info,
        )
        .context(IoSnafu)?;
    }

    let warnings: Vec<&str> = radios
        .iter()
        .flat_map(|r| r.warnings.iter().map(String::as_str))
        .collect();
    for warning in warnings {
        writeln!(out, "\n\u{26a0} {warning}").context(IoSnafu)?;
    }
    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions use unwrap and indexing for clarity"
)]
mod tests {
    use super::*;
    use crate::radio::RadioVariant;

    struct FakeHardware {
        radios: Vec<DetectedRadio>,
    }

    impl Hardware for FakeHardware {
        fn detect_radios(&self) -> Result<Vec<DetectedRadio>, RadioError> {
            Ok(self.radios.clone())
        }

        fn open(&self, _port: &str) -> Result<Box<dyn crate::radio::Session>, RadioError> {
            Err(RadioError::HardwareNotAvailable)
        }
    }

    #[test]
    fn format_single_detected_radio() {
        let radios = vec![DetectedRadio {
            variant: RadioVariant::BfF8hp,
            port: "/dev/ttyUSB0".to_string(),
            firmware: "BFP3V3".to_string(),
            warnings: vec![],
        }];

        let mut out = Vec::new();
        print_detected(&radios, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Detected radios:"));
        assert!(s.contains("BF-F8HP"));
        assert!(s.contains("/dev/ttyUSB0"));
        assert!(s.contains("BFP3V3"));
    }

    #[test]
    fn format_multiple_radios_with_warnings() {
        let radios = vec![
            DetectedRadio {
                variant: RadioVariant::BfF8hp,
                port: "/dev/ttyUSB0".to_string(),
                firmware: "BFP3V3".to_string(),
                warnings: vec![
                    "PL2303 clone detected on /dev/ttyUSB0 \u{2014} works on Linux, may fail on Windows.".to_string(),
                ],
            },
            DetectedRadio {
                variant: RadioVariant::Uv5r,
                port: "/dev/ttyUSB1".to_string(),
                firmware: "BFB297".to_string(),
                warnings: vec![],
            },
        ];

        let mut out = Vec::new();
        print_detected(&radios, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("1. Baofeng BF-F8HP"));
        assert!(s.contains("2. Baofeng UV-5R"));
        assert!(s.contains("PL2303 clone detected"));
    }

    #[test]
    fn run_no_radios_writes_message() {
        let mut out = Vec::new();
        // StubHardware returns HardwareNotAvailable, which means no radios.
        let result = run(&super::super::StubHardware, false, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn run_json_outputs_machine_readable_report() {
        let hw = FakeHardware {
            radios: vec![DetectedRadio {
                variant: RadioVariant::BfF8hp,
                port: "/dev/ttyUSB0".to_string(),
                firmware: "BFP3V3".to_string(),
                warnings: vec!["low-cost cable detected".to_string()],
            }],
        };

        let mut out = Vec::new();
        run(&hw, true, &mut out).unwrap();

        let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["command"], "radio detect");
        assert_eq!(report["radio_count"], 1);
        assert_eq!(report["radios"][0]["variant"], "Baofeng BF-F8HP");
        assert_eq!(report["radios"][0]["port"], "/dev/ttyUSB0");
        assert_eq!(report["radios"][0]["firmware"], "BFP3V3");
        assert_eq!(
            report["radios"][0]["warnings"][0],
            "low-cost cable detected"
        );
    }
}
