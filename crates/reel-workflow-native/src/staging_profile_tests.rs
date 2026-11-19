//! Versioned owned capacity, not native input/output custody or film approval.
use super::*;

fn profile(parent: &Path) -> Profile {
    Profile {
        schema: "reel.workflow-staging.v2".into(),
        parent: parent.into(),
        maximum_bytes: 32 * 1024 * 1024 * 1024,
        capacity_policy: capacity::Policy {
            max_object_bytes: 32 * 1024 * 1024 * 1024,
            ..Default::default()
        },
    }
}
fn plan() -> Plan {
    Plan {
        manifest: Manifest {
            schema: "reel.workflow-staging-manifest.v1".into(),
            operation: reel_workflow::digest(b"streamed owned profile fixture"),
            files: BTreeMap::from([(
                "metadata.json".into(),
                Entry {
                    object: Object::from_bytes(b"{}"),
                    input: None,
                },
            )]),
        },
        controls: BTreeMap::from([("metadata.json".into(), b"{}".to_vec())]),
    }
}
#[test]
fn explicit_streaming_profile_keeps_legacy_ceiling_and_requires_its_capacity_policy() {
    let root = tempfile::tempdir().unwrap();
    let mut p = profile(root.path());
    configuration(&p, &root.path().join("coordination")).unwrap();
    p.schema = "reel.workflow-staging.v1".into();
    assert!(configuration(&p, root.path()).is_err());
    p.schema = "reel.workflow-staging.v2".into();
    p.capacity_policy.max_object_bytes = 8 * 1024 * 1024 * 1024;
    assert!(configuration(&p, root.path()).is_err());
    p.capacity_policy.max_object_bytes = MAX_STREAMED_BYTES + 1;
    p.maximum_bytes = MAX_STREAMED_BYTES + 1;
    assert!(configuration(&p, root.path()).is_err());
}
#[test]
fn owned_v2_intent_refuses_changed_profile_while_legacy_intent_bytes_stay_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let p = profile(root.path());
    let plan = plan();
    let parent = open_workspace_directory(root.path()).unwrap();
    let owner = create_owner(&plan, &parent, &p).unwrap();
    drop(open_owner(&p, &owner, &plan).unwrap());
    for change in ["budget", "policy", "version"] {
        let mut changed = p.clone();
        match change {
            "budget" => changed.maximum_bytes -= 1,
            "policy" => changed.capacity_policy.minimum_free_basis_points += 1,
            _ => changed.schema = "reel.workflow-staging.v1".into(),
        }
        assert_eq!(
            open_owner(&changed, &owner, &plan).err().unwrap().code,
            "staging-intent-binding"
        );
    }
    let mut legacy = p.clone();
    legacy.schema = "reel.workflow-staging.v1".into();
    assert_eq!(
        owned_intent(&legacy, &plan).unwrap(),
        json(&plan.manifest).unwrap()
    );
    let wrapped: serde_json::Value =
        serde_json::from_slice(&owned_intent(&p, &plan).unwrap()).unwrap();
    assert_eq!(
        wrapped["profile"],
        serde_json::to_value(Object::from_bytes(&json(&p).unwrap())).unwrap()
    );
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recovery {
    schema: String,
    profile: Profile,
    reservation: Reservation,
    owner: Owner,
    manifest: Manifest,
    controls: BTreeMap<String, Vec<u8>>,
    coordination: PathBuf,
}
fn save(path: &Path, value: &impl Serialize) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&json(value).unwrap()).unwrap();
    file.sync_all().unwrap();
}

#[test]
#[ignore = "Existing synthetic 3.6 GB input and capacity coordinator; reserves 32 GiB, preserves scratch, and spawns only the exact recovery fixture"]
fn large_owned_workspace_recovers_in_a_new_process_under_the_original_capacity_and_profile() {
    let source_result = PathBuf::from(
        std::env::var_os("REEL_STREAMED_WORKSPACE_SOURCE_RESULT")
            .expect("Set qualified large-hash result"),
    );
    let admission_path = PathBuf::from(
        std::env::var_os("REEL_STREAMED_WORKSPACE_ADMISSION")
            .expect("Set existing capacity coordinator profile"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_STREAMED_WORKSPACE_EVIDENCE").expect("Set evidence directory"),
    );
    let source: serde_json::Value = crate::read_control(&source_result).unwrap();
    let source_path = PathBuf::from(source["root"].as_str().unwrap()).join("synthetic-master.bin");
    let object = Object {
        sha256: tig_core::Digest::parse(source["sha256"].as_str().unwrap()).unwrap(),
        bytes: source["bytes"].as_u64().unwrap(),
    };
    assert_eq!(object.bytes, 3_600_000_001);
    let (observed, _source_guard) = observe_held(&source_path, object.bytes).unwrap();
    assert_eq!(observed, object);
    let admission: admission::Profile = crate::read_control(&admission_path).unwrap();
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    let marker = serde_json::json!({"root":root,"source_result":source_result,"bytes":object.bytes,"workspace_reservation_bytes":32u64*1024*1024*1024,"new_native_admission":false});
    save(&root.join("original-operation.json"), &marker);
    save(&evidence.join("original-operation-roots.json"), &marker);
    let scratch = root.join("scratch");
    fs::create_dir(&scratch).unwrap();
    let p = profile(&scratch);
    let mut plan = plan();
    plan.manifest.files.insert(
        "data/large.bin".into(),
        Entry {
            object: object.clone(),
            input: None,
        },
    );
    let (p, reservation, owner) =
        prepare_owned(&p, &admission.scope.coordination_root, &plan).unwrap();
    let ticket = Recovery {
        schema: "reel.workspace-qualification-ticket.v1".into(),
        profile: p,
        reservation,
        owner,
        manifest: plan.manifest,
        controls: plan.controls,
        coordination: admission.scope.coordination_root,
    };
    let raw = json(&ticket).unwrap();
    let pin = reel_workflow::digest(&raw);
    save(&root.join("recovery-ticket.json"), &ticket);
    save(&root.join("recovery-ticket-pin.json"), &pin);
    let workspace = ticket.profile.parent.join(&ticket.owner.name);
    fs::create_dir(workspace.join("data")).unwrap();
    assert_eq!(
        fs::copy(&source_path, workspace.join("data/large.bin")).unwrap(),
        object.bytes
    );
    let plan = Plan {
        manifest: ticket.manifest,
        controls: ticket.controls,
    };
    {
        let config = configuration(&ticket.profile, &ticket.coordination).unwrap();
        let _lease = capacity::resume_preserved_on(
            &config,
            &ticket.profile.parent,
            &ticket.reservation.id,
            &ticket.reservation.volume,
            ticket.profile.maximum_bytes,
        )
        .unwrap();
        let mut view = open_owner(&ticket.profile, &ticket.owner, &plan).unwrap();
        let error = materialize(
            &mut view,
            &ticket.profile,
            &plan,
            &Buffers(BTreeMap::new()),
            &pin,
            &mut |_| {
                Err(error(
                    ErrorKind::NeedsRecovery,
                    "injected-controller-boundary",
                ))
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "injected-controller-boundary");
        assert!(!workspace.join("complete.json").exists());
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "staging::profile_tests::resume_only_original_streamed_workspace",
            "--nocapture",
        ])
        .env("REEL_STREAMED_WORKSPACE_RECOVERY", root)
        .env("REEL_STREAMED_WORKSPACE_PIN", &pin)
        .env("REEL_STREAMED_WORKSPACE_EVIDENCE", &evidence)
        .output()
        .unwrap();
    fs::write(evidence.join("recovery-process.log"), &output.stdout).unwrap();
    fs::write(evidence.join("recovery-process.stderr"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(root.join("recovery-ticket.json")).unwrap(), raw);
}

#[test]
#[ignore = "Resume only the original streamed-workspace ticket; no new reservation, workspace, render or native admission"]
fn resume_only_original_streamed_workspace() {
    let root = PathBuf::from(
        std::env::var_os("REEL_STREAMED_WORKSPACE_RECOVERY").expect("Set original workspace root"),
    );
    let pin = std::env::var("REEL_STREAMED_WORKSPACE_PIN").expect("Set original ticket pin");
    let evidence = PathBuf::from(
        std::env::var_os("REEL_STREAMED_WORKSPACE_EVIDENCE").expect("Set evidence directory"),
    );
    let (raw, _ticket) = read_held(
        &root.join("recovery-ticket.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )
    .unwrap();
    assert_eq!(reel_workflow::digest(&raw), pin);
    let r: Recovery = reel_workflow::decode(&raw).unwrap();
    assert_eq!(r.schema, "reel.workspace-qualification-ticket.v1");
    let plan = Plan {
        manifest: r.manifest,
        controls: r.controls,
    };
    let config = configuration(&r.profile, &r.coordination).unwrap();
    assert!(
        capacity::resume_preserved_on(
            &config,
            &r.profile.parent,
            &r.reservation.id,
            &r.reservation.volume,
            r.profile.maximum_bytes - 1
        )
        .is_err()
    );
    let _lease = capacity::resume_preserved_on(
        &config,
        &r.profile.parent,
        &r.reservation.id,
        &r.reservation.volume,
        r.profile.maximum_bytes,
    )
    .unwrap();
    let mut changed = r.profile.clone();
    changed.capacity_policy.minimum_free_basis_points += 1;
    assert!(open_owner(&changed, &r.owner, &plan).is_err());
    let workspace = r.profile.parent.join(&r.owner.name);
    let mut view = open_owner(&r.profile, &r.owner, &plan).unwrap();
    let summary = materialize(
        &mut view,
        &r.profile,
        &plan,
        &Buffers(BTreeMap::new()),
        &pin,
        &mut |index| {
            assert_ne!(index, 0, "large in-place bytes must not be copied");
            Ok(())
        },
    )
    .unwrap();
    assert!(!summary.already_complete);
    let large = plan
        .manifest
        .files
        .keys()
        .position(|name| name == "data/large.bin")
        .unwrap();
    assert!(!workspace.join(format!("parts/{large:03}-0.part")).exists());
    assert!(
        OpenOptions::new()
            .write(true)
            .open(workspace.join("data/large.bin"))
            .is_err()
    );
    let result = serde_json::json!({"root":root,"workspace":workspace,"bytes":plan.manifest.files["data/large.bin"].object.bytes,"reservation_bytes":r.profile.maximum_bytes,"reservation_id":r.reservation.id,"recovered_in_new_process":true,"profile_change_refused":true,"reservation_change_refused":true,"large_in_place_part_absent":true,"capacity_preserved":true,"scratch_retired":false,"new_native_admission":false,"rendered":false,"published":false,"consumer_adopted":false});
    save(&root.join("qualification-result.json"), &result);
    save(&evidence.join("qualification-result.json"), &result);
}
