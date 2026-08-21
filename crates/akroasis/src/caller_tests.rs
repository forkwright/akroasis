//! Application caller-boundary unit fixtures.

#![expect(
    clippy::expect_used,
    reason = "fixed service-boundary fixtures use checked constructors"
)]

use std::cell::Cell;
use std::rc::Rc;

use tekmerion::{
    AuthorityClaims, AuthorityDecision, AuthorityGrant, AuthorizationDenial,
    AuthorizationRequirement, CALLER_CONTEXT_VERSION, CallerContractError, CapabilityRef,
    PersonaRef, PolicyEpoch, PrincipalSource, ScopeRef, authorize_caller,
};

use super::*;

impl AuthenticatedServiceIdentity {
    fn for_test(
        schema_version: u16,
        identity_ref: CallerRef,
        authenticated_at: Timestamp,
        expires_at: Timestamp,
        revocation: RevocationState,
    ) -> Self {
        Self {
            schema_version,
            identity_ref,
            authenticated_at,
            expires_at,
            revocation,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FixedClock(Timestamp);

impl TrustedClock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
enum ServiceMode {
    Granted,
    ReferenceMismatch,
    TimeMismatch,
    ExpiryMismatch,
}

#[derive(Debug)]
struct ServiceAuthority {
    mode: ServiceMode,
    calls: Rc<Cell<usize>>,
    grant: AuthorityGrant,
    policy_epoch: PolicyEpoch,
}

impl CallerAuthority for ServiceAuthority {
    fn resolve_local(
        &self,
        _peer: LocalPeerCredentials,
        _observed_at: Timestamp,
    ) -> AuthorityDecision {
        AuthorityDecision::UnknownIdentity
    }

    fn resolve_service(
        &self,
        identity_ref: CallerRef,
        authenticated_at: Timestamp,
        _evidence_expires_at: Timestamp,
        _observed_at: Timestamp,
    ) -> AuthorityDecision {
        self.calls.set(self.calls.get() + 1);
        let caller_ref = if matches!(self.mode, ServiceMode::ReferenceMismatch) {
            CallerRef::from_canonical(b"different-service")
        } else {
            identity_ref
        };
        let claim_time = if matches!(self.mode, ServiceMode::TimeMismatch) {
            timestamp(1_100)
        } else {
            authenticated_at
        };
        let claims_expires_at = if matches!(self.mode, ServiceMode::ExpiryMismatch) {
            timestamp(4_000)
        } else {
            timestamp(3_000)
        };
        let claims = AuthorityClaims::builder()
            .schema_version(CALLER_CONTEXT_VERSION)
            .caller_ref(caller_ref)
            .grant(self.grant)
            .policy_epoch(self.policy_epoch)
            .validity(claim_time, timestamp(2_000), claims_expires_at)
            .persona(PersonaRef::from_canonical(b"service-persona"))
            .build()
            .expect("valid service authority claims");
        AuthorityDecision::Granted(claims)
    }
}

fn timestamp(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis).expect("valid fixed timestamp")
}

fn resolver(mode: ServiceMode) -> (ApplicationCallerResolver<ServiceAuthority>, Rc<Cell<usize>>) {
    let calls = Rc::new(Cell::new(0));
    let authority = ServiceAuthority {
        mode,
        calls: Rc::clone(&calls),
        grant: AuthorityGrant::new(
            CapabilityRef::from_canonical(b"service.read"),
            ScopeRef::from_canonical(b"service.scope"),
        ),
        policy_epoch: PolicyEpoch::try_from_u64(4).expect("test policy epoch"),
    };
    (
        ApplicationCallerResolver::current(authority).expect("current resolver"),
        calls,
    )
}

fn evidence(
    schema_version: u16,
    authenticated_at: Timestamp,
    expires_at: Timestamp,
    revocation: RevocationState,
) -> AuthenticatedServiceIdentity {
    AuthenticatedServiceIdentity::for_test(
        schema_version,
        CallerRef::from_canonical(b"accepted-service"),
        authenticated_at,
        expires_at,
        revocation,
    )
}

#[test]
fn service_evidence_fails_closed_before_authority_lookup() {
    let (resolver, calls) = resolver(ServiceMode::Granted);
    let clock = FixedClock(timestamp(1_500));
    assert!(
        matches!(
            resolver.resolve_service(None, &clock),
            Err(ApplicationCallerError::LoopbackIsNotIdentity { .. })
        ),
        "loopback without identity must fail closed"
    );
    assert_eq!(calls.get(), 0, "loopback must not consult authority");

    for identity in [
        evidence(
            SERVICE_IDENTITY_EVIDENCE_VERSION + 1,
            timestamp(1_000),
            timestamp(3_000),
            RevocationState::Active,
        ),
        evidence(
            SERVICE_IDENTITY_EVIDENCE_VERSION,
            timestamp(1_600),
            timestamp(3_000),
            RevocationState::Active,
        ),
        evidence(
            SERVICE_IDENTITY_EVIDENCE_VERSION,
            timestamp(1_000),
            timestamp(1_500),
            RevocationState::Active,
        ),
        evidence(
            SERVICE_IDENTITY_EVIDENCE_VERSION,
            timestamp(1_000),
            timestamp(3_000),
            RevocationState::Revoked,
        ),
    ] {
        assert!(
            resolver.resolve_service(Some(&identity), &clock).is_err(),
            "invalid service evidence must fail closed"
        );
    }
    assert_eq!(
        calls.get(),
        0,
        "invalid evidence must fail before authority lookup"
    );
}

#[test]
fn service_claims_bind_to_authenticated_identity_and_time() {
    let identity = evidence(
        SERVICE_IDENTITY_EVIDENCE_VERSION,
        timestamp(1_000),
        timestamp(3_000),
        RevocationState::Active,
    );
    let clock = FixedClock(timestamp(1_500));

    let (granted_resolver, calls) = resolver(ServiceMode::Granted);
    let caller = granted_resolver
        .resolve_service(Some(&identity), &clock)
        .expect("accepted service caller");
    assert_eq!(caller.source(), PrincipalSource::ServiceIdentity);
    assert_eq!(
        caller.expires_at(),
        identity.expires_at,
        "caller validity must not outlive accepted service evidence"
    );
    let requirement =
        AuthorizationRequirement::new(caller.grant(), caller.policy_epoch(), caller.persona());
    assert_eq!(
        authorize_caller(Some(&caller), &requirement, identity.expires_at),
        Err(AuthorizationDenial::Expired),
        "service caller must fail at the evidence expiry boundary"
    );
    assert_eq!(calls.get(), 1, "authority should be consulted once");

    let (reference_mismatch_resolver, _) = resolver(ServiceMode::ReferenceMismatch);
    let result = reference_mismatch_resolver.resolve_service(Some(&identity), &clock);
    assert!(
        matches!(
            result,
            Err(ApplicationCallerError::CallerResolution {
                source: CallerContractError::IdentityReferenceMismatch { .. },
                ..
            })
        ),
        "service claims cannot substitute another identity"
    );

    let (time_mismatch_resolver, _) = resolver(ServiceMode::TimeMismatch);
    let result = time_mismatch_resolver.resolve_service(Some(&identity), &clock);
    assert!(
        matches!(
            result,
            Err(ApplicationCallerError::CallerResolution {
                source: CallerContractError::ServiceAuthenticationTimeMismatch { .. },
                ..
            })
        ),
        "service claims cannot backdate authentication"
    );

    let (expiry_mismatch_resolver, _) = resolver(ServiceMode::ExpiryMismatch);
    let result = expiry_mismatch_resolver.resolve_service(Some(&identity), &clock);
    assert!(
        matches!(
            result,
            Err(ApplicationCallerError::CallerResolution {
                source: CallerContractError::ServiceEvidenceExpiryMismatch { .. },
                ..
            })
        ),
        "service claims cannot outlive accepted identity evidence"
    );
}
