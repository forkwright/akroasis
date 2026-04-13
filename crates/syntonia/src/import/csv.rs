//! CHIRP CSV file import.

use std::io::Read;
use std::path::Path;

use koinon::Frequency;
use snafu::{ResultExt, Snafu};

use crate::channel::Channel;
use crate::plan::FrequencyPlan;
use crate::tone::{CtcssTone, DcsCode, DcsPolarity, ToneMode};
use crate::types::{Bandwidth, FrequencyOffset, PowerLevel, ScanMode};

/// Warning generated during CSV import for non-fatal issues.
#[derive(Debug, Clone)]
pub enum ImportWarning {
    /// A field is not supported by the native model (e.g., D-STAR fields).
    UnsupportedField {
        /// CSV row number (1-indexed).
        row: usize,
        /// Field name.
        field: String,
        /// Field value.
        value: String,
    },
    /// Frequency could not be parsed.
    InvalidFrequency {
        /// CSV row number.
        row: usize,
        /// The invalid frequency string.
        freq: String,
    },
    /// Unrecognized tone mode string.
    UnknownToneMode {
        /// CSV row number.
        row: usize,
        /// The unrecognized mode string.
        mode: String,
    },
    /// Unrecognized power level string.
    UnknownPowerLevel {
        /// CSV row number.
        row: usize,
        /// The unrecognized power string.
        level: String,
    },
    /// Row had no frequency and was skipped.
    SkippedEmptyRow {
        /// CSV row number.
        row: usize,
    },
}

/// Errors FROM CHIRP CSV import.
#[derive(Debug, Snafu)]
pub enum CsvImportError {
    /// Failed to read the CSV file FROM disk.
    #[snafu(display("failed to read CSV file at {}: {source}", path.display()))]
    ReadFile {
        /// Path to the file.
        path: std::path::PathBuf,
        /// The I/O error.
        source: std::io::Error,
    },

    /// CSV parsing error.
    #[snafu(display("CSV parse error: {source}"))]
    CsvParse {
        /// The underlying CSV parse error.
        source: csv::Error,
    },
}

/// Import a CHIRP CSV file FROM disk.
///
/// # Errors
///
/// Returns `CsvImportError` on I/O or CSV parse failure.
pub fn import_chirp_csv(
    path: &Path,
) -> Result<(FrequencyPlan, Vec<ImportWarning>), CsvImportError> {
    let file = std::fs::File::open(path).context(ReadFileSnafu {
        path: path.to_path_buf(),
    })?;
    import_chirp_csv_reader(file)
}

/// Import CHIRP CSV data FROM any reader.
///
/// # Errors
///
/// Returns `CsvImportError` on CSV parse failure.
pub fn import_chirp_csv_reader<R: Read>(
    reader: R,
) -> Result<(FrequencyPlan, Vec<ImportWarning>), CsvImportError> {
    let mut csv_reader = csv::ReaderBuilder::new().flexible(true).from_reader(reader);
    let mut channels = Vec::new();
    let mut warnings = Vec::new();

    for (row_idx, result) in csv_reader.records().enumerate() {
        let record = result.context(CsvParseSnafu)?;
        let row = row_idx + 2; // 1-indexed, +1 for header

        if let Some(channel) = parse_record(&record, row, &mut warnings) {
            channels.push(channel);
        }
    }

    let plan = FrequencyPlan {
        name: String::new(),
        radio_model: None,
        channels,
        created: None,
    };

    Ok((plan, warnings))
}

fn col(record: &csv::StringRecord, idx: usize) -> &str {
    record.get(idx).unwrap_or("").trim()
}

fn mhz_to_hz(mhz: f64) -> u64 {
    // SAFETY(cast): caller ensures mhz > 0
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let hz = (mhz * 1_000_000.0) as u64;
    hz
}

fn parse_record(
    record: &csv::StringRecord,
    row: usize,
    warnings: &mut Vec<ImportWarning>,
) -> Option<Channel> {
    let freq_str = col(record, 2);
    if freq_str.is_empty() || freq_str == "0.000000" {
        warnings.push(ImportWarning::SkippedEmptyRow { row });
        return None;
    }

    let freq_mhz: f64 = match freq_str.parse() {
        Ok(f) if f > 0.0 => f,
        _ => {
            warnings.push(ImportWarning::InvalidFrequency {
                row,
                freq: freq_str.to_string(),
            });
            return None;
        }
    };

    let rx_freq = Frequency::hz(mhz_to_hz(freq_mhz));

    let location: u16 = col(record, 0).parse().unwrap_or(0);
    let name = col(record, 1).to_string();

    let duplex = col(record, 3);
    let offset_val: f64 = col(record, 4).parse().unwrap_or(0.0);
    let offset_hz = mhz_to_hz(offset_val.abs());

    let (tx_freq, offset) = parse_duplex(duplex, rx_freq, offset_hz);

    let tone_mode_str = col(record, 5);
    let rtone_str = record.get(6).unwrap_or("88.5").trim();
    let ctone_str = record.get(7).unwrap_or("88.5").trim();
    let dtcs_code_str = record.get(8).unwrap_or("023").trim();
    let dtcs_pol_str = record.get(9).unwrap_or("NN").trim();

    let tone = parse_tone(
        tone_mode_str,
        rtone_str,
        ctone_str,
        dtcs_code_str,
        dtcs_pol_str,
        row,
        warnings,
    );

    let mode = record.get(10).unwrap_or("FM").trim();
    let bandwidth = match mode {
        "NFM" => Bandwidth::Narrow,
        "FM" => Bandwidth::Wide,
        other => {
            warnings.push(ImportWarning::UnsupportedField {
                row,
                field: "Mode".to_string(),
                value: other.to_string(),
            });
            Bandwidth::Wide
        }
    };

    let skip = record.get(12).unwrap_or("").trim();
    let scan = if skip == "S" {
        ScanMode::Skip
    } else {
        ScanMode::Include
    };

    let power_str = record.get(13).unwrap_or("High").trim();
    let power = parse_power(power_str, row, warnings);

    // D-STAR fields: warn if present
    for (field_col, field_name) in [
        (15, "URCALL"),
        (16, "RPT1CALL"),
        (17, "RPT2CALL"),
        (18, "DVCODE"),
    ] {
        let val = col(record, field_col);
        if !val.is_empty() {
            warnings.push(ImportWarning::UnsupportedField {
                row,
                field: field_name.to_string(),
                value: val.to_string(),
            });
        }
    }

    Some(Channel {
        index: location,
        name,
        rx_freq,
        tx_freq,
        offset,
        tone,
        power,
        bandwidth,
        scan,
        busy_lock: false,
    })
}

fn parse_duplex(
    duplex: &str,
    rx_freq: Frequency,
    offset_hz: u64,
) -> (Option<Frequency>, FrequencyOffset) {
    match duplex {
        "+" => {
            let tx = Frequency::hz(rx_freq.as_hz() + offset_hz);
            (Some(tx), FrequencyOffset::Plus(Frequency::hz(offset_hz)))
        }
        "-" => {
            let tx = Frequency::hz(rx_freq.as_hz().saturating_sub(offset_hz));
            (Some(tx), FrequencyOffset::Minus(Frequency::hz(offset_hz)))
        }
        "split" => {
            let tx = Frequency::hz(offset_hz);
            (Some(tx), FrequencyOffset::Split(tx))
        }
        "off" => (None, FrequencyOffset::None),
        _ => (Some(rx_freq), FrequencyOffset::None),
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_tone(
    mode: &str,
    rtone: &str,
    ctone: &str,
    dtcs_code: &str,
    dtcs_pol: &str,
    row: usize,
    warnings: &mut Vec<ImportWarning>,
) -> ToneMode {
    match mode {
        "" | "none" => ToneMode::None,
        "Tone" => parse_ctcss(rtone, "Tone", row, warnings),
        "TSQL" => parse_ctcss(ctone, "TSQL", row, warnings),
        "DTCS" => {
            let code_val: u16 = dtcs_code.parse().unwrap_or(0);
            DcsCode::new(code_val).map_or_else(
                |_| {
                    warnings.push(ImportWarning::UnknownToneMode {
                        row,
                        mode: format!("DTCS with invalid code {dtcs_code}"),
                    });
                    ToneMode::None
                },
                |code| {
                    let polarity = parse_dcs_polarity(dtcs_pol);
                    ToneMode::Dcs(code, polarity)
                },
            )
        }
        other => {
            warnings.push(ImportWarning::UnknownToneMode {
                row,
                mode: other.to_string(),
            });
            ToneMode::None
        }
    }
}

fn parse_ctcss(
    freq_str: &str,
    label: &str,
    row: usize,
    warnings: &mut Vec<ImportWarning>,
) -> ToneMode {
    let freq: f32 = freq_str.parse().unwrap_or(0.0);
    CtcssTone::new(freq).map_or_else(
        |_| {
            warnings.push(ImportWarning::UnknownToneMode {
                row,
                mode: format!("{label} with invalid freq {freq_str}"),
            });
            ToneMode::None
        },
        ToneMode::Ctcss,
    )
}

fn parse_dcs_polarity(pol: &str) -> DcsPolarity {
    // WHY: CHIRP uses two-character polarity strings WHERE the first char
    // is the TX polarity and the second is the RX polarity. We only store
    // a single polarity, so use the TX (first) character.
    if pol.starts_with('R') {
        DcsPolarity::Inverted
    } else {
        DcsPolarity::Normal
    }
}

fn parse_power(power: &str, row: usize, warnings: &mut Vec<ImportWarning>) -> PowerLevel {
    match power.to_lowercase().as_str() {
        "high" => PowerLevel::High,
        "mid" | "medium" => PowerLevel::Mid,
        "low" => PowerLevel::Low,
        other => {
            let stripped = other.trim_end_matches('w');
            if let Ok(watts) = stripped.parse::<f64>() {
                if watts >= 4.0 {
                    return PowerLevel::High;
                }
                if watts >= 2.0 {
                    return PowerLevel::Mid;
                }
                return PowerLevel::Low;
            }
            warnings.push(ImportWarning::UnknownPowerLevel {
                row,
                level: power.to_string(),
            });
            PowerLevel::High
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const FIXTURE_CSV: &str = "\
Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,DtcsPolarity,Mode,TStep,Skip,Power,Comment,URCALL,RPT1CALL,RPT2CALL,DVCODE
0,CALL,146.520000,,0.000000,,88.5,88.5,023,NN,FM,5.00,,High,,,,,,
1,RPT IN,147.060000,+,0.600000,Tone,100.0,100.0,023,NN,FM,5.00,,High,,,,,,
2,UHF DCS,446.000000,,0.000000,DTCS,88.5,88.5,023,NN,NFM,5.00,S,Low,,,,,,
3,SPLIT,146.520000,split,443.000000,,88.5,88.5,023,NN,FM,5.00,,Mid,,,,,,
4,TXOFF,146.520000,off,0.000000,,88.5,88.5,023,NN,FM,5.00,,High,,,,,,
5,TSQL,146.520000,,0.000000,TSQL,88.5,100.0,023,NN,FM,5.00,,High,,,,,,
6,MINUS,147.360000,-,0.600000,Tone,100.0,100.0,023,NN,FM,5.00,,High,,,,,,
";

    #[test]
    fn parse_fixture_csv() {
        let (plan, warnings) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        assert_eq!(plan.channel_count(), 7);

        let non_empty_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| !matches!(w, ImportWarning::SkippedEmptyRow { .. }))
            .collect();
        assert!(
            non_empty_warnings.is_empty(),
            "unexpected warnings: {non_empty_warnings:?}"
        );
    }

    #[test]
    fn simplex_channel_parsed() {
        let (plan, _) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        let ch = plan.channel(0).unwrap();
        assert_eq!(ch.rx_freq, Frequency::hz(146_520_000));
        assert_eq!(ch.offset, FrequencyOffset::None);
        assert_eq!(ch.name, "CALL");
    }

    #[test]
    fn duplex_plus_offset() {
        let (plan, _) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        let ch = plan.channel(1).unwrap();
        assert_eq!(ch.rx_freq, Frequency::hz(147_060_000));
        assert_eq!(ch.offset, FrequencyOffset::Plus(Frequency::hz(600_000)));
        assert_eq!(ch.tx_freq, Some(Frequency::hz(147_660_000)));
    }

    #[test]
    fn duplex_minus_offset() {
        let (plan, _) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        let ch = plan.channel(6).unwrap();
        assert_eq!(ch.rx_freq, Frequency::hz(147_360_000));
        assert_eq!(ch.offset, FrequencyOffset::Minus(Frequency::hz(600_000)));
        assert_eq!(ch.tx_freq, Some(Frequency::hz(146_760_000)));
    }

    #[test]
    fn duplex_split() {
        let (plan, _) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        let ch = plan.channel(3).unwrap();
        assert_eq!(
            ch.offset,
            FrequencyOffset::Split(Frequency::hz(443_000_000))
        );
        assert_eq!(ch.tx_freq, Some(Frequency::hz(443_000_000)));
    }

    #[test]
    fn duplex_off_tx_disabled() {
        let (plan, _) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        let ch = plan.channel(4).unwrap();
        assert_eq!(ch.tx_freq, None);
    }

    #[test]
    fn ctcss_tone_mode_parsed() {
        let (plan, _) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        let ch = plan.channel(1).unwrap();
        assert_eq!(ch.tone, ToneMode::Ctcss(CtcssTone::new(100.0).unwrap()));
    }

    #[test]
    fn tsql_mode_uses_ctone_freq() {
        let (plan, _) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        let ch = plan.channel(5).unwrap();
        assert_eq!(ch.tone, ToneMode::Ctcss(CtcssTone::new(100.0).unwrap()));
    }

    #[test]
    fn dcs_mode_parsed() {
        let (plan, _) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        let ch = plan.channel(2).unwrap();
        assert_eq!(
            ch.tone,
            ToneMode::Dcs(DcsCode::new(23).unwrap(), DcsPolarity::Normal)
        );
    }

    #[test]
    fn power_level_parsing() {
        let (plan, _) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        assert_eq!(plan.channel(0).unwrap().power, PowerLevel::High);
        assert_eq!(plan.channel(2).unwrap().power, PowerLevel::Low);
        assert_eq!(plan.channel(3).unwrap().power, PowerLevel::Mid);
    }

    #[test]
    fn narrow_bandwidth_parsed() {
        let (plan, _) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        assert_eq!(plan.channel(2).unwrap().bandwidth, Bandwidth::Narrow);
        assert_eq!(plan.channel(0).unwrap().bandwidth, Bandwidth::Wide);
    }

    #[test]
    fn skip_flag_parsed() {
        let (plan, _) = import_chirp_csv_reader(FIXTURE_CSV.as_bytes()).unwrap();
        assert_eq!(plan.channel(2).unwrap().scan, ScanMode::Skip);
        assert_eq!(plan.channel(0).unwrap().scan, ScanMode::Include);
    }

    #[test]
    fn dstar_fields_generate_warnings() {
        let csv_data = "\
Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,DtcsPolarity,Mode,TStep,Skip,Power,Comment,URCALL,RPT1CALL,RPT2CALL,DVCODE
0,DSTAR,146.520000,,0.000000,,88.5,88.5,023,NN,FM,5.00,,High,,CQCQCQ,RPT1,RPT2,
";
        let (plan, warnings) = import_chirp_csv_reader(csv_data.as_bytes()).unwrap();
        assert_eq!(plan.channel_count(), 1);

        let dstar_count = warnings
            .iter()
            .filter(|w| matches!(w, ImportWarning::UnsupportedField { field, .. } if field == "URCALL" || field == "RPT1CALL" || field == "RPT2CALL"))
            .count();
        assert_eq!(dstar_count, 3);
    }

    #[test]
    fn empty_row_skipped_with_warning() {
        let csv_data = "\
Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,DtcsPolarity,Mode,TStep,Skip,Power,Comment,URCALL,RPT1CALL,RPT2CALL,DVCODE
0,,,,,,88.5,88.5,023,NN,FM,5.00,,High,,,,,,
";
        let (plan, warnings) = import_chirp_csv_reader(csv_data.as_bytes()).unwrap();
        assert_eq!(plan.channel_count(), 0);
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ImportWarning::SkippedEmptyRow { .. }))
        );
    }

    #[test]
    fn invalid_frequency_generates_warning() {
        let csv_data = "\
Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,DtcsPolarity,Mode,TStep,Skip,Power,Comment,URCALL,RPT1CALL,RPT2CALL,DVCODE
0,BAD,not_a_number,,0.000000,,88.5,88.5,023,NN,FM,5.00,,High,,,,,,
";
        let (plan, warnings) = import_chirp_csv_reader(csv_data.as_bytes()).unwrap();
        assert_eq!(plan.channel_count(), 0);
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ImportWarning::InvalidFrequency { .. }))
        );
    }

    #[test]
    fn wattage_power_string_parsed() {
        let csv_data = "\
Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,DtcsPolarity,Mode,TStep,Skip,Power,Comment,URCALL,RPT1CALL,RPT2CALL,DVCODE
0,HI,146.520000,,0.000000,,88.5,88.5,023,NN,FM,5.00,,5.0W,,,,,,
1,LO,146.520000,,0.000000,,88.5,88.5,023,NN,FM,5.00,,1.0W,,,,,,
";
        let (plan, _) = import_chirp_csv_reader(csv_data.as_bytes()).unwrap();
        assert_eq!(plan.channel(0).unwrap().power, PowerLevel::High);
        assert_eq!(plan.channel(1).unwrap().power, PowerLevel::Low);
    }
}
