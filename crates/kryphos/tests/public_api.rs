//! Asserts the crate's public surface from a consumer's vantage.

/// The crate root re-exported a `SALT_LEN` of 16 while key derivation used 32,
/// because two modules defined that name at different values and the root
/// re-exported the dead one (forkwright/akroasis#380).
///
/// WHY this test rather than a convention: both spellings type-check and both
/// compile, so nothing inside the crate notices. The failure surfaces only in
/// a consumer, at runtime, as `InvalidSaltLength { expected: 32, actual: 16 }`
/// — an error that names the right value without naming where the wrong one
/// came from. A test written from outside the crate is the only vantage that
/// sees what a consumer sees.
#[test]
fn the_root_salt_len_is_the_length_key_derivation_actually_requires() {
    let generated = kryphos::generate_salt();

    assert_eq!(
        generated.len(),
        kryphos::SALT_LEN,
        "kryphos::SALT_LEN must describe the salt kryphos::generate_salt produces"
    );

    // Dispositive rather than merely consistent: derive_key is what rejects a
    // wrong-length salt, so a salt sized by the public constant must be one it
    // accepts. Two constants agreeing with each other proves nothing if both
    // disagree with the function that validates.
    let derived = kryphos::derive_key(
        b"correct horse battery staple",
        &generated,
        &kryphos::KdfParams::default(),
    );
    assert!(
        derived.is_ok(),
        "a salt sized by the public SALT_LEN must be accepted by the public \
         derive_key, got {derived:?}"
    );
}
