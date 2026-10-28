use super::*;
use reel::tig_inputs::ArchiveProfile;
use tig_assets::graph::recipe::{
    Plan,
    produced::{Claim, ContextPin, Correspondence},
};
use tig_core::tree::{MemoryNodes, NodeStore, TreeLimits};
use tig_git::source::native::context::ProductionContext;

#[test]
#[ignore = "explicit existing-source projected recipe; requires profile and request paths"]
fn explicit_existing_source_projected_recipe_rehearsal() {
    use reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA;
    use tig_store::metadata::{Limits, MetadataReader, MetadataStore};
    let profile_path =
        std::env::var_os("REEL_TIG_SCOPE_PROFILE").expect("explicit profile required");
    let request_path =
        std::env::var_os("REEL_TIG_SCOPE_REQUEST").expect("explicit request required");
    let profile = Profile::read(Path::new(&profile_path)).unwrap();
    let admitted = profile.admitted.clone();
    let byte_source = profile.byte_source.clone();
    let limits = profile.limits;
    let request = reel::tig_scene::Request::read(Path::new(&request_path)).unwrap();
    let mut source = Source::open(profile).unwrap();
    let disk = tempfile::tempdir().unwrap();
    let path = disk.path().join("metadata");
    let mut nodes = MetadataStore::create(
        &path,
        Limits {
            nodes: 32768,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let producer = Object::from_bytes(
        b"metadata-only external producer declaration, not actual runtime admission",
    );
    let context = reel::tig_scene::freeze_render_context(
        &mut source,
        &request,
        &disk.path().join("before"),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let original = reel::tig_scene::render_recipe(
        &mut source,
        &request,
        &context,
        &producer,
        &disk.path().join("before-recipe"),
    )
    .unwrap();
    let profile = ArchiveProfile {
        schema: ARCHIVE_PROFILE_SCHEMA.into(),
        archive: context.archive().clone(),
        checkpoint: source.checkpoint().clone(),
        limits,
        admitted,
        byte_source,
    };
    let witness = nodes.finish().unwrap();
    drop(nodes);
    drop(source);
    let mut reader = MetadataReader::open(&path, &witness).unwrap();
    let (mut source, restored) = reel::tig_scene::restore_render_context(
        profile,
        &request,
        context.selection(),
        &disk.path().join("after"),
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    let replay = reel::tig_scene::render_recipe(
        &mut source,
        &request,
        &restored,
        &producer,
        &disk.path().join("after-recipe"),
    )
    .unwrap();
    assert!(
        original.fingerprint() == replay.fingerprint(),
        "recipe differs"
    );
    assert!(
        context.requirements() == restored.requirements(),
        "requirements differ"
    );
    assert!(
        original.operation() == replay.operation(),
        "operation differs"
    );
    assert!(!replay.complete_semantic_coverage());
    println!(
        "{}",
        json!({"recipe_equal":true,"primary":context.requirements().primary().len(),"controls":context.requirements().controls().len(),"roots":context.roots().len(),"media_rendered":false,"actual_runtime_admitted":false,"authorizes_eviction":false})
    );
}

pub(super) fn plan(
    f: &Fixture,
    request: &reel::tig_scene::Request,
    producer: &Object,
    name: &str,
    nodes: &mut impl NodeStore,
) -> (ProductionContext, Plan, ArchiveProfile) {
    let mut source = f.source();
    let context = reel::tig_scene::freeze_render_context(
        &mut source,
        request,
        &f.root().join(format!("{name}-context")),
        nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let plan = reel::tig_scene::render_recipe(
        &mut source,
        request,
        &context,
        producer,
        &f.root().join(format!("{name}-recipe")),
    )
    .unwrap();
    assert_eq!(source.byte_requests(), 12);
    assert_eq!(
        &plan.operation().parameters,
        &Object::from_bytes(context.descriptor())
    );
    assert_eq!(plan.binding(), context.requirements().binding_digest());
    assert!(!plan.complete_semantic_coverage());
    let profile = archived_profile(f, context.archive().clone(), source.checkpoint().clone());
    (context, plan, profile)
}

fn edit_text(f: &mut Fixture, language: &str) {
    let mut parent = load(f, "records/scene.tig");
    let pin: Object = serde_json::from_value(parent["selected"].clone()).unwrap();
    let mut scene: Value = serde_json::from_slice(
        &fs::read(f.root().join("cache/objects").join(pin.sha256.as_str())).unwrap(),
    )
    .unwrap();
    scene["languages"][language]["cues"][0]["exact_text_sha256"] = json!("b".repeat(64));
    parent["selected"] = json!(cached(f, &serde_json::to_vec(&scene).unwrap()));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(f);
}

#[test]
fn projected_recipe_ignores_other_language_controls_but_retains_them_and_invalidates_consumed_changes()
 {
    let (mut f, request) = bilingual();
    let producer = Object::from_bytes(b"explicit synthetic producer");
    let mut nodes = MemoryNodes::default();
    let (before_context, before, _) = plan(&f, &request, &producer, "before", &mut nodes);
    edit_text(&mut f, "es");
    let (other_context, other, _) = plan(&f, &request, &producer, "other-language", &mut nodes);
    assert_eq!(before.fingerprint(), other.fingerprint());
    assert_ne!(before.checkpoint(), other.checkpoint());
    assert_ne!(before.binding(), other.binding());
    assert_ne!(
        before_context.requirements().controls(),
        other_context.requirements().controls()
    );
    assert_eq!(
        before_context.requirements().primary(),
        other_context.requirements().primary()
    );
    edit_text(&mut f, "en");
    let (text_context, text, _) = plan(&f, &request, &producer, "selected-text", &mut nodes);
    assert_ne!(other.fingerprint(), text.fingerprint());
    assert_eq!(
        other_context.requirements().primary(),
        text_context.requirements().primary()
    );
    let a: Value = serde_json::from_slice(other_context.descriptor()).unwrap();
    let b: Value = serde_json::from_slice(text_context.descriptor()).unwrap();
    assert_eq!(a["compiled_inputs"], b["compiled_inputs"]);
    let mut cel = load(&f, "records/cel-b.tig");
    let replacement = cached(&f, &picture([0, 255, 0]));
    cel["selected"] = json!(replacement);
    write(f.root(), "records/cel-b.tig", &cel);
    save_commit(&mut f);
    let (cel_context, changed, _) = plan(&f, &request, &producer, "selected-cel", &mut nodes);
    assert_ne!(text.fingerprint(), changed.fingerprint());
    assert_eq!(
        cel_context
            .requirements()
            .primary()
            .difference(text_context.requirements().primary())
            .cloned()
            .collect::<Vec<_>>(),
        vec![replacement]
    );
    let runtime = reel::tig_scene::render_recipe(
        &mut f.source(),
        &request,
        &cel_context,
        &Object::from_bytes(b"other producer"),
        &f.root().join("runtime-change"),
    )
    .unwrap();
    assert_ne!(changed.fingerprint(), runtime.fingerprint());
    let mut parent = load(&f, "records/scene.tig");
    let slot = parent["streams"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["slot"] == "profile.json")
        .unwrap();
    let pin: Object = serde_json::from_value(slot["object"].clone()).unwrap();
    let mut profile: Value = serde_json::from_slice(
        &fs::read(f.root().join("cache/objects").join(pin.sha256.as_str())).unwrap(),
    )
    .unwrap();
    profile["width"] = json!(32);
    slot["object"] = json!(cached(&f, &serde_json::to_vec(&profile).unwrap()));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let (profile_context, profile_plan, _) = plan(&f, &request, &producer, "profile", &mut nodes);
    assert_eq!(
        cel_context.requirements().primary(),
        profile_context.requirements().primary()
    );
    assert_ne!(changed.fingerprint(), profile_plan.fingerprint());
}

#[test]
fn projected_recipe_refuses_wrong_request_mode_and_source_before_byte_io_or_output() {
    let (mut f, mut request) = bilingual();
    let producer = Object::from_bytes(b"synthetic producer");
    let mut nodes = MemoryNodes::default();
    let (context, _, profile) = plan(&f, &request, &producer, "original", &mut nodes);
    let (mut bound, _) = reel::tig_scene::restore_render_context(
        profile,
        &request,
        context.selection(),
        &f.root().join("restore"),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let reads = bound.byte_requests();
    let output = f.root().join("refused");
    request.language = "es".into();
    assert!(
        reel::tig_scene::render_recipe(&mut bound, &request, &context, &producer, &output).is_err()
    );
    assert_eq!(bound.byte_requests(), reads);
    assert!(!output.exists());
    request.language = "en".into();
    let valid = reel::tig_scene::render_recipe(
        &mut bound,
        &request,
        &context,
        &producer,
        &f.root().join("bound-recipe"),
    )
    .unwrap();
    assert_eq!(
        valid.operation().parameters,
        Object::from_bytes(context.descriptor())
    );
    let mut source = f.source();
    let legacy =
        reel::tig_scene::freeze_context(&mut source, &request, &mut nodes, TreeLimits::default())
            .unwrap();
    assert!(
        reel::tig_scene::render_recipe(&mut source, &request, &legacy, &producer, &output).is_err()
    );
    assert_eq!(source.byte_requests(), 0);
    assert!(!output.exists());
    edit_text(&mut f, "es");
    let mut newer = f.source();
    assert!(
        reel::tig_scene::render_recipe(&mut newer, &request, &context, &producer, &output).is_err()
    );
    assert_eq!(newer.byte_requests(), 0);
    assert!(!output.exists());
}

#[test]
fn projected_claim_replays_original_context_offline_and_compares_current_recipes_without_metadata_writes()
 {
    use tig_assets::graph::recipe::produced;
    use tig_store::metadata::{Limits, MetadataReader, MetadataStore};
    let (mut f, request) = bilingual();
    let producer = Object::from_bytes(b"externally admitted test producer");
    let disk = tempfile::tempdir().unwrap();
    let path = disk.path().join("metadata");
    let mut nodes = MetadataStore::create(
        &path,
        Limits {
            nodes: 16384,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let (legacy, legacy_profile, subject) =
        metadata_claim_fixture(&f, &request, &producer, &mut nodes);
    let (context, original, profile) = plan(&f, &request, &producer, "original", &mut nodes);
    // Explicit unsigned assertion; never evidence that a producer executed.
    let claim = Claim::create(
        &original,
        ContextPin {
            root: context.selection().root.clone(),
            identity: context.selection().context.clone(),
        },
        vec![subject.subject().clone()],
    )
    .unwrap();
    edit_text(&mut f, "es");
    let (_, unaffected, _) = plan(&f, &request, &producer, "unaffected", &mut nodes);
    edit_text(&mut f, "en");
    let (_, changed, _) = plan(&f, &request, &producer, "changed", &mut nodes);
    let witness = nodes.finish().unwrap();
    drop(nodes);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let reader = MetadataReader::open(&path, &witness).unwrap();
    let mut readonly = ReadOnlyMetadata {
        inner: &reader,
        reads: 0.into(),
    };
    let (mut source, replay) = reel::tig_scene::restore_render_claim(
        profile.clone(),
        &request,
        &producer,
        &claim,
        &f.root().join("claim-replay"),
        &mut readonly,
        TreeLimits::default(),
    )
    .unwrap();
    assert!(readonly.reads.get() > 0);
    assert_eq!(source.byte_requests(), 6);
    assert_eq!(
        produced::compare(&unaffected, Some(&subject), Some(replay.checked())).unwrap(),
        Correspondence::ClaimedRecipeMatch
    );
    assert_eq!(
        produced::compare(&changed, Some(&subject), Some(replay.checked())).unwrap(),
        Correspondence::StaleRecipe
    );
    let replayed = reel::tig_scene::render_recipe(
        &mut source,
        &request,
        replay.context(),
        &producer,
        &f.root().join("replayed-plan"),
    )
    .unwrap();
    assert_eq!(original.fingerprint(), replayed.fingerprint());
    assert!(
        reel::tig_scene::restore_claim(
            profile.clone(),
            &request,
            &producer,
            &claim,
            &mut readonly,
            TreeLimits::default()
        )
        .is_err()
    );
    let refused = f.root().join("refused-legacy");
    assert!(
        reel::tig_scene::restore_render_claim(
            legacy_profile,
            &request,
            &producer,
            &legacy,
            &refused,
            &mut readonly,
            TreeLimits::default()
        )
        .is_err()
    );
    assert!(!refused.exists());
    let before = readonly.reads.get();
    assert!(
        reel::tig_scene::restore_render_claim(
            profile,
            &request,
            &Object::from_bytes(b"wrong producer"),
            &claim,
            &refused,
            &mut readonly,
            TreeLimits::default()
        )
        .is_err()
    );
    assert_eq!(readonly.reads.get(), before);
    assert!(!refused.exists());
}
