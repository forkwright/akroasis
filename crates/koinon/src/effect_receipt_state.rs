//! Linear persistence and recovery states for effect receipts.

use snafu::Snafu;

use crate::Timestamp;
use crate::caller::{EffectRef, EvidenceDigest, ReceiptDigest, SchemaEpoch};
use crate::effect_receipt::{
    EffectDescriptor, EffectOutcome, EffectReceipt, EffectReceiptError, ReceiptEvent,
    RecoveryRelation,
};

/// Append-only application ledger for minimized effect receipts.
///
/// Implementations bind this contract to the accepted tamper-log primitive;
/// domains do not create a second receipt schema or log. Linked ledger
/// adapters are trusted application composition, not request data.
pub trait EffectReceiptSink {
    /// Sink-specific append failure.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Durably append one receipt and return its canonical ledger digest.
    ///
    /// The returned digest must identify exactly one accepted ledger entry.
    /// An adapter must either allocate a distinct position/hash-chain digest
    /// or atomically reject a byte-identical receipt already present. It must
    /// validate child receipts with [`EffectReceipt::validate_child_of`]
    /// before returning success, so an effect never runs on ambiguous intent.
    ///
    /// # Errors
    ///
    /// Returns the adapter's typed failure when the receipt was not durably
    /// accepted. Callers must not construct a persisted handle on failure.
    fn append(&mut self, receipt: &EffectReceipt) -> Result<ReceiptDigest, Self::Error>;
}

/// Linear intent that has not yet been durably appended.
#[derive(Debug)]
pub struct PendingIntent {
    receipt: EffectReceipt,
}

impl PendingIntent {
    pub(crate) const fn new(receipt: EffectReceipt) -> Self {
        Self { receipt }
    }

    /// Borrow the intent receipt before durable append.
    #[must_use]
    pub const fn receipt(&self) -> &EffectReceipt {
        &self.receipt
    }

    /// Append the exact intent and return a sealed persisted handle.
    ///
    /// A handle cannot be fabricated from a bare digest during normal
    /// execution. The accepted ledger remains a trusted application adapter;
    /// hostile request data never implements it.
    ///
    /// # Errors
    ///
    /// Returns the ledger error without producing a persisted handle.
    pub fn append_to<S>(self, sink: &mut S) -> Result<PersistedIntent, S::Error>
    where
        S: EffectReceiptSink,
    {
        let digest = sink.append(&self.receipt)?;
        Ok(PersistedIntent {
            receipt: self.receipt,
            digest,
        })
    }
}

/// Sealed proof that one exact intent was accepted by the application ledger.
///
/// The handle is linear, non-serializable, and the only normal constructor for
/// an outcome. Restart reconciliation may reconstruct it only from a receipt
/// the accepted ledger has verified as durably present and unmatched.
#[derive(Debug)]
pub struct PersistedIntent {
    receipt: EffectReceipt,
    digest: ReceiptDigest,
}

impl PersistedIntent {
    /// Reconstruct an unmatched intent after accepted-ledger verification.
    ///
    /// This is a ledger boundary, not a parser for request data.
    ///
    /// # Errors
    ///
    /// Returns [`EffectReceiptError::PredecessorIsNotIntent`] unless the
    /// verified receipt is an intent.
    pub fn from_verified_unmatched(
        receipt: EffectReceipt,
        digest: ReceiptDigest,
    ) -> Result<Self, EffectReceiptError> {
        if receipt.event() != ReceiptEvent::Intent {
            return Err(EffectReceiptError::PredecessorIsNotIntent {
                location: snafu::location!(),
            });
        }
        Ok(Self { receipt, digest })
    }

    /// Borrow the persisted intent receipt.
    #[must_use]
    pub const fn receipt(&self) -> &EffectReceipt {
        &self.receipt
    }

    /// Consume this intent into exactly one pending outcome.
    ///
    /// # Errors
    ///
    /// Returns a typed transition error carrying the same reconciliation
    /// candidate when time moves backwards or the transition is invalid.
    pub fn outcome(
        self,
        observed_at: Timestamp,
        outcome: EffectOutcome,
        evidence_digest: Option<EvidenceDigest>,
    ) -> Result<PendingOutcome, ReceiptTransitionError> {
        let receipt = match EffectReceipt::outcome_from(
            &self.receipt,
            self.digest,
            observed_at,
            outcome,
            evidence_digest,
        ) {
            Ok(receipt) => receipt,
            Err(source) => {
                return Err(ReceiptTransitionError {
                    source,
                    recovery: Box::new(RecoveryTicket { intent: self }),
                    location: snafu::location!(),
                });
            }
        };
        Ok(PendingOutcome {
            receipt,
            intent: self,
        })
    }

    /// Consume the unmatched intent into an explicit reconciliation ticket.
    #[must_use]
    pub const fn into_recovery_ticket(self) -> RecoveryTicket {
        RecoveryTicket { intent: self }
    }
}

/// Linear outcome waiting for durable append.
#[derive(Debug)]
pub struct PendingOutcome {
    receipt: EffectReceipt,
    intent: PersistedIntent,
}

impl PendingOutcome {
    /// Borrow the pending outcome receipt.
    #[must_use]
    pub const fn receipt(&self) -> &EffectReceipt {
        &self.receipt
    }

    /// Append the outcome and bind its ledger digest to the transition.
    ///
    /// # Errors
    ///
    /// Returns the sink error plus the original unmatched intent ticket.
    pub fn append_to<S>(
        self,
        sink: &mut S,
    ) -> Result<PersistedOutcome, OutcomeAppendError<S::Error>>
    where
        S: EffectReceiptSink,
    {
        let digest = match sink.append(&self.receipt) {
            Ok(digest) => digest,
            Err(source) => {
                return Err(OutcomeAppendError {
                    source,
                    recovery: Box::new(RecoveryTicket {
                        intent: self.intent,
                    }),
                    location: snafu::location!(),
                });
            }
        };
        Ok(PersistedOutcome {
            receipt: self.receipt,
            digest,
        })
    }
}

/// One outcome proven present by the accepted application ledger.
#[derive(Debug)]
pub struct PersistedOutcome {
    receipt: EffectReceipt,
    digest: ReceiptDigest,
}

impl PersistedOutcome {
    /// Reconstruct a persisted outcome after accepted-ledger verification.
    ///
    /// # Errors
    ///
    /// Returns [`EffectReceiptError::PersistedReceiptIsNotOutcome`] unless the
    /// verified receipt is an outcome.
    pub fn from_verified(
        receipt: EffectReceipt,
        digest: ReceiptDigest,
    ) -> Result<Self, EffectReceiptError> {
        if !matches!(receipt.event(), ReceiptEvent::Outcome(_)) {
            return Err(EffectReceiptError::PersistedReceiptIsNotOutcome {
                location: snafu::location!(),
            });
        }
        Ok(Self { receipt, digest })
    }

    /// Return the recorded outcome.
    ///
    /// # Errors
    ///
    /// Returns [`EffectReceiptError::PersistedReceiptIsNotOutcome`] if a
    /// ledger adapter violated the persisted-outcome constructor contract.
    pub fn outcome(&self) -> Result<EffectOutcome, EffectReceiptError> {
        match self.receipt.event() {
            ReceiptEvent::Outcome(outcome) => Ok(outcome),
            ReceiptEvent::Intent => Err(EffectReceiptError::PersistedReceiptIsNotOutcome {
                location: snafu::location!(),
            }),
        }
    }

    /// Return the canonical durable outcome digest.
    #[must_use]
    pub const fn digest(&self) -> ReceiptDigest {
        self.digest
    }

    /// Borrow the persisted outcome receipt.
    #[must_use]
    pub const fn receipt(&self) -> &EffectReceipt {
        &self.receipt
    }

    /// Consume an unresolved outcome into one continuation authority.
    ///
    /// # Errors
    ///
    /// Partial and recovery-required outcomes authorize recovery of their
    /// referenced operation. A failed, cancelled, backpressured, or
    /// authorization-denied recovery attempt authorizes another attempt for
    /// the original operation. Successful and recovered outcomes are final.
    ///
    /// Returns [`EffectReceiptError::NotRecoveryRequirement`] for an outcome
    /// that does not authorize recovery.
    pub fn into_recovery_authorization(self) -> Result<RecoveryAuthorization, EffectReceiptError> {
        match (self.receipt.event(), self.receipt.recovery()) {
            (
                ReceiptEvent::Outcome(EffectOutcome::Partial | EffectOutcome::RecoveryRequired),
                RecoveryRelation::RequiredFor(original_effect),
            )
            | (
                ReceiptEvent::Outcome(
                    EffectOutcome::Failed
                    | EffectOutcome::Cancelled
                    | EffectOutcome::Backpressured
                    | EffectOutcome::AuthorizationDenied,
                ),
                RecoveryRelation::RecoveryOf(original_effect),
            ) => Ok(RecoveryAuthorization {
                original_effect,
                requirement_digest: self.digest,
                requirement_observed_at: self.receipt.observed_at(),
            }),
            _ => Err(EffectReceiptError::NotRecoveryRequirement {
                location: snafu::location!(),
            }),
        }
    }
}

/// Linear authority to describe one explicit recovery operation.
///
/// The authority comes only from a persisted unresolved outcome, binding a
/// recovery or continuation attempt to that outcome's ledger digest and time.
#[derive(Debug, PartialEq, Eq)]
pub struct RecoveryAuthorization {
    original_effect: EffectRef,
    requirement_digest: ReceiptDigest,
    requirement_observed_at: Timestamp,
}

impl RecoveryAuthorization {
    pub(crate) const fn original_effect(&self) -> EffectRef {
        self.original_effect
    }

    /// Consume this authority into one recovery effect descriptor.
    #[must_use]
    pub const fn into_descriptor(
        self,
        effect: EffectRef,
        schema_epoch: SchemaEpoch,
        evidence_digest: Option<EvidenceDigest>,
    ) -> EffectDescriptor {
        EffectDescriptor::from_recovery(effect, schema_epoch, evidence_digest, self)
    }

    pub(crate) const fn into_parts(self) -> (EffectRef, ReceiptDigest, Timestamp) {
        (
            self.original_effect,
            self.requirement_digest,
            self.requirement_observed_at,
        )
    }
}

/// Restart-safe ticket for an intent with no accepted terminal outcome.
///
/// Recovery is explicit and never replays the original effect. The ticket is
/// linear and reconstructed after restart only through
/// [`PersistedIntent::from_verified_unmatched`].
#[derive(Debug)]
pub struct RecoveryTicket {
    intent: PersistedIntent,
}

impl RecoveryTicket {
    /// Return the original protected effect reference.
    #[must_use]
    pub const fn effect(&self) -> EffectRef {
        self.intent.receipt.context().effect()
    }

    /// Consume this ticket into a recovery-required outcome.
    ///
    /// # Errors
    ///
    /// Returns [`ReceiptTransitionError`] if the supplied time precedes the
    /// original intent.
    pub fn recovery_required(
        self,
        observed_at: Timestamp,
    ) -> Result<PendingOutcome, ReceiptTransitionError> {
        self.intent
            .outcome(observed_at, EffectOutcome::RecoveryRequired, None)
    }
}

/// Failed persisted-intent transition with its reconciliation candidate.
#[derive(Debug, Snafu)]
#[snafu(display("persisted intent transition failed: {source}"))]
#[non_exhaustive]
pub struct ReceiptTransitionError {
    /// Receipt state-machine failure.
    source: EffectReceiptError,
    /// Original unmatched intent.
    recovery: Box<RecoveryTicket>,
    /// Source location of the failed transition.
    #[snafu(implicit)]
    location: snafu::Location,
}

impl ReceiptTransitionError {
    /// Split the error into the contract failure and reconciliation ticket.
    #[must_use]
    pub fn into_parts(self) -> (EffectReceiptError, Box<RecoveryTicket>) {
        (self.source, self.recovery)
    }
}

/// Failed durable outcome append with its reconciliation candidate.
#[derive(Debug, Snafu)]
#[snafu(display("effect outcome append failed: {source}"))]
#[non_exhaustive]
pub struct OutcomeAppendError<E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    /// Accepted ledger failure.
    source: E,
    /// Original unmatched intent.
    recovery: Box<RecoveryTicket>,
    /// Source location of the failed append.
    #[snafu(implicit)]
    location: snafu::Location,
}

impl<E> OutcomeAppendError<E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    /// Split the append failure from the reconciliation candidate.
    #[must_use]
    pub fn into_parts(self) -> (E, Box<RecoveryTicket>) {
        (self.source, self.recovery)
    }
}
