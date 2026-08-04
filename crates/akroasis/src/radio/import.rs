//! `akroasis radio import`  -  parse and display a frequency plan FROM a file.

use std::io::Write;
use std::path::Path;

use koinon::Frequency;
use serde::Serialize;
use snafu::ResultExt;
use syntonia::{Bandwidth, Channel, FrequencyPlan, PowerLevel, ScanMode, ToneMode};

use super::errors::{IoSnafu, JsonReportSnafu, RadioError, ReadFileSnafu, SyntoniaSnafu};
use super::read;

const RADIO_IMPORT_JSON_SCHEMA: u8 = 1;

#[derive(Serialize)]
struct ImportReport<'a> {
    schema_version: u8,
    command: &'static str,
    source: String,
    format: &'static str,
    channel_count: usize,
    empty_channel_slots: usize,
    plan: &'a FrequencyPlan,
}

/// Supported file formats for import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileFormat {
    Toml,
    Json,
    ChirpCsv,
    BinaryImage,
}

/// Detects file format FROM the extension.
pub(crate) fn detect_format(path: &Path) -> Result<FileFormat, RadioError> {
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
pub(crate) fn run(file: &Path, json: bool, out: &mut dyn Write) -> Result<(), RadioError> {
    let format = detect_format(file)?;
    let plan = import_plan(file, format)?;

    if json {
        write_json_report(file, format, &plan, out)?;
        return Ok(());
    }

    read::print_channel_table_owned(&plan.channels, out)?;

    let warning_count = plan
        .channels
        .iter()
        .filter(|ch| ch.rx_freq.as_hz() == 0)
        .count();
    if warning_count > 0 {
        writeln!(out, "\u{26a0} {warning_count} empty channel slots skipped").context(IoSnafu)?;
    }

    writeln!(
        out,
        "Imported {} channels FROM {}",
        plan.channel_count(),
        file.display(),
    )
    .context(IoSnafu)?;

    Ok(())
}

fn write_json_report(
    file: &Path,
    format: FileFormat,
    plan: &FrequencyPlan,
    out: &mut dyn Write,
) -> Result<(), RadioError> {
    let report = ImportReport {
        schema_version: RADIO_IMPORT_JSON_SCHEMA,
        command: "radio import",
        source: file.display().to_string(),
        format: format.as_str(),
        channel_count: plan.channel_count(),
        empty_channel_slots: empty_channel_slots(plan),
        plan,
    };

    serde_json::to_writer_pretty(&mut *out, &report).context(JsonReportSnafu)?;
    writeln!(out).context(IoSnafu)?;
    Ok(())
}

fn empty_channel_slots(plan: &FrequencyPlan) -> usize {
    plan.channels
        .iter()
        .filter(|ch| ch.rx_freq.as_hz() == 0)
        .count()
}

impl FileFormat {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Toml => "toml",
            Self::Json => "json",
            Self::ChirpCsv => "chirp-csv",
            Self::BinaryImage => "binary-image",
        }
    }
}

/// Imports a plan FROM a file in the detected format.
pub(crate) fn import_plan(path: &Path, format: FileFormat) -> Result<FrequencyPlan, RadioError> {
    if format == FileFormat::BinaryImage {
        return Err(RadioError::HardwareNotAvailable);
    }

    let content = std::fs::read_to_string(path).context(ReadFileSnafu {
        path: path.to_path_buf(),
    })?;
    import_from_string(&content, format, path)
}

/// Imports a plan FROM a string in the given format.
pub(crate) fn import_from_string(
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

/// Parses a CHIRP-compatible CSV INTO a `FrequencyPlan`.
#[expect(
    clippy::too_many_lines,
    reason = "flat CSV-column parsing; splitting into helpers would fragment a linear schema decode without clarity gain"
)]
pub(crate) fn parse_chirp_csv(content: &str) -> Result<FrequencyPlan, RadioError> {
    let mut channels = Vec::new();

    // WHY: CHIRP quotes any field containing a comma, a quote or a newline —
    // `export::csv_escape` writes exactly that form — so splitting on ','
    // misaligns every column after a quoted name and silently decodes the wrong
    // frequency. RFC4180 decoding is the csv crate's job, not this parser's.
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(content.as_bytes());

    for record in reader.records() {
        // WHY: a quoted field may span newlines, so the record's own position is
        // the only accurate line number; there is no line index to count.
        let record = record.map_err(|source| RadioError::CsvParse {
            line: csv_error_line(&source),
            message: source.to_string(),
        })?;

        let line_num = record
            .position()
            .map_or(0, |p| usize::try_from(p.line()).unwrap_or(usize::MAX));

        if record.iter().all(str::is_empty) {
            continue;
        }

        let cols = &record;
        if cols.len() < 15 {
            return Err(RadioError::CsvParse {
                line: line_num,
                message: format!("expected at least 15 columns, got {}", cols.len()),
            });
        }

        let index: u16 =
            cols.get(0)
                .unwrap_or_default()
                .parse()
                .map_err(|_| RadioError::CsvParse {
                    line: line_num,
                    message: format!("invalid location: '{}'", cols.get(0).unwrap_or_default()),
                })?;

        // WHY: the reader already removed the surrounding quotes and undoubled
        // any escaped ones, so trimming '"' here would corrupt a name whose own
        // first or last character is a quote.
        let name = cols.get(1).unwrap_or_default().to_string();

        let rx_mhz: f64 =
            cols.get(2)
                .unwrap_or_default()
                .parse()
                .map_err(|_| RadioError::CsvParse {
                    line: line_num,
                    message: format!("invalid frequency: '{}'", cols.get(2).unwrap_or_default()),
                })?;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "CHIRP frequencies are within u64 Hz range after MHz->Hz conversion; parser already validated rx_mhz"
        )]
        let rx_freq = Frequency::hz((rx_mhz * 1_000_000.0).round() as u64);

        let duplex = cols.get(3).unwrap_or_default();
        let offset_mhz: f64 = cols.get(4).unwrap_or_default().parse().unwrap_or(0.0);

        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "CHIRP offsets are within u64 Hz range after MHz->Hz conversion; parser already validated offset_mhz"
        )]
        let offset_freq = Frequency::hz((offset_mhz * 1_000_000.0).round() as u64);

        // WHY: Frequency's Add/Sub overload panics (debug) / wraps (release) on
        // out-of-range CSV offsets; check on raw hertz so a malformed row is a
        // CsvParse error instead of a crash or a silently-wrong tx frequency.
        let rx_hz = rx_freq.as_hz();
        let off_hz = offset_freq.as_hz();

        let (offset, tx_freq) = match duplex {
            "+" => {
                let tx_hz = rx_hz
                    .checked_add(off_hz)
                    .ok_or_else(|| RadioError::CsvParse {
                        line: line_num,
                        message: format!("frequency offset overflow: {rx_freq} + {offset_freq}"),
                    })?;
                (
                    syntonia::FrequencyOffset::Plus(offset_freq),
                    Some(Frequency::hz(tx_hz)),
                )
            }
            "-" => {
                let tx_hz = rx_hz
                    .checked_sub(off_hz)
                    .ok_or_else(|| RadioError::CsvParse {
                        line: line_num,
                        message: format!("frequency offset underflow: {rx_freq} - {offset_freq}"),
                    })?;
                (
                    syntonia::FrequencyOffset::Minus(offset_freq),
                    Some(Frequency::hz(tx_hz)),
                )
            }
            "split" => (
                syntonia::FrequencyOffset::Split(offset_freq),
                Some(offset_freq),
            ),
            _ => (syntonia::FrequencyOffset::None, None),
        };

        let tone = parse_chirp_tone(
            cols.get(5).unwrap_or_default(),
            cols.get(6).unwrap_or_default(),
            cols.get(8).unwrap_or_default(),
            cols.get(9).unwrap_or_default(),
        );

        let bandwidth = match cols.get(11) {
            Some("NFM") => Bandwidth::Narrow,
            _ => Bandwidth::Wide,
        };

        let scan = match cols.get(13) {
            Some("S") => ScanMode::Skip,
            _ => ScanMode::Include,
        };

        let power = match cols.get(14) {
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

/// Recovers the 1-based source line a CSV reader error occurred on.
///
/// Returns 0 when the reader could not attribute the failure to a position
/// (an I/O fault rather than a malformed record).
fn csv_error_line(err: &csv::Error) -> usize {
    err.position()
        .map_or(0, |p| usize::try_from(p.line()).unwrap_or(usize::MAX))
}

fn parse_chirp_tone(tone_mode: &str, r_tone: &str, dtcs_code: &str, dtcs_pol: &str) -> ToneMode {
    match tone_mode {
        "Tone" | "TSQL" => {
            let freq: f32 = r_tone.parse().unwrap_or(88.5);
            syntonia::CtcssTone::new(freq).map_or(ToneMode::None, ToneMode::Ctcss)
        }
        "DTCS" => {
            let code: u16 = dtcs_code.parse().unwrap_or(23);
            let polarity = match dtcs_pol {
                "RR" | "RN" => syntonia::DcsPolarity::Inverted,
                _ => syntonia::DcsPolarity::Normal,
            };
            syntonia::DcsCode::new(code).map_or(ToneMode::None, |c| ToneMode::Dcs(c, polarity))
        }
        _ => ToneMode::None,
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: panics and unwraps acceptable in assertions"
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
    fn parse_chirp_csv_duplex_minus_underflow_errors() {
        let csv = "\
Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,DtcsPolarity,RxDtcsCode,Mode,TStep,Skip,Power,Comment,URCALL,RPT1CALL,RPT2CALL,DVCODE
0,CALL,146.520000,-,200.000000,,88.5,88.5,023,NN,023,FM,5.00,,High,,,,,\n";

        let result = parse_chirp_csv(csv);
        assert!(matches!(result, Err(RadioError::CsvParse { .. })));
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

    /// Builds a one-row CHIRP CSV whose Name column is `name`, quoted verbatim.
    fn chirp_csv_with_quoted_name(name: &str) -> String {
        format!(
            "Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,\
             DtcsPolarity,RxDtcsCode,Mode,TStep,Skip,Power,Comment,URCALL,RPT1CALL,RPT2CALL,DVCODE\n\
             7,{name},146.520000,,0.000000,,88.5,88.5,023,NN,023,FM,5.00,,Low,,,,,\n"
        )
    }

    #[test]
    fn a_quoted_name_containing_a_comma_keeps_the_later_columns_aligned() {
        // WHY: splitting on ',' turns this one row into 21 fields, so Frequency
        // reads " North\"" and every column after it shifts by one.
        let csv = chirp_csv_with_quoted_name("\"Repeater, North\"");

        let plan = parse_chirp_csv(&csv).unwrap();

        assert_eq!(plan.channels.len(), 1);
        assert_eq!(plan.channels[0].name, "Repeater, North");
        assert_eq!(plan.channels[0].rx_freq.as_hz(), 146_520_000);
        // The Power column sits after the comma, so a shift would misread it.
        assert_eq!(plan.channels[0].power, PowerLevel::Low);
    }

    #[test]
    fn a_doubled_quote_in_a_name_is_undoubled_rather_than_stripped() {
        // RFC4180 escapes an embedded '"' by doubling it; the old parser used
        // trim_matches('"'), which neither undoubles nor survives interior quotes.
        let csv = chirp_csv_with_quoted_name("\"MT \"\"HIGH\"\"\"");

        let plan = parse_chirp_csv(&csv).unwrap();

        assert_eq!(plan.channels[0].name, "MT \"HIGH\"");
        assert_eq!(plan.channels[0].rx_freq.as_hz(), 146_520_000);
    }

    #[test]
    fn exported_names_needing_quotes_reimport_unchanged() {
        use crate::radio::export::export_chirp_csv;

        // WHY: export::csv_escape already writes RFC4180 quoting, so every name
        // here is one this crate itself emits — the roundtrip was broken against
        // akroasis's OWN export, not just against foreign CHIRP files.
        let tricky = ["Repeater, North", "MT \"HIGH\"", "A,B,C", "plain"];

        let plan = FrequencyPlan {
            name: "Roundtrip".to_string(),
            radio_model: None,
            channels: tricky
                .iter()
                .enumerate()
                .map(|(i, name)| Channel {
                    index: u16::try_from(i).unwrap(),
                    name: (*name).to_string(),
                    rx_freq: Frequency::hz(146_520_000),
                    tx_freq: None,
                    offset: syntonia::FrequencyOffset::None,
                    tone: ToneMode::None,
                    power: PowerLevel::High,
                    bandwidth: Bandwidth::Wide,
                    scan: ScanMode::Include,
                    busy_lock: false,
                })
                .collect(),
            created: None,
        };

        let imported = parse_chirp_csv(&export_chirp_csv(&plan)).unwrap();

        assert_eq!(imported.channels.len(), tricky.len());
        for (got, want) in imported.channels.iter().zip(tricky) {
            assert_eq!(got.name, want);
            assert_eq!(got.rx_freq.as_hz(), 146_520_000);
        }
    }

    #[test]
    fn a_quoted_embedded_newline_does_not_desynchronise_the_reported_line() {
        // The bad row is the 4th physical line; a record counter would say 3.
        let csv = "\
Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,DtcsPolarity,RxDtcsCode,Mode,TStep,Skip,Power,Comment,URCALL,RPT1CALL,RPT2CALL,DVCODE
0,\"two
line\",146.520000,,0.000000,,88.5,88.5,023,NN,023,FM,5.00,,High,,,,,
1,BAD,not-a-frequency,,0.000000,,88.5,88.5,023,NN,023,FM,5.00,,High,,,,,\n";

        let err = parse_chirp_csv(csv).unwrap_err();

        assert!(
            matches!(err, RadioError::CsvParse { line: 4, .. }),
            "expected CsvParse on physical line 4, got: {err:?}"
        );
        if let RadioError::CsvParse { message, .. } = err {
            assert!(message.contains("not-a-frequency"), "got: {message}");
        }
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

    #[test]
    fn import_run_outputs_channels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.toml");
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
        std::fs::write(&path, toml).unwrap();

        let mut out = Vec::new();
        run(&path, false, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("CALL"));
        assert!(s.contains("Imported 1 channels FROM"));
    }

    #[test]
    fn import_run_json_outputs_machine_readable_report() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.toml");
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
        std::fs::write(&path, toml).unwrap();

        let mut out = Vec::new();
        run(&path, true, &mut out).unwrap();

        let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["command"], "radio import");
        assert_eq!(report["format"], "toml");
        assert_eq!(report["channel_count"], 1);
        assert_eq!(report["empty_channel_slots"], 0);
        assert_eq!(report["plan"]["name"], "Test Plan");
        assert_eq!(report["plan"]["channels"][0]["name"], "CALL");
    }
}
