//! Vault data model: entries, headers, and credential types.

use chacha20poly1305::aead::{Aead, AeadCore, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use compact_str::CompactString;
use jiff::Timestamp;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use zeroize::Zeroizing;

use crate::error::{CryptoError, KeyParseSnafu};
use crate::key::{InstallationIdentity, SIGNING_KEY_LEN, SigningKey, VaultKey};

/// Size of the Argon2id salt in bytes.
pub const SALT_LEN: usize = 16;

/// Size of the ChaCha20-Poly1305 nonce in bytes.
pub const NONCE_LEN: usize = 12;

/// Current vault format version, written by [`VaultHeader::new`] into every
/// newly created vault.
///
/// WARNING: bumping this is a hard break for anything below
/// [`MIN_SUPPORTED_VAULT_VERSION`] — `open` rejects any header whose
/// `version` falls outside `MIN_SUPPORTED_VAULT_VERSION..=VAULT_VERSION`, so
/// a vault written under an older format simply fails to open under a newer
/// one. v2 folds together two independent changes that landed close
/// together: names, types, tags, status, and history moved from plaintext
/// fields into `encrypted_metadata`, and the fjall record key changed from
/// the plaintext name to a keyed-hash lookup key (forkwright/akroasis#215);
/// entry ciphertexts are ALSO now bound to their identity via AEAD
/// associated data (forkwright/akroasis#283). A true v1 store (predating
/// both changes) has neither the fields, the keys, nor the AAD binding v2
/// code expects — its migration path is re-initialization: read out entries
/// with the prior release, create a fresh vault, re-add them.
/// `envelope_version` (a per-ENTRY field, distinct from this header-level
/// version — see [`crate::crypto::ENTRY_ENVELOPE_VERSION`]) is the
/// finer-grained axis that DOES support transparent migration, for entries
/// written under a v2 header before the AAD binding existed: it defaults to
/// 0 via serde on a record that never had the field, and
/// [`crate::storage::Vault::get`] selects the correct AAD from it at decrypt
/// time. This IS the migration path forkwright/akroasis#283's Desired
/// Correction asked for, scoped to what it can actually promise once v2's
/// OWN entry/key-layout change (forkwright/akroasis#215) is accounted for:
/// a v2-header vault whose entries predate AAD binding opens and decrypts
/// transparently, with no separate `migrate` command and no operator
/// round-trip through an old release; a true v1 vault does not, because its
/// entry shape and fjall key derivation are unrelated to what this crate
/// reads today.
pub const VAULT_VERSION: u32 = 2;

/// Oldest vault header version [`crate::storage::Vault::open`] still
/// accepts.
///
/// Equal to [`VAULT_VERSION`], not lower: unlike the AAD-binding change,
/// v2's entry-shape and fjall-key-derivation change
/// (forkwright/akroasis#215) has no transparent migration path (see
/// [`VAULT_VERSION`]'s doc) — accepting a header below this floor would let
/// `open` succeed on a vault whose entries this build cannot correctly
/// locate or parse, failing confusingly deep in `get`/`list` instead of with
/// a clear, up-front `InvalidHeader`. A header above [`VAULT_VERSION`] is
/// from a NEWER binary's format this build predates; reject that too rather
/// than open it partially. Kept as a distinct constant (not inlined as
/// `VAULT_VERSION`) because the range-check pattern itself is real: a future
/// header-shape change that FUNDAMENTALLY differs from a floor-required
/// exact match again gives this room to widen.
pub const MIN_SUPPORTED_VAULT_VERSION: u32 = 2;

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

/// Lifecycle status of a vault entry.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EntryStatus {
    /// The credential is active and available for retrieval.
    #[default]
    Active,
    /// The credential has been revoked and cannot be retrieved.
    Revoked,
    /// The credential has expired (time-based).
    Expired,
}

impl std::fmt::Display for EntryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => f.write_str("active"),
            Self::Revoked => f.write_str("revoked"),
            Self::Expired => f.write_str("expired"),
        }
    }
}

/// A lifecycle event in a vault entry's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEvent {
    /// When the event occurred.
    pub timestamp: Timestamp,
    /// What happened.
    pub kind: HistoryEventKind,
}

/// The kind of lifecycle event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HistoryEventKind {
    /// The entry was created.
    Created,
    /// The entry's secret was rotated.
    Rotated,
    /// The entry was revoked.
    Revoked,
}

impl std::fmt::Display for HistoryEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => f.write_str("created"),
            Self::Rotated => f.write_str("rotated"),
            Self::Revoked => f.write_str("revoked"),
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
    /// When the credential was revoked, if ever.
    #[serde(default)]
    pub revoked_at: Option<Timestamp>,
    /// Number of times the secret has been rotated.
    #[serde(default)]
    pub rotation_count: u32,
    /// Free-form tags for filtering and grouping.
    pub tags: Vec<CompactString>,
}

/// A single credential stored in the vault.
///
/// The `encrypted_data` field holds the ciphertext produced by
/// ChaCha20-Poly1305. Decryption requires the [`VaultKey`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
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

// WHY: manual Debug instead of #[derive(Debug)] — the type touches
// `credential_type` (RUST/no-debug-derive-on-public-types matches on the
// "credential" token). `encrypted_data` is ChaCha20-Poly1305 ciphertext, not
// plaintext, so this mirrors the derived output exactly.
impl std::fmt::Debug for VaultEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultEntry")
            .field("name", &self.name)
            .field("credential_type", &self.credential_type)
            .field("encrypted_data", &self.encrypted_data)
            .field("metadata", &self.metadata)
            .finish()
    }
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

/// Format version of the domain-separation AEAD associated data built by
/// [`seal_signing_key`]/[`unseal_signing_key`].
///
/// Distinct from [`VAULT_VERSION`] and [`crate::crypto::ENTRY_ENVELOPE_VERSION`]:
/// this versions only the signing-key seal's own binding, independent of the
/// header format and of any vault entry's envelope.
const SIGNING_KEY_ENVELOPE_VERSION: u8 = 1;

/// Domain-separation tag baked into the signing key seal's AEAD associated
/// data.
///
/// WHY: without this, a ciphertext produced by [`seal_signing_key`] and any
/// OTHER ciphertext encrypted under the same [`VaultKey`] (a vault entry via
/// [`crate::crypto::entry_aad`], the header's key-check plaintext, or any
/// future caller) authenticate identically wherever the key matches — the
/// same unauthenticated-context defect forkwright/akroasis#283 fixed for
/// vault entries, surviving in the one sibling function that reuses the
/// same key material for a different purpose.
const SIGNING_KEY_AAD_TAG: &[u8] = b"kryphos/vault/signing-key";

/// Builds the fixed AEAD associated data for a signing-key seal.
fn signing_key_aad() -> Vec<u8> {
    let mut aad = Vec::with_capacity(1 + SIGNING_KEY_AAD_TAG.len());
    aad.push(SIGNING_KEY_ENVELOPE_VERSION);
    aad.extend_from_slice(SIGNING_KEY_AAD_TAG);
    aad
}

/// Encrypts the signing key of an [`InstallationIdentity`] under the given
/// vault key, returning `nonce || ciphertext || tag` — the same envelope
/// [`crate::crypto::encrypt`] produces.
///
/// WHY the nonce is generated here rather than accepted: repeating a nonce
/// under one key costs ChaCha20-Poly1305 both confidentiality and integrity,
/// and the domain-separation AAD below does not mitigate it — that tag
/// separates *purposes*, not *uses*, so two seals under one vault key are
/// exactly the case it cannot see. A parameter is the only way a caller can
/// repeat a nonce here, so there is no parameter.
///
/// # Errors
///
/// Returns [`CryptoError::EncryptionFailed`] if encryption fails.
pub fn seal_signing_key(
    identity: &InstallationIdentity,
    vault_key: &VaultKey,
) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(vault_key.as_bytes().into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    // WHY: `to_bytes()` returns the raw Ed25519 signing key in an ordinary
    // array; wrap immediately so this ephemeral copy is scrubbed on drop
    // rather than left on the stack after `encrypt` returns — the same
    // coverage `unseal_signing_key` gives the decrypt direction below
    // (RUST/#218).
    let plaintext = zeroizing_signing_key_bytes(identity);
    let aad = signing_key_aad();

    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext.as_ref(),
                aad: &aad,
            },
        )
        .map_err(|e| CryptoError::EncryptionFailed {
            reason: e.to_string(),
        })?;

    let mut sealed = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ciphertext);
    Ok(sealed)
}

/// Copies the signing key's raw bytes into a zero-on-drop buffer.
fn zeroizing_signing_key_bytes(
    identity: &InstallationIdentity,
) -> Zeroizing<[u8; SIGNING_KEY_LEN]> {
    Zeroizing::new(identity.signing_key().to_bytes())
}

/// Decrypts a signing key from vault ciphertext, reconstructing the
/// [`InstallationIdentity`].
///
/// # Errors
///
/// Returns [`CryptoError::DecryptionFailed`] if decryption fails (wrong key,
/// tampered ciphertext, or ciphertext sealed for a different purpose under
/// the same key — see [`SIGNING_KEY_AAD_TAG`]).
/// Returns [`CryptoError::KeyParse`] if the decrypted bytes are not a valid
/// Ed25519 key.
/// Returns [`CryptoError::InvalidNonceLength`] if `sealed` is shorter than the
/// nonce it must carry.
pub fn unseal_signing_key(
    sealed: &[u8],
    vault_key: &VaultKey,
) -> Result<InstallationIdentity, CryptoError> {
    if sealed.len() < NONCE_LEN {
        return Err(CryptoError::InvalidNonceLength {
            expected: NONCE_LEN,
            actual: sealed.len(),
        });
    }

    let (nonce_bytes, ciphertext) = sealed.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(vault_key.as_bytes().into());
    let nonce = Nonce::from_slice(nonce_bytes);
    let aad = signing_key_aad();

    // WHY: wrap at the point of allocation — `decrypt`'s return (the raw
    // Ed25519 signing key bytes) is moved straight into `Zeroizing::new`
    // with no intermediate unwrapped binding, so this ephemeral copy is
    // scrubbed on drop rather than left in freed heap memory.
    // `SigningKey::from_bytes` (key.rs) makes one further copy crossing
    // into its own fixed-size array before constructing the
    // `ZeroizeOnDrop`-protected `ed25519_dalek::SigningKey`; that copy is
    // wrapped the same way (`zeroizing_key_array`), so no unprotected frame
    // remains between this decrypt output and the protected key it becomes
    // (RUST/#218).
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::DecryptionFailed)?,
    );

    let signing = SigningKey::from_bytes(&plaintext).context(KeyParseSnafu)?;

    Ok(InstallationIdentity::from_signing_key(signing))
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    fn sample_metadata() -> EntryMetadata {
        EntryMetadata {
            created_at: Timestamp::UNIX_EPOCH,
            rotated_at: None,
            revoked_at: None,
            rotation_count: 0,
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
            revoked_at: None,
            rotation_count: 3,
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

    // -----------------------------------------------------------------
    // Seal / unseal signing key
    // -----------------------------------------------------------------

    #[test]
    fn seal_unseal_round_trip_preserves_identity() {
        let identity = InstallationIdentity::generate();
        let vault_key = VaultKey::from_bytes([0x42; 32]);

        let sealed = seal_signing_key(&identity, &vault_key).unwrap();
        let recovered = unseal_signing_key(&sealed, &vault_key).unwrap();

        assert_eq!(
            identity.public_key_bytes(),
            recovered.public_key_bytes(),
            "recovered identity must have the same public key"
        );

        let message = b"round-trip test";
        let sig = identity.sign(message);
        assert!(
            recovered.verify(message, &sig).is_ok(),
            "recovered identity must verify signatures FROM the original"
        );
    }

    /// Dispositive by construction, same mechanism as
    /// `decrypted_secret_is_zeroized_on_drop_by_type` (kryphos storage
    /// tests): `[u8; N]` alone does not implement `ZeroizeOnDrop` (only
    /// `Zeroize`), so this specific bound fails to compile against a bare
    /// array and passes only because `zeroizing_signing_key_bytes` returns
    /// `Zeroizing<[u8; N]>` — the encrypt-direction counterpart of the
    /// `Zeroizing` wrap `unseal_signing_key` already applies on decrypt.
    #[test]
    fn seal_signing_key_plaintext_buffer_is_zeroize_on_drop_by_type() {
        use zeroize::ZeroizeOnDrop;
        fn assert_zeroizes_on_drop<T: ZeroizeOnDrop>(_: &T) {}

        let identity = InstallationIdentity::generate();
        let plaintext = zeroizing_signing_key_bytes(&identity);
        assert_zeroizes_on_drop(&plaintext);
        assert_eq!(
            *plaintext,
            identity.signing_key().to_bytes(),
            "wrapped buffer must carry the same bytes through"
        );
    }

    #[test]
    fn unseal_with_wrong_key_fails() {
        let identity = InstallationIdentity::generate();
        let vault_key = VaultKey::from_bytes([0x42; 32]);
        let wrong_key = VaultKey::from_bytes([0xFF; 32]);

        let sealed = seal_signing_key(&identity, &vault_key).unwrap();
        let result = unseal_signing_key(&sealed, &wrong_key);
        assert!(result.is_err(), "decryption must fail with wrong vault key");
    }

    /// The acceptance partner to every rejection test here, and the one that
    /// actually witnesses the #231 fix: a caller can no longer choose a nonce,
    /// so the property to prove is that the function chooses a different one
    /// each time. Sealing the SAME identity under the SAME key twice must
    /// still produce two distinct envelopes, and must differ in the nonce
    /// prefix specifically — identical ciphertext bodies under one key would
    /// be the exact two-time-pad break the parameter's removal exists to
    /// prevent.
    #[test]
    fn seal_draws_a_fresh_nonce_on_every_call() {
        let identity = InstallationIdentity::generate();
        let vault_key = VaultKey::from_bytes([0x42; 32]);

        let first = seal_signing_key(&identity, &vault_key).unwrap();
        let second = seal_signing_key(&identity, &vault_key).unwrap();

        assert_ne!(
            first.get(..NONCE_LEN),
            second.get(..NONCE_LEN),
            "two seals under one key must not share a nonce"
        );
        assert_ne!(
            first.get(NONCE_LEN..),
            second.get(NONCE_LEN..),
            "distinct nonces must yield distinct ciphertext for identical plaintext"
        );

        for sealed in [&first, &second] {
            assert_eq!(
                unseal_signing_key(sealed, &vault_key)
                    .unwrap()
                    .public_key_bytes(),
                identity.public_key_bytes(),
                "both envelopes must still round-trip"
            );
        }
    }

    /// The nonce now travels inside the envelope, so corrupting it is a
    /// distinct failure path from corrupting the ciphertext body — this test
    /// covers the prefix, `unseal_tampered_ciphertext_fails` covers the rest.
    #[test]
    fn unseal_with_tampered_nonce_fails() {
        let identity = InstallationIdentity::generate();
        let vault_key = VaultKey::from_bytes([0x42; 32]);

        let mut sealed = seal_signing_key(&identity, &vault_key).unwrap();
        sealed[0] ^= 0xFF;

        let result = unseal_signing_key(&sealed, &vault_key);
        assert!(
            result.is_err(),
            "decryption must fail when the carried nonce is altered"
        );
    }

    /// An envelope too short to carry a nonce is malformed input, not a
    /// failed decryption, and must be reported as such rather than reaching
    /// the AEAD with a truncated slice.
    #[test]
    fn unseal_rejects_envelope_shorter_than_its_nonce() {
        let vault_key = VaultKey::from_bytes([0x42; 32]);
        let truncated = [0u8; NONCE_LEN - 1];

        let result = unseal_signing_key(&truncated, &vault_key);

        assert!(
            matches!(
                result,
                Err(CryptoError::InvalidNonceLength { expected, actual })
                    if expected == NONCE_LEN && actual == NONCE_LEN - 1
            ),
            "got {result:?}"
        );
    }

    #[test]
    fn unseal_wrong_length_plaintext_is_key_parse_error() {
        // WHY sealed under `signing_key_aad()` rather than the raw cipher
        // with no AAD: decryption must actually SUCCEED for this test to
        // reach the length check it targets. Since unseal_signing_key now
        // requires the domain-separation tag (this same test file's
        // `unseal_rejects_ciphertext_sealed_for_a_different_purpose`
        // verifies that requirement), a ciphertext sealed without it no
        // longer decrypts at all — it fails authentication before
        // `SigningKey::from_bytes` ever runs, which would make this
        // assertion pass for the wrong reason.
        let vault_key = VaultKey::from_bytes([0x42; 32]);
        let nonce = [0x01; NONCE_LEN];
        let cipher = ChaCha20Poly1305::new(vault_key.as_bytes().into());
        let aad = signing_key_aad();
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &[0u8; 16][..],
                    aad: &aad,
                },
            )
            .unwrap();
        let sealed = [&nonce[..], &ciphertext[..]].concat();

        let result = unseal_signing_key(&sealed, &vault_key);

        assert!(
            matches!(result, Err(CryptoError::KeyParse { .. })),
            "got {result:?}"
        );
    }

    #[test]
    fn unseal_rejects_ciphertext_sealed_for_a_different_purpose() {
        // WHY: proves domain separation. A ciphertext produced under the
        // SAME VaultKey+nonce for an unrelated purpose (any other
        // same-key/same-nonce caller — a vault entry, a header key-check, a
        // future consumer) must not unseal as a signing key. Before this
        // fix, seal/unseal used no AAD at all, so any same-key ciphertext of
        // the right plaintext length was interchangeable — the same
        // unauthenticated-context defect forkwright/akroasis#283 fixed for
        // vault entries, surviving in this sibling function.
        let vault_key = VaultKey::from_bytes([0x42; 32]);
        let nonce = [0x01; NONCE_LEN];

        // A foreign ciphertext: same key, same nonce, no domain-separation
        // tag, 32-byte plaintext (the exact length a signing key seed is).
        let cipher = ChaCha20Poly1305::new(vault_key.as_bytes().into());
        let foreign_ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), &[0x11u8; 32][..])
            .unwrap();
        let foreign_envelope = [&nonce[..], &foreign_ciphertext[..]].concat();

        let result = unseal_signing_key(&foreign_envelope, &vault_key);
        assert!(
            result.is_err(),
            "a ciphertext sealed without the signing-key domain tag must \
             not unseal as a signing key, got {result:?}"
        );
    }

    #[test]
    fn unseal_tampered_ciphertext_fails() {
        let identity = InstallationIdentity::generate();
        let vault_key = VaultKey::from_bytes([0x42; 32]);

        let mut sealed = seal_signing_key(&identity, &vault_key).unwrap();
        // WHY not index 0: the envelope now leads with the nonce, so byte 0
        // corrupts that rather than the ciphertext this test names.
        // `unseal_with_tampered_nonce_fails` owns the prefix.
        sealed[NONCE_LEN] ^= 0xFF;

        let result = unseal_signing_key(&sealed, &vault_key);
        assert!(
            result.is_err(),
            "decryption must fail with tampered ciphertext"
        );
    }

    // -----------------------------------------------------------------
    // EntryStatus
    // -----------------------------------------------------------------

    #[test]
    fn entry_status_default_is_active() {
        assert_eq!(EntryStatus::default(), EntryStatus::Active);
    }

    #[test]
    fn entry_status_display() {
        assert_eq!(EntryStatus::Active.to_string(), "active");
        assert_eq!(EntryStatus::Revoked.to_string(), "revoked");
        assert_eq!(EntryStatus::Expired.to_string(), "expired");
    }

    #[test]
    fn entry_status_serde_round_trip() {
        for status in [
            EntryStatus::Active,
            EntryStatus::Revoked,
            EntryStatus::Expired,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: EntryStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back);
        }
    }

    // -----------------------------------------------------------------
    // HistoryEvent / HistoryEventKind
    // -----------------------------------------------------------------

    #[test]
    fn history_event_kind_display() {
        assert_eq!(HistoryEventKind::Created.to_string(), "created");
        assert_eq!(HistoryEventKind::Rotated.to_string(), "rotated");
        assert_eq!(HistoryEventKind::Revoked.to_string(), "revoked");
    }

    #[test]
    fn history_event_serde_round_trip() {
        let event = HistoryEvent {
            timestamp: Timestamp::UNIX_EPOCH,
            kind: HistoryEventKind::Rotated,
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: HistoryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }
}
