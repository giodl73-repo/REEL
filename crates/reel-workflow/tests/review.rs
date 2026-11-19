//! Review/repair protocol checks, without custody, viewing or creative approval.
use reel_workflow::{review::*, *};
use std::collections::BTreeMap;

struct Memory(BTreeMap<String, Vec<u8>>);
impl publication::InputReader for Memory {
    fn read(&mut self, artifact: &Artifact, _: u64) -> Result<Vec<u8>> {
        self.0.get(&artifact.logical_id).cloned().ok_or(Error {
            kind: ErrorKind::Missing,
            code: "fixture-input".into(),
        })
    }
}
fn structured() -> (Plan, Memory, ReturnedResult, Vec<u8>) {
    let copy = copy(true, "es");
    let report = report(&copy);
    let copy_bytes = serde_json::to_vec(&copy).unwrap();
    let request_bytes = serde_json::to_vec(&Request {
        schema: "reel.workflow-review-request.v1".into(),
        production_commit: copy.source_commit.clone(),
        copy_input: "control/copy".into(),
        report_output: "report".into(),
    })
    .unwrap();
    let report_bytes = serde_json::to_vec(&report).unwrap();
    let memory = Memory(BTreeMap::from([
        (REQUEST_ID.into(), request_bytes.clone()),
        ("control/copy".into(), copy_bytes.clone()),
        ("publication/source".into(), b"source".to_vec()),
        ("review/copy".into(), b"review".to_vec()),
        ("evidence/text".into(), b"synthetic observation".to_vec()),
    ]));
    let mut task = task(
        "review",
        "es",
        vec![
            pin(REQUEST_ID, &request_bytes),
            pin("control/copy", &copy_bytes),
            copy.publication,
            copy.review,
            report.findings[0].evidence[0].clone(),
        ],
        &[],
    );
    task.operation = Operation::ReviewReport;
    task.outputs = BTreeMap::from([(
        "report".into(),
        OutputContract {
            media_type: "application/json".into(),
            max_bytes: MAX_CONTROL_BYTES as u64,
            required: true,
        },
    )]);
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic".into(),
        source_commit: "b".repeat(40),
        tasks: vec![task],
    };
    let packet = make_packet(&plan, "review", BTreeMap::new()).unwrap();
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: "review".into(),
        task_key: packet.task_key,
        attempt_id: "attempt-1".into(),
        artifacts: BTreeMap::from([("report".into(), pin("candidate/report", &report_bytes))]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
        worker_receipt: None,
    };
    (plan, memory, result, report_bytes)
}
#[test]
fn structured_review_binds_before_dispatch_and_checks_returned_semantics() {
    let (plan, mut memory, result, raw) = structured();
    let packet = make_packet(&plan, "review", BTreeMap::new()).unwrap();
    let bound = bind(&packet, &mut memory).unwrap();
    assert_eq!(bound.report_output(), "report");
    assert_eq!(bound.check(&result, &raw).unwrap().findings().len(), 1);
    let mut bad: Report = decode(&raw).unwrap();
    bad.copy_sha256 = "b".repeat(64);
    let bad_raw = serde_json::to_vec(&bad).unwrap();
    let mut bad_result = result.clone();
    bad_result
        .artifacts
        .insert("report".into(), pin("candidate/report", &bad_raw));
    assert_eq!(
        bound.check(&bad_result, &bad_raw).unwrap_err().code,
        "review-copy-pin"
    );
    let mut bad_result = result;
    bad_result
        .artifacts
        .insert("report".into(), pin("candidate/report", b"{}"));
    assert!(bound.check(&bad_result, b"{}").is_err());
}

#[test]
fn model_brief_freezes_unit_and_copy_keys_and_omits_hidden_evidence() {
    let (plan, mut memory, _, _) = structured();
    let packet = make_packet(&plan, "review", BTreeMap::new()).unwrap();
    let bound = bind(&packet, &mut memory).unwrap();
    let visible = std::collections::BTreeSet::from([
        REQUEST_ID.into(),
        "control/copy".into(),
        "review/copy".into(),
    ]);
    let brief = bound.brief("attempt-1", &visible).unwrap();
    assert_eq!(brief.task_key, packet.task_key);
    assert_eq!(brief.task_id, packet.task.id);
    assert_eq!(brief.language, "es");
    assert_ne!(brief.production_commit, packet.source_commit);
    assert_eq!(brief.evidence.len(), 3);
    assert!(
        !serde_json::to_string(&brief)
            .unwrap()
            .contains("evidence/text")
    );
    assert_eq!(
        brief.return_schema["properties"]["copy_sha256"]["const"],
        brief.empty_report.copy_sha256
    );
    let properties = &brief.return_schema["properties"]["findings"]["items"]["properties"];
    assert_eq!(
        properties["unit"]["enum"],
        serde_json::json!(["stable-unit"])
    );
    assert_eq!(properties["frames"], serde_json::json!({"type":"null"}));
    assert_eq!(
        properties["modality"]["enum"],
        serde_json::json!(["visual", "text", "layout"])
    );
    let raw = serde_json::to_vec(&brief.empty_report).unwrap();
    bound.pack_handoff("attempt-1", &raw).unwrap();
    let mut denied = visible.clone();
    denied.remove(REQUEST_ID);
    assert_eq!(
        bound.brief("attempt-1", &denied).unwrap_err().kind,
        ErrorKind::Denied
    );
    denied.insert("undeclared/private".into());
    assert_eq!(
        bound.brief("attempt-1", &denied).unwrap_err().kind,
        ErrorKind::Missing
    );
}
#[test]
fn handoff_packer_checks_semantics_and_hashes_exact_pretty_bytes() {
    let (plan, mut memory, _, raw) = structured();
    let packet = make_packet(&plan, "review", BTreeMap::new()).unwrap();
    let bound = bind(&packet, &mut memory).unwrap();
    let report: Report = decode(&raw).unwrap();
    let mut pretty = serde_json::to_vec_pretty(&report).unwrap();
    pretty.push(b'\n');
    let result = bound.pack_handoff("attempt-1", &pretty).unwrap();
    assert_eq!(result.provenance.mode, Mode::Handoff);
    assert_eq!(result.artifacts["report"].sha256, digest(&pretty));
    assert_eq!(result.artifacts["report"].bytes, pretty.len() as u64);
    assert_ne!(digest(&raw), digest(&pretty));
    assert!(bound.pack_handoff("../other", &pretty).is_err());
    let mut invalid = report;
    invalid.copy_sha256 = "b".repeat(64);
    assert_eq!(
        bound
            .pack_handoff("attempt-1", &serde_json::to_vec(&invalid).unwrap())
            .unwrap_err()
            .kind,
        ErrorKind::Stale
    );
    assert!(bound.pack_handoff("attempt-1", b"{}").is_err());
}
#[test]
fn handoff_packer_never_discards_required_companion_outputs() {
    let (mut plan, mut memory, _, raw) = structured();
    plan.tasks[0].outputs.insert(
        "evidence".into(),
        OutputContract {
            media_type: "text/plain".into(),
            max_bytes: 100,
            required: true,
        },
    );
    let packet = make_packet(&plan, "review", BTreeMap::new()).unwrap();
    let bound = bind(&packet, &mut memory).unwrap();
    assert_eq!(
        bound.pack_handoff("attempt-1", &raw).unwrap_err().kind,
        ErrorKind::Missing
    );
}
#[test]
fn structured_request_is_explicit_bounded_and_cannot_retarget_outputs_or_language() {
    let (mut plan, mut memory, _, _) = structured();
    let original = make_packet(&plan, "review", BTreeMap::new()).unwrap();
    memory.0.insert(REQUEST_ID.into(), b"{}".to_vec());
    assert_eq!(
        bind(&original, &mut memory).err().unwrap().kind,
        ErrorKind::Integrity
    );
    let (mut plan2, mut memory, _, _) = structured();
    plan2.tasks[0].language = Some("en".into());
    let changed = make_packet(&plan2, "review", BTreeMap::new()).unwrap();
    assert_eq!(
        bind(&changed, &mut memory).err().unwrap().code,
        "review-task-edition"
    );
    plan.tasks[0].inputs.retain(|a| a.logical_id != REQUEST_ID);
    assert_eq!(check_plan(&plan).unwrap_err().code, "review-task-request");
    plan.tasks[0].operation = Operation::ReviewFinding;
    check_plan(&plan).unwrap();
    let (mut plan, mut memory, _, _) = structured();
    plan.tasks[0].outputs.get_mut("report").unwrap().media_type = "text/plain".into();
    let changed = make_packet(&plan, "review", BTreeMap::new()).unwrap();
    assert_eq!(
        bind(&changed, &mut memory).err().unwrap().code,
        "review-report-output-contract"
    );
}

fn pin(id: &str, raw: &[u8]) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    }
}
fn copy(comic: bool, language: &str) -> review::Copy {
    review::Copy {
        schema: "reel.workflow-review-copy.v1".into(),
        project: "synthetic".into(),
        source_commit: "a".repeat(40),
        language: language.into(),
        publication: pin("publication/source", b"source"),
        review: pin("review/copy", b"review"),
        format: if comic {
            Format::Comic {
                page_order: vec!["page-1".into(), "page-2".into()],
            }
        } else {
            Format::Film { frames: 48 }
        },
        units: BTreeMap::from([(
            "stable-unit".into(),
            if comic {
                Unit::Comic {
                    page_id: "page-1".into(),
                    panel_id: Some("panel-2".into()),
                    balloon_id: Some("balloon-3".into()),
                }
            } else {
                Unit::Film {
                    cel_id: "existing-cel-id".into(),
                    shot_id: None,
                    cue_id: Some("cue-1".into()),
                    start_frame: 0,
                    end_frame: 24,
                }
            },
        )]),
    }
}
fn report(copy: &review::Copy) -> Report {
    Report {
        schema: "reel.workflow-review-report.v1".into(),
        copy_sha256: canonical_digest(copy).unwrap(),
        findings: vec![Finding {
            code: "wording".into(),
            unit: "stable-unit".into(),
            frames: None,
            modality: Modality::Text,
            route: Route::Project,
            requested_change: "Preserve the approved Spanish accent in this line.".into(),
            evidence: vec![pin("evidence/text", b"synthetic observation")],
        }],
    }
}
fn task(id: &str, language: &str, inputs: Vec<Artifact>, depends: &[&str]) -> Task {
    Task {
        id: id.into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::TextProposal,
        tool: None,
        language: Some(language.into()),
        prompt: "Synthetic fixture".into(),
        inputs,
        depends_on: depends.iter().map(|s| (*s).into()).collect(),
        outputs: BTreeMap::from([(
            "candidate".into(),
            OutputContract {
                media_type: "application/json".into(),
                max_bytes: 1024,
                required: true,
            },
        )]),
    }
}
fn plan() -> Plan {
    Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic".into(),
        source_commit: "a".repeat(40),
        tasks: vec![
            task("art", "es", vec![pin("input/art", b"shared art")], &[]),
            task(
                "es-lettering",
                "es",
                vec![pin("input/es-text", b"Spanish text")],
                &["art"],
            ),
            task(
                "en-lettering",
                "en",
                vec![pin("input/en-text", b"English text")],
                &["art"],
            ),
            task("es-reading", "es", vec![], &["es-lettering"]),
            task("en-reading", "en", vec![], &["en-lettering"]),
            task("issue", "es", vec![], &["es-reading", "en-reading"]),
        ],
    }
}
fn repair(checked: &CheckedReport, before: Artifact) -> Repair {
    Repair {
        schema: "reel.workflow-repair.v1".into(),
        report_sha256: checked.report_sha256().into(),
        finding_keys: checked.findings().keys().cloned().collect(),
        edits: vec![Edit {
            after: pin(&before.logical_id, b"corrected candidate"),
            before,
        }],
    }
}

struct ResolutionCase {
    plan: Plan,
    before: CheckedReport,
    repair: Repair,
    revision: reel_workflow::repair::Revision,
    after: BTreeMap<String, CheckedReport>,
    decision: reel_workflow::resolution::Decision,
}
impl ResolutionCase {
    fn new(comic: bool) -> Self {
        let mut old = copy(comic, "es");
        if comic {
            for page in ["page-1", "page-2"] {
                old.units.insert(
                    page.into(),
                    Unit::Comic {
                        page_id: page.into(),
                        panel_id: None,
                        balloon_id: None,
                    },
                );
            }
        } else {
            old.units.insert(
                "neighbor".into(),
                Unit::Shot {
                    shot_id: "neighbor-shot".into(),
                    start_frame: 24,
                    end_frame: 48,
                },
            );
        }
        let before = check_report(&old, &report(&old)).unwrap();
        let plan = plan();
        let repair = repair(&before, pin("input/art", b"shared art"));
        let revision = reel_workflow::repair::Revision {
            schema: "reel.workflow-repair-revision.v1".into(),
            source_commit: "b".repeat(40),
            native_inputs: BTreeMap::new(),
        };
        let candidate =
            reel_workflow::repair::materialize(&plan, &before, &repair, &revision).unwrap();
        let mut after = BTreeMap::new();
        let mut contexts = BTreeMap::new();
        for language in ["es", "en"] {
            let mut copy = old.clone();
            copy.language = language.into();
            copy.source_commit = revision.source_commit.clone();
            copy.publication = pin("publication/source", b"corrected source");
            copy.review = pin(
                "review/copy",
                format!("corrected {language} proof").as_bytes(),
            );
            let checked = check_report(
                &copy,
                &Report {
                    schema: "reel.workflow-review-report.v1".into(),
                    copy_sha256: canonical_digest(&copy).unwrap(),
                    findings: vec![],
                },
            )
            .unwrap();
            contexts.insert(
                language.into(),
                reel_workflow::resolution::Context {
                    report_sha256: checked.report_sha256().into(),
                    unit_modalities: copy
                        .units
                        .keys()
                        .map(|k| (k.clone(), vec![Modality::Text]))
                        .collect(),
                },
            );
            after.insert(language.into(), checked);
        }
        let decision=reel_workflow::resolution::Decision{schema:"reel.workflow-resolution-decision.v1".into(),baseline_report_sha256:before.report_sha256().into(),candidate_plan_sha256:candidate.plan_sha256,contexts,unit_map:old.units.keys().map(|k|(k.clone(),k.clone())).collect(),evaluations:vec![reel_workflow::resolution::Evaluation{finding_key:before.findings().keys().next().unwrap().clone(),corrected_unit:"stable-unit".into(),rationale:"Explicit synthetic follow-up review declaration, not inferred from an empty report.".into(),evidence:vec![after["es"].copy().review.clone()]}]};
        Self {
            plan,
            before,
            repair,
            revision,
            after,
            decision,
        }
    }
    fn propose(&self) -> Result<reel_workflow::resolution::Proposal> {
        reel_workflow::resolution::propose(
            &self.plan,
            &self.before,
            &self.repair,
            &self.revision,
            &self.after,
            &self.decision,
        )
    }
    fn update_report(&mut self, language: &str, copy: review::Copy, findings: Vec<Finding>) {
        let r = check_report(
            &copy,
            &Report {
                schema: "reel.workflow-review-report.v1".into(),
                copy_sha256: canonical_digest(&copy).unwrap(),
                findings,
            },
        )
        .unwrap();
        let context = self.decision.contexts.get_mut(language).unwrap();
        context.report_sha256 = r.report_sha256().into();
        context.unit_modalities = copy
            .units
            .keys()
            .map(|k| (k.clone(), vec![Modality::Text]))
            .collect();
        self.after.insert(language.into(), r);
    }
}
#[test]
fn resolution_requires_explicit_bilingual_context_and_keeps_remote_authority_separate() {
    for comic in [true, false] {
        let c = ResolutionCase::new(comic);
        let p = c.propose().unwrap();
        assert_eq!(p.corrected_reports.len(), 2);
        assert_eq!(p.finding_keys, c.repair.finding_keys);
        assert!(!p.remote_issues_closed && !p.selected && !p.approved);
        let mut c = ResolutionCase::new(comic);
        c.after.remove("en");
        assert_eq!(
            c.propose().unwrap_err().code,
            "resolution-context-languages"
        );
        let mut c = ResolutionCase::new(comic);
        c.decision.evaluations.clear();
        assert_eq!(
            c.propose().unwrap_err().code,
            "resolution-explicit-evaluations"
        );
        let mut c = ResolutionCase::new(comic);
        c.decision
            .contexts
            .get_mut("es")
            .unwrap()
            .unit_modalities
            .remove("stable-unit");
        assert_eq!(
            c.propose().unwrap_err().code,
            "resolution-full-context-required"
        );
    }
}
#[test]
fn resolution_refuses_stale_editions_missing_neighborhood_and_unbound_evidence() {
    for comic in [true, false] {
        let mut c = ResolutionCase::new(comic);
        c.decision.candidate_plan_sha256 = "f".repeat(64);
        assert_eq!(
            c.propose().unwrap_err().code,
            "resolution-baseline-or-candidate"
        );
        let mut c = ResolutionCase::new(comic);
        let mut copy = c.after["en"].copy().clone();
        copy.source_commit = "c".repeat(40);
        c.update_report("en", copy, vec![]);
        assert_eq!(
            c.propose().unwrap_err().code,
            "resolution-corrected-edition"
        );
        let mut c = ResolutionCase::new(comic);
        let mut copy = c.after["es"].copy().clone();
        copy.units.remove(if comic { "page-2" } else { "neighbor" });
        c.update_report("es", copy, vec![]);
        assert_eq!(
            c.propose().unwrap_err().code,
            if comic {
                "resolution-whole-page-context"
            } else {
                "resolution-whole-film-context"
            }
        );
        let mut c = ResolutionCase::new(comic);
        c.decision.evaluations[0].evidence = vec![c.before.copy().review.clone()];
        assert_eq!(
            c.propose().unwrap_err().code,
            "resolution-corrected-review-evidence"
        );
        let mut c = ResolutionCase::new(comic);
        c.decision.evaluations[0].rationale.clear();
        assert_eq!(
            c.propose().unwrap_err().code,
            "resolution-finding-evaluation"
        );
        let mut c = ResolutionCase::new(comic);
        c.decision
            .contexts
            .get_mut("es")
            .unwrap()
            .unit_modalities
            .get_mut("stable-unit")
            .unwrap()[0] = Modality::Visual;
        assert_eq!(c.propose().unwrap_err().code, "resolution-finding-modality");
    }
}
#[test]
fn resolution_cannot_hide_a_persisting_finding_behind_an_aliased_unit_key() {
    for alias in [false, true] {
        let mut c = ResolutionCase::new(true);
        let mut copy = c.after["es"].copy().clone();
        let mut finding = report(&copy).findings.remove(0);
        if alias {
            copy.units
                .insert("alias".into(), copy.units["stable-unit"].clone());
            finding.unit = "alias".into();
        }
        c.update_report("es", copy, vec![finding]);
        assert_eq!(
            c.propose().unwrap_err().code,
            "resolution-finding-still-reported"
        );
    }
    let mut c = ResolutionCase::new(true);
    let copy = c.after["es"].copy().clone();
    let mut finding = report(&copy).findings.remove(0);
    finding.code = "another-defect".into();
    c.update_report("es", copy, vec![finding]);
    assert_eq!(c.propose().unwrap().remaining_findings["es"].len(), 1);
}
#[test]
fn resolution_supports_explicit_retiming_and_refuses_identity_changes_or_page_reordering() {
    let mut c = ResolutionCase::new(false);
    let mut copy = c.after["es"].copy().clone();
    copy.format = Format::Film { frames: 60 };
    copy.units = BTreeMap::from([
        (
            "retimed".into(),
            Unit::Film {
                cel_id: "existing-cel-id".into(),
                shot_id: None,
                cue_id: Some("cue-1".into()),
                start_frame: 0,
                end_frame: 30,
            },
        ),
        (
            "retimed-neighbor".into(),
            Unit::Shot {
                shot_id: "neighbor-shot".into(),
                start_frame: 30,
                end_frame: 60,
            },
        ),
    ]);
    c.update_report("es", copy, vec![]);
    c.decision.unit_map = BTreeMap::from([
        ("stable-unit".into(), "retimed".into()),
        ("neighbor".into(), "retimed-neighbor".into()),
    ]);
    c.decision.evaluations[0].corrected_unit = "retimed".into();
    c.propose().unwrap();
    c.decision
        .unit_map
        .insert("stable-unit".into(), "retimed-neighbor".into());
    assert!(c.propose().is_err());
    let mut c = ResolutionCase::new(true);
    let mut copy = c.after["es"].copy().clone();
    if let Format::Comic { page_order } = &mut copy.format {
        page_order.reverse();
    }
    c.update_report("es", copy, vec![]);
    assert_eq!(
        c.propose().unwrap_err().code,
        "resolution-context-page-order"
    );
}

#[test]
fn returned_review_binds_task_language_source_bytes_and_evidence() {
    for comic in [false, true] {
        let copy = copy(comic, "es");
        let copy_bytes = serde_json::to_vec(&copy).unwrap();
        let r = report(&copy);
        let report_bytes = serde_json::to_vec(&r).unwrap();
        let mut task = task(
            "review",
            "es",
            vec![
                pin("control/review-copy", &copy_bytes),
                copy.publication.clone(),
                copy.review.clone(),
                r.findings[0].evidence[0].clone(),
            ],
            &[],
        );
        task.operation = Operation::ReviewFinding;
        task.outputs = BTreeMap::from([(
            "report".into(),
            OutputContract {
                media_type: "application/json".into(),
                max_bytes: MAX_CONTROL_BYTES as u64,
                required: true,
            },
        )]);
        let mut plan = Plan {
            schema: PLAN_SCHEMA.into(),
            project: copy.project.clone(),
            source_commit: copy.source_commit.clone(),
            tasks: vec![task],
        };
        let packet = make_packet(&plan, "review", BTreeMap::new()).unwrap();
        let result = ReturnedResult {
            schema: RESULT_SCHEMA.into(),
            task_id: "review".into(),
            task_key: packet.task_key.clone(),
            attempt_id: "attempt-1".into(),
            artifacts: BTreeMap::from([("report".into(), pin("candidate/report", &report_bytes))]),
            provenance: Provenance {
                mode: Mode::Handoff,
                provider: None,
                model: None,
                executable: None,
            },
            worker_receipt: None,
        };
        let check =
            |packet: &Packet, result: &ReturnedResult, copy_bytes: &[u8], report_bytes: &[u8]| {
                check_returned_report(
                    packet,
                    result,
                    "control/review-copy",
                    copy_bytes,
                    "report",
                    report_bytes,
                )
            };
        assert_eq!(
            check(&packet, &result, &copy_bytes, &report_bytes)
                .unwrap()
                .findings()
                .len(),
            1
        );
        let local = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("packet", serde_json::to_vec(&packet).unwrap()),
            ("result", serde_json::to_vec(&result).unwrap()),
            ("copy", copy_bytes.clone()),
            ("report", report_bytes.clone()),
        ] {
            std::fs::write(local.path().join(name), bytes).unwrap();
        }
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
            .current_dir(local.path())
            .args([
                "check-review-result",
                "packet",
                "result",
                "control/review-copy",
                "copy",
                "report",
                "report",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(!String::from_utf8_lossy(&out.stdout).contains("Spanish accent"));
        assert_eq!(
            check(&packet, &result, b"{}", &report_bytes)
                .unwrap_err()
                .kind,
            ErrorKind::Integrity
        );
        assert_eq!(
            check(&packet, &result, &copy_bytes, b"{}")
                .unwrap_err()
                .kind,
            ErrorKind::Integrity
        );
        plan.tasks[0].language = Some("en".into());
        let changed = make_packet(&plan, "review", BTreeMap::new()).unwrap();
        let changed_result = ReturnedResult {
            task_key: changed.task_key.clone(),
            ..result.clone()
        };
        assert_eq!(
            check(&changed, &changed_result, &copy_bytes, &report_bytes)
                .unwrap_err()
                .code,
            "review-task-edition"
        );
        plan.tasks[0].language = Some("es".into());
        plan.tasks[0].inputs.pop();
        let changed = make_packet(&plan, "review", BTreeMap::new()).unwrap();
        let changed_result = ReturnedResult {
            task_key: changed.task_key.clone(),
            ..result.clone()
        };
        assert_eq!(
            check(&changed, &changed_result, &copy_bytes, &report_bytes)
                .unwrap_err()
                .code,
            "review-task-evidence-pin"
        );
        plan.tasks[0].inputs.pop();
        let changed = make_packet(&plan, "review", BTreeMap::new()).unwrap();
        let changed_result = ReturnedResult {
            task_key: changed.task_key.clone(),
            ..result
        };
        assert_eq!(
            check(&changed, &changed_result, &copy_bytes, &report_bytes)
                .unwrap_err()
                .code,
            "review-task-source-pins"
        );
    }
}

#[test]
fn shot_targets_remain_distinct_and_old_cel_metadata_omits_optional_shot() {
    let mut copy = copy(false, "es");
    let old = serde_json::to_value(&copy.units["stable-unit"]).unwrap();
    assert!(old.get("shot_id").is_none());
    copy.units.insert(
        "stable-unit".into(),
        Unit::Shot {
            shot_id: "existing-shot".into(),
            start_frame: 0,
            end_frame: 24,
        },
    );
    check_copy(&copy).unwrap();
    check_report(&copy, &report(&copy)).unwrap();
    let shot = serde_json::to_value(&copy.units["stable-unit"]).unwrap();
    assert!(shot.get("cel_id").is_none());
    copy.format = Format::Comic {
        page_order: vec!["page-1".into()],
    };
    assert!(check_copy(&copy).is_err());
}

#[test]
fn review_keys_are_stable_for_prose_but_scoped_to_exact_edition_and_language() {
    for comic in [false, true] {
        let copy = copy(comic, "es");
        let report = report(&copy);
        let checked = check_report(&copy, &report).unwrap();
        let key = checked.findings().keys().next().unwrap();
        let mut reworded = report.clone();
        reworded.findings[0].requested_change = "Another description of the same defect.".into();
        reworded.findings[0].evidence = vec![pin("evidence/new", b"new observation")];
        let updated = check_report(&copy, &reworded).unwrap();
        assert_eq!(Some(key), updated.findings().keys().next());
        assert_ne!(checked.report_sha256(), updated.report_sha256());
        for changed in [
            review::Copy {
                language: "en".into(),
                ..copy.clone()
            },
            review::Copy {
                review: pin("review/copy", b"new copy"),
                ..copy.clone()
            },
        ] {
            assert_eq!(
                check_report(&changed, &report).unwrap_err().kind,
                ErrorKind::Stale
            );
            let mut new_report = report.clone();
            new_report.copy_sha256 = canonical_digest(&changed).unwrap();
            assert_ne!(
                Some(key),
                check_report(&changed, &new_report)
                    .unwrap()
                    .findings()
                    .keys()
                    .next()
            );
        }
    }
}

#[test]
fn findings_refuse_missing_units_wrong_modalities_ranges_and_conflicting_duplicates() {
    let film = copy(false, "es");
    let mut r = report(&film);
    r.findings[0].frames = Some(FrameRange { start: 23, end: 24 });
    check_report(&film, &r).unwrap();
    for range in [
        FrameRange { start: 24, end: 25 },
        FrameRange { start: 4, end: 4 },
        FrameRange { start: 23, end: 48 },
    ] {
        let mut bad = r.clone();
        bad.findings[0].frames = Some(range);
        assert_eq!(
            check_report(&film, &bad).unwrap_err().code,
            "review-finding-frames"
        );
    }
    let mut bad = r.clone();
    bad.findings[0].unit = "unseen-cel".into();
    assert_eq!(
        check_report(&film, &bad).unwrap_err().kind,
        ErrorKind::Missing
    );
    r.findings.push(r.findings[0].clone());
    assert_eq!(check_report(&film, &r).unwrap().findings().len(), 1);
    r.findings[1].route = Route::Tig;
    assert_eq!(
        check_report(&film, &r).unwrap_err().kind,
        ErrorKind::Conflict
    );
    let comic = copy(true, "es");
    let mut r = report(&comic);
    r.findings[0].modality = Modality::Audio;
    assert_eq!(
        check_report(&comic, &r).unwrap_err().code,
        "review-comic-modality"
    );
    r.findings[0].modality = Modality::Layout;
    check_report(&comic, &r).unwrap();
    r.findings[0].frames = Some(FrameRange { start: 0, end: 1 });
    assert!(check_report(&comic, &r).is_err());
}

#[test]
fn invalid_copy_geometry_and_unknown_fields_refuse() {
    let mut film = copy(false, "es");
    film.format = Format::Film { frames: 23 };
    assert!(check_copy(&film).is_err());
    let mut comic = copy(true, "es");
    comic.format = Format::Comic {
        page_order: vec!["page-1".into(), "page-1".into()],
    };
    assert_eq!(check_copy(&comic).unwrap_err().code, "review-page-order");
    comic.format = Format::Comic {
        page_order: vec!["missing-page".into()],
    };
    assert!(check_copy(&comic).is_err());
    let mut value = serde_json::to_value(copy(true, "es")).unwrap();
    value["approved"] = serde_json::json!(true);
    assert!(serde_json::from_value::<review::Copy>(value).is_err());
}

#[test]
fn language_local_repair_keeps_other_edition_but_shared_art_rebuilds_both() {
    for comic in [false, true] {
        let copy = copy(comic, "es");
        let checked = check_report(&copy, &report(&copy)).unwrap();
        let plan = plan();
        let impact = repair_impact(
            &plan,
            &checked,
            &repair(&checked, pin("input/es-text", b"Spanish text")),
        )
        .unwrap();
        assert_eq!(impact.directly_affected, ["es-lettering"]);
        assert_eq!(impact.rebuild, ["es-lettering", "es-reading", "issue"]);
        assert_eq!(impact.unaffected, ["art", "en-lettering", "en-reading"]);
        assert_eq!(impact.contextual_review, copy);
        assert_eq!(impact.contextual_languages, ["es"]);
        let impact = repair_impact(
            &plan,
            &checked,
            &repair(&checked, pin("input/art", b"shared art")),
        )
        .unwrap();
        assert_eq!(impact.directly_affected, ["art"]);
        assert_eq!(impact.rebuild.len(), 6);
        assert!(impact.unaffected.is_empty());
        assert_eq!(impact.contextual_languages, ["en", "es"]);
    }
}

#[test]
fn repair_refuses_stale_baseline_unknown_finding_and_wrong_old_bytes() {
    let copy = copy(true, "es");
    let checked = check_report(&copy, &report(&copy)).unwrap();
    let plan = plan();
    let original = repair(&checked, pin("input/es-text", b"Spanish text"));
    let mut bad_plan = plan.clone();
    bad_plan.source_commit = "b".repeat(40);
    assert_eq!(
        repair_impact(&bad_plan, &checked, &original)
            .unwrap_err()
            .kind,
        ErrorKind::Stale
    );
    let mut bad = original.clone();
    bad.report_sha256 = "b".repeat(64);
    assert_eq!(
        repair_impact(&plan, &checked, &bad).unwrap_err().kind,
        ErrorKind::Stale
    );
    let mut bad = original.clone();
    bad.finding_keys = vec!["b".repeat(64)];
    assert_eq!(
        repair_impact(&plan, &checked, &bad).unwrap_err().kind,
        ErrorKind::Missing
    );
    let mut bad = original.clone();
    bad.edits[0].before = pin("input/es-text", b"stale text");
    assert_eq!(
        repair_impact(&plan, &checked, &bad).unwrap_err().code,
        "review-repair-input-pin"
    );
    let mut bad = original.clone();
    bad.edits[0].after = bad.edits[0].before.clone();
    assert!(repair_impact(&plan, &checked, &bad).is_err());
    let mut bad = original;
    bad.edits.push(bad.edits[0].clone());
    assert_eq!(
        repair_impact(&plan, &checked, &bad).unwrap_err().kind,
        ErrorKind::Conflict
    );
}

#[test]
fn issue_proposals_route_and_deduplicate_without_delivering() {
    let copy = copy(true, "es");
    let mut r = report(&copy);
    let routes = Routes {
        project: "example/project".into(),
        reel: "example/reel".into(),
        tig: "example/tig".into(),
    };
    let mut keys = Vec::new();
    for (route, repo) in [
        (Route::Project, "example/project"),
        (Route::Reel, "example/reel"),
        (Route::Tig, "example/tig"),
    ] {
        r.findings[0].route = route;
        let checked = check_report(&copy, &r).unwrap();
        let proposals = issue_proposals(&checked, &routes).unwrap();
        assert_eq!(proposals[0].repository, repo);
        assert_eq!(
            proposals[0].operation_key,
            issue_proposals(&checked, &routes).unwrap()[0].operation_key
        );
        let uppercase = Routes {
            project: routes.project.to_ascii_uppercase(),
            reel: routes.reel.to_ascii_uppercase(),
            tig: routes.tig.to_ascii_uppercase(),
        };
        assert_eq!(
            proposals[0].operation_key,
            issue_proposals(&checked, &uppercase).unwrap()[0].operation_key
        );
        keys.push(proposals[0].operation_key.clone());
    }
    assert_ne!(keys[0], keys[1]);
    assert_ne!(keys[1], keys[2]);
    let checked = check_report(&copy, &r).unwrap();
    assert!(
        issue_proposals(
            &checked,
            &Routes {
                project: "https://example.invalid/a/b".into(),
                ..routes
            }
        )
        .is_err()
    );
}

#[test]
fn tracker_draft_uses_explicit_text_and_keeps_private_review_context_out_of_payload() {
    let copy = copy(true, "es");
    let mut report = report(&copy);
    report.findings[0].requested_change = "PRIVATE REVIEW NOTE".into();
    let checked = check_report(&copy, &report).unwrap();
    let key = checked.findings().keys().next().unwrap().clone();
    let routes = Routes {
        project: "example/project".into(),
        reel: "example/reel".into(),
        tig: "example/tig".into(),
    };
    let text = tracker::Text {
        schema: "reel.workflow-issue-text.v1".into(),
        finding_key: key,
        repository: "EXAMPLE/PROJECT".into(),
        title: "Explicit synthetic title".into(),
        body: "Explicit shareable text only.".into(),
        synthetic_reproduction: None,
    };
    let draft = tracker::draft(&checked, &routes, &text).unwrap();
    let payload = String::from_utf8(draft.payload().unwrap()).unwrap();
    assert!(!payload.contains("PRIVATE"));
    assert!(!payload.contains("finding_key"));
    assert!(!payload.contains("report_sha256"));
    assert!(!payload.contains("publication/source"));
    assert_eq!(
        draft.operation_id,
        issue_proposals(&checked, &routes).unwrap()[0].operation_key
    );
    let mut wrong = text.clone();
    wrong.repository = "example/other".into();
    assert_eq!(
        tracker::draft(&checked, &routes, &wrong).unwrap_err().kind,
        ErrorKind::Conflict
    );
    report.findings[0].route = Route::Reel;
    let checked = check_report(&copy, &report).unwrap();
    let mut generic = text;
    generic.repository = "example/reel".into();
    assert_eq!(
        tracker::draft(&checked, &routes, &generic)
            .unwrap_err()
            .kind,
        ErrorKind::Denied
    );
    generic.synthetic_reproduction =
        Some(pin("synthetic/reproduction", b"bounded synthetic fixture"));
    tracker::draft(&checked, &routes, &generic).unwrap();
}

#[test]
fn finding_and_expanded_issue_output_budgets_refuse() {
    let mut copy = copy(false, "es");
    let unit = copy.units["stable-unit"].clone();
    for i in 0..1000 {
        copy.units
            .insert(format!("{i}-{}", "x".repeat(128)), unit.clone());
    }
    check_copy(&copy).unwrap();
    let mut r = report(&copy);
    let finding = r.findings[0].clone();
    r.findings = (0..12)
        .map(|i| Finding {
            code: format!("defect-{i}"),
            ..finding.clone()
        })
        .collect();
    let checked = check_report(&copy, &r).unwrap();
    let routes = Routes {
        project: "example/project".into(),
        reel: "example/reel".into(),
        tig: "example/tig".into(),
    };
    assert_eq!(
        issue_proposals(&checked, &routes).unwrap_err().kind,
        ErrorKind::Budget
    );
    r.findings = vec![finding; 129];
    assert_eq!(
        check_report(&copy, &r).unwrap_err().code,
        "review-finding-count"
    );
}

#[test]
fn repair_revision_materializes_exact_inputs_and_preserves_other_languages() {
    for comic in [false, true] {
        let copy = copy(comic, "es");
        let checked = check_report(&copy, &report(&copy)).unwrap();
        let baseline = plan();
        let repair = repair(&checked, pin("input/es-text", b"Spanish text"));
        let revision = reel_workflow::repair::Revision {
            schema: "reel.workflow-repair-revision.v1".into(),
            source_commit: "b".repeat(40),
            native_inputs: BTreeMap::new(),
        };
        let candidate =
            reel_workflow::repair::materialize(&baseline, &checked, &repair, &revision).unwrap();
        assert_eq!(
            candidate.plan.tasks[1].inputs,
            vec![repair.edits[0].after.clone()]
        );
        for index in [0, 2, 3, 4, 5] {
            assert_eq!(candidate.plan.tasks[index], baseline.tasks[index]);
        }
        assert_eq!(
            candidate.impact.rebuild,
            ["es-lettering", "es-reading", "issue"]
        );
        assert_eq!(candidate.plan.source_commit, revision.source_commit);
        assert_eq!(
            candidate.plan_sha256,
            canonical_digest(&candidate.plan).unwrap()
        );
        let mut bad = revision.clone();
        bad.source_commit = baseline.source_commit.clone();
        assert!(reel_workflow::repair::materialize(&baseline, &checked, &repair, &bad).is_err());
        bad.source_commit = "0".repeat(40);
        assert!(reel_workflow::repair::materialize(&baseline, &checked, &repair, &bad).is_err());
    }
}

#[test]
fn repair_revision_requires_every_native_declaration_and_updates_tool_references() {
    let copy = copy(true, "es");
    let checked = check_report(&copy, &report(&copy)).unwrap();
    let mut baseline = plan();
    for task in &mut baseline.tasks {
        task.native_inputs = Some(pin(&format!("native/{}", task.id), b"old declaration"));
    }
    let publication = pin("input/publication", b"old master");
    let request = pin("input/request", b"producer request");
    let t = &mut baseline.tasks[0];
    t.operation = Operation::DeterministicTool;
    t.prompt.clear();
    t.inputs.extend([publication.clone(), request.clone()]);
    t.tool = Some(ToolContract {
        schema: "reel.workflow-tool.v1".into(),
        capability: ToolCapability::ComicRender,
        executable: pin("tool/reel", b"runtime"),
        auxiliary_executables: BTreeMap::new(),
        publication: publication.clone(),
        edition_id: "es".into(),
        request,
        source_bindings: BTreeMap::new(),
        part: None,
    });
    let repair = repair(&checked, publication);
    let mut revision = reel_workflow::repair::Revision {
        schema: "reel.workflow-repair-revision.v1".into(),
        source_commit: "b".repeat(40),
        native_inputs: BTreeMap::new(),
    };
    assert_eq!(
        reel_workflow::repair::materialize(&baseline, &checked, &repair, &revision)
            .unwrap_err()
            .code,
        "repair-revision-native-task-set"
    );
    for task in &baseline.tasks {
        let before = task.native_inputs.clone().unwrap();
        revision.native_inputs.insert(
            task.id.clone(),
            Edit {
                after: pin(&before.logical_id, b"new declaration"),
                before,
            },
        );
    }
    let candidate =
        reel_workflow::repair::materialize(&baseline, &checked, &repair, &revision).unwrap();
    assert_eq!(
        candidate.plan.tasks[0].tool.as_ref().unwrap().publication,
        repair.edits[0].after
    );
    let runtime = baseline.tasks[0].tool.as_ref().unwrap().executable.clone();
    let runtime_repair = self::repair(&checked, runtime);
    let runtime_candidate =
        reel_workflow::repair::materialize(&baseline, &checked, &runtime_repair, &revision)
            .unwrap();
    assert_eq!(runtime_candidate.impact.directly_affected, ["art"]);
    assert_eq!(
        runtime_candidate.plan.tasks[0]
            .tool
            .as_ref()
            .unwrap()
            .executable,
        runtime_repair.edits[0].after
    );
    assert_eq!(candidate.impact.rebuild.len(), baseline.tasks.len());
    assert_ne!(
        candidate.plan.tasks[2].native_inputs,
        baseline.tasks[2].native_inputs
    );
    revision.native_inputs.get_mut("art").unwrap().before.sha256 = "f".repeat(64);
    assert_eq!(
        reel_workflow::repair::materialize(&baseline, &checked, &repair, &revision)
            .unwrap_err()
            .kind,
        ErrorKind::Stale
    );
}

#[test]
fn cli_exports_new_private_controls_and_preserves_existing_files() {
    let root = tempfile::tempdir().unwrap();
    let copy = copy(true, "es");
    let report = report(&copy);
    let checked = check_report(&copy, &report).unwrap();
    let plan = plan();
    let repair = repair(&checked, pin("input/es-text", b"Spanish text"));
    let values = [
        ("copy", serde_json::to_value(&copy).unwrap()),
        ("report", serde_json::to_value(report).unwrap()),
        ("plan", serde_json::to_value(plan).unwrap()),
        ("repair", serde_json::to_value(repair).unwrap()),
        (
            "revision",
            serde_json::json!({"schema":"reel.workflow-repair-revision.v1","source_commit":"b".repeat(40),"native_inputs":{}}),
        ),
        (
            "routes",
            serde_json::json!({"project":"example/project","reel":"example/reel","tig":"example/tig"}),
        ),
    ];
    for (name, value) in values {
        std::fs::write(root.path().join(name), serde_json::to_vec(&value).unwrap()).unwrap();
    }
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run(&["check-review", "copy", "report"]).status.success());
    assert!(
        run(&[
            "export-review-issues",
            "copy",
            "report",
            "routes",
            "proposals"
        ])
        .status
        .success()
    );
    let original = std::fs::read(root.path().join("proposals")).unwrap();
    assert!(
        !run(&[
            "export-review-issues",
            "copy",
            "report",
            "routes",
            "proposals"
        ])
        .status
        .success()
    );
    assert_eq!(
        original,
        std::fs::read(root.path().join("proposals")).unwrap()
    );
    let out = run(&["plan-repair", "plan", "copy", "report", "repair", "impact"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["edits_applied"], false);
    assert_eq!(result["findings_closed"], false);
    let args = [
        "revise-repair",
        "plan",
        "copy",
        "report",
        "repair",
        "revision",
        "candidate",
    ];
    let out = run(&args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let candidate = std::fs::read(root.path().join("candidate")).unwrap();
    let executable_plan: Plan = serde_json::from_slice(&candidate).unwrap();
    check_plan(&executable_plan).unwrap();
    assert!(!run(&args).status.success());
    assert_eq!(
        candidate,
        std::fs::read(root.path().join("candidate")).unwrap()
    );
}
