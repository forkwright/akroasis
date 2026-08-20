//! End-to-end fixtures for the shared caller and effect-receipt contracts.

#![expect(
    clippy::expect_used,
    reason = "fixed test fixtures use checked constructors and serialization"
)]

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use akroasis_lib::caller::{
    AdmissionDecision, ApplicationCallerError, ApplicationCallerResolver, EffectExecution,
    EffectGateError, EffectRequest, TrustedClock, execute_effect,
};
use koinon::{
    AuthorityClaims, AuthorityDecision, AuthorityGrant, AuthorizationDenial,
    AuthorizationRequirement, CALLER_CONTEXT_VERSION, CALLER_RESOLVER_VERSION, CallerAuthority,
    CallerContractError, CallerRef, CapabilityRef, EffectDescriptor, EffectOutcome, EffectReceipt,
    EffectReceiptError, EffectReceiptSink, EffectRef, LocalPeerCredentials, PersonaRef,
    PolicyEpoch, ReceiptDigest, ReceiptEvent, SchemaEpoch, ScopeRef, Timestamp,
};
use snafu::Snafu;
use tokio::net::UnixStream;

#[derive(Debug, Clone, Copy)]
struct FixedClock(Timestamp);

impl TrustedClock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

#[derive(Debug)]
struct SequenceClock(RefCell<VecDeque<Timestamp>>);

impl SequenceClock {
    fn new(values: impl IntoIterator<Item = Timestamp>) -> Self {
        Self(RefCell::new(values.into_iter().collect()))
    }
}

impl TrustedClock for SequenceClock {
    fn now(&self) -> Timestamp {
        self.0
            .borrow_mut()
            .pop_front()
            .expect("clock fixture has one value per observation")
    }
}

#[derive(Debug, Clone, Copy)]
enum AuthorityMode {
    Granted,
    UnknownIdentity,
    Untrusted,
    Revoked,
    Unavailable,
    UnknownContextVersion,
}

fn assert_resolution_error(mode: AuthorityMode, error: &ApplicationCallerError) {
    assert!(
        matches!(
            (mode, error),
            (
                AuthorityMode::UnknownIdentity,
                ApplicationCallerError::CallerResolution {
                    source: CallerContractError::UnknownIdentity { .. },
                    ..
                }
            ) | (
                AuthorityMode::Untrusted,
                ApplicationCallerError::CallerResolution {
                    source: CallerContractError::UntrustedIdentity { .. },
                    ..
                }
            ) | (
                AuthorityMode::Revoked,
                ApplicationCallerError::CallerResolution {
                    source: CallerContractError::RevokedIdentity { .. },
                    ..
                }
            ) | (
                AuthorityMode::Unavailable,
                ApplicationCallerError::CallerResolution {
                    source: CallerContractError::AuthorityUnavailable { .. },
                    ..
                }
            ) | (
                AuthorityMode::UnknownContextVersion,
                ApplicationCallerError::CallerResolution {
                    source: CallerContractError::UnknownCallerVersion { .. },
                    ..
                }
            )
        ),
        "{mode:?} must retain its exact typed resolver denial"
    );
}

#[derive(Debug)]
struct FakeAuthority {
    mode: AuthorityMode,
    calls: Rc<Cell<usize>>,
    caller_ref: CallerRef,
    grant: AuthorityGrant,
    policy_epoch: PolicyEpoch,
    expected_peer: Option<LocalPeerCredentials>,
    seen_peer: Rc<RefCell<Option<LocalPeerCredentials>>>,
}

impl FakeAuthority {
    fn granted(grant: AuthorityGrant, policy_epoch: PolicyEpoch) -> Self {
        Self {
            mode: AuthorityMode::Granted,
            calls: Rc::new(Cell::new(0)),
            caller_ref: CallerRef::from_canonical(b"operator-a"),
            grant,
            policy_epoch,
            expected_peer: None,
            seen_peer: Rc::new(RefCell::new(None)),
        }
    }

    fn decision(&self, authenticated_at: Timestamp) -> AuthorityDecision {
        self.calls.set(self.calls.get() + 1);
        match self.mode {
            AuthorityMode::Granted | AuthorityMode::UnknownContextVersion => {
                let schema_version = if matches!(self.mode, AuthorityMode::Granted) {
                    CALLER_CONTEXT_VERSION
                } else {
                    CALLER_CONTEXT_VERSION + 1
                };
                let claims = AuthorityClaims::builder()
                    .schema_version(schema_version)
                    .caller_ref(self.caller_ref)
                    .grant(self.grant)
                    .policy_epoch(self.policy_epoch)
                    .validity(authenticated_at, timestamp(2_000), timestamp(3_000))
                    .build()
                    .expect("valid fake authority claims");
                AuthorityDecision::Granted(claims)
            }
            AuthorityMode::UnknownIdentity => AuthorityDecision::UnknownIdentity,
            AuthorityMode::Untrusted => AuthorityDecision::UntrustedIdentity,
            AuthorityMode::Revoked => AuthorityDecision::Revoked,
            AuthorityMode::Unavailable => AuthorityDecision::Unavailable,
        }
    }
}

impl CallerAuthority for FakeAuthority {
    fn resolve_local(
        &self,
        peer: LocalPeerCredentials,
        _observed_at: Timestamp,
    ) -> AuthorityDecision {
        self.seen_peer.replace(Some(peer));
        if self.expected_peer.is_some_and(|expected| expected != peer) {
            return AuthorityDecision::UnknownIdentity;
        }
        self.decision(timestamp(1_000))
    }

    fn resolve_service(
        &self,
        _identity_ref: CallerRef,
        authenticated_at: Timestamp,
        _evidence_expires_at: Timestamp,
        _observed_at: Timestamp,
    ) -> AuthorityDecision {
        self.decision(authenticated_at)
    }
}

#[derive(Debug, Snafu)]
enum FakeSinkError {
    #[snafu(display("injected receipt append failure"))]
    Injected {
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("receipt predecessor is not present in the fake ledger"))]
    UnknownPredecessor {
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("receipt transition violates the shared contract: {source}"))]
    InvalidTransition {
        source: EffectReceiptError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

#[derive(Default)]
struct FakeSink {
    attempts: usize,
    fail_at: Option<usize>,
    receipts: Vec<EffectReceipt>,
    digests: Vec<ReceiptDigest>,
    transcript: Option<Rc<RefCell<Vec<&'static str>>>>,
}

impl FakeSink {
    fn failing_at(attempt: usize) -> Self {
        Self {
            fail_at: Some(attempt),
            ..Self::default()
        }
    }

    fn with_transcript(transcript: Rc<RefCell<Vec<&'static str>>>) -> Self {
        Self {
            transcript: Some(transcript),
            ..Self::default()
        }
    }

    fn validate_child(&self, receipt: &EffectReceipt) -> Result<(), FakeSinkError> {
        let Some(predecessor) = receipt.predecessor() else {
            return Ok(());
        };
        let parent = self
            .digests
            .iter()
            .zip(&self.receipts)
            .find_map(|(digest, receipt)| (*digest == predecessor).then_some(receipt))
            .ok_or(FakeSinkError::UnknownPredecessor {
                location: snafu::location!(),
            })?;
        let _link = receipt
            .validate_child_of(parent, predecessor)
            .map_err(|source| FakeSinkError::InvalidTransition {
                source,
                location: snafu::location!(),
            })?;
        Ok(())
    }
}

impl EffectReceiptSink for FakeSink {
    type Error = FakeSinkError;

    fn append(&mut self, receipt: &EffectReceipt) -> Result<ReceiptDigest, Self::Error> {
        self.validate_child(receipt)?;
        let attempt = self.attempts;
        self.attempts += 1;
        if self.fail_at == Some(attempt) {
            return Err(FakeSinkError::Injected {
                location: snafu::location!(),
            });
        }
        if let Some(transcript) = &self.transcript {
            transcript.borrow_mut().push(match receipt.event() {
                ReceiptEvent::Intent => "audit:intent",
                ReceiptEvent::Outcome(_) => "audit:outcome",
            });
        }
        let digest = ledger_digest(receipt, self.receipts.len());
        self.receipts.push(receipt.clone());
        self.digests.push(digest);
        Ok(digest)
    }
}

fn timestamp(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis).expect("valid fixed timestamp")
}

fn ledger_digest(receipt: &EffectReceipt, position: usize) -> ReceiptDigest {
    let mut bytes = b"caller-contract-test-ledger\0".to_vec();
    bytes.extend_from_slice(
        &u64::try_from(position)
            .expect("test ledger position fits u64")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&serde_json::to_vec(receipt).expect("serialize fake receipt"));
    ReceiptDigest::from_canonical(&bytes)
}

fn policy_epoch(value: u64) -> PolicyEpoch {
    PolicyEpoch::try_from_u64(value).expect("non-zero policy epoch")
}

fn schema_epoch(value: u64) -> SchemaEpoch {
    SchemaEpoch::try_from_u64(value).expect("non-zero schema epoch")
}

fn standard_grant() -> AuthorityGrant {
    AuthorityGrant::new(
        CapabilityRef::from_canonical(b"test.effect"),
        ScopeRef::from_canonical(b"test.scope"),
    )
}

fn valid_caller() -> koinon::ValidatedCaller {
    let authority = FakeAuthority::granted(standard_grant(), policy_epoch(7));
    let resolver = ApplicationCallerResolver::current(authority).expect("current resolver");
    let (client, _server) = UnixStream::pair().expect("Unix socket pair");
    resolver
        .resolve_local(&client, &FixedClock(timestamp(1_500)))
        .expect("valid local caller")
}

fn request(requirement: AuthorizationRequirement, effect: &[u8]) -> EffectRequest {
    EffectRequest::new(
        requirement,
        EffectDescriptor::new(EffectRef::from_canonical(effect), schema_epoch(3), None),
    )
}

fn kernel_peer(stream: &UnixStream) -> LocalPeerCredentials {
    let credentials = stream.peer_cred().expect("Unix peer credentials");
    let pid = credentials
        .pid()
        .map(u32::try_from)
        .transpose()
        .expect("peer PID fits the shared contract");
    LocalPeerCredentials::from_os_peer(credentials.uid(), credentials.gid(), pid)
}

#[tokio::test(flavor = "current_thread")]
async fn local_peer_credentials_are_forwarded_and_acl_checked() {
    let (client, _server) = UnixStream::pair().expect("Unix socket pair");
    let expected = kernel_peer(&client);
    let mut authority = FakeAuthority::granted(standard_grant(), policy_epoch(7));
    authority.expected_peer = Some(expected);
    let seen = Rc::clone(&authority.seen_peer);
    let resolver = ApplicationCallerResolver::current(authority).expect("current resolver");
    resolver
        .resolve_local(&client, &FixedClock(timestamp(1_500)))
        .expect("matching kernel peer should resolve");
    assert_eq!(
        *seen.borrow(),
        Some(expected),
        "the authority must receive exact kernel UID/GID/PID evidence"
    );

    let mut rejected = FakeAuthority::granted(standard_grant(), policy_epoch(7));
    rejected.expected_peer = Some(LocalPeerCredentials::from_os_peer(
        expected.uid() ^ 1,
        expected.gid(),
        expected.pid(),
    ));
    let resolver = ApplicationCallerResolver::current(rejected).expect("current resolver");
    assert!(
        resolver
            .resolve_local(&client, &FixedClock(timestamp(1_500)))
            .is_err(),
        "an ACL mismatch must fail before any effect can be requested"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn resolver_failures_never_reach_a_fake_effect() {
    let modes = [
        AuthorityMode::UnknownIdentity,
        AuthorityMode::Untrusted,
        AuthorityMode::Revoked,
        AuthorityMode::Unavailable,
        AuthorityMode::UnknownContextVersion,
    ];
    let effect_calls = Cell::new(0);
    let requirement = AuthorizationRequirement::new(standard_grant(), policy_epoch(7), None);
    for mode in modes {
        let mut authority = FakeAuthority::granted(standard_grant(), policy_epoch(7));
        authority.mode = mode;
        let calls = Rc::clone(&authority.calls);
        let resolver = ApplicationCallerResolver::current(authority).expect("current resolver");
        let (client, _server) = UnixStream::pair().expect("Unix socket pair");
        let caller = resolver.resolve_local(&client, &FixedClock(timestamp(1_500)));
        if let Ok(caller) = &caller {
            let mut sink = FakeSink::default();
            let _ = execute_effect(
                Some(caller),
                request(requirement, b"resolver-failure"),
                &FixedClock(timestamp(1_500)),
                AdmissionDecision::Ready,
                &mut sink,
                || {
                    effect_calls.set(effect_calls.get() + 1);
                    EffectExecution::succeeded(None)
                },
            );
        }
        assert!(caller.is_err(), "caller resolution unexpectedly succeeded");
        if let Err(error) = caller.as_ref() {
            assert_resolution_error(mode, error);
        }
        assert_eq!(calls.get(), 1, "authority should be consulted exactly once");
        assert_eq!(
            effect_calls.get(),
            0,
            "resolution failure must not invoke a domain effect"
        );
    }

    let authority = FakeAuthority::granted(standard_grant(), policy_epoch(7));
    let calls = Rc::clone(&authority.calls);
    let result = ApplicationCallerResolver::try_new(CALLER_RESOLVER_VERSION + 1, authority);
    assert!(matches!(
        result,
        Err(ApplicationCallerError::CallerResolution {
            source: CallerContractError::UnknownResolverVersion {
                resolver_version,
                ..
            },
            ..
        }) if resolver_version == CALLER_RESOLVER_VERSION + 1
    ));
    assert_eq!(
        calls.get(),
        0,
        "unknown resolver versions must fail before authority lookup"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn every_effect_time_denial_prevents_intent_and_effect() {
    let caller = valid_caller();
    let grant = standard_grant();
    let current_epoch = policy_epoch(7);
    let valid = AuthorizationRequirement::new(grant, current_epoch, None);
    let cases = [
        (
            None,
            valid,
            timestamp(1_500),
            AuthorizationDenial::MissingContext,
        ),
        (
            Some(&caller),
            valid,
            timestamp(999),
            AuthorizationDenial::NotYetValid,
        ),
        (
            Some(&caller),
            valid,
            timestamp(2_000),
            AuthorizationDenial::Stale,
        ),
        (
            Some(&caller),
            valid,
            timestamp(3_000),
            AuthorizationDenial::Expired,
        ),
        (
            Some(&caller),
            AuthorizationRequirement::new(grant, policy_epoch(8), None),
            timestamp(1_500),
            AuthorizationDenial::PolicyEpochMismatch,
        ),
        (
            Some(&caller),
            AuthorizationRequirement::new(
                grant,
                current_epoch,
                Some(PersonaRef::from_canonical(b"wrong-persona")),
            ),
            timestamp(1_500),
            AuthorizationDenial::WrongPersona,
        ),
        (
            Some(&caller),
            AuthorizationRequirement::new(
                AuthorityGrant::new(
                    CapabilityRef::from_canonical(b"other.effect"),
                    grant.scope(),
                ),
                current_epoch,
                None,
            ),
            timestamp(1_500),
            AuthorizationDenial::InsufficientCapability,
        ),
        (
            Some(&caller),
            AuthorizationRequirement::new(
                AuthorityGrant::new(grant.capability(), ScopeRef::from_canonical(b"other.scope")),
                current_epoch,
                None,
            ),
            timestamp(1_500),
            AuthorizationDenial::InsufficientScope,
        ),
    ];

    for (caller, requirement, now, expected) in cases {
        let mut sink = FakeSink::default();
        let effect_calls = Cell::new(0);
        let result = execute_effect(
            caller,
            request(requirement, b"denied-effect"),
            &FixedClock(now),
            AdmissionDecision::Ready,
            &mut sink,
            || {
                effect_calls.set(effect_calls.get() + 1);
                EffectExecution::succeeded(None)
            },
        );
        let actual = match result {
            Err(EffectGateError::Denied { denial, .. }) => Some(denial),
            _ => None,
        };
        assert_eq!(actual, Some(expected), "denial reason must be stable");
        assert_eq!(effect_calls.get(), 0, "denied effects must never run");
        assert!(sink.receipts.is_empty(), "denial is not an effect receipt");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn audit_order_and_failures_are_fail_closed() {
    let caller = valid_caller();
    let requirement = AuthorizationRequirement::new(standard_grant(), policy_epoch(7), None);
    let transcript = Rc::new(RefCell::new(Vec::new()));
    let mut sink = FakeSink::with_transcript(Rc::clone(&transcript));

    let completion = execute_effect(
        Some(&caller),
        request(requirement, b"ordered-effect"),
        &FixedClock(timestamp(1_500)),
        AdmissionDecision::Ready,
        &mut sink,
        || {
            transcript.borrow_mut().push("effect");
            EffectExecution::succeeded(None)
        },
    )
    .expect("ordered effect should complete");
    assert_eq!(completion.outcome(), EffectOutcome::Succeeded);
    assert_eq!(
        transcript.borrow().as_slice(),
        ["audit:intent", "effect", "audit:outcome"],
        "durable intent must precede effect and outcome"
    );

    let calls = Cell::new(0);
    let mut intent_failure = FakeSink::failing_at(0);
    let result = execute_effect(
        Some(&caller),
        request(requirement, b"intent-failure"),
        &FixedClock(timestamp(1_500)),
        AdmissionDecision::Ready,
        &mut intent_failure,
        || {
            calls.set(calls.get() + 1);
            EffectExecution::succeeded(None)
        },
    );
    assert!(
        matches!(result, Err(EffectGateError::IntentAudit { .. })),
        "intent append failure must propagate"
    );
    assert_eq!(calls.get(), 0, "intent failure must prevent the effect");

    let mut outcome_failure = FakeSink::failing_at(1);
    let result = execute_effect(
        Some(&caller),
        request(requirement, b"outcome-failure"),
        &FixedClock(timestamp(1_500)),
        AdmissionDecision::Ready,
        &mut outcome_failure,
        || {
            calls.set(calls.get() + 1);
            EffectExecution::succeeded(None)
        },
    );
    assert!(
        matches!(result, Err(EffectGateError::OutcomeAudit { .. })),
        "outcome append failure must require reconciliation"
    );
    assert_eq!(calls.get(), 1, "the admitted effect runs exactly once");

    let clock = SequenceClock::new([timestamp(1_500), timestamp(1_500), timestamp(1_499)]);
    let mut backward_outcome = FakeSink::default();
    let result = execute_effect(
        Some(&caller),
        request(requirement, b"backward-outcome-time"),
        &clock,
        AdmissionDecision::Ready,
        &mut backward_outcome,
        || {
            calls.set(calls.get() + 1);
            EffectExecution::succeeded(None)
        },
    );
    assert!(
        matches!(result, Err(EffectGateError::PostIntentContract { .. })),
        "an outcome timestamp before its intent must require reconciliation"
    );
    assert_eq!(
        calls.get(),
        2,
        "the admitted effect runs before a post-effect clock fault is observed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn authority_is_rechecked_after_durable_intent() {
    let caller = valid_caller();
    let requirement = AuthorizationRequirement::new(standard_grant(), policy_epoch(7), None);
    let clock = SequenceClock::new([timestamp(1_999), timestamp(2_000)]);
    let calls = Cell::new(0);
    let mut sink = FakeSink::default();

    let result = execute_effect(
        Some(&caller),
        request(requirement, b"lapsed-after-intent"),
        &clock,
        AdmissionDecision::Ready,
        &mut sink,
        || {
            calls.set(calls.get() + 1);
            EffectExecution::succeeded(None)
        },
    );
    assert!(
        matches!(
            result,
            Err(EffectGateError::PostIntentDenied {
                denial: AuthorizationDenial::Stale,
                ..
            })
        ),
        "authority that lapses during intent append must fail closed"
    );
    assert_eq!(
        calls.get(),
        0,
        "lapsed authority must cause zero domain I/O"
    );
    assert_eq!(
        sink.receipts.len(),
        2,
        "intent and no-effect outcome expected"
    );
    assert_eq!(
        sink.receipts
            .get(1)
            .expect("no-effect outcome receipt")
            .event(),
        ReceiptEvent::Outcome(EffectOutcome::AuthorizationDenied)
    );

    let clock = SequenceClock::new([timestamp(1_500), timestamp(1_499)]);
    let mut sink = FakeSink::default();
    let result = execute_effect(
        Some(&caller),
        request(requirement, b"regressed-clock"),
        &clock,
        AdmissionDecision::Ready,
        &mut sink,
        || {
            calls.set(calls.get() + 1);
            EffectExecution::succeeded(None)
        },
    );
    assert!(
        matches!(
            result,
            Err(EffectGateError::PostIntentDenied {
                denial: AuthorizationDenial::ClockRegression,
                ..
            })
        ),
        "a backwards trusted clock must produce a no-effect outcome"
    );
    assert_eq!(
        calls.get(),
        0,
        "clock regression must cause zero domain I/O"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_and_backpressure_never_invoke_the_effect() {
    let caller = valid_caller();
    let requirement = AuthorizationRequirement::new(standard_grant(), policy_epoch(7), None);
    for (admission, expected) in [
        (AdmissionDecision::Cancelled, EffectOutcome::Cancelled),
        (
            AdmissionDecision::Backpressured,
            EffectOutcome::Backpressured,
        ),
    ] {
        let calls = Cell::new(0);
        let mut sink = FakeSink::default();
        let completion = execute_effect(
            Some(&caller),
            request(requirement, b"bounded-effect"),
            &FixedClock(timestamp(1_500)),
            admission,
            &mut sink,
            || {
                calls.set(calls.get() + 1);
                EffectExecution::succeeded(None)
            },
        )
        .expect("bounded admission should record a terminal outcome");
        assert_eq!(completion.outcome(), expected);
        assert_eq!(calls.get(), 0, "bounded refusal must not invoke the effect");
        assert_eq!(
            sink.receipts.len(),
            2,
            "intent and terminal outcome expected"
        );
    }
}
