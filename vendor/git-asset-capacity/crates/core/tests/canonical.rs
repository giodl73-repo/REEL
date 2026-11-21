use git_asset_core::identity::{Digest, canonical_json, parse_json};
use serde_json::json;

#[test]
fn canonical_vector_sorts_keys_and_preserves_arrays() {
    let input = br#"{ "z": [3,1], "a": {"b":true,"a":null} }"#;
    assert_eq!(
        canonical_json(&parse_json(input).unwrap()).unwrap(),
        br#"{"a":{"a":null,"b":true},"z":[3,1]}"#
    );
}

#[test]
fn equivalent_escapes_and_layout_have_one_digest() {
    let a = parse_json(br#"{"x":"\u0061","z":0}"#).unwrap();
    let b = parse_json(br#"{ "z": 0, "x": "a" }"#).unwrap();
    assert_eq!(
        Digest::semantic("revision", &a).unwrap(),
        Digest::semantic("revision", &b).unwrap()
    );
}

#[test]
fn unicode_is_not_normalized() {
    assert_ne!(
        Digest::semantic("text", &json!("é")).unwrap(),
        Digest::semantic("text", &json!("e\u{301}")).unwrap()
    );
}

#[test]
fn duplicate_keys_are_rejected_at_every_depth() {
    for input in [r#"{"a":1,"a":2}"#, r#"{"a":[{"x":1,"\u0078":1}]}"#] {
        assert!(parse_json(input.as_bytes()).is_err(), "{input}");
    }
}

#[test]
fn unsupported_numbers_are_rejected() {
    for input in [
        "1.0",
        "1e0",
        "-0",
        "-0.0",
        "9007199254740992",
        "-9007199254740992",
        "18446744073709551616",
    ] {
        assert!(parse_json(input.as_bytes()).is_err(), "{input}");
    }
    assert!(canonical_json(&json!(1.0)).is_err());
}

#[test]
fn safe_integer_boundaries_are_accepted() {
    for input in ["0", "9007199254740991", "-9007199254740991"] {
        assert_eq!(
            canonical_json(&parse_json(input.as_bytes()).unwrap()).unwrap(),
            input.as_bytes()
        );
    }
}

#[test]
fn byte_depth_and_node_budgets_are_enforced() {
    assert!(parse_json(&vec![b' '; 65_537]).is_err());
    let deep = format!("{}0{}", "[".repeat(33), "]".repeat(33));
    assert!(parse_json(deep.as_bytes()).is_err());
    assert!(canonical_json(&json!(vec![0; 4096])).is_err());
    assert!(canonical_json(&json!("a".repeat(65_536))).is_err());
}

#[test]
fn malformed_utf8_trailing_data_and_bom_are_rejected() {
    for input in [&b"\xff"[..], b"{}{}", b"\xef\xbb\xbf{}", br#""\ud800""#] {
        assert!(parse_json(input).is_err());
    }
}

#[test]
fn hash_domains_and_exact_blob_identity_are_separate() {
    let value = json!({"a":1});
    assert_ne!(
        Digest::semantic("revision", &value).unwrap(),
        Digest::semantic("instance", &value).unwrap()
    );
    assert_ne!(
        Digest::semantic("revision", &value).unwrap(),
        Digest::blob(&canonical_json(&value).unwrap())
    );
    // Independently generated with PowerShell SHA256 and big-endian framing.
    assert_eq!(
        Digest::semantic("revision", &value).unwrap().as_str(),
        "70ef914ace609bf02f6e220bc9e990bf403a553a4a85d6d56380da4948e0bef1"
    );
    assert_eq!(
        Digest::blob(b"abc").as_str(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert!(Digest::semantic("", &value).is_err());
}

#[test]
fn digest_wire_format_is_strict() {
    assert!(Digest::parse(&"A".repeat(64)).is_err());
    assert!(Digest::parse(&"f".repeat(63)).is_err());
    assert!(Digest::parse(&"g".repeat(64)).is_err());
    assert!(Digest::parse(&"0".repeat(64)).is_ok());
}
