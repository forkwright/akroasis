//! Figment provider that resolves `vault:`-prefixed config values.

use figment::value::{Dict, Map, Value};
use figment::{Error, Metadata, Profile, Provider};
use zeroize::Zeroizing;

use crate::storage::Vault;

/// Prefix marking a config value as a vault reference.
const VAULT_PREFIX: &str = "vault:";

/// Figment provider that resolves `vault:`-prefixed config values
/// from an encrypted vault.
///
/// Wraps an inner provider (typically TOML) and transparently replaces
/// `vault:entry_name` strings with the decrypted vault entry. Non-vault
/// values pass through unchanged.
///
/// # Errors
///
/// Returns an error if a `vault:` value references a missing entry,
/// if the vault entry contains non-UTF-8 data, or if the provider was
/// created without a vault via [`VaultProvider::without_vault`].
pub struct VaultProvider<P> {
    inner: P,
    vault: Option<Vault>,
}

impl<P> std::fmt::Debug for VaultProvider<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultProvider")
            .field("has_vault", &self.vault.is_some())
            .finish_non_exhaustive()
    }
}

impl<P> VaultProvider<P> {
    /// Creates a provider that resolves vault references using the given vault.
    pub const fn new(inner: P, vault: Vault) -> Self {
        Self {
            inner,
            vault: Some(vault),
        }
    }

    /// Creates a provider without a vault.
    ///
    /// Non-vault values pass through unchanged. Any `vault:` reference
    /// produces an error indicating the vault is not initialized.
    pub const fn without_vault(inner: P) -> Self {
        Self { inner, vault: None }
    }

    #[expect(
        clippy::result_large_err,
        reason = "figment::Error size is outside our control"
    )]
    fn resolve_dict(&self, dict: Dict) -> Result<Dict, Error> {
        let mut resolved = Dict::new();

        for (key, value) in dict {
            resolved.insert(key, self.resolve_value(value)?);
        }

        Ok(resolved)
    }

    #[expect(
        clippy::result_large_err,
        reason = "figment::Error size is outside our control"
    )]
    fn resolve_value(&self, value: Value) -> Result<Value, Error> {
        match value {
            Value::String(tag, s) => {
                if let Some(entry_name) = s.strip_prefix(VAULT_PREFIX) {
                    let vault = self.vault.as_ref().ok_or_else(|| {
                        Error::from(format!(
                            "vault is not initialized; config references vault entry '{entry_name}'"
                        ))
                    })?;

                    let decrypted = vault
                        .get(entry_name)
                        .map_err(|e| Error::from(e.to_string()))?;

                    // WHY: `decrypted.secret` is `Zeroizing<Vec<u8>>` and has
                    // no public escape hatch to a bare `Vec<u8>`/`String` —
                    // `str::from_utf8` borrows instead of consuming, so the
                    // validated bytes never leave the zeroizing wrapper.
                    // `decrypted` (and its `secret` field) is scrubbed on
                    // drop at the end of this function.
                    let validated = std::str::from_utf8(&decrypted.secret).map_err(|_| {
                        Error::from(format!(
                            "vault entry '{entry_name}' contains non-UTF-8 data"
                        ))
                    })?;
                    let secret_str = Zeroizing::new(validated.to_owned());

                    // NOTE: `figment::Value::String` requires an owned,
                    // non-zeroizing `String` — this clone (deref past the
                    // `Zeroizing` wrapper first) is the one unavoidable copy
                    // that crosses into a type we don't control. `secret_str`
                    // itself still zeroizes on drop immediately after.
                    Ok(Value::String(tag, (*secret_str).clone()))
                } else {
                    Ok(Value::String(tag, s))
                }
            }
            Value::Dict(tag, dict) => Ok(Value::Dict(tag, self.resolve_dict(dict)?)),
            Value::Array(tag, arr) => {
                let resolved: Result<Vec<Value>, Error> =
                    arr.into_iter().map(|v| self.resolve_value(v)).collect();
                Ok(Value::Array(tag, resolved?))
            }
            other => Ok(other),
        }
    }
}

impl<P: Provider> Provider for VaultProvider<P> {
    fn metadata(&self) -> Metadata {
        Metadata::named("vault provider")
    }

    fn data(&self) -> Result<Map<Profile, Dict>, Error> {
        let data = self.inner.data()?;
        let mut resolved = Map::new();

        for (profile, dict) in data {
            resolved.insert(profile, self.resolve_dict(dict)?);
        }

        Ok(resolved)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions use unwrap for clarity")]
#[expect(clippy::expect_used, reason = "test assertions use expect for clarity")]
mod tests {
    use figment::Figment;
    use figment::providers::Serialized;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::storage::Vault;
    use crate::vault::CredentialType;

    const TEST_PASSPHRASE: &[u8] = b"correct horse battery staple";

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestConfig {
        api_key: String,
        host: String,
        port: u16,
    }

    #[test]
    fn resolves_vault_prefixed_value() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault");
        let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
        vault
            .add("my_api_key", CredentialType::ApiKey, b"sk-secret-123")
            .unwrap();

        let inner = Serialized::defaults(TestConfig {
            api_key: "vault:my_api_key".to_owned(),
            host: "test.invalid".to_owned(),
            port: 8080,
        });

        let config: TestConfig = Figment::from(VaultProvider::new(inner, vault))
            .extract()
            .unwrap();

        assert_eq!(
            config.api_key, "sk-secret-123",
            "vault value must be decrypted"
        );
    }

    #[test]
    fn non_vault_values_pass_through_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault");
        let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

        let inner = Serialized::defaults(TestConfig {
            api_key: "plain-key-value".to_owned(),
            host: "test.invalid".to_owned(),
            port: 443,
        });

        let config: TestConfig = Figment::from(VaultProvider::new(inner, vault))
            .extract()
            .unwrap();

        assert_eq!(
            config.api_key, "plain-key-value",
            "non-vault string must not be modified"
        );
        assert_eq!(
            config.host, "test.invalid",
            "non-vault string must pass through"
        );
        assert_eq!(config.port, 443, "non-string value must pass through");
    }

    /// A vault stores arbitrary bytes while `figment::Value::String` holds a
    /// `String`, so resolution has to refuse a secret that is not UTF-8. The
    /// production path for that refusal existed with nothing exercising it
    /// (forkwright/akroasis#231).
    #[test]
    fn non_utf8_vault_entry_returns_error_naming_entry() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault");
        let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
        // A lone 0xFF is not a legal UTF-8 byte in any position.
        vault
            .add("binary_key", CredentialType::ApiKey, &[0xFF, 0xFE, 0x00])
            .unwrap();

        let inner = Serialized::defaults(TestConfig {
            api_key: "vault:binary_key".to_owned(),
            host: "test.invalid".to_owned(),
            port: 8080,
        });

        let result: Result<TestConfig, _> =
            Figment::from(VaultProvider::new(inner, vault)).extract();

        let message = result.expect_err("non-UTF-8 secret must not resolve").to_string();
        assert!(
            message.contains("binary_key") && message.contains("non-UTF-8"),
            "error must name the offending entry and the reason, got: {message}"
        );
    }

    /// The acceptance partner to `non_utf8_vault_entry_returns_error_naming_entry`.
    /// Without it that test passes just as well against a resolver that
    /// rejects every byte sequence outside ASCII, which would be a different
    /// bug wearing the same green check.
    #[test]
    fn multibyte_utf8_vault_entry_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault");
        let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
        let secret = "κρυφός-π@ss–word✓";
        vault
            .add("unicode_key", CredentialType::ApiKey, secret.as_bytes())
            .unwrap();

        let inner = Serialized::defaults(TestConfig {
            api_key: "vault:unicode_key".to_owned(),
            host: "test.invalid".to_owned(),
            port: 8080,
        });

        let config: TestConfig = Figment::from(VaultProvider::new(inner, vault))
            .extract()
            .unwrap();

        assert_eq!(
            config.api_key, secret,
            "a multi-byte UTF-8 secret must survive resolution intact"
        );
    }

    #[test]
    fn missing_vault_entry_returns_error_naming_entry() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault");
        let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

        let inner = Serialized::defaults(TestConfig {
            api_key: "vault:nonexistent_key".to_owned(),
            host: "test.invalid".to_owned(),
            port: 8080,
        });

        let result: Result<TestConfig, _> =
            Figment::from(VaultProvider::new(inner, vault)).extract();
        let err = result.expect_err("missing vault entry must produce an error");
        let err_msg = err.to_string();

        assert!(
            err_msg.contains("nonexistent_key"),
            "error must name the missing entry, got: {err_msg}"
        );
    }

    #[test]
    fn vault_not_initialized_returns_clear_error() {
        let inner = Serialized::defaults(TestConfig {
            api_key: "vault:some_key".to_owned(),
            host: "test.invalid".to_owned(),
            port: 8080,
        });

        let result: Result<TestConfig, _> =
            Figment::from(VaultProvider::<_>::without_vault(inner)).extract();
        let err = result.expect_err("uninitialized vault must produce an error");
        let err_msg = err.to_string();

        assert!(
            err_msg.contains("not initialized"),
            "error must mention vault not initialized, got: {err_msg}"
        );
    }

    #[test]
    fn without_vault_passes_non_vault_values() {
        let inner = Serialized::defaults(TestConfig {
            api_key: "plain-key".to_owned(),
            host: "test.invalid".to_owned(),
            port: 8080,
        });

        let config: TestConfig = Figment::from(VaultProvider::<_>::without_vault(inner))
            .extract()
            .unwrap();

        assert_eq!(
            config.api_key, "plain-key",
            "non-vault VALUES must work without vault"
        );
    }
}
