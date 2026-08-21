//! Errors for the shared caller resolver and contract constructors.

use snafu::Snafu;

/// Errors produced while constructing or resolving caller contracts.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum CallerContractError {
    /// A required epoch was zero.
    #[snafu(display("{field} must be non-zero"))]
    ZeroEpoch {
        /// Epoch type whose value was invalid.
        field: &'static str,
        /// Source location of the invalid construction.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// A required authority claim was absent.
    #[snafu(display("authority claim '{field}' is required"))]
    MissingCallerField {
        /// Missing claim name.
        field: &'static str,
        /// Source location of the invalid construction.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Caller validity bounds were empty or non-monotonic.
    #[snafu(display("caller validity must satisfy authenticated_at < fresh_until < expires_at"))]
    InvalidValidityWindow {
        /// Source location of the invalid construction.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Resolver configuration named an unknown version.
    #[snafu(display("unknown caller resolver version {resolver_version}"))]
    UnknownResolverVersion {
        /// Unsupported resolver version.
        resolver_version: u16,
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Authority claims named an unknown caller-context version.
    #[snafu(display("unknown caller context version {schema_version}"))]
    UnknownCallerVersion {
        /// Unsupported caller-context version.
        schema_version: u16,
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The authority did not know the presented identity.
    #[snafu(display("caller identity is unknown"))]
    UnknownIdentity {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The authority could not authenticate the identity.
    #[snafu(display("caller identity is untrusted"))]
    UntrustedIdentity {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The authority reported the identity as revoked.
    #[snafu(display("caller identity is revoked"))]
    RevokedIdentity {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The authority could not make a current decision.
    #[snafu(display("caller authority is unavailable"))]
    AuthorityUnavailable {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Service evidence claimed to authenticate in the future.
    #[snafu(display("service identity evidence is not yet valid"))]
    ServiceEvidenceNotYetValid {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Service evidence and authority claims named different identities.
    #[snafu(display("service identity reference does not match authority claims"))]
    IdentityReferenceMismatch {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Service evidence and claims named different authentication times.
    #[snafu(display("service authentication time does not match authority claims"))]
    ServiceAuthenticationTimeMismatch {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Service claims outlived the accepted authentication evidence.
    #[snafu(display("service caller validity exceeds its authentication evidence"))]
    ServiceEvidenceExpiryMismatch {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Caller claims were used before authentication.
    #[snafu(display("caller context is not yet valid"))]
    NotYetValid {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Caller claims reached the freshness boundary.
    #[snafu(display("caller context is stale"))]
    Stale {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Caller claims reached their hard expiry.
    #[snafu(display("caller context is expired"))]
    Expired {
        /// Source location of the failed resolution.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
