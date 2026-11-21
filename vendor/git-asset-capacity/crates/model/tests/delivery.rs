#![cfg(feature = "schema")]
use git_asset_core::{Object, identity};
use git_asset_model::{
    delivery::{self, Error, History, Stage, State},
    native::Instance,
    revisions::{self, Coverage, Limits, Reader},
    schema::{Dialect, Schema},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn raw(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn object(label: &str) -> Object {
    Object::from_bytes(label.as_bytes())
}
fn target_identity(key: &str) -> Value {
    serde_json::to_value(Object::from_bytes(&identity::canonical_json(&json!({
        "schema":"tig.declared-publication-target.experimental-1","provider":"synthetic-service",
        "scope":"test-scope","path":format!("{key}/file.bin")
    })).unwrap()).sha256).unwrap()
}
#[derive(Default)]
struct Store {
    objects: BTreeMap<Object, Vec<u8>>,
    reads: BTreeMap<Object, usize>,
}
impl Reader for Store {
    fn read(&mut self, expected: &Object, maximum: usize) -> Result<Vec<u8>, revisions::Error> {
        assert_eq!(expected.bytes as usize, maximum);
        *self.reads.entry(expected.clone()).or_default() += 1;
        self.objects
            .get(expected)
            .cloned()
            .ok_or(revisions::Error::Missing)
    }
}
impl Store {
    fn put(&mut self, value: Value) -> Object {
        self.bytes(raw(&value))
    }
    fn bytes(&mut self, bytes: Vec<u8>) -> Object {
        let pin = Object::from_bytes(&bytes);
        self.objects.insert(pin.clone(), bytes);
        pin
    }
    fn value(&self, pin: &Object) -> Value {
        serde_json::from_slice(&self.objects[pin]).unwrap()
    }
}
struct Fixture {
    store: Store,
    record: Value,
    definition: Vec<u8>,
    schema: Vec<u8>,
    a: Object,
    b: Object,
    policy: Object,
    mapping: Object,
}
impl Fixture {
    fn new(kind: &str) -> Self {
        let schema = raw(&json!({"type":"object","additionalProperties":false}));
        let definition = raw(
            &json!({"schema":"tig.definition.design-1","id":format!("example/{kind}"),
            "authored_schema":Object::from_bytes(&schema),"authoring":{"properties":[],"required":[],"defaults":{}},
            "stream_rules":{},"semantic_rules":{}}),
        );
        let mut definition_pin = serde_json::to_value(Object::from_bytes(&definition)).unwrap();
        definition_pin["id"] = json!(format!("example/{kind}"));
        let mut store = Store::default();
        let revision = |key: &str, payload: Object| {
            json!({"schema":"tig.asset-revision.experimental-1","asset":"example/asset",
            "revision":key,"parents":[],"representations":[{"key":"main","object":payload}],
            "provenance":{"kind":"imported","source":object("retained source")}})
        };
        let a = store.put(revision("run-a", object("A")));
        let b = store.put(revision("run-b", object("B")));
        let revisions = store.put(
            json!({"schema":"tig.revision-history.experimental-1","asset":"example/asset",
            "coverage":"complete","parents":[],"revisions":[a,b]}),
        );
        let targets = ["review", "public"].map(|key| json!({
            "key":key,"identity":target_identity(key),"destination":object(&format!("destination-{key}")),
            "representation":"main","profile":object("profile"),"gates":object("gate policy")
        }));
        let policy =
            store.put(json!({"schema":"tig.delivery-policy.experimental-1","targets":targets}));
        let mapping = store.put(json!({"schema":"tig.receipt-mapping.experimental-1",
            "limits":{"bytes":1048576,"nodes":8192,"depth":32},
            "rules":[{"kind":"array-rows","root":"/rows","fields":{"sha256":"/sha256","bytes":"/bytes","target":"/path"}}]}));
        let record = json!({"schema":"tig.instance.design-1","id":"example/asset","definition":definition_pin,
            "authored":{},"selected":object("B"),"status":{"review":"unclassified"},
            "streams":[{"slot":"revisions","role":"history/revisions","group":"retained","object":revisions},
                {"slot":"selected-revision","role":"revision/selected","group":"retained","object":b}],
            "metadata":{"revisions":{"schema":"tig.revision-state.experimental-1","history_slot":"revisions",
                "selection":{"slot":"selected-revision","representation":"main"}}}});
        Self {
            store,
            record,
            definition,
            schema,
            a,
            b,
            policy,
            mapping,
        }
    }
    fn binding(&mut self, subject: &Object, receipt: &Object, selector: &str) -> Object {
        self.store.put(json!({"schema":"tig.receipt-binding.experimental-1","subject":subject,
            "receipt":receipt,"provider":"synthetic-service","scope":"test-scope","selector":selector,"mapping":self.mapping}))
    }
    fn event(
        &mut self,
        key: &str,
        target: &str,
        revision: &Object,
        stage: &str,
        parents: &[Object],
    ) -> Value {
        let subject: Object = serde_json::from_value(
            self.store.value(revision)["representations"][0]["object"].clone(),
        )
        .unwrap();
        let evidence = match stage {
            "acknowledged" | "readback" => {
                let receipt = self.store.put(
                    json!({"rows":[{"sha256":subject.sha256,"bytes":subject.bytes,
                    "path":format!("{target}/file.bin")}],"external_decimal":0.125}),
                );
                let binding = self.binding(&subject, &receipt, "/rows/0");
                json!({"kind":"receipt","binding":binding})
            }
            "absent" => json!({"kind":"observation","object":object("declared target absence")}),
            _ => Value::Null,
        };
        json!({"schema":"tig.delivery-event.experimental-1","asset":"example/asset","event":key,"operation":format!("op-{key}"),
            "parents":parents,"policy":self.policy,"target":target,"revision":revision,"representation":"main",
            "subject":subject,"profile":object("profile"),"stage":stage,"evidence":evidence})
    }
    fn page(&mut self, events: &[Object], parents: &[Object], coverage: &str) -> Object {
        self.store.put(
            json!({"schema":"tig.delivery-history.experimental-1","asset":"example/asset",
            "coverage":coverage,"parents":parents,"events":events}),
        )
    }
    fn attach(&mut self, history: Option<&Object>) {
        self.record["streams"].as_array_mut().unwrap().retain(|s| {
            !matches!(
                s["slot"].as_str(),
                Some("delivery-policy" | "delivery-history")
            )
        });
        self.record["metadata"]["delivery"] =
            json!({"schema":"tig.delivery-state.experimental-1","policy_slot":"delivery-policy"});
        self.record["streams"].as_array_mut().unwrap().push(json!({"slot":"delivery-policy","role":"policy/delivery","group":"evidence","object":self.policy}));
        if let Some(history) = history {
            self.record["metadata"]["delivery"]["history_slot"] = json!("delivery-history");
            self.record["streams"].as_array_mut().unwrap().push(json!({"slot":"delivery-history","role":"history/delivery","group":"evidence","object":history}));
        }
    }
    fn check(&mut self, limits: Limits) -> Result<Option<History>, Error> {
        let parsed = Instance::parse(&raw(&self.record)).unwrap();
        let bound = parsed.bind(&self.definition, &self.schema).unwrap();
        let checked = Schema::compile(bound.definition(), &self.schema, Dialect::Draft202012)
            .unwrap()
            .check(&bound)
            .unwrap();
        delivery::check(&bound, &checked, limits, &mut self.store)
    }
    fn history(&mut self) -> History {
        self.check(Limits::default()).unwrap().unwrap()
    }
    fn error(&mut self) -> Error {
        self.check(Limits::default()).err().unwrap()
    }
    fn add(
        &mut self,
        key: &str,
        target: &str,
        revision: &Object,
        stage: &str,
        parents: &[Object],
    ) -> Object {
        let event = self.event(key, target, revision, stage, parents);
        self.store.put(event)
    }
}
fn assert_subject(history: &History, target: &str, expected: &Object) {
    assert!(
        matches!(history.state(target), Some(State::Claim { subject, .. }) if subject == *expected)
    );
}

#[test]
fn unconfigured_and_configured_untracked_assets_make_no_never_delivered_claim() {
    let mut f = Fixture::new("document");
    assert!(f.check(Limits::default()).unwrap().is_none());
    assert!(f.store.reads.is_empty());
    f.attach(None);
    let h = f.history();
    assert!(matches!(h.state("review"), Some(State::Untracked)));
    assert!(h.coverage().is_none());
    assert_eq!(h.event_count(), 0);
    assert!(
        !h.current_remote_content_verified()
            && !h.complete_semantic_coverage()
            && !h.authorizes_eviction()
    );
}

#[test]
fn image_audio_document_keep_selection_and_independent_target_versions() {
    for kind in ["image", "audio", "document"] {
        let mut f = Fixture::new(kind);
        let a = f.a.clone();
        let b = f.b.clone();
        let review_a = f.add("review-a", "review", &a, "acknowledged", &[]);
        let public_a = f.add("public-a", "public", &a, "acknowledged", &[]);
        let review_b = f.add("review-b", "review", &b, "readback", &[review_a]);
        let root = f.page(&[public_a, review_b], &[], "complete");
        f.attach(Some(&root));
        let before = raw(&f.record);
        let h = f.history();
        assert_subject(&h, "review", &object("B"));
        assert_subject(&h, "public", &object("A"));
        assert_eq!(h.revisions().unwrap().selected_revision(), Some(&b));
        assert_eq!(h.event_count(), 3);
        assert_eq!(before, raw(&f.record));
        assert!(!h.current_remote_content_verified() && !h.authorizes_eviction());
        assert!(
            !f.store.reads.contains_key(&object("A")) && !f.store.reads.contains_key(&object("B"))
        );
    }
}

#[test]
fn failed_or_attempted_descendants_preserve_prior_publication_but_absence_is_a_claim() {
    let mut f = Fixture::new("movie");
    let a = f.a.clone();
    let b = f.b.clone();
    let published = f.add("published", "review", &a, "acknowledged", &[]);
    let attempted = f.add("attempt", "review", &b, "attempted", &[published]);
    let failed = f.add("failed", "review", &b, "failed", &[attempted]);
    let root = f.page(std::slice::from_ref(&failed), &[], "complete");
    f.attach(Some(&root));
    assert_subject(&f.history(), "review", &object("A"));
    let absent = f.add("absent", "review", &b, "absent", &[failed]);
    let next = f.page(&[absent], &[root], "complete");
    f.attach(Some(&next));
    assert!(matches!(
        f.history().state("review"),
        Some(State::Claim {
            stage: Stage::Absent,
            ..
        })
    ));
}

#[test]
fn incomparable_same_target_claims_remain_ambiguous_until_causally_resolved() {
    let mut f = Fixture::new("movie");
    let a = f.a.clone();
    let b = f.b.clone();
    let left = f.add("left", "review", &a, "acknowledged", &[]);
    let right = f.add("right", "review", &b, "acknowledged", &[]);
    let lroot = f.page(std::slice::from_ref(&left), &[], "complete");
    let rroot = f.page(std::slice::from_ref(&right), &[], "complete");
    let union = f.page(&[], &[lroot, rroot], "complete");
    f.attach(Some(&union));
    assert!(
        matches!(f.history().state("review"), Some(State::Ambiguous { events }) if events.len() == 2)
    );
    let resolved = f.add("resolved", "review", &b, "readback", &[left, right]);
    let root = f.page(&[resolved], &[union], "complete");
    f.attach(Some(&root));
    assert_subject(&f.history(), "review", &object("B"));
}

#[test]
fn duplicate_event_identity_and_changed_operation_subject_conflict() {
    for mode in ["event", "operation"] {
        let mut f = Fixture::new("movie");
        let a = f.a.clone();
        let b = f.b.clone();
        let left = f.add("left", "review", &a, "acknowledged", &[]);
        let mut right = f.event("right", "review", &b, "acknowledged", &[]);
        right[mode] = json!(if mode == "event" { "left" } else { "op-left" });
        let right = f.store.put(right);
        let root = f.page(&[left, right], &[], "complete");
        f.attach(Some(&root));
        assert_eq!(f.error(), Error::Conflict);
    }
}

#[test]
fn retries_and_shared_event_references_are_idempotent_but_real_publications_stay_distinct() {
    let mut f = Fixture::new("movie");
    let a = f.a.clone();
    let attempt = f.add("attempt", "review", &a, "attempted", &[]);
    let mut ack = f.event("ack", "review", &a, "acknowledged", &[attempt]);
    ack["operation"] = json!("op-attempt");
    let ack = f.store.put(ack);
    let left = f.page(std::slice::from_ref(&ack), &[], "complete");
    let right = f.page(
        std::slice::from_ref(&ack),
        std::slice::from_ref(&left),
        "complete",
    );
    f.attach(Some(&right));
    assert_eq!(f.history().event_count(), 2);
    assert_eq!(f.store.reads[&ack], 1);
    let real_publish = f.add(
        "different-real-publication",
        "review",
        &a,
        "acknowledged",
        &[ack],
    );
    let root = f.page(&[real_publish], &[right], "complete");
    f.attach(Some(&root));
    assert_eq!(f.history().event_count(), 3);
}

#[test]
fn subject_language_profile_and_evidence_are_not_inferred_or_substituted() {
    for mode in [
        "subject", "language", "profile", "receipt", "target", "stage",
    ] {
        let mut f = Fixture::new("audio");
        let a = f.a.clone();
        let mut event = f.event("delivery", "review", &a, "acknowledged", &[]);
        match mode {
            "subject" => event["subject"] = serde_json::to_value(object("B")).unwrap(),
            "language" => event["representation"] = json!("other-language"),
            "profile" => {
                event["profile"] = serde_json::to_value(object("different profile")).unwrap()
            }
            "receipt" => event["evidence"] = Value::Null,
            "target" => event["target"] = json!("public"),
            _ => event["stage"] = json!("approved-and-safe-to-delete"),
        }
        let event = f.store.put(event);
        let root = f.page(&[event], &[], "complete");
        f.attach(Some(&root));
        let expected = match mode {
            "subject" | "language" | "profile" => Error::Subject,
            "stage" => Error::Metadata(revisions::Error::Schema),
            _ => Error::Evidence,
        };
        assert_eq!(f.error(), expected);
    }
}

#[test]
fn retained_policy_survives_retargeting_without_claiming_the_new_destination_was_published() {
    let mut f = Fixture::new("document");
    let a = f.a.clone();
    let old_event = f.add("old", "review", &a, "acknowledged", &[]);
    let mut policy = f.store.value(&f.policy);
    policy["targets"][0]["identity"] = target_identity("new-place");
    f.policy = f.store.put(policy);
    let root = f.page(std::slice::from_ref(&old_event), &[], "complete");
    f.attach(Some(&root));
    let h = f.history();
    assert_eq!(h.event_count(), 1);
    assert!(matches!(h.state("review"), Some(State::Untracked)));
    let wrong_parent = f.add("new", "review", &a, "attempted", &[old_event]);
    let root = f.page(&[wrong_parent], &[root], "complete");
    f.attach(Some(&root));
    assert_eq!(f.error(), Error::Target);
}

#[test]
fn partial_history_is_unknown_and_missing_or_corrupt_pages_fail_closed() {
    let mut f = Fixture::new("document");
    let a = f.a.clone();
    let event = f.add("ack", "review", &a, "acknowledged", &[]);
    let root = f.page(&[event], &[], "partial");
    f.attach(Some(&root));
    let h = f.history();
    assert!(h.coverage() == Some(Coverage::Partial));
    assert!(matches!(h.state("review"), Some(State::UnknownCoverage)));
    let good = f.store.objects.remove(&root).unwrap();
    assert_eq!(f.error(), Error::Metadata(revisions::Error::Missing));
    f.store
        .objects
        .insert(root.clone(), b"corrupt bytes".to_vec());
    assert_eq!(f.error(), Error::Metadata(revisions::Error::Pin));
    f.store.objects.insert(root, good);
}

#[test]
fn revision_and_delivery_limits_share_one_budget_before_any_extra_fetch() {
    let mut f = Fixture::new("document");
    let a = f.a.clone();
    let event = f.add("ack", "review", &a, "acknowledged", &[]);
    let root = f.page(&[event], &[], "complete");
    f.attach(Some(&root));
    assert_eq!(
        f.check(Limits {
            documents: 3,
            ..Limits::default()
        })
        .err(),
        Some(Error::Metadata(revisions::Error::Budget))
    );
    assert_eq!(f.store.reads.values().sum::<usize>(), 3);
}

#[test]
fn thousand_publications_share_one_receipt_and_keep_git_record_bounded() {
    let mut f = Fixture::new("movie");
    let a = f.a.clone();
    let mut event_parent = None;
    let mut page_parent = None;
    let mut first_size = 0;
    for n in 0..1000 {
        let event = f.add(
            &format!("publication-{n}"),
            "review",
            &a,
            "acknowledged",
            &event_parent.into_iter().collect::<Vec<_>>(),
        );
        let page = f.page(
            std::slice::from_ref(&event),
            &page_parent.into_iter().collect::<Vec<_>>(),
            "complete",
        );
        f.attach(Some(&page));
        if n == 0 {
            first_size = raw(&f.record).len();
        }
        event_parent = Some(event);
        page_parent = Some(page);
    }
    let h = f.history();
    assert_eq!(h.event_count(), 1000);
    assert_eq!(h.receipt_binding_count(), 1);
    assert_eq!(h.receipts().len(), 1);
    assert_eq!(f.store.reads[h.receipts().first().unwrap()], 1);
    assert_eq!(h.metadata().len(), 2006);
    assert!(raw(&f.record).len().abs_diff(first_size) < 16);
    assert_subject(&h, "review", &object("A"));
    assert_eq!(h.revisions().unwrap().selected_revision(), Some(&f.b));
}

#[test]
fn target_key_rename_preserves_exact_endpoint_history_but_duplicate_endpoint_rules_refuse() {
    let mut f = Fixture::new("document");
    let a = f.a.clone();
    let old = f.add("old-key", "review", &a, "acknowledged", &[]);
    let mut policy = f.store.value(&f.policy);
    policy["targets"][0]["key"] = json!("renamed-review");
    f.policy = f.store.put(policy);
    let root = f.page(std::slice::from_ref(&old), &[], "complete");
    f.attach(Some(&root));
    assert_subject(&f.history(), "renamed-review", &object("A"));
    let next = f.add("new-key", "renamed-review", &a, "attempted", &[old]);
    let root = f.page(&[next], &[root], "complete");
    f.attach(Some(&root));
    assert_subject(&f.history(), "renamed-review", &object("A"));
    let mut policy = f.store.value(&f.policy);
    policy["targets"][1]["identity"] = policy["targets"][0]["identity"].clone();
    f.policy = f.store.put(policy);
    f.attach(Some(&root));
    assert_eq!(f.error(), Error::Policy);
}

#[test]
fn operation_cannot_silently_change_its_pinned_gate_policy() {
    let mut f = Fixture::new("document");
    let a = f.a.clone();
    let first = f.add("first", "review", &a, "attempted", &[]);
    let mut policy = f.store.value(&f.policy);
    policy["targets"][0]["gates"] =
        serde_json::to_value(object("new authorization policy")).unwrap();
    f.policy = f.store.put(policy);
    let mut second = f.event("second", "review", &a, "attempted", &[first]);
    second["operation"] = json!("op-first");
    let second = f.store.put(second);
    let root = f.page(&[second], &[], "complete");
    f.attach(Some(&root));
    assert_eq!(f.error(), Error::Conflict);
}

#[test]
fn shared_external_receipt_rows_bind_two_targets_and_wrong_rows_do_not_substitute() {
    let mut f = Fixture::new("document");
    let a = f.a.clone();
    let b = f.b.clone();
    let mut review = f.event("review", "review", &a, "acknowledged", &[]);
    let mut public = f.event("public", "public", &b, "acknowledged", &[]);
    let shared = f.store.put(json!({"rows":[
        {"sha256":object("A").sha256,"bytes":1,"path":"review/file.bin"},
        {"sha256":object("B").sha256,"bytes":1,"path":"public/file.bin"}],"external_fraction":0.125}));
    let review_binding = f.binding(&object("A"), &shared, "/rows/0");
    let public_binding = f.binding(&object("B"), &shared, "/rows/1");
    review["evidence"]["binding"] = serde_json::to_value(&review_binding).unwrap();
    public["evidence"]["binding"] = serde_json::to_value(&public_binding).unwrap();
    let review = f.store.put(review);
    let public = f.store.put(public.clone());
    let root = f.page(&[review, public], &[], "complete");
    f.attach(Some(&root));
    let h = f.history();
    assert_subject(&h, "review", &object("A"));
    assert_subject(&h, "public", &object("B"));
    assert_eq!(h.receipt_binding_count(), 2);
    assert_eq!(h.receipts().len(), 1);
    assert_eq!(f.store.reads[&shared], 1);
    let mut bad_binding = f.store.value(&public_binding);
    bad_binding["selector"] = json!("/rows/0");
    let bad_binding = f.store.put(bad_binding);
    let mut bad = f.event("bad-row", "public", &b, "acknowledged", &[]);
    bad["evidence"]["binding"] = serde_json::to_value(bad_binding).unwrap();
    let bad = f.store.put(bad);
    let root = f.page(&[bad], &[], "complete");
    f.attach(Some(&root));
    assert_eq!(f.error(), Error::Evidence);
}

#[test]
fn malformed_attachment_policies_and_unknown_event_fields_refuse() {
    for mode in ["attachment", "role", "policy", "event", "parent-target"] {
        let mut f = Fixture::new("document");
        let a = f.a.clone();
        let parents = if mode == "parent-target" {
            vec![f.add("other", "public", &a, "acknowledged", &[])]
        } else {
            vec![]
        };
        let mut event = f.event("event", "review", &a, "attempted", &parents);
        if mode == "event" {
            event["timestamp_decides_order"] = json!(true);
        }
        let event = f.store.put(event);
        let root = f.page(&[event], &[], "complete");
        if mode == "policy" {
            let mut p = f.store.value(&f.policy);
            p["targets"][0]["extra"] = json!(true);
            f.policy = f.store.put(p);
        }
        f.attach(Some(&root));
        if mode == "attachment" {
            f.record["metadata"]["delivery"]["unknown"] = json!(true);
        }
        if mode == "role" {
            f.record["streams"][3]["role"] = json!("output/document");
        }
        let expected = match mode {
            "attachment" | "role" => Error::Attachment,
            "parent-target" => Error::Target,
            _ => Error::Metadata(revisions::Error::Schema),
        };
        assert_eq!(f.error(), expected);
    }
}

#[test]
fn available_unregistered_revisions_and_missing_revision_contract_are_not_substituted() {
    for missing_contract in [false, true] {
        let mut f = Fixture::new("movie");
        let a = f.a.clone();
        let mut event = f.event("ack", "review", &a, "acknowledged", &[]);
        if !missing_contract {
            let mut orphan = f.store.value(&a);
            orphan["revision"] = json!("available-but-unregistered");
            event["revision"] = serde_json::to_value(f.store.put(orphan)).unwrap();
        }
        let event = f.store.put(event);
        let root = f.page(&[event], &[], "complete");
        f.attach(Some(&root));
        if missing_contract {
            f.record["metadata"]
                .as_object_mut()
                .unwrap()
                .remove("revisions");
        }
        assert_eq!(f.error(), Error::Subject);
    }
}

#[test]
fn retained_claim_keeps_original_profile_and_missing_receipt_is_not_accepted() {
    let mut f = Fixture::new("movie");
    let a = f.a.clone();
    let event = f.add("ack", "review", &a, "acknowledged", &[]);
    let old_policy = f.policy.clone();
    let mut policy = f.store.value(&f.policy);
    policy["targets"][0]["profile"] =
        serde_json::to_value(object("new target resolution")).unwrap();
    f.policy = f.store.put(policy);
    let root = f.page(&[event], &[], "complete");
    f.attach(Some(&root));
    let h = f.history();
    assert!(
        matches!(h.state("review"), Some(State::Claim { profile, policy, .. }) if profile == object("profile") && policy == old_policy)
    );
    let receipt = h.receipts().first().unwrap().clone();
    f.store.objects.remove(&receipt);
    assert_eq!(f.error(), Error::Metadata(revisions::Error::Missing));
}
