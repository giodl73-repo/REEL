#![cfg(feature = "comic")]
use reel_publication::packet::{decode, encode};
use std::collections::BTreeMap;
#[test]
fn exact_files_roundtrip_and_corrupt_or_truncated_carriers_refuse() {
    let files = BTreeMap::from([
        ("sources/request.json".into(), b"{}".to_vec()),
        (
            "page-reading.png".into(),
            b"opaque synthetic bytes".to_vec(),
        ),
    ]);
    let raw = encode(&files).unwrap();
    assert_eq!(decode(&raw).unwrap(), files);
    assert_eq!(encode(&decode(&raw).unwrap()).unwrap(), raw);
    for size in [0, 7, 11, raw.len() - 1] {
        assert!(decode(&raw[..size]).is_err());
    }
    let mut corrupt = raw.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(decode(&corrupt).is_err());
    let mut trailing = raw;
    trailing.push(0);
    assert!(decode(&trailing).is_err());
}
#[test]
fn unsafe_names_and_namespace_aliases_refuse_before_packing() {
    for path in ["../x", "/x", "a\\x", "CON", "a/NUL.txt", "a.", "a/COM1.bin"] {
        assert!(
            encode(&BTreeMap::from([(path.into(), vec![1])])).is_err(),
            "{path}"
        );
    }
    for names in [["a", "a/b"], ["A.json", "a.json"]] {
        assert!(encode(&names.into_iter().map(|n| (n.into(), vec![1])).collect()).is_err());
    }
}
#[test]
fn noncanonical_duplicate_header_members_and_oversized_lengths_refuse() {
    let object = reel_publication::comic::object(b"x");
    let pin = serde_json::to_string(&object).unwrap();
    let header =
        format!("{{\"schema\":\"reel.editable-packet.v1\",\"files\":{{\"a\":{pin},\"a\":{pin}}}}}");
    let mut raw = b"REELPKT1".to_vec();
    raw.extend_from_slice(&(header.len() as u32).to_le_bytes());
    raw.extend(header.as_bytes());
    raw.push(b'x');
    assert!(decode(&raw).is_err());
    raw[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode(&raw).is_err());
}
