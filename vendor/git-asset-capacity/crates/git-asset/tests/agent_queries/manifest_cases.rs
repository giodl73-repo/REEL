use super::*;
use git_asset_store::existing::Layout;

pub(super) struct Manifest {
    pub(super) f: Fixture,
    pub(super) cache: std::path::PathBuf,
    pub(super) layout: Layout,
    pub(super) root: Object,
    pub(super) leaf: Object,
}
impl Manifest {
    pub(super) fn new(layout: Layout, document: Value) -> Self {
        let f = Fixture::new();
        let cache = f.temp.path().join("existing-cache");
        fs::create_dir(&cache).unwrap();
        let raw = serde_json::to_vec(&document).unwrap();
        let root = Object::from_bytes(&raw);
        let path = layout.object_path(&cache, &root.sha256);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, raw).unwrap();
        let path = f.root().join("records/scene.asset");
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["selected"] = json!(root);
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
        f.advance();
        let pp = f.temp.path().join("profile.json");
        let mut p: Value = serde_json::from_slice(&fs::read(&pp).unwrap()).unwrap();
        p["byte_source"] = json!({"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"},"places":[{"root":cache,"layout":layout}]});
        fs::write(pp, serde_json::to_vec(&p).unwrap()).unwrap();
        let mut c: Value = serde_json::from_slice(&fs::read(&f.enrollment).unwrap()).unwrap();
        c["projects"][0]["collections"] = json!([{"id":"works","label":"Works","icon":"document","roots":[{"pattern":"^scene$","key":"scene"}]}]);
        c["projects"][0]["storage"] = json!({"schema":"tig.catalogue-storage.experimental-1","encoding":"json","max_decoded_bytes":1048576,"places":[{"label":"Fixture","place_index":0,"live_probe":true}],"nested_content":{"max_documents":512,"max_document_bytes":4194304,"max_total_bytes":67108864,"max_references":100000,"max_depth":16,"adapters":[{"id":"bundle","schema":"fixture.bundle.v1","roots":[{"definition":"type/scene","asset_pattern":"^scene$","slot_pattern":"^selected$"}],"objects":[{"pointer":"/assets/*"}],"documents":[{"selector":{"pointer":"/child","optional":true},"adapter":"bundle"}]}]}});
        fs::write(&f.enrollment, serde_json::to_vec(&c).unwrap()).unwrap();
        Self {
            f,
            cache,
            layout,
            root,
            leaf: leaf(),
        }
    }
    pub(super) fn put(&self, value: &Value) -> Object {
        let raw = serde_json::to_vec(value).unwrap();
        let o = Object::from_bytes(&raw);
        let path = self.layout.object_path(&self.cache, &o.sha256);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, raw).unwrap();
        o
    }
    pub(super) fn request(&self) -> Value {
        request(&self.f, &self.leaf)
    }
}
pub(super) fn leaf() -> Object {
    Object::from_bytes(b"media leaf deliberately not stored")
}
fn request(f: &Fixture, leaf: &Object) -> Value {
    let works = f.evaluate(
        &f.request(json!({"operation":"works","project":"project","collection":"works"})),
    );
    assert_eq!(works["data"]["items"][0]["id"], "scene");
    let mut request=f.request(json!({"operation":"manifest_references","project":"project","collection":"works","work":"scene","sha256":leaf.sha256}));
    request["expected_basis"] = works["basis_sha256"].clone();
    request
}
pub(super) fn edit(f: &Fixture, change: impl FnOnce(&mut Value)) {
    let mut c: Value = serde_json::from_slice(&fs::read(&f.enrollment).unwrap()).unwrap();
    change(&mut c);
    fs::write(&f.enrollment, serde_json::to_vec(&c).unwrap()).unwrap();
}

#[test]
fn explicit_manifests_preserve_provenance_without_opening_leaf_bytes_in_flat_and_lfs_layouts() {
    for layout in [Layout::Flat, Layout::GitLfs] {
        let child = json!({"schema":"fixture.bundle.v1","assets":{"leaf":leaf()},"private":"DO_NOT_EXPORT"});
        let child_pin = Object::from_bytes(&serde_json::to_vec(&child).unwrap());
        let root = json!({"schema":"fixture.bundle.v1","assets":{"leaf":leaf()},"child":child_pin,"unrelated_fraction":1.25,"url":"https://never.invalid"});
        let m = Manifest::new(layout, root);
        m.put(&child);
        let (out, result) = m.f.run(&m.request(), &[]);
        assert!(out.status.success(), "{result}");
        assert_eq!(result["data"]["total"], 2);
        assert_eq!(result["data"]["metadata_read_attempts"], 2);
        assert_eq!(result["data"]["metadata_documents_decoded"], 2);
        assert!(result["coverage"]["payloads_opened"].is_null());
        assert_eq!(result["coverage"]["media_leaves_opened"], 0);
        assert_eq!(result["coverage"]["complete_project_scope"], false);
        for row in result["data"]["items"].as_array().unwrap() {
            assert_eq!(row["reference"]["asset"], "scene");
            assert_eq!(row["reference"]["root"], json!(m.root));
            assert_eq!(row["reference"]["root_slot"], "selected");
            assert_eq!(row["reference"]["pointer"], "/assets/leaf");
            assert!(
                row["reference"]["document"] == json!(m.root)
                    || row["reference"]["document"] == json!(child_pin)
            );
        }
        assert!(!m.layout.object_path(&m.cache, &m.leaf.sha256).exists());
    }
}

#[test]
fn missing_child_discards_partial_root_and_recovery_invalidates_content_token() {
    let child = json!({"schema":"fixture.bundle.v1","assets":{}});
    let pin = Object::from_bytes(&serde_json::to_vec(&child).unwrap());
    let m = Manifest::new(
        Layout::Flat,
        json!({"schema":"fixture.bundle.v1","assets":{"leaf":leaf()},"child":pin}),
    );
    let mut request = m.request();
    let (out, partial) = m.f.run(&request, &[]);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(partial["data"]["total"], 0);
    assert_eq!(partial["data"]["complete_extraction"], false);
    assert_eq!(
        partial["data"]["extraction"]["failures"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    request["query"]["expected_content"] = partial["data"]["content_sha256"].clone();
    m.put(&child);
    let (out, error) = m.f.run(&request, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "stale_content");
    request["query"]
        .as_object_mut()
        .unwrap()
        .remove("expected_content");
    let full = m.f.evaluate(&request);
    assert_eq!(full["status"], "complete");
    assert_eq!(full["data"]["total"], 1);
}

#[test]
fn manifest_paging_requires_content_fence_and_refuses_changed_availability() {
    let assets = (0..120)
        .map(|n| (format!("key-{n:03}"), json!(leaf())))
        .collect::<serde_json::Map<_, _>>();
    let m = Manifest::new(
        Layout::Flat,
        json!({"schema":"fixture.bundle.v1","assets":assets}),
    );
    let mut request = m.request();
    let first = m.f.evaluate(&request);
    assert_eq!(first["paging"]["returned"], 100);
    assert_eq!(first["paging"]["next_offset"], 100);
    request["offset"] = json!(100);
    assert!(Request::parse(&serde_json::to_vec(&request).unwrap()).is_err());
    request["query"]["expected_content"] = first["data"]["content_sha256"].clone();
    let second = m.f.evaluate(&request);
    assert_eq!(second["paging"]["returned"], 20);
    assert!(second["paging"]["next_offset"].is_null());
    fs::remove_file(m.layout.object_path(&m.cache, &m.root.sha256)).unwrap();
    let (out, error) = m.f.run(&request, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "stale_content");
}

#[test]
fn changed_adapter_and_source_policies_refuse_before_metadata_reads() {
    let m = Manifest::new(
        Layout::Flat,
        json!({"schema":"fixture.bundle.v1","assets":{"leaf":leaf()}}),
    );
    let request = m.request();
    edit(&m.f, |c| {
        c["projects"][0]["storage"]["places"][0]["live_probe"] = json!(false)
    });
    let (out, error) = m.f.run(&request, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "stale_basis");
    let mut missing = request;
    missing.as_object_mut().unwrap().remove("expected_basis");
    assert!(Request::parse(&serde_json::to_vec(&missing).unwrap()).is_err());
}

#[test]
fn corrupt_or_wrong_schema_metadata_is_partial_and_never_uses_unrelated_strings() {
    for wrong_schema in [false, true] {
        let m = Manifest::new(
            Layout::Flat,
            json!({"schema":if wrong_schema{"wrong"}else{"fixture.bundle.v1"},"assets":{"leaf":leaf()},"cache_uri":"C:/never","url":"https://never.invalid"}),
        );
        if !wrong_schema {
            fs::write(m.layout.object_path(&m.cache, &m.root.sha256), b"corrupt").unwrap();
        }
        let (out, result) = m.f.run(&m.request(), &[]);
        assert_eq!(out.status.code(), Some(3));
        assert_eq!(result["data"]["total"], 0);
        assert_eq!(result["data"]["complete_extraction"], false);
        assert_eq!(result["coverage"]["media_leaves_opened"], 0);
    }
}

#[test]
fn unadmitted_manifest_scope_is_unknown_and_does_not_read_bytes() {
    let m = Manifest::new(
        Layout::Flat,
        json!({"schema":"fixture.bundle.v1","assets":{"leaf":leaf()}}),
    );
    edit(&m.f, |c| {
        c["projects"][0]
            .as_object_mut()
            .unwrap()
            .remove("storage")
            .map(|_| ())
            .unwrap_or(())
    });
    let (out, result) = m.f.run(&m.request(), &[]);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(result["data"]["metadata_read_attempts"], 0);
    assert_eq!(result["data"]["complete_extraction"], false);
    let query =
        m.f.request(json!({"operation":"works","project":"project","collection":"missing"}));
    let (out, error) = m.f.run(&query, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "unknown_collection");
}

#[test]
fn agent_root_budget_refuses_more_than_256_declared_roots() {
    let m = Manifest::new(
        Layout::Flat,
        json!({"schema":"fixture.bundle.v1","assets":{"leaf":leaf()}}),
    );
    let path = m.f.root().join("records/scene.asset");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["streams"]=json!((0..256).map(|n|json!({"slot":format!("s{n}"),"role":"manifest","group":"fixtures","object":m.root})).collect::<Vec<_>>());
    fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    m.f.advance();
    edit(&m.f, |c| {
        c["projects"][0]["storage"]["nested_content"]["adapters"][0]["roots"][0]["slot_pattern"] =
            json!("^.*$")
    });
    let (out, error) = m.f.run(&m.request(), &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "manifest_budget_exceeded");
}
