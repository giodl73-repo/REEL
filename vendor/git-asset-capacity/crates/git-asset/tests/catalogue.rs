#![cfg(feature = "schema")]
mod common;
use common::{git, git_executable};
use git_asset::catalogue::{Catalogue, Config};
use git_asset_core::Object;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn write(root: &Path, path: &str, value: &Value) -> Object {
    let bytes = serde_json::to_vec(value).unwrap();
    fs::write(root.join(path), &bytes).unwrap();
    Object::from_bytes(&bytes)
}
fn commit(root: &Path) -> String {
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "synthetic catalogue"]);
    String::from_utf8(git(root, &["rev-parse", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_owned()
}
struct Fixture {
    temp: tempfile::TempDir,
    config: std::path::PathBuf,
    first: String,
    second: String,
}

#[test]
fn shared_selected_is_exact_deduplicated_scoped_and_payload_free() {
    use git_asset::catalogue::shared::compare;
    let f = Fixture::new();
    let load = || Config::load(&f.config).unwrap();
    let report = compare(load(), "alpha", "beta", "").unwrap();
    assert_eq!(report["shared_objects"], 1);
    assert_eq!(report["items"][0]["left"].as_array().unwrap().len(), 3);
    assert_eq!(report["items"][0]["right"].as_array().unwrap().len(), 3);
    assert_eq!(
        report["shared_unique_bytes"],
        Object::from_bytes(b"deliberately not stored anywhere").bytes
    );
    assert_eq!(report["coverage"][0]["commit"], f.first);
    assert_eq!(report["payloads_opened"], 0);
    assert_eq!(report["complete_project_comparison"], false);
    assert!(!report.to_string().contains("NEVER_EXPOSE"));
    let filtered = compare(load(), "alpha", "beta", "Photographs").unwrap();
    assert_eq!(filtered["shared_objects"], 0);
    assert_eq!(filtered["coverage"][1]["matched_records"], 0);
    assert!(compare(load(), "alpha", "alpha", "").is_err());
    assert!(compare(load(), "alpha", "missing", "").is_err());
    let mut config = load();
    let mut unrelated = config.projects[0].clone();
    unrelated.id = "unavailable".into();
    unrelated.profile = f.temp.path().join("missing.json");
    config.projects.push(unrelated);
    assert!(compare(config, "alpha", "beta", "").is_ok());

    // The CLI writes an explicit report, never overwrites it, and emits only totals.
    let output = f.temp.path().join("report.json");
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_git-asset-catalogue"))
            .arg("shared")
            .arg(&f.config)
            .args(["alpha", "beta"])
            .arg(&output)
            .output()
            .unwrap()
    };
    let result = run();
    assert!(result.status.success(), "{:?}", result);
    let stdout: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(stdout["shared_objects"], 1);
    let saved = fs::read(&output).unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read(&output).unwrap(), saved);

    let repo = f.temp.path().join("beta");
    let mut record: Value =
        serde_json::from_slice(&fs::read(repo.join("records/0.asset")).unwrap()).unwrap();
    record["selected"] = Value::Null;
    write(&repo, "records/0.asset", &record);
    let pin_profile = |head: String| {
        let path = f.temp.path().join("beta.json");
        let mut profile: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        profile["commit"] = json!(head);
        fs::write(path, serde_json::to_vec(&profile).unwrap()).unwrap();
    };
    pin_profile(commit(&repo));
    let missing = compare(load(), "alpha", "beta", "").unwrap();
    assert_eq!(missing["coverage"][1]["without_selected_object"], 1);
    let object = Object::from_bytes(b"deliberately not stored anywhere");
    record["selected"] = json!({"sha256":object.sha256,"bytes":object.bytes+1});
    write(&repo, "records/0.asset", &record);
    pin_profile(commit(&repo));
    assert!(compare(load(), "alpha", "beta", "").is_err());
}

#[test]
fn collection_union_retains_each_configured_state_and_search() {
    let f = Fixture::new();
    let mut config: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    config["projects"][0]["collections"] = json!([
        {"id":"one","label":"One","icon":"document","roots":[{"pattern":"^asset/([01])$","key":"asset/${1}"}],"stages":[{"label":"Accepted","pointer":"/id","pattern":"^asset/0$"}],"count_state":"Accepted"},
        {"id":"two","label":"Two","icon":"document","roots":[{"pattern":"^asset/2$","key":"asset/2"}]}
    ]);
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let c = f.load();
    let union = c.work_selection("alpha", "one,two", "", 0).unwrap();
    assert_eq!(union["total"], 2);
    assert_eq!(c.work_selection("alpha", "two,one", "", 0).unwrap(), union);
    assert_eq!(
        c.work_selection("alpha", "one,two", "item 2", 0).unwrap()["total"],
        1
    );
    assert_eq!(
        c.work_selection("alpha", "one,two", "", 1).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(c.work_selection("alpha", "one,two", "", 1_000_001).is_err());
}

#[test]
fn reference_lookup_distinguishes_selected_and_named_bindings_and_pages() {
    let f = Fixture::new();
    let repo = f.temp.path().join("beta");
    let mut record: Value =
        serde_json::from_slice(&fs::read(repo.join("records/0.asset")).unwrap()).unwrap();
    let old = Object::from_bytes(b"retained fixture bytes");
    record["streams"] = json!([{"slot":"old","role":"result","group":"retained","object":old}]);
    write(&repo, "records/0.asset", &record);
    for n in 3..105 {
        record["id"] = json!(format!("asset/{n}"));
        record["streams"] = json!([]);
        write(&repo, &format!("records/{n}.asset"), &record);
    }
    let head = commit(&repo);
    let path = f.temp.path().join("beta.json");
    let mut profile: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    profile["commit"] = json!(head);
    fs::write(path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let c = f.load();
    let sha = Object::from_bytes(b"deliberately not stored anywhere").sha256;
    let all = c.object_references(sha.as_str(), "all", 0).unwrap();
    assert_eq!(all["total"], 108);
    assert_eq!(all["items"].as_array().unwrap().len(), 100);
    assert_eq!(
        c.object_references(sha.as_str(), "all", 100).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    assert_eq!(
        c.object_references(sha.as_str(), "alpha", 0).unwrap()["total"],
        3
    );
    let retained = c.object_references(old.sha256.as_str(), "all", 0).unwrap();
    assert_eq!(retained["total"], 1);
    assert_eq!(retained["items"][0]["binding"], "stream");
    assert_eq!(retained["items"][0]["slot"], "old");
    assert_eq!(retained["items"][0]["group"], "retained");
    assert_eq!(retained["payloads_opened"], 0);
    assert_eq!(retained["complete_project_comparison"], false);
    assert!(c.object_references("not-a-hash", "all", 0).is_err());
    assert!(c.object_references(sha.as_str(), "missing", 0).is_err());
    assert!(c.object_references(sha.as_str(), "all", 1_000_001).is_err());
    let shared = c.shared_page("alpha", "beta", "", 0).unwrap();
    assert_eq!(shared["items"][0]["right_total"], 105);
    assert_eq!(shared["items"][0]["right"].as_array().unwrap().len(), 100);
    assert!(c.shared_page("alpha", "alpha", "", 0).is_err());
    assert!(c.shared_page("alpha", "beta", "", 1_000_001).is_err());
    let output = f.temp.path().join("references.json");
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_git-asset-catalogue"))
        .args(["references"])
        .arg(&f.config)
        .arg(old.sha256.as_str())
        .arg(output)
        .args(["beta", "0"])
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result);
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["total"],
        1
    );
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let mut projects = vec![];
        let mut commits = vec![];
        for (id, category) in [("alpha", "Photographs"), ("beta", "Contracts")] {
            let repo = root.join(id);
            fs::create_dir(&repo).unwrap();
            git(&repo, &["init", "-b", "main"]);
            git(&repo, &["config", "user.name", "Fixture"]);
            git(&repo, &["config", "user.email", "fixture@example.invalid"]);
            fs::create_dir(repo.join("records")).unwrap();
            fs::create_dir(repo.join("definitions")).unwrap();
            let schema = write(
                &repo,
                "definitions/authored.json",
                &json!({"type":"object","required":["title"],"properties":{"title":{"type":"string"}}}),
            );
            let definition = write(
                &repo,
                "definitions/type.json",
                &json!({"schema":"tig.definition.design-1","id":"type/item","authored_schema":schema,"authoring":{"properties":["title","private","version"],"required":["title"],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
            );
            let mut pin = serde_json::to_value(definition).unwrap();
            pin["id"] = json!("type/item");
            for n in 0..3 {
                write(
                    &repo,
                    &format!("records/{n}.asset"),
                    &json!({"schema":"tig.instance.design-1","id":format!("asset/{n}"),"definition":pin,"authored":{"title":format!("{id} item {n}"),"private":"NEVER_EXPOSE"},"streams":[],"selected":Object::from_bytes(b"deliberately not stored anywhere"),"status":{"lifecycle":"draft"},"metadata":{"private":"NEVER_EXPOSE"}}),
                );
            }
            let first = commit(&repo);
            git(&repo, &["tag", "-a", "previous", "-m", "old snapshot"]);
            if id == "alpha" {
                let path = repo.join("records/0.asset");
                let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                record["authored"]["title"] = json!("<img src=x onerror=alert(1)> New title");
                fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
                let second = commit(&repo);
                commits = vec![first.clone(), second];
                // Uncommitted edits must never change the selected snapshot.
                fs::write(&path, "uncommitted invalid JSON").unwrap();
            }
            let profile = root.join(format!("{id}.json"));
            fs::write(&profile, serde_json::to_vec(&json!({"schema":"tig.asset-resolver-profile.experimental-1","git_executable":git_executable(),"repository":repo,"commit":first,"records":["records"],"definitions":["definitions"]})).unwrap()).unwrap();
            projects.push(json!({"id":id,"label":id,"profile":profile,"display":[{"category":category,"title_pointers":["/authored/title"]}],"fields":[{"label":"Title","pointer":"/authored/title","section":"source"}]}));
        }
        let config = root.join("enrollment.json");
        fs::write(
            &config,
            serde_json::to_vec(
                &json!({"schema":"tig.catalogue-enrollment.experimental-1","projects":projects}),
            )
            .unwrap(),
        )
        .unwrap();
        Self {
            temp,
            config,
            first: commits[0].clone(),
            second: commits[1].clone(),
        }
    }
    fn load(&self) -> Catalogue {
        Catalogue::load(Config::load(&self.config).unwrap()).unwrap()
    }
}

#[test]
fn nested_catalogue_uses_guarded_metadata_preserves_shared_current_and_fences_adoption() {
    use git_asset::catalogue::storage::{nested, objects::Query};
    let f = Fixture::new();
    let root = f.temp.path();
    let repo = root.join("alpha");
    let fast = root.join("fast");
    let history = root.join("history");
    for p in [&fast, &history] {
        fs::create_dir_all(p.join("objects")).unwrap();
    }
    let old = Object::from_bytes(b"old leaf");
    let new = Object::from_bytes(b"new leaf");
    let shared = Object::from_bytes(b"shared leaf");
    let child = write(
        &fast.join("objects"),
        "child.json",
        &json!({"schema":"fixture.bundle.v1","assets":{"shared":shared}}),
    );
    let prior = write(
        &fast.join("objects"),
        "prior.json",
        &json!({"schema":"fixture.bundle.v1","assets":{"image":old},"child":child}),
    );
    let current = write(
        &fast.join("objects"),
        "current.json",
        &json!({"schema":"fixture.bundle.v1","assets":{"image":new},"child":child}),
    );
    for (o, name) in [
        (&child, "child.json"),
        (&prior, "prior.json"),
        (&current, "current.json"),
    ] {
        fs::copy(
            fast.join("objects").join(name),
            fast.join("objects").join(o.sha256.as_str()),
        )
        .unwrap();
    }
    for (o, b) in [
        (&old, b"old leaf".as_slice()),
        (&new, b"new leaf".as_slice()),
        (&shared, b"shared leaf".as_slice()),
    ] {
        fs::write(fast.join("objects").join(o.sha256.as_str()), b).unwrap();
    }
    fs::write(
        history.join("objects").join(old.sha256.as_str()),
        b"old leaf",
    )
    .unwrap();
    let mut record: Value = serde_json::from_slice(
        &git(&repo, &["show", &format!("{}:records/0.asset", f.first)]).stdout,
    )
    .unwrap();
    record["selected"] = json!(current);
    record["authored"]["version"] = json!("r2");
    record["streams"] = json!([{"group":"versions","role":"result","slot":"run/r1","object":prior},{"group":"versions","role":"result","slot":"run/r2","object":current}]);
    write(&repo, "records/0.asset", &record);
    let mut other = record.clone();
    other["id"] = json!("asset/1");
    other["selected"] = json!(child);
    other["streams"] = json!([]);
    write(&repo, "records/1.asset", &other);
    other["id"] = json!("asset/2");
    other["selected"] = json!(old);
    write(&repo, "records/2.asset", &other);
    let head = commit(&repo);
    let pp = root.join("alpha.json");
    let mut profile: Value = serde_json::from_slice(&fs::read(&pp).unwrap()).unwrap();
    profile["commit"] = json!(head);
    profile["byte_source"] = json!({"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"},"places":[{"layout":"flat","root":fast},{"layout":"flat","root":history}]});
    fs::write(&pp, serde_json::to_vec(&profile).unwrap()).unwrap();
    let mut config: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    config["projects"][0]["collections"] = json!([{"id":"works","label":"Works","icon":"document","roots":[{"pattern":"^asset/([0-9])$","key":"asset/${1}"}],"version_pointer":"/authored/version","version_role":"result","version_slot_prefix":"run/"}]);
    config["projects"][0]["storage"] = json!({"schema":"tig.catalogue-storage.experimental-1","encoding":"json","max_decoded_bytes":1024,"places":[{"label":"Fast","place_index":0,"live_probe":true},{"label":"History","place_index":1,"live_probe":true}],"placement_rules":[{"id":"current","label":"Working","pattern":"^asset/0$","usages":["current-revision"],"place_index":0},{"id":"history","label":"History","pattern":"^asset/0$","usages":["previous-revision"],"place_index":1}],"nested_content":{"max_documents":16,"max_document_bytes":1048576,"max_total_bytes":4194304,"max_references":100,"max_depth":4,"adapters":[{"id":"bundle","schema":"fixture.bundle.v1","roots":[{"definition":"type/item","asset_pattern":"^asset/[01]$","slot_pattern":"^(selected|run/r1)$"}],"objects":[{"pointer":"/assets/*"}],"documents":[{"selector":{"pointer":"/child","optional":true},"adapter":"bundle"}]}]}});
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut c = f.load();
    let query = Query {
        project: "alpha",
        kind: "",
        usage: "",
        anomaly: "",
        primary: "",
        observed: "",
        q: "",
        sort: "bytes",
        offset: 0,
        sha: "",
    };
    let before = c.object_inventory(&query).unwrap();
    let coverage = c.coverage("alpha", "works", "", "", 0).unwrap();
    assert_eq!(coverage["total"], 3);
    assert_eq!(coverage["metadata_reads"], 0);
    assert_eq!(coverage["states_before_status_filter"]["not-loaded"], 2);
    assert_eq!(
        c.coverage("alpha", "works", "not-loaded", "", 0).unwrap()["total"],
        2
    );
    assert_eq!(
        c.coverage("alpha", "works", "", "", 1).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        c.coverage("alpha", "works", "", "asset/0", 0).unwrap()["total"],
        1
    );
    assert!(c.coverage("missing", "", "", "", 0).is_err());
    let eligible = c.work_object_inventory(&query, "works", "asset/0").unwrap();
    assert_eq!(eligible["scope"]["nested_roots"], 2);
    let absent = c.work_object_inventory(&query, "works", "asset/2").unwrap();
    assert_eq!(absent["scope"]["nested_roots"], 0);
    assert_eq!(absent["scope"]["nested_available"], false);
    let req = c
        .nested_request("alpha", &head, "works", "asset/0")
        .unwrap();
    let loaded = nested::execute(&req).unwrap();
    let summary = c.adopt_nested(&req, loaded).unwrap();
    assert_eq!(summary["successful_roots"], 2);
    assert_eq!(summary["documents_read"], 3);
    c.observe_registered("alpha", &head, "").unwrap();
    let work_storage = c
        .storage(
            "alpha",
            &head,
            &git_asset::catalogue::storage::Query {
                cache: "",
                family: "Works",
                owner: "asset/0",
                version: "",
                state: "",
                pivot: "works",
                metric: "recorded_file_bytes",
                q: "",
                offset: 0,
            },
        )
        .unwrap();
    assert_eq!(work_storage["groups_total"], 1);
    assert!(work_storage["objects"].as_array().unwrap().iter().any(|o| {
        o["sha256"] == shared.sha256.as_str()
            && o["owners"]
                .as_array()
                .unwrap()
                .contains(&json!(["Works", "asset/0"]))
    }));
    assert_eq!(work_storage["removal_authorized"], false);
    let d = c.work_object_inventory(&query, "works", "asset/0").unwrap();
    assert_ne!(
        c.coverage("alpha", "works", "", "", 0).unwrap()["basis"],
        coverage["basis"]
    );
    assert_eq!(
        c.coverage("alpha", "works", "expanded", "", 0).unwrap()["total"],
        1
    );
    let pair = c
        .comparison("alpha", &head, "works", "asset/0", "run/r1", "run/r2")
        .unwrap();
    let comparison = c
        .finish_comparison(git_asset::catalogue::storage::review::execute(pair).unwrap())
        .unwrap();
    assert_eq!(comparison["added"]["count"], 2);
    assert_eq!(comparison["removed"]["count"], 2);
    assert_eq!(comparison["shared"]["count"], 2);
    assert_eq!(comparison["removed_with_other_records"], 1);
    assert_eq!(comparison["configured_paths_complete"], true);
    assert_eq!(comparison["safe_reclaimable_bytes"], Value::Null);
    assert_eq!(comparison["removal_authorized"], false);
    assert!(!comparison.to_string().contains("NEVER_EXPOSE"));
    let same = c
        .comparison("alpha", &head, "works", "asset/0", "run/r2", "run/r2")
        .unwrap();
    let same = c
        .finish_comparison(git_asset::catalogue::storage::review::execute(same).unwrap())
        .unwrap();
    assert_eq!(same["added"]["count"], 0);
    assert_eq!(same["removed"]["count"], 0);
    assert_eq!(same["shared"]["count"], 4);
    assert!(
        c.comparison("alpha", &head, "works", "asset/0", "missing", "run/r2")
            .is_err()
    );
    let rows = d["items"].as_array().unwrap();
    let shared_row = rows
        .iter()
        .find(|r| r["sha256"] == shared.sha256.as_str())
        .unwrap();
    assert_eq!(shared_row["has_current_revision"], true);
    assert_eq!(shared_row["primary"].as_array().unwrap().len(), 1);
    assert_eq!(shared_row["primary"][0]["label"], "Fast");
    assert_eq!(shared_row["copies"].as_array().unwrap().len(), 1);
    assert!(
        shared_row["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["nested"]["document"] == json!(child))
    );
    let old_row = rows
        .iter()
        .find(|r| r["sha256"] == old.sha256.as_str())
        .unwrap();
    assert_eq!(old_row["primary"][0]["label"], "History");
    assert_eq!(old_row["copies"].as_array().unwrap().len(), 2);
    assert!(
        c.object_preview(
            "alpha",
            shared.sha256.as_str(),
            shared_row["basis"].as_str().unwrap(),
            "evict-review"
        )
        .unwrap()["proposals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_ne!(before["policy_pins"], d["policy_pins"]);
    let second = c
        .nested_request("alpha", &head, "works", "asset/1")
        .unwrap();
    c.adopt_nested(&second, nested::execute(&second).unwrap())
        .unwrap();
    assert!(
        c.object_preview(
            "alpha",
            shared.sha256.as_str(),
            shared_row["basis"].as_str().unwrap(),
            "align"
        )
        .is_err()
    );
    let all = c.object_inventory(&query).unwrap();
    let s = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["sha256"] == shared.sha256.as_str())
        .unwrap();
    assert!(
        s["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["id"] == "asset/1")
    );
    let pinned = c.object_inventory(&query).unwrap();
    fs::write(
        fast.join("objects").join(prior.sha256.as_str()),
        b"corrupt metadata",
    )
    .unwrap();
    let incomplete = nested::execute(&req).unwrap();
    let partial = c
        .comparison("alpha", &head, "works", "asset/0", "run/r1", "run/r2")
        .unwrap();
    let partial = c
        .finish_comparison(git_asset::catalogue::storage::review::execute(partial).unwrap())
        .unwrap();
    assert_eq!(partial["configured_paths_complete"], false);
    assert!(c.adopt_nested(&req, incomplete).is_err());
    assert_eq!(
        c.object_inventory(&query).unwrap()["policy_pins"],
        pinned["policy_pins"]
    );
    fs::copy(
        fast.join("objects/prior.json"),
        fast.join("objects").join(prior.sha256.as_str()),
    )
    .unwrap();
    let candidate = nested::execute(&req).unwrap();
    let stale = c
        .comparison("alpha", &head, "works", "asset/0", "run/r1", "run/r2")
        .unwrap();
    let stale = git_asset::catalogue::storage::review::execute(stale).unwrap();
    c.projects
        .get_mut("alpha")
        .unwrap()
        .change_snapshot(&f.first)
        .unwrap();
    assert!(c.adopt_nested(&req, candidate).is_err());
    assert!(c.finish_comparison(stale).is_err());
    config["projects"][0]["storage"]["nested_content"]["max_loaded_scopes"] = json!(1);
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut capped = f.load();
    let a = capped
        .nested_request("alpha", &head, "works", "asset/0")
        .unwrap();
    capped
        .adopt_nested(&a, nested::execute(&a).unwrap())
        .unwrap();
    let prior = capped.object_inventory(&query).unwrap();
    let b = capped
        .nested_request("alpha", &head, "works", "asset/1")
        .unwrap();
    assert!(
        capped
            .adopt_nested(&b, nested::execute(&b).unwrap())
            .is_err()
    );
    assert_eq!(
        capped.object_inventory(&query).unwrap()["policy_pins"],
        prior["policy_pins"]
    );
    assert_eq!(
        fs::read(fast.join("objects").join(new.sha256.as_str())).unwrap(),
        b"new leaf"
    );
    config["projects"][0]["storage"]["nested_content"] = Value::Null;
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let direct = f.load();
    let pair = direct
        .comparison("alpha", &head, "works", "asset/0", "run/r1", "run/r2")
        .unwrap();
    let pair = direct
        .finish_comparison(git_asset::catalogue::storage::review::execute(pair).unwrap())
        .unwrap();
    assert_eq!(pair["metadata"], Value::Null);
    assert_eq!(pair["configured_paths_complete"], false);
    assert_eq!(pair["added"]["count"], 1);
    assert_eq!(
        direct
            .coverage("alpha", "works", "unconfigured", "", 0)
            .unwrap()["total"],
        3
    );
}

#[test]
fn named_object_placement_versions_shared_bytes_and_stale_previews() {
    use git_asset::catalogue::storage::objects::Query;
    let f = Fixture::new();
    let root = f.temp.path();
    let fast = root.join("fast");
    let history = root.join("history");
    for p in [&fast, &history] {
        fs::create_dir_all(p.join("objects")).unwrap();
    }
    let old = Object::from_bytes(b"old body");
    let new = Object::from_bytes(b"new longer body");
    let shared = Object::from_bytes(b"deliberately not stored anywhere");
    for (o, b) in [
        (&old, b"old body".as_slice()),
        (&new, b"new longer body".as_slice()),
        (&shared, b"deliberately not stored anywhere".as_slice()),
    ] {
        fs::write(fast.join("objects").join(o.sha256.as_str()), b).unwrap();
    }
    fs::write(
        history.join("objects").join(old.sha256.as_str()),
        b"old body",
    )
    .unwrap();
    let repo = root.join("alpha");
    let mut record: Value = serde_json::from_slice(
        &git(&repo, &["show", &format!("{}:records/0.asset", f.first)]).stdout,
    )
    .unwrap();
    record["authored"]["version"] = json!("r2");
    record["selected"] = json!(new);
    record["streams"] = json!([{"group":"version","role":"result","slot":"run/r1","object":old},{"group":"version","role":"result","slot":"run/r2","object":new}]);
    fs::write(
        repo.join("records/0.asset"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let head = commit(&repo);
    let mut config: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    config["storage_domains"] =
        json!([{"id":"local","label":"Local","root":root,"live_probe":false}]);
    for (i, id) in ["alpha", "beta"].iter().enumerate() {
        let path = root.join(format!("{id}.json"));
        let mut profile: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if *id == "alpha" {
            profile["commit"] = json!(head);
        }
        profile["byte_source"] = json!({"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"},"places":[{"layout":"flat","root":fast},{"layout":"flat","root":history}]});
        fs::write(path, serde_json::to_vec(&profile).unwrap()).unwrap();
        config["projects"][i]["storage"] = json!({"schema":"tig.catalogue-storage.experimental-1","inventory":null,"encoding":"json","max_decoded_bytes":1024,"places":[{"label":"Working","place_index":0,"report_tier":null,"live_probe":true},{"label":"History","place_index":1,"report_tier":null,"live_probe":true}],"placement_rules":[{"id":"current","label":"Current revision working preference","pattern":"^asset/0$","usages":["current-revision"],"place_index":0},{"id":"previous","label":"Previous revision history preference","pattern":"^asset/0$","usages":["previous-revision"],"place_index":1}]});
    }
    config["projects"][0]["collections"] = json!([{"id":"documents","label":"Documents","icon":"document","roots":[{"pattern":"^asset/([0-9]+)$","key":"asset/${1}"}],"version_pointer":"/authored/version","version_role":"result","version_slot_prefix":"run/"}]);
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut c = f.load();
    let beta_head = serde_json::from_slice::<Value>(&fs::read(root.join("beta.json")).unwrap())
        .unwrap()["commit"]
        .as_str()
        .unwrap()
        .to_owned();
    c.observe_registered("alpha", &head, "").unwrap();
    c.observe_registered("beta", &beta_head, "").unwrap();
    let q = |usage| Query {
        project: "alpha",
        kind: "",
        usage,
        anomaly: "",
        primary: "",
        observed: "",
        q: "",
        sort: "bytes",
        offset: 0,
        sha: "",
    };
    let all = c.object_inventory(&q("")).unwrap();
    assert_eq!(all["total"], 3);
    assert_eq!(
        all["unique_content_bytes"],
        old.bytes + new.bytes + shared.bytes
    );
    let previous = c.object_inventory(&q("previous-revision")).unwrap();
    let scoped = c
        .work_object_inventory(&q(""), "documents", "asset/0")
        .unwrap();
    assert_eq!(scoped["scope"]["id"], "asset/0");
    assert_eq!(scoped["scope"]["complete_dependency_coverage"], false);
    assert_eq!(scoped["total"], 2);
    assert_eq!(scoped["unique_content_bytes"], old.bytes + new.bytes);
    let old_scoped = scoped["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["sha256"] == old.sha256.as_str())
        .unwrap();
    assert_eq!(old_scoped["basis"], previous["items"][0]["basis"]);
    let shared_scope = c
        .work_object_inventory(&q(""), "documents", "asset/1")
        .unwrap();
    assert_eq!(shared_scope["total"], 1);
    assert_eq!(shared_scope["unique_content_bytes"], shared.bytes);
    let mut foreign = q("");
    foreign.q = "beta item";
    assert_eq!(
        c.work_object_inventory(&foreign, "documents", "asset/1")
            .unwrap()["total"],
        0
    );
    assert!(
        shared_scope["items"][0]["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["project"] == "beta")
    );
    let summaries = scoped["copy_summary"].as_array().unwrap();
    assert_eq!(
        summaries
            .iter()
            .map(|s| s["file_bytes"].as_u64().unwrap())
            .sum::<u64>(),
        old.bytes * 2 + new.bytes
    );
    let mut page = q("");
    page.offset = 1;
    let paged = c
        .work_object_inventory(&page, "documents", "asset/0")
        .unwrap();
    assert_eq!(paged["copy_summary"], scoped["copy_summary"]);
    assert_eq!(
        paged["unique_content_bytes"],
        scoped["unique_content_bytes"]
    );
    assert!(
        c.work_object_inventory(&q(""), "documents", "asset/404")
            .is_err()
    );
    assert!(c.work_object_inventory(&q(""), "other", "asset/0").is_err());
    assert!(c.work_object_inventory(&q(""), "", "asset/0").is_err());
    assert_eq!(previous["total"], 1);
    let row = &previous["items"][0];
    assert_eq!(row["sha256"], old.sha256.as_str());
    assert_eq!(row["primary"][0]["label"], "History");
    assert_eq!(row["copies"].as_array().unwrap().len(), 2);
    assert!(row["title"].as_str().unwrap().contains("alpha item"));
    let plan = c
        .object_preview(
            "alpha",
            old.sha256.as_str(),
            row["basis"].as_str().unwrap(),
            "evict-review",
        )
        .unwrap();
    assert_eq!(plan["executable"], false);
    assert_eq!(plan["safe_reclaimable_bytes"], Value::Null);
    assert!(plan["holds"].as_array().unwrap().len() >= 3);
    for copy in row["copies"].as_array().unwrap() {
        let (object, profile) = c
            .object_copy_request(
                "alpha",
                old.sha256.as_str(),
                copy["cache_id"].as_str().unwrap(),
                row["basis"].as_str().unwrap(),
            )
            .unwrap();
        let checked = git_asset::catalogue::verify(object, profile).unwrap();
        assert_eq!(checked["verified"], true);
        assert_eq!(checked["independent_custody_verified"], false);
    }
    let cache = row["copies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|copy| {
            let wanted = copy["cache_id"].as_str().unwrap();
            previous["caches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == wanted && c["root"] == fast.to_string_lossy().as_ref())
        })
        .unwrap();
    let mut observed = q("previous-revision");
    observed.observed = cache["cache_id"].as_str().unwrap();
    let filtered = c.object_inventory(&observed).unwrap();
    assert_eq!(filtered["stored_file_bytes"], old.bytes);
    assert_eq!(filtered["items"][0]["stored_file_bytes"], old.bytes * 2);
    assert_eq!(filtered["items"][0]["matching_copy_bytes"], old.bytes);
    let selection = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            format!(
                "{}:{}",
                r["sha256"].as_str().unwrap(),
                r["basis"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let batch = c
        .object_batch_preview("alpha", &selection, observed.observed, "evict-review")
        .unwrap();
    assert_eq!(batch["count"], 3);
    assert_eq!(
        batch["unique_content_bytes"],
        old.bytes + new.bytes + shared.bytes
    );
    assert_eq!(batch["older_copy_review_file_bytes"], old.bytes);
    assert_eq!(batch["removal_authorized"], false);
    assert_eq!(batch["safe_reclaimable_bytes"], Value::Null);
    let align = c
        .object_batch_preview("alpha", &selection, "", "align")
        .unwrap();
    assert_eq!(align["executable"], false);
    let first = selection.split(',').next().unwrap();
    let duplicate = format!("{first},{first}");
    assert!(
        c.object_batch_preview("alpha", &duplicate, "", "align")
            .is_err()
    );
    let stale = format!("{}:{}", first.split(':').next().unwrap(), "0".repeat(64));
    let mixed = selection.replacen(first, &stale, 1);
    assert!(
        c.object_batch_preview("alpha", &mixed, "", "align")
            .is_err()
    );
    assert!(c.object_batch_preview("alpha", "", "", "align").is_err());
    assert!(
        c.object_batch_preview("alpha", &selection, "unknown", "align")
            .is_err()
    );
    assert!(
        c.object_batch_preview("alpha", &selection, "", "delete")
            .is_err()
    );
    assert!(
        c.object_batch_preview("alpha", &first.repeat(100), "", "align")
            .is_err()
    );
    fs::write(fast.join("objects").join(old.sha256.as_str()), b"bad body").unwrap();
    let (object, profile) = c
        .object_copy_request(
            "alpha",
            old.sha256.as_str(),
            cache["cache_id"].as_str().unwrap(),
            row["basis"].as_str().unwrap(),
        )
        .unwrap();
    assert!(git_asset::catalogue::verify(object, profile).is_err());
    fs::write(fast.join("objects").join(old.sha256.as_str()), b"old body").unwrap();
    let shared_row = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["sha256"] == shared.sha256.as_str())
        .unwrap();
    assert!(
        shared_row["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["project"] == "beta")
    );
    assert!(
        c.object_preview("alpha", old.sha256.as_str(), "stale", "align")
            .is_err()
    );
    c.projects
        .get_mut("alpha")
        .unwrap()
        .change_snapshot(&f.first)
        .unwrap();
    assert!(
        c.object_batch_preview("alpha", &selection, "", "align")
            .is_err()
    );
    assert!(
        c.object_preview(
            "alpha",
            old.sha256.as_str(),
            row["basis"].as_str().unwrap(),
            "evict-review"
        )
        .is_err()
    );
    assert_eq!(
        fs::read(fast.join("objects").join(old.sha256.as_str())).unwrap(),
        b"old body"
    );
    assert_eq!(
        fs::read(history.join("objects").join(old.sha256.as_str())).unwrap(),
        b"old body"
    );
    // Revisions with unchanged bytes keep the working preference and every ref.
    record["streams"][0]["object"] = json!(new);
    fs::write(
        repo.join("records/0.asset"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let unchanged = commit(&repo);
    let path = root.join("alpha.json");
    let mut profile: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    profile["commit"] = json!(unchanged);
    fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
    config["projects"][0]["storage"]["placement_rules"].as_array_mut().unwrap().push(json!({"id":"draft-priority","label":"Admitted status preference","pattern":"^asset/0$","usages":["current-revision"],"place_index":0,"priority":10,"field":"/status/lifecycle","field_pattern":"^draft$"}));
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let c = f.load();
    let same = c.object_inventory(&q("previous-revision")).unwrap();
    let same_row = &same["items"][0];
    assert_eq!(same_row["sha256"], new.sha256.as_str());
    assert_eq!(same_row["primary"].as_array().unwrap().len(), 1);
    assert_eq!(same_row["primary"][0]["rule"], "draft-priority");
    assert!(
        same_row["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["usage"] == "current-revision")
    );
    config["projects"][0]["storage"]["placement_rules"].as_array_mut().unwrap().push(json!({"id":"conflict","label":"Conflicting equal priority","pattern":"^asset/0$","usages":["current-revision"],"place_index":1,"priority":10}));
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let conflict = f.load().object_inventory(&q("current-revision")).unwrap();
    assert!(
        conflict["items"][0]["anomalies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "policy-conflict")
    );
}

#[test]
fn global_storage_counts_shared_cache_once_and_preserves_cross_project_references() {
    let f = Fixture::new();
    let root = f.temp.path();
    let store = root.join("shared-store");
    fs::create_dir_all(store.join("objects")).unwrap();
    let bytes = b"deliberately not stored anywhere";
    let object = Object::from_bytes(bytes);
    fs::write(store.join("objects").join(object.sha256.as_str()), bytes).unwrap();
    let mut config: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    config["storage_domains"] =
        json!([{"id":"shared","label":"Shared drive","root":root,"live_probe":false}]);
    for (i, id) in ["alpha", "beta"].iter().enumerate() {
        config["projects"][i]["storage"] = json!({"schema":"tig.catalogue-storage.experimental-1","inventory":null,"encoding":"json","max_decoded_bytes":1024,"places":[{"label":"Shared cache","place_index":0,"report_tier":null,"live_probe":true}]});
        let path = root.join(format!("{id}.json"));
        let mut profile: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        profile["byte_source"] = json!({"places":[{"root":store,"layout":"flat"}],"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"}});
        fs::write(path, serde_json::to_vec(&profile).unwrap()).unwrap();
    }
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut c = f.load();
    let before = c.global_storage("", "", false, 0).unwrap();
    assert_eq!(before["caches"].as_array().unwrap().len(), 1);
    assert_eq!(before["caches"][0]["totals"], Value::Null);
    assert_eq!(before["domains"][0]["capacity_measured"], false);
    c.observe_registered("alpha", &f.first, "").unwrap();
    c.observe_registered("alpha", &f.first, "Assets").unwrap();
    assert_eq!(
        c.select_storage_scope("alpha", &f.first, "").unwrap()["cached_observation"],
        true
    );
    assert!(c.select_storage_scope("alpha", &f.second, "").is_err());
    let beta_commit = serde_json::from_slice::<Value>(&fs::read(root.join("beta.json")).unwrap())
        .unwrap()["commit"]
        .as_str()
        .unwrap()
        .to_owned();
    c.observe_registered("beta", &beta_commit, "").unwrap();
    let all = c.global_storage("", "", true, 0).unwrap();
    assert_eq!(all["summary"]["file_copies"], 1);
    assert_eq!(all["summary"]["unique_objects"], 1);
    assert_eq!(all["summary"]["recorded_file_bytes"], bytes.len());
    assert_eq!(
        all["objects"][0]["referencing_projects"],
        json!(["alpha", "beta"])
    );
    assert_eq!(
        all["caches"][0]["enrolled_projects"],
        json!(["alpha", "beta"])
    );
    assert_eq!(
        all["caches"][0]["observations"].as_array().unwrap().len(),
        3
    );
    assert_eq!(all["domains"][0]["capacity_measured"], false);
    assert_eq!(all["coverage_complete"], false);
    assert_eq!(all["safe_reclaimable_bytes"], Value::Null);
    assert_eq!(
        c.global_storage("alpha", "shared", false, 0).unwrap()["summary"]["file_copies"],
        1
    );
    assert!(c.global_storage("unknown", "", false, 0).is_err());
    assert!(c.global_storage("", "unknown", false, 0).is_err());
    assert!(c.use_retained_inventory("alpha", &f.first).is_err());
    assert_eq!(
        c.global_storage("", "", false, 0).unwrap()["summary"]["file_copies"],
        1
    );
    config["storage_domains"][0]["root"] = json!("relative");
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(Config::load(&f.config).is_err());
}

#[test]
fn registered_storage_refresh_is_explicit_scoped_and_snapshot_fenced() {
    use git_asset::catalogue::storage::Query;
    let f = Fixture::new();
    let root = f.temp.path();
    let store = root.join("store");
    fs::create_dir_all(store.join("objects")).unwrap();
    let payload = b"deliberately not stored anywhere";
    let object = Object::from_bytes(payload);
    fs::write(store.join("objects").join(object.sha256.as_str()), payload).unwrap();
    let mut config: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    config["projects"][0]["collections"] = json!([{"id":"documents","label":"Documents","icon":"document","roots":[{"pattern":"^asset/([0-9]+)$","key":"asset/${1}"}]}]);
    config["projects"][0]["storage"] = json!({"schema":"tig.catalogue-storage.experimental-1","inventory":Object::from_bytes(b"never opened during refresh"),"encoding":"json","max_decoded_bytes":1024,"places":[{"label":"Local","place_index":0,"report_tier":null,"live_probe":true},{"label":"Offline","place_index":1,"report_tier":null,"live_probe":false}]});
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let path = root.join("alpha.json");
    let mut profile: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    profile["byte_source"] = json!({"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"},"places":[{"root":store,"layout":"flat"},{"root":root.join("not-mounted"),"layout":"flat"}]});
    fs::write(path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let mut c = f.load();
    assert!(
        c.observe_registered("alpha", &f.second, "Documents")
            .is_err()
    );
    let observation = c
        .observe_registered("alpha", &f.first, "Documents")
        .unwrap();
    assert_eq!(observation["payloads_read"], 0);
    assert_eq!(observation["declared_objects"], 1);
    assert_eq!(observation["objects_not_observed"], 0);
    assert!(c.use_retained_inventory("alpha", &f.second).is_err());
    assert!(c.use_retained_inventory("alpha", &f.first).is_err());
    let q = Query {
        cache: "Local",
        family: "Documents",
        owner: "",
        version: "",
        state: "",
        pivot: "works",
        metric: "recorded_file_bytes",
        q: "",
        offset: 0,
    };
    let r = c.storage("alpha", &f.first, &q).unwrap();
    assert_eq!(r["objects_total"], 1);
    assert_eq!(r["groups_total"], 3);
    assert_eq!(r["summary"]["recorded_file_bytes"], payload.len());
    assert_eq!(r["source"]["kind"], "registered-object-metadata");
    assert_eq!(r["places"][1]["observed"], false);
    assert_eq!(r["removal_authorized"], false);
    assert!(
        c.observe_registered("alpha", &f.first, "unknown family")
            .is_err()
    );
    assert_eq!(
        c.storage("alpha", &f.first, &q).unwrap()["objects_total"],
        1
    );
    assert_eq!(
        fs::read(store.join("objects").join(object.sha256.as_str())).unwrap(),
        payload
    );
}

#[test]
fn project_scoped_identity_search_paging_and_private_projection_without_payload_io() {
    let f = Fixture::new();
    let c = f.load();
    let all = c.list("all", "", "", 0, 2).unwrap();
    assert_eq!(all["total"], 6);
    assert_eq!(all["items"].as_array().unwrap().len(), 2);
    assert_eq!(c.list("all", "", "asset/0", 0, 10).unwrap()["total"], 2);
    assert_eq!(c.list("all", "Contracts", "", 0, 10).unwrap()["total"], 3);
    assert_eq!(
        c.list("alpha", "", "ALPHA ITEM", 2, 2).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(c.list("unknown", "", "", 0, 10).is_err());
    assert!(c.list("all", "", "", 0, 201).is_err());
    let p = &c.projects["alpha"];
    let detail = p.inspect("asset/0").unwrap();
    assert_eq!(detail["card"]["title"], "alpha item 0");
    assert_eq!(detail["payload_verified"], false);
    assert!(!detail.to_string().contains("NEVER_EXPOSE"));
    assert!(p.verification("asset/0", None).is_err());
    assert_eq!(p.summary()["unique_selected_objects"], 1);
    assert_eq!(p.summary()["automatic_eviction"], false);
    assert!(!f.temp.path().join("alpha/cache").exists());
}

#[test]
fn named_snapshot_selection_is_atomic_and_stale_inspection_is_refused() {
    let f = Fixture::new();
    let mut c = f.load();
    let p = c.projects.get_mut("alpha").unwrap();
    assert!(
        p.summary()["refs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r[0] == "refs/tags/previous" && r[1] == f.first)
    );
    assert_eq!(p.dependencies("asset/0").unwrap()["supported"], false);
    p.change_snapshot(&f.second).unwrap();
    assert!(
        p.inspect("asset/0").unwrap()["card"]["title"]
            .as_str()
            .unwrap()
            .starts_with("<img")
    );
    assert!(p.check_snapshot(&f.first).is_err());
    assert!(p.change_snapshot(&"0".repeat(40)).is_err());
    p.check_snapshot(&f.second).unwrap();
    p.change_snapshot(&f.first).unwrap();
    assert_eq!(
        p.inspect("asset/0").unwrap()["card"]["title"],
        "alpha item 0"
    );
}

#[test]
fn enrollment_duplicate_fields_and_schema_violations_fail_closed() {
    let f = Fixture::new();
    let mut config: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    config["projects"][1]["id"] = json!("alpha");
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(Config::load(&f.config).is_err());
    fs::write(&f.config, br#"{"schema":"a","schema":"b","projects":[]}"#).unwrap();
    assert!(Config::load(&f.config).is_err());
    let f = Fixture::new();
    let repo = f.temp.path().join("alpha");
    let path = repo.join("records/0.asset");
    let original = git(&repo, &["show", "HEAD:records/0.asset"]).stdout;
    let mut record: Value = serde_json::from_slice(&original).unwrap();
    record["authored"]["title"] = json!(42);
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    let bad = commit(&repo);
    let mut c = f.load();
    let p = c.projects.get_mut("alpha").unwrap();
    assert!(p.change_snapshot(&bad).is_err());
    p.check_snapshot(&f.first).unwrap();
}

#[test]
fn explicit_guarded_check_verifies_exact_bytes_without_claiming_custody() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"synthetic payload";
    let object = Object::from_bytes(bytes);
    fs::create_dir(root.path().join("objects")).unwrap();
    let path = root.path().join("objects").join(object.sha256.as_str());
    fs::write(&path, bytes).unwrap();
    let required = if cfg!(windows) {
        "prevent-mutation"
    } else {
        "cooperative-immutable"
    };
    let profile: git_asset_store::existing::Profile = serde_json::from_value(
        json!({"places":[{"root":root.path(),"layout":"flat"}],"required":required}),
    )
    .unwrap();
    let result = git_asset::catalogue::verify(object.clone(), profile.clone()).unwrap();
    assert_eq!(result["verified"], true);
    assert_eq!(result["independent_custody_verified"], false);
    assert_eq!(result["authorizes_eviction"], false);
    #[cfg(not(windows))]
    {
        let mut strict = profile.clone();
        strict.required = git_asset_store::existing::RequiredProtection::PreventMutation;
        assert!(git_asset::catalogue::verify(object.clone(), strict).is_err());
    }
    fs::write(&path, b"corrupt").unwrap();
    assert!(git_asset::catalogue::verify(object, profile).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"corrupt");
}

#[test]
fn work_counts_ignore_renditions_orphans_and_same_byte_runs() {
    let f = Fixture::new();
    let repo = f.temp.path().join("alpha");
    let mut root: Value =
        serde_json::from_slice(&git(&repo, &["show", "HEAD:records/0.asset"]).stdout).unwrap();
    root["authored"]["version"] = json!("r2");
    let payload = root["selected"].clone();
    root["streams"] = json!([{"slot":"run/r1","role":"result","group":"retained","object":payload},{"slot":"run/r2","role":"result","group":"current","object":payload}]);
    write(&repo, "records/0.asset", &root);
    let mut stale = root.clone();
    stale["id"] = json!("asset/1");
    stale["streams"]
        .as_array_mut()
        .unwrap()
        .retain(|s| s["slot"] == "run/r1");
    write(&repo, "records/1.asset", &stale);
    let mut planned = root.clone();
    planned["id"] = json!("asset/2");
    planned["selected"] = Value::Null;
    planned["streams"] = json!([]);
    write(&repo, "records/2.asset", &planned);
    for (n, id) in [
        ("en", "asset/0/delivery/en"),
        ("es", "asset/0/delivery/es"),
        ("cue", "asset/0/input/cue"),
        ("orphan", "asset/99/delivery/en"),
    ] {
        let mut child = root.clone();
        child["id"] = json!(id);
        write(&repo, &format!("records/{n}.asset"), &child);
    }
    let head = commit(&repo);
    let profile_path = f.temp.path().join("alpha.json");
    let mut profile: Value = serde_json::from_slice(&fs::read(&profile_path).unwrap()).unwrap();
    profile["commit"] = json!(head);
    fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let mut config: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    config["default_project"] = json!("alpha");
    config["projects"][0]["collections"] = json!([{"id":"works","label":"Works","icon":"document","roots":[{"pattern":"^asset/([0-9]+)$","key":"asset/${1}"}],"members":[{"pattern":"^asset/([0-9]+)/","key":"asset/${1}"}],"stages":[{"label":"Accepted","pointer":"/authored/version","pattern":"^r2$","selected_slot_prefix":"run/"}],"count_state":"Accepted","version_pointer":"/authored/version","version_role":"result","version_slot_prefix":"run/","delivery_pattern":"^asset/[0-9]+/delivery/","facets":[{"label":"First","pattern":"^asset/0$"}]}]);
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let c = f.load();
    assert_eq!(c.summaries()["default_project"], "alpha");
    let summary = c.projects["alpha"].summary();
    assert_eq!(summary["work_count"], 3);
    assert_eq!(summary["collections"][0]["count_value"], 1);
    assert_eq!(
        c.work_selection("alpha", "works", "", 0).unwrap()["total"],
        1
    );
    for bad in ["", "unknown", "works,works", "works,"] {
        assert!(c.work_selection("alpha", bad, "", 0).is_err());
    }
    let rows = c
        .work_list(("alpha", "works"), "Accepted", "First", "", 0)
        .unwrap();
    assert_eq!(rows["total"], 1);
    assert_eq!(rows["items"][0]["id"], "asset/0");
    let detail = c
        .work_detail(("alpha", "works"), "asset/0", &head, "", 0)
        .unwrap();
    assert_eq!(detail["deliveries"].as_array().unwrap().len(), 2);
    assert_eq!(detail["member_total"], 4);
    assert_eq!(
        detail["versions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["current"] == true)
            .count(),
        1
    );
    assert!(
        c.work_detail(("alpha", "works"), "asset/99", &head, "", 0)
            .is_err()
    );
    assert!(
        c.work_detail(("alpha", "works"), "asset/0", &f.first, "", 0)
            .is_err()
    );
    assert_eq!(
        c.work_list(("alpha", "works"), "", "", "", 2).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    config["projects"][0]["collections"][0]["roots"][0]["pattern"] = json!("[bad");
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(Config::load(&f.config).is_err());
}

#[test]
fn nested_navigation_and_cross_project_search_preserve_work_counts_and_scope() {
    let f = Fixture::new();
    let repo = f.temp.path().join("alpha");
    let root: Value =
        serde_json::from_slice(&git(&repo, &["show", "HEAD:records/0.asset"]).stdout).unwrap();
    write(&repo, "records/0.asset", &root);
    for language in ["en", "es"] {
        let mut record = root.clone();
        record["id"] = json!(format!("asset/0/delivery/{language}"));
        write(&repo, &format!("records/{language}.asset"), &record);
    }
    let head = commit(&repo);
    let path = f.temp.path().join("alpha.json");
    let mut profile: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    profile["commit"] = json!(head);
    fs::write(path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let mut config: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    let top = json!({"id":"works","label":"Works","icon":"folder","roots":[{"pattern":"^asset/([0-9]+)$","key":"asset/${1}"}]});
    let child = json!({"id":"renditions","label":"Renditions","icon":"audio","top_level":false,"roots":[{"definition":"type/item","pattern":"^asset/([0-9]+)/delivery/(?:en|es)$","key":"asset/${1}/delivery"}],"parent":{"pattern":"^asset/([0-9]+)/","key":"asset/${1}"}});
    config["projects"][0]["collections"] = json!([top, child]);
    config["projects"][1]["collections"] = json!([top]);
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let c = f.load();
    assert_eq!(c.projects["alpha"].summary()["work_count"], 3);
    let page = c
        .children(("alpha", "works"), "asset/0", &head, "", 0)
        .unwrap();
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["member_count"], 2);
    let child = c
        .work_detail(("alpha", "renditions"), "asset/0/delivery", &head, "", 0)
        .unwrap();
    assert_eq!(child["parents"][0]["id"], "asset/0");
    assert_eq!(
        c.children(("alpha", "works"), "asset/0", &head, "renditions", 1)
            .unwrap()["items"],
        json!([])
    );
    assert!(
        c.children(("alpha", "works"), "asset/0", &f.first, "", 0)
            .is_err()
    );
    assert_eq!(c.search("all", "works", "asset/0", 0).unwrap()["total"], 2);
    assert_eq!(
        c.search("alpha", "renditions", "asset/0", 0).unwrap()["total"],
        1
    );
    assert_eq!(
        c.search("all", "works", "asset/0", 1).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(c.search("all", "", "", 0).is_err());
    assert!(c.search("missing", "", "asset", 0).is_err());
}
