use reel_assembly::*;

fn hash(letter: char) -> String {
    std::iter::repeat_n(letter, 64).collect()
}
fn asset(id: &str, letter: char) -> Asset {
    let sha256 = hash(letter);
    Asset {
        logical_id: id.into(),
        cache_uri: format!("{CACHE_PREFIX}{sha256}"),
        sha256,
    }
}
fn reference(id: &str, letter: char) -> ImmutableRef {
    ImmutableRef {
        logical_id: id.into(),
        sha256: hash(letter),
    }
}

#[test]
fn closure_is_hash_bound_and_follows_inputs_slots_and_events() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock-v1", 'a'),
        slots: vec![Slot {
            slot_id: "s.picture".into(),
            beat_id: "beat.a".into(),
            lane: Lane::Picture,
            disposition: Disposition::Selected,
            selected_revision_id: Some("r2".into()),
            revisions: vec![
                Revision {
                    revision_id: "r1".into(),
                    supersedes: None,
                    asset: asset("cel-old", 'b'),
                },
                Revision {
                    revision_id: "r2".into(),
                    supersedes: Some("r1".into()),
                    asset: asset("cel-current", 'c'),
                },
            ],
        }],
        events: vec![SemanticEvent {
            event_id: "event.cue".into(),
            scene_id: "scene-1".into(),
            language: "es".into(),
            narration: reference("take", 'd'),
            picture: reference("cel-current", 'c'),
            phrase_start_seconds: 1.0,
            phrase_end_seconds: 2.0,
        }],
        nodes: vec![
            Node {
                id: "scene".into(),
                inputs: vec![],
                slots: vec!["s.picture".into()],
                events: vec!["event.cue".into()],
            },
            Node {
                id: "episode".into(),
                inputs: vec!["scene".into()],
                slots: vec![],
                events: vec![],
            },
        ],
        presentation_targets: vec![PresentationTarget {
            target_id: "episode-private-review".into(),
            node: "episode".into(),
            contract: reference("episode-presentation-v1", 'e'),
        }],
    };
    let report = closure(&graph, "episode").unwrap();
    assert_eq!(report.node_ids, vec!["episode", "scene"]);
    assert_eq!(report.selected_assets[0].logical_id, "cel-current");
    assert_eq!(report.semantic_events[0].event_id, "event.cue");
    assert_eq!(report.digest_sha256.len(), 64);
    let presentation = presentation_closure(&graph, "episode-private-review").unwrap();
    assert_eq!(presentation.closure.target, "episode");
    assert_eq!(presentation.contract.logical_id, "episode-presentation-v1");
    assert_eq!(presentation.digest_sha256.len(), 64);
    let selected = selected_closure(
        &SelectedPointer {
            schema: POINTER_SCHEMA.into(),
            logical_id: "s1e02-current-private".into(),
            selected_lock: graph.lock.clone(),
        },
        &graph,
        "episode",
    )
    .unwrap();
    assert_eq!(selected.closure.digest_sha256, report.digest_sha256);
    assert_eq!(selected.digest_sha256.len(), 64);
}

#[test]
fn rejects_filename_authority_and_nonappend_revision_selection() {
    let mut item = asset("cel", 'a');
    item.cache_uri = "assets/latest.png".into();
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock", 'b'),
        slots: vec![Slot {
            slot_id: "slot".into(),
            beat_id: "beat".into(),
            lane: Lane::Picture,
            disposition: Disposition::Selected,
            selected_revision_id: Some("r2".into()),
            revisions: vec![Revision {
                revision_id: "r2".into(),
                supersedes: Some("r1".into()),
                asset: item,
            }],
        }],
        events: vec![],
        nodes: vec![Node {
            id: "target".into(),
            inputs: vec![],
            slots: vec!["slot".into()],
            events: vec![],
        }],
        presentation_targets: vec![],
    };
    assert!(closure(&graph, "target").is_err());
}

#[test]
fn unselected_slot_is_valid_but_contributes_no_asset_to_a_closure() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("episode-3-planning-lock", 'a'),
        slots: vec![Slot {
            slot_id: "s1e03.scene-001.picture.beat-001".into(),
            beat_id: "s1e03.scene-001.beat-001".into(),
            lane: Lane::Picture,
            disposition: Disposition::Unselected,
            selected_revision_id: None,
            revisions: vec![],
        }],
        events: vec![],
        nodes: vec![Node {
            id: "s1e03.scene-001".into(),
            inputs: vec![],
            slots: vec!["s1e03.scene-001.picture.beat-001".into()],
            events: vec![],
        }],
        presentation_targets: vec![],
    };

    let report = closure(&graph, "s1e03.scene-001").unwrap();
    assert_eq!(report.node_ids, vec!["s1e03.scene-001"]);
    assert!(report.selected_assets.is_empty());
}

#[test]
fn unselected_slot_cannot_name_a_selected_revision() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("planning-lock", 'a'),
        slots: vec![Slot {
            slot_id: "slot".into(),
            beat_id: "beat".into(),
            lane: Lane::Picture,
            disposition: Disposition::Unselected,
            selected_revision_id: Some("r1".into()),
            revisions: vec![Revision {
                revision_id: "r1".into(),
                supersedes: None,
                asset: asset("candidate", 'b'),
            }],
        }],
        events: vec![],
        nodes: vec![Node {
            id: "target".into(),
            inputs: vec![],
            slots: vec!["slot".into()],
            events: vec![],
        }],
        presentation_targets: vec![],
    };

    assert!(
        validate_graph(&graph)
            .unwrap_err()
            .to_string()
            .contains("cannot select a revision")
    );
}

#[test]
fn rejects_dependency_cycles_instead_of_silently_deduplicating_them() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock", 'a'),
        slots: vec![],
        events: vec![],
        nodes: vec![
            Node {
                id: "a".into(),
                inputs: vec!["b".into()],
                slots: vec![],
                events: vec![],
            },
            Node {
                id: "b".into(),
                inputs: vec!["a".into()],
                slots: vec![],
                events: vec![],
            },
        ],
        presentation_targets: vec![],
    };
    assert!(
        closure(&graph, "a")
            .unwrap_err()
            .to_string()
            .contains("dependency cycle")
    );
}

#[test]
fn presentation_target_must_bind_an_existing_node_and_changes_with_its_contract() {
    let mut graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock", 'a'),
        slots: vec![],
        events: vec![],
        nodes: vec![Node {
            id: "credits".into(),
            inputs: vec![],
            slots: vec![],
            events: vec![],
        }],
        presentation_targets: vec![PresentationTarget {
            target_id: "end-credits".into(),
            node: "credits".into(),
            contract: reference("credits-contract-v1", 'b'),
        }],
    };
    let first = presentation_closure(&graph, "end-credits").unwrap();
    graph.presentation_targets[0].contract = reference("credits-contract-v2", 'c');
    let second = presentation_closure(&graph, "end-credits").unwrap();
    assert_ne!(first.digest_sha256, second.digest_sha256);
    graph.presentation_targets[0].node = "missing".into();
    assert!(validate_graph(&graph).is_err());
}

#[test]
fn selected_pointer_rejects_a_stale_or_differently_named_graph_lock() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("episode-lock-v2", 'a'),
        slots: vec![],
        events: vec![],
        nodes: vec![Node {
            id: "episode".into(),
            inputs: vec![],
            slots: vec![],
            events: vec![],
        }],
        presentation_targets: vec![],
    };
    let stale = SelectedPointer {
        schema: POINTER_SCHEMA.into(),
        logical_id: "episode-current".into(),
        selected_lock: reference("episode-lock-v1", 'b'),
    };
    assert!(selected_closure(&stale, &graph, "episode").is_err());
}

#[test]
fn selected_revision_is_append_only_and_advances_the_pointer_to_a_new_lock() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("episode-lock-v1", 'a'),
        slots: vec![Slot {
            slot_id: "scene.picture".into(),
            beat_id: "scene.beat".into(),
            lane: Lane::Picture,
            disposition: Disposition::Selected,
            selected_revision_id: Some("r1".into()),
            revisions: vec![Revision {
                revision_id: "r1".into(),
                supersedes: None,
                asset: asset("cel-v1", 'b'),
            }],
        }],
        events: vec![],
        nodes: vec![Node {
            id: "scene".into(),
            inputs: vec![],
            slots: vec!["scene.picture".into()],
            events: vec![],
        }],
        presentation_targets: vec![],
    };
    let request = SlotRevisionRequest {
        schema: REVISION_REQUEST_SCHEMA.into(),
        slot_id: "scene.picture".into(),
        revision: Revision {
            revision_id: "r2".into(),
            supersedes: Some("r1".into()),
            asset: asset("cel-v2", 'c'),
        },
        next_lock_logical_id: "episode-lock-v2".into(),
    };
    let next = append_selected_revision(&graph, &request).unwrap();
    assert_eq!(next.slots[0].revisions.len(), 2);
    assert_eq!(next.slots[0].selected_revision_id.as_deref(), Some("r2"));
    assert_ne!(next.lock.sha256, graph.lock.sha256);
    let pointer = advance_pointer("episode-current", &next).unwrap();
    assert!(selected_closure(&pointer, &next, "scene").is_ok());
    assert!(selected_closure(&pointer, &graph, "scene").is_err());
}

#[test]
fn selected_revision_cannot_rewind_or_branch_from_the_current_asset() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock-v1", 'a'),
        slots: vec![Slot {
            slot_id: "picture".into(),
            beat_id: "beat".into(),
            lane: Lane::Picture,
            disposition: Disposition::Selected,
            selected_revision_id: Some("r2".into()),
            revisions: vec![
                Revision {
                    revision_id: "r1".into(),
                    supersedes: None,
                    asset: asset("old", 'b'),
                },
                Revision {
                    revision_id: "r2".into(),
                    supersedes: Some("r1".into()),
                    asset: asset("current", 'c'),
                },
            ],
        }],
        events: vec![],
        nodes: vec![Node {
            id: "scene".into(),
            inputs: vec![],
            slots: vec!["picture".into()],
            events: vec![],
        }],
        presentation_targets: vec![],
    };
    let request = SlotRevisionRequest {
        schema: REVISION_REQUEST_SCHEMA.into(),
        slot_id: "picture".into(),
        revision: Revision {
            revision_id: "r3".into(),
            supersedes: Some("r1".into()),
            asset: asset("stale-branch", 'd'),
        },
        next_lock_logical_id: "lock-v3".into(),
    };
    assert!(append_selected_revision(&graph, &request).is_err());
}
