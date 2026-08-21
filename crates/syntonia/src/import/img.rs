//! CHIRP `.img` file import (raw EEPROM dumps).

use std::path::Path;

use snafu::{ResultExt, Snafu};

use crate::baofeng::codec::{self, CodecError};
use crate::baofeng::image::MemoryImage;
use crate::plan::FrequencyPlan;

/// Standard UV-5R .img file size: 0x1800 main block + 8-byte ident header.
const UV5R_STANDARD_SIZE: usize = 0x1808;

/// UV-5R .img with aux block: 0x1C00 data + 8-byte ident header.
const UV5R_AUX_SIZE: usize = 0x1C08;

/// Ident header length at the start of CHIRP .img files.
const IDENT_HEADER_LEN: usize = 8;

/// Errors from `.img` file import.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum ImgImportError {
    /// Failed to read the `.img` file from disk.
    #[snafu(display("failed to read .img file at {}: {source}", path.display()))]
    ReadFile {
        /// Path to the file.
        path: std::path::PathBuf,
        /// The I/O error.
        source: std::io::Error,
    },

    /// File size does not match any known radio model.
    #[snafu(display(
        "unsupported .img file size: {size} bytes (expected {UV5R_STANDARD_SIZE} for UV-5R standard or {UV5R_AUX_SIZE} for UV-5R with aux block)"
    ))]
    UnsupportedImageSize {
        /// The actual file size.
        size: usize,
    },

    /// EEPROM codec error during channel decoding.
    #[snafu(display("codec error during .img import: {source}"))]
    Codec {
        /// The underlying codec error.
        source: CodecError,
    },
}

/// Import a CHIRP `.img` file from disk.
///
/// # Errors
///
/// Returns `ImgImportError` if the file cannot be read, has an unsupported
/// size, or contains invalid channel data.
pub fn import_img(path: &Path) -> Result<FrequencyPlan, ImgImportError> {
    // WHY: reject by size from the directory entry before allocating. A `.img`
    // is a fixed-size EEPROM dump of a few kilobytes, so reading first and
    // checking after let any file the user pointed at — a disk image, a core
    // dump — be pulled into memory in full only to be rejected.
    let metadata = std::fs::metadata(path).context(ReadFileSnafu {
        path: path.to_path_buf(),
    })?;
    let size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if size != UV5R_STANDARD_SIZE && size != UV5R_AUX_SIZE {
        return Err(ImgImportError::UnsupportedImageSize { size });
    }

    // NOTE: `import_img_bytes` re-checks the length, so a file that changes
    // size between the stat above and the read below is still rejected.

    let data = std::fs::read(path).context(ReadFileSnafu {
        path: path.to_path_buf(),
    })?;
    import_img_bytes(&data)
}

/// Import a CHIRP `.img` from raw bytes.
///
/// # Errors
///
/// Returns `ImgImportError` if the data has an unsupported size or contains
/// invalid channel data.
#[expect(
    clippy::indexing_slicing,
    reason = "Slice at IDENT_HEADER_LEN is safe after size validation"
)]
pub fn import_img_bytes(data: &[u8]) -> Result<FrequencyPlan, ImgImportError> {
    let model = match data.len() {
        UV5R_STANDARD_SIZE => "UV-5R",
        UV5R_AUX_SIZE => "UV-5R (aux)",
        size => return Err(ImgImportError::UnsupportedImageSize { size }),
    };

    // WHY: CHIRP .img files prepend an 8-byte ident header before EEPROM data.
    // EEPROM address 0x0000 maps to file offset 8.
    let eeprom_data = &data[IDENT_HEADER_LEN..];
    let image = MemoryImage::from_bytes(eeprom_data.to_vec());

    let mut plan = codec::decode_all_channels(&image).context(CodecSnafu)?;
    plan.radio_model = Some(model.to_string());

    Ok(plan)
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use stoicheion::Frequency;

    use super::*;
    use crate::baofeng::codec;
    use crate::baofeng::image::MemoryImage;
    use crate::channel::Channel;
    use crate::plan::FrequencyPlan;
    use crate::tone::ToneMode;
    use crate::types::{Bandwidth, FrequencyOffset, PowerLevel, ScanMode};

    fn make_img_bytes(eeprom_size: usize) -> Vec<u8> {
        let mut image = MemoryImage::new(eeprom_size);
        let plan = FrequencyPlan {
            name: String::new(),
            radio_model: None,
            channels: vec![Channel {
                index: 0,
                name: "TEST".to_string(),
                rx_freq: Frequency::hz(146_520_000),
                tx_freq: Some(Frequency::hz(146_520_000)),
                offset: FrequencyOffset::None,
                tone: ToneMode::None,
                power: PowerLevel::High,
                bandwidth: Bandwidth::Wide,
                scan: ScanMode::Include,
                busy_lock: false,
            }],
            created: None,
        };
        codec::encode_all_channels(&plan, &mut image).unwrap();

        let mut data = vec![0u8; IDENT_HEADER_LEN];
        data.extend_from_slice(image.as_slice());
        data
    }

    // WHY: `import_img` used to read the file in full and check its length
    // afterwards, so pointing it at a disk image or a core dump pulled the
    // whole thing into memory only to reject it. The size now comes from the
    // directory entry. A 64 MiB sparse file is large enough to be an obvious
    // mistake and costs no disk to create; the property being fixed is that
    // nothing of it is read, which the error alone cannot show — see the
    // commit message for the differential.
    #[test]
    fn an_oversized_file_is_rejected_by_its_size_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.img");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(64 * 1024 * 1024).unwrap();
        drop(file);

        let err = import_img(&path).unwrap_err();
        assert!(
            matches!(
                err,
                ImgImportError::UnsupportedImageSize { size: 67_108_864 }
            ),
            "expected UnsupportedImageSize with the on-disk length, got: {err:?}"
        );
    }

    // WHY: the falsifying sibling — a file of a supported size must still be
    // read and decoded, so the stat is a gate rather than a refusal.
    #[test]
    fn a_supported_size_is_still_read_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("uv5r.img");
        std::fs::write(&path, make_img_bytes(0x1800)).unwrap();

        let plan = import_img(&path).unwrap();
        assert_eq!(plan.radio_model.as_deref(), Some("UV-5R"));
        assert_eq!(plan.channel_count(), 1);
    }

    #[test]
    fn a_missing_file_is_reported_as_a_read_failure() {
        let dir = tempfile::tempdir().unwrap();
        let err = import_img(&dir.path().join("absent.img")).unwrap_err();
        assert!(
            matches!(err, ImgImportError::ReadFile { .. }),
            "expected ReadFile, got: {err:?}"
        );
    }

    #[test]
    fn standard_size_accepted() {
        let data = make_img_bytes(0x1800);
        assert_eq!(data.len(), UV5R_STANDARD_SIZE);
        let plan = import_img_bytes(&data).unwrap();
        assert_eq!(plan.channel_count(), 1);
        assert_eq!(plan.radio_model.as_deref(), Some("UV-5R"));
    }

    #[test]
    fn aux_size_accepted() {
        let data = make_img_bytes(0x1C00);
        assert_eq!(data.len(), UV5R_AUX_SIZE);
        let plan = import_img_bytes(&data).unwrap();
        assert_eq!(plan.channel_count(), 1);
        assert_eq!(plan.radio_model.as_deref(), Some("UV-5R (aux)"));
    }

    #[test]
    fn wrong_size_rejected() {
        let data = vec![0u8; 1000];
        let err = import_img_bytes(&data).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unsupported"), "error: {msg}");
        assert!(msg.contains("1000"), "error should mention size: {msg}");
    }

    #[test]
    fn ident_header_stripped_correctly() {
        let data = make_img_bytes(0x1800);

        // Verify the ident header doesn't affect channel decoding
        let plan = import_img_bytes(&data).unwrap();
        let ch = plan.channel(0).unwrap();
        assert_eq!(ch.rx_freq, Frequency::hz(146_520_000));
        assert_eq!(ch.name, "TEST");
    }

    #[test]
    fn channels_decoded_from_img() {
        let mut image = MemoryImage::new(0x1800);
        let channels = vec![
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
                tone: ToneMode::Ctcss(crate::tone::CtcssTone::new(100.0).unwrap()),
                power: PowerLevel::High,
                bandwidth: Bandwidth::Wide,
                scan: ScanMode::Include,
                busy_lock: false,
            },
        ];
        let plan = FrequencyPlan {
            name: String::new(),
            radio_model: None,
            channels,
            created: None,
        };
        codec::encode_all_channels(&plan, &mut image).unwrap();

        let mut data = vec![0u8; IDENT_HEADER_LEN];
        data.extend_from_slice(image.as_slice());

        let imported = import_img_bytes(&data).unwrap();
        assert_eq!(imported.channel_count(), 2);
        assert_eq!(imported.channel(0).unwrap().name, "CALL");
        assert_eq!(imported.channel(1).unwrap().name, "RPT");
    }
}
