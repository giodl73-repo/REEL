//! Filesystem recovery component fixtures, not simulated native custody.
use super::*;

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
