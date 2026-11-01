//! Sparse edit -> current native metadata -> parent render, without an edited conform.
use super::*;
use reel::tig_episode::{
    self, CompositionFiles, CompositionJobPlan, CompositionOwnership, CurrentSceneControls,
    ProtectionBudget, SceneAdmission,
};
use tig_core::tree::{MemoryNodes, NodeStore};
use tig_git::source::native::job::Role;

#[test]
#[ignore = "requires native FFmpeg; changed scene compiles exact current parent controls"]
fn sparse_scene_edit_compiles_parent_without_manual_conform_or_old_movie() {
    let mut p = Project::new();
    let mut leaf = read(p.repo.path().join("records/cel-b.tig"));
    let blue = json!(p.publish("hot", &picture([0, 0, 255])));
    if leaf["selected"] != blue {
        leaf["selected"] = blue;
        write(p.repo.path(), "records/cel-b.tig", &leaf);
        p.commit("declare unique original cel b");
    }
    let mut scenes = (0..4)
        .map(|index| p.render_delivery(index, "v1"))
        .collect::<Vec<_>>();
    p.assemble(&scenes, "v1");
    // Retained legacy witnesses are not the new sealed native proofs. They
    // must be cleared during replacement without hydrating absent evidence.
    let baseline_path = p.data.path().join("episode-v1.json");
    let mut baseline = read(&baseline_path);
    for segment in baseline["segments"].as_array_mut().unwrap() {
        segment["master_proof"] =
            json!({"path":"legacy/proof.json","sha256":"a".repeat(64),"bytes":17});
        segment["master_proof_validator"] =
            json!({"path":"legacy/validator.exe","sha256":"b".repeat(64),"bytes":19});
    }
    baseline["segments"][0]["tail_hold_frames"] = json!(1);
    baseline["output_frame_rate"] = json!({"numerator":24,"denominator":1,"policy":"repeat-only"});
    baseline["audio_frame_conform"] = json!("pad-silence-to-picture-boundaries");
    fs::write(&baseline_path, serde_json::to_vec(&baseline).unwrap()).unwrap();
    let request = add_episode_assembly(&mut p, "episode-v1.json");
    let original_conform = tig_episode::plan(&mut p.source(), &request)
        .unwrap()
        .conform;
    leaf["selected"] = json!(p.publish("hot", &picture([0, 255, 0])));
    write(p.repo.path(), "records/cel-b.tig", &leaf);
    p.commit("fix one cel without editing the episode conform");
    let changed = p.render_delivery(2, "v2");
    let old = std::mem::replace(&mut scenes[2], changed);
    p.commit("select changed native scene output");
    let parent_files = parent_controls::exercise(&p, &request);
    let mut source = p.source();
    let planned = tig_episode::plan(&mut source, &request).unwrap();
    assert_eq!(planned.conform, original_conform);
    assert_eq!(planned.changed_components, 1);
    let mut nodes = MemoryNodes::default();
    let contexts = p
        .requests
        .iter()
        .map(|request| {
            tig_scene::freeze_context(&mut source, request, &mut nodes, TreeLimits::default())
                .unwrap()
        })
        .collect::<Vec<_>>();
    let plans = p
        .requests
        .iter()
        .zip(&contexts)
        .map(|(request, context)| {
            JobPlan::from_context(&mut source, request, context, &p.programs).unwrap()
        })
        .collect::<Vec<_>>();
    let resources = plans
        .iter()
        .map(|plan| plan.freeze(&mut nodes, TreeLimits::default()).unwrap())
        .collect::<Vec<_>>();
    let proofs = scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| {
            (
                scene.unit.clone(),
                scene_correspondence(&p, scene, index, &format!("output/{}", scene.unit)).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let profile: tig_store::existing::Profile =
        serde_json::from_value(p.profile["byte_source"].clone()).unwrap();
    let outputs = scenes
        .iter()
        .map(|scene| {
            reel::tig_scene_outputs::Bundle::open(profile.clone(), &scene.delivery_receipt).unwrap()
        })
        .collect::<Vec<_>>();
    let old_outputs =
        reel::tig_scene_outputs::Bundle::open(profile.clone(), &old.delivery_receipt).unwrap();
    let refused = p.data.path().join("refused-old-receipt");
    assert!(
        CurrentSceneControls::compile(
            &mut source,
            &proofs["scene-b"],
            &plans[2],
            &old_outputs,
            &refused
        )
        .is_err()
    );
    assert!(!refused.exists());
    drop(old_outputs);
    let refused = p.data.path().join("refused-wrong-job");
    assert!(
        CurrentSceneControls::compile(
            &mut source,
            &proofs["scene-b"],
            &plans[0],
            &outputs[2],
            &refused
        )
        .is_err()
    );
    assert!(!refused.exists());
    let primary = scenes
        .iter()
        .map(|scene| {
            source
                .prepare(&reel::tig_inputs::read_job(&scene.job).unwrap())
                .unwrap()
        })
        .collect::<Vec<_>>();
    let primary_runtime = primary
        .iter()
        .map(|input| p.programs.with_inputs(input))
        .collect::<Vec<_>>();
    let inputs = scenes
        .iter()
        .zip(&primary_runtime)
        .map(|(scene, input)| (scene.unit.clone(), input as &dyn InputFiles))
        .collect();
    let output_files = scenes
        .iter()
        .zip(&outputs)
        .map(|(scene, output)| (scene.unit.clone(), output as &dyn InputFiles))
        .collect();
    let context = tig_episode::freeze_composition_context(
        &mut source,
        &request,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert!(
        CompositionJobPlan::from_current_scenes(
            &mut source,
            &request,
            &context,
            &p.programs,
            &proofs
        )
        .is_err()
    );
    for name in [".git", "records", "definitions"] {
        fs::rename(
            p.repo.path().join(name),
            p.repo.path().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let preparation = p.data.path().join("current-native-controls");
    fs::create_dir(&preparation).unwrap();
    let mut controls = scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| {
            let value = CurrentSceneControls::compile(
                &mut source,
                &proofs[&scene.unit],
                &plans[index],
                &outputs[index],
                &preparation.join(&scene.unit),
            )
            .unwrap();
            (scene.unit.clone(), value)
        })
        .collect::<BTreeMap<_, _>>();
    let presentations = BTreeMap::new();
    let control = controls.remove("scene-b").unwrap();
    controls.insert("unrelated-scene".into(), control);
    assert!(
        CompositionJobPlan::from_current_component_controls(
            &mut source,
            &request,
            &context,
            &p.programs,
            &proofs,
            &presentations,
            &controls
        )
        .is_err()
    );
    let control = controls.remove("unrelated-scene").unwrap();
    controls.insert("scene-b".into(), control);
    let plan = CompositionJobPlan::from_current_component_controls(
        &mut source,
        &request,
        &context,
        &p.programs,
        &proofs,
        &presentations,
        &controls,
    )
    .unwrap();
    let candidate = plan.freeze(&mut nodes, TreeLimits::default()).unwrap();
    assert!(candidate.of(Role::Primary).is_empty());
    assert!(!candidate.objects().contains(old.subject.subject().object()));
    let missing = nodes
        .objects()
        .map(|(_, raw)| (Object::from_bytes(raw), raw))
        .find(|(object, raw)| {
            candidate.of(Role::Evidence).contains(object)
                && raw.windows(8).any(|part| part == b"current/")
        })
        .unwrap()
        .0;
    let mut damaged = MemoryNodes::default();
    for (_, raw) in nodes.objects() {
        if Object::from_bytes(raw) != missing {
            damaged.put(raw).unwrap();
        }
    }
    assert!(
        plan.readback(candidate.selection(), &mut damaged, TreeLimits::default())
            .is_err()
    );
    fs::remove_file(object_file(&p.hot(), old.subject.subject().object())).unwrap();
    fs::rename(
        p.data.path().join("jobs"),
        p.data.path().join("offline-jobs"),
    )
    .unwrap();
    fs::rename(&preparation, p.data.path().join("offline-native-controls")).unwrap();
    let mut journal =
        SourceJournal::open(&p.data.path().join("owners"), &p.owners.checkpoint()).unwrap();
    let coordination = Coordination {
        root: p.data.path().join("coordination"),
    };
    let budget = ProtectionBudget {
        max_objects: 10000,
        max_logical_bytes: 1024 * 1024 * 1024,
    };
    let admissions = plans
        .iter()
        .zip(&resources)
        .enumerate()
        .map(|(index, (plan, resource))| {
            (
                UNITS[index].to_owned(),
                SceneAdmission {
                    readback: plan
                        .readback(resource.selection(), &mut nodes, TreeLimits::default())
                        .unwrap(),
                    identity: Identity {
                        source: key(&format!("current-child-{index}")),
                        owner: key("fixture"),
                    },
                    expected_revision: 0,
                    operation: key("adopt-current-scene"),
                },
            )
        })
        .collect();
    let children = tig_episode::protect_current_scenes(
        &mut source,
        &request,
        &proofs,
        admissions,
        &mut journal,
        &coordination,
        &budget,
    )
    .unwrap();
    let parent = plan
        .readback(candidate.selection(), &mut nodes, TreeLimits::default())
        .unwrap()
        .protect(
            &source,
            &children,
            &mut journal,
            &CompositionOwnership {
                coordination: &coordination,
                identity: Identity {
                    source: key("current-parent"),
                    owner: key("fixture"),
                },
                expected_revision: 0,
                budget: &budget,
            },
            &key("adopt-current-parent"),
        )
        .unwrap();
    let overrides = BTreeMap::from([(
        "scene-b".into(),
        episode_conform::SceneControls {
            files: &parent_files as &dyn InputFiles,
            production: parent_files.manifest_ref(),
        },
    )]);
    let refused = p.data.path().join("refused-caller-scene-override");
    assert!(
        parent
            .assemble(
                &source,
                &children,
                &mut journal,
                &refused,
                CompositionFiles {
                    parent: &parent_files,
                    scenes: &overrides,
                    inputs: &inputs,
                    outputs: &output_files
                }
            )
            .is_err()
    );
    assert!(!refused.exists());
    let output = p.data.path().join("current-parent-render");
    let rendered = parent
        .assemble(
            &source,
            &children,
            &mut journal,
            &output,
            CompositionFiles {
                parent: &parent_files,
                scenes: &BTreeMap::new(),
                inputs: &inputs,
                outputs: &output_files,
            },
        )
        .unwrap();
    assert!(rendered.receipt().decoded_master_matches_ordered_segments);
    let held = &rendered.receipt().segments[0];
    assert_eq!(held.tail_hold_frames, 1);
    assert_eq!(held.frames, held.input_frames + 1);
    assert!(held.tail_hold_samples > 0);
    assert!(rendered.receipt().timestamps_verified);
    assert_eq!(rendered.receipt().total_frames, 97);
    assert!(!object_file(&p.hot(), old.subject.subject().object()).exists());
}
