use reel_assembly::*;

fn fixture() -> Graph {
    Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: ImmutableRef {
            logical_id: "source".into(),
            sha256: "a".repeat(64),
        },
        slots: vec![],
        events: vec![],
        nodes: vec![Node {
            id: "root".into(),
            inputs: vec![],
            slots: vec![],
            events: vec![],
        }],
        presentation_targets: vec![],
    }
}

#[test]
fn native_lock_binds_graph_and_requested_identity_without_recursive_hashing() {
    let mut graph = fixture();
    let first = graph_lock(&graph, "snapshot").unwrap();
    graph.lock.sha256 = "b".repeat(64);
    assert_eq!(graph_lock(&graph, "snapshot").unwrap().sha256, first.sha256);
    assert_ne!(
        graph_lock(&graph, "another-snapshot").unwrap().sha256,
        first.sha256
    );
    graph.nodes.push(Node {
        id: "dependent".into(),
        inputs: vec!["root".into()],
        slots: vec![],
        events: vec![],
    });
    assert_ne!(graph_lock(&graph, "snapshot").unwrap().sha256, first.sha256);
}

#[test]
fn native_lock_rejects_invalid_graph_or_requested_identity() {
    let mut graph = fixture();
    assert!(graph_lock(&graph, "").is_err());
    graph.nodes[0].inputs.push("absent".into());
    assert!(graph_lock(&graph, "snapshot").is_err());
}
