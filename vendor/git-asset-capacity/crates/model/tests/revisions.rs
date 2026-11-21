#![cfg(feature = "schema")]
use git_asset_core::Object;
use git_asset_model::{
    native::Instance,
    revisions::{self, Coverage, Error, History, Limits, Reader},
    schema::{Dialect, Schema},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn raw(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
#[derive(Default)]
struct Store {
    objects: BTreeMap<Object, Vec<u8>>,
    reads: usize,
}
impl Reader for Store {
    fn read(&mut self, expected: &Object, maximum: usize) -> Result<Vec<u8>, Error> {
        assert_eq!(expected.bytes as usize, maximum);
        assert!(maximum <= 65_536);
        self.reads += 1;
        self.objects.get(expected).cloned().ok_or(Error::Missing)
    }
}
impl Store {
    fn put(&mut self, value: Value) -> Object {
        let raw = raw(&value);
        let object = Object::from_bytes(&raw);
        self.objects.insert(object.clone(), raw);
        object
    }
}
struct Fixture {
    record: Value,
    definition: Vec<u8>,
    schema: Vec<u8>,
    store: Store,
}
impl Fixture {
    fn new(kind: &str) -> Self {
        let schema = raw(&json!({"type":"object","properties":{},"additionalProperties":false}));
        let definition = raw(
            &json!({"schema":"tig.definition.design-1","id":format!("example/{kind}"),
            "authored_schema":Object::from_bytes(&schema),"authoring":{"properties":[],"required":[],"defaults":{}},
            "stream_rules":{},"semantic_rules":{}}),
        );
        let mut pin = serde_json::to_value(Object::from_bytes(&definition)).unwrap();
        pin["id"] = json!(format!("example/{kind}"));
        Self {
            record: json!({"schema":"tig.instance.design-1","id":"example/asset","definition":pin,
                "authored":{},"streams":[],"selected":null,"status":{"review":"draft"},"metadata":{}}),
            definition,
            schema,
            store: Store::default(),
        }
    }
    fn revision(&mut self, run: &str, payload: &Object, parents: &[Object]) -> Object {
        self.store.put(json!({"schema":"tig.asset-revision.experimental-1","asset":"example/asset",
            "revision":run,"parents":parents,"representations":[{"key":"main","object":payload}],
            "provenance":{"kind":"imported","source":Object::from_bytes(b"retained source witness")}}))
    }
    fn page(&mut self, entries: &[Object], parents: &[Object], coverage: &str) -> Object {
        self.store.put(
            json!({"schema":"tig.revision-history.experimental-1","asset":"example/asset",
            "coverage":coverage,"parents":parents,"revisions":entries}),
        )
    }
    fn attach(&mut self, root: &Object, selection: Option<(&Object, &Object)>) {
        self.record["metadata"]["revisions"] =
            json!({"schema":"tig.revision-state.experimental-1","history_slot":"history"});
        self.record["streams"] =
            json!([{"slot":"history","role":"history/revisions","group":"retained","object":root}]);
        if let Some((revision, payload)) = selection {
            self.record["metadata"]["revisions"]["selection"] =
                json!({"slot":"revision","representation":"main"});
            self.record["streams"].as_array_mut().unwrap().push(json!({"slot":"revision","role":"revision/selected","group":"retained","object":revision}));
            self.record["selected"] = serde_json::to_value(payload).unwrap();
        }
    }
    fn check(&mut self, limits: Limits) -> Result<Option<History>, Error> {
        let parsed = Instance::parse(&raw(&self.record)).unwrap();
        let bound = parsed.bind(&self.definition, &self.schema).unwrap();
        let checked = Schema::compile(bound.definition(), &self.schema, Dialect::Draft202012)
            .unwrap()
            .check(&bound)
            .unwrap();
        revisions::check(&bound, &checked, limits, &mut self.store)
    }
    fn history(&mut self) -> History {
        self.check(Limits::default()).unwrap().unwrap()
    }
    fn error(&mut self) -> Error {
        self.check(Limits::default()).err().unwrap()
    }
}

#[test]
fn optional_history_keeps_legacy_and_planned_records_valid_without_io() {
    let mut f = Fixture::new("document");
    assert!(f.check(Limits::default()).unwrap().is_none());
    f.record["selected"] = serde_json::to_value(Object::from_bytes(b"legacy bytes")).unwrap();
    assert!(f.check(Limits::default()).unwrap().is_none());
    assert_eq!(f.store.reads, 0);
}

#[test]
fn same_common_contract_serves_image_audio_and_document_without_payload_reads() {
    for kind in ["image", "audio", "document"] {
        let mut f = Fixture::new(kind);
        let payload = Object::from_bytes(kind.as_bytes());
        let revision = f.revision("run-1", &payload, &[]);
        let root = f.page(std::slice::from_ref(&revision), &[], "complete");
        f.attach(&root, Some((&revision, &payload)));
        let before = raw(&f.record);
        let h = f.history();
        assert!(h.coverage() == Coverage::Complete);
        assert_eq!(h.selected_revision(), Some(&revision));
        assert_eq!(h.representation("run-1", "main"), Some(&payload));
        assert_eq!(h.documents().len(), 2);
        assert_eq!(h.payloads().len(), 1);
        assert_eq!(h.provenance().len(), 1);
        assert!(!h.complete_semantic_coverage() && !h.authorizes_eviction());
        assert_eq!(f.store.reads, 2);
        assert_eq!(before, raw(&f.record));
    }
}

#[test]
fn distinct_same_byte_runs_share_payload_and_keep_both_revision_identities() {
    let mut f = Fixture::new("movie");
    let payload = Object::from_bytes(b"identical render bytes");
    let a = f.revision("run-a", &payload, &[]);
    let b = f.revision("run-b", &payload, std::slice::from_ref(&a));
    assert_ne!(a, b);
    let root = f.page(std::slice::from_ref(&b), &[], "complete");
    f.attach(&root, Some((&a, &payload)));
    let h = f.history();
    assert_eq!(h.revision_count(), 2);
    assert_eq!(h.payloads().len(), 1);
    assert_eq!(h.selected_revision(), Some(&a));
    assert_eq!(h.representation("run-b", "main"), Some(&payload));
}

#[test]
fn draft_and_receipt_annotation_do_not_advance_selection_or_revision_history() {
    let mut f = Fixture::new("movie");
    let a_bytes = Object::from_bytes(b"A");
    let b_bytes = Object::from_bytes(b"B");
    let a = f.revision("a", &a_bytes, &[]);
    let b = f.revision("b", &b_bytes, std::slice::from_ref(&a));
    let root = f.page(&[a.clone(), b.clone()], &[], "complete");
    f.attach(&root, Some((&a, &a_bytes)));
    let h = f.history();
    let documents = h.documents().clone();
    f.record["metadata"]["project_delivery_note"] =
        json!("retained observation, not rendering input");
    f.record["status"]["review"] = json!("project-specific-value");
    let after = f.history();
    assert_eq!(after.documents(), &documents);
    assert_eq!(after.selected_revision(), Some(&a));
    assert_eq!(after.representation("b", "main"), Some(&b_bytes));
}

#[test]
fn branch_union_shares_pages_and_revisions_and_preserves_partial_coverage() {
    let mut f = Fixture::new("document");
    let payload = Object::from_bytes(b"shared");
    let a = f.revision("a", &payload, &[]);
    let base = f.page(std::slice::from_ref(&a), &[], "partial");
    let b = f.revision("b", &payload, std::slice::from_ref(&a));
    let c = f.revision("c", &payload, std::slice::from_ref(&a));
    let left = f.page(&[a.clone(), b], std::slice::from_ref(&base), "complete");
    let right = f.page(&[a, c], &[base], "complete");
    let union = f.page(&[], &[left, right], "complete");
    f.attach(&union, None);
    let h = f.history();
    assert!(h.coverage() == Coverage::Partial);
    assert_eq!(h.revision_count(), 3);
    assert_eq!(f.store.reads, 7); // four pages, three manifests, not per-edge reads
}

#[test]
fn thousand_versions_append_without_rewriting_old_documents_or_growing_record() {
    let mut f = Fixture::new("document");
    let payload = Object::from_bytes(b"same payload across 1000 runs");
    let mut page = None;
    let mut revision = None;
    let mut first = None;
    let mut initial_size = 0;
    for n in 0..1000 {
        let r = f.revision(
            &format!("run-{n}"),
            &payload,
            &revision.into_iter().collect::<Vec<_>>(),
        );
        let p = f.page(
            std::slice::from_ref(&r),
            &page.into_iter().collect::<Vec<_>>(),
            "complete",
        );
        f.attach(&p, Some((&r, &payload)));
        if n == 0 {
            first = Some((p.clone(), f.store.objects[&p].clone()));
            initial_size = raw(&f.record).len();
        }
        page = Some(p);
        revision = Some(r);
    }
    let h = f.history();
    assert_eq!(h.revision_count(), 1000);
    assert_eq!(h.documents().len(), 2000);
    assert_eq!(h.payloads().len(), 1);
    assert_eq!(f.store.reads, 2000);
    assert!(raw(&f.record).len().abs_diff(initial_size) < 16);
    let (pin, original) = first.unwrap();
    assert_eq!(f.store.objects[&pin], original);
}

#[test]
fn selected_manifest_and_representation_must_belong_to_exact_history_and_bytes() {
    for mode in [
        "foreign-manifest",
        "wrong-representation",
        "wrong-payload",
        "no-payload",
    ] {
        let mut f = Fixture::new("audio");
        let payload = Object::from_bytes(b"audio");
        let a = f.revision("a", &payload, &[]);
        let other = f.revision("other", &payload, &[]);
        let root = f.page(std::slice::from_ref(&a), &[], "complete");
        f.attach(&root, Some((&a, &payload)));
        match mode {
            "foreign-manifest" => {
                f.record["streams"][1]["object"] = serde_json::to_value(other).unwrap()
            }
            "wrong-representation" => {
                f.record["metadata"]["revisions"]["selection"]["representation"] = json!("es")
            }
            "wrong-payload" => {
                f.record["selected"] =
                    serde_json::to_value(Object::from_bytes(b"different")).unwrap()
            }
            _ => f.record["selected"] = Value::Null,
        }
        assert_eq!(f.error(), Error::Selection);
    }
}

#[test]
fn duplicate_run_key_is_conflict_even_for_same_payload_not_last_writer_wins() {
    let mut f = Fixture::new("image");
    let payload = Object::from_bytes(b"image");
    let a = f.revision("same-run", &payload, &[]);
    let b = f.revision("same-run", &payload, std::slice::from_ref(&a));
    let root = f.page(&[a, b], &[], "complete");
    f.attach(&root, None);
    assert_eq!(f.error(), Error::Conflict);
}

#[test]
fn missing_corrupt_and_foreign_metadata_never_become_empty_history() {
    for mode in [
        "missing",
        "corrupt",
        "foreign",
        "unknown",
        "duplicate",
        "float",
    ] {
        let mut f = Fixture::new("document");
        let root = f.page(&[], &[], "complete");
        f.attach(&root, None);
        match mode {
            "missing" => {
                f.store.objects.clear();
            }
            "corrupt" => {
                f.store.objects.insert(root, b"wrong bytes".to_vec());
            }
            _ => {
                let mut value: Value = serde_json::from_slice(&f.store.objects[&root]).unwrap();
                match mode {
                    "foreign" => value["asset"] = json!("other/asset"),
                    "unknown" => value["extra"] = json!(true),
                    "float" => value["extra"] = json!(0.5),
                    _ => (),
                }
                let pin = if mode == "duplicate" {
                    let bytes = br#"{"schema":"tig.revision-history.experimental-1","asset":"example/asset","asset":"example/asset","coverage":"complete","parents":[],"revisions":[]}"#.to_vec();
                    let pin = Object::from_bytes(&bytes);
                    f.store.objects.insert(pin.clone(), bytes);
                    pin
                } else {
                    f.store.put(value)
                };
                f.attach(&pin, None);
            }
        }
        let error = f.error();
        assert_eq!(
            error,
            match mode {
                "missing" => Error::Missing,
                "corrupt" => Error::Pin,
                "foreign" => Error::Identity,
                _ => Error::Schema,
            }
        );
        assert!(!error.to_string().contains("example/asset"));
    }
}

#[test]
fn limits_refuse_before_reader_and_cannot_be_broadened() {
    let mut f = Fixture::new("image");
    let root = f.page(&[], &[], "complete");
    f.attach(&root, None);
    for limits in [
        Limits {
            documents: 4097,
            ..Limits::default()
        },
        Limits {
            bytes: 1,
            ..Limits::default()
        },
        Limits {
            references: 1,
            ..Limits::default()
        },
    ] {
        assert_eq!(f.check(limits).err(), Some(Error::Budget));
        assert_eq!(f.store.reads, 0);
    }
    f.record["streams"][0]["object"]["bytes"] = json!(65_537);
    assert_eq!(f.error(), Error::Budget);
    assert_eq!(f.store.reads, 0);
}

#[test]
fn checked_schema_cannot_be_reused_for_another_record_revision() {
    let mut f = Fixture::new("document");
    let original = Instance::parse(&raw(&f.record))
        .unwrap()
        .bind(&f.definition, &f.schema)
        .unwrap();
    let checked = Schema::compile(original.definition(), &f.schema, Dialect::Draft202012)
        .unwrap()
        .check(&original)
        .unwrap();
    f.record["status"]["review"] = json!("changed");
    let changed = Instance::parse(&raw(&f.record))
        .unwrap()
        .bind(&f.definition, &f.schema)
        .unwrap();
    assert_eq!(
        revisions::check(&changed, &checked, Limits::default(), &mut f.store).err(),
        Some(Error::Schema)
    );
    assert_eq!(f.store.reads, 0);
}

#[test]
fn wrong_attachment_roles_versions_and_unknown_fields_refuse_before_io() {
    for mode in ["role", "schema", "extra", "slot"] {
        let mut f = Fixture::new("document");
        let root = f.page(&[], &[], "complete");
        f.attach(&root, None);
        match mode {
            "role" => f.record["streams"][0]["role"] = json!("output/document"),
            "schema" => f.record["metadata"]["revisions"]["schema"] = json!("future"),
            "extra" => f.record["metadata"]["revisions"]["extra"] = json!(true),
            _ => f.record["metadata"]["revisions"]["history_slot"] = json!("absent"),
        }
        assert_eq!(f.error(), Error::Attachment);
        assert_eq!(f.store.reads, 0);
    }
}

#[test]
fn produced_witnesses_are_retained_references_not_another_production_proof() {
    let mut f = Fixture::new("image");
    let payload = Object::from_bytes(b"image bytes");
    let claim = Object::from_bytes(b"existing produced-claim contract bytes");
    let context = Object::from_bytes(b"existing archived input context");
    let revision = f.store.put(
        json!({"schema":"tig.asset-revision.experimental-1","asset":"example/asset",
        "revision":"produced-run","parents":[],"representations":[{"key":"main","object":payload}],
        "provenance":{"kind":"produced","claim":claim,"context":context}}),
    );
    let root = f.page(std::slice::from_ref(&revision), &[], "complete");
    f.attach(&root, Some((&revision, &payload)));
    let h = f.history();
    assert!(h.provenance().contains(&claim) && h.provenance().contains(&context));
    assert_eq!(f.store.reads, 2);
    assert!(!h.complete_semantic_coverage() && !h.authorizes_eviction());
}

#[test]
fn manifest_foreign_ancestry_conflicting_sizes_and_duplicate_representation_reject() {
    for mode in [
        "foreign",
        "size",
        "duplicate-representation",
        "unknown-provenance",
        "empty",
    ] {
        let mut f = Fixture::new("image");
        let payload = Object::from_bytes(b"payload");
        let parent = f.revision("parent", &payload, &[]);
        let mut manifest: Value = serde_json::from_slice(&f.store.objects[&parent]).unwrap();
        match mode {
            "foreign" => manifest["asset"] = json!("other/asset"),
            "size" => {
                manifest["provenance"]["source"] =
                    json!({"sha256":payload.sha256,"bytes":payload.bytes + 1})
            }
            "duplicate-representation" => {
                let rep = manifest["representations"][0].clone();
                manifest["representations"]
                    .as_array_mut()
                    .unwrap()
                    .push(rep);
            }
            "unknown-provenance" => {
                manifest["provenance"]["kind"] = json!("approved-and-safe-to-delete")
            }
            _ => manifest["representations"] = json!([]),
        }
        let malformed = f.store.put(manifest);
        let head = if mode == "foreign" {
            f.revision("child", &payload, &[malformed])
        } else {
            malformed
        };
        let root = f.page(&[head], &[], "complete");
        f.attach(&root, None);
        assert_eq!(
            f.error(),
            match mode {
                "foreign" => Error::Identity,
                "size" => Error::Pin,
                "duplicate-representation" => Error::Conflict,
                "unknown-provenance" => Error::Schema,
                _ => Error::Budget,
            }
        );
    }
}

#[test]
fn duplicate_history_edges_refuse_but_distinct_page_sharing_is_allowed() {
    let mut f = Fixture::new("document");
    let revision = f.revision("a", &Object::from_bytes(b"payload"), &[]);
    let root = f.page(&[revision.clone(), revision], &[], "complete");
    f.attach(&root, None);
    assert_eq!(f.error(), Error::Conflict);
}

#[test]
fn cumulative_document_and_byte_limits_stop_before_the_next_fetch() {
    for mode in ["documents", "bytes"] {
        let mut f = Fixture::new("document");
        let revision = f.revision("a", &Object::from_bytes(b"payload"), &[]);
        let root = f.page(std::slice::from_ref(&revision), &[], "complete");
        f.attach(&root, None);
        let limits = if mode == "documents" {
            Limits {
                documents: 1,
                ..Limits::default()
            }
        } else {
            Limits {
                bytes: (root.bytes + revision.bytes - 1) as usize,
                ..Limits::default()
            }
        };
        assert_eq!(f.check(limits).err(), Some(Error::Budget));
        assert_eq!(f.store.reads, 1);
    }
}
