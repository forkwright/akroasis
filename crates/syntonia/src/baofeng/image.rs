//! EEPROM memory image for the Baofeng UV-5R family.
//!
//! [`MemoryImage`] is a flat byte buffer representing the radio's EEPROM
//! contents. Protocol code writes downloaded blocks into an image, and reads
//! blocks from an image during upload.

/// A flat EEPROM memory image.
#[derive(Debug, Clone)]
pub struct MemoryImage {
    data: Vec<u8>,
}

impl MemoryImage {
    /// Create a zero-filled image of `size` bytes.
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0u8; size],
        }
    }

    /// Create an image from an existing byte vector.
    #[must_use]
    pub const fn from_vec(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Read a slice of bytes starting at `addr`.
    ///
    /// Returns `None` if the range exceeds the image size.
    #[must_use]
    pub fn read_bytes(&self, addr: u16, len: usize) -> Option<&[u8]> {
        let start = usize::from(addr);
        let end = start.checked_add(len)?;
        self.data.get(start..end)
    }

    /// Write `data` into the image starting at `addr`.
    ///
    /// Returns `false` if the range exceeds the image size.
    pub fn write_bytes(&mut self, addr: u16, data: &[u8]) -> bool {
        let start = usize::from(addr);
        let Some(end) = start.checked_add(data.len()) else {
            return false;
        };
        if end > self.data.len() {
            return false;
        }
        // INVARIANT: bounds checked above — start..end is within self.data.len().
        #[allow(clippy::indexing_slicing)]
        self.data[start..end].copy_from_slice(data);
        true
    }

    /// Total image size in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the image is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// View the entire image as a byte slice.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }
}
