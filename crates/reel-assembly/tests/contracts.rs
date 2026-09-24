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

#[test]
fn batch_selection_advances_multiple_lanes_in_one_immutable_transaction() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock-v1", 'a'),
        slots: vec![
            Slot {
                slot_id: "cue.narration.es".into(),
                beat_id: "cue".into(),
                lane: Lane::Narration,
                disposition: Disposition::Unselected,
                selected_revision_id: None,
                revisions: vec![],
            },
            Slot {
                slot_id: "cue.picture".into(),
                beat_id: "cue".into(),
                lane: Lane::Picture,
                disposition: Disposition::Unselected,
                selected_revision_id: None,
                revisions: vec![],
            },
        ],
        events: vec![],
        nodes: vec![Node {
            id: "scene".into(),
            inputs: vec![],
            slots: vec!["cue.narration.es".into(), "cue.picture".into()],
            events: vec![],
        }],
        presentation_targets: vec![],
    };
    let request = SlotRevisionBatchRequest {
        schema: REVISION_BATCH_REQUEST_SCHEMA.into(),
        next_lock_logical_id: "lock-v2".into(),
        revisions: vec![
            SlotRevisionSelection {
                slot_id: "cue.narration.es".into(),
                revision: Revision {
                    revision_id: "r1".into(),
                    supersedes: None,
                    asset: asset("narration-es", 'b'),
                },
            },
            SlotRevisionSelection {
                slot_id: "cue.picture".into(),
                revision: Revision {
                    revision_id: "r1".into(),
                    supersedes: None,
                    asset: asset("picture", 'c'),
                },
            },
        ],
    };
    let next = append_selected_revisions(&graph, &request).unwrap();
    assert_eq!(
        next.slots
            .iter()
            .filter(|slot| slot.disposition == Disposition::Selected)
            .count(),
        2
    );
    assert_ne!(next.lock.sha256, graph.lock.sha256);
    assert_eq!(closure(&next, "scene").unwrap().selected_assets.len(), 2);
}

#[test]
fn batch_selection_rejects_duplicate_slot_without_partial_selection() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock", 'a'),
        slots: vec![Slot {
            slot_id: "slot".into(),
            beat_id: "beat".into(),
            lane: Lane::Picture,
            disposition: Disposition::Unselected,
            selected_revision_id: None,
            revisions: vec![],
        }],
        events: vec![],
        nodes: vec![Node {
            id: "scene".into(),
            inputs: vec![],
            slots: vec!["slot".into()],
            events: vec![],
        }],
        presentation_targets: vec![],
    };
    let request = SlotRevisionBatchRequest {
        schema: REVISION_BATCH_REQUEST_SCHEMA.into(),
        next_lock_logical_id: "next".into(),
        revisions: vec![
            SlotRevisionSelection {
                slot_id: "slot".into(),
                revision: Revision {
                    revision_id: "r1".into(),
                    supersedes: None,
                    asset: asset("first", 'b'),
                },
            },
            SlotRevisionSelection {
                slot_id: "slot".into(),
                revision: Revision {
                    revision_id: "r2".into(),
                    supersedes: None,
                    asset: asset("second", 'c'),
                },
            },
        ],
    };
    assert!(append_selected_revisions(&graph, &request).is_err());
    assert_eq!(graph.slots[0].disposition, Disposition::Unselected);
}

#[test]
fn slot_extension_preserves_existing_selection_and_allows_later_sonic_revision() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock-v1", 'a'),
        slots: vec![Slot {
            slot_id: "scene.picture".into(),
            beat_id: "beat".into(),
            lane: Lane::Picture,
            disposition: Disposition::Selected,
            selected_revision_id: Some("picture-r1".into()),
            revisions: vec![Revision {
                revision_id: "picture-r1".into(),
                supersedes: None,
                asset: asset("picture", 'b'),
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
    let addition = SlotAddition {
        node_id: "scene".into(),
        slot: Slot {
            slot_id: "scene.sonic.car-arrival".into(),
            beat_id: "beat".into(),
            lane: Lane::Sonic,
            disposition: Disposition::Unselected,
            selected_revision_id: None,
            revisions: vec![],
        },
    };
    let request = SlotExtensionRequest {
        schema: SLOT_EXTENSION_REQUEST_SCHEMA.into(),
        additions: vec![addition.clone()],
        next_lock_logical_id: "lock-v2".into(),
    };
    let extended = extend_slots(&graph, &request).unwrap();
    assert_eq!(graph.slots.len(), 1);
    assert_eq!(extended.slots.len(), 2);
    assert_eq!(
        extended.slots[0].selected_revision_id.as_deref(),
        Some("picture-r1")
    );
    assert_ne!(extended.lock.sha256, graph.lock.sha256);
    assert_eq!(
        closure(&extended, "scene").unwrap().selected_assets.len(),
        1
    );
    let selected = append_selected_revisions(
        &extended,
        &SlotRevisionBatchRequest {
            schema: REVISION_BATCH_REQUEST_SCHEMA.into(),
            revisions: vec![SlotRevisionSelection {
                slot_id: addition.slot.slot_id,
                revision: Revision {
                    revision_id: "sonic-r1".into(),
                    supersedes: None,
                    asset: asset("car-arrival", 'c'),
                },
            }],
            next_lock_logical_id: "lock-v3".into(),
        },
    )
    .unwrap();
    assert_eq!(
        closure(&selected, "scene").unwrap().selected_assets.len(),
        2
    );
}

#[test]
fn slot_extension_rejects_duplicate_or_preselected_slots() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock-v1", 'a'),
        slots: vec![],
        events: vec![],
        nodes: vec![Node {
            id: "scene".into(),
            inputs: vec![],
            slots: vec![],
            events: vec![],
        }],
        presentation_targets: vec![],
    };
    let fresh = Slot {
        slot_id: "scene.sonic".into(),
        beat_id: "beat".into(),
        lane: Lane::Sonic,
        disposition: Disposition::Unselected,
        selected_revision_id: None,
        revisions: vec![],
    };
    let mut request = SlotExtensionRequest {
        schema: SLOT_EXTENSION_REQUEST_SCHEMA.into(),
        additions: vec![
            SlotAddition {
                node_id: "scene".into(),
                slot: fresh.clone(),
            },
            SlotAddition {
                node_id: "scene".into(),
                slot: fresh.clone(),
            },
        ],
        next_lock_logical_id: "lock-v2".into(),
    };
    assert!(extend_slots(&graph, &request).is_err());
    assert!(graph.slots.is_empty());
    request.additions.pop();
    request.additions[0].slot.disposition = Disposition::Selected;
    assert!(extend_slots(&graph, &request).is_err());
    request.additions[0].slot = fresh;
    request.additions[0].node_id = "unknown".into();
    assert!(extend_slots(&graph, &request).is_err());
}

#[test]
fn deprecation_retires_only_untouched_unselected_slots() {
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock-v1", 'a'),
        slots: vec![
            Slot {
                slot_id: "scene.old-picture".into(),
                beat_id: "beat".into(),
                lane: Lane::Picture,
                disposition: Disposition::Unselected,
                selected_revision_id: None,
                revisions: vec![],
            },
            Slot {
                slot_id: "scene.current-picture".into(),
                beat_id: "beat".into(),
                lane: Lane::Picture,
                disposition: Disposition::Selected,
                selected_revision_id: Some("current-r1".into()),
                revisions: vec![Revision {
                    revision_id: "current-r1".into(),
                    supersedes: None,
                    asset: asset("current", 'c'),
                }],
            },
        ],
        events: vec![],
        nodes: vec![Node {
            id: "scene".into(),
            inputs: vec![],
            slots: vec!["scene.old-picture".into(), "scene.current-picture".into()],
            events: vec![],
        }],
        presentation_targets: vec![],
    };
    let request = SlotDeprecationBatchRequest {
        schema: SLOT_DEPRECATION_BATCH_REQUEST_SCHEMA.into(),
        slot_ids: vec!["scene.old-picture".into()],
        next_lock_logical_id: "lock-v2".into(),
    };
    let revised = deprecate_unselected_slots(&graph, &request).unwrap();
    assert_eq!(graph.slots[0].disposition, Disposition::Unselected);
    assert_eq!(revised.slots[0].disposition, Disposition::Deprecated);
    assert_ne!(revised.lock.sha256, graph.lock.sha256);
    assert_eq!(closure(&revised, "scene").unwrap().selected_assets.len(), 1);
    let mut invalid = request;
    invalid.slot_ids = vec!["scene.old-picture".into(), "scene.old-picture".into()];
    assert!(deprecate_unselected_slots(&graph, &invalid).is_err());
    invalid.slot_ids = vec!["scene.current-picture".into()];
    assert!(deprecate_unselected_slots(&graph, &invalid).is_err());
    invalid.slot_ids = vec!["scene.missing".into()];
    assert!(deprecate_unselected_slots(&graph, &invalid).is_err());
}

#[test]
fn semantic_events_bind_to_selected_language_local_narration_and_picture_slots() {
    let narration = asset("narration-es", 'b');
    let picture = asset("picture", 'c');
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock-v1", 'a'),
        slots: vec![
            Slot {
                slot_id: "cue.narration.es".into(),
                beat_id: "cue".into(),
                lane: Lane::Narration,
                disposition: Disposition::Selected,
                selected_revision_id: Some("r1".into()),
                revisions: vec![Revision {
                    revision_id: "r1".into(),
                    supersedes: None,
                    asset: narration.clone(),
                }],
            },
            Slot {
                slot_id: "cue.picture".into(),
                beat_id: "cue".into(),
                lane: Lane::Picture,
                disposition: Disposition::Selected,
                selected_revision_id: Some("r1".into()),
                revisions: vec![Revision {
                    revision_id: "r1".into(),
                    supersedes: None,
                    asset: picture.clone(),
                }],
            },
        ],
        events: vec![],
        nodes: vec![Node {
            id: "scene".into(),
            inputs: vec![],
            slots: vec!["cue.narration.es".into(), "cue.picture".into()],
            events: vec![],
        }],
        presentation_targets: vec![],
    };
    let event = SemanticEvent {
        event_id: "cue.es.phrase-1".into(),
        scene_id: "scene".into(),
        language: "es".into(),
        narration: ImmutableRef {
            logical_id: narration.logical_id.clone(),
            sha256: narration.sha256.clone(),
        },
        picture: ImmutableRef {
            logical_id: picture.logical_id.clone(),
            sha256: picture.sha256.clone(),
        },
        phrase_start_seconds: 0.0,
        phrase_end_seconds: 1.25,
    };
    let request = EventBindingRequest {
        schema: EVENT_BINDING_REQUEST_SCHEMA.into(),
        next_lock_logical_id: "lock-v2".into(),
        bindings: vec![SemanticEventBinding {
            event,
            node_id: "scene".into(),
            narration_slot_id: "cue.narration.es".into(),
            picture_slot_id: "cue.picture".into(),
            supersedes_event_id: None,
        }],
    };
    let next = append_semantic_events(&graph, &request).unwrap();
    assert_ne!(next.lock.sha256, graph.lock.sha256);
    assert_eq!(next.nodes[0].events, vec!["cue.es.phrase-1"]);
    assert_eq!(closure(&next, "scene").unwrap().semantic_events.len(), 1);
}

#[test]
fn semantic_events_reject_wrong_language_take_or_unselected_picture() {
    let narration = asset("narration-es", 'b');
    let picture = asset("picture", 'c');
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock", 'a'),
        slots: vec![
            Slot {
                slot_id: "narration".into(),
                beat_id: "cue".into(),
                lane: Lane::Narration,
                disposition: Disposition::Selected,
                selected_revision_id: Some("r1".into()),
                revisions: vec![Revision {
                    revision_id: "r1".into(),
                    supersedes: None,
                    asset: narration.clone(),
                }],
            },
            Slot {
                slot_id: "picture".into(),
                beat_id: "cue".into(),
                lane: Lane::Picture,
                disposition: Disposition::Unselected,
                selected_revision_id: None,
                revisions: vec![],
            },
        ],
        events: vec![],
        nodes: vec![Node {
            id: "scene".into(),
            inputs: vec![],
            slots: vec![],
            events: vec![],
        }],
        presentation_targets: vec![],
    };
    let request = EventBindingRequest {
        schema: EVENT_BINDING_REQUEST_SCHEMA.into(),
        next_lock_logical_id: "next".into(),
        bindings: vec![SemanticEventBinding {
            event: SemanticEvent {
                event_id: "event".into(),
                scene_id: "scene".into(),
                language: "en".into(),
                narration: ImmutableRef {
                    logical_id: narration.logical_id,
                    sha256: narration.sha256,
                },
                picture: ImmutableRef {
                    logical_id: picture.logical_id,
                    sha256: picture.sha256,
                },
                phrase_start_seconds: 0.0,
                phrase_end_seconds: 1.0,
            },
            node_id: "scene".into(),
            narration_slot_id: "narration".into(),
            picture_slot_id: "picture".into(),
            supersedes_event_id: None,
        }],
    };
    assert!(append_semantic_events(&graph, &request).is_err());
}

#[test]
fn semantic_event_supersession_preserves_history_and_replaces_active_binding() {
    let narration = asset("narration-es", 'b');
    let picture_old = asset("picture-old", 'c');
    let picture_new = asset("picture-new", 'd');
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: reference("lock", 'a'),
        slots: vec![
            Slot {
                slot_id: "cue.narration.es".into(),
                beat_id: "cue".into(),
                lane: Lane::Narration,
                disposition: Disposition::Selected,
                selected_revision_id: Some("n1".into()),
                revisions: vec![Revision {
                    revision_id: "n1".into(),
                    supersedes: None,
                    asset: narration.clone(),
                }],
            },
            Slot {
                slot_id: "cue.picture".into(),
                beat_id: "cue".into(),
                lane: Lane::Picture,
                disposition: Disposition::Selected,
                selected_revision_id: Some("p2".into()),
                revisions: vec![
                    Revision {
                        revision_id: "p1".into(),
                        supersedes: None,
                        asset: picture_old.clone(),
                    },
                    Revision {
                        revision_id: "p2".into(),
                        supersedes: Some("p1".into()),
                        asset: picture_new.clone(),
                    },
                ],
            },
        ],
        events: vec![SemanticEvent {
            event_id: "event-old".into(),
            scene_id: "scene".into(),
            language: "es".into(),
            narration: reference("narration-es", 'b'),
            picture: reference("picture-old", 'c'),
            phrase_start_seconds: 1.0,
            phrase_end_seconds: 3.0,
        }],
        nodes: vec![Node {
            id: "scene".into(),
            inputs: vec![],
            slots: vec!["cue.narration.es".into(), "cue.picture".into()],
            events: vec!["event-old".into()],
        }],
        presentation_targets: vec![],
    };
    let replacement = SemanticEventBinding {
        event: SemanticEvent {
            event_id: "event-new".into(),
            scene_id: "scene".into(),
            language: "es".into(),
            narration: reference("narration-es", 'b'),
            picture: reference("picture-new", 'd'),
            phrase_start_seconds: 1.25,
            phrase_end_seconds: 2.75,
        },
        node_id: "scene".into(),
        narration_slot_id: "cue.narration.es".into(),
        picture_slot_id: "cue.picture".into(),
        supersedes_event_id: Some("event-old".into()),
    };
    let request = EventBindingRequest {
        schema: EVENT_BINDING_REQUEST_SCHEMA.into(),
        bindings: vec![replacement.clone()],
        next_lock_logical_id: "next".into(),
    };
    let next = append_semantic_events(&graph, &request).unwrap();
    assert_eq!(next.events.len(), 2);
    assert_eq!(next.nodes[0].events, vec!["event-new"]);
    assert_eq!(closure(&next, "scene").unwrap().semantic_events.len(), 1);
    assert!(
        append_semantic_events(
            &next,
            &EventBindingRequest {
                next_lock_logical_id: "again".into(),
                ..request
            }
        )
        .is_err()
    );
}
