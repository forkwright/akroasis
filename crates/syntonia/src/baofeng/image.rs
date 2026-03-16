//! In-memory representation of a radio EEPROM image.

use crate::error::{ImageTooSmallSnafu, SliceOutOfBoundsSnafu};

/// Size of a full UV-5R EEPROM image in bytes.
pub const IMAGE_SIZE: usize = 0x2000; // 8 KB

/// An in-memory copy of a radio's EEPROM contents.
///
/// Provides bounds-checked read and write access to the underlying bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryImage {
    data: Vec<u8>,
}

impl MemoryImage {
    /// Creates a new image from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the buffer is smaller than [`IMAGE_SIZE`].
    pub fn from_bytes(bytes: &[u8]) -> crate::error::Result<Self> {
        snafu::ensure!(
            bytes.len() >= IMAGE_SIZE,
            ImageTooSmallSnafu {
                actual: bytes.len(),
                required: IMAGE_SIZE
            }
        );
        Ok(Self {
            data: bytes.to_vec(),
        })
    }

    /// Creates a blank image filled with `0xFF` (erased EEPROM state).
    #[must_use]
    pub fn blank() -> Self {
        Self {
            data: vec![0xFF; IMAGE_SIZE],
        }
    }

    /// Returns a slice of the image at the given offset and length.
    ///
    /// # Errors
    ///
    /// Returns an error if the range exceeds the image size.
    #[allow(clippy::indexing_slicing)] // bounds checked by ensure! above
    pub fn slice(&self, offset: usize, len: usize) -> crate::error::Result<&[u8]> {
        let end = offset + len;
        snafu::ensure!(
            end <= self.data.len(),
            SliceOutOfBoundsSnafu {
                offset,
                len,
                size: self.data.len()
            }
        );
        Ok(&self.data[offset..end])
    }

    /// Writes bytes into the image at the given offset.
    ///
    /// # Errors
    ///
    /// Returns an error if the write would exceed the image size.
    #[allow(clippy::indexing_slicing)] // bounds checked by ensure! above
    pub fn write(&mut self, offset: usize, bytes: &[u8]) -> crate::error::Result<()> {
        let end = offset + bytes.len();
        snafu::ensure!(
            end <= self.data.len(),
            SliceOutOfBoundsSnafu {
                offset,
                len: bytes.len(),
                size: self.data.len()
            }
        );
        self.data[offset..end].copy_from_slice(bytes);
        Ok(())
    }

    /// Returns the full image as a byte slice.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}
