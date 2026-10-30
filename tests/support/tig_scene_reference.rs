//! Legacy master acceptance requires fresh primary comparison and live ownership.
use super::*;
use reel::{scene_primary_reference::Budget, tig_scene_production::jobs::reference::ReferencePlan};
use tig_core::tree::{MemoryNodes, NodeStore};
use tig_git::source::native::job::Role;
use tig_store::source_claims::Command;

#[test]
#[ignore = "requires native FFmpeg; guarded scene reference without original receipt"]
fn retained_scene_matches_without_history_and_with_independent_owners() {
    let mut p = Project::new();
    let _scene = p.render_delivery(1, "retained");
    p.commit("select retained scene master");
    let mut source = p.source();
    let cel: Object =
        serde_json::from_value(read(p.repo.path().join("records/cel-a.tig"))["selected"].clone())
            .unwrap();
    let mut nodes = MemoryNodes::default();
    let context = tig_scene::freeze_context(
        &mut source,
        &p.requests[1],
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let native = JobPlan::from_context(&mut source, &p.requests[1], &context, &p.programs).unwrap();
    let invalid = p.data.path().join("wrong-output");
    assert!(
        ReferencePlan::compile(
            &mut source,
            &native,
            "output/scene-b",
            "types/output",
            &invalid
        )
        .is_err()
    );
    assert!(!invalid.exists());
    assert!(
        ReferencePlan::compile(
            &mut source,
            &native,
            "output/scene-a",
            "types/cel",
            &invalid
        )
        .is_err()
    );
    assert!(!invalid.exists());
    let preparation = p.data.path().join("reference-controls");
    let plan = ReferencePlan::compile(
        &mut source,
        &native,
        "output/scene-a",
        "types/output",
        &preparation,
    )
    .unwrap();
    let resources = plan.freeze(&mut nodes, TreeLimits::default()).unwrap();
    let master: Object = serde_json::from_value(
        serde_json::json!({"sha256":plan.selected().sha256,"bytes":plan.selected().bytes}),
    )
    .unwrap();
    assert!(resources.of(Role::Composition).contains(&master));
    assert!(!resources.of(Role::Primary).contains(&master));
    let missing = nodes
        .objects()
        .map(|(_, raw)| (Object::from_bytes(raw), raw))
        .find(|(object, raw)| {
            resources.of(Role::Evidence).contains(object)
                && raw.windows(7).any(|part| part == b"\"width\"")
        })
        .unwrap()
        .0;
    let mut damaged = MemoryNodes::default();
    for (_, raw) in nodes.objects() {
        if Object::from_bytes(raw) != missing {
            damaged.put(raw).unwrap();
        }
    }
    assert!(
        plan.readback(resources.selection(), &mut damaged, TreeLimits::default())
            .is_err()
    );
    let mut changed = read(p.repo.path().join("records/output-scene-a.tig"));
    changed["metadata"]["note"] = json!("new source revision");
    write(p.repo.path(), "records/output-scene-a.tig", &changed);
    git(p.repo.path(), &["add", "records/output-scene-a.tig"]);
    git(
        p.repo.path(),
        &["commit", "-m", "advance source after reference planning"],
    );
    let mut advanced_profile = p.profile.clone();
    advanced_profile["commit"] = json!(git(p.repo.path(), &["rev-parse", "HEAD"]));
    let advanced = Source::open(serde_json::from_value(advanced_profile).unwrap()).unwrap();
    let coordination = Coordination {
        root: p.data.path().join("coordination"),
    };
    let budget = reel::tig_episode::ProtectionBudget {
        max_objects: 10000,
        max_logical_bytes: 1024 * 1024 * 1024,
    };
    let mut journal =
        SourceJournal::open(&p.data.path().join("owners"), &p.owners.checkpoint()).unwrap();
    let identity = Identity {
        source: key("scene-reference"),
        owner: key("fixture"),
    };
    let peer_identity = Identity {
        source: key("scene-reference-peer"),
        owner: key("fixture"),
    };
    let protect = |journal: &mut SourceJournal, nodes: &mut MemoryNodes, identity: Identity| {
        plan.readback(resources.selection(), nodes, TreeLimits::default())
            .unwrap()
            .protect(
                &source,
                journal,
                &coordination,
                identity,
                0,
                &key("verify-reference"),
                &budget,
            )
            .unwrap()
    };
    let owned = protect(&mut journal, &mut nodes, identity.clone());
    let peer = protect(&mut journal, &mut nodes, peer_identity);
    // Source metadata is captured, generated controls are held as exact bytes;
    // neither old job/receipt files nor the source checkout is consulted below.
    for name in [".git", "records", "definitions"] {
        fs::rename(
            p.repo.path().join(name),
            p.repo.path().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    fs::rename(
        p.data.path().join("jobs"),
        p.data.path().join("offline-jobs"),
    )
    .unwrap();
    fs::rename(&preparation, p.data.path().join("offline-preparation")).unwrap();
    let stage = p.data.path().join("reference-stage");
    fs::create_dir(&stage).unwrap();
    let comparison_budget = Budget {
        max_staged_bytes: 16 * 1024 * 1024,
        timeout_seconds: 120,
        max_picture_processes: 16,
    };
    let proof = owned
        .verify_selected_master(&mut source, &mut journal, &stage, &comparison_budget)
        .unwrap();
    proof.verify_current(&source, &mut journal).unwrap();
    assert!(proof.verify_current(&advanced, &mut journal).is_err());
    assert_eq!(proof.selected(), plan.selected());
    assert_eq!(proof.scene().id, "scene/scene-a");
    let declaration: Value = serde_json::from_slice(&proof.declaration().unwrap()).unwrap();
    assert_eq!(declaration["original_producer_assertion"], false);
    assert_eq!(declaration["approval"], false);
    assert_eq!(declaration["authorizes_eviction"], false);
    assert_eq!(declaration["comparison"]["movie_files_written"], 0);
    assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
    let master_path = object_file(&p.hot(), &master);
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(&master_path)
            .is_err()
    );
    // Windows read guards exclude mutation while proof is in use.
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(object_file(&p.hot(), &cel))
            .is_err()
    );
    let peer_proof = peer
        .verify_selected_master(&mut source, &mut journal, &stage, &comparison_budget)
        .unwrap();
    journal
        .apply(
            key("withdraw-reference"),
            Command::Withdraw {
                source: identity.source,
                owner: identity.owner,
                expected_revision: 2,
            },
        )
        .unwrap();
    assert!(proof.verify_current(&source, &mut journal).is_err());
    assert!(
        owned
            .verify_selected_master(&mut source, &mut journal, &stage, &comparison_budget)
            .is_err()
    );
    peer_proof.verify_current(&source, &mut journal).unwrap();
    assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
}
