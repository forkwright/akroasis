//! `akroasis radio export` — read radio and export channels to a file.

use std::fmt::Write;
use std::path::Path;

use snafu::ResultExt;
use syntonia::{Bandwidth, Channel, FrequencyPlan, PowerLevel, ScanMode, ToneMode};

use super::errors::{RadioError, SyntoniaSnafu, WriteFileSnafu};
use super::progress;
use super::{ExportFormat, Hardware, resolve_target};

/// Runs the export subcommand.
pub fn run(
    port: Option<&str>,
    format: &ExportFormat,
    output: Option<&Path>,
    hw: &dyn Hardware,
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
            println!(
                "Exported {} channels to {}",
                plan.channel_count(),
                path.display()
            );
        }
        None => print!("{content}"),
    }

    Ok(())
}

/// Serializes a plan to the requested format.
pub fn serialize_plan(plan: &FrequencyPlan, format: &ExportFormat) -> Result<String, RadioError> {
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
pub fn export_native_csv(plan: &FrequencyPlan) -> String {
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
pub fn export_chirp_csv(plan: &FrequencyPlan) -> String {
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
                format!("{:03}", code.code()),
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
        ToneMode::Dcs(code, _) => format!("DCS {:03}", code.code()),
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
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests {
    use koinon::Frequency;
    use syntonia::tone::CtcssTone;
    use syntonia::types::FrequencyOffset;

    use super::*;

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
}
