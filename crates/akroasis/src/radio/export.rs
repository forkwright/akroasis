//! `akroasis radio export` — read radio and export channels to a file.

use std::io::Write;
use std::path::Path;

use serde::Serialize;
use snafu::ResultExt;
use syntonia::{Bandwidth, Channel, FrequencyPlan, PowerLevel, ScanMode, ToneMode};

use super::errors::{IoSnafu, JsonReportSnafu, RadioError, SyntoniaSnafu, WriteFileSnafu};
use super::progress;
use super::{ExportFormat, Hardware, RadioVariant, resolve_target};

const RADIO_EXPORT_JSON_SCHEMA: u8 = 1;

#[derive(Serialize)]
struct ExportReport<'a> {
    schema_version: u8,
    command: &'static str,
    port: &'a str,
    variant: &'static str,
    firmware: &'a str,
    format: &'static str,
    output: &'a str,
    channel_count: usize,
    plan: &'a FrequencyPlan,
}

/// Runs the export subcommand.
pub(crate) fn run(
    port: Option<&str>,
    json: bool,
    format: &ExportFormat,
    output: Option<&Path>,
    hw: &dyn Hardware,
    out: &mut dyn Write,
) -> Result<(), RadioError> {
    let target = resolve_target(port, hw)?;
    let mut session = hw.open(&target.port)?;

    let pb = progress::download_bar(128);
    let image = session.download_image(&|done, total| {
        pb.set_length(u64::from(total));
        pb.set_position(u64::from(done));
    })?;
    pb.finish_and_clear();

    let channels = session.decode_channels(&image)?;

    let plan = FrequencyPlan {
        name: format!("{} export", target.variant.display_name()),
        radio_model: Some(target.variant.display_name().to_string()),
        channels,
        created: None,
    };

    let content = serialize_plan(&plan, format)?;

    match output {
        Some(path) => {
            std::fs::write(path, &content).context(WriteFileSnafu {
                path: path.to_path_buf(),
            })?;
            if json {
                write_json_report(
                    &target.port,
                    target.variant,
                    &target.firmware,
                    format,
                    path,
                    &plan,
                    out,
                )?;
                return Ok(());
            }
            writeln!(
                out,
                "Exported {} channels to {}",
                plan.channel_count(),
                path.display()
            )
            .context(IoSnafu)?;
        }
        None => write!(out, "{content}").context(IoSnafu)?,
    }

    Ok(())
}

fn write_json_report(
    port: &str,
    variant: RadioVariant,
    firmware: &str,
    format: &ExportFormat,
    output: &Path,
    plan: &FrequencyPlan,
    out: &mut dyn Write,
) -> Result<(), RadioError> {
    let output = output.display().to_string();
    let report = ExportReport {
        schema_version: RADIO_EXPORT_JSON_SCHEMA,
        command: "radio export",
        port,
        variant: variant.display_name(),
        firmware,
        format: format.as_str(),
        output: &output,
        channel_count: plan.channel_count(),
        plan,
    };

    serde_json::to_writer_pretty(&mut *out, &report).context(JsonReportSnafu)?;
    writeln!(out).context(IoSnafu)?;
    Ok(())
}

/// Serializes a plan to the requested format.
pub(crate) fn serialize_plan(
    plan: &FrequencyPlan,
    format: &ExportFormat,
) -> Result<String, RadioError> {
    match format {
        ExportFormat::Toml => plan.to_toml().context(SyntoniaSnafu),
        ExportFormat::Json => plan.to_json().context(SyntoniaSnafu),
        ExportFormat::Csv => Ok(export_native_csv(plan)),
        ExportFormat::ChirpCsv => Ok(export_chirp_csv(plan)),
    }
}

// ---------------------------------------------------------------------------
// Native CSV (simple format)
// ---------------------------------------------------------------------------

/// Exports a plan as a simple CSV with the most useful fields.
pub(crate) fn export_native_csv(plan: &FrequencyPlan) -> String {
    use std::fmt::Write;
    let mut out = String::from("Index,Name,RX Freq (MHz),TX Freq (MHz),Tone,Power\n");
    for ch in &plan.channels {
        let tx = ch.tx_freq.unwrap_or(ch.rx_freq);
        let _ = writeln!(
            out,
            "{},{},{:.6},{:.6},{},{}",
            ch.index,
            csv_escape(&ch.name),
            ch.rx_freq.as_mhz_f64(),
            tx.as_mhz_f64(),
            format_tone_csv(ch.tone),
            format_power_csv(ch.power),
        );
    }
    out
}

// ---------------------------------------------------------------------------
// CHIRP-compatible CSV (20 columns)
// ---------------------------------------------------------------------------

const CHIRP_HEADER: &str = "Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,\
    DtcsCode,DtcsPolarity,RxDtcsCode,Mode,TStep,Skip,Power,Comment,\
    URCALL,RPT1CALL,RPT2CALL,DVCODE";

/// Exports a plan as a CHIRP-compatible 20-column CSV.
pub(crate) fn export_chirp_csv(plan: &FrequencyPlan) -> String {
    use std::fmt::Write;
    let mut out = String::from(CHIRP_HEADER);
    out.push('\n');

    for ch in &plan.channels {
        let (duplex, offset_mhz) = chirp_duplex_offset(ch);
        let (tone_mode, r_tone, c_tone, dtcs_code, dtcs_polarity) = chirp_tone_fields(ch);
        let mode = match ch.bandwidth {
            Bandwidth::Narrow => "NFM",
            _ => "FM",
        };
        let skip = match ch.scan {
            ScanMode::Skip => "S",
            _ => "",
        };
        let power = format_power_csv(ch.power);

        let _ = writeln!(
            out,
            "{},{},{:.6},{},{:.6},{},{},{},{},{},{},{},5.00,{},{},,,,,",
            ch.index,
            csv_escape(&ch.name),
            ch.rx_freq.as_mhz_f64(),
            duplex,
            offset_mhz,
            tone_mode,
            r_tone,
            c_tone,
            dtcs_code,
            dtcs_polarity,
            dtcs_code, // RxDtcsCode mirrors DtcsCode
            mode,
            skip,
            power,
        );
    }
    out
}

fn chirp_duplex_offset(ch: &Channel) -> (&'static str, f64) {
    use syntonia::FrequencyOffset;
    match ch.offset {
        FrequencyOffset::Plus(f) => ("+", f.as_mhz_f64()),
        FrequencyOffset::Minus(f) => ("-", f.as_mhz_f64()),
        FrequencyOffset::Split(_) => ("split", {
            let tx = ch.tx_freq.unwrap_or(ch.rx_freq);
            tx.as_mhz_f64()
        }),
        _ => ("", 0.0),
    }
}

fn chirp_tone_fields(ch: &Channel) -> (&'static str, String, String, String, &'static str) {
    match ch.tone {
        ToneMode::Ctcss(tone) => (
            "Tone",
            format!("{:.1}", tone.as_hz()),
            format!("{:.1}", tone.as_hz()),
            "023".to_string(),
            "NN",
        ),
        ToneMode::Dcs(code, polarity) => {
            use syntonia::DcsPolarity;
            let pol = match polarity {
                DcsPolarity::Inverted => "RR",
                _ => "NN",
            };
            (
                "DTCS",
                "88.5".to_string(),
                "88.5".to_string(),
                format!("{:03}", code.as_code()),
                pol,
            )
        }
        _ => (
            "",
            "88.5".to_string(),
            "88.5".to_string(),
            "023".to_string(),
            "NN",
        ),
    }
}

fn format_tone_csv(tone: ToneMode) -> String {
    match tone {
        ToneMode::Ctcss(t) => format!("{:.1} Hz", t.as_hz()),
        ToneMode::Dcs(code, _) => format!("DCS {:03}", code.as_code()),
        _ => String::new(),
    }
}

const fn format_power_csv(power: PowerLevel) -> &'static str {
    match power {
        PowerLevel::Mid => "Mid",
        PowerLevel::Low => "Low",
        _ => "High",
    }
}

/// Escapes a CSV field if it contains special characters.
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use koinon::Frequency;
    use syntonia::tone::CtcssTone;
    use syntonia::types::FrequencyOffset;

    use super::*;
    use crate::radio::{DetectedRadio, Session};

    fn sample_plan() -> FrequencyPlan {
        FrequencyPlan {
            name: "Test".to_string(),
            radio_model: Some("Baofeng UV-5R".to_string()),
            channels: vec![
                Channel {
                    index: 0,
                    name: "CALL".to_string(),
                    rx_freq: Frequency::hz(146_520_000),
                    tx_freq: None,
                    offset: FrequencyOffset::None,
                    tone: ToneMode::None,
                    power: PowerLevel::High,
                    bandwidth: Bandwidth::Wide,
                    scan: ScanMode::Include,
                    busy_lock: false,
                },
                Channel {
                    index: 1,
                    name: "RPT-IN".to_string(),
                    rx_freq: Frequency::hz(147_060_000),
                    tx_freq: Some(Frequency::hz(147_660_000)),
                    offset: FrequencyOffset::Plus(Frequency::khz(600)),
                    tone: ToneMode::Ctcss(CtcssTone::new(100.0).unwrap()),
                    power: PowerLevel::High,
                    bandwidth: Bandwidth::Wide,
                    scan: ScanMode::Include,
                    busy_lock: false,
                },
            ],
            created: None,
        }
    }

    struct FakeHardware {
        plan: FrequencyPlan,
    }

    impl Hardware for FakeHardware {
        fn detect_radios(&self) -> Result<Vec<DetectedRadio>, RadioError> {
            Ok(vec![DetectedRadio {
                variant: RadioVariant::Uv5r,
                port: "/dev/ttyUSB0".to_string(),
                firmware: "BFB297".to_string(),
                warnings: Vec::new(),
            }])
        }

        fn open(&self, _port: &str) -> Result<Box<dyn Session>, RadioError> {
            Ok(Box::new(FakeSession {
                plan: self.plan.clone(),
            }))
        }
    }

    struct FakeSession {
        plan: FrequencyPlan,
    }

    impl Session for FakeSession {
        fn variant(&self) -> RadioVariant {
            RadioVariant::Uv5r
        }

        fn download_image(&mut self, on_block: &dyn Fn(u16, u16)) -> Result<Vec<u8>, RadioError> {
            on_block(128, 128);
            Ok(vec![0; 16])
        }

        fn upload_image(
            &mut self,
            _data: &[u8],
            _on_block: &dyn Fn(u16, u16),
        ) -> Result<(), RadioError> {
            Ok(())
        }

        fn decode_channels(&self, _image: &[u8]) -> Result<Vec<Channel>, RadioError> {
            Ok(self.plan.channels.clone())
        }

        fn encode_channels(&self, _channels: &[Channel]) -> Result<Vec<u8>, RadioError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn chirp_csv_has_20_columns() {
        let plan = sample_plan();
        let csv = export_chirp_csv(&plan);
        let lines: Vec<&str> = csv.lines().collect();

        // Header
        assert_eq!(
            lines[0].split(',').count(),
            20,
            "header should have 20 columns"
        );

        // Data rows
        for (i, line) in lines.iter().skip(1).enumerate() {
            assert_eq!(
                line.split(',').count(),
                20,
                "row {i} should have 20 columns"
            );
        }
    }

    #[test]
    fn native_csv_has_expected_header() {
        let plan = sample_plan();
        let csv = export_native_csv(&plan);
        let header = csv.lines().next().unwrap();
        assert_eq!(header, "Index,Name,RX Freq (MHz),TX Freq (MHz),Tone,Power");
    }

    #[test]
    fn chirp_csv_tone_column_for_ctcss() {
        let plan = sample_plan();
        let csv = export_chirp_csv(&plan);
        let rpt_line = csv.lines().nth(2).unwrap();
        let cols: Vec<&str> = rpt_line.split(',').collect();
        assert_eq!(cols[5], "Tone"); // Tone mode
        assert_eq!(cols[6], "100.0"); // rToneFreq
    }

    #[test]
    fn run_json_outputs_export_completion_report() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("channels.csv");
        let hw = FakeHardware {
            plan: sample_plan(),
        };
        let mut out = Vec::new();

        run(None, true, &ExportFormat::Csv, Some(&output), &hw, &mut out).unwrap();

        assert!(output.exists());
        let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["command"], "radio export");
        assert_eq!(report["port"], "/dev/ttyUSB0");
        assert_eq!(report["variant"], "Baofeng UV-5R");
        assert_eq!(report["firmware"], "BFB297");
        assert_eq!(report["format"], "csv");
        assert_eq!(report["output"], output.display().to_string());
        assert_eq!(report["channel_count"], 2);
        assert_eq!(report["plan"]["name"], "Baofeng UV-5R export");
    }
}
