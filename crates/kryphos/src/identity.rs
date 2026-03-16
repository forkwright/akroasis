//! Installation identity — Ed25519 keypair for provenance signing.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use snafu::{ResultExt, Snafu};

use crate::vault::{VaultError, VaultKey, VaultStorage};

/// Errors from identity operations.
#[derive(Debug, Snafu)]
pub enum IdentityError {
    /// Failed to store identity in vault.
    #[snafu(display("failed to store identity: {source}"))]
    VaultStore {
        /// Underlying vault error.
        source: VaultError,
    },

    /// Failed to load identity from vault.
    #[snafu(display("failed to load identity: {source}"))]
    VaultLoad {
        /// Underlying vault error.
        source: VaultError,
    },

    /// Stored key material has wrong length.
    #[snafu(display("invalid key length: expected 32 bytes, got {actual}"))]
    InvalidKeyLength {
        /// Actual length of the decrypted bytes.
        actual: usize,
    },
}

/// An installation's Ed25519 identity for signing tamper log entries.
///
/// Each Akroasis installation has a unique keypair. The private key signs
/// tamper log entries (proving provenance). The public key is the
/// installation's identity fingerprint.
pub struct InstallationIdentity {
    signing_key: SigningKey,
}

impl InstallationIdentity {
    /// Generates a new random Ed25519 keypair.
    pub fn generate() -> Self {
        Self {
            signing_key: SigningKey::generate(&mut OsRng),
        }
    }

    /// Signs a message and returns the Ed25519 signature.
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }

    /// Verifies a signature against a message using this identity's public key.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
        self.signing_key
            .verifying_key()
            .verify(message, signature)
            .is_ok()
    }

    /// Returns the 32-byte public key for identity fingerprinting.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Returns the verifying (public) key.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Signs a tamper log entry hash (the 32-byte BLAKE3 hash from the chain).
    ///
    /// Use with [`koinon::TamperLog::last_hash`] to sign the most recent entry.
    pub fn sign_entry(&self, entry_hash: &[u8; 32]) -> Signature {
        self.signing_key.sign(entry_hash.as_slice())
    }

    /// Stores the keypair encrypted in the vault.
    ///
    /// Only the 32-byte private key seed is stored; the public key is
    /// derived on load.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::VaultStore`] if encryption or I/O fails.
    pub fn store(&self, vault: &VaultStorage, vault_key: &VaultKey) -> Result<(), IdentityError> {
        let private_bytes = self.signing_key.to_bytes();
        vault
            .store(&private_bytes, vault_key)
            .context(VaultStoreSnafu)
    }

    /// Loads a keypair from the vault.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::VaultLoad`] if decryption fails, or
    /// [`IdentityError::InvalidKeyLength`] if the decrypted data is not 32 bytes.
    pub fn load(vault: &VaultStorage, vault_key: &VaultKey) -> Result<Self, IdentityError> {
        let decrypted = vault.load(vault_key).context(VaultLoadSnafu)?;
        let bytes: [u8; 32] = decrypted
            .try_into()
            .map_err(|v: Vec<u8>| IdentityError::InvalidKeyLength { actual: v.len() })?;
        let signing_key = SigningKey::from_bytes(&bytes);
        Ok(Self { signing_key })
    }
}

/// Verifies a signature using a standalone public key (no private key needed).
///
/// Returns `false` if the public key bytes are invalid or verification fails.
pub fn verify_with_public_key(
    public_key_bytes: &[u8; 32],
    message: &[u8],
    signature: &Signature,
) -> bool {
    let Ok(verifying_key) = VerifyingKey::from_bytes(public_key_bytes) else {
        return false;
    };
    verifying_key.verify(message, signature).is_ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_docs_in_private_items
)]
mod tests {
    use super::*;
    use koinon::tamper_log::{LogEntryKind, TamperLog};

    #[test]
    fn generate_creates_valid_keypair() {
        let id = InstallationIdentity::generate();
        let pk = id.public_key_bytes();
        // Public key must not be all zeros.
        assert_ne!(pk, [0u8; 32]);
        // Verify the verifying key is derivable.
        assert_eq!(pk, id.verifying_key().to_bytes());
    }

    #[test]
    fn sign_verify_roundtrip() {
        let id = InstallationIdentity::generate();
        let message = b"tamper log entry hash placeholder";
        let signature = id.sign(message);
        assert!(id.verify(message, &signature));
    }

    #[test]
    fn wrong_key_rejects_signature() {
        let id_a = InstallationIdentity::generate();
        let id_b = InstallationIdentity::generate();

        let message = b"signed by identity A";
        let signature = id_a.sign(message);

        // Verification with a different identity's key must fail.
        assert!(!id_b.verify(message, &signature));
    }

    #[test]
    fn tampered_message_rejects_signature() {
        let id = InstallationIdentity::generate();
        let signature = id.sign(b"original");
        assert!(!id.verify(b"tampered", &signature));
    }

    #[test]
    fn vault_storage_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("identity.vault");
        let vault = VaultStorage::new(&vault_path);
        let vault_key = VaultKey::from_bytes([42u8; 32]);

        let original = InstallationIdentity::generate();
        original.store(&vault, &vault_key).unwrap();

        let loaded = InstallationIdentity::load(&vault, &vault_key).unwrap();
        assert_eq!(original.public_key_bytes(), loaded.public_key_bytes());

        // Loaded identity can still sign and verify.
        let sig = loaded.sign(b"after vault roundtrip");
        assert!(loaded.verify(b"after vault roundtrip", &sig));
        // Original can verify loaded's signature (same keypair).
        assert!(original.verify(b"after vault roundtrip", &sig));
    }

    #[test]
    fn vault_load_wrong_key_fails() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("identity.vault");
        let vault = VaultStorage::new(&vault_path);

        let id = InstallationIdentity::generate();
        let correct_key = VaultKey::from_bytes([1u8; 32]);
        id.store(&vault, &correct_key).unwrap();

        let wrong_key = VaultKey::from_bytes([99u8; 32]);
        let result = InstallationIdentity::load(&vault, &wrong_key);
        assert!(result.is_err());
    }

    #[test]
    fn sign_entry_signs_tamper_log_hash() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        let mut log = TamperLog::open(&log_path).unwrap();
        log.append(LogEntryKind::ActionTaken {
            actor: "operator".into(),
            action: "deploy".into(),
            target: Some("production".into()),
        })
        .unwrap();

        let entry_hash = log.last_hash();
        let id = InstallationIdentity::generate();

        let signature = id.sign_entry(entry_hash);
        // Verify using the generic verify method.
        assert!(id.verify(entry_hash, &signature));
        // Verify using the standalone public key function.
        assert!(verify_with_public_key(
            &id.public_key_bytes(),
            entry_hash,
            &signature
        ));
    }

    #[test]
    fn sign_entry_different_identity_rejects() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        let mut log = TamperLog::open(&log_path).unwrap();
        log.append(LogEntryKind::ConfigChanged {
            key: "threshold".into(),
            old_value: Some("10".into()),
            new_value: "20".into(),
        })
        .unwrap();

        let entry_hash = log.last_hash();
        let signer = InstallationIdentity::generate();
        let impostor = InstallationIdentity::generate();

        let signature = signer.sign_entry(entry_hash);
        assert!(!impostor.verify(entry_hash, &signature));
    }

    #[test]
    fn verify_with_standalone_public_key() {
        let id = InstallationIdentity::generate();
        let message = b"standalone verification";
        let signature = id.sign(message);
        let pk = id.public_key_bytes();

        assert!(verify_with_public_key(&pk, message, &signature));
        // Wrong message fails.
        assert!(!verify_with_public_key(&pk, b"wrong", &signature));
        // Wrong key fails.
        let other = InstallationIdentity::generate();
        assert!(!verify_with_public_key(
            &other.public_key_bytes(),
            message,
            &signature
        ));
    }
}
