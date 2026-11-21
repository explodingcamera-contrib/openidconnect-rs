use crate::core::{CoreGrantType, CoreJwsSigningAlgorithm};
use crate::helpers::serialize_identifier;

#[test]
fn test_grant_type_serialize() {
    let serialized_implicit = serde_json::to_string(&CoreGrantType::Implicit).unwrap();
    assert_eq!("\"implicit\"", serialized_implicit);
    assert_eq!(
        CoreGrantType::Implicit,
        serde_json::from_str::<CoreGrantType>(&serialized_implicit).unwrap()
    );
}

#[test]
fn test_signature_algorithm_name() {
    assert_eq!(
        serialize_identifier(&CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256).unwrap(),
        "RS256"
    );
    assert_eq!(
        serde_json::from_str::<CoreJwsSigningAlgorithm>(r#""RS256""#).unwrap(),
        CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256
    );
}
