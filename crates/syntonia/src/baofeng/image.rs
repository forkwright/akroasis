//! Raw EEPROM memory image for Baofeng radios.

/// A raw EEPROM memory image stored as a byte vector.
#[derive(Debug, Clone)]
pub struct MemoryImage {
    data: Vec<u8>,
}

impl MemoryImage {
    /// Create a new image filled with 0xFF (erased EEPROM state).
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0xFF; size],
        }
    }

    /// Construct FROM an existing byte vector.
    pub const fn from_bytes(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Read a slice of bytes starting at the given EEPROM address.
    ///
    /// # Panics
    ///
    /// Panics if `addr + len` exceeds the image size. Callers are expected to
    /// stay within the 8 KiB EEPROM layout; this is a debug-only invariant.
    #[expect(
        clippy::indexing_slicing,
        reason = "EEPROM layout is fixed at image construction; caller invariant validated by unit tests"
    )]
    pub(crate) fn read_bytes(&self, addr: u16, len: usize) -> &[u8] {
        let start = usize::from(addr);
        &self.data[start..start + len]
    }

    /// Write bytes to the given EEPROM address.
    ///
    /// # Panics
    ///
    /// Panics if `addr + data.len()` exceeds the image size. Callers are
    /// expected to stay within the 8 KiB EEPROM layout.
    #[expect(
        clippy::indexing_slicing,
        reason = "EEPROM layout is fixed at image construction; caller invariant validated by unit tests"
    )]
    pub(crate) fn write_bytes(&mut self, addr: u16, data: &[u8]) {
        let start = usize::from(addr);
        self.data[start..start + data.len()].copy_from_slice(data);
    }

    /// Return the image size in bytes.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Return true if the image is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Return the raw image bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }
}
