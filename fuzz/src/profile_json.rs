//! Bounded typed profile parsing and exact deterministic JSON round-trip checks.
use karasu_core::profiles::store::ProfileDocument;

/// Exercise at most 64 KiB of untrusted profile JSON without filesystem access.
///
/// A successful decode must validate, serialize deterministically, reload without changing any
/// physical value, and serialize identically again. Malformed/schema/range failures are expected.
pub fn check_profile_json(data: &[u8]) -> bool {
    if data.len() > crate::MAX_INPUT_BYTES {
        return false;
    }
    let Ok(profile) = ProfileDocument::from_json(data) else {
        return false;
    };
    profile.validate().expect("decoded profile must validate");
    let first = profile
        .to_json()
        .expect("bounded validated profile serializes");
    assert_eq!(
        first,
        profile.to_json().expect("deterministic serialization")
    );
    let reloaded = ProfileDocument::from_json(&first).expect("serialized profile reloads");
    assert_eq!(profile, reloaded);
    assert_eq!(
        first,
        reloaded.to_json().expect("stable second serialization")
    );
    true
}
