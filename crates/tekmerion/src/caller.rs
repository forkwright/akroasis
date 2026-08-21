//! Versioned caller authorization contracts.
//!
//! Authentication happens at an application boundary. This module gives that
//! boundary one closed type to return and gives domain crates one common,
//! fail-closed authorization vocabulary. It deliberately contains no
//! credential, token, clock, or policy store.

use std::fmt;

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ensure};

pub use crate::caller_error::CallerContractError;
use crate::caller_error::{
    AuthorityUnavailableSnafu, ExpiredSnafu, IdentityReferenceMismatchSnafu,
    InvalidValidityWindowSnafu, MissingCallerFieldSnafu, NotYetValidSnafu, RevokedIdentitySnafu,
    ServiceAuthenticationTimeMismatchSnafu, ServiceEvidenceExpiryMismatchSnafu,
    ServiceEvidenceNotYetValidSnafu, StaleSnafu, UnknownCallerVersionSnafu, UnknownIdentitySnafu,
    UnknownResolverVersionSnafu, UntrustedIdentitySnafu,
};
pub use crate::caller_ref::{
    CallerRef, CapabilityRef, EffectRef, EvidenceDigest, PersonaRef, PolicyEpoch, ReceiptDigest,
    SchemaEpoch, ScopeRef,
};
use stoicheion::Timestamp;

/// Caller-context schema version understood by this crate.
pub const CALLER_CONTEXT_VERSION: u16 = 1;

/// Policy-free resolver version understood by this crate.
pub const CALLER_RESOLVER_VERSION: u16 = 1;

/// Source that authenticated a human or service caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum PrincipalSource {
    // kanon:ignore RUST/non-exhaustive-enum -- closed security vocabulary; changes require a caller-contract version bump
    /// Local operating-system peer credentials checked against an ACL.
    LocalOsPeer,
    /// A nonlocal service identity accepted by the service boundary.
    ServiceIdentity,
}

/// Authentication trust recorded on a resolved caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum TrustState {
    // kanon:ignore RUST/non-exhaustive-enum -- closed security vocabulary; changes require a caller-contract version bump
    /// The configured authority authenticated the principal.
    Trusted,
    /// The principal was not authenticated.
    Untrusted,
}

/// Revocation state recorded on a resolved caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum RevocationState {
    // kanon:ignore RUST/non-exhaustive-enum -- closed security vocabulary; changes require a caller-contract version bump
    /// The caller remains active under the current policy epoch.
    Active,
    /// The caller was revoked and cannot authorize an operation.
    Revoked,
}

/// One exact capability and scope granted by an authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct AuthorityGrant {
    capability: CapabilityRef,
    scope: ScopeRef,
}

impl AuthorityGrant {
    /// Construct one exact grant.
    #[must_use]
    pub const fn new(capability: CapabilityRef, scope: ScopeRef) -> Self {
        Self { capability, scope }
    }

    /// Return the granted capability reference.
    #[must_use]
    pub const fn capability(&self) -> CapabilityRef {
        self.capability
    }

    /// Return the granted scope reference.
    #[must_use]
    pub const fn scope(&self) -> ScopeRef {
        self.scope
    }
}

/// Operating-system credentials read from a connected local peer socket.
///
/// The application passes kernel-derived values to the shared resolver rather
/// than treating an address, header, or caller-supplied UID as identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalPeerCredentials {
    uid: u32,
    gid: u32,
    pid: Option<u32>,
}

impl LocalPeerCredentials {
    /// Construct credentials obtained from an operating-system peer query.
    #[must_use]
    pub const fn from_os_peer(uid: u32, gid: u32, pid: Option<u32>) -> Self {
        Self { uid, gid, pid }
    }

    /// Return the effective peer user ID.
    #[must_use]
    pub const fn uid(&self) -> u32 {
        self.uid
    }

    /// Return the effective peer group ID.
    #[must_use]
    pub const fn gid(&self) -> u32 {
        self.gid
    }

    /// Return the peer process ID when the platform supplies one.
    #[must_use]
    pub const fn pid(&self) -> Option<u32> {
        self.pid
    }
}

/// Builder for claims emitted by a trusted application authority adapter.
///
/// Authority adapters are trusted application composition, not request data.
/// The final [`ValidatedCaller`] remains constructible only by
/// [`CallerResolver`].
#[derive(Debug, Default)]
pub struct AuthorityClaimsBuilder {
    schema_version: Option<u16>,
    caller_ref: Option<CallerRef>,
    grant: Option<AuthorityGrant>,
    policy_epoch: Option<PolicyEpoch>,
    authenticated_at: Option<Timestamp>,
    fresh_until: Option<Timestamp>,
    expires_at: Option<Timestamp>,
    persona: Option<PersonaRef>,
}

impl AuthorityClaimsBuilder {
    /// Start an empty authority-claims builder.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            schema_version: None,
            caller_ref: None,
            grant: None,
            policy_epoch: None,
            authenticated_at: None,
            fresh_until: None,
            expires_at: None,
            persona: None,
        }
    }

    /// Set the caller-contract schema version emitted by the authority.
    #[must_use]
    pub const fn schema_version(mut self, schema_version: u16) -> Self {
        self.schema_version = Some(schema_version);
        self
    }

    /// Set the opaque authenticated identity reference.
    #[must_use]
    pub const fn caller_ref(mut self, caller_ref: CallerRef) -> Self {
        self.caller_ref = Some(caller_ref);
        self
    }

    /// Set the exact capability and scope grant.
    #[must_use]
    pub const fn grant(mut self, grant: AuthorityGrant) -> Self {
        self.grant = Some(grant);
        self
    }

    /// Set the granting policy epoch.
    #[must_use]
    pub const fn policy_epoch(mut self, policy_epoch: PolicyEpoch) -> Self {
        self.policy_epoch = Some(policy_epoch);
        self
    }

    /// Set authentication, freshness, and expiry bounds.
    #[must_use]
    pub const fn validity(
        mut self,
        authenticated_at: Timestamp,
        fresh_until: Timestamp,
        expires_at: Timestamp,
    ) -> Self {
        self.authenticated_at = Some(authenticated_at);
        self.fresh_until = Some(fresh_until);
        self.expires_at = Some(expires_at);
        self
    }

    /// Bind the caller to a persona.
    #[must_use]
    pub const fn persona(mut self, persona: PersonaRef) -> Self {
        self.persona = Some(persona);
        self
    }

    /// Build authority claims whose structural invariants hold.
    ///
    /// # Errors
    ///
    /// Returns [`CallerContractError`] when a required claim is absent or the
    /// validity window is empty or non-monotonic.
    pub fn build(self) -> Result<AuthorityClaims, CallerContractError> {
        let schema_version = self.schema_version.context(MissingCallerFieldSnafu {
            field: "schema_version",
        })?;
        let caller_ref = self.caller_ref.context(MissingCallerFieldSnafu {
            field: "caller_ref",
        })?;
        let grant = self
            .grant
            .context(MissingCallerFieldSnafu { field: "grant" })?;
        let policy_epoch = self.policy_epoch.context(MissingCallerFieldSnafu {
            field: "policy_epoch",
        })?;
        let authenticated_at = self.authenticated_at.context(MissingCallerFieldSnafu {
            field: "authenticated_at",
        })?;
        let fresh_until = self.fresh_until.context(MissingCallerFieldSnafu {
            field: "fresh_until",
        })?;
        let expires_at = self.expires_at.context(MissingCallerFieldSnafu {
            field: "expires_at",
        })?;
        ensure!(
            authenticated_at < fresh_until && fresh_until < expires_at,
            InvalidValidityWindowSnafu
        );

        Ok(AuthorityClaims {
            schema_version,
            caller_ref,
            grant,
            policy_epoch,
            authenticated_at,
            fresh_until,
            expires_at,
            persona: self.persona,
        })
    }
}

/// Policy claims returned by a trusted application authority adapter.
///
/// This intermediate type is not caller authority and cannot invoke an
/// effect. Only [`CallerResolver`] can turn it into a [`ValidatedCaller`].
#[derive(Debug)]
pub struct AuthorityClaims {
    schema_version: u16,
    caller_ref: CallerRef,
    grant: AuthorityGrant,
    policy_epoch: PolicyEpoch,
    authenticated_at: Timestamp,
    fresh_until: Timestamp,
    expires_at: Timestamp,
    persona: Option<PersonaRef>,
}

impl AuthorityClaims {
    /// Start an authority-claims builder.
    #[must_use]
    pub const fn builder() -> AuthorityClaimsBuilder {
        AuthorityClaimsBuilder::new()
    }
}

/// Result of consulting the application-owned ACL or service policy.
#[derive(Debug)]
pub enum AuthorityDecision {
    // kanon:ignore RUST/non-exhaustive-enum -- every authority result is handled fail-closed by the versioned resolver
    /// The authority returned closed policy claims for the principal.
    Granted(AuthorityClaims),
    /// The presented identity is not known to the authority.
    UnknownIdentity,
    /// The authority could not authenticate the identity.
    UntrustedIdentity,
    /// The identity was explicitly revoked.
    Revoked,
    /// The authority could not make a current decision.
    Unavailable,
}

/// Application-owned source of caller authority.
///
/// Implementations validate peer credentials against an ACL and accept a
/// nonlocal identity only after the service boundary has authenticated it.
/// Linked application authority adapters are trusted code; hostile request or
/// wire data cannot implement this trait or deserialize a caller context.
pub trait CallerAuthority {
    /// Resolve kernel-derived local peer credentials.
    fn resolve_local(
        &self,
        peer: LocalPeerCredentials,
        observed_at: Timestamp,
    ) -> AuthorityDecision;

    /// Resolve an already-authenticated nonlocal identity reference.
    fn resolve_service(
        &self,
        identity_ref: CallerRef,
        authenticated_at: Timestamp,
        evidence_expires_at: Timestamp,
        observed_at: Timestamp,
    ) -> AuthorityDecision;
}

/// Policy-free resolver that is the sole caller-context constructor.
///
/// Application composition owns this resolver. Domain crates receive only the
/// resulting [`ValidatedCaller`] and independently enforce their effect-time
/// scope policy.
#[doc(hidden)]
#[derive(Debug)]
pub struct CallerResolver<A> {
    authority: A,
}

impl<A> CallerResolver<A>
where
    A: CallerAuthority,
{
    /// Construct a resolver for a known resolver version.
    ///
    /// # Errors
    ///
    /// Returns [`CallerContractError::UnknownResolverVersion`] for an unknown
    /// version before consulting the authority.
    pub fn try_new(resolver_version: u16, authority: A) -> Result<Self, CallerContractError> {
        ensure!(
            resolver_version == CALLER_RESOLVER_VERSION,
            UnknownResolverVersionSnafu { resolver_version }
        );
        Ok(Self { authority })
    }

    /// Resolve a caller from kernel-derived local peer credentials.
    ///
    /// # Errors
    ///
    /// Returns a typed fail-closed error for rejected, unavailable, unknown,
    /// revoked, stale, expired, or structurally invalid authority output.
    pub fn resolve_local(
        &self,
        peer: LocalPeerCredentials,
        observed_at: Timestamp,
    ) -> Result<ValidatedCaller, CallerContractError> {
        let decision = self.authority.resolve_local(peer, observed_at);
        Self::resolve_decision(PrincipalSource::LocalOsPeer, None, observed_at, decision)
    }

    /// Resolve a caller from accepted service-identity evidence.
    ///
    /// # Errors
    ///
    /// Returns a typed fail-closed error when the authority rejects the
    /// service or returns claims for a different identity reference.
    pub fn resolve_service(
        &self,
        identity_ref: CallerRef,
        authenticated_at: Timestamp,
        evidence_expires_at: Timestamp,
        observed_at: Timestamp,
    ) -> Result<ValidatedCaller, CallerContractError> {
        ensure!(
            authenticated_at <= observed_at,
            ServiceEvidenceNotYetValidSnafu
        );
        ensure!(
            observed_at < evidence_expires_at,
            ServiceEvidenceExpiryMismatchSnafu
        );
        let decision = self.authority.resolve_service(
            identity_ref,
            authenticated_at,
            evidence_expires_at,
            observed_at,
        );
        Self::resolve_decision(
            PrincipalSource::ServiceIdentity,
            Some((identity_ref, authenticated_at, evidence_expires_at)),
            observed_at,
            decision,
        )
    }

    fn resolve_decision(
        source: PrincipalSource,
        expected_service: Option<(CallerRef, Timestamp, Timestamp)>,
        observed_at: Timestamp,
        decision: AuthorityDecision,
    ) -> Result<ValidatedCaller, CallerContractError> {
        let claims = match decision {
            AuthorityDecision::Granted(claims) => claims,
            AuthorityDecision::UnknownIdentity => return UnknownIdentitySnafu.fail(),
            AuthorityDecision::UntrustedIdentity => return UntrustedIdentitySnafu.fail(),
            AuthorityDecision::Revoked => return RevokedIdentitySnafu.fail(),
            AuthorityDecision::Unavailable => return AuthorityUnavailableSnafu.fail(),
        };

        ensure!(
            claims.schema_version == CALLER_CONTEXT_VERSION,
            UnknownCallerVersionSnafu {
                schema_version: claims.schema_version,
            }
        );
        if let Some((expected_identity, expected_authenticated_at, evidence_expires_at)) =
            expected_service
        {
            ensure!(
                claims.caller_ref == expected_identity,
                IdentityReferenceMismatchSnafu
            );
            ensure!(
                claims.authenticated_at == expected_authenticated_at,
                ServiceAuthenticationTimeMismatchSnafu
            );
            ensure!(
                claims.expires_at <= evidence_expires_at,
                ServiceEvidenceExpiryMismatchSnafu
            );
        }
        ensure!(observed_at >= claims.authenticated_at, NotYetValidSnafu);
        ensure!(observed_at < claims.expires_at, ExpiredSnafu);
        ensure!(observed_at < claims.fresh_until, StaleSnafu);

        Ok(ValidatedCaller::from_authority(source, claims))
    }
}

/// Authenticated caller claims returned by the application resolver.
///
/// The type is serializable for minimized diagnostics but deliberately has no
/// `Deserialize`, `Default`, builder, or public conversion implementation.
#[derive(PartialEq, Eq, Serialize)]
pub struct ValidatedCaller {
    schema_version: u16,
    source: PrincipalSource,
    caller_ref: CallerRef,
    grant: AuthorityGrant,
    policy_epoch: PolicyEpoch,
    authenticated_at: Timestamp,
    fresh_until: Timestamp,
    expires_at: Timestamp,
    persona: Option<PersonaRef>,
    trust: TrustState,
    revocation: RevocationState,
}

impl ValidatedCaller {
    #[expect(
        clippy::needless_pass_by_value,
        reason = "authority claims are consumed exactly once at the resolver boundary"
    )]
    const fn from_authority(source: PrincipalSource, claims: AuthorityClaims) -> Self {
        Self {
            schema_version: CALLER_CONTEXT_VERSION,
            source,
            caller_ref: claims.caller_ref,
            grant: claims.grant,
            policy_epoch: claims.policy_epoch,
            authenticated_at: claims.authenticated_at,
            fresh_until: claims.fresh_until,
            expires_at: claims.expires_at,
            persona: claims.persona,
            trust: TrustState::Trusted,
            revocation: RevocationState::Active,
        }
    }

    /// Return the caller-context schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Return the source that authenticated the caller.
    #[must_use]
    pub const fn source(&self) -> PrincipalSource {
        self.source
    }

    /// Return the caller's opaque identity reference.
    #[must_use]
    pub const fn caller_ref(&self) -> CallerRef {
        self.caller_ref
    }

    /// Return the exact grant carried by the caller context.
    #[must_use]
    pub const fn grant(&self) -> AuthorityGrant {
        self.grant
    }

    /// Return the policy epoch that granted this context.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }

    /// Return the optional persona binding.
    #[must_use]
    pub const fn persona(&self) -> Option<PersonaRef> {
        self.persona
    }

    /// Return the authentication time.
    #[must_use]
    pub const fn authenticated_at(&self) -> Timestamp {
        self.authenticated_at
    }

    /// Return the first instant at which the context is stale.
    #[must_use]
    pub const fn fresh_until(&self) -> Timestamp {
        self.fresh_until
    }

    /// Return the first instant at which the context is expired.
    #[must_use]
    pub const fn expires_at(&self) -> Timestamp {
        self.expires_at
    }
}

impl fmt::Debug for ValidatedCaller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidatedCaller")
            .field("schema_version", &self.schema_version)
            .field("source", &self.source)
            .field("policy_epoch", &self.policy_epoch)
            .field("authenticated_at", &self.authenticated_at)
            .field("fresh_until", &self.fresh_until)
            .field("expires_at", &self.expires_at)
            .field("trust", &self.trust)
            .field("revocation", &self.revocation)
            .finish_non_exhaustive()
    }
}

/// Domain requirement checked immediately before a protected read or effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizationRequirement {
    grant: AuthorityGrant,
    policy_epoch: PolicyEpoch,
    persona: Option<PersonaRef>,
}

impl AuthorizationRequirement {
    /// Construct one effect-time authorization requirement.
    #[must_use]
    pub const fn new(
        grant: AuthorityGrant,
        policy_epoch: PolicyEpoch,
        persona: Option<PersonaRef>,
    ) -> Self {
        Self {
            grant,
            policy_epoch,
            persona,
        }
    }

    /// Return the required exact grant.
    #[must_use]
    pub const fn grant(&self) -> AuthorityGrant {
        self.grant
    }

    /// Return the policy epoch that must still be current.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }

    /// Return the exact required persona binding.
    #[must_use]
    pub const fn persona(&self) -> Option<PersonaRef> {
        self.persona
    }
}

/// Typed reason a caller failed an immediate authorization check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthorizationDenial {
    // kanon:ignore RUST/non-exhaustive-enum -- denial precedence is a closed, versioned security contract
    /// No caller context was supplied.
    MissingContext,
    /// The caller context schema is not understood.
    UnknownContextVersion,
    /// The authority source did not trust the caller.
    Untrusted,
    /// The context was used before its authentication time.
    NotYetValid,
    /// The caller context reached its freshness bound.
    Stale,
    /// The caller context reached its hard expiry.
    Expired,
    /// The caller was revoked.
    Revoked,
    /// The granting policy epoch is no longer current.
    PolicyEpochMismatch,
    /// The caller is bound to a different persona.
    WrongPersona,
    /// The caller lacks the required domain capability.
    InsufficientCapability,
    /// The capability grant has a different domain scope.
    InsufficientScope,
    /// The trusted application clock moved backwards during one effect gate.
    ClockRegression,
}

impl fmt::Display for AuthorizationDenial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::MissingContext => "caller context is missing",
            Self::UnknownContextVersion => "caller context version is unknown",
            Self::Untrusted => "caller is untrusted",
            Self::NotYetValid => "caller context is not yet valid",
            Self::Stale => "caller context is stale",
            Self::Expired => "caller context is expired",
            Self::Revoked => "caller is revoked",
            Self::PolicyEpochMismatch => "caller policy epoch is not current",
            Self::WrongPersona => "caller persona does not match",
            Self::InsufficientCapability => "caller capability is insufficient",
            Self::InsufficientScope => "caller scope is insufficient",
            Self::ClockRegression => "trusted clock moved backwards during effect admission",
        };
        f.write_str(message)
    }
}

/// Opaque proof that the shared caller checks passed for one requirement.
///
/// The type is linear and constructible only by [`authorize_caller`].
#[derive(Debug, PartialEq, Eq)]
pub struct AuthorizedCaller {
    caller_ref: CallerRef,
    grant: AuthorityGrant,
    policy_epoch: PolicyEpoch,
    persona: Option<PersonaRef>,
}

impl AuthorizedCaller {
    /// Return the authorized caller reference.
    #[must_use]
    pub const fn caller_ref(&self) -> CallerRef {
        self.caller_ref
    }

    /// Return the exact grant checked for this authorization.
    #[must_use]
    pub const fn grant(&self) -> AuthorityGrant {
        self.grant
    }

    /// Return the policy epoch checked for this authorization.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }

    /// Return the checked persona binding.
    #[must_use]
    pub const fn persona(&self) -> Option<PersonaRef> {
        self.persona
    }
}

/// Authorize one caller against a domain requirement at `observed_at`.
///
/// Domains remain responsible for their current capability/scope policy and
/// for durably recording intent before an effect.
///
/// # Errors
///
/// Returns [`AuthorizationDenial`] for every fail-closed condition.
pub fn authorize_caller(
    caller: Option<&ValidatedCaller>,
    requirement: &AuthorizationRequirement,
    observed_at: Timestamp,
) -> Result<AuthorizedCaller, AuthorizationDenial> {
    let caller = caller.ok_or(AuthorizationDenial::MissingContext)?;
    if caller.schema_version != CALLER_CONTEXT_VERSION {
        return Err(AuthorizationDenial::UnknownContextVersion);
    }
    if caller.trust != TrustState::Trusted {
        return Err(AuthorizationDenial::Untrusted);
    }
    if caller.revocation != RevocationState::Active {
        return Err(AuthorizationDenial::Revoked);
    }
    if observed_at < caller.authenticated_at {
        return Err(AuthorizationDenial::NotYetValid);
    }
    if observed_at >= caller.expires_at {
        return Err(AuthorizationDenial::Expired);
    }
    if observed_at >= caller.fresh_until {
        return Err(AuthorizationDenial::Stale);
    }
    if caller.policy_epoch != requirement.policy_epoch {
        return Err(AuthorizationDenial::PolicyEpochMismatch);
    }
    if caller.persona != requirement.persona {
        return Err(AuthorizationDenial::WrongPersona);
    }
    if caller.grant.capability != requirement.grant.capability {
        return Err(AuthorizationDenial::InsufficientCapability);
    }
    if caller.grant.scope != requirement.grant.scope {
        return Err(AuthorizationDenial::InsufficientScope);
    }

    Ok(AuthorizedCaller {
        caller_ref: caller.caller_ref,
        grant: caller.grant,
        policy_epoch: caller.policy_epoch,
        persona: caller.persona,
    })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "fixed test fixtures use checked constructors"
)]
mod tests {
    use super::*;

    #[test]
    fn forged_context_states_fail_closed() {
        let timestamp = |millis| Timestamp::from_unix_millis(millis).expect("test timestamp");
        let grant = AuthorityGrant::new(
            CapabilityRef::from_canonical(b"radio.read"),
            ScopeRef::from_canonical(b"configured-radio"),
        );
        let requirement = AuthorizationRequirement::new(
            grant,
            PolicyEpoch::try_from_u64(1).expect("test epoch"),
            None,
        );
        let base = || ValidatedCaller {
            schema_version: CALLER_CONTEXT_VERSION,
            source: PrincipalSource::LocalOsPeer,
            caller_ref: CallerRef::from_canonical(b"test-caller"),
            grant,
            policy_epoch: PolicyEpoch::try_from_u64(1).expect("test epoch"),
            authenticated_at: timestamp(1_000),
            fresh_until: timestamp(2_000),
            expires_at: timestamp(3_000),
            persona: None,
            trust: TrustState::Trusted,
            revocation: RevocationState::Active,
        };

        let mut unknown = base();
        unknown.schema_version = CALLER_CONTEXT_VERSION + 1;
        assert_eq!(
            authorize_caller(Some(&unknown), &requirement, timestamp(1_500)),
            Err(AuthorizationDenial::UnknownContextVersion),
            "unknown caller versions must fail closed"
        );

        let mut untrusted = base();
        untrusted.trust = TrustState::Untrusted;
        assert_eq!(
            authorize_caller(Some(&untrusted), &requirement, timestamp(1_500)),
            Err(AuthorizationDenial::Untrusted),
            "untrusted contexts must fail closed"
        );

        let mut revoked = base();
        revoked.revocation = RevocationState::Revoked;
        assert_eq!(
            authorize_caller(Some(&revoked), &requirement, timestamp(1_500)),
            Err(AuthorizationDenial::Revoked),
            "revoked contexts must fail closed"
        );
    }
}
