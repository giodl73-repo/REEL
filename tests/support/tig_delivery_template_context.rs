use super::*;
use tig_core::tree::{MemoryNodes, TreeLimits};

#[test]
fn frozen_scope_has_only_consumed_primary_bytes_and_replays_without_git_or_cache() {
    let (f, request) = fixture();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let (prepared, context) =
        adapter::freeze_context(&mut source, &request, &mut nodes, TreeLimits::default()).unwrap();
    assert_eq!(context.requirements().primary().len(), 3);
    assert!(context.requirements().controls().is_empty());
    assert_eq!(source.byte_requests(), 0);
    let producer = Object::from_bytes(b"explicit synthetic producer");
    let expected = adapter::recipe(&mut source, &request, &context, &producer).unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    drop(source);
    for name in [".git", "records", "definitions", "cache"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let (mut replay, restored) = adapter::restore_context(
        profile,
        &request,
        context.selection(),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let actual = adapter::recipe(&mut replay, &request, &restored, &producer).unwrap();
    assert_eq!(actual.fingerprint(), expected.fingerprint());
    assert_eq!(actual.checkpoint(), expected.checkpoint());
    assert_eq!(prepared.source_checkpoint, *replay.checkpoint());
    assert_eq!(replay.byte_requests(), 0);
}

#[test]
fn unused_alternative_and_annotations_preserve_recipe_but_used_replacement_changes_it() {
    let (mut f, request) = fixture();
    let producer = Object::from_bytes(b"explicit synthetic producer");
    let mut nodes = MemoryNodes::default();
    let mut source = f.source();
    let (_, old_context) =
        adapter::freeze_context(&mut source, &request, &mut nodes, Default::default()).unwrap();
    let old = adapter::recipe(&mut source, &request, &old_context, &producer).unwrap();
    let parent = fs::read(f.root().join("records/root.tig")).unwrap();
    let mut draft: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-a.tig")).unwrap()).unwrap();
    draft["id"] = json!("cel/unused");
    draft["selected"] = Value::Null;
    draft["status"]["selection_state"] = json!("draft");
    write(f.root(), "records/unused.tig", &draft);
    record(&f, "root", |r| {
        r["authored"]["binding_ids"]["unused"] = json!({"id":"cel/unused"});
        r["metadata"]["note"] = json!("non-production annotation");
    });
    commit(&mut f);
    let mut current = f.source();
    assert!(adapter::recipe(&mut current, &request, &old_context, &producer).is_err());
    let (_, context) =
        adapter::freeze_context(&mut current, &request, &mut nodes, Default::default()).unwrap();
    assert_eq!(context.requirements().primary().len(), 3);
    let same = adapter::recipe(&mut current, &request, &context, &producer).unwrap();
    assert_eq!(old.fingerprint(), same.fingerprint());
    assert_ne!(old.checkpoint(), same.checkpoint());
    let other = adapter::recipe(
        &mut current,
        &request,
        &context,
        &Object::from_bytes(b"other producer"),
    )
    .unwrap();
    assert_ne!(same.fingerprint(), other.fingerprint());
    let changed = Object::from_bytes(b"new selected picture; bytes deliberately unavailable");
    record(&f, "cel-b", |r| r["selected"] = json!(changed));
    commit(&mut f);
    let mut changed_source = f.source();
    let (_, changed_context) = adapter::freeze_context(
        &mut changed_source,
        &request,
        &mut nodes,
        Default::default(),
    )
    .unwrap();
    let changed_recipe =
        adapter::recipe(&mut changed_source, &request, &changed_context, &producer).unwrap();
    assert_ne!(same.fingerprint(), changed_recipe.fingerprint());
    assert_eq!(changed_source.byte_requests(), 0);
    assert_ne!(parent, fs::read(f.root().join("records/root.tig")).unwrap());
}

#[test]
fn control_change_and_foreign_request_refuse_old_context() {
    let (mut f, mut request) = fixture();
    let mut nodes = MemoryNodes::default();
    let mut source = f.source();
    let (_, context) =
        adapter::freeze_context(&mut source, &request, &mut nodes, Default::default()).unwrap();
    let archive = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    request.root_id = "not-the-admitted-root".into();
    assert!(
        adapter::restore_context(
            archive,
            &request,
            context.selection(),
            &mut nodes,
            Default::default()
        )
        .is_err()
    );
    request.root_id = "delivery/source".into();
    let producer = Object::from_bytes(b"explicit producer");
    let old = adapter::recipe(&mut source, &request, &context, &producer).unwrap();
    record(&f, "control-job", |r| {
        r["authored"]["document"]["width"] = json!(128)
    });
    commit(&mut f);
    let mut next = f.source();
    let (_, next_context) =
        adapter::freeze_context(&mut next, &request, &mut nodes, Default::default()).unwrap();
    let next_recipe = adapter::recipe(&mut next, &request, &next_context, &producer).unwrap();
    assert_ne!(old.fingerprint(), next_recipe.fingerprint());
}

#[cfg(windows)]
#[test]
#[ignore = "requires native FFmpeg; replay exact frozen delivery context before rendering"]
fn frozen_context_replay_prepares_and_checks_recording_delivery_without_source_checkout() {
    let (mut f, request) = fixture();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let (_, context) =
        adapter::freeze_context(&mut source, &request, &mut nodes, Default::default()).unwrap();
    let producer =
        Object::from_bytes(b"synthetic externally admitted producer; not an execution attestation");
    let before = adapter::recipe(&mut source, &request, &context, &producer).unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    drop(source);
    for name in [
        ".git",
        "records",
        "definitions",
        "job.json",
        "contract.json",
        "production.json",
    ] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let (mut restored, replay) = adapter::restore_context(
        profile,
        &request,
        context.selection(),
        &mut nodes,
        Default::default(),
    )
    .unwrap();
    let after = adapter::recipe(&mut restored, &request, &replay, &producer).unwrap();
    assert_eq!(before.fingerprint(), after.fingerprint());
    let prepared = adapter::lower(&mut restored, &request).unwrap();
    let inputs = restored.prepare(&prepared.job).unwrap();
    drop(restored);
    let controls = f.root().join("frozen-controls");
    fs::create_dir(&controls).unwrap();
    fs::write(
        controls.join("job.json"),
        serde_json::to_vec(&prepared.job).unwrap(),
    )
    .unwrap();
    fs::write(controls.join("contract.json"), prepared.contract_bytes).unwrap();
    fs::write(controls.join("production.json"), prepared.production_bytes).unwrap();
    let output = f.root().join("frozen-render");
    reel::scene_delivery::render_with_inputs(&controls.join("job.json"), &inputs, &output).unwrap();
    reel::scene_delivery::check_with_inputs(&controls.join("job.json"), &inputs, &output).unwrap();
    let pcm = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(output.join("M.wav"))
        .args(["-acodec", "pcm_s24le", "-f", "s24le", "-"])
        .output()
        .unwrap();
    assert!(pcm.status.success());
    assert!(
        pcm.stdout == wav()[44..],
        "frozen-context music PCM differs"
    );
}
