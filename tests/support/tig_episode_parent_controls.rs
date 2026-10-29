//! Parent JSON comes from guarded caches even when local copies exist.
use super::*;
use reel::tig_episode::{self, ParentControls};
use tig_core::{
    LogicalPath,
    tree::{Tree, TreeLimits},
};
use tig_store::existing::{Profile as CacheProfile, RequiredProtection};

pub(super) struct SceneControlSets {
    pub files: BTreeMap<String, tig_episode::ControlTree>,
    pub production: BTreeMap<String, FileRef>,
}
impl SceneControlSets {
    pub fn borrowed(&self) -> BTreeMap<String, episode_conform::SceneControls<'_>> {
        self.files
            .iter()
            .map(|(id, files)| {
                (
                    id.clone(),
                    episode_conform::SceneControls {
                        files,
                        production: &self.production[id],
                    },
                )
            })
            .collect()
    }
}
pub(super) fn scene_controls(p: &Project, parent: &ParentControls) -> SceneControlSets {
    let root = p.data.path().join("scene-control-trees");
    let mut writer = MetadataStore::create(
        &root,
        tig_store::metadata::Limits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let mut roots = BTreeMap::new();
    let mut production = BTreeMap::new();
    let mut expected = BTreeMap::new();
    for segment in &parent.manifest().segments {
        let job_ref = segment.delivery_job.as_ref().unwrap();
        let raw_job = fs::read(p.data.path().join(&job_ref.path)).unwrap();
        let job: Job = serde_json::from_slice(&raw_job).unwrap();
        let mut contract_ref = job.contract.clone();
        contract_ref.path = job_ref.path.parent().unwrap().join(&contract_ref.path);
        let raw_contract = fs::read(p.data.path().join(&contract_ref.path)).unwrap();
        let contract: reel::cue_relative::Contract = serde_json::from_slice(&raw_contract).unwrap();
        let production_path = contract_ref
            .path
            .parent()
            .unwrap()
            .join(&contract.production_manifest);
        let raw_production = fs::read(p.data.path().join(&production_path)).unwrap();
        let production_pin = Object::from_bytes(&raw_production);
        assert_eq!(
            production_pin.sha256.as_str(),
            job.production_manifest_sha256
        );
        let production_ref = FileRef {
            path: production_path,
            sha256: production_pin.sha256.as_str().into(),
            bytes: production_pin.bytes,
        };
        let semantic_ref = segment.semantic_delivery.as_ref().unwrap();
        let raw_semantic = fs::read(p.data.path().join(&semantic_ref.path)).unwrap();
        let mut names = BTreeMap::new();
        for (reference, bytes) in [
            (job_ref, &raw_job),
            (&contract_ref, &raw_contract),
            (&production_ref, &raw_production),
            (semantic_ref, &raw_semantic),
        ] {
            let object = Object::from_bytes(bytes);
            assert_eq!(
                (object.sha256.as_str(), object.bytes),
                (reference.sha256.as_str(), reference.bytes)
            );
            assert_eq!(p.publish("history", bytes), object);
            names.insert(
                LogicalPath::parse(&reference.path.to_str().unwrap().replace('\\', "/")).unwrap(),
                object,
            );
        }
        expected.insert(
            segment.id.clone(),
            names.values().cloned().collect::<BTreeSet<_>>(),
        );
        roots.insert(
            segment.id.clone(),
            Tree::build(&names, &mut writer, TreeLimits::default())
                .unwrap()
                .0
                .root()
                .clone(),
        );
        production.insert(segment.id.clone(), production_ref);
    }
    let checkpoint = writer.finish().unwrap();
    drop(writer);
    let mut reader = MetadataReader::open(&root, &checkpoint).unwrap();
    let profile: CacheProfile = serde_json::from_value(p.profile["byte_source"].clone()).unwrap();
    let files = roots
        .into_iter()
        .map(|(id, root)| {
            let files =
                tig_episode::ControlTree::open(profile.clone(), &root, &mut reader).unwrap();
            assert_eq!(
                files.objects().cloned().collect::<BTreeSet<_>>(),
                expected[&id]
            );
            (id, files)
        })
        .collect();
    SceneControlSets { files, production }
}

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
