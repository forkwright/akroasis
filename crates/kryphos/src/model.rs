//! Vault data model types.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Type of credential stored in the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CredentialType {
    /// API key for external services.
    ApiKey,
    /// Pre-shared key for mesh networks.
    Psk,
    /// Certificate (PEM or DER).
    Certificate,
    /// Radio programming key.
    RadioKey,
    /// Arbitrary secret blob.
    Custom(CompactString),
}

/// Metadata for a vault entry (never contains decrypted secrets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryMetadata {
    /// Human-readable name.
    pub name: CompactString,
    /// What kind of credential this is.
    pub credential_type: CredentialType,
    /// Unix milliseconds when the entry was created.
    pub created_at_ms: i64,
    /// Unix milliseconds when the entry was last rotated.
    pub rotated_at_ms: Option<i64>,
    /// User-defined tags.
    pub tags: Vec<CompactString>,
}

/// Internal representation stored encrypted in fjall.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VaultEntryInner {
    /// Entry metadata.
    pub metadata: EntryMetadata,
    /// Secret bytes.
    pub secret: Vec<u8>,
}

/// A decrypted vault entry returned to callers.
#[derive(Debug, Clone)]
pub struct VaultEntry {
    /// Entry metadata.
    pub metadata: EntryMetadata,
    /// The decrypted secret bytes.
    pub secret: Vec<u8>,
}

/// KDF parameters stored in the vault header.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory cost in KiB.
    pub m_cost: u32,
    /// Time cost (iterations).
    pub t_cost: u32,
    /// Parallelism.
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_cost: 65536,
            t_cost: 3,
            p_cost: 4,
        }
    }
}

/// Vault header stored on disk alongside the fjall data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultHeader {
    /// Header format version.
    pub version: u8,
    /// Random salt for KDF.
    pub salt: Vec<u8>,
    /// KDF parameters.
    pub kdf_params: KdfParams,
    /// Verification tag: encrypt a known plaintext to detect wrong passphrase.
    pub verify_tag: Vec<u8>,
}

/// Magic bytes at the start of the header file.
pub(crate) const HEADER_MAGIC: &[u8; 8] = b"KRYPHOS\0";

/// Current header version.
pub(crate) const HEADER_VERSION: u8 = 1;

/// Known plaintext used to verify the passphrase on open.
pub(crate) const VERIFY_PLAINTEXT: &[u8] = b"kryphos-vault-verify";
