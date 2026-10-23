use reel_assets::inventory_evidence::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Write};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn blob(bytes: &[u8]) -> Blob {
    Blob {
        sha256: hash(bytes),
        bytes: bytes.len() as u64,
    }
}
fn report() -> Value {
    json!({"schema":SOURCE_SCHEMA,"project_id":"synthetic","registry_generation":3,
        "profile_sha256":hash(b"profile"),"scope":"registered only","coverage_complete":true,
        "census_started_unix_seconds":10,"census_completed_unix_seconds":11,
        "payload_hashes_verified":false,"tiers":[{"tier":0,"root":"SYNTHETIC_PRIVATE_PATH","volume_id":"synthetic-volume"}],
        "objects":[{"sha256":hash(b"payload"),"registered":true,"bytes":7,"copies":[{"tier":0,"bytes":7,"physical_id":"synthetic-file","allocated_bytes":4096,"hard_links":2,"descriptor_size_matches":true}]}]})
}
fn source(raw: &[u8]) -> Source {
    Source {
        encoded: blob(raw),
        decoded: blob(raw),
        encoding: Encoding::Identity,
        schema: SOURCE_SCHEMA.into(),
        project_id: "synthetic".into(),
        authority_namespace: "synthetic-authority".into(),
        adapter_version: ADAPTER_VERSION,
    }
}
fn parse(v: &Value) -> PreparedInventory {
    let raw = serde_json::to_vec(v).unwrap();
    prepare(raw.as_slice(), &source(&raw), Limits::default()).unwrap()
}
fn rejected(v: &Value) -> bool {
    let raw = serde_json::to_vec(v).unwrap();
    prepare(raw.as_slice(), &source(&raw), Limits::default()).is_err()
}
fn gzip(raw: &[u8]) -> Vec<u8> {
    let mut gz = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
    gz.write_all(raw).unwrap();
    gz.finish().unwrap()
}

#[test]
fn preserves_exact_rows_locations_capture_and_source_record_witness() {
    let v = report();
    let index = parse(&v);
    let id = hash(b"payload");
    let Lookup::Listed(row) = index.lookup(&id).unwrap() else {
        panic!()
    };
    assert_eq!(row.bytes, Some(7));
    assert_eq!(row.copies[0].hard_links, Some(2));
    assert_eq!(row.copies[0].physical_id.as_deref(), Some("synthetic-file"));
    assert_eq!(index.tier(0).unwrap().root, "SYNTHETIC_PRIVATE_PATH");
    assert_eq!(index.capture().unwrap().completed, 11);
    assert_eq!(index.registry_generation(), 3);
    assert_eq!(index.profile_sha256(), hash(b"profile"));
    let witness = index.witness(&id).unwrap().unwrap();
    assert_eq!(witness.json_pointer, "/objects/0");
    assert!(witness.source.encoded == blob(&serde_json::to_vec(&v).unwrap()));
    assert_eq!(index.producer_dependency_coverage(), Some(true));
}
#[test]
fn missing_subject_is_only_not_listed_at_capture() {
    let index = parse(&report());
    assert!(matches!(
        index.lookup(&hash(b"absent")).unwrap(),
        Lookup::NotListedAtCapture
    ));
    assert!(index.witness(&hash(b"absent")).unwrap().is_none());
    assert!(index.lookup("अ").is_err());
}
#[test]
fn named_observation_scope_does_not_invent_capture_times() {
    let mut v = report();
    let map = v.as_object_mut().unwrap();
    for key in [
        "scope",
        "coverage_complete",
        "census_started_unix_seconds",
        "census_completed_unix_seconds",
    ] {
        map.remove(key);
    }
    map.insert(
        "inventory_scope".into(),
        json!("exact named registered hashes only; not a full cache inventory"),
    );
    let index = parse(&v);
    assert!(index.capture().is_none());
    assert!(index.scope().is_none());
    assert!(index.named_scope().unwrap().contains("not a full"));
    assert_eq!(index.producer_dependency_coverage(), None);
}
#[test]
fn null_size_and_unmeasured_physical_metadata_remain_unknown() {
    let mut v = report();
    v["objects"][0]["registered"] = json!(false);
    v["objects"][0]["bytes"] = Value::Null;
    v["objects"][0]["copies"] = json!([{"tier":0,"bytes":8,"physical_identity_measured":false,"allocated_bytes":null,"hard_links":null}]);
    let index = parse(&v);
    let Lookup::Listed(row) = index.lookup(&hash(b"payload")).unwrap() else {
        panic!()
    };
    assert_eq!(row.bytes, None);
    assert_eq!(row.copies[0].physical_id, None);
    assert_eq!(row.copies[0].allocated_bytes, None);
}
#[test]
fn wrong_size_observation_is_preserved_as_a_mismatch_not_promoted_to_verification() {
    let mut v = report();
    v["objects"][0]["copies"][0]["bytes"] = json!(9);
    v["objects"][0]["copies"][0]["descriptor_size_matches"] = json!(false);
    let index = parse(&v);
    let Lookup::Listed(row) = index.lookup(&hash(b"payload")).unwrap() else {
        panic!()
    };
    assert_eq!(row.copies[0].descriptor_size_matches, Some(false));
    assert_eq!(row.copies[0].bytes, 9);
}
#[test]
fn aggregate_producer_flags_do_not_change_the_metadata_only_adapter() {
    let mut v = report();
    v["payload_hashes_verified"] = json!(true);
    v["removal_authorized"] = json!(true);
    let index = parse(&v);
    assert_eq!(index.summary().copies, 1);
    // Public observations have no verified-custody or removal-authorized field.
    assert_eq!(
        serde_json::to_value(index.summary()).unwrap(),
        json!({"adapter":ADAPTER_ID,"objects":1,"copies":1})
    );
}
#[test]
fn private_paths_subject_hashes_and_authority_are_absent_from_public_summary() {
    let index = parse(&report());
    let summary = serde_json::to_string(&index.summary()).unwrap();
    for value in [
        "SYNTHETIC_PRIVATE_PATH",
        "synthetic-authority",
        &hash(b"payload"),
        &index.source().encoded.sha256,
    ] {
        assert!(!summary.contains(value));
    }
}
#[test]
fn source_bytes_and_decoded_identity_are_both_verified() {
    let raw = serde_json::to_vec(&report()).unwrap();
    let mut s = source(&raw);
    s.encoded.sha256 = hash(b"other");
    assert!(prepare(raw.as_slice(), &s, Limits::default()).is_err());
    s = source(&raw);
    s.decoded.sha256 = hash(b"other");
    assert!(prepare(raw.as_slice(), &s, Limits::default()).is_err());
}
#[test]
fn explicit_gzip_and_identity_produce_equivalent_observations() {
    let raw = serde_json::to_vec(&report()).unwrap();
    let packed = gzip(&raw);
    let mut s = source(&raw);
    s.encoding = Encoding::Gzip;
    s.encoded = blob(&packed);
    let gz = prepare(packed.as_slice(), &s, Limits::default()).unwrap();
    let plain = parse(&report());
    assert_eq!(
        serde_json::to_value(gz.summary()).unwrap(),
        serde_json::to_value(plain.summary()).unwrap()
    );
    assert!(gz.source().encoded != gz.source().decoded);
}
#[test]
fn gzip_expansion_past_declared_decoded_length_is_rejected() {
    let raw = serde_json::to_vec(&report()).unwrap();
    let mut larger = raw.clone();
    larger.extend_from_slice(&[b' '; 4096]);
    let packed = gzip(&larger);
    let mut s = source(&raw);
    s.encoding = Encoding::Gzip;
    s.encoded = blob(&packed);
    assert!(prepare(packed.as_slice(), &s, Limits::default()).is_err());
}
#[test]
fn gzip_truncation_crc_and_trailing_garbage_cannot_yield_a_partial_index() {
    let raw = serde_json::to_vec(&report()).unwrap();
    for variant in 0..3 {
        let mut packed = gzip(&raw);
        match variant {
            0 => {
                packed.truncate(packed.len() - 5);
            }
            1 => {
                let n = packed.len();
                packed[n - 8] ^= 1;
            }
            _ => packed.extend_from_slice(b"garbage"),
        }
        let mut s = source(&raw);
        s.encoding = Encoding::Gzip;
        s.encoded = blob(&packed);
        assert!(prepare(packed.as_slice(), &s, Limits::default()).is_err());
    }
}
#[test]
fn multi_member_gzip_consumes_and_checks_the_complete_decoded_stream() {
    let raw = serde_json::to_vec(&report()).unwrap();
    let split = raw.len() / 2;
    let packed = [gzip(&raw[..split]), gzip(&raw[split..])].concat();
    let mut s = source(&raw);
    s.encoding = Encoding::Gzip;
    s.encoded = blob(&packed);
    assert_eq!(
        prepare(packed.as_slice(), &s, Limits::default())
            .unwrap()
            .summary()
            .objects,
        1
    );
}
#[test]
fn object_budget_rejects_before_deserializing_the_next_object() {
    let raw = serde_json::to_vec(&report()).unwrap();
    let limits = Limits {
        objects: 0,
        ..Limits::default()
    };
    let error = match prepare(raw.as_slice(), &source(&raw), limits) {
        Err(e) => e.to_string(),
        Ok(_) => panic!(),
    };
    assert!(error.contains("before next record decode"));
}
#[test]
fn encoded_decoded_and_total_copy_limits_are_enforced() {
    let raw = serde_json::to_vec(&report()).unwrap();
    let s = source(&raw);
    for limits in [
        Limits {
            encoded_bytes: 1,
            ..Limits::default()
        },
        Limits {
            decoded_bytes: 1,
            ..Limits::default()
        },
        Limits {
            total_copies: 0,
            ..Limits::default()
        },
    ] {
        assert!(prepare(raw.as_slice(), &s, limits).is_err());
    }
}
#[test]
fn duplicate_subjects_tiers_and_copy_locations_are_rejected() {
    let mut v = report();
    let row = v["objects"][0].clone();
    v["objects"].as_array_mut().unwrap().push(row);
    assert!(rejected(&v));
    v = report();
    let tier = v["tiers"][0].clone();
    v["tiers"].as_array_mut().unwrap().push(tier);
    assert!(rejected(&v));
    v = report();
    let copy = v["objects"][0]["copies"][0].clone();
    v["objects"][0]["copies"].as_array_mut().unwrap().push(copy);
    assert!(rejected(&v));
}
#[test]
fn malformed_subject_unknown_tier_and_inconsistent_identity_are_rejected() {
    let mut v = report();
    v["objects"][0]["sha256"] = json!("अ");
    assert!(rejected(&v));
    v = report();
    v["objects"][0]["copies"][0]["tier"] = json!(99);
    assert!(rejected(&v));
    v = report();
    v["objects"][0]["copies"][0]["physical_identity_measured"] = json!(false);
    assert!(rejected(&v));
    v = report();
    v["objects"][0]["copies"][0]["descriptor_size_matches"] = json!(false);
    assert!(rejected(&v));
}
#[test]
fn schema_project_version_and_authority_admission_are_explicit() {
    let raw = serde_json::to_vec(&report()).unwrap();
    for field in 0..4 {
        let mut s = source(&raw);
        match field {
            0 => s.schema = "unknown".into(),
            1 => s.project_id = "foreign".into(),
            2 => s.adapter_version = 99,
            _ => s.authority_namespace.clear(),
        };
        assert!(prepare(raw.as_slice(), &s, Limits::default()).is_err());
    }
    let mut v = report();
    v["schema"] = json!("future-format");
    assert!(rejected(&v));
}
#[test]
fn duplicate_fields_trailing_json_and_invalid_capture_are_rejected() {
    let v = report();
    let raw = serde_json::to_string(&v).unwrap();
    let duplicated = format!(
        "{},\"project_id\":\"synthetic\"}}",
        raw.strip_suffix('}').unwrap()
    );
    assert!(
        prepare(
            duplicated.as_bytes(),
            &source(duplicated.as_bytes()),
            Limits::default()
        )
        .is_err()
    );
    let trailing = format!("{raw} {{}}");
    assert!(
        prepare(
            trailing.as_bytes(),
            &source(trailing.as_bytes()),
            Limits::default()
        )
        .is_err()
    );
    let mut v = report();
    v["census_completed_unix_seconds"] = json!(9);
    assert!(rejected(&v));
    v = report();
    v.as_object_mut()
        .unwrap()
        .remove("census_completed_unix_seconds");
    assert!(rejected(&v));
}

#[test]
fn actual_census_and_named_producer_outputs_have_exact_metadata_and_witness_parity() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for path in [".local", "registry", "hot", "history", "archive"] {
        fs::create_dir_all(root.join(path)).unwrap();
    }
    fs::write(root.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&reel_assets::versions::State::default()).unwrap(),
    )
    .unwrap();
    fs::write(root.join(".local/storage-rust.json"),json!({"path_policy":"configured-local-roots","cache_roots":[root.join("hot"),root.join("history")],"archive_roots":[root.join("archive")],"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}}).to_string()).unwrap();
    fs::write(root.join("asset-project.json"),json!({"schema":"reel.asset-project.v1","project_id":"synthetic","storage_config":".local/storage-rust.json","selections":[],"scan_cas":true,"measure_unregistered_physical":true,"max_inventory_objects":100,"lfs_namespaces":[]}).to_string()).unwrap();
    let bytes = b"synthetic primary input";
    let id = hash(bytes);
    for tier in ["hot", "history"] {
        let path = root
            .join(tier)
            .join("objects/sha256")
            .join(&id[..2])
            .join(&id);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fs::write(
        root.join("registry/objects.tsv"),
        format!("{id}\t{}\n", bytes.len()),
    )
    .unwrap();
    let before = fs::read(root.join("registry/versions.json")).unwrap();
    // Unregistered locations have no declared-size authority. One copy differs
    // in size; the native producer still reports its no-expected-size flag true.
    let unknown_id = hash(b"unregistered input");
    for (tier, data) in [
        ("hot", b"unregistered input".as_slice()),
        ("history", b"short".as_slice()),
    ] {
        let p = root
            .join(tier)
            .join("objects/sha256")
            .join(&unknown_id[..2])
            .join(&unknown_id);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }
    for report in [
        reel_assets::accounting::report(&root).unwrap(),
        reel_assets::accounting::observe_named(
            &root,
            &std::collections::BTreeSet::from([id.clone()]),
        )
        .unwrap(),
    ] {
        let index = parse(&report);
        if report.get("scope").is_some() {
            assert!(matches!(
                index.lookup(&unknown_id).unwrap(),
                Lookup::Listed(_)
            ));
        }
        if let Lookup::Listed(unknown) = index.lookup(&unknown_id).unwrap() {
            assert!(!unknown.registered);
            assert_ne!(unknown.copies[0].bytes, unknown.copies[1].bytes);
            assert!(
                unknown
                    .copies
                    .iter()
                    .all(|c| c.descriptor_size_matches == Some(true))
            );
        }
        let Lookup::Listed(row) = index.lookup(&id).unwrap() else {
            panic!()
        };
        let witness = index.witness(&id).unwrap().unwrap();
        let original = report.pointer(&witness.json_pointer).unwrap();
        assert_eq!(row.bytes, original["bytes"].as_u64());
        assert_eq!(row.registered, original["registered"].as_bool().unwrap());
        assert_eq!(
            row.copies.len(),
            original["copies"].as_array().unwrap().len()
        );
        for (copy, raw) in row
            .copies
            .iter()
            .zip(original["copies"].as_array().unwrap())
        {
            assert_eq!(copy.bytes, raw["bytes"].as_u64().unwrap());
            assert_eq!(copy.physical_id.as_deref(), raw["physical_id"].as_str());
            assert_eq!(copy.allocated_bytes, raw["allocated_bytes"].as_u64());
            assert_eq!(copy.hard_links, raw["hard_links"].as_u64());
            assert_eq!(
                copy.known_lfs_hard_links,
                raw["known_lfs_hard_links"].as_u64()
            );
            assert_eq!(copy.other_hard_links, raw["other_hard_links"].as_u64());
            let original_tier = report["tiers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["tier"].as_u64() == Some(copy.tier as u64))
                .unwrap();
            let captured_tier = index.tier(copy.tier).unwrap();
            assert_eq!(
                Some(captured_tier.root.as_str()),
                original_tier["root"].as_str()
            );
            assert_eq!(
                captured_tier.volume_id.as_deref(),
                original_tier["volume_id"].as_str()
            );
        }
    }
    assert_eq!(
        fs::read(root.join("registry/versions.json")).unwrap(),
        before
    );
}

#[test]
fn lookup_preserves_original_record_positions_even_when_index_keys_sort_differently() {
    let mut v = report();
    let mut second = v["objects"][0].clone();
    second["sha256"] = json!(hash(b"second"));
    v["objects"].as_array_mut().unwrap().insert(0, second);
    let index = parse(&v);
    let w = index.witness(&hash(b"payload")).unwrap().unwrap();
    assert_eq!(w.json_pointer, "/objects/1");
    assert_eq!(
        v.pointer(&w.json_pointer).unwrap()["sha256"],
        hash(b"payload")
    );
}
#[test]
fn empty_inventory_is_valid_under_zero_record_budgets_but_does_not_claim_current_absence() {
    let mut v = report();
    v["objects"] = json!([]);
    v["tiers"] = json!([]);
    let bytes = serde_json::to_vec(&v).unwrap();
    let index = prepare(
        bytes.as_slice(),
        &source(&bytes),
        Limits {
            objects: 0,
            total_copies: 0,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(index.summary().objects, 0);
    assert!(matches!(
        index.lookup(&hash(b"missing")).unwrap(),
        Lookup::NotListedAtCapture
    ));
}
#[test]
fn declared_source_length_limits_actual_reads_even_if_the_reader_is_longer() {
    struct Count<'a> {
        cursor: std::io::Cursor<&'a [u8]>,
        count: &'a std::cell::Cell<usize>,
    }
    impl std::io::Read for Count<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.cursor.read(buf)?;
            self.count.set(self.count.get() + n);
            Ok(n)
        }
    }
    let bytes = serde_json::to_vec(&report()).unwrap();
    let one = &bytes[..1];
    let count = std::cell::Cell::new(0);
    assert!(
        prepare(
            Count {
                cursor: std::io::Cursor::new(bytes.as_slice()),
                count: &count
            },
            &source(one),
            Limits::default()
        )
        .is_err()
    );
    assert_eq!(count.get(), 2);
}
#[test]
fn per_object_copy_array_is_bounded_before_the_next_copy_decode() {
    let mut v = report();
    let copy = v["objects"][0]["copies"][0].clone();
    v["objects"][0]["copies"] = Value::Array(
        (0..65)
            .map(|i| {
                let mut c = copy.clone();
                c["tier"] = json!(i);
                c
            })
            .collect(),
    );
    let raw = serde_json::to_vec(&v).unwrap();
    let error = match prepare(raw.as_slice(), &source(&raw), Limits::default()) {
        Err(e) => e.to_string(),
        Ok(_) => panic!(),
    };
    assert!(error.contains("before next record decode"));
}
