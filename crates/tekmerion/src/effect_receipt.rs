//! Minimized receipts for protected effects.
//!
//! The schema carries only opaque references, epochs, closed state, and
//! allowlisted digests. Raw paths, frequencies, rules, payloads, content,
//! keys, credentials, and free-form descriptions have no field to occupy.

use serde::{Deserialize, Deserializer, Serialize};
use snafu::Snafu;

use crate::caller::{
    AuthorizedCaller, CallerRef, CapabilityRef, EffectRef, EvidenceDigest, PolicyEpoch,
    ReceiptDigest, SchemaEpoch,
};
use crate::effect_receipt_state::{PendingIntent, RecoveryAuthorization};
use stoicheion::Timestamp;

/// Current serialized effect-receipt schema version.
pub const EFFECT_RECEIPT_VERSION: u16 = 1;

/// Closed outcome of a protected effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EffectOutcome {
    // kanon:ignore RUST/non-exhaustive-enum -- closed receipt vocabulary; changes require a receipt-contract version bump
    /// The effect completed successfully.
    Succeeded,
    /// The effect failed without a committed state change.
    Failed,
    /// The effect changed state only partially.
    Partial,
    /// Admission was cancelled before the domain effect ran.
    Cancelled,
    /// Admission was refused by a bounded resource budget.
    Backpressured,
    /// Caller authority lapsed after durable intent but before domain I/O.
    AuthorizationDenied,
    /// Restart reconciliation found an unmatched or uncertain intent.
    RecoveryRequired,
    /// An explicitly authorized recovery completed.
    Recovered,
}

/// Typed relation between an operation and recovery work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RecoveryRelation {
    // kanon:ignore RUST/non-exhaustive-enum -- closed recovery state machine rejects unknown wire variants
    /// The operation is not recovery work and needs no recovery.
    None,
    /// The referenced operation requires explicit recovery.
    RequiredFor(EffectRef),
    /// This operation is an explicit recovery of the referenced effect.
    RecoveryOf(EffectRef),
}

/// Closed event carried by an [`EffectReceipt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReceiptEvent {
    // kanon:ignore RUST/non-exhaustive-enum -- receipt schema intentionally permits only intent and outcome
    /// Durable intent recorded before invoking the effect.
    Intent,
    /// Terminal, partial, or recovery state after an intent.
    Outcome(EffectOutcome),
}

/// Proven parent/child relation between two durable receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValidatedReceiptLink {
    // kanon:ignore RUST/non-exhaustive-enum -- closed transition proof; unknown link kinds must not be admitted
    /// An outcome is bound to its exact protected intent.
    Outcome,
    /// A recovery intent is bound to one unresolved outcome.
    RecoveryIntent,
}

/// Immutable metadata describing one protected operation.
#[derive(Debug, PartialEq, Eq)]
pub struct EffectDescriptor {
    effect: EffectRef,
    schema_epoch: SchemaEpoch,
    evidence_digest: Option<EvidenceDigest>,
    recovery: Option<RecoveryAuthorization>,
}

impl EffectDescriptor {
    /// Construct metadata for a normal protected operation.
    #[must_use]
    pub const fn new(
        effect: EffectRef,
        schema_epoch: SchemaEpoch,
        evidence_digest: Option<EvidenceDigest>,
    ) -> Self {
        Self {
            effect,
            schema_epoch,
            evidence_digest,
            recovery: None,
        }
    }

    pub(crate) const fn from_recovery(
        effect: EffectRef,
        schema_epoch: SchemaEpoch,
        evidence_digest: Option<EvidenceDigest>,
        recovery: RecoveryAuthorization,
    ) -> Self {
        Self {
            effect,
            schema_epoch,
            evidence_digest,
            recovery: Some(recovery),
        }
    }

    /// Return the protected effect reference.
    #[must_use]
    pub const fn effect(&self) -> EffectRef {
        self.effect
    }

    /// Return the governing schema epoch.
    #[must_use]
    pub const fn schema_epoch(&self) -> SchemaEpoch {
        self.schema_epoch
    }

    /// Return the optional allowlisted evidence digest.
    #[must_use]
    pub const fn evidence_digest(&self) -> Option<EvidenceDigest> {
        self.evidence_digest
    }

    /// Return the original effect when this is recovery work.
    #[must_use]
    pub const fn recovery_of(&self) -> Option<EffectRef> {
        match &self.recovery {
            Some(recovery) => Some(recovery.original_effect()),
            None => None,
        }
    }
}

/// Stable references shared by receipts for one protected operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptContext {
    caller_ref: CallerRef,
    effect: EffectRef,
    capability: CapabilityRef,
    policy_epoch: PolicyEpoch,
    schema_epoch: SchemaEpoch,
}

impl ReceiptContext {
    const fn from_authorized(
        caller: &AuthorizedCaller,
        effect: EffectRef,
        schema_epoch: SchemaEpoch,
    ) -> Self {
        Self {
            caller_ref: caller.caller_ref(),
            effect,
            capability: caller.grant().capability(),
            policy_epoch: caller.policy_epoch(),
            schema_epoch,
        }
    }

    /// Return the canonical caller reference.
    #[must_use]
    pub const fn caller_ref(&self) -> CallerRef {
        self.caller_ref
    }

    /// Return the protected effect reference.
    #[must_use]
    pub const fn effect(&self) -> EffectRef {
        self.effect
    }

    /// Return the checked capability reference.
    #[must_use]
    pub const fn capability(&self) -> CapabilityRef {
        self.capability
    }

    /// Return the granting policy epoch.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }

    /// Return the domain schema epoch.
    #[must_use]
    pub const fn schema_epoch(&self) -> SchemaEpoch {
        self.schema_epoch
    }
}

/// One versioned, minimized intent or outcome receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EffectReceipt {
    schema_version: u16,
    context: ReceiptContext,
    event: ReceiptEvent,
    observed_at: Timestamp,
    evidence_digest: Option<EvidenceDigest>,
    predecessor: Option<ReceiptDigest>,
    recovery: RecoveryRelation,
}

impl EffectReceipt {
    /// Construct an intent from a linear, effect-time authorization proof.
    ///
    /// # Errors
    ///
    /// Returns [`EffectReceiptError::RecoveryIntentPrecedesRequirement`] when
    /// a recovery operation is observed before its durable requirement.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "one authorization proof must be consumed by exactly one intent"
    )]
    pub fn intent(
        caller: AuthorizedCaller,
        descriptor: EffectDescriptor,
        observed_at: Timestamp,
    ) -> Result<PendingIntent, EffectReceiptError> {
        let EffectDescriptor {
            effect,
            schema_epoch,
            evidence_digest,
            recovery,
        } = descriptor;
        let (predecessor, recovery) = match recovery {
            Some(recovery) => {
                let (original_effect, requirement_digest, requirement_observed_at) =
                    recovery.into_parts();
                if observed_at < requirement_observed_at {
                    return Err(EffectReceiptError::RecoveryIntentPrecedesRequirement {
                        location: snafu::location!(),
                    });
                }
                (
                    Some(requirement_digest),
                    RecoveryRelation::RecoveryOf(original_effect),
                )
            }
            None => (None, RecoveryRelation::None),
        };
        Ok(PendingIntent::new(Self {
            schema_version: EFFECT_RECEIPT_VERSION,
            context: ReceiptContext::from_authorized(&caller, effect, schema_epoch),
            event: ReceiptEvent::Intent,
            observed_at,
            evidence_digest,
            predecessor,
            recovery,
        }))
    }

    pub(crate) fn outcome_from(
        intent: &Self,
        intent_digest: ReceiptDigest,
        observed_at: Timestamp,
        outcome: EffectOutcome,
        evidence_digest: Option<EvidenceDigest>,
    ) -> Result<Self, EffectReceiptError> {
        if intent.event != ReceiptEvent::Intent {
            return Err(EffectReceiptError::PredecessorIsNotIntent {
                location: snafu::location!(),
            });
        }
        let valid_intent = matches!(
            (intent.predecessor, intent.recovery),
            (None, RecoveryRelation::None) | (Some(_), RecoveryRelation::RecoveryOf(_))
        );
        if !valid_intent {
            return Err(EffectReceiptError::InvalidIntentShape {
                location: snafu::location!(),
            });
        }
        if observed_at < intent.observed_at {
            return Err(EffectReceiptError::OutcomePrecedesIntent {
                location: snafu::location!(),
            });
        }

        let (outcome, recovery) = match (outcome, intent.recovery) {
            (
                EffectOutcome::Succeeded | EffectOutcome::Recovered,
                RecoveryRelation::RecoveryOf(original),
            ) => (
                EffectOutcome::Recovered,
                RecoveryRelation::RecoveryOf(original),
            ),
            (EffectOutcome::Recovered, _) => {
                return Err(EffectReceiptError::RecoveryRelationMismatch {
                    location: snafu::location!(),
                });
            }
            (EffectOutcome::Partial | EffectOutcome::RecoveryRequired, _) => (
                outcome,
                RecoveryRelation::RequiredFor(intent.context.effect),
            ),
            (other, relation @ RecoveryRelation::RecoveryOf(_)) => (other, relation),
            (other, RecoveryRelation::None) => (other, RecoveryRelation::None),
            (_, RecoveryRelation::RequiredFor(_)) => {
                return Err(EffectReceiptError::RecoveryRelationMismatch {
                    location: snafu::location!(),
                });
            }
        };

        Self::validate(
            EFFECT_RECEIPT_VERSION,
            intent.context,
            ReceiptEvent::Outcome(outcome),
            observed_at,
            evidence_digest,
            Some(intent_digest),
            recovery,
        )
    }

    /// Return the receipt schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Return the minimized receipt context.
    #[must_use]
    pub const fn context(&self) -> &ReceiptContext {
        &self.context
    }

    /// Return the closed event.
    #[must_use]
    pub const fn event(&self) -> ReceiptEvent {
        self.event
    }

    /// Return the observation time.
    #[must_use]
    pub const fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    /// Return the optional allowlisted evidence digest.
    #[must_use]
    pub const fn evidence_digest(&self) -> Option<EvidenceDigest> {
        self.evidence_digest
    }

    /// Return the durable predecessor for an outcome or recovery intent.
    #[must_use]
    pub const fn predecessor(&self) -> Option<ReceiptDigest> {
        self.predecessor
    }

    /// Return the typed recovery relation.
    #[must_use]
    pub const fn recovery(&self) -> RecoveryRelation {
        self.recovery
    }

    /// Validate this receipt as the exact child of a durable predecessor.
    ///
    /// Ledger adapters call this before accepting a child. Outcome context is
    /// copied exactly from its intent. Recovery intents may use a new effect
    /// context, but their recovery target must equal the unresolved target
    /// named by the predecessor outcome.
    ///
    /// # Errors
    ///
    /// Returns a typed contract error when the digest, time, context, event,
    /// or recovery target does not form an allowed transition.
    pub fn validate_child_of(
        &self,
        predecessor: &Self,
        predecessor_digest: ReceiptDigest,
    ) -> Result<ValidatedReceiptLink, EffectReceiptError> {
        if self.predecessor != Some(predecessor_digest) {
            return Err(EffectReceiptError::PredecessorDigestMismatch {
                location: snafu::location!(),
            });
        }
        if self.observed_at < predecessor.observed_at {
            return Err(EffectReceiptError::ChildPrecedesPredecessor {
                location: snafu::location!(),
            });
        }
        match (self.event, predecessor.event) {
            (ReceiptEvent::Outcome(outcome), ReceiptEvent::Intent) => {
                self.validate_outcome_child(predecessor, outcome)?;
                Ok(ValidatedReceiptLink::Outcome)
            }
            (ReceiptEvent::Intent, ReceiptEvent::Outcome(outcome)) => {
                self.validate_recovery_intent_child(predecessor, outcome)?;
                Ok(ValidatedReceiptLink::RecoveryIntent)
            }
            _ => Err(EffectReceiptError::InvalidPredecessorTransition {
                location: snafu::location!(),
            }),
        }
    }

    fn validate_outcome_child(
        &self,
        intent: &Self,
        outcome: EffectOutcome,
    ) -> Result<(), EffectReceiptError> {
        if self.context != intent.context {
            return Err(EffectReceiptError::PredecessorContextMismatch {
                location: snafu::location!(),
            });
        }
        let expected = expected_outcome_relation(intent, outcome)?;
        if self.recovery != expected {
            return Err(EffectReceiptError::RecoveryTargetMismatch {
                location: snafu::location!(),
            });
        }
        Ok(())
    }

    fn validate_recovery_intent_child(
        &self,
        outcome: &Self,
        outcome_kind: EffectOutcome,
    ) -> Result<(), EffectReceiptError> {
        let expected = match (outcome_kind, outcome.recovery) {
            (
                EffectOutcome::Partial | EffectOutcome::RecoveryRequired,
                RecoveryRelation::RequiredFor(effect),
            )
            | (
                EffectOutcome::Failed
                | EffectOutcome::Cancelled
                | EffectOutcome::Backpressured
                | EffectOutcome::AuthorizationDenied,
                RecoveryRelation::RecoveryOf(effect),
            ) => RecoveryRelation::RecoveryOf(effect),
            _ => {
                return Err(EffectReceiptError::InvalidPredecessorTransition {
                    location: snafu::location!(),
                });
            }
        };
        if self.recovery != expected {
            return Err(EffectReceiptError::RecoveryTargetMismatch {
                location: snafu::location!(),
            });
        }
        Ok(())
    }

    fn validate(
        schema_version: u16,
        context: ReceiptContext,
        event: ReceiptEvent,
        observed_at: Timestamp,
        evidence_digest: Option<EvidenceDigest>,
        predecessor: Option<ReceiptDigest>,
        recovery: RecoveryRelation,
    ) -> Result<Self, EffectReceiptError> {
        if schema_version != EFFECT_RECEIPT_VERSION {
            return Err(EffectReceiptError::UnknownVersion {
                schema_version,
                location: snafu::location!(),
            });
        }
        match event {
            ReceiptEvent::Intent => {
                let valid = matches!((predecessor, recovery), (None, RecoveryRelation::None))
                    || matches!(
                        (predecessor, recovery),
                        (Some(_), RecoveryRelation::RecoveryOf(_))
                    );
                if !valid {
                    return Err(EffectReceiptError::InvalidIntentShape {
                        location: snafu::location!(),
                    });
                }
            }
            ReceiptEvent::Outcome(outcome) => {
                if predecessor.is_none() {
                    return Err(EffectReceiptError::MissingPredecessor {
                        location: snafu::location!(),
                    });
                }
                validate_outcome_relation(context.effect, outcome, recovery)?;
            }
        }
        Ok(Self {
            schema_version,
            context,
            event,
            observed_at,
            evidence_digest,
            predecessor,
            recovery,
        })
    }
}

const fn expected_outcome_relation(
    intent: &EffectReceipt,
    outcome: EffectOutcome,
) -> Result<RecoveryRelation, EffectReceiptError> {
    match (intent.recovery, outcome) {
        (
            RecoveryRelation::None,
            EffectOutcome::Succeeded
            | EffectOutcome::Failed
            | EffectOutcome::Cancelled
            | EffectOutcome::Backpressured
            | EffectOutcome::AuthorizationDenied,
        ) => Ok(RecoveryRelation::None),
        (RecoveryRelation::None, EffectOutcome::Partial | EffectOutcome::RecoveryRequired) => {
            Ok(RecoveryRelation::RequiredFor(intent.context.effect))
        }
        (
            RecoveryRelation::RecoveryOf(effect),
            EffectOutcome::Failed
            | EffectOutcome::Cancelled
            | EffectOutcome::Backpressured
            | EffectOutcome::AuthorizationDenied
            | EffectOutcome::Recovered,
        ) => Ok(RecoveryRelation::RecoveryOf(effect)),
        (
            RecoveryRelation::RecoveryOf(_),
            EffectOutcome::Partial | EffectOutcome::RecoveryRequired,
        ) => Ok(RecoveryRelation::RequiredFor(intent.context.effect)),
        _ => Err(EffectReceiptError::InvalidPredecessorTransition {
            location: snafu::location!(),
        }),
    }
}

fn validate_outcome_relation(
    effect: EffectRef,
    outcome: EffectOutcome,
    recovery: RecoveryRelation,
) -> Result<(), EffectReceiptError> {
    let valid = match outcome {
        EffectOutcome::Partial | EffectOutcome::RecoveryRequired => {
            recovery == RecoveryRelation::RequiredFor(effect)
        }
        EffectOutcome::Recovered => matches!(recovery, RecoveryRelation::RecoveryOf(_)),
        EffectOutcome::Succeeded => recovery == RecoveryRelation::None,
        EffectOutcome::Failed
        | EffectOutcome::Cancelled
        | EffectOutcome::Backpressured
        | EffectOutcome::AuthorizationDenied => matches!(
            recovery,
            RecoveryRelation::None | RecoveryRelation::RecoveryOf(_)
        ),
    };
    if valid {
        Ok(())
    } else {
        Err(EffectReceiptError::RecoveryRelationMismatch {
            location: snafu::location!(),
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEffectReceipt {
    schema_version: u16,
    context: ReceiptContext,
    event: ReceiptEvent,
    observed_at: Timestamp,
    evidence_digest: Option<EvidenceDigest>,
    predecessor: Option<ReceiptDigest>,
    recovery: RecoveryRelation,
}

impl<'de> Deserialize<'de> for EffectReceipt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawEffectReceipt::deserialize(deserializer)?;
        Self::validate(
            raw.schema_version,
            raw.context,
            raw.event,
            raw.observed_at,
            raw.evidence_digest,
            raw.predecessor,
            raw.recovery,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Errors returned when a minimized effect receipt violates its schema.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum EffectReceiptError {
    /// The serialized schema version is not understood.
    #[snafu(display("unknown effect-receipt schema version {schema_version}"))]
    UnknownVersion {
        /// Unsupported version.
        schema_version: u16,
        /// Source location of the invalid receipt.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// An intent carried an invalid predecessor or recovery relation.
    #[snafu(display("effect intent has an invalid predecessor or recovery relation"))]
    InvalidIntentShape {
        /// Source location of the invalid receipt.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// An outcome was not linked to its durable intent.
    #[snafu(display("effect outcome requires a durable intent predecessor"))]
    MissingPredecessor {
        /// Source location of the invalid receipt.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// An outcome constructor was given a non-intent predecessor.
    #[snafu(display("effect outcome predecessor is not an intent receipt"))]
    PredecessorIsNotIntent {
        /// Source location of the invalid receipt.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// A persisted receipt presented as an outcome was not one.
    #[snafu(display("persisted receipt is not an effect outcome"))]
    PersistedReceiptIsNotOutcome {
        /// Source location of the invalid reconstruction.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// A terminal outcome was presented as recovery authority.
    #[snafu(display("persisted outcome does not require recovery"))]
    NotRecoveryRequirement {
        /// Source location of the invalid transition.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Outcome time moved backwards relative to its intent.
    #[snafu(display("effect outcome precedes its durable intent"))]
    OutcomePrecedesIntent {
        /// Source location of the invalid transition.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// A recovery intent predates the durable requirement authorizing it.
    #[snafu(display("recovery intent precedes its durable recovery requirement"))]
    RecoveryIntentPrecedesRequirement {
        /// Source location of the invalid transition.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// A child did not name the supplied predecessor's canonical digest.
    #[snafu(display("effect receipt predecessor digest does not match"))]
    PredecessorDigestMismatch {
        /// Source location of the invalid transition.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// A child receipt predates its durable predecessor.
    #[snafu(display("effect receipt child precedes its durable predecessor"))]
    ChildPrecedesPredecessor {
        /// Source location of the invalid transition.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// An outcome changed the protected intent's receipt context.
    #[snafu(display("effect outcome context does not match its intent"))]
    PredecessorContextMismatch {
        /// Source location of the invalid transition.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Parent and child events do not form an allowed receipt transition.
    #[snafu(display("effect receipts do not form an allowed predecessor transition"))]
    InvalidPredecessorTransition {
        /// Source location of the invalid transition.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// A recovery child named a different unresolved operation.
    #[snafu(display("effect receipt recovery target does not match its predecessor"))]
    RecoveryTargetMismatch {
        /// Source location of the invalid transition.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The outcome and recovery relation describe incompatible state.
    #[snafu(display("effect outcome has an incompatible recovery relation"))]
    RecoveryRelationMismatch {
        /// Source location of the invalid receipt.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
