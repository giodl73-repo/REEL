//! Retained whole-issue controls and exact part PNGs -> protected report handoff.
//! Scripted technical findings exercise protocol scope, not visual/creative review.
use super::*;
use reel_workflow::publication::InputReader;
use reel_workflow_native::{
    NativeInputs, NativeProfile, admission, handoff, retention, staging, upstream,
};
use std::io::Read;

fn load<T: serde::de::DeserializeOwned>(root: &Path, name: &str) -> T {
    decode(&fs::read(root.join(name)).unwrap()).unwrap()
}
fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    let path = root.join(name);
    assert!(!path.exists(), "existing evidence refused: {name}");
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn invoke(exe: &Path, root: &Path, name: &str, args: &[&std::ffi::OsStr]) -> std::process::Output {
    let out = Command::new(exe).args(args).output().unwrap();
    save(
        root,
        name,
        &json!({"exit_code":out.status.code(),"stdout":String::from_utf8_lossy(&out.stdout),"stderr":String::from_utf8_lossy(&out.stderr)}),
    );
    out
}
fn success(out: &std::process::Output) {
    assert!(
        out.status.success(),
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "Read-only audit of the original pre-admission projection refusal; proves no close/new admission and an unchanged actual Index checkpoint before explicit replan"]
fn refused_projection_preserves_original_control_activation_before_replanning() {
    let failed = PathBuf::from(
        std::env::var_os("REEL_ISSUE_REPORT_REFUSED_PROJECTION").expect("original failed evidence"),
    );
    let original = PathBuf::from(
        std::env::var_os("REEL_ISSUE_REPORT_CONTROL_ROOT").expect("original retained control"),
    );
    let process: serde_json::Value = load(&failed, "process-result.json");
    assert_eq!(process["exit_code"], 101);
    assert_eq!(
        fs::read_dir(failed.join("job"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsString::from("original-pins.json")]
    );
    let pins: BTreeMap<PathBuf, String> = load(&failed.join("job"), "original-pins.json");
    for (path, pin) in &pins {
        assert_eq!(&digest(&fs::read(path).unwrap()), pin);
    }
    let packet: Packet = load(&original, "packet.json");
    let profile: handoff::Profile = load(&original, "profile.json");
    assert_eq!(
        reel_workflow::handoff::Journal::open(&original.join("journal"))
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Retained
    );
    drop(
        admission::Prepared::open(
            &packet,
            &profile.source,
            &profile.admission,
            &profile.binding.admission.attempt_id,
        )
        .unwrap()
        .reopen(&profile.binding.admission)
        .unwrap(),
    );
    let index = tig_store::source_claims::protection::Index::open(
        &profile.admission.index_root,
        &profile.binding.admission.index_checkpoint,
    )
    .unwrap();
    let checkpoint = canonical_digest(&index.checkpoint()).unwrap();
    let previous: serde_json::Value = load(original.parent().unwrap(), "qualification-result.json");
    assert_eq!(previous["checkpoint_sha256"], checkpoint);
    save(
        &failed,
        "original-state-after-refusal.json",
        &json!({"schema":"reel.whole-issue-report-refusal-audit.v1","original_task_key":packet.task_key,"actual_checkpoint_sha256":checkpoint,"original_control_activation_reopened":true,"original_controls_unchanged":true,"new_admission":false,"original_close_started":false,"runtime_adopted":false}),
    );
}

#[test]
#[ignore = "Existing retained synthetic whole-issue controls only; guarded same-slot reuse, exact PNG material, protected manual report refusal/retention/replay; no model/tracker/selection or deletion"]
fn retained_whole_issue_controls_and_reading_images_drive_protected_reports() {
    let original = PathBuf::from(
        std::env::var_os("REEL_ISSUE_REPORT_CONTROL_ROOT").expect("retained control job root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_ISSUE_REPORT_EVIDENCE").expect("new evidence directory"),
    );
    let runtime = PathBuf::from(
        std::env::var_os("REEL_ISSUE_REPORT_RUNTIME").expect("frozen new-task runtime"),
    );
    let waiting = PathBuf::from(
        std::env::var_os("REEL_ISSUE_REPORT_WAITING_ROOT").expect("other protected Waiting root"),
    );
    assert!(
        original.is_absolute()
            && evidence.is_absolute()
            && evidence.is_dir()
            && runtime.is_absolute()
            && waiting.is_absolute()
    );
    let root = evidence.join("job");
    fs::create_dir(&root).unwrap();
    let producer: Packet = load(&original, "packet.json");
    let produced: ReturnedResult = load(&original, "result.json");
    let producer_profile: handoff::Profile = load(&original, "profile.json");
    let producer_ack: retention::Acknowledgement = load(&original, "retained.json");
    assert_eq!(producer.project, "synthetic-comic");
    assert_eq!(producer.task.id, "owned-issue-review-controls");
    let waiting_packet: Packet = load(&waiting, "next-packet.json");
    let waiting_profile: handoff::Profile = load(&waiting, "next-handoff-profile.json");
    let check_waiting = || {
        let journal = reel_workflow::handoff::Journal::open(&waiting.join("next-journal")).unwrap();
        assert_eq!(journal.state(), reel_workflow::handoff::State::Waiting);
        assert_eq!(journal.ticket().packet, waiting_packet);
        drop(
            admission::Prepared::open(
                &waiting_packet,
                &waiting_profile.source,
                &waiting_profile.admission,
                &waiting_profile.binding.admission.attempt_id,
            )
            .unwrap()
            .reopen(&waiting_profile.binding.admission)
            .unwrap(),
        );
    };
    check_waiting();
    let preserved = [
        original.join("packet.json"),
        original.join("profile.json"),
        original.join("result.json"),
        original.join("retained.json"),
        waiting.join("next-packet.json"),
        waiting.join("next-handoff-profile.json"),
    ];
    let pins: BTreeMap<_, _> = preserved
        .iter()
        .map(|path| (path.clone(), digest(&fs::read(path).unwrap())))
        .collect();
    save(&root, "original-pins.json", &pins);
    // Reopen native control outputs after complete producer/custody verification;
    // local convenience copies alone are not input authority.
    drop(
        retention::verify(
            &producer,
            &producer_profile.source,
            &producer_profile.admission,
            &producer_profile.binding.admission,
            &produced,
            &producer_ack,
        )
        .unwrap(),
    );
    let mut controls = BTreeMap::new();
    for (name, pin) in &produced.artifacts {
        let mut stream = tig_git::annex::workflow::reader::open(
            &producer_profile.source.source,
            &Object {
                sha256: tig_core::Digest::parse(&pin.sha256).unwrap(),
                bytes: pin.bytes,
            },
        )
        .unwrap();
        let mut raw = Vec::new();
        Read::by_ref(&mut stream)
            .take(pin.bytes + 1)
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw.len() as u64, pin.bytes);
        assert_eq!(digest(&raw), pin.sha256);
        controls.insert(name.clone(), raw);
    }
    let copy: review::Copy = decode(&controls["review-copy.json"]).unwrap();
    let images = review::comic_reading_inputs(&copy, &controls["review-sources.json"]).unwrap();
    assert_eq!(images.len(), 2);
    let originals: upstream::Parents = load(&original, "parents.json");
    let mut parents = upstream::Parents {
        schema: "reel.workflow-native-parents.v1".into(),
        parents: BTreeMap::new(),
    };
    let mut bindings = reel_workflow::upstream::Bindings {
        schema: "reel.workflow-upstream-inputs.v2".into(),
        streaming: None,
        inputs: BTreeMap::new(),
    };
    let mut inputs = vec![copy.publication.clone()];
    for (name, pin) in &produced.artifacts {
        inputs.push(pin.clone());
        bindings.inputs.insert(
            pin.logical_id.clone(),
            reel_workflow::upstream::Reference {
                task_id: producer.task.id.clone(),
                artifact: reel_workflow::upstream::Selector::Output { name: name.clone() },
            },
        );
    }
    for image in &images {
        let matches: Vec<_> = originals
            .parents
            .iter()
            .flat_map(|(id, p)| {
                p.result
                    .artifacts
                    .iter()
                    .filter(move |(_, pin)| *pin == image)
                    .map(move |(name, _)| (id, name))
            })
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "reading image must retain its exact original producer"
        );
        let (id, name) = matches[0];
        assert!(matches!(id.as_str(), "opening" | "closing"));
        let parent: upstream::ParentControl =
            decode(&serde_json::to_vec(&originals.parents[id]).unwrap()).unwrap();
        parents.parents.insert(id.clone(), parent);
        inputs.push(image.clone());
        bindings.inputs.insert(
            image.logical_id.clone(),
            reel_workflow::upstream::Reference {
                task_id: id.clone(),
                artifact: reel_workflow::upstream::Selector::Output { name: name.clone() },
            },
        );
    }
    parents.parents.insert(
        producer.task.id.clone(),
        upstream::ParentControl {
            packet: producer.clone(),
            result: produced.clone(),
            profile: load(&original, "profile.json"),
            acknowledgement: producer_ack,
        },
    );
    assert_eq!(inputs.len(), 6);
    assert!(
        inputs
            .iter()
            .all(|pin| matches!(pin.media_type.as_str(), "application/json" | "image/png"))
    );
    let mut source: NativeProfile =
        decode(&serde_json::to_vec(&producer_profile.source).unwrap()).unwrap();
    source.snapshot = source
        .snapshot
        .fixed_subset(&std::collections::BTreeSet::from([copy
            .publication
            .logical_id
            .clone()]))
        .unwrap();
    source.snapshot.upstream = Some(upstream::Frozen::capture(
        &bindings,
        &parents.borrowed().unwrap(),
    ));
    let raw = bindings.canonical_bytes().unwrap();
    let task=Task {id:"whole-issue-structured-report".into(),native_inputs:Some(Fixture::declaration(&source.snapshot)),upstream_inputs:Some(Artifact {logical_id:"control/whole-issue-report-origins".into(),sha256:digest(&raw),bytes:raw.len() as u64,media_type:"application/json".into()}),operation:Operation::ReviewReport,tool:None,language:Some(copy.language.clone()),prompt:"Synthetic whole-issue technical report fixture. Preserve exact Spanish source wording and all original page/panel/balloon IDs. No creative defect, model viewing or approval is asserted.".into(),inputs,depends_on:parents.parents.keys().cloned().collect(),outputs:BTreeMap::from([("report".into(),OutputContract {media_type:"application/json".into(),required:true,max_bytes:MAX_CONTROL_BYTES as u64})])};
    let mut plan: Plan = load(&original, "plan.json");
    plan.tasks.push(task);
    let packet = make_packet(
        &plan,
        "whole-issue-structured-report",
        parents
            .parents
            .iter()
            .map(|(id, p)| {
                (
                    id.clone(),
                    DependencyResult {
                        result_sha256: canonical_digest(&p.result).unwrap(),
                        artifacts: p.result.artifacts.clone(),
                    },
                )
            })
            .collect(),
    )
    .unwrap();
    // Pure native projection preflight precedes any slot close/admission.
    let snapshot = SnapshotInputs::open(&packet, &source.snapshot).unwrap();
    assert_eq!(snapshot.input_count(), 6);
    drop(snapshot);
    let runtime_raw = fs::read(&runtime).unwrap();
    let runtime_pin = Tool {
        path: runtime.clone(),
        sha256: digest(&runtime_raw),
        bytes: runtime_raw.len() as u64,
    };
    assert_ne!(runtime_pin.sha256, producer_profile.runtime.sha256);
    let mut admission = producer_profile.admission.clone();
    assert_eq!(admission.scope.actor.as_str(), "slot2");
    admission.scope.operation = tig_store::Key::parse("whole-issue-report-original").unwrap();
    save(&root, "packet.json", &packet);
    save(&root, "plan.json", &plan);
    save(&root, "source.json", &source);
    save(&root, "parents.json", &parents);
    save(&root, "upstream.json", &bindings);
    save(&root, "admission.json", &admission);
    save(&root, "runtime.json", &runtime_pin);
    save(
        &root,
        "original-operation.json",
        &json!({"schema":"reel.whole-issue-report-original.v1","task_key":packet.task_key,"attempt_id":"issue-report-attempt-1","scope":admission.scope,"runtime":runtime_pin,"input_images":images,"new_task":true,"original_runtime_adopted":false}),
    );
    // Close only the completed control job through its original frozen runtime.
    let close = root.join("control-close");
    fs::create_dir(&close).unwrap();
    save(&close, "target.json", &producer_profile.binding.target);
    let ticket = close.join("ticket.json");
    success(&invoke(
        &producer_profile.runtime.path,
        &close,
        "prepare-result.json",
        &[
            "prepare-close".as_ref(),
            original.join("journal").as_os_str(),
            original.join("profile.json").as_os_str(),
            close.join("target.json").as_os_str(),
            ticket.as_os_str(),
            "--apply".as_ref(),
        ],
    ));
    let ticket_pin = digest(&fs::read(&ticket).unwrap());
    save(&close, "ticket-pin.json", &ticket_pin);
    success(&invoke(
        &producer_profile.runtime.path,
        &close,
        "apply-result.json",
        &[
            "close-job".as_ref(),
            ticket.as_os_str(),
            ticket_pin.as_ref(),
            close.join("closed.json").as_os_str(),
            "--apply".as_ref(),
        ],
    ));
    let closed: reel_workflow_native::closing::Outcome = load(&close, "closed.json");
    assert!(closed.slot_idle && closed.original_clear_complete);
    let prepared = upstream::Admission::prepare(
        &packet,
        &source,
        &admission,
        "issue-report-attempt-1",
        parents.borrowed().unwrap(),
    )
    .unwrap();
    let preview = prepared.preview().unwrap();
    assert!(preview.declared_objects + 4 <= preview.changed_path_limit);
    save(&root, "admission-preview.json", &preview);
    let receipt = prepared.accept().unwrap();
    save(&root, "receipt.json", &receipt);
    let acks = root.join("acknowledgements");
    fs::create_dir(&acks).unwrap();
    let profile = handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source,
        admission,
        binding: handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            retained_owner_policy: None,
            admission: receipt.clone(),
            target: producer_profile.binding.target.clone(),
        },
        runtime: runtime_pin,
        acknowledgements: acks,
    };
    save(&root, "profile.json", &profile);
    let journal = root.join("journal");
    success(&invoke(
        &runtime,
        &root,
        "create-result.json",
        &[
            "create-handoff".as_ref(),
            root.join("packet.json").as_os_str(),
            root.join("profile.json").as_os_str(),
            journal.as_os_str(),
            "--apply".as_ref(),
        ],
    ));
    let stage_parent = root.join("input-scratch");
    fs::create_dir(&stage_parent).unwrap();
    let stage = staging::Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent: stage_parent,
        maximum_bytes: 64 * 1024 * 1024,
        capacity_policy: reel_assets::admission::Policy {
            minimum_free_bytes: 65536,
            minimum_free_basis_points: 0,
            max_object_bytes: 64 * 1024 * 1024,
            ..Default::default()
        },
    };
    let staged = staging::prepare(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &stage,
    )
    .unwrap();
    save(&root, "input-ticket.json", &staged);
    success(&invoke(
        &runtime,
        &root,
        "brief-result.json",
        &[
            "export-review-brief".as_ref(),
            root.join("packet.json").as_os_str(),
            root.join("source.json").as_os_str(),
            root.join("admission.json").as_os_str(),
            root.join("receipt.json").as_os_str(),
            root.join("review-brief.json").as_os_str(),
            "--apply".as_ref(),
        ],
    ));
    let brief: review::Brief = load(&root, "review-brief.json");
    assert_eq!(brief.units.len(), 6);
    assert_eq!(brief.evidence.len(), 6);
    for image in &images {
        assert!(brief.evidence.contains(image));
    }
    let mut native =
        NativeInputs::open_admitted(&packet, &profile.source, &profile.admission, &receipt)
            .unwrap();
    let bound = review::bind(&packet, &mut native).unwrap();
    for image in &images {
        let raw = native.read(image, image.bytes).unwrap();
        assert!(raw.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
    drop(native);
    let findings=["page01","page02"].into_iter().map(|page|review::Finding {code:format!("synthetic-{page}-target"),unit:copy.units.iter().find(|(_,u)|matches!(u,review::Unit::Comic {page_id,balloon_id:Some(_),..} if page_id==page)).unwrap().0.clone(),frames:None,modality:review::Modality::Text,route:review::Route::Project,requested_change:"Synthetic metadata-target test; preserve source wording. This asserts no creative defect or actual viewing.".into(),evidence:vec![copy.review.clone()]}).collect();
    let report = review::Report {
        schema: "reel.workflow-review-report.v1".into(),
        copy_sha256: canonical_digest(&copy).unwrap(),
        findings,
    };
    let checkpoint = || {
        let index = tig_store::source_claims::protection::Index::open(
            &profile.admission.index_root,
            &receipt.index_checkpoint,
        )
        .unwrap();
        canonical_digest(&index.checkpoint()).unwrap()
    };
    let before = checkpoint();
    let waiting_record = fs::read(journal.join("00000.json")).unwrap();
    for kind in ["copy", "unit", "evidence"] {
        let refused = root.join(format!("refused-{kind}"));
        fs::create_dir(&refused).unwrap();
        let mut bad = report.clone();
        match kind {
            "copy" => bad.copy_sha256 = "b".repeat(64),
            "unit" => bad.findings[1].unit = "nonexistent-other-page".into(),
            "evidence" => bad.findings[0].evidence[0].sha256 = "0".repeat(64),
            _ => unreachable!(),
        }
        let raw = serde_json::to_vec(&bad).unwrap();
        fs::write(refused.join("report.json"), &raw).unwrap();
        let result = ReturnedResult {
            schema: RESULT_SCHEMA.into(),
            task_id: packet.task.id.clone(),
            task_key: packet.task_key.clone(),
            attempt_id: receipt.attempt_id.clone(),
            artifacts: BTreeMap::from([(
                "report".into(),
                Artifact {
                    logical_id: format!(
                        "workflow/review/{}/{}/report",
                        packet.task_key, receipt.attempt_id
                    ),
                    sha256: digest(&raw),
                    bytes: raw.len() as u64,
                    media_type: "application/json".into(),
                },
            )]),
            provenance: Provenance {
                mode: Mode::Handoff,
                provider: None,
                model: None,
                executable: None,
            },
            worker_receipt: None,
        };
        save(&refused, "result.json", &result);
        save(
            &refused,
            "returns.json",
            &reel_workflow_native::response::Returns {
                schema: "reel.workflow-returns.v1".into(),
                root: refused.clone(),
                files: BTreeMap::from([("report".into(), "report.json".into())]),
            },
        );
        let out = invoke(
            &runtime,
            &refused,
            "import-result.json",
            &[
                "import-handoff".as_ref(),
                journal.as_os_str(),
                root.join("profile.json").as_os_str(),
                refused.join("result.json").as_os_str(),
                refused.join("returns.json").as_os_str(),
                "--apply".as_ref(),
            ],
        );
        assert!(!out.status.success());
        let error: serde_json::Value = decode(&out.stdout).unwrap();
        let expected = match kind {
            "copy" => "review-copy-pin",
            "unit" => "review-finding-unit",
            "evidence" => "review-task-evidence-pin",
            _ => unreachable!(),
        };
        assert_eq!(error["error"]["code"], expected);
        assert_eq!(
            reel_workflow::handoff::Journal::open(&journal)
                .unwrap()
                .state(),
            reel_workflow::handoff::State::Waiting
        );
        assert_eq!(fs::read_dir(&journal).unwrap().count(), 1);
        assert_eq!(
            waiting_record,
            fs::read(journal.join("00000.json")).unwrap()
        );
        assert_eq!(before, checkpoint());
    }
    let returns = root.join("returns");
    fs::create_dir(&returns).unwrap();
    let raw = serde_json::to_vec_pretty(&report).unwrap();
    fs::write(returns.join("report.json"), &raw).unwrap();
    let packed = bound.pack_handoff(&receipt.attempt_id, &raw).unwrap();
    success(&invoke(
        &runtime,
        &root,
        "pack-result.json",
        &[
            "pack-review-result".as_ref(),
            root.join("packet.json").as_os_str(),
            root.join("source.json").as_os_str(),
            root.join("admission.json").as_os_str(),
            root.join("receipt.json").as_os_str(),
            returns.join("report.json").as_os_str(),
            root.join("result.json").as_os_str(),
            "--apply".as_ref(),
        ],
    ));
    let result: ReturnedResult = load(&root, "result.json");
    assert_eq!(result, packed);
    save(
        &root,
        "returns.json",
        &reel_workflow_native::response::Returns {
            schema: "reel.workflow-returns.v1".into(),
            root: returns,
            files: BTreeMap::from([("report".into(), "report.json".into())]),
        },
    );
    success(&invoke(
        &runtime,
        &root,
        "import-result.json",
        &[
            "import-handoff".as_ref(),
            journal.as_os_str(),
            root.join("profile.json").as_os_str(),
            root.join("result.json").as_os_str(),
            root.join("returns.json").as_os_str(),
            "--apply".as_ref(),
        ],
    ));
    assert_eq!(
        reel_workflow::handoff::Journal::open(&journal)
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Retained
    );
    let ack: retention::Acknowledgement = decode(
        &fs::read(profile.acknowledgements.join(format!(
            "retained-{}-{}.json",
            packet.task_key, receipt.attempt_id
        )))
        .unwrap(),
    )
    .unwrap();
    save(&root, "retained.json", &ack);
    let completed = checkpoint();
    success(&invoke(
        &runtime,
        &root,
        "resume-result.json",
        &[
            "resume-handoff".as_ref(),
            journal.as_os_str(),
            root.join("profile.json").as_os_str(),
            "-".as_ref(),
            "--apply".as_ref(),
        ],
    ));
    assert_eq!(completed, checkpoint());
    let guard = retention::verify(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &ack,
    )
    .unwrap();
    assert_eq!(guard.review_report().unwrap().findings().len(), 2);
    drop(guard);
    for (path, pin) in &pins {
        assert_eq!(&digest(&fs::read(path).unwrap()), pin);
    }
    check_waiting();
    assert_eq!(completed, checkpoint());
    save(
        &evidence,
        "qualification-result.json",
        &json!({"schema":"reel.whole-issue-report-qualification.v1","job":root,"task_key":packet.task_key,"attempt_id":receipt.attempt_id,"pages":2,"units":brief.units.len(),"reading_images_declared_and_staged":2,"stale_copy_unknown_unit_and_wrong_evidence_refused_before_intent":true,"refusals_preserved_actual_checkpoint":true,"report_retained":true,"task_completed":true,"replay_without_returns":true,"actual_replay_checkpoint_unchanged":true,"checkpoint_sha256":completed,"original_controls_unchanged":true,"other_waiting_generation_reopened":true,"findings":2,"provider_called":false,"viewing_verified":false,"approved":false,"selected":false,"scratch_retired":false,"runtime_adopted":false}),
    );
}
