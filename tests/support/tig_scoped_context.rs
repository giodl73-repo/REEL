use super::*;
use tig_core::tree::{MemoryNodes, TreeLimits};
use tig_store::metadata::{Limits as StoreLimits, MetadataReader, MetadataStore};

#[test]
fn explicit_scoped_compile_reproduces_original_contracts_offline_and_refuses_other_modes() {
    let (f, request) = bilingual();
    let mut source = f.source();
    let baseline = f.root().join("baseline-compilation");
    reel::tig_scene::compile(&mut source, &request, &baseline).unwrap();
    let disk = tempfile::tempdir().unwrap();
    let metadata_path = disk.path().join("metadata");
    let mut nodes = MetadataStore::create(
        &metadata_path,
        StoreLimits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let context = reel::tig_scene::freeze_language_context(
        &mut source,
        &request,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let legacy =
        reel::tig_scene::freeze_context(&mut source, &request, &mut nodes, TreeLimits::default())
            .unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    let legacy_profile =
        archived_profile(&f, legacy.archive().clone(), source.checkpoint().clone());
    let witness = nodes.finish().unwrap();
    drop(nodes);
    let mut nodes = MetadataReader::open(&metadata_path, &witness).unwrap();
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let (mut restored, _) = reel::tig_scene::restore_language_context(
        profile,
        &request,
        context.selection(),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let reads = restored.byte_requests();
    let mut wrong_request: reel::tig_scene::Request =
        serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap();
    wrong_request.language = "es".into();
    let wrong_output = f.root().join("wrong-request");
    assert!(
        reel::tig_scene::compile_language_context(&mut restored, &wrong_request, &wrong_output)
            .is_err()
    );
    assert_eq!(restored.byte_requests(), reads);
    assert!(!wrong_output.exists());
    let wrong_mode = f.root().join("wrong-legacy-mode");
    assert!(reel::tig_scene::compile(&mut restored, &request, &wrong_mode).is_err());
    assert_eq!(restored.byte_requests(), reads);
    assert!(!wrong_mode.exists());
    let output = f.root().join("scoped-compilation");
    reel::tig_scene::compile_language_context(&mut restored, &request, &output).unwrap();
    assert_eq!(restored.byte_requests() - reads, 6);
    for name in [
        "production.json",
        "contract.json",
        "job.json",
        "semantic-delivery.json",
    ] {
        assert_eq!(
            fs::read(baseline.join("delivery").join(name)).unwrap(),
            fs::read(output.join("delivery").join(name)).unwrap()
        );
    }
    let (mut legacy_source, _) = reel::tig_scene::restore_context(
        legacy_profile,
        &request,
        legacy.selection(),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let output = f.root().join("wrong-scoped-mode");
    assert!(
        reel::tig_scene::compile_language_context(&mut legacy_source, &request, &output).is_err()
    );
    assert_eq!(legacy_source.byte_requests(), 0);
    assert!(!output.exists());
}

#[test]
fn scoped_context_replays_original_language_roots_after_main_moves_and_git_is_offline() {
    let (mut f, request) = bilingual();
    let mut original = f.source();
    let disk = tempfile::tempdir().unwrap();
    let path = disk.path().join("metadata");
    let mut store = MetadataStore::create(
        &path,
        StoreLimits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let context = reel::tig_scene::freeze_language_context(
        &mut original,
        &request,
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(original.byte_requests(), 5);
    assert_eq!(context.requirements().primary().len(), 4);
    assert_eq!(context.requirements().controls().len(), 5);
    assert_eq!(context.roots().len(), 5);
    let profile = archived_profile(&f, context.archive().clone(), original.checkpoint().clone());
    let mut record = load(&f, "records/cel-b.tig");
    let replacement = cached(&f, &picture([0, 255, 0]));
    record["selected"] = json!(replacement);
    write(f.root(), "records/cel-b.tig", &record);
    save_commit(&mut f);
    let next = reel::tig_scene::freeze_language_context(
        &mut f.source(),
        &request,
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    assert_ne!(context.selection(), next.selection());
    assert_eq!(
        next.requirements()
            .primary()
            .difference(context.requirements().primary())
            .cloned()
            .collect::<Vec<_>>(),
        vec![replacement]
    );
    let witness = store.finish().unwrap();
    drop(store);
    drop(original);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut reader = MetadataReader::open(&path, &witness).unwrap();
    let (mut restored, replay) = reel::tig_scene::restore_language_context(
        profile,
        &request,
        context.selection(),
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(context.selection(), replay.selection());
    assert_eq!(context.requirements(), replay.requirements());
    assert_eq!(context.roots(), replay.roots());
    assert_eq!(restored.byte_requests(), 5);
    let requests = restored.byte_requests();
    let output = disk.path().join("wrong-mode");
    assert!(reel::tig_scene::compile(&mut restored, &request, &output).is_err());
    assert_eq!(restored.byte_requests(), requests);
    assert!(!output.exists());
    assert!(
        reel::tig_scene::recipe(&mut restored, &request, &Object::from_bytes(b"producer")).is_err()
    );
}

#[test]
fn scoped_freeze_refuses_unknown_language_and_schema_without_metadata_writes() {
    let (mut f, mut request) = bilingual();
    let mut nodes = MemoryNodes::default();
    request.language = "fr".into();
    assert!(
        reel::tig_scene::freeze_language_context(
            &mut f.source(),
            &request,
            &mut nodes,
            TreeLimits::default()
        )
        .is_err()
    );
    assert_eq!(nodes.objects().count(), 0);
    request.language = "en".into();
    let mut parent = load(&f, "records/scene.tig");
    let slot = parent["streams"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["slot"] == "profile.json")
        .unwrap();
    let pin: Object = serde_json::from_value(slot["object"].clone()).unwrap();
    let mut value: Value = serde_json::from_slice(
        &fs::read(f.root().join("cache/objects").join(pin.sha256.as_str())).unwrap(),
    )
    .unwrap();
    value["schema"] = "unknown-profile".into();
    slot["object"] = json!(cached(&f, &serde_json::to_vec(&value).unwrap()));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    assert!(
        reel::tig_scene::freeze_language_context(
            &mut f.source(),
            &request,
            &mut nodes,
            TreeLimits::default()
        )
        .is_err()
    );
    assert_eq!(nodes.objects().count(), 0);
}

#[test]
fn scoped_restore_refuses_wrong_request_before_cache_access_and_missing_or_corrupt_controls() {
    let (f, mut request) = bilingual();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let context = reel::tig_scene::freeze_language_context(
        &mut source,
        &request,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    let parent = load(&f, "records/scene.tig");
    let body: Object = serde_json::from_value(parent["selected"].clone()).unwrap();
    let path = f.root().join("cache/objects").join(body.sha256.as_str());
    let raw = fs::read(&path).unwrap();
    fs::rename(&path, f.root().join("offline-scene-control")).unwrap();
    request.language = "es".into();
    let error = reel::tig_scene::restore_language_context(
        profile.clone(),
        &request,
        context.selection(),
        &mut nodes,
        TreeLimits::default(),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("descriptor differs"));
    request.language = "en".into();
    assert!(
        reel::tig_scene::restore_language_context(
            profile.clone(),
            &request,
            context.selection(),
            &mut nodes,
            TreeLimits::default()
        )
        .is_err()
    );
    fs::write(&path, b"corrupt").unwrap();
    assert!(
        reel::tig_scene::restore_language_context(
            profile.clone(),
            &request,
            context.selection(),
            &mut nodes,
            TreeLimits::default()
        )
        .is_err()
    );
    fs::write(&path, raw).unwrap();
    assert!(
        reel::tig_scene::restore_language_context(
            profile,
            &request,
            context.selection(),
            &mut nodes,
            TreeLimits::default()
        )
        .is_ok()
    );
}

#[test]
fn valid_generic_context_cannot_self_admit_an_incomplete_native_language_root_set() {
    use tig_assets::{
        graph::{self, NodeKind, Payloads},
        resolve::AssetResolver,
    };
    use tig_core::LogicalPath;
    use tig_git::source::{GitId, Repository, Scope};
    let (f, request) = bilingual();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let valid = reel::tig_scene::freeze_language_context(
        &mut source,
        &request,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let archive = archived_profile(&f, valid.archive().clone(), source.checkpoint().clone());
    let repository =
        Repository::new(git_executable(), f.root().to_owned(), Default::default()).unwrap();
    let commit = repository
        .commit(GitId::parse(f.profile["commit"].as_str().unwrap()).unwrap())
        .unwrap();
    let mut snapshot = repository
        .native(
            &commit,
            Scope::new([LogicalPath::parse("records").unwrap()]).unwrap(),
            Scope::new([LogicalPath::parse("definitions").unwrap()]).unwrap(),
            Default::default(),
        )
        .unwrap();
    let profiles = f.profile["admitted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let id = p["id"].as_str().unwrap();
            let container = id == request.scene_definition;
            let payloads = if container {
                Payloads::Named {
                    selected: true,
                    slots: BTreeSet::from([
                        "catalog.json".into(),
                        "episode.json".into(),
                        "policy.json".into(),
                        "profile.json".into(),
                    ]),
                }
            } else if p["payload"]["kind"] == "selected" {
                Payloads::SelectedObject
            } else {
                Payloads::Named {
                    selected: false,
                    slots: BTreeSet::from([p["payload"]["slot"].as_str().unwrap().to_owned()]),
                }
            };
            graph::Profile {
                definition_id: id.into(),
                definition: serde_json::from_value(p["object"].clone()).unwrap(),
                class: if container {
                    NodeKind::Container
                } else {
                    NodeKind::Primary
                },
                covered: BTreeSet::from([graph::EdgeKind::Production]),
                rules: vec![],
                payloads,
            }
        })
        .collect::<Vec<_>>();
    let query = graph::Query {
        checkpoint: snapshot.checkpoint().clone(),
        roots: BTreeSet::from([snapshot
            .resolve_id(&request.scene_id)
            .unwrap()
            .reference()
            .clone()]),
        kinds: BTreeSet::from([graph::EdgeKind::Production]),
        limits: graph::Limits {
            nodes: 4097,
            edges: 4096,
            depth: 2,
            bytes: 16 * 1024 * 1024,
        },
    };
    let incomplete = snapshot
        .freeze_production_context(
            &query,
            &profiles,
            valid.descriptor(),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
    assert_eq!(incomplete.archive(), valid.archive());
    assert_eq!(
        incomplete.requirements().admission(),
        valid.requirements().admission()
    );
    assert_eq!(incomplete.requirements().primary().len(), 0);
    assert!(
        reel::tig_scene::restore_language_context(
            archive,
            &request,
            incomplete.selection(),
            &mut nodes,
            TreeLimits::default()
        )
        .is_err()
    );
}

#[test]
#[ignore = "explicit existing-source metadata rehearsal; requires profile and request paths"]
fn explicit_existing_source_scoped_context_rehearsal() {
    existing_source_scoped_rehearsal(false);
}

#[test]
#[ignore = "explicit existing-source scoped compilation; requires profile and request paths"]
fn explicit_existing_source_scoped_compile_rehearsal() {
    existing_source_scoped_rehearsal(true);
}

fn existing_source_scoped_rehearsal(compile: bool) {
    use reel::tig_inputs::{ARCHIVE_PROFILE_SCHEMA, ArchiveProfile};
    let profile_path =
        std::env::var_os("REEL_TIG_SCOPE_PROFILE").expect("explicit input profile required");
    let request_path =
        std::env::var_os("REEL_TIG_SCOPE_REQUEST").expect("explicit native request required");
    let profile = Profile::read(Path::new(&profile_path)).unwrap();
    let admitted = profile.admitted.clone();
    let byte_source = profile.byte_source.clone();
    let limits = profile.limits;
    let request = reel::tig_scene::Request::read(Path::new(&request_path)).unwrap();
    let mut source = Source::open(profile).unwrap();
    let disk = tempfile::tempdir().unwrap();
    let baseline = disk.path().join("baseline");
    if compile {
        reel::tig_scene::compile(&mut source, &request, &baseline).unwrap();
    }
    let path = disk.path().join("metadata");
    let mut store = MetadataStore::create(
        &path,
        StoreLimits {
            nodes: 32768,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let context = reel::tig_scene::freeze_language_context(
        &mut source,
        &request,
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    let archive = ArchiveProfile {
        schema: ARCHIVE_PROFILE_SCHEMA.into(),
        archive: context.archive().clone(),
        checkpoint: source.checkpoint().clone(),
        limits,
        admitted,
        byte_source,
    };
    let witness = store.finish().unwrap();
    drop(store);
    drop(source);
    let mut reader = MetadataReader::open(&path, &witness).unwrap();
    let (mut source, restored) = reel::tig_scene::restore_language_context(
        archive,
        &request,
        context.selection(),
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(context.selection(), restored.selection());
    assert_eq!(context.requirements(), restored.requirements());
    assert_eq!(context.roots(), restored.roots());
    if compile {
        let output = disk.path().join("scoped");
        reel::tig_scene::compile_language_context(&mut source, &request, &output).unwrap();
        for name in [
            "production.json",
            "contract.json",
            "job.json",
            "semantic-delivery.json",
        ] {
            assert_eq!(
                fs::read(baseline.join("delivery").join(name)).unwrap(),
                fs::read(output.join("delivery").join(name)).unwrap()
            );
        }
        println!(
            "{}",
            json!({"compiled_contracts_equal":true,"media_rendered":false,"authorizes_eviction":false})
        );
    }
    println!(
        "{}",
        json!({"primary":context.requirements().primary().len(),"controls":context.requirements().controls().len(),"interpretation":context.requirements().metadata().len(),"roots":context.roots().len(),"replay_equal":true,"media_rendered":false,"authorizes_eviction":false})
    );
}
