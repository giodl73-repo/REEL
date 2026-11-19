//! Filesystem recovery component fixtures, not simulated native custody.
use super::*;

#[test]
#[ignore = "Existing accepted synthetic replacement job only; owned staging capacity/ticket preserved; no new native admission or publication"]
fn native_streamed_export_reopens_original_admission_and_preserves_ticket_claims_and_bytes() {
    let original = PathBuf::from(
        std::env::var_os("REEL_STREAMED_STAGE_ORIGINAL").expect("Set original replacement job"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_STREAMED_STAGE_EVIDENCE").expect("Set evidence directory"),
    );
    let packet: Packet = crate::read_control(&original.join("replacement-packet.json")).unwrap();
    let source: NativeProfile =
        crate::read_control(&original.join("replacement-source.json")).unwrap();
    let admission: admission::Profile =
        crate::read_control(&original.join("replacement-profile.json")).unwrap();
    let receipt: Receipt = crate::read_control(&original.join("replacement-receipt.json")).unwrap();
    assert!(packet.task.tool.is_none());
    let claims_path = admission.index_root.join("claims.json");
    let before = fs::read(&claims_path).unwrap();
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    let marker = serde_json::json!({"root":root,"original":original,"task_key":packet.task_key,"new_native_admission":false});
    fs::write(root.join("original-operation.json"), json(&marker).unwrap()).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(evidence.join("original-operation-roots.json"))
        .unwrap()
        .write_all(&json(&marker).unwrap())
        .unwrap();
    eprintln!("Native streamed staging root: {}", root.display());
    let parent = root.join("scratch");
    fs::create_dir(&parent).unwrap();
    let profile = Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent,
        maximum_bytes: 16 * 1024 * 1024,
        capacity_policy: capacity::Policy {
            minimum_free_bytes: 0,
            minimum_free_basis_points: 0,
            max_object_bytes: MAX_BYTES,
            ..Default::default()
        },
    };
    let ticket = prepare(&packet, &source, &admission, &receipt, &profile).unwrap();
    let raw = ticket.canonical_bytes().unwrap();
    let pin = reel_workflow::digest(&raw);
    fs::write(root.join("ticket.json"), &raw).unwrap();
    fs::write(root.join("ticket-sha256.txt"), &pin).unwrap();
    let first = export(&packet, &source, &admission, &raw, &pin).unwrap();
    assert!(!first.already_complete);
    let replay = export(&packet, &source, &admission, &raw, &pin).unwrap();
    assert!(replay.already_complete);
    assert_eq!(first.staged_files, replay.staged_files);
    assert_eq!(first.logical_bytes, replay.logical_bytes);
    // The dedicated stream port retains actual native object gates and never
    // inserts media into the buffered InputReader's captured map.
    let mut native = NativeInputs::open_admitted(&packet, &source, &admission, &receipt).unwrap();
    let plan = plan(&packet, &source, &receipt).unwrap();
    assert!(native.captured.is_empty());
    for (path, entry) in &plan.manifest.files {
        if let Some(id) = &entry.input {
            let asset = packet
                .task
                .inputs
                .iter()
                .find(|a| &a.logical_id == id)
                .unwrap();
            native
                .with_stream(asset, |reader| {
                    transfer(reader, &mut std::io::sink(), &entry.object)
                })
                .unwrap();
            assert!(native.captured.is_empty());
            let (actual, _held) =
                observe_held(&ticket.directory().join(path), entry.object.bytes).unwrap();
            assert_eq!(actual, entry.object);
        }
    }
    assert_eq!(native.held.len(), packet.task.inputs.len());
    drop(native);
    assert_eq!(fs::read(root.join("ticket.json")).unwrap(), raw);
    assert_eq!(fs::read(claims_path).unwrap(), before);
    let result = serde_json::json!({"root":root,"original":original,"task_key":packet.task_key,"staged_files":first.staged_files,"logical_bytes":first.logical_bytes,"original_admission_reopened":true,"native_stream_gates_retained":true,"native_capture_map_empty":true,"replay_verified":true,"ticket_and_claims_unchanged":true,"capacity_preserved":true,"new_native_admission":false,"published":false,"selected":false,"scratch_retired":false,"consumer_adopted":false});
    fs::write(
        root.join("qualification-result.json"),
        json(&result).unwrap(),
    )
    .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(evidence.join("qualification-result.json"))
        .unwrap()
        .write_all(&json(&result).unwrap())
        .unwrap();
}

#[derive(Default)]
struct GeneratedStreams {
    fail_after: Option<u64>,
    length_delta: i64,
    corrupt: bool,
    observed_buffer: usize,
    consumed: u64,
}
impl InputStreams for GeneratedStreams {
    fn with_stream<T>(
        &mut self,
        _id: &str,
        expected: &Object,
        consume: impl FnOnce(&mut dyn Read) -> Result<T>,
    ) -> Result<T> {
        struct Generated<'a> {
            source: &'a mut GeneratedStreams,
            length: u64,
            offset: u64,
            interrupted: bool,
        }
        impl Read for Generated<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.source.observed_buffer = self.source.observed_buffer.max(buffer.len());
                assert!(buffer.len() <= 65536);
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                if self.source.fail_after.is_some_and(|n| self.offset >= n) {
                    return Err(std::io::Error::other("injected stream interruption"));
                }
                let mut count = (self.length - self.offset).min(buffer.len() as u64).min(17);
                if let Some(n) = self.source.fail_after {
                    count = count.min(n - self.offset);
                }
                buffer[..count as usize].fill(if self.source.corrupt { 38 } else { 37 });
                self.offset += count;
                self.source.consumed += count;
                Ok(count as usize)
            }
        }
        let length = expected
            .bytes
            .checked_add_signed(self.length_delta)
            .unwrap();
        consume(&mut Generated {
            source: self,
            length,
            offset: 0,
            interrupted: false,
        })
    }
}
fn streamed_case() -> (tempfile::TempDir, Profile, Plan, Owner) {
    let (temp, profile, mut plan, _, _) = case();
    plan.manifest.operation = reel_workflow::digest(b"generated streaming input fixture");
    plan.manifest
        .files
        .get_mut("inputs/000.bin")
        .unwrap()
        .object = Object::from_bytes(&vec![37; 2 * 65536 + 31]);
    let parent = open_workspace_directory(&profile.parent).unwrap();
    let owner = create_owner(&plan, &parent).unwrap();
    (temp, profile, plan, owner)
}
#[test]
fn generated_stream_materialization_bounds_buffers_and_rechecks_completed_source_without_rewriting()
{
    let (_temp, profile, plan, owner) = streamed_case();
    let hash = reel_workflow::digest(b"streamed input ticket");
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    let mut source = GeneratedStreams::default();
    materialize_streams(&mut view, &profile, &plan, &mut source, &hash, &mut |_| {
        Ok(())
    })
    .unwrap();
    assert_eq!(source.observed_buffer, 65536);
    assert_eq!(
        source.consumed,
        plan.manifest.files["inputs/000.bin"].object.bytes
    );
    drop(view);
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    let mut source = GeneratedStreams::default();
    assert!(
        materialize_streams(
            &mut view,
            &profile,
            &plan,
            &mut source,
            &hash,
            &mut |_| panic!("no rewriting completed stream")
        )
        .unwrap()
        .already_complete
    );
    assert_eq!(
        source.consumed,
        plan.manifest.files["inputs/000.bin"].object.bytes
    );
}
#[test]
fn interrupted_stream_preserves_partial_part_and_resumes_under_original_manifest() {
    let (_temp, profile, plan, owner) = streamed_case();
    let hash = reel_workflow::digest(b"streamed input ticket");
    let root = profile.parent.join(&owner.name);
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    let mut source = GeneratedStreams {
        fail_after: Some(70000),
        ..Default::default()
    };
    assert_eq!(
        materialize_streams(&mut view, &profile, &plan, &mut source, &hash, &mut |_| Ok(
            ()
        ))
        .unwrap_err()
        .kind,
        ErrorKind::NeedsRecovery
    );
    drop(view);
    assert_eq!(
        fs::metadata(root.join("parts/000-0.part")).unwrap().len(),
        70000
    );
    assert!(!root.join("inputs/000.bin").exists());
    assert!(!root.join("complete.json").exists());
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    materialize_streams(
        &mut view,
        &profile,
        &plan,
        &mut GeneratedStreams::default(),
        &hash,
        &mut |_| Ok(()),
    )
    .unwrap();
    assert_eq!(
        fs::metadata(root.join("parts/000-0.part")).unwrap().len(),
        70000
    );
    assert!(root.join("parts/000-1.part").exists());
    assert!(root.join("complete.json").exists());
}
#[test]
fn truncated_trailing_and_corrupt_streams_never_publish_target_or_completion() {
    for (delta, corrupt) in [(-1, false), (1, false), (0, true)] {
        let (_temp, profile, plan, owner) = streamed_case();
        let root = profile.parent.join(&owner.name);
        let mut view = open_owner(&profile, &owner, &plan).unwrap();
        let mut source = GeneratedStreams {
            length_delta: delta,
            corrupt,
            ..Default::default()
        };
        assert_eq!(
            materialize_streams(
                &mut view,
                &profile,
                &plan,
                &mut source,
                &reel_workflow::digest(b"streamed input ticket"),
                &mut |_| Ok(())
            )
            .unwrap_err()
            .kind,
            ErrorKind::Integrity
        );
        assert!(!root.join("inputs/000.bin").exists());
        assert!(!root.join("complete.json").exists());
        assert!(root.join("parts/000-0.part").exists());
    }
}

#[test]
fn in_place_producer_file_survives_interruption_without_a_buffer_or_copy_part() {
    let (_temp, profile, mut plan, _old_owner, buffers) = case();
    plan.manifest.operation = reel_workflow::digest(b"in-place producer fixture");
    let media = vec![37u8; 3 * 65536 + 17];
    let path = "media/master.mkv";
    plan.manifest.files.insert(
        path.into(),
        Entry {
            object: Object::from_bytes(&media),
            input: None,
        },
    );
    assert!(!plan.controls.contains_key(path));
    let parent = open_workspace_directory(&profile.parent).unwrap();
    let owner = create_owner(&plan, &parent).unwrap();
    let root = profile.parent.join(&owner.name);
    fs::create_dir(root.join("media")).unwrap();
    fs::write(root.join(path), &media).unwrap();
    let hash = reel_workflow::digest(b"in-place producer completion");
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    assert!(
        materialize(&mut view, &profile, &plan, &buffers, &hash, &mut |_| Err(
            error(ErrorKind::NeedsRecovery, "injected-interruption")
        ))
        .is_err()
    );
    drop(view);
    assert_eq!(fs::read(root.join(path)).unwrap(), media);
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    materialize(&mut view, &profile, &plan, &buffers, &hash, &mut |_| Ok(())).unwrap();
    let media_index = plan.manifest.files.keys().position(|p| p == path).unwrap();
    assert!(!root.join(format!("parts/{media_index:03}-0.part")).exists());
    assert!(
        scan(
            &mut view,
            &profile,
            &plan,
            &completion(&plan, &hash).unwrap()
        )
        .unwrap()
    );
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(root.join(path))
            .is_err()
    );
}

#[test]
fn missing_or_corrupt_in_place_file_preserves_bytes_and_never_completes() {
    for corrupt in [false, true] {
        let (_temp, profile, mut plan, _old_owner, buffers) = case();
        plan.manifest.operation = reel_workflow::digest(b"in-place refusal fixture");
        let path = "media/master.mkv";
        plan.manifest.files.insert(
            path.into(),
            Entry {
                object: Object::from_bytes(b"original"),
                input: None,
            },
        );
        let parent = open_workspace_directory(&profile.parent).unwrap();
        let owner = create_owner(&plan, &parent).unwrap();
        let root = profile.parent.join(&owner.name);
        fs::create_dir(root.join("media")).unwrap();
        if corrupt {
            fs::write(root.join(path), b"tampered").unwrap();
        }
        let mut view = open_owner(&profile, &owner, &plan).unwrap();
        let err = materialize(
            &mut view,
            &profile,
            &plan,
            &buffers,
            &reel_workflow::digest(b"in-place refusal"),
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(
            err.kind,
            if corrupt {
                ErrorKind::Integrity
            } else {
                ErrorKind::Missing
            }
        );
        assert!(!root.join("complete.json").exists());
        if corrupt {
            assert_eq!(fs::read(root.join(path)).unwrap(), b"tampered");
        }
    }
}
fn case() -> (tempfile::TempDir, Profile, Plan, Owner, Buffers) {
    let temp = tempfile::tempdir().unwrap();
    let profile = Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent: temp.path().into(),
        maximum_bytes: 8 * 1024 * 1024,
        capacity_policy: capacity::Policy {
            minimum_free_bytes: 0,
            minimum_free_basis_points: 0,
            max_object_bytes: MAX_BYTES,
            ..capacity::Policy::default()
        },
    };
    let bytes = b"synthetic staged bytes".to_vec();
    let artifact = Artifact {
        logical_id: "input/fixture".into(),
        sha256: reel_workflow::digest(&bytes),
        bytes: bytes.len() as u64,
        media_type: "application/json".into(),
    };
    let controls = BTreeMap::from([("packet.json".into(), b"{}".to_vec())]);
    let files = BTreeMap::from([
        (
            "inputs/000.bin".into(),
            Entry {
                object: Object::from_bytes(&bytes),
                input: Some(artifact.logical_id.clone()),
            },
        ),
        (
            "packet.json".into(),
            Entry {
                object: Object::from_bytes(b"{}"),
                input: None,
            },
        ),
    ]);
    let plan = Plan {
        manifest: Manifest {
            schema: "reel.workflow-staging-manifest.v1".into(),
            operation: reel_workflow::digest(b"filesystem component fixture"),
            files,
        },
        controls,
    };
    let parent = open_workspace_directory(temp.path()).unwrap();
    let owner = create_owner(&plan, &parent).unwrap();
    (
        temp,
        profile,
        plan,
        owner,
        Buffers(BTreeMap::from([(
            artifact.logical_id.clone(),
            (artifact, bytes),
        )])),
    )
}
#[test]
fn interrupted_copy_preserves_partial_evidence_and_resumes_with_exclusive_new_part() {
    let (_temp, profile, plan, owner, buffers) = case();
    let hash = reel_workflow::digest(b"externally pinned ticket");
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    assert!(
        materialize(&mut view, &profile, &plan, &buffers, &hash, &mut |_| Err(
            error(ErrorKind::NeedsRecovery, "injected-interruption")
        ))
        .is_err()
    );
    drop(view);
    let root = profile.parent.join(&owner.name);
    let part = root.join("parts/000-0.part");
    fs::OpenOptions::new()
        .write(true)
        .open(&part)
        .unwrap()
        .set_len(3)
        .unwrap();
    let original = fs::read(&part).unwrap();
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    materialize(&mut view, &profile, &plan, &buffers, &hash, &mut |_| Ok(())).unwrap();
    drop(view);
    assert_eq!(fs::read(&part).unwrap(), original);
    assert!(root.join("parts/000-1.part").is_file());
    assert_eq!(
        fs::read(root.join("inputs/000.bin")).unwrap(),
        b"synthetic staged bytes"
    );
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    let result = materialize(&mut view, &profile, &plan, &buffers, &hash, &mut |_| {
        panic!("completed bytes must not be rewritten")
    })
    .unwrap();
    assert!(result.already_complete);
}
#[test]
fn flushed_part_before_publication_is_reconciled_without_another_write() {
    let (_temp, profile, plan, owner, buffers) = case();
    let hash = reel_workflow::digest(b"ticket");
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    assert!(
        materialize(&mut view, &profile, &plan, &buffers, &hash, &mut |_| Err(
            error(ErrorKind::NeedsRecovery, "after-flush-before-link")
        ))
        .is_err()
    );
    drop(view);
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    let mut writes = Vec::new();
    materialize(&mut view, &profile, &plan, &buffers, &hash, &mut |index| {
        writes.push(index);
        Ok(())
    })
    .unwrap();
    assert!(!writes.contains(&0));
}
#[test]
fn changed_completed_inputs_and_unknown_entries_are_preserved_and_refused() {
    let (_temp, profile, plan, owner, buffers) = case();
    let hash = reel_workflow::digest(b"ticket");
    let root = profile.parent.join(&owner.name);
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    materialize(&mut view, &profile, &plan, &buffers, &hash, &mut |_| Ok(())).unwrap();
    drop(view);
    fs::write(root.join("inputs/000.bin"), b"changed input").unwrap();
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    assert!(materialize(&mut view, &profile, &plan, &buffers, &hash, &mut |_| Ok(())).is_err());
    drop(view);
    assert_eq!(
        fs::read(root.join("inputs/000.bin")).unwrap(),
        b"changed input"
    );
    let (_temp, profile, plan, owner, buffers) = case();
    let root = profile.parent.join(&owner.name);
    fs::write(root.join("unknown.bin"), b"preserve").unwrap();
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    assert!(materialize(&mut view, &profile, &plan, &buffers, &hash, &mut |_| Ok(())).is_err());
    assert_eq!(fs::read(root.join("unknown.bin")).unwrap(), b"preserve");
}
#[test]
fn replaced_workspace_identity_is_not_adopted_even_with_copied_intent() {
    let (_temp, profile, plan, owner, _buffers) = case();
    let root = profile.parent.join(&owner.name);
    let previous = profile.parent.join("previous-owned");
    assert_eq!(root.parent(), Some(profile.parent.as_path()));
    fs::rename(&root, &previous).unwrap();
    fs::create_dir(&root).unwrap();
    fs::copy(previous.join("intent.json"), root.join("intent.json")).unwrap();
    assert!(open_owner(&profile, &owner, &plan).is_err());
    assert!(previous.join("intent.json").exists());
}
#[test]
fn part_attempt_bound_preserves_all_failed_attempts() {
    let (_temp, profile, plan, owner, buffers) = case();
    let root = profile.parent.join(&owner.name);
    fs::create_dir(root.join("parts")).unwrap();
    for n in 0..ATTEMPTS {
        fs::write(root.join(format!("parts/000-{n}.part")), b"x").unwrap();
    }
    let mut view = open_owner(&profile, &owner, &plan).unwrap();
    assert!(
        materialize(
            &mut view,
            &profile,
            &plan,
            &buffers,
            &reel_workflow::digest(b"ticket"),
            &mut |_| Ok(())
        )
        .is_err()
    );
    assert!(!root.join("complete.json").exists());
    for n in 0..ATTEMPTS {
        assert_eq!(
            fs::read(root.join(format!("parts/000-{n}.part"))).unwrap(),
            b"x"
        );
    }
}
