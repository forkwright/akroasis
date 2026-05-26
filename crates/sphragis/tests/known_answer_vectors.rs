//! Known-answer-test acceptance gate for `sphragis` (preview-pq).
//!
//! Proves the construction matches the published standards:
//! - X-Wing draft KAT (full hybrid encaps→decaps→shared secret).
//! - RFC 5869 HKDF-SHA256.
//! - RFC 7748 X25519.
//! - Round-trip + negative tests (wrong recipient, wrong version, tamper).
//!
//! ChaCha20-Poly1305 (RFC 8439) and ML-KEM-768 (FIPS-203 ACVP) primitive vectors
//! are covered upstream in `kryphos` and the `ml-kem` crate respectively; this
//! gate validates the *hybrid composition* and *envelope*.

#![cfg(feature = "preview-pq")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use hex_literal::hex;

use sphragis::envelope::derive_wrap_key;
use sphragis::hybrid::{DecapsulationKey, EncapsulationKey, HybridKem};
use sphragis::seal::{CONTENT_KEY_LEN, seal_for, unseal, WrappedContentKey};
use sphragis::SEAL_VERSION_V1;

// ---------------------------------------------------------------------------
// X-Wing draft known-answer vector (RustCrypto x-wing test-vectors.json, [0]).
// draft-connolly-cfrg-xwing-kem. seed -> keypair; eseed -> deterministic encaps.
// ---------------------------------------------------------------------------

/// X-Wing KAT: deterministic encapsulation reproduces the published shared
/// secret, and decapsulation recovers it.
#[test]
fn xwing_draft_kat_vector_0() {
    let seed = hex!("7f9c2ba4e88f827d616045507605853ed73b8093f6efbc88eb1a6eacfa66ef26");
    let eseed = hex!(
        "3cb1eea988004b93103cfb0aeefd2a686e01fa4a58e8a3639ca8a1e3f9ae57e2"
        "35b8cc873c23dc62b8d260169afa2f75ab916a58d974918835d25e6a435085b2"
    );
    let expected_ss =
        hex!("d2df0522128f09dd8e2c92b1e905c793d8f57a54c3da25861f10bf4ca613e384");

    let dk = DecapsulationKey::from_seed(seed);
    let ek = dk.encapsulation_key();

    let (ct, ss_send) = ek.encapsulate_deterministic(&eseed);
    assert_eq!(
        ss_send.as_slice(),
        &expected_ss,
        "X-Wing deterministic encaps must reproduce the draft KAT shared secret"
    );

    let ss_recv = dk.decapsulate(&ct).unwrap();
    assert_eq!(
        ss_recv.as_slice(),
        &expected_ss,
        "X-Wing decaps must recover the draft KAT shared secret"
    );
}

// ---------------------------------------------------------------------------
// RFC 5869 HKDF-SHA256 — Test Case 1.
// We drive it through derive_wrap_key with the matching salt/info would require
// a configurable salt, so we test the primitive path the envelope relies on
// directly via the hkdf crate to prove the dependency is correct, then assert
// derive_wrap_key is deterministic and domain-separated.
// ---------------------------------------------------------------------------

/// RFC 5869 Appendix A.1 (HKDF-SHA256, Test Case 1) against the `hkdf` crate.
#[test]
fn hkdf_sha256_rfc5869_case_1() {
    use hkdf::Hkdf;
    use sha2::Sha256;

    let ikm = hex!("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
    let salt = hex!("000102030405060708090a0b0c");
    let info = hex!("f0f1f2f3f4f5f6f7f8f9");
    let expected_okm = hex!(
        "3cb25f25faacd57a90434f64d0362f2a"
        "2d2d0a90cf1a5a4c5db02d56ecc4c5bf"
        "34007208d5b887185865"
    );

    let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut okm = [0u8; 42];
    hk.expand(&info, &mut okm).unwrap();
    assert_eq!(
        okm.as_slice(),
        &expected_okm,
        "HKDF-SHA256 must match RFC 5869 Test Case 1"
    );
}

/// The envelope wrap-key derivation is deterministic and domain-separated.
#[test]
fn derive_wrap_key_is_deterministic_and_domain_separated() {
    let ss = [0x42u8; 32];
    let a = derive_wrap_key(&ss, b"akroasis-sphragis-ck-wrap-v1").unwrap();
    let b = derive_wrap_key(&ss, b"akroasis-sphragis-ck-wrap-v1").unwrap();
    let c = derive_wrap_key(&ss, b"akroasis-sphragis-ck-wrap-v2").unwrap();
    assert_eq!(a.as_slice(), b.as_slice(), "same inputs -> same key");
    assert_ne!(
        a.as_slice(),
        c.as_slice(),
        "different domain tag -> different key"
    );
}

// ---------------------------------------------------------------------------
// RFC 7748 Section 5.2 — X25519 known-answer vector (first vector).
// Validates the x25519-dalek dependency the hybrid relies on.
// ---------------------------------------------------------------------------

/// RFC 7748 Section 5.2 X25519 first test vector.
#[test]
fn x25519_rfc7748_vector_1() {
    use x25519_dalek::{PublicKey, StaticSecret};

    let scalar = hex!("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4");
    let u_coord = hex!("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c");
    let expected = hex!("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552");

    let sk = StaticSecret::from(scalar);
    let peer = PublicKey::from(u_coord);
    let shared = sk.diffie_hellman(&peer);
    assert_eq!(
        shared.as_bytes(),
        &expected,
        "X25519 must match RFC 7748 Section 5.2 vector 1"
    );
}

// ---------------------------------------------------------------------------
// End-to-end sealing: round-trip, multi-recipient, revocation, negatives.
// ---------------------------------------------------------------------------

fn fresh() -> (DecapsulationKey, EncapsulationKey) {
    HybridKem::generate()
}

/// A content key seals and unseals through a single recipient.
#[test]
fn seal_unseal_round_trip() {
    let (dk, ek) = fresh();
    let content_key = [0xACu8; CONTENT_KEY_LEN];

    let wrapped = seal_for(&content_key, &[ek]).unwrap();
    assert_eq!(wrapped.len(), 1);

    let recovered = unseal(&dk, &wrapped[0]).unwrap();
    assert_eq!(recovered.as_slice(), &content_key);
}

/// One content key wraps for several devices; each recovers the same key.
#[test]
fn multi_recipient_all_recover_same_key() {
    let (dk1, ek1) = fresh();
    let (dk2, ek2) = fresh();
    let (dk3, ek3) = fresh();
    let content_key = [0x11u8; CONTENT_KEY_LEN];

    let wrapped = seal_for(&content_key, &[ek1, ek2, ek3]).unwrap();
    assert_eq!(wrapped.len(), 3);

    assert_eq!(unseal(&dk1, &wrapped[0]).unwrap().as_slice(), &content_key);
    assert_eq!(unseal(&dk2, &wrapped[1]).unwrap().as_slice(), &content_key);
    assert_eq!(unseal(&dk3, &wrapped[2]).unwrap().as_slice(), &content_key);
}

/// Revocation: re-sealing for the remaining recipients excludes the revoked one.
#[test]
fn revocation_excludes_device() {
    let (dk1, ek1) = fresh();
    let (dk2, ek2) = fresh();
    let content_key = [0x22u8; CONTENT_KEY_LEN];

    // Revoke device 2: re-seal for device 1 only.
    let rewrapped = seal_for(&content_key, &[ek1]).unwrap();
    assert_eq!(rewrapped.len(), 1);
    assert_eq!(unseal(&dk1, &rewrapped[0]).unwrap().as_slice(), &content_key);

    // Device 2 has no wrap addressed to it.
    let _ = (dk2, ek2);
}

/// A wrap for one device cannot be opened by another (wrong recipient).
#[test]
fn wrong_recipient_fails() {
    let (_dk1, ek1) = fresh();
    let (dk2, _ek2) = fresh();
    let content_key = [0x33u8; CONTENT_KEY_LEN];

    let wrapped = seal_for(&content_key, &[ek1]).unwrap();
    assert!(
        unseal(&dk2, &wrapped[0]).is_err(),
        "a different device must not unseal another device's wrap"
    );
}

/// An unknown version is rejected, not reinterpreted.
#[test]
fn unsupported_version_rejected() {
    let (dk, ek) = fresh();
    let content_key = [0x44u8; CONTENT_KEY_LEN];
    let mut wrapped = seal_for(&content_key, &[ek]).unwrap()[0].clone();
    wrapped.version = SEAL_VERSION_V1 + 7;
    assert!(unseal(&dk, &wrapped).is_err(), "unknown version must fail");
}

/// A corrupted sealed key fails the AEAD tag check.
#[test]
fn tampered_sealed_key_fails() {
    let (dk, ek) = fresh();
    let content_key = [0x55u8; CONTENT_KEY_LEN];
    let mut wrapped = seal_for(&content_key, &[ek]).unwrap()[0].clone();
    let last = wrapped.sealed_key.len() - 1;
    wrapped.sealed_key[last] ^= 0xFF;
    assert!(
        unseal(&dk, &wrapped).is_err(),
        "tampered sealed key must fail the AEAD tag"
    );
}

/// A corrupted KEM ciphertext yields a different shared secret and fails open.
#[test]
fn corrupted_kem_ciphertext_fails() {
    let (dk, ek) = fresh();
    let content_key = [0x66u8; CONTENT_KEY_LEN];
    let mut wrapped = seal_for(&content_key, &[ek]).unwrap()[0].clone();
    wrapped.kem_ciphertext[0] ^= 0xFF;
    assert!(
        unseal(&dk, &wrapped).is_err(),
        "corrupted KEM ciphertext must not recover the content key"
    );
}

/// The wrapped key round-trips through CBOR.
#[test]
fn cbor_round_trip() {
    let (dk, ek) = fresh();
    let content_key = [0x77u8; CONTENT_KEY_LEN];
    let wrapped = seal_for(&content_key, &[ek]).unwrap();

    let bytes = wrapped[0].to_cbor().unwrap();
    let decoded = WrappedContentKey::from_cbor(&bytes).unwrap();
    assert_eq!(unseal(&dk, &decoded).unwrap().as_slice(), &content_key);
}
