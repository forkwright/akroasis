//! `akroasis radio program` — write a frequency plan to a radio.

use std::io::Write;
use std::path::Path;

use dialoguer::Confirm;
use snafu::ResultExt;
use syntonia::{FrequencyPlan, ValidationIssue, validate_plan};

use super::errors::{IoSnafu, RadioError, ReadFileSnafu, SyntoniaSnafu};
use super::progress;
use super::{Hardware, resolve_target};

/// Runs the program subcommand.
pub(crate) fn run(
    port: Option<&str>,
    plan_path: &Path,
    hw: &dyn Hardware,
    out: &mut dyn Write,
) -> Result<(), RadioError> {
    let plan = load_plan(plan_path)?;

    let target = resolve_target(port, hw)?;
    let variant = target.variant;
    let constraints = variant.constraints();

    let issues = validate_plan(&plan, &constraints);
    let errors: Vec<&ValidationIssue> = issues
        .iter()
        .filter(|i| matches!(i, ValidationIssue::Error(_)))
        .collect();
    let warnings: Vec<&ValidationIssue> = issues
        .iter()
        .filter(|i| matches!(i, ValidationIssue::Warning(_)))
        .collect();

    if !errors.is_empty() {
        for e in &errors {
            writeln!(out, "error: {e:?}").context(IoSnafu)?;
        }
        return Err(RadioError::ValidationFailed {
            message: format!("{} validation errors", errors.len()),
        });
    }

    for w in &warnings {
        writeln!(out, "warning: {w:?}").context(IoSnafu)?;
    }

    writeln!(
        out,
        "About to program {} on {}. {} channels.",
        variant.display_name(),
        target.port,
        plan.channel_count(),
    )
    .context(IoSnafu)?;

    let confirmed = Confirm::new()
        .with_prompt("Continue?")
        .default(false)
        .interact()
        .map_err(|e| RadioError::Plan {
            message: format!("prompt failed: {e}"),
        })?;

    if !confirmed {
        return Err(RadioError::WriteAborted);
    }

    let mut session = hw.open(&target.port)?;
    let image = session.encode_channels(&plan.channels)?;

    // Upload
    let pb = progress::upload_bar(128);
    session.upload_image(&image, &|done, total| {
        pb.set_length(u64::from(total));
        pb.set_position(u64::from(done));
    })?;
    pb.finish_and_clear();

    // Verify: re-download and compare
    writeln!(out, "Verifying...").context(IoSnafu)?;
    let pb = progress::download_bar(128);
    let readback = session.download_image(&|done, total| {
        pb.set_length(u64::from(total));
        pb.set_position(u64::from(done));
    })?;
    pb.finish_and_clear();

    if readback != image {
        return Err(RadioError::VerificationFailed {
            message: "readback does not match uploaded image".to_string(),
        });
    }

    writeln!(
        out,
        "Programming complete. {} channels written.",
        plan.channel_count()
    )
    .context(IoSnafu)?;

    Ok(())
}

/// Loads a frequency plan from a file, detecting format by extension.
pub(crate) fn load_plan(path: &Path) -> Result<FrequencyPlan, RadioError> {
    let content = std::fs::read_to_string(path).context(ReadFileSnafu {
        path: path.to_path_buf(),
    })?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "toml" => FrequencyPlan::from_toml(&content).context(SyntoniaSnafu),
        "json" => FrequencyPlan::from_json(&content).context(SyntoniaSnafu),
        other => Err(RadioError::UnsupportedFormat {
            ext: other.to_string(),
        }),
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
    use syntonia::{Bandwidth, Channel, PowerLevel, ScanMode, ToneMode, types::FrequencyOffset};

    use super::*;

    #[test]
    fn load_plan_from_toml_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.toml");

        let plan = FrequencyPlan {
            name: "Test".to_string(),
            radio_model: Some("Baofeng UV-5R".to_string()),
            channels: vec![Channel {
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
            }],
            created: None,
        };

        let toml = plan.to_toml().unwrap();
        std::fs::write(&path, &toml).unwrap();

        let loaded = load_plan(&path).unwrap();
        assert_eq!(loaded.channel_count(), 1);
        assert_eq!(loaded.channels[0].name, "CALL");
    }

    #[test]
    fn load_plan_unsupported_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.xml");
        std::fs::write(&path, "<plan/>").unwrap();

        let result = load_plan(&path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("xml"));
    }

    #[test]
    fn validation_catches_out_of_band_frequency() {
        let plan = FrequencyPlan {
            name: "Bad".to_string(),
            radio_model: None,
            channels: vec![Channel {
                index: 0,
                name: "OOB".to_string(),
                rx_freq: Frequency::mhz(100), // out of band for UV-5R
                tx_freq: None,
                offset: FrequencyOffset::None,
                tone: ToneMode::None,
                power: PowerLevel::High,
                bandwidth: Bandwidth::Wide,
                scan: ScanMode::Include,
                busy_lock: false,
            }],
            created: None,
        };

        let constraints = crate::radio::RadioVariant::Uv5r.constraints();
        let issues = validate_plan(&plan, &constraints);
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, ValidationIssue::Error(_))),
            "should have validation errors for out-of-band frequency"
        );
    }
}
