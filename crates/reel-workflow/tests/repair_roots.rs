//! Explicit branch repair with shared metadata and preserved historical tasks.
use reel_workflow::{review::*, *};
use std::collections::BTreeMap;
fn pin(id: &str, raw: &[u8]) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    }
}
fn fixture() -> (Plan, CheckedReport, Repair) {
    let master = pin("publication/master", b"old master");
    let copy = Copy {
        schema: "reel.workflow-review-copy.v1".into(),
        project: "synthetic".into(),
        source_commit: "a".repeat(40),
        language: "es".into(),
        publication: master.clone(),
        review: pin("review/manifest", b"old reading manifest"),
        format: Format::Comic {
            page_order: vec!["page01".into(), "page02".into()],
        },
        units: BTreeMap::from([(
            "target".into(),
            Unit::Comic {
                page_id: "page01".into(),
                panel_id: Some("panel-a".into()),
                balloon_id: Some("line-a".into()),
            },
        )]),
    };
    let report = check_report(
        &copy,
        &Report {
            schema: "reel.workflow-review-report.v1".into(),
            copy_sha256: canonical_digest(&copy).unwrap(),
            findings: vec![Finding {
                code: "synthetic-layout".into(),
                unit: "target".into(),
                frames: None,
                modality: Modality::Layout,
                route: Route::Project,
                requested_change: "Authored synthetic repair scope; no creative approval.".into(),
                evidence: vec![copy.review.clone()],
            }],
        },
    )
    .unwrap();
    let task = |id: &str, language: &str, inputs: Vec<Artifact>, deps: &[&str]| Task {
        id: id.into(),
        native_inputs: Some(pin(&format!("native/{id}"), b"old native declaration")),
        upstream_inputs: None,
        operation: Operation::TextProposal,
        tool: None,
        language: Some(language.into()),
        prompt: "Synthetic rooted repair metadata test".into(),
        inputs,
        depends_on: deps.iter().map(|id| (*id).into()).collect(),
        outputs: BTreeMap::from([(
            "output".into(),
            OutputContract {
                media_type: "application/json".into(),
                max_bytes: 1024,
                required: true,
            },
        )]),
    };
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: copy.project.clone(),
        source_commit: copy.source_commit.clone(),
        tasks: vec![
            task(
                "opening",
                "es",
                vec![master.clone(), pin("page/1", b"old page one")],
                &[],
            ),
            task(
                "closing",
                "es",
                vec![master.clone(), pin("page/2", b"old page two")],
                &[],
            ),
            task("english", "en", vec![master.clone()], &[]),
            task(
                "controls",
                "es",
                vec![master.clone()],
                &["opening", "closing"],
            ),
            task("issue", "es", vec![master.clone()], &["controls"]),
            task("review", "es", vec![master.clone()], &["issue"]),
        ],
    };
    let repair = Repair {
        schema: "reel.workflow-repair.v2".into(),
        report_sha256: report.report_sha256().into(),
        finding_keys: report.findings().keys().cloned().collect(),
        edits: vec![
            Edit {
                before: master,
                after: pin("publication/master", b"new master"),
            },
            Edit {
                before: pin("page/1", b"old page one"),
                after: pin("page/1", b"new page one"),
            },
        ],
        roots: Some(vec!["opening".into()]),
    };
    (plan, report, repair)
}
#[test]
fn explicit_part_repair_preserves_other_part_and_language_at_old_snapshots() {
    let (plan, report, repair) = fixture();
    let impact = repair_impact(&plan, &report, &repair).unwrap();
    assert_eq!(impact.schema, "reel.workflow-repair-impact.v2");
    assert_eq!(
        impact.rebuild,
        vec!["opening", "controls", "issue", "review"]
    );
    assert_eq!(impact.unaffected, vec!["closing", "english"]);
    assert_eq!(impact.contextual_languages, vec!["es"]);
    assert_eq!(impact.contextual_review, report.copy().clone());
    let revision = repair::Revision {
        task_snapshots: BTreeMap::new(),
        schema: "reel.workflow-repair-revision.v2".into(),
        source_commit: "b".repeat(40),
        native_inputs: plan
            .tasks
            .iter()
            .filter(|t| impact.rebuild.contains(&t.id))
            .map(|t| {
                (
                    t.id.clone(),
                    Edit {
                        before: t.native_inputs.clone().unwrap(),
                        after: pin(&format!("native/{}", t.id), b"new native declaration"),
                    },
                )
            })
            .collect(),
    };
    let candidate = repair::materialize(&plan, &report, &repair, &revision).unwrap();
    assert_eq!(candidate.plan.schema, SCOPED_PLAN_SCHEMA);
    for id in [&"closing", &"english"] {
        let old = plan.tasks.iter().find(|t| &t.id == id).unwrap();
        assert_eq!(
            candidate.plan.tasks.iter().find(|t| &t.id == id).unwrap(),
            old
        );
        assert_eq!(
            candidate.plan.source_commit_for(id).unwrap(),
            plan.source_commit
        );
        assert_eq!(
            make_packet(&candidate.plan, id, BTreeMap::new()).unwrap(),
            make_packet(&plan, id, BTreeMap::new()).unwrap()
        );
    }
    for id in &impact.rebuild {
        let corrected = candidate.plan.tasks.iter().find(|t| &t.id == id).unwrap();
        assert!(corrected.inputs.contains(&repair.edits[0].after));
        assert_eq!(
            candidate.plan.source_commit_for(id).unwrap(),
            revision.source_commit
        );
    }
    let mut global = repair.clone();
    global.schema = "reel.workflow-repair.v1".into();
    global.roots = None;
    let global = repair_impact(&plan, &report, &global).unwrap();
    assert!(
        global.rebuild.contains(&"closing".into()) && global.rebuild.contains(&"english".into())
    );
    assert_eq!(global.contextual_languages, vec!["en", "es"]);
}
#[test]
fn rooted_repairs_refuse_ambiguous_versions_invalid_roots_and_outside_edits() {
    let (plan, report, repair) = fixture();
    for (schema, roots, code) in [
        (
            "reel.workflow-repair.v1",
            Some(vec!["opening".into()]),
            "review-repair-version-envelope",
        ),
        (
            "reel.workflow-repair.v2",
            None,
            "review-repair-version-envelope",
        ),
        (
            "reel.workflow-repair.v2",
            Some(vec![]),
            "review-repair-root-count",
        ),
        (
            "reel.workflow-repair.v2",
            Some(vec!["opening".into(), "opening".into()]),
            "review-repair-root-duplicate",
        ),
        (
            "reel.workflow-repair.v2",
            Some(vec!["absent".into()]),
            "review-repair-root-input",
        ),
    ] {
        let mut bad = repair.clone();
        bad.schema = schema.into();
        bad.roots = roots;
        assert_eq!(repair_impact(&plan, &report, &bad).unwrap_err().code, code);
    }
    let mut bad = repair.clone();
    bad.edits.push(Edit {
        before: pin("page/2", b"old page two"),
        after: pin("page/2", b"new page two"),
    });
    assert_eq!(
        repair_impact(&plan, &report, &bad).unwrap_err().code,
        "review-repair-edit-outside-scope"
    );
    let revision = repair::Revision {
        task_snapshots: BTreeMap::new(),
        schema: "reel.workflow-repair-revision.v1".into(),
        source_commit: "b".repeat(40),
        native_inputs: BTreeMap::new(),
    };
    assert_eq!(
        repair::materialize(&plan, &report, &repair, &revision)
            .unwrap_err()
            .code,
        "repair-rooted-revision-required"
    );
    let mut legacy = repair;
    legacy.schema = "reel.workflow-repair.v1".into();
    legacy.roots = None;
    let raw = serde_json::to_vec(&legacy).unwrap();
    let value: serde_json::Value = decode(&raw).unwrap();
    assert!(value.get("roots").is_none());
    let restored: Repair = decode(&raw).unwrap();
    assert!(restored.roots.is_none());
    assert_eq!(serde_json::to_vec(&restored).unwrap(), raw);
}

#[test]
fn rooted_resolution_still_requires_whole_corrected_issue_and_explicit_evaluation() {
    let (plan, before, repair) = fixture();
    let impact = repair_impact(&plan, &before, &repair).unwrap();
    let revision = repair::Revision {
        task_snapshots: BTreeMap::new(),
        schema: "reel.workflow-repair-revision.v2".into(),
        source_commit: "b".repeat(40),
        native_inputs: plan
            .tasks
            .iter()
            .filter(|t| impact.rebuild.contains(&t.id))
            .map(|t| {
                (
                    t.id.clone(),
                    Edit {
                        before: t.native_inputs.clone().unwrap(),
                        after: pin(&format!("native/{}", t.id), b"new declaration"),
                    },
                )
            })
            .collect(),
    };
    let candidate = repair::materialize(&plan, &before, &repair, &revision).unwrap();
    let mut copy = before.copy().clone();
    copy.source_commit = revision.source_commit.clone();
    copy.publication = repair.edits[0].after.clone();
    copy.review = pin("review/manifest", b"corrected whole issue");
    for page in ["page01", "page02"] {
        copy.units.insert(
            page.into(),
            Unit::Comic {
                page_id: page.into(),
                panel_id: None,
                balloon_id: None,
            },
        );
    }
    let after = check_report(
        &copy,
        &Report {
            schema: "reel.workflow-review-report.v1".into(),
            copy_sha256: canonical_digest(&copy).unwrap(),
            findings: vec![],
        },
    )
    .unwrap();
    let reports = BTreeMap::from([("es".into(), after)]);
    let mut decision = resolution::Decision {
        schema: "reel.workflow-resolution-decision.v1".into(),
        baseline_report_sha256: before.report_sha256().into(),
        candidate_plan_sha256: candidate.plan_sha256,
        contexts: BTreeMap::from([(
            "es".into(),
            resolution::Context {
                report_sha256: reports["es"].report_sha256().into(),
                production_task: "issue".into(),
                unit_modalities: copy
                    .units
                    .keys()
                    .map(|id| (id.clone(), vec![Modality::Layout]))
                    .collect(),
            },
        )]),
        unit_map: BTreeMap::from([("target".into(), "target".into())]),
        evaluations: vec![resolution::Evaluation {
            finding_key: before.findings().keys().next().unwrap().clone(),
            corrected_unit: "target".into(),
            rationale: "Explicit synthetic full-issue contextual evaluation; not actual viewing."
                .into(),
            evidence: vec![copy.review],
        }],
    };
    let proposal =
        resolution::propose(&plan, &before, &repair, &revision, &reports, &decision).unwrap();
    assert_eq!(proposal.corrected_reports.len(), 1);
    assert!(!proposal.remote_issues_closed && !proposal.selected && !proposal.approved);
    decision
        .contexts
        .get_mut("es")
        .unwrap()
        .unit_modalities
        .remove("page02");
    assert_eq!(
        resolution::propose(&plan, &before, &repair, &revision, &reports, &decision)
            .unwrap_err()
            .code,
        "resolution-full-context-required"
    );
}

#[test]
fn later_consumer_source_commits_preserve_an_already_frozen_repair_root_packet() {
    let (baseline, report, repair) = fixture();
    let impact = repair_impact(&baseline, &report, &repair).unwrap();
    let mut revision = repair::Revision {
        schema: "reel.workflow-repair-revision.v2".into(),
        source_commit: "b".repeat(40),
        task_snapshots: BTreeMap::new(),
        native_inputs: baseline
            .tasks
            .iter()
            .filter(|t| impact.rebuild.contains(&t.id))
            .map(|task| {
                (
                    task.id.clone(),
                    Edit {
                        before: task.native_inputs.clone().unwrap(),
                        after: pin(
                            &format!("native/{}", task.id),
                            b"first exact native declaration",
                        ),
                    },
                )
            })
            .collect(),
    };
    let first = repair::materialize(&baseline, &report, &repair, &revision).unwrap();
    let frozen = make_packet(&first.plan, "opening", BTreeMap::new()).unwrap();
    revision.schema = "reel.workflow-repair-revision.v3".into();
    revision.source_commit = "c".repeat(40);
    revision.task_snapshots = impact
        .rebuild
        .iter()
        .map(|id| {
            (
                id.clone(),
                if id == "opening" {
                    "b".repeat(40)
                } else {
                    revision.source_commit.clone()
                },
            )
        })
        .collect();
    for (id, edit) in &mut revision.native_inputs {
        if id != "opening" {
            edit.after = pin(
                &edit.before.logical_id,
                b"later consumer native declaration",
            );
        }
    }
    let later = repair::materialize(&baseline, &report, &repair, &revision).unwrap();
    assert_eq!(later.plan.source_commit, revision.source_commit);
    assert_eq!(
        make_packet(&later.plan, "opening", BTreeMap::new()).unwrap(),
        frozen
    );
    for id in ["closing", "english"] {
        assert_eq!(
            make_packet(&later.plan, id, BTreeMap::new()).unwrap(),
            make_packet(&baseline, id, BTreeMap::new()).unwrap()
        );
    }
    for id in ["controls", "issue", "review"] {
        assert_eq!(
            later.plan.source_commit_for(id).unwrap(),
            revision.source_commit
        );
        assert_eq!(
            later
                .plan
                .tasks
                .iter()
                .find(|t| t.id == id)
                .unwrap()
                .native_inputs
                .as_ref()
                .unwrap(),
            &revision.native_inputs[id].after
        );
    }
    assert_eq!(later.impact.rebuild, first.impact.rebuild);
    assert_eq!(
        later.impact.contextual_review,
        first.impact.contextual_review
    );
    assert_ne!(later.plan_sha256, first.plan_sha256);
    // V2's one-commit rule would retarget the exact root even if its input
    // descriptors had not otherwise changed. V3 requires the explicit choice.
    revision.schema = "reel.workflow-repair-revision.v2".into();
    revision.task_snapshots.clear();
    let retargeted = repair::materialize(&baseline, &report, &repair, &revision).unwrap();
    assert_ne!(
        make_packet(&retargeted.plan, "opening", BTreeMap::new())
            .unwrap()
            .task_key,
        frozen.task_key
    );
}

#[test]
fn explicit_repair_snapshots_refuse_missing_extra_invalid_and_legacy_maps() {
    let (baseline, report, repair) = fixture();
    let impact = repair_impact(&baseline, &report, &repair).unwrap();
    let valid = repair::Revision {
        schema: "reel.workflow-repair-revision.v3".into(),
        source_commit: "c".repeat(40),
        task_snapshots: impact
            .rebuild
            .iter()
            .map(|id| (id.clone(), "b".repeat(40)))
            .collect(),
        native_inputs: baseline
            .tasks
            .iter()
            .filter(|t| impact.rebuild.contains(&t.id))
            .map(|task| {
                (
                    task.id.clone(),
                    Edit {
                        before: task.native_inputs.clone().unwrap(),
                        after: pin(&format!("native/{}", task.id), b"new declaration"),
                    },
                )
            })
            .collect(),
    };
    repair::materialize(&baseline, &report, &repair, &valid).unwrap();
    for id in ["opening", "review"] {
        let mut bad = valid.clone();
        bad.task_snapshots.remove(id);
        assert_eq!(
            repair::materialize(&baseline, &report, &repair, &bad)
                .unwrap_err()
                .code,
            "repair-revision-snapshot-task-set"
        );
    }
    let mut bad = valid.clone();
    bad.task_snapshots.clear();
    assert_eq!(
        repair::materialize(&baseline, &report, &repair, &bad)
            .unwrap_err()
            .code,
        "repair-revision-snapshot-task-set"
    );
    for id in ["closing", "unknown"] {
        let mut bad = valid.clone();
        bad.task_snapshots.insert(id.into(), "b".repeat(40));
        assert_eq!(
            repair::materialize(&baseline, &report, &repair, &bad)
                .unwrap_err()
                .code,
            "repair-revision-snapshot-task-set"
        );
    }
    for commit in ["".into(), "0".repeat(40), "B".repeat(40), "b".repeat(39)] {
        let mut bad = valid.clone();
        bad.task_snapshots.insert("opening".into(), commit);
        assert_eq!(
            repair::materialize(&baseline, &report, &repair, &bad)
                .unwrap_err()
                .code,
            "repair-revision-task-snapshot"
        );
    }
    let mut global = repair.clone();
    global.schema = "reel.workflow-repair.v1".into();
    global.roots = None;
    for schema in [
        "reel.workflow-repair-revision.v1",
        "reel.workflow-repair-revision.v2",
    ] {
        let mut bad = valid.clone();
        bad.schema = schema.into();
        assert_eq!(
            repair::materialize(&baseline, &report, &global, &bad)
                .unwrap_err()
                .code,
            "repair-revision-snapshot-mode"
        );
        bad.task_snapshots.clear();
        let raw = serde_json::to_vec(&bad).unwrap();
        let value: serde_json::Value = decode(&raw).unwrap();
        assert!(value.get("task_snapshots").is_none());
        assert_eq!(
            serde_json::to_vec(&decode::<repair::Revision>(&raw).unwrap()).unwrap(),
            raw
        );
    }
}
