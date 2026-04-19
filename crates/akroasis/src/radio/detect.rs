//! `akroasis radio detect` — discover connected radios.

use super::errors::RadioError;
use super::{DetectedRadio, Hardware};

/// Runs the detect subcommand.
pub(crate) fn run(hw: &dyn Hardware) -> Result<(), RadioError> {
    let radios = hw.detect_radios()?;

    if radios.is_empty() {
        println!(
            "No radios detected. Check that the radio is on \
             and the programming cable is connected."
        );
        return Ok(());
    }

    print_detected(&radios);
    Ok(())
}

/// Formats and prints detected radios.
pub(crate) fn print_detected(radios: &[DetectedRadio]) {
    println!("Detected radios:");
    for (i, radio) in radios.iter().enumerate() {
        let firmware_info = if radio.firmware.is_empty() {
            String::new()
        } else {
            format!(" (firmware: {})", radio.firmware)
        };
        println!(
            "  {}. {} on {}{}",
            i + 1,
            radio.variant.display_name(),
            radio.port,
            firmware_info,
        );
    }

    let warnings: Vec<&str> = radios
        .iter()
        .flat_map(|r| r.warnings.iter().map(String::as_str))
        .collect();
    for warning in warnings {
        println!("\n\u{26a0} {warning}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radio::RadioVariant;

    #[test]
    fn format_single_detected_radio() {
        let radios = vec![DetectedRadio {
            variant: RadioVariant::BfF8hp,
            port: "/dev/ttyUSB0".to_string(),
            firmware: "BFP3V3".to_string(),
            warnings: vec![],
        }];

        // Capture by calling print — we just verify it doesn't panic.
        // The format is validated by the integration test.
        print_detected(&radios);
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

        print_detected(&radios);
    }
}
