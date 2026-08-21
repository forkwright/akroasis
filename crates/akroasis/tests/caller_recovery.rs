//! Stateful recovery fixtures for minimized effect receipts.

#![expect(
    clippy::expect_used,
    reason = "fixed recovery fixtures use checked constructors and ledger assertions"
)]

use std::cell::Cell;
use std::collections::BTreeSet;

use akroasis_lib::caller::{
    AdmissionDecision, ApplicationCallerResolver, EffectExecution, EffectGateError, EffectRequest,
    TrustedClock, execute_effect,
};
use snafu::Snafu;
use stoicheion::Timestamp;
use tekmerion::{
    AuthorityClaims, AuthorityDecision, AuthorityGrant, AuthorizationRequirement,
    CALLER_CONTEXT_VERSION, CallerAuthority, CallerRef, CapabilityRef, EFFECT_RECEIPT_VERSION,
    EffectDescriptor, EffectOutcome, EffectReceipt, EffectReceiptError, EffectReceiptSink,
    EffectRef, LocalPeerCredentials, PersistedIntent, PersistedOutcome, PolicyEpoch, ReceiptDigest,
    ReceiptEvent, RecoveryAuthorization, RecoveryRelation, SchemaEpoch, ScopeRef,
};
use tokio::net::UnixStream;

#[derive(Debug, Clone, Copy)]
struct FixedClock(Timestamp);

impl TrustedClock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

#[derive(Debug)]
struct RecoveryAuthority {
    grant: AuthorityGrant,
}

impl CallerAuthority for RecoveryAuthority {
    fn resolve_local(
        &self,
        _peer: LocalPeerCredentials,
        _observed_at: Timestamp,
    ) -> AuthorityDecision {
        AuthorityDecision::Granted(
            AuthorityClaims::builder()
                .schema_version(CALLER_CONTEXT_VERSION)
                .caller_ref(CallerRef::from_canonical(b"recovery-operator"))
                .grant(self.grant)
                .policy_epoch(policy_epoch(7))
                .validity(timestamp(1_000), timestamp(2_000), timestamp(3_000))
                .build()
                .expect("valid recovery authority"),
        )
    }

    fn resolve_service(
        &self,
        _identity_ref: CallerRef,
        _authenticated_at: Timestamp,
        _evidence_expires_at: Timestamp,
        _observed_at: Timestamp,
    ) -> AuthorityDecision {
        AuthorityDecision::UnknownIdentity
    }
}

#[derive(Debug, Snafu)]
enum LedgerError {
    #[snafu(display("injected durable append failure"))]
    Injected {
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("ledger predecessor is absent or has the wrong state"))]
    InvalidPredecessor {
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("ledger transition was already matched"))]
    AlreadyMatched {
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("ledger reconciliation claim was already consumed"))]
    AlreadyClaimed {
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("ledger already contains the same receipt identity"))]
    DuplicateReceipt {
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

#[derive(Default)]
struct StatefulLedger {
    attempts: usize,
    fail_at: Option<usize>,
    receipts: Vec<EffectReceipt>,
    claimed_intents: BTreeSet<ReceiptDigest>,
    claimed_recoveries: BTreeSet<ReceiptDigest>,
}

impl StatefulLedger {
    fn failing_at(attempt: usize) -> Self {
        Self {
            fail_at: Some(attempt),
            ..Self::default()
        }
    }

    const fn clear_failure(&mut self) {
        self.fail_at = None;
    }

    fn receipt(&self, index: usize) -> &EffectReceipt {
        self.receipts.get(index).expect("ledger receipt fixture")
    }

    fn receipt_with_digest(&self, digest: ReceiptDigest) -> Option<EffectReceipt> {
        self.receipts
            .iter()
            .find(|receipt| receipt_digest(receipt) == digest)
            .cloned()
    }

    fn has_descendant(&self, digest: ReceiptDigest) -> bool {
        self.receipts
            .iter()
            .any(|receipt| receipt.predecessor() == Some(digest))
    }

    fn claim_unmatched_intent(
        &mut self,
        digest: ReceiptDigest,
    ) -> Result<PersistedIntent, LedgerError> {
        let receipt = self
            .receipt_with_digest(digest)
            .filter(|receipt| receipt.event() == ReceiptEvent::Intent)
            .ok_or(LedgerError::InvalidPredecessor {
                location: snafu::location!(),
            })?;
        if self.has_descendant(digest) {
            return Err(LedgerError::AlreadyMatched {
                location: snafu::location!(),
            });
        }
        if !self.claimed_intents.insert(digest) {
            return Err(LedgerError::AlreadyClaimed {
                location: snafu::location!(),
            });
        }
        PersistedIntent::from_verified_unmatched(receipt, digest).map_err(|_| {
            LedgerError::InvalidPredecessor {
                location: snafu::location!(),
            }
        })
    }

    fn claim_recovery(
        &mut self,
        digest: ReceiptDigest,
    ) -> Result<RecoveryAuthorization, LedgerError> {
        if self.has_descendant(digest) {
            return Err(LedgerError::AlreadyMatched {
                location: snafu::location!(),
            });
        }
        let receipt = self
            .receipt_with_digest(digest)
            .ok_or(LedgerError::InvalidPredecessor {
                location: snafu::location!(),
            })?;
        let authorization = PersistedOutcome::from_verified(receipt, digest)
            .and_then(PersistedOutcome::into_recovery_authorization)
            .map_err(|_| LedgerError::InvalidPredecessor {
                location: snafu::location!(),
            })?;
        if !self.claimed_recoveries.insert(digest) {
            return Err(LedgerError::AlreadyClaimed {
                location: snafu::location!(),
            });
        }
        Ok(authorization)
    }

    fn validate_append(&self, receipt: &EffectReceipt) -> Result<(), LedgerError> {
        if self.receipt_with_digest(receipt_digest(receipt)).is_some() {
            return Err(LedgerError::DuplicateReceipt {
                location: snafu::location!(),
            });
        }
        let Some(predecessor) = receipt.predecessor() else {
            return Ok(());
        };
        let parent =
            self.receipt_with_digest(predecessor)
                .ok_or(LedgerError::InvalidPredecessor {
                    location: snafu::location!(),
                })?;
        let _link = receipt
            .validate_child_of(&parent, predecessor)
            .map_err(|_| LedgerError::InvalidPredecessor {
                location: snafu::location!(),
            })?;
        if self.has_descendant(predecessor) {
            return Err(LedgerError::AlreadyMatched {
                location: snafu::location!(),
            });
        }
        Ok(())
    }
}

impl EffectReceiptSink for StatefulLedger {
    type Error = LedgerError;

    fn append(&mut self, receipt: &EffectReceipt) -> Result<ReceiptDigest, Self::Error> {
        self.validate_append(receipt)?;
        let attempt = self.attempts;
        self.attempts += 1;
        if self.fail_at == Some(attempt) {
            return Err(LedgerError::Injected {
                location: snafu::location!(),
            });
        }
        let digest = receipt_digest(receipt);
        self.receipts.push(receipt.clone());
        Ok(digest)
    }
}

fn timestamp(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis).expect("valid timestamp fixture")
}

fn receipt_digest(receipt: &EffectReceipt) -> ReceiptDigest {
    ReceiptDigest::from_canonical(&serde_json::to_vec(receipt).expect("serialize ledger receipt"))
}

fn policy_epoch(value: u64) -> PolicyEpoch {
    PolicyEpoch::try_from_u64(value).expect("non-zero policy epoch")
}

fn schema_epoch(value: u64) -> SchemaEpoch {
    SchemaEpoch::try_from_u64(value).expect("non-zero schema epoch")
}

fn grant() -> AuthorityGrant {
    AuthorityGrant::new(
        CapabilityRef::from_canonical(b"test.recovery"),
        ScopeRef::from_canonical(b"test.scope"),
    )
}

fn caller() -> tekmerion::ValidatedCaller {
    let resolver = ApplicationCallerResolver::current(RecoveryAuthority { grant: grant() })
        .expect("current caller resolver");
    let (client, _server) = UnixStream::pair().expect("Unix socket pair");
    resolver
        .resolve_local(&client, &FixedClock(timestamp(1_500)))
        .expect("valid recovery caller")
}

fn request(descriptor: EffectDescriptor) -> EffectRequest {
    EffectRequest::new(
        AuthorizationRequirement::new(grant(), policy_epoch(7), None),
        descriptor,
    )
}

fn wire_receipt(
    context: &serde_json::Value,
    event: ReceiptEvent,
    observed_at: Timestamp,
    predecessor: ReceiptDigest,
    recovery: RecoveryRelation,
) -> EffectReceipt {
    serde_json::from_value(serde_json::json!({
        "schema_version": EFFECT_RECEIPT_VERSION,
        "context": context,
        "event": event,
        "observed_at": observed_at,
        "evidence_digest": null,
        "predecessor": predecessor,
        "recovery": recovery,
    }))
    .expect("forged child is individually wire-valid")
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_intent_is_rejected_before_a_second_effect() {
    let caller = caller();
    let effect = EffectRef::from_canonical(b"idempotent-effect");
    let calls = Cell::new(0);
    let mut ledger = StatefulLedger::default();
    let invoke = |ledger: &mut StatefulLedger| {
        execute_effect(
            Some(&caller),
            request(EffectDescriptor::new(effect, schema_epoch(3), None)),
            &FixedClock(timestamp(1_500)),
            AdmissionDecision::Ready,
            ledger,
            || {
                calls.set(calls.get() + 1);
                EffectExecution::succeeded(None)
            },
        )
    };
    invoke(&mut ledger).expect("first intent and outcome are durable");
    let duplicate = invoke(&mut ledger);
    assert!(matches!(
        duplicate,
        Err(EffectGateError::IntentAudit {
            source: LedgerError::DuplicateReceipt { .. },
            ..
        })
    ));
    assert_eq!(calls.get(), 1, "duplicate intent performs no second effect");
    assert_eq!(ledger.receipts.len(), 2, "only the first pair is durable");
}

#[tokio::test(flavor = "current_thread")]
async fn forged_child_contexts_and_recovery_targets_are_rejected() {
    let caller = caller();
    let original = EffectRef::from_canonical(b"normal-effect");
    let unrelated = EffectRef::from_canonical(b"unrelated-effect");
    let mut ledger = StatefulLedger::failing_at(1);
    let failed_outcome = execute_effect(
        Some(&caller),
        request(EffectDescriptor::new(original, schema_epoch(3), None)),
        &FixedClock(timestamp(1_500)),
        AdmissionDecision::Ready,
        &mut ledger,
        || EffectExecution::failed(None),
    );
    assert!(matches!(
        failed_outcome,
        Err(EffectGateError::OutcomeAudit { .. })
    ));
    ledger.clear_failure();
    let intent = ledger.receipt(0);
    let intent_digest = receipt_digest(intent);
    let context = serde_json::to_value(intent.context()).expect("serialize receipt context");
    let forged_target = wire_receipt(
        &context,
        ReceiptEvent::Outcome(EffectOutcome::Failed),
        timestamp(1_600),
        intent_digest,
        RecoveryRelation::RecoveryOf(unrelated),
    );
    assert!(matches!(
        ledger.append(&forged_target),
        Err(LedgerError::InvalidPredecessor { .. })
    ));

    let mut wrong_context = context;
    wrong_context
        .as_object_mut()
        .expect("receipt context object")
        .insert(
            "effect".to_owned(),
            serde_json::to_value(unrelated).expect("serialize unrelated effect"),
        );
    let forged_context = wire_receipt(
        &wrong_context,
        ReceiptEvent::Outcome(EffectOutcome::Failed),
        timestamp(1_600),
        intent_digest,
        RecoveryRelation::None,
    );
    assert!(matches!(
        ledger.append(&forged_context),
        Err(LedgerError::InvalidPredecessor { .. })
    ));

    let mut partial_ledger = StatefulLedger::default();
    execute_effect(
        Some(&caller),
        request(EffectDescriptor::new(original, schema_epoch(3), None)),
        &FixedClock(timestamp(1_500)),
        AdmissionDecision::Ready,
        &mut partial_ledger,
        || EffectExecution::partial(None),
    )
    .expect("partial chain fixture");
    let partial = partial_ledger.receipt(1);
    let partial_context =
        serde_json::to_value(partial.context()).expect("serialize partial context");
    let forged_recovery = wire_receipt(
        &partial_context,
        ReceiptEvent::Intent,
        timestamp(1_700),
        receipt_digest(partial),
        RecoveryRelation::RecoveryOf(unrelated),
    );
    assert!(matches!(
        partial_ledger.append(&forged_recovery),
        Err(LedgerError::InvalidPredecessor { .. })
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn partial_recovery_rejects_a_backdated_child() {
    let caller = caller();
    let original = EffectRef::from_canonical(b"partial-time-order");
    let mut ledger = StatefulLedger::default();
    let partial = execute_effect(
        Some(&caller),
        request(EffectDescriptor::new(original, schema_epoch(3), None)),
        &FixedClock(timestamp(1_500)),
        AdmissionDecision::Ready,
        &mut ledger,
        || EffectExecution::partial(None),
    )
    .expect("partial outcome is durably recorded");
    let descriptor = partial
        .into_recovery()
        .expect("partial outcome authorizes recovery")
        .into_descriptor(
            EffectRef::from_canonical(b"backdated-recovery"),
            schema_epoch(3),
            None,
        );
    let calls = Cell::new(0);
    let result = execute_effect(
        Some(&caller),
        request(descriptor),
        &FixedClock(timestamp(1_499)),
        AdmissionDecision::Ready,
        &mut ledger,
        || {
            calls.set(calls.get() + 1);
            EffectExecution::succeeded(None)
        },
    );
    assert!(matches!(
        result,
        Err(EffectGateError::ReceiptContract {
            source: EffectReceiptError::RecoveryIntentPrecedesRequirement { .. },
            ..
        })
    ));
    assert_eq!(calls.get(), 0, "backdated recovery performs no domain I/O");
    assert_eq!(ledger.receipts.len(), 2, "no recovery receipt was appended");
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_recovery_can_retry_once_on_the_same_ledger() {
    let caller = caller();
    let original = EffectRef::from_canonical(b"partial-retry");
    let mut ledger = StatefulLedger::default();
    let partial = execute_effect(
        Some(&caller),
        request(EffectDescriptor::new(original, schema_epoch(3), None)),
        &FixedClock(timestamp(1_500)),
        AdmissionDecision::Ready,
        &mut ledger,
        || EffectExecution::partial(None),
    )
    .expect("partial outcome is durably recorded");
    let first_recovery = partial
        .into_recovery()
        .expect("partial outcome authorizes recovery")
        .into_descriptor(
            EffectRef::from_canonical(b"cancelled-recovery"),
            schema_epoch(3),
            None,
        );
    let cancelled = execute_effect(
        Some(&caller),
        request(first_recovery),
        &FixedClock(timestamp(1_700)),
        AdmissionDecision::Cancelled,
        &mut ledger,
        || EffectExecution::succeeded(None),
    )
    .expect("cancelled recovery records a no-I/O outcome");
    let cancelled_digest = cancelled.receipt_digest();
    let retry = cancelled
        .into_recovery()
        .expect("cancelled recovery preserves continuation authority")
        .into_descriptor(
            EffectRef::from_canonical(b"retry-recovery"),
            schema_epoch(3),
            None,
        );
    let recovered = execute_effect(
        Some(&caller),
        request(retry),
        &FixedClock(timestamp(1_800)),
        AdmissionDecision::Ready,
        &mut ledger,
        || EffectExecution::succeeded(None),
    )
    .expect("retry completes the explicit recovery chain");
    assert_eq!(recovered.outcome(), EffectOutcome::Recovered);
    assert_eq!(
        ledger.receipt(4).predecessor(),
        Some(cancelled_digest),
        "retry intent links the cancelled recovery outcome"
    );
    assert!(
        ledger.claim_recovery(cancelled_digest).is_err(),
        "a recovery requirement with a descendant cannot be replayed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn restart_claim_is_atomic_and_recovery_chain_is_continuous() {
    let caller = caller();
    let original = EffectRef::from_canonical(b"restart-effect");
    let mut ledger = StatefulLedger::failing_at(1);
    let result = execute_effect(
        Some(&caller),
        request(EffectDescriptor::new(original, schema_epoch(3), None)),
        &FixedClock(timestamp(1_500)),
        AdmissionDecision::Ready,
        &mut ledger,
        || EffectExecution::succeeded(None),
    );
    let ticket = match result {
        Err(EffectGateError::OutcomeAudit { recovery, .. }) => Some(recovery),
        _ => None,
    }
    .expect("outcome failure returns a restart ticket");
    assert_eq!(ticket.effect(), original);
    let intent_digest = receipt_digest(ledger.receipt(0));
    ledger.clear_failure();
    let restored = ledger
        .claim_unmatched_intent(intent_digest)
        .expect("ledger atomically claims an unmatched intent");
    assert!(
        ledger.claim_unmatched_intent(intent_digest).is_err(),
        "the same unmatched intent cannot be claimed twice"
    );
    let requirement_digest = restored
        .into_recovery_ticket()
        .recovery_required(timestamp(1_600))
        .expect("reconciliation records recovery required")
        .append_to(&mut ledger)
        .expect("recovery requirement is durable")
        .digest();
    let authorization = ledger
        .claim_recovery(requirement_digest)
        .expect("ledger atomically claims the recovery requirement");
    assert!(
        ledger.claim_recovery(requirement_digest).is_err(),
        "the same recovery requirement cannot be claimed twice"
    );
    let descriptor = authorization.into_descriptor(
        EffectRef::from_canonical(b"restart-recovery"),
        schema_epoch(3),
        None,
    );
    let recovered = execute_effect(
        Some(&caller),
        request(descriptor),
        &FixedClock(timestamp(1_800)),
        AdmissionDecision::Ready,
        &mut ledger,
        || EffectExecution::succeeded(None),
    )
    .expect("restart recovery completes on the same ledger");
    assert_eq!(recovered.outcome(), EffectOutcome::Recovered);
    assert_eq!(ledger.receipt(2).predecessor(), Some(requirement_digest));
    assert!(
        ledger.claim_unmatched_intent(intent_digest).is_err(),
        "a matched intent cannot be reclaimed after recovery"
    );
}
