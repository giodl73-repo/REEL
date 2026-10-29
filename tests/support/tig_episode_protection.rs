//! Existing cache-only fixture; no closed global eviction-ledger assertion.
use super::*;
use reel::tig_episode::{self, ProtectionBudget, SceneAdmission};
use tig_git::source::native::job::Role;
use tig_store::source_claims::Command as ClaimCommand;

pub(super) fn exercise(
    p: &Project,
    request: &tig_episode::Request,
    scenes: &[Scene],
    providers: &BTreeMap<String, &dyn InputFiles>,
) {
    let root = p.data.path().join("episode-child-contexts");
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
    let contexts = p
        .requests
        .iter()
        .map(|request| {
            tig_scene::freeze_context(&mut p.source(), request, &mut writer, TreeLimits::default())
                .unwrap()
        })
        .collect::<Vec<_>>();
    let plans = p
        .requests
        .iter()
        .zip(&contexts)
        .map(|(request, context)| {
            JobPlan::from_context(&mut p.source(), request, context, &p.programs).unwrap()
        })
        .collect::<Vec<_>>();
    let candidates = plans
        .iter()
        .map(|plan| plan.freeze(&mut writer, TreeLimits::default()).unwrap())
        .collect::<Vec<_>>();
    let checkpoint = writer.finish().unwrap();
    drop(writer);
    let mut reader = MetadataReader::open(&root, &checkpoint).unwrap();
    let proofs = scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| {
            (
                scene.unit.clone(),
                scene_correspondence(p, scene, index, &format!("output/{}", scene.unit)).unwrap(),
            )
        })
        .collect();
    let mut journal =
        SourceJournal::open(&p.data.path().join("owners"), &p.owners.checkpoint()).unwrap();
    let coordination = Coordination {
        root: p.data.path().join("coordination"),
    };
    let budget = ProtectionBudget {
        max_objects: 1024,
        max_logical_bytes: 1024 * 1024 * 1024,
    };
    let mut children = |prefix: &str| {
        plans
            .iter()
            .zip(&candidates)
            .enumerate()
            .map(|(index, (plan, resource))| {
                (
                    UNITS[index].to_string(),
                    SceneAdmission {
                        readback: plan
                            .readback(resource.selection(), &mut reader, TreeLimits::default())
                            .unwrap(),
                        identity: Identity {
                            source: key(&format!("{prefix}-{index}")),
                            owner: key("episode-owner"),
                        },
                        expected_revision: 0,
                        operation: key("protect-episode-inputs"),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    let unchanged = journal.checkpoint();
    for case in [
        "missing",
        "wrong-job",
        "duplicate-source",
        "objects",
        "bytes",
    ] {
        let mut inputs = children("child");
        let mut limit = ProtectionBudget {
            max_objects: budget.max_objects,
            max_logical_bytes: budget.max_logical_bytes,
        };
        match case {
            "missing" => {
                inputs.remove("scene-b");
            }
            "wrong-job" => {
                inputs.get_mut("scene-b").unwrap().readback = plans[0]
                    .readback(
                        candidates[0].selection(),
                        &mut MetadataReader::open(&root, &checkpoint).unwrap(),
                        TreeLimits::default(),
                    )
                    .unwrap();
            }
            "duplicate-source" => {
                inputs.get_mut("scene-b").unwrap().identity.source = key("child-0")
            }
            "objects" => limit.max_objects = 1,
            _ => limit.max_logical_bytes = 1,
        }
        assert!(
            tig_episode::protect_current_scenes(
                &mut p.source(),
                request,
                &proofs,
                inputs,
                &mut journal,
                &coordination,
                &limit
            )
            .is_err(),
            "{case}"
        );
        assert_eq!(
            journal.checkpoint(),
            unchanged,
            "preflight changed journal: {case}"
        );
    }
    let protected = tig_episode::protect_current_scenes(
        &mut p.source(),
        request,
        &proofs,
        children("child"),
        &mut journal,
        &coordination,
        &budget,
    )
    .unwrap();
    let expected = candidates
        .iter()
        .flat_map(|candidate| candidate.of(Role::Primary).iter().cloned())
        .collect::<BTreeSet<_>>();
    assert_eq!(protected.primary_objects(), &expected);
    assert!(expected.len() < candidates.iter().map(|c| c.of(Role::Primary).len()).sum());
    assert_eq!(protected.selections().count(), 4);
    assert!(protected.logical_bytes() <= budget.max_logical_bytes);
    for (_, selection) in protected.selections() {
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.selection() == selection)
        );
    }
    let snapshot = p.source();
    protected.verify_current(&snapshot, &mut journal).unwrap();
    let peer = plans[0]
        .readback(
            candidates[0].selection(),
            &mut MetadataReader::open(&root, &checkpoint).unwrap(),
            TreeLimits::default(),
        )
        .unwrap()
        .protect(
            &mut journal,
            &coordination,
            Identity {
                source: key("independent-peer"),
                owner: key("peer"),
            },
            0,
            &key("protect-peer"),
        )
        .unwrap();
    let locked = plans[0]
        .readback(
            candidates[0].selection(),
            &mut MetadataReader::open(&root, &checkpoint).unwrap(),
            TreeLimits::default(),
        )
        .unwrap()
        .resume(
            &mut journal,
            &coordination,
            Identity {
                source: key("child-0"),
                owner: key("episode-owner"),
            },
            2,
        );
    assert!(locked.is_err(), "same source fence must remain held");
    composition_protection::exercise(
        p,
        request,
        &proofs,
        &protected,
        &mut journal,
        scenes,
        providers,
    );
    drop(peer);
    drop(protected);

    let prepared = journal
        .apply(
            key("foreign-prepare"),
            ClaimCommand::Prepare {
                source: key("recover-2"),
                owner: key("foreign"),
                expected_revision: 0,
                selection: candidates[2].selection().clone(),
            },
        )
        .unwrap();
    journal
        .apply(
            key("foreign-adopt"),
            ClaimCommand::Adopt {
                source: key("recover-2"),
                owner: key("foreign"),
                expected_revision: prepared.source_revision,
                prepared: prepared.reference,
            },
        )
        .unwrap();
    assert!(
        tig_episode::protect_current_scenes(
            &mut p.source(),
            request,
            &proofs,
            children("recover"),
            &mut journal,
            &coordination,
            &budget
        )
        .is_err()
    );
    // The segment-key order puts scene-b last; all three earlier adoptions persist.
    for index in [0, 1, 3] {
        let retained = journal
            .source(&key(&format!("recover-{index}")))
            .unwrap()
            .unwrap();
        assert_eq!(retained.revision, 2);
        assert_eq!(
            retained.current.unwrap().selection,
            *candidates[index].selection()
        );
    }
    let mut retry = children("recover");
    retry.get_mut("scene-b").unwrap().identity.source = key("recover-alternate-2");
    let recovered = tig_episode::protect_current_scenes(
        &mut p.source(),
        request,
        &proofs,
        retry,
        &mut journal,
        &coordination,
        &budget,
    )
    .unwrap();
    recovered.verify_current(&snapshot, &mut journal).unwrap();
    assert_eq!(
        journal.source(&key("recover-0")).unwrap().unwrap().revision,
        2,
        "retry must not create another protection revision"
    );
    assert_eq!(
        journal.source(&key("recover-2")).unwrap().unwrap().owner,
        key("foreign")
    );
    let ran = std::cell::Cell::new(false);
    let failed = recovered.with_current(&snapshot, &mut journal, |journal| {
        ran.set(true);
        journal.apply(
            key("external-withdraw"),
            ClaimCommand::Withdraw {
                source: key("recover-0"),
                owner: key("episode-owner"),
                expected_revision: 2,
            },
        )?;
        Ok(())
    });
    assert!(
        ran.get() && failed.is_err(),
        "stale postcondition cannot release a successful result"
    );
    let ran = std::cell::Cell::new(false);
    assert!(
        recovered
            .with_current(&snapshot, &mut journal, |_| {
                ran.set(true);
                Ok(())
            })
            .is_err()
    );
    assert!(!ran.get(), "stale ownership must refuse before callback");
    assert!(
        journal
            .source(&key("recover-0"))
            .unwrap()
            .unwrap()
            .predecessors
            .iter()
            .any(|predecessor| predecessor.selection == *candidates[0].selection())
    );
}
