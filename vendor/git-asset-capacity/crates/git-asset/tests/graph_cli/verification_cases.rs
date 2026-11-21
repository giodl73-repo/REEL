use super::*;

fn provider(f: &Fixture, name: &str, values: &[&[u8]]) -> PathBuf {
    let root = f.path(name);
    fs::create_dir_all(root.join("objects")).unwrap();
    for bytes in values {
        fs::write(
            root.join("objects")
                .join(Object::from_bytes(bytes).sha256.as_str()),
            bytes,
        )
        .unwrap();
    }
    root
}
fn bytes_profile(graph: &Path, places: &[PathBuf]) -> Value {
    json!({"schema":"tig.graph-byte-verification-profile.experimental-1",
        "graph_profile":Object::from_bytes(&fs::read(graph).unwrap()),"selection":"primary-and-controls",
        "byte_source":{"places":places.iter().map(|root| json!({"root":root,"layout":"flat"})).collect::<Vec<_>>(),
        "required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"},
        "limits":{"object_bytes":1024,"total_bytes":4096,"requests":32,"live_readers":1,"verify_millis":30000}}})
}
fn control_fixture() -> Fixture {
    let mut f = Fixture::new();
    let schema = Object::from_bytes(&fs::read(f.root().join("definitions/authored.json")).unwrap());
    let definition = write(
        &f.root(),
        "definitions/control.json",
        &json!({
        "schema":"tig.definition.design-1","id":"types/control","authored_schema":schema,
        "authoring":{"properties":["translation_sources","private_text"],"required":["translation_sources"],"defaults":{}},
        "stream_rules":{},"semantic_rules":{}}),
    );
    let pin = json!({"id":"types/control","sha256":definition.sha256,"bytes":definition.bytes});
    f.record(
        "control",
        "private/control",
        &pin,
        json!([]),
        b"control bytes",
    );
    f.record(
        "en",
        "private/english",
        &f.text,
        json!([{"id":"private/spanish"},{"id":"private/control"}]),
        b"translation",
    );
    f.profiles.as_array_mut().unwrap().push(
        json!({"definition_id":"types/control","definition":definition,
        "class":"container","covered":["production"],"rules":[],"payloads":"SelectedObject"}),
    );
    f.commit();
    f
}

#[test]
fn byte_verification_selects_current_objects_and_controls_without_claiming_custody() {
    let f = control_fixture();
    let empty = provider(&f, "empty", &[]);
    let cache = provider(&f, "cache", &[b"source-v1", b"translation"]);
    let graph = f.save("graph.json", &f.profile());
    let mut profile = bytes_profile(&graph, &[empty, cache.clone()]);
    profile["selection"] = json!("primary");
    let bytes = f.save("bytes.json", &profile);
    let out = f.path("primary.json");
    assert!(f.run("verify", &[&graph, &bytes, &out]).status.success());
    let report = f.load("primary.json");
    assert_eq!(report["verified_objects"], 2);
    assert_eq!(report["selected_objects_verified"], true);
    assert_eq!(report["complete_declared_payload_verification"], false);
    for name in [
        "protection_retained",
        "all_observations_same_instant",
        "independent_custody_verified",
        "complete_claim_coverage",
        "authorizes_eviction",
    ] {
        assert_eq!(report[name], false);
    }
    assert!(
        report["observations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["source_index"] == 1)
    );
    assert_eq!(report["graph"]["authored_schemas_validated"], false);
    profile["selection"] = json!("primary-and-controls");
    let bytes = f.save("with-controls.json", &profile);
    let missing = f.path("missing.json");
    assert!(
        !f.run("verify", &[&graph, &bytes, &missing])
            .status
            .success()
    );
    let report = f.load("missing.json");
    assert_eq!(report["verified_objects"], 2);
    assert_eq!(report["observations"].as_array().unwrap().len(), 3);
    assert_eq!(report["selected_objects_verified"], false);
    assert!(
        report["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["outcome"] == "missing")
    );
    let control = Object::from_bytes(b"control bytes");
    fs::write(
        cache.join("objects").join(control.sha256.as_str()),
        b"control bytes",
    )
    .unwrap();
    let success = f.path("all.json");
    assert!(
        f.run("verify", &[&graph, &bytes, &success])
            .status
            .success()
    );
    assert_eq!(
        f.load("all.json")["complete_declared_payload_verification"],
        true
    );
    let original = fs::read(&success).unwrap();
    assert!(
        !f.run("verify", &[&graph, &bytes, &success])
            .status
            .success()
    );
    assert_eq!(fs::read(&success).unwrap(), original);
    assert_eq!(fs::read_dir(cache.join("objects")).unwrap().count(), 3);
    for data in [b"source-v1".as_slice(), b"translation", b"control bytes"] {
        assert_eq!(
            fs::read(
                cache
                    .join("objects")
                    .join(Object::from_bytes(data).sha256.as_str())
            )
            .unwrap(),
            data
        );
    }
}

#[test]
fn byte_verification_never_hides_present_corruption_with_later_valid_bytes() {
    let f = Fixture::new();
    let first = provider(&f, "first", &[]);
    let later = provider(&f, "later", &[b"source-v1", b"translation"]);
    fs::write(
        first
            .join("objects")
            .join(Object::from_bytes(b"source-v1").sha256.as_str()),
        b"source-vX",
    )
    .unwrap();
    let graph = f.save("graph.json", &f.profile());
    let bytes = f.save(
        "bytes.json",
        &bytes_profile(&graph, &[first.clone(), later]),
    );
    let out = f.path("corrupt.json");
    assert!(!f.run("verify", &[&graph, &bytes, &out]).status.success());
    let report = f.load("corrupt.json");
    assert_eq!(report["verified_objects"], 1);
    let rows = report["observations"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .filter(|row| row["outcome"] == "integrity")
            .count(),
        1
    );
    assert_eq!(report["selected_objects_verified"], false);
    assert_eq!(report["authorizes_eviction"], false);
    assert_eq!(
        fs::read(
            first
                .join("objects")
                .join(Object::from_bytes(b"source-v1").sha256.as_str())
        )
        .unwrap(),
        b"source-vX"
    );
}

#[test]
fn verification_refuses_mismatched_profiles_budgets_and_incomplete_graphs() {
    let f = Fixture::new();
    let cache = provider(&f, "cache", &[b"source-v1", b"translation"]);
    let graph = f.save("graph.json", &f.profile());
    for case in 0..9 {
        let mut profile = bytes_profile(&graph, std::slice::from_ref(&cache));
        match case {
            0 => profile["schema"] = json!("unknown"),
            1 => profile["graph_profile"] = json!(Object::from_bytes(b"other profile")),
            2 => profile["unexpected"] = json!(true),
            3 => profile["selection"] = json!("all-history"),
            4 => profile["byte_source"]["limits"]["requests"] = json!(1),
            5 => profile["byte_source"]["limits"]["object_bytes"] = json!(1),
            6 => profile["byte_source"]["limits"]["total_bytes"] = json!(1),
            7 => profile["byte_source"]["limits"]["unknown"] = json!(1),
            8 => {
                profile.as_object_mut().unwrap().remove("selection");
            }
            _ => unreachable!(),
        }
        let byte_path = f.save("bytes.json", &profile);
        let out = f.path(&format!("refused-{case}.json"));
        assert!(
            !f.run("verify", &[&graph, &byte_path, &out])
                .status
                .success(),
            "case {case}"
        );
        assert!(!out.exists());
    }
    let mut tiny = f.profile();
    tiny["max_report_bytes"] = json!(1);
    let graph = f.save("tiny.json", &tiny);
    let bytes = f.save(
        "bytes.json",
        &bytes_profile(&graph, std::slice::from_ref(&cache)),
    );
    let out = f.path("too-small.json");
    assert!(!f.run("verify", &[&graph, &bytes, &out]).status.success());
    assert!(!out.exists());
    f.record(
        "en",
        "private/english",
        &f.text,
        json!([{"id":"missing"}]),
        b"translation",
    );
    f.commit();
    let graph = f.save("incomplete.json", &f.profile());
    let bytes = f.save("bytes.json", &bytes_profile(&graph, &[cache]));
    let out = f.path("incomplete-report.json");
    assert!(!f.run("verify", &[&graph, &bytes, &out]).status.success());
    assert!(!out.exists());
}

#[test]
fn output_only_and_nonproduction_queries_cannot_be_reported_as_verified_inputs() {
    let f = Fixture::new();
    f.record(
        "output",
        "private/delivery",
        &f.output,
        json!([]),
        b"delivered bytes are not production inputs",
    );
    f.commit();
    let cache = provider(&f, "cache", &[b"source-v1", b"translation"]);
    for (name, roots, kinds) in [
        (
            "output-only",
            json!(["private/delivery"]),
            json!(["production"]),
        ),
        (
            "nonproduction",
            json!(["private/english"]),
            json!(["evidence"]),
        ),
    ] {
        let mut p = f.profile();
        p["roots"] = roots;
        p["kinds"] = kinds;
        let graph = f.save(&format!("{name}-graph.json"), &p);
        let bytes = f.save(
            &format!("{name}-bytes.json"),
            &bytes_profile(&graph, std::slice::from_ref(&cache)),
        );
        let out = f.path(&format!("{name}-report.json"));
        assert!(!f.run("verify", &[&graph, &bytes, &out]).status.success());
        assert!(!out.exists());
    }
}

#[test]
fn verification_deduplicates_bytes_across_logical_assets_and_excludes_history() {
    let f = Fixture::new();
    f.record(
        "en",
        "private/english",
        &f.text,
        json!([{"id":"private/spanish"}]),
        b"source-v1",
    );
    f.commit();
    let cache = provider(&f, "cache", &[b"source-v1"]);
    let graph = f.save("graph.json", &f.profile());
    let mut profile = bytes_profile(&graph, &[cache]);
    profile["byte_source"]["limits"]["requests"] = json!(1);
    let bytes = f.save("bytes.json", &profile);
    let out = f.path("deduplicated.json");
    assert!(f.run("verify", &[&graph, &bytes, &out]).status.success());
    let report = f.load("deduplicated.json");
    assert_eq!(report["verified_objects"], 1);
    assert_eq!(report["verified_bytes"], b"source-v1".len());
    assert_eq!(report["complete_declared_payload_verification"], true);
    assert_eq!(report["graph"]["nodes"].as_array().unwrap().len(), 2);
}
