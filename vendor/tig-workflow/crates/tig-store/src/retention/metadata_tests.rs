use super::tests::Fixture;
use super::*;
#[test]
fn metadata_observation_reuses_settled_claim_without_new_commands_or_payload_hash() {
    let mut f = Fixture::new();
    let receipt = f.retain();
    let before = f.ledger.checkpoint();
    let observed = observe_claim(&mut f.ledger, &f.store, &receipt.grant).unwrap();
    let value = serde_json::to_value(observed).unwrap();
    assert_eq!(value["claim"], serde_json::json!(receipt.grant.claim_id()));
    assert_eq!(value["settled_file_data"], receipt.charged_footprint);
    assert_eq!(value["payload_hashed"], false);
    assert_eq!(value["effective_hold"], false);
    assert_eq!(f.ledger.checkpoint(), before);
    std::fs::write(
        f.temp
            .path()
            .join("store/objects")
            .join(f.object().sha256.as_str()),
        b"BBB",
    )
    .unwrap();
    assert!(observe_claim(&mut f.ledger, &f.store, &receipt.grant).is_ok());
    assert!(verify(&mut f.ledger, &f.store, &receipt.grant).is_err());
    assert_eq!(f.ledger.checkpoint(), before);
}
#[test]
fn metadata_observation_refuses_released_claim_instead_of_renewing_it() {
    let mut f = crate::retention::release::tests::Fixture::new_removable();
    let attempt = f.start();
    let released = f.run(attempt).unwrap();
    f.finish(attempt, released.evidence);
    f.reopen();
    let before = f.ledger.checkpoint();
    assert!(matches!(
        observe_claim(&mut f.ledger, &f.store, &f.grant),
        Err(RetentionError::Released)
    ));
    assert_eq!(f.ledger.checkpoint(), before);
}
#[test]
fn metadata_observation_rejects_foreign_store_and_missing_acknowledged_payload() {
    let mut f = Fixture::new();
    let grant = f.retain().grant;
    let before = f.ledger.checkpoint();
    let other = crate::local::LocalStore::create(
        &f.temp.path().join("other"),
        crate::Key::parse("place").unwrap(),
    )
    .unwrap();
    assert!(observe_claim(&mut f.ledger, &other, &grant).is_err());
    std::fs::rename(
        f.temp
            .path()
            .join("store/objects")
            .join(f.object().sha256.as_str()),
        f.temp.path().join("hidden-payload"),
    )
    .unwrap();
    assert!(observe_claim(&mut f.ledger, &f.store, &grant).is_err());
    assert_eq!(f.ledger.checkpoint(), before);
}
