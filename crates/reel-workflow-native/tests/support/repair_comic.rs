//! A real retained synthetic finding -> corrected committed sources -> owned
//! render/retention -> new full-edition review copy. No creative approval.
use super::*;
use reel_publication::{
    Content, Publication,
    comic_source::{PageSource, Request},
};
use reel_workflow::publication::InputReader;
use reel_workflow_native::{NativeInputs, NativeProfile, admission::Prepared, retention};
use std::io::Read;

fn load<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> T {
    decode(&fs::read(path).unwrap()).unwrap()
}
fn save(path: impl AsRef<Path>, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn ack_path(profile: &reel_workflow_native::handoff::Profile, packet: &Packet) -> PathBuf {
    profile.acknowledgements.join(format!(
        "retained-{}-{}.json",
        packet.task_key, profile.binding.admission.attempt_id
    ))
}
fn native_bytes(source: &NativeProfile, pin: &Artifact) -> Vec<u8> {
    let object = Object {
        sha256: tig_core::Digest::parse(&pin.sha256).unwrap(),
        bytes: pin.bytes,
    };
    let mut held = tig_git::annex::workflow::reader::open(&source.source, &object).unwrap();
    let mut raw = Vec::new();
    Read::by_ref(&mut held)
        .take(pin.bytes + 1)
        .read_to_end(&mut raw)
        .unwrap();
    assert_eq!(raw.len() as u64, pin.bytes);
    assert_eq!(digest(&raw), pin.sha256);
    raw
}

#[test]
#[ignore = "Resume only the preserved corrected-comic operation after runtime-pin refusal; no new admission or input publication"]
fn resume_original_corrected_comic_after_runtime_pin_refusal() {
    use reel_workflow_native::graph;
    let root = PathBuf::from(
        std::env::var_os("REEL_COMIC_REPAIR_RESUME_ROOT")
            .expect("Set original accepted operation root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_COMIC_REPAIR_EVIDENCE_DIRECTORY")
            .expect("Set recovery evidence directory"),
    );
    let marker: serde_json::Value = load(root.join("original-operation.json"));
    assert_eq!(
        PathBuf::from(marker["root"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        root.canonicalize().unwrap()
    );
    let original = PathBuf::from(marker["original"].as_str().unwrap());
    let report_root = PathBuf::from(marker["report"].as_str().unwrap());
    let packet: Packet = load(root.join("packet.json"));
    assert_eq!(marker["task_key"], packet.task_key);
    let profile_path = root.join("handoff-profile.json");
    let profile: reel_workflow_native::handoff::Profile = load(&profile_path);
    let receipt: reel_workflow_native::admission::Receipt = load(root.join("receipt.json"));
    assert_eq!(
        canonical_digest(&receipt).unwrap(),
        canonical_digest(&profile.binding.admission).unwrap()
    );
    let mut admitted =
        NativeInputs::open_admitted(&packet, &profile.source, &profile.admission, &receipt)
            .unwrap();
    reel_workflow::publication::bind_comic(&packet, &mut admitted).unwrap();
    drop(admitted);
    let old_profile: reel_workflow_native::handoff::Profile =
        load(original.join("comic-profile.json"));
    let old_result: ReturnedResult = load(original.join("comic-result.json"));
    let report_packet = load(report_root.join("packet.json"));
    let report_source = load(report_root.join("source.json"));
    let report_profile: reel_workflow_native::admission::Profile =
        load(report_root.join("admission.json"));
    let guard = retention::verify(
        &report_packet,
        &report_source,
        &report_profile,
        &load(report_root.join("receipt.json")),
        &load(report_root.join("result.json")),
        &load(report_root.join("ack.json")),
    )
    .unwrap();
    let checked = guard.review_report().unwrap().clone();
    drop(guard);
    let candidate = repair::materialize(
        &load(root.join("baseline.json")),
        &checked,
        &load(root.join("repair.json")),
        &load(root.join("revision.json")),
    )
    .unwrap();
    assert_eq!(
        make_packet(&candidate.plan, &packet.task.id, BTreeMap::new()).unwrap(),
        packet
    );
    let old_claims = fs::read(old_profile.admission.index_root.join("claims.json")).unwrap();
    let report_claims = fs::read(report_profile.index_root.join("claims.json")).unwrap();
    fs::write(
        evidence.join("original-comic-claims-at-resume.bin"),
        &old_claims,
    )
    .unwrap();
    fs::write(
        evidence.join("original-report-claims-at-resume.bin"),
        &report_claims,
    )
    .unwrap();
    let old_clean = native_bytes(
        &old_profile.source,
        &old_result.artifacts["page01-clean.png"],
    );
    let old_reading = native_bytes(
        &old_profile.source,
        &old_result.artifacts["page01-reading.png"],
    );
    let journal = root.join("journal");
    assert!(
        !journal.exists(),
        "This recovery is only for the pre-journal runtime-pin refusal"
    );
    save(evidence.join("original-operation-roots.json"), &marker);
    save(
        root.join("phase.json"),
        &json!({"phase":"resume-original-accepted-operation-with-pinned-runtime"}),
    );
    let created = Command::new(&profile.runtime.path)
        .arg("create-handoff")
        .arg(root.join("packet.json"))
        .arg(&profile_path)
        .arg(&journal)
        .arg("--apply")
        .output()
        .unwrap();
    fs::write(evidence.join("create-handoff-stdout.json"), &created.stdout).unwrap();
    assert!(
        created.status.success(),
        "{} {}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([(
            packet.task.id.clone(),
            graph::Binding {
                journal: journal.clone(),
                profile: graph::PinnedFile {
                    path: profile_path,
                    sha256: digest(&fs::read(root.join("handoff-profile.json")).unwrap()),
                },
                staging_ticket: None,
            },
        )]),
    };
    let mut controls = Vec::new();
    let result_path = root.join("result.json");
    automatic_dispatch::run_graph_root(
        &root,
        &packet,
        &profile,
        &profile.runtime.path,
        &result_path,
        &mut controls,
        &BTreeMap::new(),
        &candidate.plan,
        &bindings,
    );
    for (name, raw) in controls {
        fs::write(root.join(name), raw).unwrap();
    }
    let result: ReturnedResult = load(&result_path);
    let ack = load(ack_path(&profile, &packet));
    save(root.join("ack.json"), &ack);
    save(
        root.join("phase.json"),
        &json!({"phase":"verify-original-corrected-render-after-recovery"}),
    );
    drop(
        retention::verify(
            &packet,
            &profile.source,
            &profile.admission,
            &receipt,
            &result,
            &ack,
        )
        .unwrap(),
    );
    assert_eq!(
        native_bytes(&profile.source, &result.artifacts["page01-clean.png"]),
        old_clean
    );
    assert_ne!(
        native_bytes(&profile.source, &result.artifacts["page01-reading.png"]),
        old_reading
    );
    assert!(
        String::from_utf8(native_bytes(
            &profile.source,
            &result.artifacts["transcript.json"]
        ))
        .unwrap()
        .contains("Mira la luz.")
    );
    let reviewed = retention::review_copy(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &ack,
    )
    .unwrap();
    assert_eq!(reviewed.copy().units, checked.copy().units);
    assert_ne!(reviewed.copy().review, checked.copy().review);
    assert_eq!(reviewed.copy().source_commit, packet.source_commit);
    let old_report = review::Report {
        schema: "reel.workflow-review-report.v1".into(),
        copy_sha256: canonical_digest(checked.copy()).unwrap(),
        findings: checked
            .findings()
            .values()
            .map(|f| f.finding().clone())
            .collect(),
    };
    assert_eq!(
        review::check_report(reviewed.copy(), &old_report)
            .unwrap_err()
            .kind,
        ErrorKind::Stale
    );
    save(root.join("corrected-review-copy.json"), reviewed.copy());
    fs::write(
        root.join("corrected-review-manifest.json"),
        reviewed.manifest().unwrap(),
    )
    .unwrap();
    drop(reviewed);
    assert_eq!(
        canonical_digest(&receipt).unwrap(),
        canonical_digest(&load::<reel_workflow_native::admission::Receipt>(
            root.join("receipt.json")
        ))
        .unwrap()
    );
    assert_eq!(
        old_claims,
        fs::read(old_profile.admission.index_root.join("claims.json")).unwrap()
    );
    assert_eq!(
        report_claims,
        fs::read(report_profile.index_root.join("claims.json")).unwrap()
    );
    let outcome = json!({"root":root,"fixture":marker["fixture"],"finding_bound":true,"new_source_commit":packet.source_commit,"original_task_key":packet.task_key,"original_receipt_unchanged":true,"original_admitted_inputs_reopened_before_recovery":true,"readmission":false,"input_republication":false,"retained":true,"replay_without_render_or_claim_changes":true,"clean_art_unchanged":true,"reading_pixels_changed":true,"corrected_transcript_verified":true,"review_unit_ids_preserved":true,"stale_report_refused":true,"old_claims_unchanged_during_resume":true,"human_reviewed":false,"selected":false,"issue_closed":false});
    save(root.join("qualification-result.json"), &outcome);
    save(evidence.join("qualification-result.json"), &outcome);
    save(
        root.join("phase.json"),
        &json!({"phase":"qualified-corrected-render-awaiting-contextual-review"}),
    );
}

#[test]
#[ignore = "Existing retained synthetic comic/report only; creates a new protected corrected render and review copy in the existing pool, preserves every root"]
fn retained_finding_drives_corrected_comic_render_retention_and_contextual_review_copy() {
    let original = PathBuf::from(
        std::env::var_os("REEL_COMIC_REPAIR_ORIGINAL").expect("Set retained comic root"),
    );
    let report_root = PathBuf::from(
        std::env::var_os("REEL_COMIC_REPAIR_REPORT").expect("Set retained report root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_COMIC_REPAIR_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let controller =
        PathBuf::from(std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set pinned controller"));
    let old_packet: Packet = load(original.join("comic-packet.json"));
    let old_profile: reel_workflow_native::handoff::Profile =
        load(original.join("comic-profile.json"));
    let old_result: ReturnedResult = load(original.join("comic-result.json"));
    let old_ack = load(ack_path(&old_profile, &old_packet));
    drop(
        retention::verify(
            &old_packet,
            &old_profile.source,
            &old_profile.admission,
            &old_profile.binding.admission,
            &old_result,
            &old_ack,
        )
        .unwrap(),
    );
    let report_packet = load(report_root.join("packet.json"));
    let report_source = load(report_root.join("source.json"));
    let report_profile: reel_workflow_native::admission::Profile =
        load(report_root.join("admission.json"));
    let report_guard = retention::verify(
        &report_packet,
        &report_source,
        &report_profile,
        &load(report_root.join("receipt.json")),
        &load(report_root.join("result.json")),
        &load(report_root.join("ack.json")),
    )
    .unwrap();
    let checked = report_guard.review_report().unwrap().clone();
    drop(report_guard);
    let tool = old_packet.task.tool.as_ref().unwrap();
    assert_eq!(checked.copy().publication, tool.publication);
    assert_eq!(checked.copy().source_commit, old_packet.source_commit);
    assert_eq!(checked.copy().project, old_packet.project);
    let report_claims = fs::read(report_profile.index_root.join("claims.json")).unwrap();
    let original_claims = fs::read(old_profile.admission.index_root.join("claims.json")).unwrap();
    let mut old_inputs = NativeInputs::open(&old_packet, &old_profile.source).unwrap();
    reel_workflow::publication::bind_comic(&old_packet, &mut old_inputs).unwrap();
    let mut bytes = BTreeMap::new();
    for pin in &old_packet.task.inputs {
        bytes.insert(
            pin.logical_id.clone(),
            old_inputs.read(pin, 16 * 1024 * 1024).unwrap(),
        );
    }
    drop(old_inputs);
    let old_clean = native_bytes(
        &old_profile.source,
        &old_result.artifacts["page01-clean.png"],
    );
    let old_reading = native_bytes(
        &old_profile.source,
        &old_result.artifacts["page01-reading.png"],
    );
    let mut page: PageSource = decode(&bytes["comic/page"]).unwrap();
    assert_eq!(page.utterances[0].lines, ["La luz sigue."]);
    page.utterances[0].lines = vec!["Mira la luz.".into()];
    let page_raw = serde_json::to_vec_pretty(&page).unwrap();
    let pin = |before: &Artifact, raw: &[u8]| Artifact {
        logical_id: before.logical_id.clone(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: before.media_type.clone(),
    };
    let old_page = old_packet
        .task
        .inputs
        .iter()
        .find(|a| a.logical_id == "comic/page")
        .unwrap();
    let page_pin = pin(old_page, &page_raw);
    let mut request: Request = decode(&bytes["comic/request"]).unwrap();
    request.pages[0].sha256 = page_pin.sha256.clone();
    request.pages[0].bytes = page_pin.bytes;
    let request_raw = serde_json::to_vec_pretty(&request).unwrap();
    let mut publication: Publication = decode(&bytes["comic/publication"]).unwrap();
    let Content::Comic { pages, .. } = &mut publication.editions[0].content else {
        panic!("comic edition required")
    };
    pages[0].source = page_pin;
    let publication_raw = serde_json::to_vec_pretty(&publication).unwrap();
    let changed = BTreeMap::from([
        ("comic/page".to_owned(), page_raw),
        ("comic/request".to_owned(), request_raw),
        ("comic/publication".to_owned(), publication_raw),
    ]);
    let edits: Vec<_> = old_packet
        .task
        .inputs
        .iter()
        .filter_map(|before| {
            changed.get(&before.logical_id).map(|raw| review::Edit {
                before: before.clone(),
                after: pin(before, raw),
            })
        })
        .collect();
    assert_eq!(edits.len(), 3);
    for (id, raw) in &changed {
        bytes.insert(id.clone(), raw.clone());
    }
    let mut fixture = Fixture::new();
    fixture.temp.disable_cleanup(true);
    let template: serde_json::Value = load(fixture.temp.path().join("records/input.tig"));
    fixture.profile.records.clear();
    fixture.profile.bindings.clear();
    fixture.profile.input_streams.clear();
    for (i, before) in old_packet.task.inputs.iter().enumerate() {
        let a = pin(before, &bytes[&before.logical_id]);
        let object = Object::from_bytes(&bytes[&before.logical_id]);
        let record = format!("records/repair-{i}.tig");
        let payload = format!("payloads/repair-{i}.bin");
        let mut value = template.clone();
        value["id"] = json!(a.logical_id);
        value["selected"] = serde_json::to_value(&object).unwrap();
        value["streams"] = json!([]);
        write(fixture.temp.path(), &record, value);
        fs::write(
            fixture.temp.path().join(&payload),
            Key::from_object(&object).unlocked_pointer(),
        )
        .unwrap();
        fixture.profile.records.push(record);
        fixture.profile.bindings.insert(
            a.logical_id.clone(),
            Binding {
                selected: LogicalPath::parse(&payload).unwrap(),
                streams: BTreeMap::new(),
            },
        );
        fixture.profile.input_streams.insert(a.logical_id, None);
    }
    git(fixture.temp.path(), &["add", "."]);
    git(
        fixture.temp.path(),
        &[
            "commit",
            "-m",
            "synthetic reviewed Spanish lettering correction",
        ],
    );
    fixture.profile.commit = git(fixture.temp.path(), &["rev-parse", "HEAD"]);
    fixture.profile.git = old_profile.source.snapshot.git.clone();
    let baseline = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: old_packet.project.clone(),
        source_commit: old_packet.source_commit.clone(),
        tasks: vec![old_packet.task.clone()],
    };
    let repair = review::Repair {
        roots: None,
        schema: "reel.workflow-repair.v1".into(),
        report_sha256: checked.report_sha256().into(),
        finding_keys: checked.findings().keys().cloned().collect(),
        edits,
    };
    let revision = repair::Revision {
        task_snapshots: BTreeMap::new(),
        schema: "reel.workflow-repair-revision.v1".into(),
        source_commit: fixture.profile.commit.clone(),
        native_inputs: BTreeMap::from([(
            old_packet.task.id.clone(),
            review::Edit {
                before: old_packet.task.native_inputs.clone().unwrap(),
                after: Fixture::declaration(&fixture.profile),
            },
        )]),
    };
    let candidate = repair::materialize(&baseline, &checked, &repair, &revision).unwrap();
    fixture.packet = make_packet(&candidate.plan, &old_packet.task.id, BTreeMap::new()).unwrap();
    assert_ne!(fixture.packet.task_key, old_packet.task_key);
    let mut case = AdmissionCase::with_store(&fixture, old_profile.source.source.store.clone());
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        snapshot: fixture.profile.clone(),
        annex: old_profile.source.annex.clone(),
        source: serde_json::from_slice(&serde_json::to_vec(&old_profile.source.source).unwrap())
            .unwrap(),
        movie_verifier: None,
    };
    case.source.source.pool.control = control;
    let root = case.temp.path();
    fs::write(root.join("original-comic-claims.bin"), &original_claims).unwrap();
    fs::write(root.join("original-report-claims.bin"), &report_claims).unwrap();
    let source_root = root.join("corrected-inputs");
    fs::create_dir(&source_root).unwrap();
    let marker = json!({"root":root,"fixture":fixture.temp.path(),"original":original,"report":report_root,"task_key":fixture.packet.task_key,"attempt":"attempt-1","runtime":old_profile.runtime,"controller":controller});
    save(root.join("original-operation.json"), &marker);
    save(evidence.join("original-operation-roots.json"), &marker);
    for (name, value) in [
        ("baseline.json", serde_json::to_value(&baseline).unwrap()),
        ("repair.json", serde_json::to_value(&repair).unwrap()),
        ("revision.json", serde_json::to_value(&revision).unwrap()),
        ("candidate.json", serde_json::to_value(&candidate).unwrap()),
        (
            "packet.json",
            serde_json::to_value(&fixture.packet).unwrap(),
        ),
        ("source.json", serde_json::to_value(&case.source).unwrap()),
        (
            "admission.json",
            serde_json::to_value(&case.profile).unwrap(),
        ),
    ] {
        save(root.join(name), &value);
    }
    save(
        root.join("phase.json"),
        &json!({"phase":"accept-original-corrected-render"}),
    );
    let admission = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    save(root.join("receipt.json"), &admission);
    for (i, (id, raw)) in changed.iter().enumerate() {
        let name = format!("changed-{i}.json");
        fs::write(source_root.join(&name), raw).unwrap();
        save(
            root.join("phase.json"),
            &json!({"phase":"publish-corrected-input","logical_id":id,"path":name}),
        );
        tig_git::annex::workflow::publication::publish(
            &tig_git::annex::workflow::publication::Profile {
                schema: "tig.annex-file-publication.experimental-1".into(),
                pool: case.source.source.pool.clone(),
                store: case.profile.scope.store.clone(),
                source_root: source_root.clone(),
                source: LogicalPath::parse(&name).unwrap(),
                source_owned: true,
                expected: Some(Object::from_bytes(raw)),
                include_private_location: false,
                maximum_bytes: 16 * 1024 * 1024,
                minimum_free_bytes: 100 * 1024 * 1024,
            },
        )
        .unwrap();
    }
    let profile = handoff_profile(&case, admission, old_profile.runtime.path.clone());
    let profile_path = root.join("handoff-profile.json");
    let packet_path = root.join("packet.json");
    let result_path = root.join("result.json");
    let journal = root.join("journal");
    save(&profile_path, &profile);
    save(
        root.join("phase.json"),
        &json!({"phase":"create-and-dispatch-corrected-render"}),
    );
    let created = Command::new(&old_profile.runtime.path)
        .arg("create-handoff")
        .arg(&packet_path)
        .arg(&profile_path)
        .arg(&journal)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{} {}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
    let mut controls = Vec::new();
    automatic_dispatch::run_comic(
        &case,
        &fixture.packet,
        &profile,
        &journal,
        &profile_path,
        &old_profile.runtime.path,
        &result_path,
        &mut controls,
        &BTreeMap::new(),
    );
    for (name, raw) in controls {
        fs::write(root.join(name), raw).unwrap();
    }
    let result: ReturnedResult = load(&result_path);
    let ack = load(ack_path(&profile, &fixture.packet));
    save(root.join("ack.json"), &ack);
    save(
        root.join("phase.json"),
        &json!({"phase":"verify-retained-corrected-comic-and-review-copy"}),
    );
    drop(
        retention::verify(
            &fixture.packet,
            &case.source,
            &case.profile,
            &profile.binding.admission,
            &result,
            &ack,
        )
        .unwrap(),
    );
    assert_eq!(
        native_bytes(&case.source, &result.artifacts["page01-clean.png"]),
        old_clean
    );
    assert_ne!(
        native_bytes(&case.source, &result.artifacts["page01-reading.png"]),
        old_reading
    );
    let transcript = native_bytes(&case.source, &result.artifacts["transcript.json"]);
    assert!(
        String::from_utf8(transcript)
            .unwrap()
            .contains("Mira la luz.")
    );
    let reviewed = retention::review_copy(
        &fixture.packet,
        &case.source,
        &case.profile,
        &profile.binding.admission,
        &result,
        &ack,
    )
    .unwrap();
    assert_eq!(reviewed.copy().units, checked.copy().units);
    assert_ne!(reviewed.copy().review, checked.copy().review);
    assert_eq!(reviewed.copy().source_commit, fixture.packet.source_commit);
    let old_report = review::Report {
        schema: "reel.workflow-review-report.v1".into(),
        copy_sha256: canonical_digest(checked.copy()).unwrap(),
        findings: checked
            .findings()
            .values()
            .map(|f| f.finding().clone())
            .collect(),
    };
    assert_eq!(
        review::check_report(reviewed.copy(), &old_report)
            .unwrap_err()
            .kind,
        ErrorKind::Stale
    );
    save(root.join("corrected-review-copy.json"), reviewed.copy());
    fs::write(
        root.join("corrected-review-manifest.json"),
        reviewed.manifest().unwrap(),
    )
    .unwrap();
    drop(reviewed);
    assert_eq!(
        report_claims,
        fs::read(report_profile.index_root.join("claims.json")).unwrap()
    );
    assert_eq!(
        original_claims,
        fs::read(old_profile.admission.index_root.join("claims.json")).unwrap()
    );
    let outcome = json!({"root":root,"fixture":fixture.temp.path(),"finding_bound":true,"new_source_commit":fixture.packet.source_commit,"retained":true,"replay_without_render_or_claim_changes":true,"clean_art_unchanged":true,"reading_pixels_changed":true,"corrected_transcript_verified":true,"review_unit_ids_preserved":true,"stale_report_refused":true,"old_claims_unchanged":true,"human_reviewed":false,"selected":false,"issue_closed":false});
    save(root.join("qualification-result.json"), &outcome);
    save(evidence.join("qualification-result.json"), &outcome);
    save(
        root.join("phase.json"),
        &json!({"phase":"qualified-corrected-render-awaiting-contextual-review"}),
    );
}
