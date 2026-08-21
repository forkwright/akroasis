//! τεκμήριον — conclusive evidence about what the system observed and did.
//!
//! Aristotle separates a *semeion*, a sign that suggests, from a *tekmerion*,
//! a proof that settles. This crate holds the second kind: the tamper-evident
//! log and its keyed hash chain, the authority a caller actually carries, and
//! the receipts an effect leaves behind. Each of them answers "can this be
//! shown?" rather than "what is this?".
//!
//! The distinction is load-bearing rather than decorative. `semaino` produces
//! signals — indications, which may be wrong. Everything here produces records
//! that can be checked against a key, and refuses to report a claim it cannot
//! substantiate.
//!
//! Built on `stoicheion`, which supplies the vocabulary these records are
//! written in. The dependency runs one way: evidence knows what a timestamp
//! and an entity are; the elements know nothing of evidence.

#![deny(missing_docs)]

pub mod caller;
mod caller_error;
mod caller_ref;
pub mod effect_receipt;
mod effect_receipt_state;
pub mod tamper_log;

pub use caller::{
    AuthorityClaims, AuthorityClaimsBuilder, AuthorityDecision, AuthorityGrant,
    AuthorizationDenial, AuthorizationRequirement, AuthorizedCaller, CALLER_CONTEXT_VERSION,
    CALLER_RESOLVER_VERSION, CallerAuthority, CallerContractError, CallerRef, CallerResolver,
    CapabilityRef, EffectRef, EvidenceDigest, LocalPeerCredentials, PersonaRef, PolicyEpoch,
    PrincipalSource, ReceiptDigest, RevocationState, SchemaEpoch, ScopeRef, TrustState,
    ValidatedCaller, authorize_caller,
};
pub use effect_receipt::{
    EFFECT_RECEIPT_VERSION, EffectDescriptor, EffectOutcome, EffectReceipt, EffectReceiptError,
    ReceiptContext, ReceiptEvent, RecoveryRelation, ValidatedReceiptLink,
};
pub use effect_receipt_state::{
    EffectReceiptSink, OutcomeAppendError, PendingIntent, PendingOutcome, PersistedIntent,
    PersistedOutcome, ReceiptTransitionError, RecoveryAuthorization, RecoveryTicket,
};
pub use tamper_log::{
    CHAIN_KEY_LEN, ChainKey, ChainStatus, DEFAULT_MAX_FILE_BYTES, KEY_ID_LEN, LogEntry,
    LogEntryKind, MAX_ENTRY_BYTES, SegmentChainStatus, TIP_SIGNATURE_LEN, TamperLog,
    TamperLogConfig, TamperLogError, TipProvenance, TipSigner, TipStatus, TipVerifier,
    VerificationResult, verify_chain, verify_segment_chain, verify_tip_provenance,
};
