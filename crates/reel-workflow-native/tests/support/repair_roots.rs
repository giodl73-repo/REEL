//! Native retained whole-issue findings -> rooted candidate and local replay.
//! No new native admission, selection, tracker send or finding closure.
use super::*;
use reel_publication::{
    Content,
    comic_issue::{self, Assembly, Plan as IssuePlan},
    comic_source::{PageSource, Pin, Request},
};
use reel_workflow_native::{admission, handoff, retention, upstream};
use std::io::{Read, Write};

fn load<T: serde::de::DeserializeOwned>(root: &Path, name: &str) -> T {
    decode(&fs::read(root.join(name)).unwrap()).unwrap()
}
fn save(root: &Path, name: &str, raw: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(raw).unwrap();
    file.sync_all().unwrap();
}
fn json_save(root: &Path, name: &str, value: &impl serde::Serialize) {
    save(root, name, &serde_json::to_vec_pretty(value).unwrap());
}
fn repin(before: &Artifact, raw: &[u8]) -> Artifact {
    Artifact {
        sha256: digest(raw),
        bytes: raw.len() as u64,
        ..before.clone()
    }
}
fn path_pin(path: String, raw: &[u8]) -> Pin {
    Pin {
        path,
        sha256: digest(raw),
        bytes: raw.len() as u64,
    }
}
fn native_control(source: &reel_workflow_native::NativeProfile, pin: &Artifact) -> Vec<u8> {
    assert!(pin.bytes <= MAX_CONTROL_BYTES as u64);
    let mut stream = tig_git::annex::workflow::reader::open(
        &source.source,
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
    raw
}
fn invoke(exe: &Path, report: &Path, command: &str, args: &[&Path]) -> std::process::Output {
    let mut child = Command::new(exe);
    child.arg(command);
    for name in [
        "packet.json",
        "source.json",
        "admission.json",
        "receipt.json",
        "result.json",
        "retained.json",
    ] {
        child.arg(report.join(name));
    }
    child.args(args).arg("--apply").output().unwrap()
}
fn record_output(root: &Path, name: &str, out: &std::process::Output, success: bool) {
    json_save(
        root,
        name,
        &json!({"exit_code":out.status.code(),"stdout":String::from_utf8_lossy(&out.stdout),"stderr":String::from_utf8_lossy(&out.stderr)}),
    );
    assert_eq!(
        out.status.success(),
        success,
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "Existing retained synthetic whole-issue report only; exact proof reopen, isolated local lettering replay and private rooted plan/draft exports, no admission or tracker effects"]
fn retained_whole_issue_report_scopes_a_part_repair_and_preserves_the_other_part() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let report_root = env("REEL_ROOTED_REPAIR_REPORT");
    let controls = env("REEL_ROOTED_REPAIR_CONTROLS");
    let original = env("REEL_ROOTED_REPAIR_CAPTURE");
    let waiting = env("REEL_ROOTED_REPAIR_WAITING");
    let evidence = env("REEL_ROOTED_REPAIR_EVIDENCE");
    let executable = env("REEL_NATIVE_HANDOFF_TOOL");
    let root = evidence.join("candidate");
    fs::create_dir(&root).unwrap();
    let packet: Packet = load(&report_root, "packet.json");
    let profile: handoff::Profile = load(&report_root, "profile.json");
    let baseline: Plan = load(&report_root, "plan.json");
    let original_parents: upstream::Parents = load(&controls, "parents.json");
    let issue = &original_parents.parents["issue-assemble"];
    let opening = &original_parents.parents["opening"];
    let closing = &original_parents.parents["closing"];
    let checkpoint = || {
        let index = tig_store::source_claims::protection::Index::open(
            &profile.admission.index_root,
            &profile.binding.admission.index_checkpoint,
        )
        .unwrap();
        canonical_digest(&index.checkpoint()).unwrap()
    };
    let check_waiting = || {
        let packet: Packet = load(&waiting, "next-packet.json");
        let p: handoff::Profile = load(&waiting, "next-handoff-profile.json");
        let journal = reel_workflow::handoff::Journal::open(&waiting.join("next-journal")).unwrap();
        assert_eq!(journal.state(), reel_workflow::handoff::State::Waiting);
        assert_eq!(&journal.ticket().packet, &packet);
        drop(
            admission::Prepared::open(
                &packet,
                &p.source,
                &p.admission,
                &p.binding.admission.attempt_id,
            )
            .unwrap()
            .reopen(&p.binding.admission)
            .unwrap(),
        );
    };
    check_waiting();
    let before_checkpoint = checkpoint();
    json_save(
        &root,
        "original-checkpoint.json",
        &json!({"sha256":before_checkpoint}),
    );
    let originals: BTreeMap<_, _> = [
        report_root.join("packet.json"),
        report_root.join("profile.json"),
        report_root.join("result.json"),
        report_root.join("retained.json"),
        controls.join("parents.json"),
        waiting.join("next-handoff-profile.json"),
        original.join("receipt.json"),
        original.join("issue-plan.json"),
        original.join("parts/opening/editable.packet"),
        original.join("parts/closing/editable.packet"),
    ]
    .into_iter()
    .map(|p| {
        let hash = digest(&fs::read(&p).unwrap());
        (p, hash)
    })
    .collect();
    json_save(&root, "original-pins.json", &originals);
    let guard = retention::verify(
        &packet,
        &profile.source,
        &profile.admission,
        &profile.binding.admission,
        &load(&report_root, "result.json"),
        &load(&report_root, "retained.json"),
    )
    .unwrap();
    let checked = guard.review_report().unwrap().clone();
    drop(guard);
    assert_eq!(checked.findings().len(), 2);
    assert_eq!(
        checked.copy().publication,
        issue.packet.task.tool.as_ref().unwrap().publication
    );
    assert_eq!(checked.copy().source_commit, issue.packet.source_commit);
    // Reopen full native issue proof before using a restored local packet. Local
    // replay must reproduce that exact original receipt and both declared outputs.
    drop(
        retention::verify(
            &issue.packet,
            &issue.profile.source,
            &issue.profile.admission,
            &issue.profile.binding.admission,
            &issue.result,
            &issue.acknowledgement,
        )
        .unwrap(),
    );
    let original_request = native_control(
        &issue.profile.source,
        &issue.packet.task.tool.as_ref().unwrap().request,
    );
    let original_receipt = native_control(
        &issue.profile.source,
        issue.result.worker_receipt.as_ref().unwrap(),
    );
    save(&root, "original-assembly.json", &original_request);
    save(&root, "original-assembly-receipt.json", &original_receipt);
    assert_eq!(
        repin(
            &issue.packet.task.tool.as_ref().unwrap().request,
            &original_request
        ),
        issue.packet.task.tool.as_ref().unwrap().request
    );
    let (old_receipt, old_files) = comic_issue::capture_with(&original_request, |path, max| {
        let local = if path == "intent.json" {
            "composition-intent.json"
        } else {
            path
        };
        let raw = fs::read(original.join(local))?;
        anyhow::ensure!(raw.len() as u64 <= max, "bounded local replay");
        Ok(raw)
    })
    .unwrap();
    assert_eq!(
        serde_json::to_value(&old_receipt).unwrap(),
        serde_json::to_value(
            decode::<reel_publication::comic_issue::AssemblyReceipt>(&original_receipt).unwrap()
        )
        .unwrap()
    );
    for (name, pin) in &issue.result.artifacts {
        assert_eq!(repin(pin, &old_files[name]), *pin);
    }
    let mut page: PageSource =
        decode(&old_files["parts/opening/sources/pages/page01.json"]).unwrap();
    let original_page = serde_json::to_value(&page).unwrap();
    page.utterances[0].font_size += 1;
    let page_raw = serde_json::to_vec_pretty(&page).unwrap();
    let mut request: Request = decode(&old_files["parts/opening/sources/request.json"]).unwrap();
    request.pages[0] = path_pin(request.pages[0].path.clone(), &page_raw);
    let request_raw = serde_json::to_vec_pretty(&request).unwrap();
    let (new_part_receipt, new_part_files) =
        reel_publication::comic::capture_with(&request_raw, |pin| {
            let raw = if pin.path == "pages/page01.json" {
                page_raw.clone()
            } else {
                old_files[&format!("parts/opening/sources/{}", pin.path)].clone()
            };
            Ok(raw)
        })
        .unwrap();
    assert_eq!(new_part_receipt.page_order, ["page01"]);
    let mut plan: IssuePlan = decode(&old_files["issue-plan.json"]).unwrap();
    let Content::Comic { pages, .. } = &mut plan.publication.editions[0].content else {
        panic!("comic edition")
    };
    let page_pin = repin(&pages[0].source, &page_raw);
    pages[0].source = page_pin.clone();
    let master_raw = serde_json::to_vec_pretty(&plan.publication).unwrap();
    let plan_raw = serde_json::to_vec_pretty(&plan).unwrap();
    let mut assembly: Assembly = decode(&original_request).unwrap();
    assembly.plan = path_pin(assembly.plan.path, &plan_raw);
    assembly.parts.insert(
        "opening".into(),
        path_pin(
            assembly.parts["opening"].path.clone(),
            &new_part_files["receipt.json"],
        ),
    );
    assembly.bundles.insert(
        "opening".into(),
        path_pin(
            assembly.bundles["opening"].path.clone(),
            &new_part_files["editable.packet"],
        ),
    );
    let new_assembly_raw = serde_json::to_vec_pretty(&assembly).unwrap();
    let mut available = old_files.clone();
    available.insert(assembly.plan.path.clone(), plan_raw.clone());
    if let Some(composition) = &assembly.composition {
        available.insert(
            composition.intent.path.clone(),
            old_files["composition-intent.json"].clone(),
        );
    }
    for (name, raw) in &new_part_files {
        available.insert(format!("parts/opening/{name}"), raw.clone());
    }
    let (new_receipt, new_files) = comic_issue::capture_with(&new_assembly_raw, |path, max| {
        let raw = available
            .get(path)
            .ok_or_else(|| anyhow::anyhow!("missing declared local path"))?;
        anyhow::ensure!(raw.len() as u64 <= max, "bounded corrected replay");
        Ok(raw.clone())
    })
    .unwrap();
    assert_eq!(new_receipt.page_order, old_receipt.page_order);
    assert!(
        !new_receipt.human_approval
            && !new_receipt.retained_owner_acknowledged
            && !new_receipt.print_ready
    );
    for (name, raw) in &old_files {
        if name.starts_with("parts/closing/") || name.ends_with("-clean.png") {
            assert_eq!(
                &new_files[name], raw,
                "clean art/untouched part changed: {name}"
            );
        }
    }
    assert_ne!(
        old_files["parts/opening/page01-reading.png"],
        new_files["parts/opening/page01-reading.png"]
    );
    assert_ne!(old_files["reading.pdf"], new_files["reading.pdf"]);
    let old_transcript: serde_json::Value = decode(&old_files["transcript.json"]).unwrap();
    let mut new_transcript: serde_json::Value = decode(&new_files["transcript.json"]).unwrap();
    new_transcript["pages"][0]["utterances"][0]["font_size"] =
        old_transcript["pages"][0]["utterances"][0]["font_size"].clone();
    assert_eq!(new_transcript, old_transcript);
    let mut changed_page = serde_json::to_value(&page).unwrap();
    changed_page["utterances"][0]["font_size"] =
        original_page["utterances"][0]["font_size"].clone();
    assert_eq!(
        changed_page, original_page,
        "stable IDs, source wording and art must survive typography edits"
    );
    for (name, raw) in &new_files {
        save(&root.join("local-reading"), name, raw);
    }
    let changed = BTreeMap::from([
        ("comic/page01".to_owned(), page_raw),
        ("comic/opening/request".to_owned(), request_raw),
        ("comic/master".to_owned(), master_raw),
        ("comic/plan".to_owned(), plan_raw),
    ]);
    let keys: Vec<_> = checked
        .findings()
        .iter()
        .filter_map(|(key, finding)| {
            let unit = &checked.copy().units[&finding.finding().unit];
            matches!(unit, review::Unit::Comic { page_id, .. } if page_id == "page01")
                .then(|| key.clone())
        })
        .collect();
    assert_eq!(keys.len(), 1);
    let repair = review::Repair {
        schema: "reel.workflow-repair.v2".into(),
        report_sha256: checked.report_sha256().into(),
        finding_keys: keys,
        roots: Some(vec!["opening".into()]),
        edits: opening
            .packet
            .task
            .inputs
            .iter()
            .filter_map(|before| {
                changed.get(&before.logical_id).map(|raw| review::Edit {
                    before: before.clone(),
                    after: repin(before, raw),
                })
            })
            .collect(),
    };
    assert_eq!(repair.edits.len(), 4);
    let impact = review::repair_impact(&baseline, &checked, &repair).unwrap();
    assert_eq!(impact.unaffected, ["closing"]);
    assert_eq!(impact.rebuild.len(), 5);
    assert_eq!(impact.contextual_languages, ["es"]);
    assert_eq!(impact.contextual_review, *checked.copy());
    // Candidate source identity is a real isolated authored Git commit. It is
    // deliberately not represented as an admitted or verified native snapshot.
    let mut source = Fixture::new();
    source.temp.disable_cleanup(true);
    for (id, raw) in &changed {
        save(source.temp.path(), &format!("candidate/{id}.json"), raw);
    }
    git(source.temp.path(), &["add", "candidate"]);
    git(
        source.temp.path(),
        &[
            "commit",
            "-m",
            "isolated synthetic rooted lettering candidate",
        ],
    );
    let source_commit = git(source.temp.path(), &["rev-parse", "HEAD"]);
    let mut revision = repair::Revision {
        schema: "reel.workflow-repair-revision.v2".into(),
        source_commit,
        native_inputs: BTreeMap::new(),
    };
    for task in baseline
        .tasks
        .iter()
        .filter(|t| impact.rebuild.contains(&t.id))
    {
        if let Some(before) = &task.native_inputs {
            let raw = serde_json::to_vec(&json!({"schema":"reel.synthetic-unqualified-repair-declaration.v1","task":task.id,"source_commit":revision.source_commit,"admitted":false})).unwrap();
            save(&root, &format!("pending-native/{}.json", task.id), &raw);
            revision.native_inputs.insert(
                task.id.clone(),
                review::Edit {
                    before: before.clone(),
                    after: repin(before, &raw),
                },
            );
        }
    }
    let expected = repair::materialize(&baseline, &checked, &repair, &revision).unwrap();
    for (name, value) in [
        ("baseline.json", serde_json::to_vec(&baseline).unwrap()),
        ("repair.json", serde_json::to_vec(&repair).unwrap()),
        ("revision.json", serde_json::to_vec(&revision).unwrap()),
    ] {
        save(&root, name, &value);
    }
    json_save(&root, "impact.json", &expected.impact);
    let candidate_path = root.join("plan.json");
    let out = invoke(
        &executable,
        &report_root,
        "export-repair-plan",
        &[
            &root.join("baseline.json"),
            &root.join("repair.json"),
            &root.join("revision.json"),
            &candidate_path,
        ],
    );
    record_output(&root, "plan-export.json", &out, true);
    let candidate: Plan = load(&root, "plan.json");
    assert_eq!(candidate, expected.plan);
    assert_eq!(
        candidate.source_commit_for("closing").unwrap(),
        baseline.source_commit_for("closing").unwrap()
    );
    assert_eq!(
        make_packet(&candidate, "closing", BTreeMap::new()).unwrap(),
        closing.packet
    );
    let mut stale = repair.clone();
    stale.report_sha256 = "f".repeat(64);
    json_save(&root, "stale-repair.json", &stale);
    let refused = root.join("refused-plan.json");
    let out = invoke(
        &executable,
        &report_root,
        "export-repair-plan",
        &[
            &root.join("baseline.json"),
            &root.join("stale-repair.json"),
            &root.join("revision.json"),
            &refused,
        ],
    );
    record_output(&root, "stale-export.json", &out, false);
    assert!(!refused.exists());
    let routes = review::Routes {
        project: "example/project".into(),
        reel: "example/reel".into(),
        tig: "example/tig".into(),
    };
    json_save(&root, "routes.json", &routes);
    for (i, key) in checked.findings().keys().enumerate() {
        let text = tracker::Text { schema:"reel.workflow-issue-text.v1".into(), finding_key:key.clone(), repository:routes.project.clone(), title:format!("Synthetic whole-issue protocol finding {}", i + 1), body:"Scripted technical fixture only. No actual creative defect, viewing or approval is asserted. Keep the other page finding open while evaluating this isolated lettering candidate.".into(), synthetic_reproduction:None };
        let text_path = root.join(format!("issue-text-{i}.json"));
        json_save(
            &root,
            text_path.file_name().unwrap().to_str().unwrap(),
            &text,
        );
        let draft_path = root.join(format!("issue-draft-{i}.json"));
        let out = invoke(
            &executable,
            &report_root,
            "export-issue-draft",
            &[&root.join("routes.json"), &text_path, &draft_path],
        );
        record_output(&root, &format!("issue-export-{i}.json"), &out, true);
        let draft: tracker::Draft = decode(&fs::read(draft_path).unwrap()).unwrap();
        draft.check().unwrap();
        assert_eq!(draft.finding_key, *key);
    }
    for (path, pin) in &originals {
        assert_eq!(&digest(&fs::read(path).unwrap()), pin);
    }
    assert_eq!(before_checkpoint, checkpoint());
    check_waiting();
    let outcome = json!({"schema":"reel.rooted-whole-issue-repair-qualification.v1","report_task_key":packet.task_key,"root":root,"source_candidate_repository":source.temp.path(),"actual_checkpoint_sha256":before_checkpoint,"retained_report_reopened":true,"original_native_issue_reopened":true,"original_local_replay_matches_retained_receipt_and_outputs":true,"clean_art_unchanged":true,"closing_packet_byte_identical":true,"closing_task_packet_and_snapshot_unchanged":true,"reading_pixels_changed":true,"spanish_text_and_ids_unchanged":true,"full_two_page_local_issue_replayed":true,"rooted_rebuild_tasks":impact.rebuild,"full_context_preserved":true,"private_issue_drafts":2,"findings_targeted_for_candidate":1,"stale_report_refused_before_output":true,"actual_index_unchanged":true,"other_waiting_generation_reopened":true,"new_native_snapshot_verified":false,"new_native_admission":false,"corrected_native_issue_retained":false,"contextual_viewing":false,"provider_called":false,"tracker_called":false,"finding_closed":false,"selected":false,"approved":false,"scratch_retired":false});
    json_save(&root, "qualification-result.json", &outcome);
    json_save(&evidence, "qualification-result.json", &outcome);
}
