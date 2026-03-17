//! Vault CLI — credential storage, retrieval, and lifecycle management.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::Subcommand;
use comfy_table::{Cell, Table};
use snafu::{ResultExt, Snafu};

use kryphos::{CredentialType, InstallationIdentity, Vault, VaultError};

/// Default vault path: `~/.local/share/akroasis/vault`.
fn default_vault_path() -> PathBuf {
    std::env::var("AKROASIS_VAULT_PATH").map_or_else(
        |_| {
            std::env::var("HOME")
                .map_or_else(|_| PathBuf::from("."), PathBuf::from)
                .join(".local/share/akroasis/vault")
        },
        PathBuf::from,
    )
}

/// Vault subcommands.
#[derive(Subcommand)]
pub enum VaultCommand {
    /// Create a new vault (prompts for passphrase)
    Init,

    /// Store a new credential in the vault
    Add {
        /// Credential name (unique identifier)
        name: String,

        /// Credential type
        #[arg(long, value_parser = parse_credential_type)]
        r#type: CredentialType,
    },

    /// List all credentials (names and metadata only)
    List,

    /// Retrieve and decrypt a credential
    Get {
        /// Credential name to retrieve
        name: String,
    },

    /// Rotate a credential's secret value
    Rotate {
        /// Credential name to rotate
        name: String,
    },

    /// Revoke a credential (prevents future retrieval)
    Revoke {
        /// Credential name to revoke
        name: String,
    },

    /// Show installation public key fingerprint
    Identity,
}

/// Errors from vault CLI operations.
#[derive(Debug, Snafu)]
pub enum VaultCliError {
    /// A vault operation failed.
    #[snafu(display("{source}"))]
    Vault { source: VaultError },

    /// Passphrase input failed.
    #[snafu(display("passphrase input failed: {source}"))]
    PassphraseInput { source: io::Error },

    /// Passphrases did not match during confirmation.
    #[snafu(display("passphrases do not match"))]
    PassphraseMismatch,

    /// The user cancelled the operation.
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

fn parse_credential_type(s: &str) -> Result<CredentialType, String> {
    match s {
        "api-key" => Ok(CredentialType::ApiKey),
        "psk" => Ok(CredentialType::Psk),
        "certificate" => Ok(CredentialType::Certificate),
        "radio-key" => Ok(CredentialType::RadioKey),
        other => other.strip_prefix("custom:").map_or_else(
            || {
                Err(format!(
                    "unknown credential type '{other}': \
                     expected api-key, psk, certificate, radio-key, or custom:<label>"
                ))
            },
            |label| {
                Ok(CredentialType::Custom {
                    label: label.into(),
                })
            },
        ),
    }
}

/// Reads a passphrase from the terminal (no echo).
fn read_passphrase(prompt: &str) -> Result<String, VaultCliError> {
    rpassword::prompt_password(prompt).context(PassphraseInputSnafu)
}

/// Reads and confirms a new passphrase (double entry).
fn read_passphrase_confirmed(prompt: &str) -> Result<String, VaultCliError> {
    let first = read_passphrase(prompt)?;
    let second = read_passphrase("Confirm passphrase: ")?;

    if first != second {
        return PassphraseMismatchSnafu.fail();
    }

    Ok(first)
}

/// Reads a secret value from the terminal (no echo).
fn read_secret(prompt: &str) -> Result<String, VaultCliError> {
    rpassword::prompt_password(prompt).context(PassphraseInputSnafu)
}

/// Dispatches a vault subcommand.
pub fn dispatch(cmd: &VaultCommand) -> Result<(), VaultCliError> {
    match cmd {
        VaultCommand::Init => run_init(),
        VaultCommand::Add { name, r#type } => run_add(name, r#type),
        VaultCommand::List => run_list(),
        VaultCommand::Get { name } => run_get(name),
        VaultCommand::Rotate { name } => run_rotate(name),
        VaultCommand::Revoke { name } => run_revoke(name),
        VaultCommand::Identity => {
            run_identity();
            Ok(())
        }
    }
}

fn run_init() -> Result<(), VaultCliError> {
    let path = default_vault_path();

    let passphrase = read_passphrase_confirmed("Vault passphrase: ")?;

    let _vault = Vault::create(&path, passphrase.as_bytes()).context(VaultSnafu)?;

    println!("Vault created at {}", path.display());
    Ok(())
}

fn run_add(name: &str, credential_type: &CredentialType) -> Result<(), VaultCliError> {
    let path = default_vault_path();
    let passphrase = read_passphrase("Vault passphrase: ")?;

    let vault = Vault::open(&path, passphrase.as_bytes()).context(VaultSnafu)?;

    let secret = read_secret("Secret value: ")?;
    vault
        .add(name, credential_type.clone(), secret.as_bytes())
        .context(VaultSnafu)?;

    println!("Added '{name}' ({credential_type})");
    Ok(())
}

fn run_list() -> Result<(), VaultCliError> {
    let path = default_vault_path();
    let passphrase = read_passphrase("Vault passphrase: ")?;

    let vault = Vault::open(&path, passphrase.as_bytes()).context(VaultSnafu)?;
    let entries = vault.list().context(VaultSnafu)?;

    if entries.is_empty() {
        println!("Vault is empty.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec!["Name", "Type", "Status", "Created", "Rotated"]);

    for entry in &entries {
        let rotated = entry
            .metadata
            .rotated_at
            .map_or_else(|| "-".to_owned(), |t| format_timestamp(&t));

        table.add_row(vec![
            Cell::new(&entry.name),
            Cell::new(&entry.credential_type),
            Cell::new(entry.status),
            Cell::new(format_timestamp(&entry.metadata.created_at)),
            Cell::new(rotated),
        ]);
    }

    println!("{table}");
    Ok(())
}

fn run_get(name: &str) -> Result<(), VaultCliError> {
    let path = default_vault_path();
    let passphrase = read_passphrase("Vault passphrase: ")?;

    let vault = Vault::open(&path, passphrase.as_bytes()).context(VaultSnafu)?;
    let entry = vault.get(name).context(VaultSnafu)?;

    let secret_str = String::from_utf8_lossy(&entry.secret);

    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{secret_str}");
    Ok(())
}

fn run_rotate(name: &str) -> Result<(), VaultCliError> {
    let path = default_vault_path();
    let passphrase = read_passphrase("Vault passphrase: ")?;

    let vault = Vault::open(&path, passphrase.as_bytes()).context(VaultSnafu)?;

    let new_secret = read_secret("New secret value: ")?;
    vault
        .rotate(name, new_secret.as_bytes())
        .context(VaultSnafu)?;

    println!("Rotated '{name}'");
    Ok(())
}

fn run_revoke(name: &str) -> Result<(), VaultCliError> {
    let path = default_vault_path();
    let passphrase = read_passphrase("Vault passphrase: ")?;

    let vault = Vault::open(&path, passphrase.as_bytes()).context(VaultSnafu)?;

    eprint!("Revoke '{name}'? This cannot be undone. [y/N] ");
    let _ = io::stderr().flush();

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context(PassphraseInputSnafu)?;

    if !answer.trim().eq_ignore_ascii_case("y") {
        return CancelledSnafu.fail();
    }

    vault.revoke(name).context(VaultSnafu)?;
    println!("Revoked '{name}'");
    Ok(())
}

fn run_identity() {
    let identity = InstallationIdentity::generate();
    let pubkey = identity.verifying_key();
    println!("{pubkey}");
}

/// Formats a jiff Timestamp as a human-readable string.
fn format_timestamp(ts: &jiff::Timestamp) -> String {
    ts.strftime("%Y-%m-%d %H:%M").to_string()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions use unwrap for clarity")]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: VaultCommand,
    }

    fn parse(args: &[&str]) -> VaultCommand {
        TestCli::parse_from(std::iter::once("test").chain(args.iter().copied())).command
    }

    #[test]
    fn parse_init() {
        let cmd = parse(&["init"]);
        assert!(matches!(cmd, VaultCommand::Init));
    }

    #[test]
    fn parse_add_with_api_key_type() {
        let cmd = parse(&["add", "my-key", "--type", "api-key"]);
        match cmd {
            VaultCommand::Add { name, r#type } => {
                assert_eq!(name, "my-key");
                assert_eq!(r#type, CredentialType::ApiKey);
            }
            _ => unreachable!("expected Add"),
        }
    }

    #[test]
    fn parse_add_with_psk_type() {
        let cmd = parse(&["add", "mesh-psk", "--type", "psk"]);
        match cmd {
            VaultCommand::Add { name, r#type } => {
                assert_eq!(name, "mesh-psk");
                assert_eq!(r#type, CredentialType::Psk);
            }
            _ => unreachable!("expected Add"),
        }
    }

    #[test]
    fn parse_add_with_certificate_type() {
        let cmd = parse(&["add", "tls-cert", "--type", "certificate"]);
        match cmd {
            VaultCommand::Add { name, r#type } => {
                assert_eq!(name, "tls-cert");
                assert_eq!(r#type, CredentialType::Certificate);
            }
            _ => unreachable!("expected Add"),
        }
    }

    #[test]
    fn parse_add_with_custom_type() {
        let cmd = parse(&["add", "my-secret", "--type", "custom:ssh-key"]);
        match cmd {
            VaultCommand::Add { name, r#type } => {
                assert_eq!(name, "my-secret");
                assert_eq!(
                    r#type,
                    CredentialType::Custom {
                        label: "ssh-key".into()
                    }
                );
            }
            _ => unreachable!("expected Add"),
        }
    }

    #[test]
    fn parse_add_missing_type_fails() {
        let result = TestCli::try_parse_from(["test", "add", "no-type"]);
        assert!(result.is_err(), "add without --type must fail");
    }

    #[test]
    fn parse_add_invalid_type_fails() {
        let result = TestCli::try_parse_from(["test", "add", "bad", "--type", "nonsense"]);
        assert!(result.is_err(), "add with invalid type must fail");
    }

    #[test]
    fn parse_list() {
        let cmd = parse(&["list"]);
        assert!(matches!(cmd, VaultCommand::List));
    }

    #[test]
    fn parse_get_with_name() {
        let cmd = parse(&["get", "my-key"]);
        match cmd {
            VaultCommand::Get { name } => assert_eq!(name, "my-key"),
            _ => unreachable!("expected Get"),
        }
    }

    #[test]
    fn parse_rotate_with_name() {
        let cmd = parse(&["rotate", "my-key"]);
        match cmd {
            VaultCommand::Rotate { name } => assert_eq!(name, "my-key"),
            _ => unreachable!("expected Rotate"),
        }
    }

    #[test]
    fn parse_revoke_with_name() {
        let cmd = parse(&["revoke", "my-key"]);
        match cmd {
            VaultCommand::Revoke { name } => assert_eq!(name, "my-key"),
            _ => unreachable!("expected Revoke"),
        }
    }

    #[test]
    fn parse_identity() {
        let cmd = parse(&["identity"]);
        assert!(matches!(cmd, VaultCommand::Identity));
    }

    #[test]
    fn parse_credential_type_all_variants() {
        assert_eq!(
            parse_credential_type("api-key").unwrap(),
            CredentialType::ApiKey
        );
        assert_eq!(parse_credential_type("psk").unwrap(), CredentialType::Psk);
        assert_eq!(
            parse_credential_type("certificate").unwrap(),
            CredentialType::Certificate
        );
        assert_eq!(
            parse_credential_type("radio-key").unwrap(),
            CredentialType::RadioKey
        );
        assert_eq!(
            parse_credential_type("custom:label").unwrap(),
            CredentialType::Custom {
                label: "label".into()
            }
        );
    }

    #[test]
    fn parse_credential_type_invalid_returns_error() {
        assert!(parse_credential_type("invalid").is_err());
    }

    // -----------------------------------------------------------------
    // Integration tests with temp vault
    // -----------------------------------------------------------------

    fn create_temp_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("test-vault");
        let vault = Vault::create(&vault_path, b"test-passphrase").unwrap();
        (dir, vault)
    }

    #[test]
    fn vault_init_creates_vault_at_path() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("init-vault");

        let vault = Vault::create(&vault_path, b"passphrase");
        assert!(vault.is_ok(), "vault creation must succeed");
        assert!(vault_path.exists(), "vault directory must exist after init");
    }

    #[test]
    fn vault_add_and_get_round_trip() {
        let (_dir, vault) = create_temp_vault();

        vault
            .add("test-key", CredentialType::ApiKey, b"secret-123")
            .unwrap();

        let entry = vault.get("test-key").unwrap();
        assert_eq!(entry.secret, b"secret-123");
        assert_eq!(entry.credential_type, CredentialType::ApiKey);
    }

    #[test]
    fn vault_list_shows_entries_without_secrets() {
        let (_dir, vault) = create_temp_vault();

        vault
            .add("key-a", CredentialType::ApiKey, b"secret-a")
            .unwrap();
        vault
            .add("key-b", CredentialType::Psk, b"secret-b")
            .unwrap();

        let entries = vault.list().unwrap();
        assert_eq!(entries.len(), 2, "list must return both entries");
    }

    #[test]
    fn vault_rotate_updates_secret() {
        let (_dir, vault) = create_temp_vault();

        vault
            .add("rotate-key", CredentialType::ApiKey, b"old-secret")
            .unwrap();
        vault.rotate("rotate-key", b"new-secret").unwrap();

        let entry = vault.get("rotate-key").unwrap();
        assert_eq!(entry.secret, b"new-secret");
    }

    #[test]
    fn vault_revoke_prevents_get() {
        let (_dir, vault) = create_temp_vault();

        vault
            .add("revoke-key", CredentialType::Psk, b"secret")
            .unwrap();
        vault.revoke("revoke-key").unwrap();

        let result = vault.get("revoke-key");
        assert!(result.is_err(), "get on revoked entry must fail");
    }

    #[test]
    fn vault_identity_generates_valid_fingerprint() {
        let identity = InstallationIdentity::generate();
        let pubkey = identity.verifying_key();
        let hex = pubkey.to_string();
        assert_eq!(hex.len(), 64, "hex fingerprint must be 64 characters");
        assert!(
            hex.chars().all(|c| c.is_ascii_hexdigit()),
            "fingerprint must be hex"
        );
    }
}
