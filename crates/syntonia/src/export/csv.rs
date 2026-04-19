//! CHIRP CSV export.

use std::io::Write;

use snafu::Snafu;

use crate::channel::Channel;
use crate::plan::FrequencyPlan;
use crate::tone::ToneMode;
use crate::types::{Bandwidth, FrequencyOffset, PowerLevel, ScanMode};

/// Errors from CHIRP CSV export.
#[derive(Debug, Snafu)]
pub enum CsvExportError {
    /// CSV serialization error.
    #[snafu(display("CSV write error: {source}"))]
    CsvWrite {
        /// The underlying CSV error.
        source: csv::Error,
    },

    /// I/O error during write.
    #[snafu(display("I/O error during CSV export: {source}"))]
    Io {
        /// The I/O error.
        source: std::io::Error,
    },
}

/// Export a frequency plan to CHIRP-compatible 20-column CSV.
///
/// # Errors
///
/// Returns `CsvExportError` if writing to the output fails.
pub fn export_chirp_csv(
    plan: &FrequencyPlan,
    writer: &mut dyn Write,
) -> Result<(), CsvExportError> {
    let mut csv_writer = csv::Writer::from_writer(writer);

    csv_writer
        .write_record([
            "Location",
            "Name",
            "Frequency",
            "Duplex",
            "Offset",
            "Tone",
            "rToneFreq",
            "cToneFreq",
            "DtcsCode",
            "DtcsPolarity",
            "Mode",
            "TStep",
            "Skip",
            "Power",
            "Comment",
            "URCALL",
            "RPT1CALL",
            "RPT2CALL",
            "DVCODE",
        ])
        .map_err(|source| CsvExportError::CsvWrite { source })?;

    for channel in &plan.channels {
        let record = channel_to_record(channel);
        csv_writer
            .write_record(&record)
            .map_err(|source| CsvExportError::CsvWrite { source })?;
    }

    csv_writer
        .flush()
        .map_err(|source| CsvExportError::Io { source })?;

    Ok(())
}

fn channel_to_record(ch: &Channel) -> [String; 19] {
    let freq = format!("{:.6}", ch.rx_freq.as_mhz_f64());

    let (duplex, offset) = match ch.offset {
        FrequencyOffset::None => {
            if ch.tx_freq.is_none() {
                ("off".to_string(), "0.000000".to_string())
            } else {
                (String::new(), "0.000000".to_string())
            }
        }
        FrequencyOffset::Plus(off) => ("+".to_string(), format!("{:.6}", off.as_mhz_f64())),
        FrequencyOffset::Minus(off) => ("-".to_string(), format!("{:.6}", off.as_mhz_f64())),
        FrequencyOffset::Split(tx) => ("split".to_string(), format!("{:.6}", tx.as_mhz_f64())),
    };

    let (tone_mode, rtone, ctone, dtcs_code, dtcs_pol) = match &ch.tone {
        ToneMode::None => (
            String::new(),
            "88.5".to_string(),
            "88.5".to_string(),
            "023".to_string(),
            "NN".to_string(),
        ),
        ToneMode::Ctcss(tone) => {
            let freq_str = format!("{:.1}", tone.as_hz());
            (
                "Tone".to_string(),
                freq_str.clone(),
                freq_str,
                "023".to_string(),
                "NN".to_string(),
            )
        }
        ToneMode::Dcs(code, polarity) => {
            let pol_str = match polarity {
                crate::tone::DcsPolarity::Normal => "NN",
                crate::tone::DcsPolarity::Inverted => "RN",
            };
            (
                "DTCS".to_string(),
                "88.5".to_string(),
                "88.5".to_string(),
                format!("{:03}", code.as_code()),
                pol_str.to_string(),
            )
        }
    };

    let mode = match ch.bandwidth {
        Bandwidth::Wide => "FM",
        Bandwidth::Narrow => "NFM",
    };

    let skip = match ch.scan {
        ScanMode::Include => "",
        ScanMode::Skip => "S",
    };

    let power = match ch.power {
        PowerLevel::High => "High",
        PowerLevel::Mid => "Mid",
        PowerLevel::Low => "Low",
    };

    [
        ch.index.to_string(),
        ch.name.clone(),
        freq,
        duplex,
        offset,
        tone_mode,
        rtone,
        ctone,
        dtcs_code,
        dtcs_pol,
        mode.to_string(),
        "5.00".to_string(),
        skip.to_string(),
        power.to_string(),
        String::new(), // Comment
        String::new(), // URCALL
        String::new(), // RPT1CALL
        String::new(), // RPT2CALL
        String::new(), // DVCODE
    ]
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;
    use crate::import::csv::import_chirp_csv_reader;
    use crate::tone::CtcssTone;
    use koinon::Frequency;

    fn make_test_plan() -> FrequencyPlan {
        FrequencyPlan {
            name: String::new(),
            radio_model: None,
            channels: vec![
                Channel {
                    index: 0,
                    name: "CALL".to_string(),
                    rx_freq: Frequency::hz(146_520_000),
                    tx_freq: Some(Frequency::hz(146_520_000)),
                    offset: FrequencyOffset::None,
                    tone: ToneMode::None,
                    power: PowerLevel::High,
                    bandwidth: Bandwidth::Wide,
                    scan: ScanMode::Include,
                    busy_lock: false,
                },
                Channel {
                    index: 1,
                    name: "RPT".to_string(),
                    rx_freq: Frequency::hz(147_060_000),
                    tx_freq: Some(Frequency::hz(147_660_000)),
                    offset: FrequencyOffset::Plus(Frequency::hz(600_000)),
                    tone: ToneMode::Ctcss(CtcssTone::new(100.0).unwrap()),
                    power: PowerLevel::High,
                    bandwidth: Bandwidth::Wide,
                    scan: ScanMode::Include,
                    busy_lock: false,
                },
                Channel {
                    index: 2,
                    name: "UHF".to_string(),
                    rx_freq: Frequency::hz(446_000_000),
                    tx_freq: Some(Frequency::hz(446_000_000)),
                    offset: FrequencyOffset::None,
                    tone: ToneMode::Dcs(
                        crate::tone::DcsCode::new(23).unwrap(),
                        crate::tone::DcsPolarity::Normal,
                    ),
                    power: PowerLevel::Low,
                    bandwidth: Bandwidth::Narrow,
                    scan: ScanMode::Skip,
                    busy_lock: false,
                },
            ],
            created: None,
        }
    }

    #[test]
    fn export_produces_valid_csv() {
        let plan = make_test_plan();
        let mut buf = Vec::new();
        export_chirp_csv(&plan, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.starts_with("Location,Name,Frequency,"));
        assert!(output.contains("146.520000"));
        assert!(output.contains("CALL"));
    }

    #[test]
    fn roundtrip_csv_import_export_import() {
        let plan = make_test_plan();

        // Export
        let mut buf = Vec::new();
        export_chirp_csv(&plan, &mut buf).unwrap();

        // Re-import
        let (plan2, warnings) = import_chirp_csv_reader(buf.as_slice()).unwrap();

        // No real warnings expected
        let real_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| !matches!(w, crate::import::csv::ImportWarning::SkippedEmptyRow { .. }))
            .collect();
        assert!(
            real_warnings.is_empty(),
            "unexpected warnings: {real_warnings:?}"
        );

        // Plans should match
        assert_eq!(plan.channel_count(), plan2.channel_count());
        for (a, b) in plan.channels.iter().zip(plan2.channels.iter()) {
            assert_eq!(a.index, b.index, "index mismatch");
            assert_eq!(a.name, b.name, "name mismatch");
            assert_eq!(a.rx_freq, b.rx_freq, "rx_freq mismatch");
            assert_eq!(a.offset, b.offset, "OFFSET mismatch for {}", a.name);
            assert_eq!(a.tone, b.tone, "tone mismatch for {}", a.name);
            assert_eq!(a.power, b.power, "power mismatch for {}", a.name);
            assert_eq!(
                a.bandwidth, b.bandwidth,
                "bandwidth mismatch for {}",
                a.name
            );
            assert_eq!(a.scan, b.scan, "scan mismatch for {}", a.name);
        }
    }
}
