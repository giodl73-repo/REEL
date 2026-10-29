//! Generated real assembly under independent parent and exact child obligations.
use super::*;
use reel::tig_episode::{
    self, CompositionJobPlan, CompositionOwnership, CurrentScene, ProtectedScenes, ProtectionBudget,
};
use tig_git::source::native::job::Role;
use tig_store::source_claims::Command as ClaimCommand;

pub(super) fn exercise(
    p: &Project,
    request: &tig_episode::Request,
    proofs: &BTreeMap<String, CurrentScene>,
    children: &ProtectedScenes<'_, '_>,
    journal: &mut SourceJournal,
    scenes: &[Scene],
    providers: &BTreeMap<String, &dyn InputFiles>,
) {
    let root = p.data.path().join("episode-parent-context");
    let mut writer = MetadataStore::create(
        &root,
        tig_store::metadata::Limits {
            nodes: 32768,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let context = tig_episode::freeze_composition_context(
        &mut p.source(),
        request,
        &mut writer,
        TreeLimits::default(),
    )
    .unwrap();
    let plan = CompositionJobPlan::from_current_scenes(
        &mut p.source(),
        request,
        &context,
        &p.programs,
        proofs,
    )
    .unwrap();
    let candidate = plan.freeze(&mut writer, TreeLimits::default()).unwrap();
    let different_proofs = alternate_checker_proofs(p, scenes);
    let different_plan = CompositionJobPlan::from_current_scenes(
        &mut p.source(),
        request,
        &context,
        &p.programs,
        &different_proofs,
    )
    .unwrap();
    let different_candidate = different_plan
        .freeze(&mut writer, TreeLimits::default())
        .unwrap();
    assert_ne!(
        candidate.selection(),
        different_candidate.selection(),
        "a different child ownership recipe must change the durable parent job"
    );
    let checkpoint = writer.finish().unwrap();
    drop(writer);
    let readback = || {
        plan.readback(
            candidate.selection(),
            &mut MetadataReader::open(&root, &checkpoint).unwrap(),
            TreeLimits::default(),
        )
        .unwrap()
    };
    assert!(
        different_plan
            .readback(
                candidate.selection(),
                &mut MetadataReader::open(&root, &checkpoint).unwrap(),
                TreeLimits::default()
            )
            .is_err(),
        "child checker requirements cannot be silently substituted on replay"
    );
    assert!(
        candidate.of(Role::Primary).is_empty(),
        "movies cannot become primary inputs"
    );
    assert!(candidate.of(Role::ClaimedOutput).is_empty());
    assert_eq!(
        candidate.of(Role::Composition),
        &scenes
            .iter()
            .map(|s| s.subject.subject().object().clone())
            .collect()
    );
    let original_claims: BTreeSet<_> = proofs.values().map(|p| p.claim().clone()).collect();
    assert!(original_claims.is_subset(candidate.of(Role::Evidence)));
    assert_eq!(
        candidate.of(Role::Evidence).len(),
        original_claims.len() + 1,
        "retain the child-job contract as well as original claims"
    );
    assert!(!candidate.of(Role::Runtime).is_empty());
    let mut substituted = candidate.selection().clone();
    substituted.context = Object::from_bytes(b"foreign parent identity").sha256;
    assert!(
        plan.readback(
            &substituted,
            &mut MetadataReader::open(&root, &checkpoint).unwrap(),
            TreeLimits::default()
        )
        .is_err()
    );
    assert!(
        plan.readback(
            candidate.selection(),
            &mut tig_core::tree::MemoryNodes::default(),
            TreeLimits::default()
        )
        .is_err(),
        "missing actual runtime manifest refuses"
    );
    let coordination = Coordination {
        root: p.data.path().join("coordination"),
    };
    let budget = ProtectionBudget {
        max_objects: 1024,
        max_logical_bytes: 1024 * 1024 * 1024,
    };
    let identity = Identity {
        source: key("parent-assembly"),
        owner: key("parent-owner"),
    };
    let snapshot = p.source();
    let unchanged = journal.checkpoint();
    for case in ["objects", "bytes", "same-source"] {
        let mut limited = ProtectionBudget {
            max_objects: budget.max_objects,
            max_logical_bytes: budget.max_logical_bytes,
        };
        let mut owner = identity.clone();
        match case {
            "objects" => limited.max_objects = 1,
            "bytes" => limited.max_logical_bytes = 1,
            _ => owner.source = key("child-0"),
        }
        let ownership = CompositionOwnership {
            coordination: &coordination,
            identity: owner,
            expected_revision: 0,
            budget: &limited,
        };
        assert!(
            readback()
                .protect(
                    &snapshot,
                    children,
                    journal,
                    &ownership,
                    &key("parent-adopt")
                )
                .is_err(),
            "{case}"
        );
        assert_eq!(
            journal.checkpoint(),
            unchanged,
            "preflight mutated source journal: {case}"
        );
    }
    let mut ownership = CompositionOwnership {
        coordination: &coordination,
        identity,
        expected_revision: 0,
        budget: &budget,
    };
    assert!(
        different_plan
            .readback(
                different_candidate.selection(),
                &mut MetadataReader::open(&root, &checkpoint).unwrap(),
                TreeLimits::default()
            )
            .unwrap()
            .protect(
                &snapshot,
                children,
                journal,
                &ownership,
                &key("different-child-job")
            )
            .is_err()
    );
    assert_eq!(
        journal.checkpoint(),
        unchanged,
        "same snapshot with different child jobs must refuse before adoption"
    );
    let parent = readback()
        .protect(
            &snapshot,
            children,
            journal,
            &ownership,
            &key("parent-adopt"),
        )
        .unwrap();
    assert_eq!(parent.revision(), 2);
    assert_eq!(parent.resources().selection(), candidate.selection());
    assert!(parent.logical_bytes() > children.logical_bytes());
    // A parent-only-sized limit must not omit independent child obligations.
    let parent_only_budget = ProtectionBudget {
        max_objects: 1024,
        max_logical_bytes: candidate.logical_bytes(),
    };
    let limited = CompositionOwnership {
        coordination: &coordination,
        identity: Identity {
            source: key("parent-too-small"),
            owner: key("parent-owner"),
        },
        expected_revision: 0,
        budget: &parent_only_budget,
    };
    assert!(
        readback()
            .protect(&snapshot, children, journal, &limited, &key("too-small"))
            .is_err()
    );
    // Independent workers may own the same plan. Their candidate capabilities
    // must nevertheless remain bound to the actual producing job guard.
    let peer = readback()
        .protect(
            &snapshot,
            children,
            journal,
            &CompositionOwnership {
                coordination: &coordination,
                identity: Identity {
                    source: key("parent-peer"),
                    owner: key("parent-owner"),
                },
                expected_revision: 0,
                budget: &budget,
            },
            &key("parent-peer-adopt"),
        )
        .unwrap();
    let held = journal.checkpoint();
    assert!(
        readback()
            .resume(&snapshot, children, journal, &ownership)
            .is_err(),
        "parent per-source fence must remain held"
    );
    assert_eq!(journal.checkpoint(), held);
    // An unrelated native worker's fence remains available while parent/children
    // are held (the caller fixture already owns an independent peer job).
    let control_files = parent_controls::exercise(p, request);
    let primary = scenes
        .iter()
        .map(|scene| {
            let job: Job = serde_json::from_slice(&fs::read(&scene.job).unwrap()).unwrap();
            p.source().prepare(&job).unwrap()
        })
        .collect::<Vec<_>>();
    let primary_runtime = primary
        .iter()
        .map(|inputs| p.programs.with_inputs(inputs))
        .collect::<Vec<_>>();
    let inputs = scenes
        .iter()
        .zip(&primary_runtime)
        .map(|(scene, inputs)| (scene.unit.clone(), inputs as &dyn InputFiles))
        .collect();
    let local = PathBuf::from(".");
    let programs = p.programs.with_inputs(&local);
    let output = p.data.path().join("episode-protected-cached-episode");
    struct Refuse<'a> {
        inner: &'a tig_episode::ParentControls,
        spoof: bool,
    }
    impl InputFiles for Refuse<'_> {
        fn verified_path(&self, reference: &FileRef) -> anyhow::Result<PathBuf> {
            if reference.path == self.inner.manifest().catalog.path {
                if self.spoof {
                    return self.inner.verified_path(self.inner.manifest_ref());
                }
                anyhow::bail!("fixture parent control provider refused");
            }
            self.inner.verified_path(reference)
        }
    }
    assert!(
        p.data
            .path()
            .join(&control_files.manifest().catalog.path)
            .is_file(),
        "a valid local copy must not rescue an explicit provider refusal"
    );
    for spoof in [false, true] {
        let refused_output = p
            .data
            .path()
            .join(format!("parent-control-refused-{spoof}"));
        let error = episode_conform::build_with_parent_controls(
            control_files.manifest_ref(),
            p.data.path(),
            p.data.path(),
            &refused_output,
            episode_conform::Files {
                scene_inputs: &inputs,
                scene_outputs: providers,
                presentations: &BTreeMap::new(),
                programs: &programs,
            },
            &Refuse {
                inner: &control_files,
                spoof,
            },
        )
        .unwrap_err();
        assert!(
            error.to_string().contains(if spoof {
                "differs from its admitted pin"
            } else {
                "provider refused"
            }),
            "{error:#}"
        );
        assert!(!refused_output.exists());
    }
    let source_controls = parent_controls::scene_controls(p, &control_files);
    let scene_controls = source_controls.borrowed();
    let mut incomplete = source_controls.borrowed();
    incomplete.remove("scene-b");
    let refused = p.data.path().join("incomplete-scene-controls");
    assert!(
        episode_conform::build_with_all_control_files(
            control_files.manifest_ref(),
            p.data.path(),
            p.data.path(),
            &refused,
            episode_conform::Files {
                scene_inputs: &inputs,
                scene_outputs: providers,
                presentations: &BTreeMap::new(),
                programs: &programs
            },
            &control_files,
            &incomplete,
        )
        .is_err()
    );
    assert!(!refused.exists());
    let mut wrong_production = source_controls.production["opening"].clone();
    wrong_production.path = "wrong-scope/production.json".into();
    let mut wrong_scope = source_controls.borrowed();
    wrong_scope.get_mut("opening").unwrap().production = &wrong_production;
    let refused = p.data.path().join("wrong-production-control-scope");
    let error = episode_conform::build_with_all_control_files(
        control_files.manifest_ref(),
        p.data.path(),
        p.data.path(),
        &refused,
        episode_conform::Files {
            scene_inputs: &inputs,
            scene_outputs: providers,
            presentations: &BTreeMap::new(),
            programs: &programs,
        },
        &control_files,
        &wrong_scope,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("production metadata differs from declared scene scope"),
        "{error:#}"
    );
    assert!(!refused.exists());
    struct DenyScene<'a>(&'a dyn InputFiles);
    impl InputFiles for DenyScene<'_> {
        fn verified_path(&self, reference: &FileRef) -> anyhow::Result<PathBuf> {
            if reference
                .path
                .to_string_lossy()
                .contains("semantic-delivery")
            {
                anyhow::bail!("scene control provider refused");
            }
            self.0.verified_path(reference)
        }
    }
    let denied = DenyScene(&source_controls.files["scene-b"]);
    let mut explicit_refusal = source_controls.borrowed();
    explicit_refusal.get_mut("scene-b").unwrap().files = &denied;
    let refused = p.data.path().join("denied-scene-controls");
    let error = episode_conform::build_with_all_control_files(
        control_files.manifest_ref(),
        p.data.path(),
        p.data.path(),
        &refused,
        episode_conform::Files {
            scene_inputs: &inputs,
            scene_outputs: providers,
            presentations: &BTreeMap::new(),
            programs: &programs,
        },
        &control_files,
        &explicit_refusal,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("scene control provider refused"),
        "{error:#}"
    );
    assert!(!refused.exists());
    // Both roots are deliberately absent. Any hidden local metadata read must
    // fail, while every source-control body remains in a protected cache tree.
    let absent = p.data.path().join("no-local-source-controls");
    assert!(!absent.exists());
    let original_repo = p.repo.path();
    let offline_repo = original_repo.with_extension("unavailable");
    assert!(!offline_repo.exists());
    fs::rename(original_repo, &offline_repo).unwrap();
    let result = parent.assemble(
        &snapshot,
        children,
        journal,
        &output,
        tig_episode::CompositionFiles {
            parent: &control_files,
            scenes: &scene_controls,
            inputs: &inputs,
            outputs: providers,
        },
    );
    fs::rename(&offline_repo, original_repo).unwrap();
    let episode_candidate = result.unwrap();
    let result = episode_candidate.receipt();
    assert!(result.decoded_master_matches_ordered_segments && result.timestamps_verified);
    assert_eq!((result.total_frames, result.total_samples), (96, 192000));
    assert!(result.master_bytes > 0 && output.join("master.mkv").is_file());
    super::episode_publication::exercise(
        p,
        &plan,
        &different_plan,
        (&root, &checkpoint),
        &parent,
        &peer,
        children,
        journal,
        &episode_candidate,
        &output,
        scenes,
    );
    drop(episode_candidate);
    let failed = parent.with_current::<()>(&snapshot, children, journal, |_| {
        anyhow::bail!("bounded action refused")
    });
    assert!(
        failed
            .unwrap_err()
            .to_string()
            .contains("bounded action refused")
    );
    parent.verify_current(&snapshot, children, journal).unwrap();
    drop(parent);
    assert_eq!(
        journal.checkpoint(),
        held,
        "Drop must not withdraw durable parent obligations"
    );
    assert!(
        readback()
            .resume(&snapshot, children, journal, &ownership)
            .is_err(),
        "wrong revision refuses without transition"
    );
    assert_eq!(journal.checkpoint(), held);
    ownership.expected_revision = 2;
    let resumed = readback()
        .resume(&snapshot, children, journal, &ownership)
        .unwrap();
    assert_eq!(journal.checkpoint(), held, "resume is read-only");
    let ran = std::cell::Cell::new(false);
    assert!(
        resumed
            .with_current(&snapshot, children, journal, |journal| {
                ran.set(true);
                journal.apply(
                    key("parent-withdraw"),
                    ClaimCommand::Withdraw {
                        source: ownership.identity.source.clone(),
                        owner: ownership.identity.owner.clone(),
                        expected_revision: 2,
                    },
                )?;
                Ok(())
            })
            .is_err()
    );
    assert!(
        ran.get(),
        "postcondition must detect withdrawal during the action"
    );
    ran.set(false);
    assert!(
        resumed
            .with_current(&snapshot, children, journal, |_| {
                ran.set(true);
                Ok(())
            })
            .is_err()
    );
    assert!(!ran.get(), "stale parent must refuse before action");
    drop(resumed);
    // Retry of a historical successful operation cannot mint a current guard.
    ownership.expected_revision = 0;
    assert!(
        readback()
            .protect(
                &snapshot,
                children,
                journal,
                &ownership,
                &key("parent-adopt")
            )
            .is_err()
    );
    let parent_state = journal.source(&ownership.identity.source).unwrap().unwrap();
    assert!(parent_state.current.is_none());
    assert!(
        parent_state
            .predecessors
            .iter()
            .any(|p| p.selection == *candidate.selection())
    );
    ownership.expected_revision = parent_state.revision;
    let restored = readback()
        .protect(
            &snapshot,
            children,
            journal,
            &ownership,
            &key("parent-recover"),
        )
        .unwrap();
    children.verify_current(&snapshot, journal).unwrap();
    assert!(
        restored
            .with_current(&snapshot, children, journal, |journal| {
                journal.apply(
                    key("parent-child-withdraw"),
                    ClaimCommand::Withdraw {
                        source: key("child-0"),
                        owner: key("episode-owner"),
                        expected_revision: 2,
                    },
                )?;
                Ok(())
            })
            .is_err(),
        "stale child must invalidate a successful parent action"
    );
    assert!(
        restored
            .verify_current(&snapshot, children, journal)
            .is_err()
    );
    let retained = journal.source(&ownership.identity.source).unwrap().unwrap();
    assert_eq!(
        retained.current.unwrap().selection,
        *candidate.selection(),
        "failed execution check must retain parent obligations"
    );
}

fn alternate_checker_proofs(p: &Project, scenes: &[Scene]) -> BTreeMap<String, CurrentScene> {
    // Same source, same retained outputs and claims; only one independently
    // admitted current checker's dependency declaration changes. No media render.
    let marker = p.data.path().join("checker-dependency.json");
    fs::write(
        &marker,
        b"{\"fixture\":\"independent checker declaration\"}",
    )
    .unwrap();
    let marker_pin = Object::from_bytes(&fs::read(&marker).unwrap());
    let mut bindings = p.runtime.clone();
    bindings.dependencies.insert(
        "fixture/checker".into(),
        FileRef {
            path: marker,
            sha256: marker_pin.sha256.as_str().into(),
            bytes: marker_pin.bytes,
        },
    );
    let checker = media_runtime::BoundPrograms::bind(&bindings).unwrap();
    let mut proofs = scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| {
            (
                scene.unit.clone(),
                scene_correspondence(p, scene, index, &format!("output/{}", scene.unit)).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let scene = &scenes[0];
    let mut original_nodes =
        MetadataReader::open(&scene.metadata, &scene.metadata_checkpoint).unwrap();
    let producer = Object::from_bytes(p.programs.manifest_bytes());
    let (_, original) = tig_scene::restore_claim(
        scene.archive.clone(),
        &p.requests[0],
        &producer,
        &scene.claim,
        &mut original_nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let mut current = p.source();
    let mut nodes = tig_core::tree::MemoryNodes::default();
    let context = tig_scene::freeze_context(
        &mut current,
        &p.requests[0],
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let job = JobPlan::from_context(&mut current, &p.requests[0], &context, &checker).unwrap();
    let proof = tig_episode::check_current_scene_with_retained_producer(
        &mut current,
        &format!("output/{}", scene.unit),
        &job,
        &original,
        &producer,
        None,
    )
    .unwrap();
    assert_eq!(proof.claim(), proofs[&scene.unit].claim());
    assert_eq!(proof.selected(), proofs[&scene.unit].selected());
    proofs.insert(scene.unit.clone(), proof);
    proofs
}
