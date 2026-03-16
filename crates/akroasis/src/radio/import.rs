//! `akroasis radio import` — parse and display a frequency plan from a file.

use std::path::Path;

use koinon::Frequency;
use snafu::ResultExt;
use syntonia::{Bandwidth, Channel, FrequencyPlan, PowerLevel, ScanMode, ToneMode};

use super::errors::{RadioError, ReadFileSnafu, SyntoniaSnafu};
use super::read;

/// Supported file formats for import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Toml,
    Json,
    ChirpCsv,
    BinaryImage,
}

/// Detects file format from the extension.
pub fn detect_format(path: &Path) -> Result<FileFormat, RadioError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "toml" => Ok(FileFormat::Toml),
        "json" => Ok(FileFormat::Json),
        "csv" => Ok(FileFormat::ChirpCsv),
        "img" => Ok(FileFormat::BinaryImage),
        other => Err(RadioError::UnsupportedFormat {
            ext: other.to_string(),
        }),
    }
}

/// Runs the import subcommand.
pub fn run(file: &Path) -> Result<(), RadioError> {
    let format = detect_format(file)?;
    let plan = import_plan(file, format)?;

    read::print_channel_table_owned(&plan.channels);

    let warning_count = plan
        .channels
        .iter()
        .filter(|ch| ch.rx_freq.as_hz() == 0)
        .count();
    if warning_count > 0 {
        println!("\u{26a0} {warning_count} empty channel slots skipped");
    }

    println!(
        "Imported {} channels from {}",
        plan.channel_count(),
        file.display(),
    );

    Ok(())
}

/// Imports a plan from a file in the detected format.
pub fn import_plan(path: &Path, format: FileFormat) -> Result<FrequencyPlan, RadioError> {
    if format == FileFormat::BinaryImage {
        return Err(RadioError::HardwareNotAvailable);
    }

    let content = std::fs::read_to_string(path).context(ReadFileSnafu {
        path: path.to_path_buf(),
    })?;
    import_from_string(&content, format, path)
}

/// Imports a plan from a string in the given format.
pub fn import_from_string(
    content: &str,
    format: FileFormat,
    source_path: &Path,
) -> Result<FrequencyPlan, RadioError> {
    match format {
        FileFormat::Toml => FrequencyPlan::from_toml(content).context(SyntoniaSnafu),
        FileFormat::Json => FrequencyPlan::from_json(content).context(SyntoniaSnafu),
        FileFormat::ChirpCsv => parse_chirp_csv(content),
        FileFormat::BinaryImage => Err(RadioError::UnsupportedFormat {
            ext: source_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("img")
                .to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// CHIRP CSV parser
// ---------------------------------------------------------------------------

/// Parses a CHIRP-compatible CSV into a `FrequencyPlan`.
#[allow(clippy::indexing_slicing)] // Column access is safe: we verify cols.len() >= 15 above each use.
pub fn parse_chirp_csv(content: &str) -> Result<FrequencyPlan, RadioError> {
    let mut channels = Vec::new();
    let mut lines = content.lines();

    // Skip header
    let _header = lines.next();

    for (line_num, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 15 {
            return Err(RadioError::CsvParse {
                line: line_num + 2,
                message: format!("expected at least 15 columns, got {}", cols.len()),
            });
        }

        let index: u16 = cols[0].trim().parse().map_err(|_| RadioError::CsvParse {
            line: line_num + 2,
            message: format!("invalid location: '{}'", cols[0].trim()),
        })?;

        let name = cols[1].trim().trim_matches('"').to_string();

        let rx_mhz: f64 = cols[2].trim().parse().map_err(|_| RadioError::CsvParse {
            line: line_num + 2,
            message: format!("invalid frequency: '{}'", cols[2].trim()),
        })?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rx_freq = Frequency::hz((rx_mhz * 1_000_000.0).round() as u64);

        let duplex = cols[3].trim();
        let offset_mhz: f64 = cols[4].trim().parse().unwrap_or(0.0);

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let offset_freq = Frequency::hz((offset_mhz * 1_000_000.0).round() as u64);

        let (offset, tx_freq) = match duplex {
            "+" => (
                syntonia::FrequencyOffset::Plus(offset_freq),
                Some(rx_freq + offset_freq),
            ),
            "-" => (
                syntonia::FrequencyOffset::Minus(offset_freq),
                Some(rx_freq - offset_freq),
            ),
            "split" => (
                syntonia::FrequencyOffset::Split(offset_freq),
                Some(offset_freq),
            ),
            _ => (syntonia::FrequencyOffset::None, None),
        };

        let tone = parse_chirp_tone(
            cols[5].trim(),
            cols[6].trim(),
            cols[8].trim(),
            cols[9].trim(),
        );

        let bandwidth = match cols.get(11).map(|s| s.trim()) {
            Some("NFM") => Bandwidth::Narrow,
            _ => Bandwidth::Wide,
        };

        let scan = match cols.get(13).map(|s| s.trim()) {
            Some("S") => ScanMode::Skip,
            _ => ScanMode::Include,
        };

        let power = match cols.get(14).map(|s| s.trim()) {
            Some("Low") => PowerLevel::Low,
            Some("Mid") => PowerLevel::Mid,
            _ => PowerLevel::High,
        };

        channels.push(Channel {
            index,
            name,
            rx_freq,
            tx_freq,
            offset,
            tone,
            power,
            bandwidth,
            scan,
            busy_lock: false,
        });
    }

    Ok(FrequencyPlan {
        name: "CHIRP Import".to_string(),
        radio_model: None,
        channels,
        created: None,
    })
}

fn parse_chirp_tone(tone_mode: &str, r_tone: &str, dtcs_code: &str, dtcs_pol: &str) -> ToneMode {
    match tone_mode {
        "Tone" | "TSQL" => {
            let freq: f32 = r_tone.parse().unwrap_or(88.5);
            syntonia::CtcssTone::new(freq)
                .map(ToneMode::Ctcss)
                .unwrap_or(ToneMode::None)
        }
        "DTCS" => {
            let code: u16 = dtcs_code.parse().unwrap_or(23);
            let polarity = match dtcs_pol {
                "RR" | "RN" => syntonia::DcsPolarity::Inverted,
                _ => syntonia::DcsPolarity::Normal,
            };
            syntonia::DcsCode::new(code)
                .map(|c| ToneMode::Dcs(c, polarity))
                .unwrap_or(ToneMode::None)
        }
        _ => ToneMode::None,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests {
    use super::*;

    #[test]
    fn detect_toml_format() {
        assert_eq!(
            detect_format(Path::new("plan.toml")).unwrap(),
            FileFormat::Toml
        );
    }

    #[test]
    fn detect_json_format() {
        assert_eq!(
            detect_format(Path::new("plan.json")).unwrap(),
            FileFormat::Json
        );
    }

    #[test]
    fn detect_csv_format() {
        assert_eq!(
            detect_format(Path::new("channels.csv")).unwrap(),
            FileFormat::ChirpCsv
        );
    }

    #[test]
    fn detect_img_format() {
        assert_eq!(
            detect_format(Path::new("radio.img")).unwrap(),
            FileFormat::BinaryImage
        );
    }

    #[test]
    fn detect_unknown_format_errors() {
        let result = detect_format(Path::new("file.xyz"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("xyz"));
        assert!(msg.contains("Supported"));
    }

    #[test]
    fn parse_chirp_csv_basic() {
        let csv = "\
Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,DtcsPolarity,RxDtcsCode,Mode,TStep,Skip,Power,Comment,URCALL,RPT1CALL,RPT2CALL,DVCODE
0,CALL,146.520000,,0.000000,,88.5,88.5,023,NN,023,FM,5.00,,High,,,,,
1,RPT-IN,147.060000,+,0.600000,Tone,100.0,100.0,023,NN,023,FM,5.00,,High,,,,,\n";

        let plan = parse_chirp_csv(csv).unwrap();
        assert_eq!(plan.channels.len(), 2);

        let ch0 = &plan.channels[0];
        assert_eq!(ch0.index, 0);
        assert_eq!(ch0.name, "CALL");
        assert_eq!(ch0.rx_freq.as_hz(), 146_520_000);
        assert!(ch0.tx_freq.is_none());
        assert!(matches!(ch0.tone, ToneMode::None));

        let ch1 = &plan.channels[1];
        assert_eq!(ch1.index, 1);
        assert_eq!(ch1.name, "RPT-IN");
        assert!(ch1.tx_freq.is_some());
        assert!(matches!(ch1.tone, ToneMode::Ctcss(_)));
    }

    #[test]
    fn chirp_csv_roundtrip() {
        use crate::radio::export::export_chirp_csv;

        let plan = FrequencyPlan {
            name: "Roundtrip".to_string(),
            radio_model: None,
            channels: vec![Channel {
                index: 0,
                name: "TEST".to_string(),
                rx_freq: Frequency::hz(146_520_000),
                tx_freq: None,
                offset: syntonia::FrequencyOffset::None,
                tone: ToneMode::None,
                power: PowerLevel::High,
                bandwidth: Bandwidth::Wide,
                scan: ScanMode::Include,
                busy_lock: false,
            }],
            created: None,
        };

        let csv = export_chirp_csv(&plan);
        let imported = parse_chirp_csv(&csv).unwrap();

        assert_eq!(imported.channels.len(), 1);
        assert_eq!(imported.channels[0].index, 0);
        assert_eq!(imported.channels[0].name, "TEST");
        assert_eq!(imported.channels[0].rx_freq, plan.channels[0].rx_freq);
    }

    #[test]
    fn import_from_toml_string() {
        let toml = r#"
name = "Test Plan"
radio_model = "Baofeng UV-5R"

[[channels]]
index = 0
name = "CALL"
rx_freq = 146520000
offset = "None"
tone = "None"
power = "High"
bandwidth = "Wide"
scan = "Include"
busy_lock = false
"#;

        let plan = import_from_string(toml, FileFormat::Toml, Path::new("test.toml")).unwrap();
        assert_eq!(plan.channels.len(), 1);
        assert_eq!(plan.channels[0].name, "CALL");
    }
}
