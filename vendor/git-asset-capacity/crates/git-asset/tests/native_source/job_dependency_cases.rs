use super::*;
use job::Dependencies;

fn pins() -> (BTreeMap<LogicalPath, Object>, BTreeMap<LogicalPath, Object>) {
    (
        BTreeMap::from([(
            LogicalPath::parse("report").unwrap(),
            Object::from_bytes(b"existing compiled report"),
        )]),
        BTreeMap::from([(
            LogicalPath::parse("receipt").unwrap(),
            Object::from_bytes(b"existing input receipt"),
        )]),
    )
}

#[test]
fn empty_dependencies_preserve_legacy_identity_and_composition_has_its_own_role() {
    let (_f, _source, plan, context, mut nodes) = fixture();
    let runtime = names();
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    let empty = BTreeMap::new();
    let legacy = job::freeze(&admission, &mut nodes, TreeLimits::default()).unwrap();
    let none = Dependencies {
        composition: &empty,
        evidence: &empty,
    };
    let same =
        job::freeze_with_dependencies(&admission, &none, None, &mut nodes, TreeLimits::default())
            .unwrap();
    assert_eq!(legacy.selection(), same.selection());
    assert!(same.of(Role::Composition).is_empty());
    let (mut composition, mut evidence) = pins();
    let compiled = composition.values().next().unwrap().clone();
    composition.insert(
        LogicalPath::parse("report-alias").unwrap(),
        compiled.clone(),
    );
    evidence.insert(
        LogicalPath::parse("shared-bytes").unwrap(),
        compiled.clone(),
    );
    let primary_alias = Object::from_bytes(b"rows-v1");
    composition.insert(
        LogicalPath::parse("shared-primary-bytes").unwrap(),
        primary_alias.clone(),
    );
    let dependencies = Dependencies {
        composition: &composition,
        evidence: &evidence,
    };
    let candidate = job::freeze_with_dependencies(
        &admission,
        &dependencies,
        None,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let restored = job::restore_with_dependencies(
        candidate.selection(),
        &admission,
        &dependencies,
        None,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(
        restored.of(Role::Composition),
        &BTreeSet::from([compiled.clone(), primary_alias.clone()])
    );
    assert!(restored.of(Role::Evidence).contains(&compiled));
    assert!(restored.of(Role::Primary).contains(&primary_alias));
    for role in [Role::Primary, Role::Runtime, Role::ClaimedOutput] {
        assert!(!restored.of(role).contains(&compiled));
    }
    assert_eq!(
        restored.logical_bytes(),
        restored.objects().iter().map(|o| o.bytes).sum::<u64>()
    );
    assert!(!restored.complete_claim_coverage());
    assert!(
        job::restore(
            candidate.selection(),
            &admission,
            &mut nodes,
            TreeLimits::default()
        )
        .is_err()
    );
}

#[test]
fn dependency_substitution_omission_rename_extra_and_role_change_refuse() {
    let (_f, _source, plan, context, mut nodes) = fixture();
    let runtime = names();
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    let (composition, evidence) = pins();
    let dependencies = Dependencies {
        composition: &composition,
        evidence: &evidence,
    };
    let candidate = job::freeze_with_dependencies(
        &admission,
        &dependencies,
        None,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    for case in 0..6 {
        let mut changed = composition.clone();
        let mut proof = evidence.clone();
        let name = LogicalPath::parse("report").unwrap();
        match case {
            0 => {
                changed.clear();
            }
            1 => {
                changed.insert(name, Object::from_bytes(b"substituted"));
            }
            2 => {
                let object = changed.remove(&name).unwrap();
                changed.insert(LogicalPath::parse("renamed").unwrap(), object);
            }
            3 => {
                changed.insert(
                    LogicalPath::parse("extra").unwrap(),
                    Object::from_bytes(b"extra"),
                );
            }
            4 => {
                let object = changed.remove(&name).unwrap();
                proof.insert(name, object);
            }
            _ => {
                proof.clear();
            }
        }
        assert!(
            job::restore_with_dependencies(
                candidate.selection(),
                &admission,
                &Dependencies {
                    composition: &changed,
                    evidence: &proof
                },
                None,
                &mut nodes,
                TreeLimits::default()
            )
            .is_err()
        );
    }
}

#[test]
fn dependency_budget_names_and_sizes_refuse_before_any_metadata_io() {
    let (_f, _source, plan, context, _nodes) = fixture();
    let runtime = names();
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    let (composition, evidence) = pins();
    for case in 0..5 {
        let mut composition = composition.clone();
        let mut evidence = evidence.clone();
        let mut limits = TreeLimits::default();
        match case {
            0 => limits.max_entries = 4 + runtime.len() + composition.len() + evidence.len() - 1,
            1 => {
                composition.insert(
                    LogicalPath::parse("report/child").unwrap(),
                    Object::from_bytes(b"child"),
                );
            }
            2 => {
                evidence.insert(
                    LogicalPath::parse("receipt/child").unwrap(),
                    Object::from_bytes(b"child"),
                );
            }
            3 => {
                let mut conflict = composition.values().next().unwrap().clone();
                conflict.bytes += 1;
                evidence.insert(LogicalPath::parse("conflict").unwrap(), conflict);
            }
            _ => {
                let mut unsafe_size = composition.values().next().unwrap().clone();
                unsafe_size.bytes = 9_007_199_254_740_992;
                composition.insert(LogicalPath::parse("report").unwrap(), unsafe_size);
            }
        }
        let dependencies = Dependencies {
            composition: &composition,
            evidence: &evidence,
        };
        assert!(
            job::freeze_with_dependencies(&admission, &dependencies, None, &mut NoIo, limits)
                .is_err()
        );
        assert!(
            job::restore_with_dependencies(
                context.selection(),
                &admission,
                &dependencies,
                None,
                &mut NoIo,
                limits
            )
            .is_err()
        );
    }
}

#[test]
fn thousand_composition_refs_share_whole_budget_with_output_evidence_and_old_root_replays() {
    let (f, source, plan, context, mut nodes) = fixture();
    let current = subject(&source, &f.profiles);
    let claim = Claim::create(&plan, pin(&context), vec![current.subject().clone()]).unwrap();
    let runtime = names();
    let (empty_composition, evidence) = pins();
    let composition = (0..1000)
        .map(|i| {
            (
                LogicalPath::parse(&format!("parts/{i:04}")).unwrap(),
                Object::from_bytes(format!("component {i}").as_bytes()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: Some(OutputEvidence {
            claim: &claim,
            attachments: &empty_composition,
        }),
    };
    let dependencies = Dependencies {
        composition: &composition,
        evidence: &evidence,
    };
    let exact = TreeLimits {
        max_entries: 4
            + runtime.len()
            + 1
            + empty_composition.len()
            + evidence.len()
            + composition.len(),
        ..TreeLimits::default()
    };
    let short = TreeLimits {
        max_entries: exact.max_entries - 1,
        ..exact
    };
    assert!(
        job::freeze_with_dependencies(&admission, &dependencies, None, &mut NoIo, short).is_err()
    );
    let original =
        job::freeze_with_dependencies(&admission, &dependencies, None, &mut nodes, exact).unwrap();
    assert!(
        job::restore_with_dependencies(
            original.selection(),
            &admission,
            &dependencies,
            None,
            &mut NoIo,
            short
        )
        .is_err()
    );
    assert_eq!(original.of(Role::Composition).len(), 1000);
    let mut changed = composition.clone();
    changed.insert(
        LogicalPath::parse("parts/0999").unwrap(),
        Object::from_bytes(b"corrected component"),
    );
    let next = job::freeze_with_dependencies(
        &admission,
        &Dependencies {
            composition: &changed,
            evidence: &evidence,
        },
        None,
        &mut nodes,
        exact,
    )
    .unwrap();
    assert_ne!(original.selection(), next.selection());
    let old = job::restore_with_dependencies(
        original.selection(),
        &admission,
        &dependencies,
        None,
        &mut nodes,
        exact,
    )
    .unwrap();
    assert_eq!(old.of(Role::Composition), original.of(Role::Composition));
    assert!(!old.of(Role::ClaimedOutput).is_empty());
}

#[test]
fn pre_execution_dependency_ownership_survives_metadata_and_journal_restart() {
    let (f, source, plan, context, initial) = fixture();
    let q = query(&source, "inputs/rows");
    let runtime = names();
    let (composition, evidence) = pins();
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    let dependencies = Dependencies {
        composition: &composition,
        evidence: &evidence,
    };
    let disk = tempfile::tempdir().unwrap();
    let metadata = disk.path().join("metadata");
    let mut store = MetadataStore::create(
        &metadata,
        StoreLimits {
            nodes: 8192,
            bytes: 8388608,
            database_bytes: 16777216,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    for (_, bytes) in initial.objects() {
        store.put(bytes).unwrap();
    }
    let candidate = job::freeze_with_dependencies(
        &admission,
        &dependencies,
        None,
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    let witness = store.finish().unwrap();
    drop(store);
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut reader = MetadataReader::open(&metadata, &witness).unwrap();
    let (mut archived, context) = AssetSnapshot::from_production_context(
        context.selection(),
        &f.profiles,
        &mut reader,
        Limits::default(),
        TreeLimits::default(),
        q.limits,
    )
    .unwrap();
    let plan = recipe(&mut archived, &f.profiles, &policies(&f), &operation());
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    let restored = job::restore_with_dependencies(
        candidate.selection(),
        &admission,
        &dependencies,
        None,
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    let journal_path = disk.path().join("owners");
    let mut journal = SourceJournal::create(&journal_path, key("fixture"), key("epoch")).unwrap();
    adopt(&mut journal, "job-a", 0, restored.selection().clone(), "a");
    adopt(&mut journal, "job-b", 0, restored.selection().clone(), "b");
    let checkpoint = journal.checkpoint();
    drop(journal);
    let mut journal = SourceJournal::open(&journal_path, &checkpoint).unwrap();
    journal
        .apply(
            key("withdraw-a"),
            OwnerCommand::Withdraw {
                source: key("job-a"),
                owner: key("fixture"),
                expected_revision: 2,
            },
        )
        .unwrap();
    let withdrawn = journal.obligations(16).unwrap();
    assert_eq!(
        withdrawn.len(),
        2,
        "withdraw preserves the owner's predecessor until explicit retirement"
    );
    journal
        .apply(
            key("retire-a"),
            OwnerCommand::Retire {
                source: key("job-a"),
                owner: key("fixture"),
                expected_revision: 3,
                predecessor_revision: 2,
            },
        )
        .unwrap();
    let obligations = journal.obligations(16).unwrap();
    assert_eq!(obligations.len(), 1);
    assert_eq!(obligations.iter().next().unwrap().source, key("job-b"));
    assert_eq!(
        obligations.iter().next().unwrap().selection,
        *restored.selection()
    );
    let replay = job::restore_with_dependencies(
        &obligations.iter().next().unwrap().selection,
        &admission,
        &dependencies,
        None,
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(replay.of(Role::Composition).len(), 1);
    assert!(replay.of(Role::ClaimedOutput).is_empty());
    assert!(!obligations.complete_claim_coverage());
}

#[test]
fn interrupted_dependency_update_preserves_old_root_and_exact_retry() {
    let (_f, _source, plan, context, mut nodes) = fixture();
    let runtime = names();
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    let (composition, evidence) = pins();
    let old_dependencies = Dependencies {
        composition: &composition,
        evidence: &evidence,
    };
    let old = job::freeze_with_dependencies(
        &admission,
        &old_dependencies,
        None,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let baseline = nodes
        .objects()
        .map(|(_, raw)| raw.to_vec())
        .collect::<Vec<_>>();
    let mut changed = composition.clone();
    changed.insert(
        LogicalPath::parse("report").unwrap(),
        Object::from_bytes(b"replacement component"),
    );
    let dependencies = Dependencies {
        composition: &changed,
        evidence: &evidence,
    };
    let expected = job::freeze_with_dependencies(
        &admission,
        &dependencies,
        None,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    for fail_after in [0, 1, 2, 5] {
        let mut interrupted = MemoryNodes::default();
        for raw in &baseline {
            interrupted.put(raw).unwrap();
        }
        assert!(
            job::freeze_with_dependencies(
                &admission,
                &dependencies,
                None,
                &mut Writes {
                    base: &mut interrupted,
                    writes: 0,
                    fail_after: Some(fail_after)
                },
                TreeLimits::default()
            )
            .is_err()
        );
        assert!(
            job::restore_with_dependencies(
                old.selection(),
                &admission,
                &old_dependencies,
                None,
                &mut interrupted,
                TreeLimits::default()
            )
            .is_ok()
        );
        let retry = job::freeze_with_dependencies(
            &admission,
            &dependencies,
            None,
            &mut interrupted,
            TreeLimits::default(),
        )
        .unwrap();
        assert_eq!(retry.selection(), expected.selection());
    }
}
