//! Vault data model: entries, headers, and credential types.

use compact_str::CompactString;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// Size of the Argon2id salt in bytes.
pub const SALT_LEN: usize = 16;

/// Size of the ChaCha20-Poly1305 nonce in bytes.
pub const NONCE_LEN: usize = 12;

/// Current vault format version.
pub const VAULT_VERSION: u32 = 1;

/// The kind of credential stored in a [`VaultEntry`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CredentialType {
    /// LLM provider, weather service, or other API key.
    ApiKey,
    /// Pre-shared key for mesh radio networks.
    Psk,
    /// X.509 or other certificate (PEM/DER bytes).
    Certificate,
    /// Radio programming key or codeplug encryption key.
    RadioKey,
    /// Arbitrary secret that does not fit other categories.
    Custom {
        /// Caller-defined label for this credential kind.
        label: CompactString,
    },
}

impl std::fmt::Display for CredentialType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey => f.write_str("api-key"),
            Self::Psk => f.write_str("psk"),
            Self::Certificate => f.write_str("certificate"),
            Self::RadioKey => f.write_str("radio-key"),
            Self::Custom { label } => write!(f, "custom({label})"),
        }
    }
}

/// Metadata attached to a [`VaultEntry`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryMetadata {
    /// When the entry was first stored.
    pub created_at: Timestamp,
    /// When the credential was last rotated, if ever.
    pub rotated_at: Option<Timestamp>,
    /// Free-form tags for filtering and grouping.
    pub tags: Vec<CompactString>,
}

/// A single credential stored in the vault.
///
/// The `encrypted_data` field holds the ciphertext produced by
/// ChaCha20-Poly1305. Decryption requires the [`VaultKey`](super::VaultKey).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultEntry {
    /// Human-readable name for this credential.
    pub name: CompactString,
    /// What kind of credential this is.
    pub credential_type: CredentialType,
    /// ChaCha20-Poly1305 ciphertext (includes 16-byte authentication tag).
    pub encrypted_data: Vec<u8>,
    /// Associated metadata.
    pub metadata: EntryMetadata,
}

/// Argon2id key-derivation parameters stored in the vault header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory cost in KiB.
    pub memory_cost_kib: u32,
    /// Number of passes over memory.
    pub time_cost: u32,
    /// Degree of parallelism.
    pub parallelism: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            memory_cost_kib: 65_536,
            time_cost: 3,
            parallelism: 4,
        }
    }
}

/// Header written at the start of a sealed vault file.
///
/// Contains everything needed to re-derive the symmetric key from
/// a passphrase (salt + KDF parameters) and decrypt entries (nonce).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultHeader {
    /// Format version (currently [`VAULT_VERSION`]).
    pub version: u32,
    /// Random salt for Argon2id key derivation.
    pub salt: [u8; SALT_LEN],
    /// Nonce for the outer ChaCha20-Poly1305 envelope.
    pub nonce: [u8; NONCE_LEN],
    /// Argon2id parameters used to derive the vault key.
    pub kdf_params: KdfParams,
}

impl VaultHeader {
    /// Creates a header with the current version, the given salt and nonce,
    /// and default KDF parameters.
    #[must_use]
    pub fn new(salt: [u8; SALT_LEN], nonce: [u8; NONCE_LEN]) -> Self {
        Self {
            version: VAULT_VERSION,
            salt,
            nonce,
            kdf_params: KdfParams::default(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn sample_metadata() -> EntryMetadata {
        EntryMetadata {
            created_at: Timestamp::UNIX_EPOCH,
            rotated_at: None,
            tags: vec![CompactString::from("test")],
        }
    }

    fn sample_entry(cred_type: CredentialType) -> VaultEntry {
        VaultEntry {
            name: CompactString::from("test-credential"),
            credential_type: cred_type,
            encrypted_data: vec![0xDE, 0xAD, 0xBE, 0xEF],
            metadata: sample_metadata(),
        }
    }

    // -----------------------------------------------------------------
    // CredentialType construction and display
    // -----------------------------------------------------------------

    #[test]
    fn credential_type_api_key_displays_correctly() {
        assert_eq!(CredentialType::ApiKey.to_string(), "api-key");
    }

    #[test]
    fn credential_type_psk_displays_correctly() {
        assert_eq!(CredentialType::Psk.to_string(), "psk");
    }

    #[test]
    fn credential_type_certificate_displays_correctly() {
        assert_eq!(CredentialType::Certificate.to_string(), "certificate");
    }

    #[test]
    fn credential_type_radio_key_displays_correctly() {
        assert_eq!(CredentialType::RadioKey.to_string(), "radio-key");
    }

    #[test]
    fn credential_type_custom_displays_label() {
        let ct = CredentialType::Custom {
            label: CompactString::from("wireguard"),
        };
        assert_eq!(ct.to_string(), "custom(wireguard)");
    }

    // -----------------------------------------------------------------
    // Serde round-trips
    // -----------------------------------------------------------------

    #[test]
    fn credential_type_serde_round_trip_api_key() {
        let ct = CredentialType::ApiKey;
        let json = serde_json::to_string(&ct).unwrap();
        let back: CredentialType = serde_json::from_str(&json).unwrap();
        assert_eq!(ct, back);
    }

    #[test]
    fn credential_type_serde_round_trip_custom() {
        let ct = CredentialType::Custom {
            label: CompactString::from("mesh-psk"),
        };
        let json = serde_json::to_string(&ct).unwrap();
        let back: CredentialType = serde_json::from_str(&json).unwrap();
        assert_eq!(ct, back);
    }

    #[test]
    fn vault_entry_serde_round_trip() {
        let entry = sample_entry(CredentialType::Psk);
        let json = serde_json::to_string(&entry).unwrap();
        let back: VaultEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }

    #[test]
    fn vault_header_serde_round_trip() {
        let header = VaultHeader::new([0xAA; SALT_LEN], [0xBB; NONCE_LEN]);
        let json = serde_json::to_string(&header).unwrap();
        let back: VaultHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(header, back);
    }

    #[test]
    fn kdf_params_serde_round_trip() {
        let params = KdfParams::default();
        let json = serde_json::to_string(&params).unwrap();
        let back: KdfParams = serde_json::from_str(&json).unwrap();
        assert_eq!(params, back);
    }

    // -----------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------

    #[test]
    fn vault_header_new_uses_current_version() {
        let header = VaultHeader::new([0; SALT_LEN], [0; NONCE_LEN]);
        assert_eq!(header.version, VAULT_VERSION);
    }

    #[test]
    fn vault_header_new_uses_default_kdf_params() {
        let header = VaultHeader::new([0; SALT_LEN], [0; NONCE_LEN]);
        assert_eq!(header.kdf_params, KdfParams::default());
    }

    #[test]
    fn kdf_params_default_has_sensible_values() {
        let params = KdfParams::default();
        assert!(
            params.memory_cost_kib >= 16_384,
            "memory cost should be at least 16 MiB"
        );
        assert!(params.time_cost >= 1, "time cost must be positive");
        assert!(params.parallelism >= 1, "parallelism must be positive");
    }

    #[test]
    fn entry_metadata_with_rotation() {
        let meta = EntryMetadata {
            created_at: Timestamp::UNIX_EPOCH,
            rotated_at: Some(Timestamp::UNIX_EPOCH),
            tags: vec![CompactString::from("prod"), CompactString::from("rotated")],
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: EntryMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn vault_entry_with_empty_encrypted_data() {
        let entry = VaultEntry {
            name: CompactString::from("empty"),
            credential_type: CredentialType::ApiKey,
            encrypted_data: vec![],
            metadata: sample_metadata(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: VaultEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }
}
