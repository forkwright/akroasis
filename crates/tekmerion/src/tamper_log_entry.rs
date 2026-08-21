//! [`LogEntryKind`] — the event payload variants recorded in a [`crate::tamper_log::LogEntry`].

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

use stoicheion::{EntityId, SignalId};

/// The kind of event recorded in a [`crate::tamper_log::LogEntry`].
#[non_exhaustive]
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub enum LogEntryKind {
    /// A signal was observed by a collector.
    SignalObserved {
        /// Identifier of the observed signal.
        signal_id: SignalId,
        /// Short tag describing the signal kind.
        kind_tag: CompactString,
    },
    /// A new entity was created in the system.
    EntityCreated {
        /// Identifier of the created entity.
        entity_id: EntityId,
        /// Short tag describing the entity kind.
        kind_tag: CompactString,
    },
    /// A configuration parameter was changed.
    ConfigChanged {
        /// Configuration key that changed.
        key: CompactString,
        /// Previous value, if any.
        old_value: Option<CompactString>,
        /// New value after the change.
        new_value: CompactString,
    },
    /// An alert was raised by the analysis pipeline.
    AlertRaised {
        /// Unique identifier for this alert.
        alert_id: CompactString,
        /// Severity level (e.g. `"critical"`, `"warning"`).
        severity: CompactString,
        /// Human-readable alert message.
        message: CompactString,
    },
    /// An operator or automation took an action.
    ActionTaken {
        /// Identity of the actor (user or system).
        actor: CompactString,
        /// Description of the action performed.
        action: CompactString,
        /// Target of the action, if applicable.
        target: Option<CompactString>,
    },
    /// A credential vault entry lifecycle mutation was committed.
    VaultMutation {
        /// Opaque reference to the credential the mutation affected.
        ///
        /// NOT a name. The writer derives this from the credential's name
        /// under a secret it holds, so the same credential yields the same
        /// reference across entries — enough to follow one credential's
        /// history through the log — while a reader without that secret learns
        /// nothing about what any of them are called
        /// (forkwright/akroasis#378).
        ///
        /// This type does not derive the reference; it only carries it. What
        /// the derivation is, and therefore who can reverse it, belongs to the
        /// writer.
        credential_ref: CompactString,
        /// Mutation operation, e.g. `"add"`, `"rotate"`, `"revoke"`, or `"remove"`.
        operation: CompactString,
    },
}

// WHY: manual Debug instead of #[derive(Debug)] — `VaultMutation` carries a
// credential reference. It is derived rather than plaintext (#378), so this
// redaction is no longer the only thing standing between a credential name and
// a log file; it stays because a stable per-credential identifier is still a
// correlation handle, and Debug output travels further than the audit log does
// (RUST/no-debug-derive-on-public-types).
impl std::fmt::Debug for LogEntryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignalObserved {
                signal_id,
                kind_tag,
            } => f
                .debug_struct("SignalObserved")
                .field("signal_id", signal_id)
                .field("kind_tag", kind_tag)
                .finish(),
            Self::EntityCreated {
                entity_id,
                kind_tag,
            } => f
                .debug_struct("EntityCreated")
                .field("entity_id", entity_id)
                .field("kind_tag", kind_tag)
                .finish(),
            Self::ConfigChanged {
                key,
                old_value,
                new_value,
            } => f
                .debug_struct("ConfigChanged")
                .field("key", key)
                .field("old_value", old_value)
                .field("new_value", new_value)
                .finish(),
            Self::AlertRaised {
                alert_id,
                severity,
                message,
            } => f
                .debug_struct("AlertRaised")
                .field("alert_id", alert_id)
                .field("severity", severity)
                .field("message", message)
                .finish(),
            Self::ActionTaken {
                actor,
                action,
                target,
            } => f
                .debug_struct("ActionTaken")
                .field("actor", actor)
                .field("action", action)
                .field("target", target)
                .finish(),
            Self::VaultMutation {
                credential_ref: _,
                operation,
            } => f
                .debug_struct("VaultMutation")
                .field("credential_ref", &"<redacted>")
                .field("operation", operation)
                .finish(),
        }
    }
}
