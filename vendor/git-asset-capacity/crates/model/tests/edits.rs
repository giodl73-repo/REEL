#![cfg(feature = "schema")]
use git_asset_core::{Object, identity};
use git_asset_model::{
    delivery::{self, State},
    edits::{self, Change, Error, Inputs, Prepared},
    native::{Instance, Pin},
    revisions::{self, Coverage, Limits, Reader},
    schema::{Dialect, Schema},
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

#[path = "edits/sequence_cases.rs"]
mod sequence_cases;

fn raw(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn payload(label: &str) -> Object {
    Object::from_bytes(label.as_bytes())
}
fn pin(object: &Object) -> Pin {
    serde_json::from_value(serde_json::to_value(object).unwrap()).unwrap()
}
fn status(value: &str) -> Map<String, Value> {
    json!({"review":value}).as_object().unwrap().clone()
}
#[derive(Default)]
struct Store {
    objects: BTreeMap<Object, Vec<u8>>,
    calls: BTreeMap<Object, usize>,
}
impl Reader for Store {
    fn read(&mut self, expected: &Object, maximum: usize) -> Result<Vec<u8>, revisions::Error> {
        assert!(expected.bytes <= maximum as u64);
        *self.calls.entry(expected.clone()).or_default() += 1;
        self.objects
            .get(expected)
            .cloned()
            .ok_or(revisions::Error::Missing)
    }
}
impl Store {
    fn put(&mut self, value: Value) -> Object {
        let bytes = raw(&value);
        let object = Object::from_bytes(&bytes);
        self.objects.insert(object.clone(), bytes);
        object
    }
}
struct Fixture {
    record: Vec<u8>,
    definition: Vec<u8>,
    schema: Vec<u8>,
    store: Store,
}
impl Fixture {
    fn new(kind: &str) -> Self {
        let schema = raw(
            &json!({"type":"object","properties":{"prompt":{"type":"string"},"rights":{"type":"string"}},"required":["prompt","rights"],"additionalProperties":false}),
        );
        let definition = raw(
            &json!({"schema":"tig.definition.design-1","id":format!("example/{kind}"),"authored_schema":Object::from_bytes(&schema),
            "authoring":{"properties":["prompt","rights"],"required":["prompt","rights"],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
        );
        let mut definition_pin = serde_json::to_value(Object::from_bytes(&definition)).unwrap();
        definition_pin["id"] = json!(format!("example/{kind}"));
        let record = serde_json::to_vec_pretty(&json!({"schema":"tig.instance.design-1","id":"asset/one","definition":definition_pin,
            "authored":{"prompt":"Conservar español\nsin cambios","rights":"private unchanged scope"},"selected":payload("A"),
            "streams":[{"slot":"current","role":"output/document","group":"active","object":payload("A")},
                {"slot":"original","role":"output/document","group":"retained","object":payload("A")}],
            "status":{"review":"accepted-project-value"},"metadata":{"creator":"private creator","notes":{"keep":"unchanged"}}})).unwrap();
        Self {
            record,
            definition,
            schema,
            store: Store::default(),
        }
    }
    fn value(&self) -> Value {
        serde_json::from_slice(&self.record).unwrap()
    }
    fn prepare(&mut self, change: Change<'_>) -> Result<Prepared, Error> {
        self.limited(change, Limits::default())
    }
    fn limited(&mut self, change: Change<'_>, limits: Limits) -> Result<Prepared, Error> {
        let expected = Object::from_bytes(&self.record);
        edits::prepare(
            Inputs {
                expected: &expected,
                instance: &self.record,
                definition: &self.definition,
                authored_schema: &self.schema,
                dialect: Dialect::Draft202012,
                limits,
            },
            change,
            &mut self.store,
        )
    }
    fn retain(&mut self, prepared: Prepared) {
        assert!(
            !prepared.retained()
                && !prepared.authorizes_eviction()
                && !prepared.complete_semantic_coverage()
        );
        assert_eq!(prepared.expected(), &Object::from_bytes(&self.record));
        for blob in prepared.metadata() {
            assert_eq!(blob.object(), &Object::from_bytes(blob.bytes()));
            self.store
                .objects
                .insert(blob.object().clone(), blob.bytes().to_vec());
        }
        self.record = prepared.record().bytes().to_vec();
    }
    fn manifest(key: &str, object: &Object, parents: &[Object]) -> Vec<u8> {
        raw(
            &json!({"schema":"tig.asset-revision.experimental-1","asset":"asset/one","revision":key,"parents":parents,
            "representations":[{"key":"main","object":object}],"provenance":{"kind":"imported","source":payload("old source witness")}}),
        )
    }
    fn register(&mut self, key: &str, object: &Object, parents: &[Object]) -> Object {
        let bytes = Self::manifest(key, object, parents);
        let object = Object::from_bytes(&bytes);
        let prepared = self
            .prepare(Change::RegisterRevision {
                manifest: &bytes,
                coverage: Coverage::Complete,
            })
            .unwrap();
        self.retain(prepared);
        object
    }
    fn select(&mut self, revision: &Object, subject: &Object, project_status: &str) {
        let subject = pin(subject);
        let status = status(project_status);
        let prepared = self
            .prepare(Change::SelectRevision {
                revision,
                representation: "main",
                subject: &subject,
                status: &status,
                streams: &["current"],
            })
            .unwrap();
        assert!(prepared.metadata().is_empty());
        self.retain(prepared);
    }
    fn root(&self, capability: &str) -> Object {
        let value = self.value();
        let slot = value["metadata"][capability]["history_slot"]
            .as_str()
            .unwrap();
        let stream = value["streams"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["slot"] == slot)
            .unwrap();
        let pin: Pin = serde_json::from_value(stream["object"].clone()).unwrap();
        pin.object()
    }
    fn policy() -> Vec<u8> {
        let target = Object::from_bytes(
            &identity::canonical_json(
                &json!({"schema":"tig.declared-publication-target.experimental-1",
            "provider":"synthetic","scope":"test","path":"review/item.bin"}),
            )
            .unwrap(),
        )
        .sha256;
        raw(
            &json!({"schema":"tig.delivery-policy.experimental-1","targets":[{"key":"review","identity":target,
            "destination":payload("opaque destination"),"representation":"main","profile":payload("profile"),"gates":payload("gates")}]}),
        )
    }
    fn event(
        &mut self,
        key: &str,
        revision: &Object,
        subject: &Object,
        policy: &Object,
        parents: &[Object],
    ) -> Vec<u8> {
        let mapping = self.store.put(json!({"schema":"tig.receipt-mapping.experimental-1","limits":{"bytes":1048576,"nodes":8192,"depth":32},
            "rules":[{"kind":"array-rows","root":"/rows","fields":{"sha256":"/sha256","bytes":"/bytes","target":"/path"}}]}));
        let receipt = self.store.put(json!({"rows":[{"sha256":subject.sha256,"bytes":subject.bytes,"path":"review/item.bin"}]}));
        let binding = self.store.put(json!({"schema":"tig.receipt-binding.experimental-1","subject":subject,"receipt":receipt,"provider":"synthetic",
            "scope":"test","selector":"/rows/0","mapping":mapping}));
        raw(
            &json!({"schema":"tig.delivery-event.experimental-1","asset":"asset/one","event":key,"operation":format!("op-{key}"),
            "parents":parents,"policy":policy,"target":"review","revision":revision,"representation":"main","subject":subject,"profile":payload("profile"),
            "stage":"acknowledged","evidence":{"kind":"receipt","binding":binding}}),
        )
    }
}

#[test]
fn draft_registration_preserves_current_subject_status_authoring_and_every_old_version() {
    for kind in ["image", "document"] {
        let mut f = Fixture::new(kind);
        let original = f.value();
        let a = f.register("run-a", &payload("A"), &[]);
        f.select(&a, &payload("A"), "accepted-project-value");
        let old_root = f.root("revisions");
        let old_objects = f.store.objects.clone();
        f.store.calls.clear();
        let bytes = Fixture::manifest("run-b", &payload("B"), std::slice::from_ref(&a));
        let prepared = f
            .prepare(Change::RegisterRevision {
                manifest: &bytes,
                coverage: Coverage::Complete,
            })
            .unwrap();
        assert_eq!(prepared.metadata().len(), 2);
        assert!(
            prepared
                .metadata()
                .iter()
                .map(|b| b.bytes().len())
                .sum::<usize>()
                < 2048
        );
        let next: Value = serde_json::from_slice(prepared.record().bytes()).unwrap();
        assert_eq!(next["authored"], original["authored"]);
        assert_eq!(next["status"], original["status"]);
        assert_eq!(next["selected"], original["selected"]);
        assert_eq!(next["metadata"]["creator"], original["metadata"]["creator"]);
        assert!(f.store.calls.values().all(|n| *n == 1));
        f.retain(prepared);
        let page: Value = serde_json::from_slice(&f.store.objects[&f.root("revisions")]).unwrap();
        assert_eq!(page["parents"][0], serde_json::to_value(old_root).unwrap());
        for (object, bytes) in old_objects {
            assert_eq!(f.store.objects[&object], bytes);
        }
    }
}

#[test]
fn explicit_selection_updates_only_requested_current_binding_and_keeps_previous_alias() {
    let mut f = Fixture::new("image");
    let a = f.register("a", &payload("A"), &[]);
    f.select(&a, &payload("A"), "accepted-project-value");
    let b = f.register("b", &payload("B"), &[a]);
    f.select(&b, &payload("B"), "draft-project-value");
    let record = f.value();
    assert_eq!(
        record["selected"],
        serde_json::to_value(payload("B")).unwrap()
    );
    assert_eq!(
        record["streams"][0]["object"],
        serde_json::to_value(payload("B")).unwrap()
    );
    assert_eq!(
        record["streams"][1]["object"],
        serde_json::to_value(payload("A")).unwrap()
    );
    assert_eq!(record["status"]["review"], "draft-project-value");
}

#[test]
fn unclassified_previous_selection_cannot_be_replaced_without_recording_it_first() {
    let mut f = Fixture::new("document");
    let b = f.register("b", &payload("B"), &[]);
    let subject = pin(&payload("B"));
    let desired = status("draft");
    assert!(matches!(
        f.prepare(Change::SelectRevision {
            revision: &b,
            representation: "main",
            subject: &subject,
            status: &desired,
            streams: &[]
        }),
        Err(Error::Selection)
    ));
    assert_eq!(
        f.value()["selected"],
        serde_json::to_value(payload("A")).unwrap()
    );
    let a = f.register("old-import", &payload("A"), &[]);
    f.select(&a, &payload("A"), "old-subject-kept");
    f.select(&b, &payload("B"), "explicit-new-decision");
}

#[test]
fn delivery_append_preserves_selection_and_both_target_versions_over_time() {
    let mut f = Fixture::new("movie");
    let a = f.register("a", &payload("A"), &[]);
    f.select(&a, &payload("A"), "accepted");
    let b = f.register("b", &payload("B"), std::slice::from_ref(&a));
    f.select(&b, &payload("B"), "draft");
    let policy = Fixture::policy();
    let policy_pin = Object::from_bytes(&policy);
    let prepared = f
        .prepare(Change::ConfigureDelivery { policy: &policy })
        .unwrap();
    assert_eq!(prepared.metadata().len(), 1);
    f.retain(prepared);
    let event_a = f.event("publish-a", &a, &payload("A"), &policy_pin, &[]);
    let event_a_pin = Object::from_bytes(&event_a);
    let prepared = f
        .prepare(Change::RecordDelivery {
            event: &event_a,
            coverage: Coverage::Complete,
        })
        .unwrap();
    assert_eq!(prepared.metadata().len(), 2);
    f.retain(prepared);
    let old_delivery_root = f.root("delivery");
    let event_b = f.event("publish-b", &b, &payload("B"), &policy_pin, &[event_a_pin]);
    let prepared = f
        .prepare(Change::RecordDelivery {
            event: &event_b,
            coverage: Coverage::Complete,
        })
        .unwrap();
    f.retain(prepared);
    let page: Value = serde_json::from_slice(&f.store.objects[&f.root("delivery")]).unwrap();
    assert_eq!(
        page["parents"][0],
        serde_json::to_value(old_delivery_root).unwrap()
    );
    let parsed = Instance::parse(&f.record).unwrap();
    let bound = parsed.bind(&f.definition, &f.schema).unwrap();
    let checked = Schema::compile(bound.definition(), &f.schema, Dialect::Draft202012)
        .unwrap()
        .check(&bound)
        .unwrap();
    let h = delivery::check(&bound, &checked, Limits::default(), &mut f.store)
        .unwrap()
        .unwrap();
    assert_eq!(h.event_count(), 2);
    assert!(
        matches!(h.state("review"), Some(State::Claim { subject, .. }) if subject == payload("B"))
    );
    assert_eq!(
        h.revisions().unwrap().representation("a", "main"),
        Some(&payload("A"))
    );
    assert_eq!(f.value()["status"]["review"], "draft");
}

#[test]
fn deterministic_retry_exact_fact_repeat_and_selection_repeat_do_not_add_history() {
    let mut f = Fixture::new("document");
    let manifest = Fixture::manifest("a", &payload("A"), &[]);
    let one = f
        .prepare(Change::RegisterRevision {
            manifest: &manifest,
            coverage: Coverage::Complete,
        })
        .unwrap();
    let two = f
        .prepare(Change::RegisterRevision {
            manifest: &manifest,
            coverage: Coverage::Complete,
        })
        .unwrap();
    assert_eq!(one.record().bytes(), two.record().bytes());
    assert_eq!(
        one.metadata()
            .iter()
            .map(|b| b.object())
            .collect::<Vec<_>>(),
        two.metadata()
            .iter()
            .map(|b| b.object())
            .collect::<Vec<_>>()
    );
    f.retain(one);
    let mut pretty = serde_json::to_vec_pretty(&f.value()).unwrap();
    pretty.extend_from_slice(b"\n\n");
    f.record = pretty;
    let before = f.record.clone();
    let repeat = f
        .prepare(Change::RegisterRevision {
            manifest: &manifest,
            coverage: Coverage::Complete,
        })
        .unwrap();
    assert!(repeat.is_noop() && repeat.metadata().is_empty());
    assert_eq!(repeat.record().bytes(), before);
    let revision = Object::from_bytes(&manifest);
    f.select(&revision, &payload("A"), "explicit");
    let subject = pin(&payload("A"));
    let desired = status("explicit");
    let repeat = f
        .prepare(Change::SelectRevision {
            revision: &revision,
            representation: "main",
            subject: &subject,
            status: &desired,
            streams: &["current"],
        })
        .unwrap();
    assert!(repeat.is_noop() && repeat.metadata().is_empty());
}

#[test]
fn stale_record_refuses_before_any_history_io_or_request_interpretation() {
    let mut f = Fixture::new("document");
    f.register("a", &payload("A"), &[]);
    f.store.calls.clear();
    let wrong = payload("wrong expected record");
    let manifest = Fixture::manifest("b", &payload("B"), &[]);
    let result = edits::prepare(
        Inputs {
            expected: &wrong,
            instance: &f.record,
            definition: &f.definition,
            authored_schema: &f.schema,
            dialect: Dialect::Draft202012,
            limits: Limits::default(),
        },
        Change::RegisterRevision {
            manifest: &manifest,
            coverage: Coverage::Complete,
        },
        &mut f.store,
    );
    assert!(matches!(result, Err(Error::Stale)));
    assert!(f.store.calls.is_empty());
}

#[test]
fn unowned_reserved_slots_are_never_overwritten_even_when_role_matches() {
    let mut f = Fixture::new("image");
    let mut record = f.value();
    record["streams"].as_array_mut().unwrap().push(json!({"slot":"tig-revision-history","role":"history/revisions","group":"unowned","object":payload("unrelated")}));
    f.record = raw(&record);
    let before = f.record.clone();
    let manifest = Fixture::manifest("a", &payload("A"), &[]);
    assert!(matches!(
        f.prepare(Change::RegisterRevision {
            manifest: &manifest,
            coverage: Coverage::Complete
        }),
        Err(Error::Conflict)
    ));
    assert_eq!(f.record, before);
    assert!(f.store.objects.is_empty());
}

#[test]
fn changed_run_reuse_invalid_subject_and_managed_stream_selection_refuse() {
    let mut f = Fixture::new("image");
    let a = f.register("a", &payload("A"), &[]);
    f.select(&a, &payload("A"), "kept");
    let collision = Fixture::manifest("a", &payload("B"), &[]);
    assert!(matches!(
        f.prepare(Change::RegisterRevision {
            manifest: &collision,
            coverage: Coverage::Complete
        }),
        Err(Error::Revision(revisions::Error::Conflict))
    ));
    let wrong = pin(&payload("wrong"));
    let desired = status("explicit");
    assert!(matches!(
        f.prepare(Change::SelectRevision {
            revision: &a,
            representation: "main",
            subject: &wrong,
            status: &desired,
            streams: &[]
        }),
        Err(Error::Selection)
    ));
    let subject = pin(&payload("A"));
    assert!(matches!(
        f.prepare(Change::SelectRevision {
            revision: &a,
            representation: "main",
            subject: &subject,
            status: &desired,
            streams: &["tig-revision-history"]
        }),
        Err(Error::Selection)
    ));
}

#[test]
fn history_pin_annotations_remain_and_new_metadata_shares_one_operation_budget() {
    let mut f = Fixture::new("image");
    let a = f.register("a", &payload("A"), &[]);
    let mut record = f.value();
    let stream = record["streams"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["slot"] == "tig-revision-history")
        .unwrap();
    stream["object"]["id"] = json!("stable/history-root");
    stream["object"]["media_type"] = json!("application/json");
    f.record = raw(&record);
    let manifest = Fixture::manifest("b", &payload("B"), &[a]);
    f.store.calls.clear();
    let objects = f.store.objects.clone();
    assert!(matches!(
        f.limited(
            Change::RegisterRevision {
                manifest: &manifest,
                coverage: Coverage::Complete
            },
            Limits {
                documents: 3,
                ..Limits::default()
            }
        ),
        Err(Error::Revision(revisions::Error::Budget))
    ));
    assert_eq!(f.store.objects, objects);
    assert!(f.store.calls.values().all(|n| *n == 1));
    let prepared = f
        .prepare(Change::RegisterRevision {
            manifest: &manifest,
            coverage: Coverage::Complete,
        })
        .unwrap();
    f.retain(prepared);
    let record = f.value();
    let stream = record["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slot"] == "tig-revision-history")
        .unwrap();
    assert_eq!(stream["object"]["id"], "stable/history-root");
    assert_eq!(stream["object"]["media_type"], "application/json");
}

#[test]
fn oversized_requests_and_project_status_refuse_before_reading_history() {
    let mut f = Fixture::new("image");
    let a = f.register("a", &payload("A"), &[]);
    f.store.calls.clear();
    let oversized = vec![b' '; 65_537];
    assert!(matches!(
        f.prepare(Change::RegisterRevision {
            manifest: &oversized,
            coverage: Coverage::Complete
        }),
        Err(Error::Budget)
    ));
    let subject = pin(&payload("A"));
    let bad_status = json!({"items":vec![true;4097]})
        .as_object()
        .unwrap()
        .clone();
    assert!(matches!(
        f.prepare(Change::SelectRevision {
            revision: &a,
            representation: "main",
            subject: &subject,
            status: &bad_status,
            streams: &[]
        }),
        Err(Error::Budget)
    ));
    assert!(f.store.calls.is_empty());
}
