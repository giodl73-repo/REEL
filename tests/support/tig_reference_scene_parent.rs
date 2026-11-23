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
    let (bootstrap, request) = crate::language_scope::bilingual_with_english_samples(60000);
    let mut p = Project::from_scene_fixture(bootstrap, request);
    for unit in UNITS {
        let scene_path = format!("records/scene-{unit}.tig");
        let mut scene_record = read(p.repo.path().join(&scene_path));
        let scene_pin: Object = serde_json::from_value(scene_record["selected"].clone()).unwrap();
        let mut scene = read(object_file(&p.hot(), &scene_pin));
        scene["languages"]["es"]["events"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        scene_record["selected"] = json!(p.publish("hot", &serde_json::to_vec(&scene).unwrap()));
        write(p.repo.path(), &scene_path, &scene_record);
    }
    p.commit("complete both language controls for each scene");

    let mut baseline = BTreeMap::new();
    let mut editions = BTreeMap::new();
    for language in ["es", "en"] {
        let mut scenes = Vec::new();
        for (index, unit) in UNITS.iter().enumerate() {
            let mut request: tig_scene::Request =
                serde_json::from_value(serde_json::to_value(&p.requests[index]).unwrap()).unwrap();
            request.language = language.into();
            let controls = p.data.path().join(format!("baseline-{language}-{unit}"));
            let mut source = p.source();
            tig_scene::compile(&mut source, &request, &controls).unwrap();
            let path = controls.join("delivery/job.json");
            let job = reel::tig_inputs::read_job(&path).unwrap();
            assert_eq!(job.pictures.len(), 1);
            let output = controls.join("render");
            let inputs = source.prepare(&job).unwrap();
            reel::scene_delivery::render_with_inputs(
                &path,
                &p.programs.with_inputs(&inputs),
                &output,
            )
            .unwrap();
            let semantic = path.with_file_name("semantic-delivery.json");
            scenes.push((path, output, semantic));
        }
        baseline.insert(language, shared_edition(&p, language, "baseline", &scenes));
        editions.insert(language, scenes);
    }
    let scene_jobs =
        ["es", "en"].map(|lang| reel::tig_inputs::read_job(&editions[lang][2].0).unwrap());
    assert_ne!(
        scene_jobs[0].audio[0].source.sha256,
        scene_jobs[1].audio[0].source.sha256
    );
    assert_ne!(baseline["es"].total_samples, baseline["en"].total_samples);
    assert_eq!(
        scene_jobs[0].pictures[0].source,
        scene_jobs[1].pictures[0].source
    );

    let new_picture = p.publish("hot", &picture([0, 255, 0]));
    let file = "records/cel-b.tig";
    let mut selected = read(p.repo.path().join(file));
    selected["selected"] = json!(new_picture);
    write(p.repo.path(), file, &selected);
    p.commit("select one shared repair");
    for (language, mut scenes) in editions {
        let (original_job, original, _) = &scenes[2];
        let job = reel::tig_inputs::read_job(original_job).unwrap();
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
        let mut request: tig_scene::Request =
            serde_json::from_value(serde_json::to_value(&p.requests[2]).unwrap()).unwrap();
        request.language = language.into();
        let current = p.data.path().join(format!("current-{language}"));
        let mut source = p.source();
        tig_scene::compile(&mut source, &request, &current).unwrap();
        let path = current.join("delivery/job-repaired.json");
        fs::write(&path, serde_json::to_vec(&candidate.job).unwrap()).unwrap();
        let semantic = current.join("delivery/semantic-delivery-repaired.json");
        let mut declaration = read(current.join("delivery/semantic-delivery.json"));
        declaration["scene_delivery_job"] = pin(path.parent().unwrap(), &path);
        fs::write(&semantic, serde_json::to_vec(&declaration).unwrap()).unwrap();
        let output = original.with_file_name("repaired");
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
        drop(inputs);
        scenes[2] = (path, output, semantic);
        let revised = shared_edition(&p, language, "repaired", &scenes);
        let original = &baseline[language];
        assert_eq!(original.total_frames, revised.total_frames);
        assert_eq!(original.total_samples, revised.total_samples);
        assert_ne!(original.master_sha256, revised.master_sha256);
        let changes: Vec<_> = original
            .segments
            .iter()
            .zip(&revised.segments)
            .filter(|(before, after)| before.selected_master_sha256 != after.selected_master_sha256)
            .map(|(_, segment)| segment.id.as_str())
            .collect();
        assert_eq!(changes, [UNITS[2]]);
        for name in ["captions.srt", "captions.vtt"] {
            let before = p
                .data
                .path()
                .join(format!("captions-{language}-baseline/{name}"));
            let after = p
                .data
                .path()
                .join(format!("captions-{language}-repaired/{name}"));
            assert_eq!(
                fs::read(before).unwrap(),
                fs::read(after).unwrap(),
                "{language}: {name}"
            );
        }
    }
}

fn shared_edition(
    project: &Project,
    language: &str,
    version: &str,
    scenes: &[(PathBuf, PathBuf, PathBuf)],
) -> episode_conform::Receipt {
    let root = project.data.path();
    let controls = root.join("controls");
    let mut owners = Vec::new();
    let mut segments = Vec::new();
    for ((job, output, semantic), unit) in scenes.iter().zip(UNITS) {
        let delivery_receipt = pin(root, &output.join("receipt.json"));
        segments.push(json!({
            "kind": "scene",
            "id": unit,
            "master": pin(root, &output.join("master.mkv")),
            "source_receipt": delivery_receipt,
            "delivery_job": pin(root, job),
            "delivery_receipt": delivery_receipt,
            "semantic_delivery": pin(root, semantic),
        }));
        let job = reel::tig_inputs::read_job(job).unwrap();
        owners.push(project.source().prepare(&job).unwrap());
    }
    let runtimes: Vec<_> = owners
        .iter()
        .map(|owner| project.programs.with_inputs(owner))
        .collect();
    let inputs = UNITS
        .iter()
        .zip(&runtimes)
        .map(|(unit, runtime)| {
            (
                (*unit).to_owned(),
                runtime as &dyn reel::scene_delivery::InputFiles,
            )
        })
        .collect();
    let manifest = json!({
        "schema": "reel.episode-conform.v1",
        "episode_id": "episode-1",
        "language": language,
        "output_sample_rate": 48000,
        "catalog": pin(root, &controls.join("catalog.json")),
        "master_template_definition": pin(root, &controls.join("master.json")),
        "episode": pin(root, &controls.join("episode.json")),
        "season_bindings": pin(root, &controls.join("season.json")),
        "episode_bindings": pin(root, &controls.join("episode-bindings.json")),
        "segments": segments,
    });
    let name = format!("episode-{language}-{version}");
    write(root, &format!("{name}.json"), &manifest);
    let conform = root.join(format!("{name}.json"));
    let rendered = root.join(name);
    let receipt =
        episode_conform::build_with_scene_inputs(&conform, root, root, &rendered, &inputs)
            .unwrap_or_else(|error| panic!("{language} {version} conform: {error:#}"));
    assert!(receipt.decoded_master_matches_ordered_segments);
    assert!(receipt.timestamps_verified);
    assert_eq!(receipt.language, language);
    assert_eq!(
        receipt
            .segments
            .iter()
            .map(|segment| segment.id.as_str())
            .collect::<Vec<_>>(),
        UNITS
    );
    let mut tracks = Vec::new();
    for (segment, (_, _, semantic)) in receipt.segments.iter().zip(scenes) {
        let text = match language {
            "es" => format!("Señal de {}", segment.id),
            "en" => format!("Cue for {}", segment.id),
            _ => unreachable!(),
        };
        let srt = root.join(format!("source-{language}-{version}-{}.srt", segment.id));
        fs::write(
            &srt,
            format!("1\n00:00:00,000 --> 00:00:00,500\n{text}\n\n"),
        )
        .unwrap();
        tracks.push(json!({
            "segment_id": segment.id,
            "language": language,
            "selected_master_sha256": segment.selected_master_sha256,
            "source_caption": pin(root, &srt),
            "source_words_sha256": Object::from_bytes(&serde_json::to_vec(&[text]).unwrap()).sha256,
            "selection_evidence": pin(root, semantic),
            "source_frame_rate": {"numerator":24,"denominator":1,"policy":"repeat-only"},
        }));
    }
    let caption_manifest = format!("captions-{language}-{version}.json");
    write(
        root,
        &caption_manifest,
        &json!({
            "schema":"reel.episode-captions.v1",
            "language": language,
            "conform_manifest": pin(root, &conform),
            "conform_receipt": pin(root, &rendered.join("receipt.json")),
            "tracks": tracks,
        }),
    );
    let captions = root.join(format!("captions-{language}-{version}"));
    reel::episode_captions::assemble(&root.join(caption_manifest), root, &captions).unwrap();
    let cues = reel::series::parse_srt(&fs::read_to_string(captions.join("captions.srt")).unwrap())
        .unwrap();
    assert_eq!(cues.len(), receipt.segments.len());
    for (cue, segment) in cues.iter().zip(&receipt.segments) {
        let start = (segment.start_sample * 1000).div_ceil(u64::from(receipt.output_sample_rate));
        assert_eq!(cue.start_ms, start);
    }
    receipt
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
