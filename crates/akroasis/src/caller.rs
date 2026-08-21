//! Application caller resolution and audit-before-effect execution.
//!
//! This module adapts authenticated transport evidence into the shared
//! `tekmerion` contract. It owns no credential store and does not implement any
//! domain-specific effect or policy.

use std::fmt;

use snafu::{ResultExt, Snafu};
use stoicheion::Timestamp;
use tekmerion::{
    AuthorizationDenial, AuthorizationRequirement, CALLER_RESOLVER_VERSION, CallerAuthority,
    CallerContractError, CallerRef, CallerResolver, EffectDescriptor, EffectOutcome, EffectReceipt,
    EffectReceiptError, EffectReceiptSink, EvidenceDigest, LocalPeerCredentials, PersistedIntent,
    PersistedOutcome, ReceiptDigest, RecoveryAuthorization, RecoveryTicket, RevocationState,
    ValidatedCaller, authorize_caller,
};
use tokio::net::UnixStream;

/// Version of accepted service-identity evidence.
pub const SERVICE_IDENTITY_EVIDENCE_VERSION: u16 = 1;

/// Time source used by caller resolution and effect-time authorization.
pub trait TrustedClock {
    /// Return the current trusted UTC timestamp.
    fn now(&self) -> Timestamp;
}

/// Production wall-clock source.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl TrustedClock for SystemClock {
    // kanon:ignore ARCHITECTURE/trait-impl-colocation -- built-in wall clock is the production adapter; consumers inject fake clocks
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// Authenticated nonlocal service evidence accepted by the application.
///
/// Fields are private and the type is not deserializable, so a request cannot
/// turn an address, header, or replayed JSON object into caller authority. A
/// future accepted Phase 05 authenticator will own production construction.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthenticatedServiceIdentity {
    schema_version: u16,
    identity_ref: CallerRef,
    authenticated_at: Timestamp,
    expires_at: Timestamp,
    revocation: RevocationState,
}

impl fmt::Debug for AuthenticatedServiceIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthenticatedServiceIdentity")
            .field("schema_version", &self.schema_version)
            .field("authenticated_at", &self.authenticated_at)
            .field("expires_at", &self.expires_at)
            .field("revocation", &self.revocation)
            .finish_non_exhaustive()
    }
}

/// Application resolver over one trusted ACL/service authority adapter.
///
/// The shared resolver constructs the opaque caller type. This wrapper obtains
/// kernel peer credentials and rejects unauthenticated network callers first.
#[derive(Debug)]
pub struct ApplicationCallerResolver<A> {
    shared: CallerResolver<A>,
}

impl<A> ApplicationCallerResolver<A>
where
    A: CallerAuthority,
{
    /// Construct the current application resolver.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationCallerError`] when the resolver version is not
    /// understood.
    pub fn try_new(resolver_version: u16, authority: A) -> Result<Self, ApplicationCallerError> {
        let shared =
            CallerResolver::try_new(resolver_version, authority).context(CallerResolutionSnafu)?;
        Ok(Self { shared })
    }

    /// Construct the resolver at the current known version.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationCallerError`] if shared resolver construction
    /// unexpectedly fails.
    pub fn current(authority: A) -> Result<Self, ApplicationCallerError> {
        Self::try_new(CALLER_RESOLVER_VERSION, authority)
    }

    /// Resolve a local caller from kernel-authenticated Unix peer credentials.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationCallerError`] when peer credentials cannot be
    /// obtained or the authority does not grant a current caller.
    pub fn resolve_local<C>(
        &self,
        stream: &UnixStream,
        clock: &C,
    ) -> Result<ValidatedCaller, ApplicationCallerError>
    where
        C: TrustedClock,
    {
        let credentials = stream.peer_cred().context(PeerCredentialsSnafu)?;
        let pid = credentials
            .pid()
            .map(u32::try_from)
            .transpose()
            .context(InvalidPeerPidSnafu)?;
        let peer = LocalPeerCredentials::from_os_peer(credentials.uid(), credentials.gid(), pid);
        self.shared
            .resolve_local(peer, clock.now())
            .context(CallerResolutionSnafu)
    }

    /// Resolve a nonlocal caller from accepted service-identity evidence.
    ///
    /// Passing `None` represents a network address, including loopback,
    /// without authenticated identity and always fails closed.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationCallerError`] when evidence is absent, unknown,
    /// future-issued, expired, revoked, mismatched, or rejected.
    pub fn resolve_service<C>(
        &self,
        identity: Option<&AuthenticatedServiceIdentity>,
        clock: &C,
    ) -> Result<ValidatedCaller, ApplicationCallerError>
    where
        C: TrustedClock,
    {
        let identity = identity.ok_or(ApplicationCallerError::LoopbackIsNotIdentity {
            location: snafu::location!(),
        })?;
        if identity.schema_version != SERVICE_IDENTITY_EVIDENCE_VERSION {
            return Err(ApplicationCallerError::UnknownServiceIdentityVersion {
                schema_version: identity.schema_version,
                location: snafu::location!(),
            });
        }
        if identity.revocation != RevocationState::Active {
            return Err(ApplicationCallerError::ServiceIdentityRevoked {
                location: snafu::location!(),
            });
        }
        let observed_at = clock.now();
        if observed_at < identity.authenticated_at {
            return Err(ApplicationCallerError::ServiceIdentityNotYetValid {
                location: snafu::location!(),
            });
        }
        if observed_at >= identity.expires_at {
            return Err(ApplicationCallerError::ServiceIdentityExpired {
                location: snafu::location!(),
            });
        }
        self.shared
            .resolve_service(
                identity.identity_ref,
                identity.authenticated_at,
                identity.expires_at,
                observed_at,
            )
            .context(CallerResolutionSnafu)
    }
}

/// Immutable request needed to authorize and record one protected effect.
#[derive(Debug, PartialEq, Eq)]
pub struct EffectRequest {
    requirement: AuthorizationRequirement,
    descriptor: EffectDescriptor,
}

impl EffectRequest {
    /// Construct one protected effect request.
    #[must_use]
    pub const fn new(requirement: AuthorizationRequirement, descriptor: EffectDescriptor) -> Self {
        Self {
            requirement,
            descriptor,
        }
    }
}

/// Injected bounded-queue and cancellation decision made before domain I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDecision {
    // kanon:ignore RUST/non-exhaustive-enum -- closed admission state prevents silent effect execution on unknown states
    /// The effect may be invoked after durable intent.
    Ready,
    /// Cancellation was observed before domain I/O.
    Cancelled,
    /// A declared resource budget refused admission.
    Backpressured,
}

/// Closed result returned by an invoked fake or domain effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectExecution {
    outcome: EffectOutcome,
    evidence_digest: Option<EvidenceDigest>,
}

impl EffectExecution {
    /// Record a successful domain effect.
    #[must_use]
    pub const fn succeeded(evidence_digest: Option<EvidenceDigest>) -> Self {
        Self {
            outcome: EffectOutcome::Succeeded,
            evidence_digest,
        }
    }

    /// Record a failed effect with no committed state change.
    #[must_use]
    pub const fn failed(evidence_digest: Option<EvidenceDigest>) -> Self {
        Self {
            outcome: EffectOutcome::Failed,
            evidence_digest,
        }
    }

    /// Record a partially committed effect that requires recovery.
    #[must_use]
    pub const fn partial(evidence_digest: Option<EvidenceDigest>) -> Self {
        Self {
            outcome: EffectOutcome::Partial,
            evidence_digest,
        }
    }
}

/// Terminal state returned only after its outcome receipt was appended.
#[derive(Debug, PartialEq, Eq)]
pub struct EffectCompletion {
    outcome: EffectOutcome,
    receipt_digest: ReceiptDigest,
    recovery: Option<RecoveryAuthorization>,
}

impl EffectCompletion {
    /// Return the recorded terminal or partial outcome.
    #[must_use]
    pub const fn outcome(&self) -> EffectOutcome {
        self.outcome
    }

    /// Return the canonical durable outcome-receipt digest.
    #[must_use]
    pub const fn receipt_digest(&self) -> ReceiptDigest {
        self.receipt_digest
    }

    /// Consume the completion and return explicit recovery authority, if the
    /// persisted outcome requires it.
    #[must_use]
    pub const fn into_recovery(self) -> Option<RecoveryAuthorization> {
        self.recovery
    }
}

/// Resolve current authorization, append intent, invoke once, and append outcome.
///
/// The effect closure is not called on denial, intent-audit failure,
/// cancellation, or backpressure. No reusable permit escapes this function.
///
/// # Errors
///
/// Returns [`EffectGateError`] for a typed denial, receipt-contract failure,
/// or durable append failure. A post-intent failure carries a restart-safe
/// recovery ticket and never reports success.
pub fn execute_effect<S, C, F>(
    caller: Option<&ValidatedCaller>,
    request: EffectRequest,
    clock: &C,
    admission: AdmissionDecision,
    sink: &mut S,
    effect: F,
) -> Result<EffectCompletion, EffectGateError<S::Error>>
where
    S: EffectReceiptSink,
    C: TrustedClock,
    F: FnOnce() -> EffectExecution,
{
    let EffectRequest {
        requirement,
        descriptor,
    } = request;
    let authorized_at = clock.now();
    let authorized = authorize_caller(caller, &requirement, authorized_at).map_err(|denial| {
        EffectGateError::Denied {
            denial,
            location: snafu::location!(),
        }
    })?;
    let pending = EffectReceipt::intent(authorized, descriptor, authorized_at)
        .context(ReceiptContractSnafu)?;
    let persisted = pending
        .append_to(sink)
        .map_err(|source| EffectGateError::IntentAudit {
            source,
            location: snafu::location!(),
        })?;

    let effect_time = clock.now();
    let effect_authorization = if effect_time < authorized_at {
        Err(AuthorizationDenial::ClockRegression)
    } else {
        authorize_caller(caller, &requirement, effect_time)
    };
    if let Err(denial) = effect_authorization {
        let denial_observed_at = std::cmp::max(effect_time, authorized_at);
        let denied = append_effect_outcome(
            persisted,
            denial_observed_at,
            EffectExecution {
                outcome: EffectOutcome::AuthorizationDenied,
                evidence_digest: None,
            },
            sink,
        )?;
        let receipt_digest = denied.digest();
        let recovery = if outcome_needs_recovery(
            EffectOutcome::AuthorizationDenied,
            denied.receipt().recovery(),
        ) {
            Some(Box::new(
                denied
                    .into_recovery_authorization()
                    .context(ReceiptContractSnafu)?,
            ))
        } else {
            None
        };
        return Err(EffectGateError::PostIntentDenied {
            denial,
            receipt_digest,
            recovery,
            location: snafu::location!(),
        });
    }

    let execution = match admission {
        AdmissionDecision::Ready => effect(),
        AdmissionDecision::Cancelled => EffectExecution {
            outcome: EffectOutcome::Cancelled,
            evidence_digest: None,
        },
        AdmissionDecision::Backpressured => EffectExecution {
            outcome: EffectOutcome::Backpressured,
            evidence_digest: None,
        },
    };
    let persisted_outcome = append_effect_outcome(persisted, clock.now(), execution, sink)?;
    completion_from_persisted(persisted_outcome)
}

fn append_effect_outcome<S>(
    persisted: PersistedIntent,
    observed_at: Timestamp,
    execution: EffectExecution,
    sink: &mut S,
) -> Result<PersistedOutcome, EffectGateError<S::Error>>
where
    S: EffectReceiptSink,
{
    let pending = persisted
        .outcome(observed_at, execution.outcome, execution.evidence_digest)
        .map_err(|error| {
            let (source, recovery) = error.into_parts();
            EffectGateError::PostIntentContract {
                source,
                recovery,
                location: snafu::location!(),
            }
        })?;
    pending.append_to(sink).map_err(|error| {
        let (source, recovery) = error.into_parts();
        EffectGateError::OutcomeAudit {
            source,
            recovery,
            location: snafu::location!(),
        }
    })
}

fn completion_from_persisted<E>(
    persisted_outcome: PersistedOutcome,
) -> Result<EffectCompletion, EffectGateError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    let recorded_outcome = persisted_outcome.outcome().context(ReceiptContractSnafu)?;
    let receipt_digest = persisted_outcome.digest();
    let recovery_relation = persisted_outcome.receipt().recovery();
    let recovery = if outcome_needs_recovery(recorded_outcome, recovery_relation) {
        Some(
            persisted_outcome
                .into_recovery_authorization()
                .context(ReceiptContractSnafu)?,
        )
    } else {
        None
    };

    Ok(EffectCompletion {
        outcome: recorded_outcome,
        receipt_digest,
        recovery,
    })
}

const fn outcome_needs_recovery(
    outcome: EffectOutcome,
    recovery: tekmerion::RecoveryRelation,
) -> bool {
    matches!(
        (outcome, recovery),
        (
            EffectOutcome::Partial | EffectOutcome::RecoveryRequired,
            tekmerion::RecoveryRelation::RequiredFor(_)
        ) | (
            EffectOutcome::Failed
                | EffectOutcome::Cancelled
                | EffectOutcome::Backpressured
                | EffectOutcome::AuthorizationDenied,
            tekmerion::RecoveryRelation::RecoveryOf(_)
        )
    )
}

/// Errors from application caller resolution.
#[derive(Snafu)]
#[non_exhaustive]
pub enum ApplicationCallerError {
    /// Kernel peer credentials could not be read from the Unix socket.
    #[snafu(display("failed to read local peer credentials: {source}"))]
    PeerCredentials {
        /// Operating-system error.
        source: std::io::Error,
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Kernel returned a peer PID outside the shared contract.
    #[snafu(display("local peer PID is outside the supported range: {source}"))]
    InvalidPeerPid {
        /// Integer conversion failure.
        source: std::num::TryFromIntError,
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// A network address was presented without authenticated service identity.
    #[snafu(display("loopback or network address is not caller identity"))]
    LoopbackIsNotIdentity {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Service evidence used an unknown schema version.
    #[snafu(display("unknown service identity schema version {schema_version}"))]
    UnknownServiceIdentityVersion {
        /// Unsupported service-evidence version.
        schema_version: u16,
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Service identity evidence is not yet valid.
    #[snafu(display("service identity evidence is not yet valid"))]
    ServiceIdentityNotYetValid {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Service identity evidence was revoked.
    #[snafu(display("service identity evidence is revoked"))]
    ServiceIdentityRevoked {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Service identity evidence expired.
    #[snafu(display("service identity evidence is expired"))]
    ServiceIdentityExpired {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Shared resolver rejected the authority decision.
    #[snafu(display("caller resolution failed: {source}"))]
    CallerResolution {
        /// Shared caller-contract failure.
        source: CallerContractError,
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

impl fmt::Debug for ApplicationCallerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::PeerCredentials { .. } => "ApplicationCallerError::PeerCredentials",
            Self::InvalidPeerPid { .. } => "ApplicationCallerError::InvalidPeerPid",
            Self::LoopbackIsNotIdentity { .. } => "ApplicationCallerError::LoopbackIsNotIdentity",
            Self::UnknownServiceIdentityVersion { .. } => {
                "ApplicationCallerError::UnknownServiceIdentityVersion"
            }
            Self::ServiceIdentityNotYetValid { .. } => {
                "ApplicationCallerError::ServiceIdentityNotYetValid"
            }
            Self::ServiceIdentityRevoked { .. } => "ApplicationCallerError::ServiceIdentityRevoked",
            Self::ServiceIdentityExpired { .. } => "ApplicationCallerError::ServiceIdentityExpired",
            Self::CallerResolution { .. } => "ApplicationCallerError::CallerResolution",
        })
    }
}

/// Failure to authorize, invoke, or durably record a protected effect.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum EffectGateError<E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    /// Immediate caller authorization failed before intent or effect.
    #[snafu(display("effect authorization denied: {denial}"))]
    Denied {
        /// Exact fail-closed authorization reason.
        denial: AuthorizationDenial,
        /// Source location of the denied request.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Receipt construction failed before the effect ran.
    #[snafu(display("effect receipt contract failed: {source}"))]
    ReceiptContract {
        /// Shared receipt-contract error.
        source: EffectReceiptError,
        /// Source location of the invalid receipt.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Durable intent append failed, so no effect ran.
    #[snafu(display("effect intent append failed: {source}"))]
    IntentAudit {
        /// Accepted receipt-sink failure.
        source: E,
        /// Source location of the failed append.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Authorization lapsed after durable intent and the no-effect outcome
    /// was durably recorded.
    #[snafu(display("effect authorization lapsed after durable intent: {denial}"))]
    PostIntentDenied {
        /// Exact effect-time denial.
        denial: AuthorizationDenial,
        /// Durable no-effect outcome receipt.
        receipt_digest: ReceiptDigest,
        /// Continuation authority when the denied intent was itself recovery.
        recovery: Option<Box<RecoveryAuthorization>>,
        /// Source location of the denied execution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// A contract failure occurred after intent and requires reconciliation.
    #[snafu(display("post-intent receipt contract failed: {source}"))]
    PostIntentContract {
        /// Shared receipt-contract error.
        source: EffectReceiptError,
        /// Restart-safe reconciliation ticket.
        recovery: Box<RecoveryTicket>,
        /// Source location of the failed transition.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Outcome append failed after intent and possibly effect execution.
    #[snafu(display("effect outcome append failed: {source}"))]
    OutcomeAudit {
        /// Accepted receipt-sink failure.
        source: E,
        /// Restart-safe reconciliation ticket.
        recovery: Box<RecoveryTicket>,
        /// Source location of the failed append.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

#[cfg(test)]
#[path = "caller_tests.rs"]
mod tests;
