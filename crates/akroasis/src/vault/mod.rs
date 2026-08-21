//! Vault CLI — credential storage, retrieval, and lifecycle management.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Subcommand;
use comfy_table::{Cell, Table};
use snafu::{ResultExt, Snafu};
use zeroize::Zeroizing;

use kryphos::{CredentialType, EntryInfo, Vault, VaultError};

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
#[non_exhaustive]
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
    List {
        /// Emit a machine-readable JSON report instead of the human table.
        #[arg(long)]
        json: bool,
    },

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
    Identity {
        /// Emit a machine-readable JSON report instead of the raw hex string.
        #[arg(long)]
        json: bool,
    },
}

/// Errors from vault CLI operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
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

    /// The passphrase was empty.
    #[snafu(display("passphrase must not be empty — please try again"))]
    PassphraseEmpty,

    /// The user cancelled the operation.
    #[snafu(display("operation cancelled"))]
    Cancelled,

    #[snafu(display("failed to write JSON report: {source}"))]
    JsonReport { source: serde_json::Error },

    #[snafu(display("I/O error: {source}"))]
    Io { source: std::io::Error },
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
///
/// WHY(#379) `Zeroizing`: what the operator types is the same plaintext
/// credential material a `DecryptedEntry` protects on the way out, so it is
/// wrapped on the way in too. A bare `String` leaves the passphrase in freed
/// heap memory after the last use.
fn read_passphrase(prompt: &str) -> Result<Zeroizing<String>, VaultCliError> {
    rpassword::prompt_password(prompt)
        .map(Zeroizing::new)
        .context(PassphraseInputSnafu)
}

/// Validates a passphrase confirmation: the two entries must match, and the
/// result must not be empty.
///
/// Split out as a pure function so this validation is unit-testable
/// directly — `rpassword::prompt_password` reads the real terminal with no
/// injection seam, so `read_passphrase_confirmed` itself cannot be driven
/// from a test.
///
/// # Errors
///
/// Returns [`VaultCliError::PassphraseMismatch`] if `first != second`.
/// Returns [`VaultCliError::PassphraseEmpty`] if the matching value is empty
/// — including two empty entries, which match each other and would
/// otherwise pass the check above silently (forkwright/akroasis#287).
fn confirm_passphrase(first: &str, second: &str) -> Result<(), VaultCliError> {
    if first != second {
        return PassphraseMismatchSnafu.fail();
    }

    if first.is_empty() {
        return PassphraseEmptySnafu.fail();
    }

    Ok(())
}

/// Reads and confirms a new passphrase (double entry).
///
/// Rejects an empty passphrase here, at the interactive boundary, so a
/// double-Enter fails immediately with a clear retry message rather than
/// silently succeeding and relying on `Vault::create`'s own rejection to
/// surface as a less specific downstream error (forkwright/akroasis#287).
fn read_passphrase_confirmed(prompt: &str) -> Result<Zeroizing<String>, VaultCliError> {
    let first = read_passphrase(prompt)?;
    let second = read_passphrase("Confirm passphrase: ")?;

    confirm_passphrase(&first, &second)?;

    Ok(first)
}

/// Reads a secret value from the terminal (no echo).
///
/// WHY(#379) `Zeroizing`: a freshly typed replacement secret is credential
/// material before it ever reaches the vault, and is wrapped for the same
/// reason as [`read_passphrase`].
fn read_secret(prompt: &str) -> Result<Zeroizing<String>, VaultCliError> {
    rpassword::prompt_password(prompt)
        .map(Zeroizing::new)
        .context(PassphraseInputSnafu)
}

/// Dispatches a vault subcommand.
///
/// # Errors
/// Returns [] if the command fails.
pub fn dispatch(cmd: &VaultCommand, out: &mut dyn Write) -> Result<(), VaultCliError> {
    match cmd {
        VaultCommand::Init => run_init(out),
        VaultCommand::Add { name, r#type } => run_add(name, r#type, out),
        VaultCommand::List { json } => run_list(*json, out),
        VaultCommand::Get { name } => run_get(name, out),
        VaultCommand::Rotate { name } => run_rotate(name, out),
        VaultCommand::Revoke { name } => run_revoke(name, out),
        VaultCommand::Identity { json } => run_identity(&default_vault_path(), *json, out),
    }
}

fn run_init(out: &mut dyn Write) -> Result<(), VaultCliError> {
    let path = default_vault_path();

    let passphrase = read_passphrase_confirmed("Vault passphrase: ")?;

    let _vault = Vault::create(&path, passphrase.as_bytes()).context(VaultSnafu)?;

    writeln!(out, "Vault created at {}", path.display()).context(IoSnafu)?;
    Ok(())
}

fn run_add(
    name: &str,
    credential_type: &CredentialType,
    out: &mut dyn Write,
) -> Result<(), VaultCliError> {
    let path = default_vault_path();
    let passphrase = read_passphrase("Vault passphrase: ")?;

    let vault = Vault::open(&path, passphrase.as_bytes()).context(VaultSnafu)?;

    let secret = read_secret("Secret value: ")?;
    vault
        .add(name, credential_type.clone(), secret.as_bytes())
        .context(VaultSnafu)?;

    writeln!(out, "Added '{name}' ({credential_type})").context(IoSnafu)?;
    Ok(())
}

fn run_list(json: bool, out: &mut dyn Write) -> Result<(), VaultCliError> {
    let path = default_vault_path();
    let passphrase = read_passphrase("Vault passphrase: ")?;

    let vault = Vault::open(&path, passphrase.as_bytes()).context(VaultSnafu)?;
    let listing = vault.list().context(VaultSnafu)?;
    let entries = listing.entries;

    if json {
        write_list_json_report(&entries, out)?;
        return Ok(());
    }

    if entries.is_empty() && listing.unreadable == 0 {
        writeln!(out, "Vault is empty.").context(IoSnafu)?;
        return Ok(());
    }

    // WHY(#231): a listing that quietly omitted damaged records would leave the
    // operator unable to tell a credential that is gone from one that cannot be
    // read. The count is surfaced where they are already looking.
    if listing.unreadable > 0 {
        writeln!(
            out,
            "warning: {} entr{} could not be read and {} omitted below.",
            listing.unreadable,
            if listing.unreadable == 1 { "y" } else { "ies" },
            if listing.unreadable == 1 { "is" } else { "are" }
        )
        .context(IoSnafu)?;
    }

    if entries.is_empty() {
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

    writeln!(out, "{table}").context(IoSnafu)?;
    Ok(())
}

fn run_get(name: &str, out: &mut dyn Write) -> Result<(), VaultCliError> {
    let path = default_vault_path();
    let passphrase = read_passphrase("Vault passphrase: ")?;

    let vault = Vault::open(&path, passphrase.as_bytes()).context(VaultSnafu)?;
    let entry = vault.get(name).context(VaultSnafu)?;

    // WHY: credential types include binary classes (Psk, Certificate, RadioKey) that are
    // routinely non-UTF-8 — from_utf8_lossy would substitute U+FFFD and corrupt the secret.
    // Write the stored bytes exactly, with no added trailing newline.
    out.write_all(&entry.secret).context(IoSnafu)?;
    Ok(())
}

fn run_rotate(name: &str, out: &mut dyn Write) -> Result<(), VaultCliError> {
    let path = default_vault_path();
    let passphrase = read_passphrase("Vault passphrase: ")?;

    let vault = Vault::open(&path, passphrase.as_bytes()).context(VaultSnafu)?;

    let new_secret = read_secret("New secret value: ")?;
    vault
        .rotate(name, new_secret.as_bytes())
        .context(VaultSnafu)?;

    writeln!(out, "Rotated '{name}'").context(IoSnafu)?;
    Ok(())
}

fn run_revoke(name: &str, out: &mut dyn Write) -> Result<(), VaultCliError> {
    let path = default_vault_path();
    let passphrase = read_passphrase("Vault passphrase: ")?;

    let vault = Vault::open(&path, passphrase.as_bytes()).context(VaultSnafu)?;

    eprint!("Revoke '{name}'? This cannot be undone. [y/N] ");
    io::stderr().flush().context(IoSnafu)?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context(PassphraseInputSnafu)?;

    if !answer.trim().eq_ignore_ascii_case("y") {
        return CancelledSnafu.fail();
    }

    vault.revoke(name).context(VaultSnafu)?;
    writeln!(out, "Revoked '{name}'").context(IoSnafu)?;
    Ok(())
}

const VAULT_IDENTITY_JSON_SCHEMA: u8 = 1;
const VAULT_LIST_JSON_SCHEMA: u8 = 1;

#[derive(serde::Serialize)]
struct ListReport {
    schema_version: u8,
    command: &'static str,
    entry_count: usize,
    entries: Vec<ListEntryReport>,
}

#[derive(serde::Serialize)]
struct ListEntryReport {
    name: String,
    credential_type: String,
    status: String,
    created_at: String,
    rotated_at: Option<String>,
    revoked_at: Option<String>,
    rotation_count: u32,
    tags: Vec<String>,
}

#[derive(serde::Serialize)]
struct IdentityReport {
    schema_version: u8,
    command: &'static str,
    /// `null` when the vault predates installation identity — distinct from
    /// an empty string, which would read as a key that is present and blank.
    public_key: Option<String>,
}

fn write_list_json_report(entries: &[EntryInfo], out: &mut dyn Write) -> Result<(), VaultCliError> {
    let report = ListReport {
        schema_version: VAULT_LIST_JSON_SCHEMA,
        command: "vault list",
        entry_count: entries.len(),
        entries: entries
            .iter()
            .map(|entry| ListEntryReport {
                name: entry.name.to_string(),
                credential_type: entry.credential_type.to_string(),
                status: entry.status.to_string(),
                created_at: entry.metadata.created_at.to_string(),
                rotated_at: entry.metadata.rotated_at.map(|t| t.to_string()),
                revoked_at: entry.metadata.revoked_at.map(|t| t.to_string()),
                rotation_count: entry.metadata.rotation_count,
                tags: entry
                    .metadata
                    .tags
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            })
            .collect(),
    };

    serde_json::to_writer_pretty(&mut *out, &report).context(JsonReportSnafu)?;
    writeln!(out).context(IoSnafu)?;
    Ok(())
}

/// Reports the vault's persisted installation identity.
///
/// WHY this reads rather than generates: an identity command that mints a
/// fresh key on every invocation answers a different question each time it is
/// asked, so it cannot identify an installation — which is the only thing it
/// is for (forkwright/akroasis#284).
///
/// Needs no passphrase. The verifying key is stored unsealed precisely so
/// that checking provenance does not require the secret.
fn run_identity(path: &Path, json: bool, out: &mut dyn Write) -> Result<(), VaultCliError> {
    let recorded = Vault::installation_public_key(path).context(VaultSnafu)?;
    let public_key = recorded.map(hex_encode);

    if json {
        let report = IdentityReport {
            schema_version: VAULT_IDENTITY_JSON_SCHEMA,
            command: "vault identity",
            public_key: public_key.clone(),
        };
        serde_json::to_writer_pretty(&mut *out, &report).context(JsonReportSnafu)?;
        writeln!(out).context(IoSnafu)?;
        return Ok(());
    }

    match public_key {
        Some(key) => writeln!(out, "{key}").context(IoSnafu)?,
        // WHY a message rather than an error or a freshly minted key: a vault
        // created before installation identity existed genuinely has none, and
        // both alternatives misreport that — an error implies breakage, and a
        // generated key implies an answer.
        None => writeln!(
            out,
            "no installation identity recorded (vault predates installation identity)"
        )
        .context(IoSnafu)?,
    }
    Ok(())
}

/// Lowercase hex, matching the fingerprint format `VerifyingKey` renders.
fn hex_encode(bytes: Vec<u8>) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Formats a jiff Timestamp as a human-readable string.
fn format_timestamp(ts: &jiff::Timestamp) -> String {
    ts.strftime("%Y-%m-%d %H:%M").to_string()
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions use unwrap and indexing for clarity"
)]
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
        match cmd {
            VaultCommand::List { json } => assert!(!json),
            _ => unreachable!("expected List"),
        }
    }

    #[test]
    fn parse_list_json_flag() {
        let cmd = parse(&["list", "--json"]);
        match cmd {
            VaultCommand::List { json } => assert!(json),
            _ => unreachable!("expected List"),
        }
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
        match cmd {
            VaultCommand::Identity { json } => assert!(!json),
            _ => unreachable!("expected Identity"),
        }
    }

    #[test]
    fn parse_identity_json_flag() {
        let cmd = parse(&["identity", "--json"]);
        match cmd {
            VaultCommand::Identity { json } => assert!(json),
            _ => unreachable!("expected Identity"),
        }
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
    // Passphrase confirmation (akroasis#287)
    // -----------------------------------------------------------------

    #[test]
    fn confirm_passphrase_rejects_two_empty_entries() {
        // The double-Enter case the issue names: two empty entries MATCH
        // each other, so the mismatch check alone would let this through.
        let result = confirm_passphrase("", "");
        assert!(
            matches!(result, Err(VaultCliError::PassphraseEmpty)),
            "two empty passphrase entries must be refused, got {result:?}"
        );
    }

    #[test]
    fn confirm_passphrase_rejects_mismatched_entries() {
        let result = confirm_passphrase("first-entry", "second-entry");
        assert!(
            matches!(result, Err(VaultCliError::PassphraseMismatch)),
            "mismatched entries must be refused, got {result:?}"
        );
    }

    #[test]
    fn confirm_passphrase_accepts_matching_nonempty_entries() {
        let result = confirm_passphrase(
            "correct horse battery staple",
            "correct horse battery staple",
        );
        assert!(
            result.is_ok(),
            "matching non-empty entries must be accepted, got {result:?}"
        );
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
        assert_eq!(entry.secret.as_slice(), b"secret-123".as_slice());
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

        let entries = vault.list().unwrap().entries;
        assert_eq!(entries.len(), 2, "list must return both entries");
    }

    #[test]
    fn write_list_json_report_outputs_metadata_without_secrets() {
        let (_dir, vault) = create_temp_vault();

        vault
            .add("json-key", CredentialType::ApiKey, b"secret-not-in-report")
            .unwrap();

        let entries = vault.list().unwrap().entries;
        let mut out = Vec::new();
        write_list_json_report(&entries, &mut out).unwrap();

        let raw = String::from_utf8(out.clone()).unwrap();
        assert!(
            !raw.contains("secret-not-in-report"),
            "list report must not contain secret material"
        );

        let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["command"], "vault list");
        assert_eq!(report["entry_count"], 1);

        let entry = &report["entries"][0];
        assert_eq!(entry["name"], "json-key");
        assert_eq!(entry["credential_type"], "api-key");
        assert_eq!(entry["status"], "active");
        assert!(entry["created_at"].as_str().unwrap().ends_with('Z'));
        assert!(entry["rotated_at"].is_null());
        assert_eq!(entry["rotation_count"], 0);
    }

    #[test]
    fn vault_rotate_updates_secret() {
        let (_dir, vault) = create_temp_vault();

        vault
            .add("rotate-key", CredentialType::ApiKey, b"old-secret")
            .unwrap();
        vault.rotate("rotate-key", b"new-secret").unwrap();

        let entry = vault.get("rotate-key").unwrap();
        assert_eq!(entry.secret.as_slice(), b"new-secret".as_slice());
    }

    #[test]
    fn vault_get_round_trip_preserves_non_utf8_secret() {
        // NOTE: this exercises the same vault.get() data path run_get() reads from —
        // run_get itself reads its passphrase interactively via rpassword and has no
        // seam for injecting one in a unit test, so this pins the byte-exact invariant
        // at the Vault layer instead: a non-UTF-8 secret (invalid as UTF-8, so lossy
        // decoding would corrupt it) must round-trip unchanged.
        let (_dir, vault) = create_temp_vault();
        let non_utf8_secret: &[u8] = b"\xFF\xFE\x00secret";
        vault
            .add("binary-key", CredentialType::Psk, non_utf8_secret)
            .unwrap();

        let entry = vault.get("binary-key").unwrap();
        assert_eq!(
            entry.secret.as_slice(),
            non_utf8_secret,
            "secret bytes must round-trip exactly, with no UTF-8 lossy substitution"
        );
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
    fn run_identity_outputs_pubkey() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        drop(Vault::create(&path, b"correct horse battery staple").unwrap());

        let mut out = Vec::new();
        run_identity(&path, false, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        let trimmed = s.trim();
        assert_eq!(trimmed.len(), 64, "hex fingerprint must be 64 characters");
        assert!(
            trimmed.chars().all(|c| c.is_ascii_hexdigit()),
            "fingerprint must be hex"
        );
    }

    /// The defect this command had: it minted a key per invocation, so it
    /// reported a different installation every time it was asked which
    /// installation this is.
    #[test]
    fn run_identity_reports_the_same_key_on_every_invocation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        drop(Vault::create(&path, b"correct horse battery staple").unwrap());

        let mut first = Vec::new();
        run_identity(&path, false, &mut first).unwrap();
        let mut second = Vec::new();
        run_identity(&path, false, &mut second).unwrap();

        assert_eq!(
            first, second,
            "the installation identity must not change between invocations"
        );
    }

    /// A vault written before this field existed has no identity, and the
    /// command must say so rather than erroring or inventing one.
    #[test]
    fn run_identity_reports_absence_for_a_vault_without_an_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        drop(Vault::create(&path, b"correct horse battery staple").unwrap());

        // Strip the identity fields, reproducing a pre-#284 header.
        let header_path = path.join("header.json");
        let mut header: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&header_path).unwrap()).unwrap();
        let obj = header.as_object_mut().unwrap();
        obj.remove("installation_public_key");
        obj.remove("sealed_signing_key");
        std::fs::write(&header_path, serde_json::to_vec(&header).unwrap()).unwrap();

        let mut out = Vec::new();
        run_identity(&path, false, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.contains("no installation identity recorded"),
            "must report absence plainly, got {s:?}"
        );

        let mut json_out = Vec::new();
        run_identity(&path, true, &mut json_out).unwrap();
        let report: serde_json::Value = serde_json::from_slice(&json_out).unwrap();
        assert!(
            report["public_key"].is_null(),
            "absence must serialize as null, not an empty string"
        );
    }

    #[test]
    fn run_identity_json_outputs_machine_readable_report() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        drop(Vault::create(&path, b"correct horse battery staple").unwrap());

        let mut out = Vec::new();
        run_identity(&path, true, &mut out).unwrap();

        let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["command"], "vault identity");
        let pk = report["public_key"].as_str().unwrap();
        assert_eq!(pk.len(), 64, "hex fingerprint must be 64 characters");
        assert!(
            pk.chars().all(|c| c.is_ascii_hexdigit()),
            "fingerprint must be hex"
        );
    }
}
