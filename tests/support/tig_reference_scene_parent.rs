//! Current-primary legacy scene proofs flow into an owned checked episode.
use super::*;
use reel::{
    scene_primary_reference::Budget,
    tig_episode::{
        self, CompositionFiles, CompositionJobPlan, CompositionOwnership, ProtectedScenes,
        ProtectionBudget,
    },
    tig_scene_production::jobs::reference::ReferencePlan,
};
use tig_git::source::native::job::Role;
use tig_store::source_claims::Command;

#[test]
#[ignore = "requires native FFmpeg; source-owned retained scenes assemble with old receipts offline"]
fn verified_scene_owners_assemble_current_parent_and_survive_independent_withdrawal() {
    exercise_repaired_master(Project::new());
}

#[test]
#[ignore = "requires native FFmpeg and a temporary synthetic episode"]
fn english_repaired_master_assembles_on_its_own_cue_clock() {
    let (bootstrap, request) = crate::language_scope::bilingual();
    exercise_repaired_master(Project::from_scene_fixture(bootstrap, request));
}

#[test]
#[ignore = "requires native FFmpeg and a temporary bilingual scene project"]
fn one_shared_cel_repairs_both_language_renders_with_exact_audio() {
    let (bootstrap, request) = crate::language_scope::bilingual();
    let mut p = Project::from_scene_fixture(bootstrap, request);
    let scene_path = "records/scene-scene-b.tig";
    let mut scene_record = read(p.repo.path().join(scene_path));
    let scene_pin: Object = serde_json::from_value(scene_record["selected"].clone()).unwrap();
    let mut scene = read(object_file(&p.hot(), &scene_pin));
    scene["languages"]["es"]["events"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    scene_record["selected"] = json!(p.publish("hot", &serde_json::to_vec(&scene).unwrap()));
    write(p.repo.path(), scene_path, &scene_record);
    p.commit("complete both language controls for the same scene");

    let mut baseline = Vec::new();
    for language in ["es", "en"] {
        let mut request: tig_scene::Request =
            serde_json::from_value(serde_json::to_value(&p.requests[2]).unwrap()).unwrap();
        request.language = language.into();
        let controls = p.data.path().join(format!("baseline-{language}"));
        let mut source = p.source();
        tig_scene::compile(&mut source, &request, &controls).unwrap();
        let path = controls.join("delivery/job.json");
        let job = reel::tig_inputs::read_job(&path).unwrap();
        assert_eq!(job.pictures.len(), 1);
        let output = controls.join("render");
        let inputs = source.prepare(&job).unwrap();
        reel::scene_delivery::render_with_inputs(&path, &p.programs.with_inputs(&inputs), &output)
            .unwrap();
        baseline.push((language, job, controls, output));
    }
    assert_ne!(
        baseline[0].1.audio[0].source.sha256,
        baseline[1].1.audio[0].source.sha256
    );
    assert_eq!(
        baseline[0].1.pictures[0].source,
        baseline[1].1.pictures[0].source
    );

    let new_picture = p.publish("hot", &picture([0, 255, 0]));
    let file = "records/cel-b.tig";
    let mut selected = read(p.repo.path().join(file));
    selected["selected"] = json!(new_picture);
    write(p.repo.path(), file, &selected);
    p.commit("select one shared repair");
    for (language, job, controls, original) in baseline {
        let candidate = reel::scene_delivery::repair::derive(
            &job,
            &reel::scene_delivery::repair::StillReplacement {
                schema: "reel.scene-still-replacement.v1".into(),
                attachments: vec![job.pictures[0].attachment_id.clone()],
                expected: job.pictures[0].source.clone(),
                replacement: reel::scene_delivery::FileRef {
                    path: job.pictures[0].source.path.clone(),
                    sha256: new_picture.sha256.as_str().into(),
                    bytes: new_picture.bytes,
                },
            },
        )
        .unwrap();
        let path = controls.join("delivery/job-repaired.json");
        fs::write(&path, serde_json::to_vec(&candidate.job).unwrap()).unwrap();
        let output = controls.join("repaired");
        let mut source = p.source();
        let inputs = source.prepare(&candidate.job).unwrap();
        reel::scene_delivery::render_with_inputs(&path, &p.programs.with_inputs(&inputs), &output)
            .unwrap();
        let before = read(original.join("receipt.json"));
        let after = read(output.join("receipt.json"));
        for field in ["content_samples", "delivery_frames"] {
            assert!(!before[field].is_null(), "{language}: {field}");
            assert_eq!(before[field], after[field], "{language}: {field}");
        }
        for name in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
            assert_eq!(
                Object::from_bytes(&fs::read(original.join(name)).unwrap()),
                Object::from_bytes(&fs::read(output.join(name)).unwrap()),
                "{language}: {name}"
            );
        }
        assert_ne!(
            Object::from_bytes(&fs::read(original.join("master.mkv")).unwrap()),
            Object::from_bytes(&fs::read(output.join("master.mkv")).unwrap()),
            "{language}: repair did not change the master"
        );
    }
}

fn exercise_repaired_master(mut p: Project) {
    let language = p.requests[2].language.clone();
    let mut original = read(p.repo.path().join("records/cel-b.tig"));
    let blue = json!(p.publish("hot", &picture([0, 0, 255])));
    if original["selected"] != blue {
        original["selected"] = blue;
        write(p.repo.path(), "records/cel-b.tig", &original);
        p.commit("declare unique original cel b");
    }
    let mut h264_scene = read(p.repo.path().join("records/scene-scene-b.tig"));
    let profile = h264_scene["streams"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|stream| stream["slot"] == p.requests[2].controls.profile)
        .unwrap();
    let old_profile: Object = serde_json::from_value(profile["object"].clone()).unwrap();
    let mut current_profile = read(object_file(&p.hot(), &old_profile));
    current_profile["still_sequence_encoding"] = json!("h264-lossless");
    profile["object"] = json!(p.publish("hot", &serde_json::to_vec(&current_profile).unwrap()));
    write(p.repo.path(), "records/scene-scene-b.tig", &h264_scene);
    p.commit("explicit lossless H264 scene among FFV1 peers");
    let scenes = (0..4)
        .map(|index| p.render_delivery(index, "v1"))
        .collect::<Vec<_>>();
    p.assemble(&scenes, "v1");
    let request = add_episode_assembly(&mut p, "episode-v1.json");
    let mut cel = read(p.repo.path().join("records/cel-b.tig"));
    cel["selected"] = json!(p.publish("hot", &picture([0, 255, 0])));
    write(p.repo.path(), "records/cel-b.tig", &cel);
    p.commit("change one primary cel without changing parent conform");
    // Render through the workflow still-control transform. The episode must
    // verify the selected master from current primary inputs and controls.
    let old = &scenes[2];
    let job = reel::tig_inputs::read_job(&old.job).unwrap();
    assert_eq!(
        job.still_sequence_encoding.as_deref(),
        Some("h264-lossless")
    );
    assert_eq!(job.pictures.len(), 1);
    let new_picture = picture([0, 255, 0]);
    let candidate = reel::scene_delivery::repair::derive(
        &job,
        &reel::scene_delivery::repair::StillReplacement {
            schema: "reel.scene-still-replacement.v1".into(),
            attachments: vec![job.pictures[0].attachment_id.clone()],
            expected: job.pictures[0].source.clone(),
            replacement: reel::scene_delivery::FileRef {
                path: job.pictures[0].source.path.clone(),
                sha256: Object::from_bytes(&new_picture).sha256.as_str().into(),
                bytes: new_picture.len() as u64,
            },
        },
    )
    .unwrap();
    let candidate_job = old.job.with_file_name("job-repaired.json");
    fs::write(&candidate_job, serde_json::to_vec(&candidate.job).unwrap()).unwrap();
    let repaired_output = p.data.path().join("workflow-repaired-scene");
    let mut repaired_source = p.source();
    let inputs = repaired_source.prepare(&candidate.job).unwrap();
    reel::scene_delivery::render_with_inputs(
        &candidate_job,
        &p.programs.with_inputs(&inputs),
        &repaired_output,
    )
    .unwrap();
    drop(inputs);
    let original_receipt = read(old.output.join("receipt.json"));
    let repaired_receipt = read(repaired_output.join("receipt.json"));
    for field in ["content_samples", "delivery_frames"] {
        assert!(!original_receipt[field].is_null());
        assert_eq!(original_receipt[field], repaired_receipt[field]);
    }
    for name in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
        assert_eq!(
            Object::from_bytes(&fs::read(old.output.join(name)).unwrap()),
            Object::from_bytes(&fs::read(repaired_output.join(name)).unwrap())
        );
    }
    let master = p.publish(
        "hot",
        &fs::read(repaired_output.join("master.mkv")).unwrap(),
    );
    let record_path = format!("records/output-{}.tig", UNITS[2]);
    let mut record = read(p.repo.path().join(&record_path));
    record["selected"] = json!(master);
    record["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("production");
    write(p.repo.path(), &record_path, &record);
    p.commit("select current scene b");
    let parent_files = parent_controls::exercise(&p, &request);
    let mut source = p.source();
    let initial = tig_episode::plan(&mut source, &request).unwrap();
    assert_eq!(initial.language, language);
    assert_eq!(initial.changed_components, 1);
    let metadata = p.data.path().join("reference-parent-metadata");
    let mut writer = MetadataStore::create(
        &metadata,
        tig_store::metadata::Limits {
            nodes: 32768,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let contexts = p
        .requests
        .iter()
        .map(|request| {
            tig_scene::freeze_context(&mut source, request, &mut writer, TreeLimits::default())
                .unwrap()
        })
        .collect::<Vec<_>>();
    let native = p
        .requests
        .iter()
        .zip(&contexts)
        .map(|(request, context)| {
            JobPlan::from_context(&mut source, request, context, &p.programs).unwrap()
        })
        .collect::<Vec<_>>();
    let preparation = p.data.path().join("reference-current-controls");
    fs::create_dir(&preparation).unwrap();
    let references = native
        .iter()
        .enumerate()
        .map(|(index, native)| {
            ReferencePlan::compile(
                &mut source,
                native,
                &format!("output/{}", UNITS[index]),
                "types/output",
                &preparation.join(UNITS[index]),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let resources = references
        .iter()
        .map(|plan| plan.freeze(&mut writer, TreeLimits::default()).unwrap())
        .collect::<Vec<_>>();
    let context = tig_episode::freeze_composition_context(
        &mut source,
        &request,
        &mut writer,
        TreeLimits::default(),
    )
    .unwrap();
    let checkpoint = writer.finish().unwrap();
    drop(writer);
    let mut reader = MetadataReader::open(&metadata, &checkpoint).unwrap();
    let mut journal =
        SourceJournal::open(&p.data.path().join("owners"), &p.owners.checkpoint()).unwrap();
    let coordination = Coordination {
        root: p.data.path().join("coordination"),
    };
    let budget = ProtectionBudget {
        max_objects: 10000,
        max_logical_bytes: 1024 * 1024 * 1024,
    };
    let owners = references
        .iter()
        .zip(&resources)
        .enumerate()
        .map(|(index, (plan, resource))| {
            plan.readback(resource.selection(), &mut reader, TreeLimits::default())
                .unwrap()
                .protect(
                    &source,
                    &mut journal,
                    &coordination,
                    Identity {
                        source: key(&format!("reference-{index}")),
                        owner: key("fixture"),
                    },
                    0,
                    &key("adopt-reference"),
                    &budget,
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    drop(reader);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            p.repo.path().join(name),
            p.repo.path().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    fs::rename(
        p.data.path().join("jobs"),
        p.data.path().join("offline-jobs"),
    )
    .unwrap();
    fs::rename(&preparation, p.data.path().join("offline-current-controls")).unwrap();
    let stage = p.data.path().join("reference-stage");
    fs::create_dir(&stage).unwrap();
    let comparison_budget = Budget {
        max_staged_bytes: 16 * 1024 * 1024,
        timeout_seconds: 120,
        max_picture_processes: 16,
    };
    let proofs = owners
        .iter()
        .enumerate()
        .map(|(index, owner)| {
            (
                UNITS[index].to_owned(),
                owner
                    .verify_selected_master(&mut source, &mut journal, &stage, &comparison_budget)
                    .unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
    let presentations = BTreeMap::new();
    let empty = BTreeMap::new();
    assert!(
        CompositionJobPlan::from_verified_components(
            &mut source,
            &request,
            &context,
            &p.programs,
            &empty,
            &presentations
        )
        .is_err()
    );
    let plan = CompositionJobPlan::from_verified_components(
        &mut source,
        &request,
        &context,
        &p.programs,
        &proofs,
        &presentations,
    )
    .unwrap();
    let mut writer = MetadataStore::open(&metadata, &checkpoint).unwrap();
    let frozen = plan.freeze(&mut writer, TreeLimits::default()).unwrap();
    let completed = writer.finish().unwrap();
    drop(writer);
    let mut reader = MetadataReader::open(&metadata, &completed).unwrap();
    assert!(frozen.of(Role::Primary).is_empty());
    for proof in proofs.values() {
        assert!(proof.resources().objects().is_subset(frozen.objects()));
    }
    assert!(!frozen.objects().contains(old.subject.subject().object()));
    let children = ProtectedScenes::without_legacy_jobs(&source);
    assert!(
        plan.readback(frozen.selection(), &mut reader, TreeLimits::default())
            .unwrap()
            .protect(
                &source,
                &children,
                &mut journal,
                &CompositionOwnership {
                    coordination: &coordination,
                    identity: Identity {
                        source: key("reference-2"),
                        owner: key("fixture")
                    },
                    expected_revision: 2,
                    budget: &budget,
                },
                &key("refused-parent-child-alias")
            )
            .is_err()
    );
    fs::rename(
        object_file(&p.hot(), old.subject.subject().object()),
        p.data.path().join("offline-old-master.mkv"),
    )
    .unwrap();
    let parent = plan
        .readback(frozen.selection(), &mut reader, TreeLimits::default())
        .unwrap()
        .protect(
            &source,
            &children,
            &mut journal,
            &CompositionOwnership {
                coordination: &coordination,
                identity: Identity {
                    source: key("reference-parent"),
                    owner: key("fixture"),
                },
                expected_revision: 0,
                budget: &budget,
            },
            &key("adopt-reference-parent"),
        )
        .unwrap();
    let overrides = BTreeMap::from([("scene-b".into(), &parent_files as &dyn InputFiles)]);
    let refused = p.data.path().join("refused-scene-override");
    assert!(
        parent
            .assemble(
                &source,
                &children,
                &mut journal,
                &refused,
                CompositionFiles {
                    parent: &parent_files,
                    scenes: &BTreeMap::new(),
                    inputs: &BTreeMap::new(),
                    outputs: &overrides
                }
            )
            .is_err()
    );
    assert!(!refused.exists());
    let rendered = parent
        .assemble(
            &source,
            &children,
            &mut journal,
            &p.data.path().join("reference-parent-render"),
            CompositionFiles {
                parent: &parent_files,
                scenes: &BTreeMap::new(),
                inputs: &BTreeMap::new(),
                outputs: &BTreeMap::new(),
            },
        )
        .unwrap();
    let receipt = rendered.receipt();
    assert!(receipt.decoded_master_matches_ordered_segments && receipt.timestamps_verified);
    assert_eq!(receipt.total_frames, 96);
    assert!(receipt.segments.iter().all(
        |segment| segment.upstream_delivery_verified && !segment.upstream_presentation_verified
    ));
    drop(rendered);
    journal
        .apply(
            key("withdraw-one-scene"),
            Command::Withdraw {
                source: key("reference-2"),
                owner: key("fixture"),
                expected_revision: 2,
            },
        )
        .unwrap();
    assert!(
        parent
            .verify_current(&source, &children, &mut journal)
            .is_err()
    );
    proofs["scene-a"]
        .verify_current(&source, &mut journal)
        .unwrap();
    assert!(
        proofs["scene-b"]
            .verify_current(&source, &mut journal)
            .is_err()
    );
}
