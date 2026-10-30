use super::*;

#[test]
fn larger_control_is_framed_as_bounded_existing_nodes() {
    let body = vec![b'x'; 150_000];
    let object = Object::from_bytes(&body);
    let (root, storage) = frame(&BTreeMap::from([(object.clone(), body.clone())])).unwrap();
    assert!(
        storage
            .values()
            .all(|raw| raw.len() <= tig_store::metadata::MAX_NODE_BYTES)
    );
    let mut reopened = MemoryNodes::default();
    for raw in storage.values() {
        reopened.put(raw).unwrap();
    }
    let (entries, _) = Tree::from_root(root)
        .entries(&mut reopened, TreeLimits::default())
        .unwrap();
    let mut restored = Vec::new();
    for (name, pin) in entries {
        if name.as_str().contains("/part-") {
            restored.extend(reopened.get(&pin).unwrap().unwrap());
        }
    }
    assert_eq!(restored, body);
    assert_eq!(Object::from_bytes(&restored), object);
}
