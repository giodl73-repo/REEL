use super::*;
use tig_core::tree::{MemoryNodes, TreeLimits};
use tig_store::metadata::{Limits as StoreLimits, MetadataReader, MetadataStore};

#[test]
#[ignore = "explicit existing-source compiled render context; requires profile and request paths"]
fn explicit_existing_source_render_context_rehearsal() {
    use reel::tig_inputs::{ARCHIVE_PROFILE_SCHEMA, ArchiveProfile};
    let profile_path =
        std::env::var_os("REEL_TIG_SCOPE_PROFILE").expect("explicit profile required");
    let request_path =
        std::env::var_os("REEL_TIG_SCOPE_REQUEST").expect("explicit request required");
    let profile = Profile::read(Path::new(&profile_path)).unwrap();
    let admitted = profile.admitted.clone();
    let byte_source = profile.byte_source.clone();
    let limits = profile.limits;
    let request = reel::tig_scene::Request::read(Path::new(&request_path)).unwrap();
    let mut source = Source::open(profile).unwrap();
    let disk = tempfile::tempdir().unwrap();
    let path = disk.path().join("metadata");
    let before = disk.path().join("before");
    let mut nodes = MetadataStore::create(
        &path,
        StoreLimits {
            nodes: 32768,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let context = reel::tig_scene::freeze_render_context(
        &mut source,
        &request,
        &before,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let profile = ArchiveProfile {
        schema: ARCHIVE_PROFILE_SCHEMA.into(),
        archive: context.archive().clone(),
        checkpoint: source.checkpoint().clone(),
        limits,
        admitted,
        byte_source,
    };
    let witness = nodes.finish().unwrap();
    drop(nodes);
    drop(source);
    let mut reader = MetadataReader::open(&path, &witness).unwrap();
    let after = disk.path().join("after");
    let (_, restored) = reel::tig_scene::restore_render_context(
        profile,
        &request,
        context.selection(),
        &after,
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert!(
        context.selection() == restored.selection(),
        "selection differs"
    );
    assert!(
        context.descriptor() == restored.descriptor(),
        "descriptor differs"
    );
    assert!(
        context.requirements() == restored.requirements(),
        "requirements differ"
    );
    for name in ["production.json", "contract.json", "job.json"] {
        assert!(
            fs::read(before.join("delivery").join(name)).unwrap()
                == fs::read(after.join("delivery").join(name)).unwrap(),
            "compiled contract differs"
        );
    }
    println!(
        "{}",
        json!({"contracts_equal":true,"replay_equal":true,"primary":context.requirements().primary().len(),"controls":context.requirements().controls().len(),"roots":context.roots().len(),"media_rendered":false,"authorizes_eviction":false})
    );
}

fn generic_context(
    f: &Fixture,
    request: &reel::tig_scene::Request,
    original: &tig_git::source::native::context::ProductionContext,
    descriptor: &[u8],
    nodes: &mut MemoryNodes,
) -> tig_git::source::native::context::ProductionContext {
    use tig_assets::graph::{self, NodeKind, Payloads};
    use tig_core::LogicalPath;
    use tig_git::source::{GitId, Repository, Scope};
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
                payloads: if container {
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
                },
            }
        })
        .collect::<Vec<_>>();
    let query = graph::Query {
        checkpoint: original.requirements().checkpoint().clone(),
        roots: original.roots().clone(),
        kinds: BTreeSet::from([graph::EdgeKind::Production]),
        limits: graph::Limits {
            nodes: 4097,
            edges: 4096,
            depth: 2,
            bytes: 16 * 1024 * 1024,
        },
    };
    snapshot
        .freeze_production_context(&query, &profiles, descriptor, nodes, TreeLimits::default())
        .unwrap()
}

#[test]
fn valid_generic_context_cannot_self_admit_forged_render_parameters() {
    let (f, request) = bilingual();
    let mut nodes = MemoryNodes::default();
    let valid = freeze(&f, &request, "original", &mut nodes);
    for mode in ["compiler", "contract", "authoring", "unknown"] {
        let mut descriptor: Value = serde_json::from_slice(valid.descriptor()).unwrap();
        match mode {
            "compiler" => descriptor["compiler"] = json!(Object::from_bytes(b"foreign compiler")),
            "authoring" => descriptor["authoring"] = json!("a".repeat(64)),
            "contract" => {
                descriptor["compiled_inputs"]["job.json"] =
                    json!(Object::from_bytes(b"untrusted generated job"))
            }
            _ => descriptor["unknown"] = json!(true),
        }
        let forged = generic_context(
            &f,
            &request,
            &valid,
            &serde_json::to_vec(&descriptor).unwrap(),
            &mut nodes,
        );
        assert_eq!(valid.archive(), forged.archive());
        assert_eq!(valid.roots(), forged.roots());
        assert_eq!(valid.requirements(), forged.requirements());
        let source = f.source();
        let profile = archived_profile(&f, valid.archive().clone(), source.checkpoint().clone());
        let output = f.root().join(format!("forged-{mode}"));
        let error = reel::tig_scene::restore_render_context(
            profile,
            &request,
            forged.selection(),
            &output,
            &mut nodes,
            TreeLimits::default(),
        )
        .err()
        .unwrap();
        if mode == "contract" || mode == "authoring" {
            assert!(error.to_string().contains("parameters differ"));
            assert!(output.join("delivery/job.json").is_file());
        } else {
            assert!(!output.exists());
        }
    }
}

fn freeze(
    f: &Fixture,
    request: &reel::tig_scene::Request,
    name: &str,
    nodes: &mut MemoryNodes,
) -> tig_git::source::native::context::ProductionContext {
    reel::tig_scene::freeze_render_context(
        &mut f.source(),
        request,
        &f.root().join(name),
        nodes,
        TreeLimits::default(),
    )
    .unwrap()
}

#[test]
fn render_context_recompiles_exact_contracts_from_reopened_metadata_with_git_offline() {
    let (f, request) = bilingual();
    let disk = tempfile::tempdir().unwrap();
    let path = disk.path().join("metadata");
    let mut nodes = MetadataStore::create(
        &path,
        StoreLimits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let mut source = f.source();
    let before = f.root().join("before");
    let context = reel::tig_scene::freeze_render_context(
        &mut source,
        &request,
        &before,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(source.byte_requests(), 6);
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    let parameters: Value = serde_json::from_slice(context.descriptor()).unwrap();
    assert_eq!(parameters["compiled_inputs"].as_object().unwrap().len(), 3);
    for name in ["production.json", "contract.json", "job.json"] {
        assert_eq!(
            parameters["compiled_inputs"][name],
            json!(Object::from_bytes(
                &fs::read(before.join("delivery").join(name)).unwrap()
            ))
        );
    }
    let witness = nodes.finish().unwrap();
    drop(nodes);
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut reader = MetadataReader::open(&path, &witness).unwrap();
    let after = f.root().join("after");
    let (mut source, restored) = reel::tig_scene::restore_render_context(
        profile,
        &request,
        context.selection(),
        &after,
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(context.selection(), restored.selection());
    assert_eq!(context.requirements(), restored.requirements());
    assert_eq!(context.descriptor(), restored.descriptor());
    assert_eq!(source.byte_requests(), 6);
    for name in ["production.json", "contract.json", "job.json"] {
        assert_eq!(
            fs::read(before.join("delivery").join(name)).unwrap(),
            fs::read(after.join("delivery").join(name)).unwrap()
        );
    }
    let reads = source.byte_requests();
    let wrong = f.root().join("wrong-mode");
    assert!(reel::tig_scene::compile_language_context(&mut source, &request, &wrong).is_err());
    assert!(reel::tig_scene::compile(&mut source, &request, &wrong).is_err());
    assert!(
        reel::tig_scene::recipe(&mut source, &request, &Object::from_bytes(b"runtime")).is_err()
    );
    assert_eq!(source.byte_requests(), reads);
    assert!(!wrong.exists());
}

#[test]
fn render_parameters_ignore_other_language_take_changes_but_capture_selected_cel_changes() {
    let (mut f, request) = bilingual();
    let mut nodes = MemoryNodes::default();
    let before = freeze(&f, &request, "before", &mut nodes);
    let mut voice = load(&f, "records/cue-main.tig");
    voice["selected"] = json!(Object::from_bytes(b"unavailable other language take"));
    write(f.root(), "records/cue-main.tig", &voice);
    save_commit(&mut f);
    let other = freeze(&f, &request, "other-language", &mut nodes);
    assert_eq!(before.descriptor(), other.descriptor());
    assert_ne!(before.selection(), other.selection());
    assert_eq!(
        before.requirements().primary(),
        other.requirements().primary()
    );
    let mut parent = load(&f, "records/scene.tig");
    let scene_pin: Object = serde_json::from_value(parent["selected"].clone()).unwrap();
    let mut scene: Value = serde_json::from_slice(
        &fs::read(
            f.root()
                .join("cache/objects")
                .join(scene_pin.sha256.as_str()),
        )
        .unwrap(),
    )
    .unwrap();
    scene["languages"]["es"]["cues"][0]["exact_text_sha256"] = json!("b".repeat(64));
    parent["selected"] = json!(cached(&f, &serde_json::to_vec(&scene).unwrap()));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let other = freeze(&f, &request, "other-language-control", &mut nodes);
    assert_eq!(before.descriptor(), other.descriptor());
    assert_ne!(
        before.requirements().controls(),
        other.requirements().controls()
    );
    let mut cel = load(&f, "records/cel-b.tig");
    let replacement = cached(&f, &picture([0, 255, 0]));
    cel["selected"] = json!(replacement);
    write(f.root(), "records/cel-b.tig", &cel);
    save_commit(&mut f);
    let changed = freeze(&f, &request, "selected-cel", &mut nodes);
    assert_ne!(other.descriptor(), changed.descriptor());
    assert_eq!(
        changed
            .requirements()
            .primary()
            .difference(other.requirements().primary())
            .cloned()
            .collect::<Vec<_>>(),
        vec![replacement]
    );
}

#[test]
fn selected_language_text_changes_parameters_even_before_recording_is_regenerated() {
    let (mut f, request) = bilingual();
    let mut nodes = MemoryNodes::default();
    let before = freeze(&f, &request, "before-text", &mut nodes);
    let mut parent = load(&f, "records/scene.tig");
    let pin: Object = serde_json::from_value(parent["selected"].clone()).unwrap();
    let mut scene: Value = serde_json::from_slice(
        &fs::read(f.root().join("cache/objects").join(pin.sha256.as_str())).unwrap(),
    )
    .unwrap();
    scene["languages"]["en"]["cues"][0]["exact_text_sha256"] = json!("b".repeat(64));
    parent["selected"] = json!(cached(&f, &serde_json::to_vec(&scene).unwrap()));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let changed = freeze(&f, &request, "after-text", &mut nodes);
    assert_eq!(
        before.requirements().primary(),
        changed.requirements().primary()
    );
    assert_ne!(before.descriptor(), changed.descriptor());
    let a: Value = serde_json::from_slice(before.descriptor()).unwrap();
    let b: Value = serde_json::from_slice(changed.descriptor()).unwrap();
    assert_eq!(a["compiled_inputs"], b["compiled_inputs"]);
    assert_ne!(a["authoring"], b["authoring"]);
}

#[test]
fn render_parameters_capture_profile_and_request_changes_over_identical_primary_inputs() {
    let (mut f, mut request) = bilingual();
    let mut nodes = MemoryNodes::default();
    let before = freeze(&f, &request, "before", &mut nodes);
    let mut parent = load(&f, "records/scene.tig");
    let slot = parent["streams"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["slot"] == "profile.json")
        .unwrap();
    let pin: Object = serde_json::from_value(slot["object"].clone()).unwrap();
    let mut profile: Value = serde_json::from_slice(
        &fs::read(f.root().join("cache/objects").join(pin.sha256.as_str())).unwrap(),
    )
    .unwrap();
    profile["width"] = json!(32);
    slot["object"] = json!(cached(&f, &serde_json::to_vec(&profile).unwrap()));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let changed = freeze(&f, &request, "profile-edit", &mut nodes);
    assert_eq!(
        before.requirements().primary(),
        changed.requirements().primary()
    );
    assert_ne!(before.descriptor(), changed.descriptor());
    let a: Value = serde_json::from_slice(before.descriptor()).unwrap();
    let b: Value = serde_json::from_slice(changed.descriptor()).unwrap();
    assert_ne!(
        a["compiled_inputs"]["job.json"],
        b["compiled_inputs"]["job.json"]
    );
    request.delivery_title.push_str(" revised");
    let title = freeze(&f, &request, "title-edit", &mut nodes);
    assert_eq!(
        changed.requirements().primary(),
        title.requirements().primary()
    );
    assert_ne!(changed.descriptor(), title.descriptor());
    let c: Value = serde_json::from_slice(title.descriptor()).unwrap();
    assert_ne!(
        b["compiled_inputs"]["production.json"],
        c["compiled_inputs"]["production.json"]
    );
}

#[test]
fn render_restore_rejects_wrong_request_before_missing_cache_and_refuses_metadata_only_mode() {
    let (f, mut request) = bilingual();
    let mut nodes = MemoryNodes::default();
    let context = freeze(&f, &request, "before", &mut nodes);
    let source = f.source();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    drop(source);
    fs::rename(
        f.root().join("cache/objects"),
        f.root().join("cache/offline-objects"),
    )
    .unwrap();
    request.language = "es".into();
    let wrong = f.root().join("wrong");
    let error = reel::tig_scene::restore_render_context(
        profile,
        &request,
        context.selection(),
        &wrong,
        &mut nodes,
        TreeLimits::default(),
    )
    .err()
    .unwrap();
    assert!(
        error
            .to_string()
            .contains("admitted request, mode or compiler")
    );
    assert!(!wrong.exists());
    request.language = "en".into();
    let source = f.source();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    assert!(
        reel::tig_scene::restore_render_context(
            profile,
            &request,
            context.selection(),
            &wrong,
            &mut nodes,
            TreeLimits::default()
        )
        .is_err()
    );
    assert!(!wrong.exists());
    fs::rename(
        f.root().join("cache/offline-objects"),
        f.root().join("cache/objects"),
    )
    .unwrap();
    let mut source = f.source();
    let metadata = reel::tig_scene::freeze_language_context(
        &mut source,
        &request,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let profile = archived_profile(&f, metadata.archive().clone(), source.checkpoint().clone());
    assert!(
        reel::tig_scene::restore_render_context(
            profile,
            &request,
            metadata.selection(),
            &wrong,
            &mut nodes,
            TreeLimits::default()
        )
        .is_err()
    );
    assert!(!wrong.exists());
}
