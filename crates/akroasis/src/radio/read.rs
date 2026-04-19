//! `akroasis radio read` — download channels from a radio and display them.

use comfy_table::{Cell, ContentArrangement, Table};
use syntonia::{Bandwidth, Channel, PowerLevel, ScanMode, ToneMode};

use super::errors::RadioError;
use super::progress;
use super::{Hardware, resolve_target};

/// Runs the read subcommand.
pub(crate) fn run(port: Option<&str>, hw: &dyn Hardware) -> Result<(), RadioError> {
    let target = resolve_target(port, hw)?;
    let mut session = hw.open(&target.port)?;

    let total_blocks: u16 = 128;
    let pb = progress::download_bar(u64::from(total_blocks));
    let image = session.download_image(&|done, total| {
        pb.set_length(u64::from(total));
        pb.set_position(u64::from(done));
    })?;
    pb.finish_and_clear();

    let channels = session.decode_channels(&image)?;
    let programmed: Vec<&Channel> = channels
        .iter()
        .filter(|ch| ch.rx_freq.as_hz() > 0)
        .collect();

    print_channel_table(&programmed);
    println!(
        "{} channels programmed (of {} slots)",
        programmed.len(),
        session.variant().max_channels(),
    );

    Ok(())
}

/// Formats a channel table for display.
pub(crate) fn print_channel_table(channels: &[&Channel]) {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "Ch", "Name", "RX Freq", "TX Freq", "Tone", "Pwr", "BW", "Scan",
    ]);

    for ch in channels {
        let tx_display = ch
            .tx_freq
            .map_or_else(|| format!("{}", ch.rx_freq), |tx| format!("{tx}"));

        table.add_row(vec![
            Cell::new(format!("{:03}", ch.index + 1)),
            Cell::new(&ch.name),
            Cell::new(format!("{}", ch.rx_freq)),
            Cell::new(tx_display),
            Cell::new(format_tone(ch.tone)),
            Cell::new(format_power(ch.power)),
            Cell::new(format_bandwidth(ch.bandwidth)),
            Cell::new(format_scan(ch.scan)),
        ]);
    }

    println!("{table}");
}

/// Formats a channel table from owned channel references (for import display).
pub(crate) fn print_channel_table_owned(channels: &[Channel]) {
    let refs: Vec<&Channel> = channels.iter().collect();
    print_channel_table(&refs);
}

fn format_tone(tone: ToneMode) -> String {
    match tone {
        ToneMode::None => "\u{2014}".to_string(),
        ToneMode::Ctcss(t) => format!("{:.1} Hz", t.as_hz()),
        ToneMode::Dcs(code, _polarity) => format!("DCS {:03}", code.as_code()),
        _ => "?".to_string(),
    }
}

const fn format_power(power: PowerLevel) -> &'static str {
    match power {
        PowerLevel::High => "High",
        PowerLevel::Mid => "Mid",
        PowerLevel::Low => "Low",
        _ => "?",
    }
}

const fn format_bandwidth(bw: Bandwidth) -> &'static str {
    match bw {
        Bandwidth::Wide => "Wide",
        Bandwidth::Narrow => "Narrow",
        _ => "?",
    }
}

const fn format_scan(scan: ScanMode) -> &'static str {
    match scan {
        ScanMode::Include => "\u{2713}",
        ScanMode::Skip => "\u{2014}",
        _ => "?",
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use koinon::Frequency;
    use syntonia::tone::CtcssTone;
    use syntonia::types::FrequencyOffset;

    use super::*;

    fn sample_channels() -> Vec<Channel> {
        vec![
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
        ]
    }

    #[test]
    fn channel_table_displays_without_panic() {
        let channels = sample_channels();
        let refs: Vec<&Channel> = channels.iter().collect();
        print_channel_table(&refs);
    }

    #[test]
    fn tone_formatting() {
        assert_eq!(format_tone(ToneMode::None), "\u{2014}");

        let ctcss = ToneMode::Ctcss(CtcssTone::new(100.0).unwrap());
        assert_eq!(format_tone(ctcss), "100.0 Hz");
    }
}
