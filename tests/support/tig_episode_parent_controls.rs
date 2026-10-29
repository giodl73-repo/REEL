//! Parent JSON comes from guarded caches even when local copies exist.
use super::*;
use reel::tig_episode::{self, ParentControls};
use tig_store::existing::{Profile as CacheProfile, RequiredProtection};

pub(super) fn exercise(p: &Project, request: &tig_episode::Request) -> ParentControls {
    let plan = tig_episode::plan(&mut p.source(), request).unwrap();
    let bytes = fs::read(object_file(&p.hot(), &plan.conform)).unwrap();
    let manifest: Value = serde_json::from_slice(&bytes).unwrap();
    let mut expected = BTreeSet::from([plan.conform.clone()]);
    for name in [
        "catalog",
        "master_template_definition",
        "source_master_template",
        "episode",
        "season_bindings",
        "episode_bindings",
    ] {
        if let Some(value) = manifest.get(name).filter(|v| !v.is_null()) {
            let reference: FileRef = serde_json::from_value(value.clone()).unwrap();
            let raw = fs::read(p.data.path().join(&reference.path)).unwrap();
            let object = Object::from_bytes(&raw);
            assert_eq!(
                (object.sha256.as_str(), object.bytes),
                (reference.sha256.as_str(), reference.bytes)
            );
            assert_eq!(p.publish("history", &raw), object);
            expected.insert(object);
        }
    }
    let profile: CacheProfile = serde_json::from_value(p.profile["byte_source"].clone()).unwrap();
    for case in ["cooperative", "readers", "bytes"] {
        let mut refused = profile.clone();
        match case {
            "cooperative" => refused.required = RequiredProtection::CooperativeImmutable,
            "readers" => refused.limits.live_readers = 1,
            _ => refused.limits.total_bytes = plan.conform.bytes,
        }
        assert!(
            ParentControls::open(refused, &plan.conform).is_err(),
            "{case}"
        );
    }
    for case in [
        "alias",
        "conflicting-length",
        "traversal",
        "huge",
        "reserved",
    ] {
        let mut bad = manifest.clone();
        match case {
            "alias" => bad["catalog"]["path"] = bad["master_template_definition"]["path"].clone(),
            "conflicting-length" => {
                bad["catalog"]["sha256"] = bad["master_template_definition"]["sha256"].clone();
                bad["catalog"]["bytes"] =
                    json!(bad["master_template_definition"]["bytes"].as_u64().unwrap() + 1);
            }
            "traversal" => bad["catalog"]["path"] = json!("../catalog.json"),
            "huge" => bad["catalog"]["bytes"] = json!(4 * 1024 * 1024 + 1),
            _ => bad["catalog"]["path"] = json!("manifest.json"),
        }
        let pin = p.publish("hot", &serde_json::to_vec(&bad).unwrap());
        assert!(
            ParentControls::open(profile.clone(), &pin).is_err(),
            "{case}"
        );
        // A failed partial open must release the manifest guard without touching
        // its bytes. This opens write access but never writes/truncates the file.
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .open(object_file(&p.hot(), &pin))
                .is_ok()
        );
    }
    let duplicate = p.publish("hot", b"{\"schema\":\"first\",\"schema\":\"second\"}");
    assert!(ParentControls::open(profile.clone(), &duplicate).is_err());
    let files = ParentControls::open(profile, &plan.conform).unwrap();
    assert_eq!(files.objects().cloned().collect::<BTreeSet<_>>(), expected);
    assert_eq!(files.manifest().episode_id, "episode-1");
    let actual = files.verified_path(&files.manifest().catalog).unwrap();
    assert_ne!(actual, p.data.path().join(&files.manifest().catalog.path));
    assert!(
        fs::OpenOptions::new().write(true).open(actual).is_err(),
        "metadata must remain effectively protected"
    );
    for case in ["unknown", "pin", "length"] {
        let mut reference = files.manifest().catalog.clone();
        match case {
            "unknown" => reference.path = "unknown.json".into(),
            "pin" => reference.sha256 = "0".repeat(64),
            _ => reference.bytes += 1,
        }
        assert!(files.verified_path(&reference).is_err(), "{case}");
    }
    files
}
