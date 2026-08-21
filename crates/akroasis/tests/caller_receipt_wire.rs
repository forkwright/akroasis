//! Wire-shape fixture for minimized effect receipts.

#![expect(
    clippy::expect_used,
    reason = "fixed wire fixtures use checked constructors and serialization"
)]

use std::collections::BTreeSet;

use akroasis_lib::caller::{
    AdmissionDecision, ApplicationCallerResolver, EffectExecution, EffectRequest, TrustedClock,
    execute_effect,
};
use koinon::{
    AuthorityClaims, AuthorityDecision, AuthorityGrant, AuthorizationRequirement,
    CALLER_CONTEXT_VERSION, CallerAuthority, CallerRef, CapabilityRef, EffectDescriptor,
    EffectReceipt, EffectReceiptError, EffectReceiptSink, EffectRef, EvidenceDigest,
    LocalPeerCredentials, PolicyEpoch, ReceiptDigest, RecoveryRelation, SchemaEpoch, ScopeRef,
    Timestamp,
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
struct WireAuthority {
    caller_ref: CallerRef,
    grant: AuthorityGrant,
    policy_epoch: PolicyEpoch,
}

impl CallerAuthority for WireAuthority {
    fn resolve_local(
        &self,
        _peer: LocalPeerCredentials,
        _observed_at: Timestamp,
    ) -> AuthorityDecision {
        let claims = AuthorityClaims::builder()
            .schema_version(CALLER_CONTEXT_VERSION)
            .caller_ref(self.caller_ref)
            .grant(self.grant)
            .policy_epoch(self.policy_epoch)
            .validity(timestamp(1_000), timestamp(2_000), timestamp(3_000))
            .build()
            .expect("valid wire authority claims");
        AuthorityDecision::Granted(claims)
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
enum WireSinkError {
    #[snafu(display("receipt predecessor is not present in the wire ledger"))]
    UnknownPredecessor {
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("wire receipt transition violates the shared contract: {source}"))]
    InvalidTransition {
        source: EffectReceiptError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

#[derive(Default)]
struct WireSink {
    receipts: Vec<EffectReceipt>,
    digests: Vec<ReceiptDigest>,
}

impl EffectReceiptSink for WireSink {
    type Error = WireSinkError;

    fn append(&mut self, receipt: &EffectReceipt) -> Result<ReceiptDigest, Self::Error> {
        if let Some(predecessor) = receipt.predecessor() {
            let parent = self
                .digests
                .iter()
                .zip(&self.receipts)
                .find_map(|(digest, receipt)| (*digest == predecessor).then_some(receipt))
                .ok_or(WireSinkError::UnknownPredecessor {
                    location: snafu::location!(),
                })?;
            let _link = receipt
                .validate_child_of(parent, predecessor)
                .map_err(|source| WireSinkError::InvalidTransition {
                    source,
                    location: snafu::location!(),
                })?;
        }
        let encoded = serde_json::to_vec(receipt).expect("serialize wire receipt");
        let mut digest_material = b"caller-wire-test-ledger\0".to_vec();
        digest_material.extend_from_slice(
            &u64::try_from(self.receipts.len())
                .expect("test ledger position fits u64")
                .to_le_bytes(),
        );
        digest_material.extend_from_slice(&encoded);
        let digest = ReceiptDigest::from_canonical(&digest_material);
        self.receipts.push(receipt.clone());
        self.digests.push(digest);
        Ok(digest)
    }
}

fn timestamp(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis).expect("valid fixed timestamp")
}

fn policy_epoch(value: u64) -> PolicyEpoch {
    PolicyEpoch::try_from_u64(value).expect("non-zero policy epoch")
}

fn schema_epoch(value: u64) -> SchemaEpoch {
    SchemaEpoch::try_from_u64(value).expect("non-zero schema epoch")
}

#[tokio::test(flavor = "current_thread")]
async fn receipt_wire_shape_is_closed_and_contains_no_raw_values() {
    let raw_identity: &[u8] = b"operator@example.invalid";
    let raw_capability: &[u8] = b"146520000";
    let raw_scope: &[u8] = b"allow:uid:1000";
    let raw_effect: &[u8] = b"/dev/ttyUSB0";
    let raw_evidence: &[u8] = b"0123456789abcdef0123456789abcdef";
    let grant = AuthorityGrant::new(
        CapabilityRef::from_canonical(raw_capability),
        ScopeRef::from_canonical(raw_scope),
    );
    let resolver = ApplicationCallerResolver::current(WireAuthority {
        caller_ref: CallerRef::from_canonical(raw_identity),
        grant,
        policy_epoch: policy_epoch(7),
    })
    .expect("current resolver");
    let (client, _server) = UnixStream::pair().expect("Unix socket pair");
    let caller = resolver
        .resolve_local(&client, &FixedClock(timestamp(1_500)))
        .expect("valid local caller");
    let mut sink = WireSink::default();
    execute_effect(
        Some(&caller),
        EffectRequest::new(
            AuthorizationRequirement::new(grant, policy_epoch(7), None),
            EffectDescriptor::new(
                EffectRef::from_canonical(raw_effect),
                schema_epoch(3),
                Some(EvidenceDigest::from_canonical(raw_evidence)),
            ),
        ),
        &FixedClock(timestamp(1_500)),
        AdmissionDecision::Ready,
        &mut sink,
        || EffectExecution::succeeded(None),
    )
    .expect("effect should create a receipt");

    let intent = sink.receipts.first().expect("intent receipt");
    assert_minimized_wire(
        intent,
        &[
            raw_identity,
            raw_capability,
            raw_scope,
            raw_effect,
            raw_evidence,
        ],
    );
    assert_receipt_field_allowlist(intent);
    assert_invalid_wire_shapes_fail(intent, sink.receipts.last().expect("outcome receipt"));
}

fn assert_minimized_wire(intent: &EffectReceipt, forbidden_values: &[&[u8]]) {
    let encoded = serde_json::to_string(intent).expect("serialize intent");
    for forbidden in forbidden_values {
        let forbidden = std::str::from_utf8(forbidden).expect("ASCII fixture");
        assert!(
            !encoded.contains(forbidden),
            "raw value must not be representable in a receipt: {forbidden}"
        );
    }
}

fn assert_receipt_field_allowlist(intent: &EffectReceipt) {
    let mut value = serde_json::to_value(intent).expect("serialize intent value");
    let object = value.as_object_mut().expect("receipt object");
    let keys: BTreeSet<_> = object.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "context",
            "event",
            "evidence_digest",
            "observed_at",
            "predecessor",
            "recovery",
            "schema_version",
        ]),
        "receipt fields must stay allowlisted"
    );
    let context = object
        .get("context")
        .and_then(serde_json::Value::as_object)
        .expect("receipt context");
    let context_keys: BTreeSet<_> = context.keys().map(String::as_str).collect();
    assert_eq!(
        context_keys,
        BTreeSet::from([
            "caller_ref",
            "capability",
            "effect",
            "policy_epoch",
            "schema_epoch",
        ]),
        "receipt context fields must stay allowlisted"
    );
}

fn assert_invalid_wire_shapes_fail(intent: &EffectReceipt, outcome: &EffectReceipt) {
    assert_receipt_round_trip(intent);
    assert_receipt_round_trip(outcome);

    let mut value = serde_json::to_value(intent).expect("serialize intent value");
    let object = value.as_object_mut().expect("receipt object");
    assert!(
        object
            .insert("raw_path".to_owned(), serde_json::json!("/dev/ttyUSB0"))
            .is_none(),
        "raw path fixture field must be new"
    );
    assert_wire_rejection(
        value,
        "unknown field `raw_path`",
        "unknown top-level fields must fail closed",
    );

    let mut nested_unknown = serde_json::to_value(intent).expect("serialize intent value");
    let context = nested_unknown
        .as_object_mut()
        .and_then(|object| object.get_mut("context"))
        .and_then(serde_json::Value::as_object_mut)
        .expect("receipt context");
    assert!(
        context
            .insert("raw_rule".to_owned(), serde_json::json!("allow all"))
            .is_none(),
        "raw rule fixture field must be new"
    );
    assert_wire_rejection(
        nested_unknown,
        "unknown field `raw_rule`",
        "unknown context fields must fail closed",
    );

    let mut unknown_version = serde_json::to_value(intent).expect("serialize intent value");
    assert!(
        unknown_version
            .as_object_mut()
            .expect("receipt object")
            .insert("schema_version".to_owned(), serde_json::json!(99))
            .is_some(),
        "schema version fixture field must exist"
    );
    assert_wire_rejection(
        unknown_version,
        "unknown effect-receipt schema version 99",
        "unknown receipt versions must fail closed",
    );

    let mut invalid_recovery = serde_json::to_value(outcome).expect("serialize outcome value");
    let recovery = serde_json::to_value(RecoveryRelation::RecoveryOf(EffectRef::from_canonical(
        b"unrelated-effect",
    )))
    .expect("serialize recovery relation");
    assert!(
        invalid_recovery
            .as_object_mut()
            .expect("receipt object")
            .insert("recovery".to_owned(), recovery)
            .is_some(),
        "recovery fixture field must exist"
    );
    assert_wire_rejection(
        invalid_recovery,
        "effect outcome has an incompatible recovery relation",
        "successful outcomes cannot claim an unrelated recovery relation",
    );
}

fn assert_receipt_round_trip(receipt: &EffectReceipt) {
    let value = serde_json::to_value(receipt).expect("serialize valid receipt fixture");
    let decoded =
        serde_json::from_value::<EffectReceipt>(value).expect("valid receipt must deserialize");
    assert_eq!(&decoded, receipt, "receipt wire round-trip must be exact");
}

fn assert_wire_rejection(value: serde_json::Value, expected: &str, message: &str) {
    let error = serde_json::from_value::<EffectReceipt>(value).expect_err(message);
    assert!(
        error.to_string().contains(expected),
        "{message}: expected {expected:?}, got {error}"
    );
}
