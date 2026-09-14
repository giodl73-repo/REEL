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
    };
    let report = closure(&graph, "episode").unwrap();
    assert_eq!(report.node_ids, vec!["episode", "scene"]);
    assert_eq!(report.selected_assets[0].logical_id, "cel-current");
    assert_eq!(report.semantic_events[0].event_id, "event.cue");
    assert_eq!(report.digest_sha256.len(), 64);
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
    };
    assert!(closure(&graph, "target").is_err());
}
