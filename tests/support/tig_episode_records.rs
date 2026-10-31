//! Real parent record candidate -> retained proposal -> explicit Git adoption.
use super::*;
pub(super) struct Fixture {
    pub specs: Value,
    original: Vec<u8>,
}
impl Fixture {
    pub fn handoff(&self, p: &Project, base: &str) -> Value {
        json!({"git_executable":git_executable(),"repository":p.repo.path(),"base":base,
            "identity":{"name":"Fixture","email":"fixture@example.invalid","unix_seconds":1700000000},
            "message":"adopt checked episode candidate","paths":{"episode:delivery:es":"records/episode-delivery.tig"},
            "protection":"prevent-mutation"})
    }
}
pub(super) fn configure(p: &mut Project) -> Fixture {
    let schema =
        Object::from_bytes(&fs::read(p.repo.path().join("definitions/authored.json")).unwrap());
    let definition = write(
        p.repo.path(),
        "definitions/parent-output.json",
        &json!({
            "schema":"tig.definition.design-1","id":"types/parent-output","authored_schema":schema,
            "authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{},
        }),
    );
    p.profile["admitted"].as_array_mut().unwrap().push(json!({
        "id":"types/parent-output","object":definition,"payload":{"kind":"selected"},
    }));
    write(
        p.repo.path(),
        "records/episode-delivery.tig",
        &json!({
            "schema":"tig.instance.design-1","id":"episode:delivery:es",
            "definition":{"id":"types/parent-output","sha256":definition.sha256,"bytes":definition.bytes},
            "authored":{},"selected":null,"streams":[{"slot":"previous","role":"delivery","group":"history","object":Object::from_bytes(b"old bytes deliberately absent")}],
            "status":{"approved":true},"metadata":{"old_review":true},
        }),
    );
    let original = fs::read(p.repo.path().join("records/episode-delivery.tig")).unwrap();
    Fixture {
        specs: json!([{"output":"master.mkv","template":{"id":"episode:delivery:es","object":Object::from_bytes(&original)},"selector":{"kind":"selected"}}]),
        original,
    }
}
pub(super) fn check(p: &Project, result: &Value, base: &str, fixture: &Fixture) {
    let outputs = &result["result"]["outputs"];
    let index = read(outputs["record-candidates.json"]["path"].as_str().unwrap());
    let record = &index["records"][0];
    let candidate = read(
        outputs[record["path"].as_str().unwrap()]["path"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(
        candidate["selected"]["sha256"],
        outputs["native/master.mkv"]["sha256"]
    );
    assert_eq!(
        candidate["selected"]["bytes"],
        outputs["native/master.mkv"]["bytes"]
    );
    assert_eq!(candidate["status"], json!({}));
    assert!(candidate["metadata"].get("old_review").is_none());
    let original: Value = serde_json::from_slice(&fixture.original).unwrap();
    assert_eq!(candidate["streams"], original["streams"]);
    assert_eq!(candidate["authored"], original["authored"]);
    assert_eq!(candidate["metadata"]["production"]["claim"], index["claim"]);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            p.repo.path().join(format!("offline-{name}")),
            p.repo.path().join(name),
        )
        .unwrap();
    }
    let path = outputs["git-proposal.json"]["path"].as_str().unwrap();
    let proposal = read(path);
    assert_eq!(proposal["base"], base);
    assert_eq!(
        proposal["byte_source"]["places"],
        json!([{"root":p.hot(),"layout":"reel-cas"}])
    );
    assert_eq!(
        proposal["changes"][0]["expected"],
        json!(Object::from_bytes(&fixture.original))
    );
    assert_eq!(proposal["changes"][0]["object"], record["object"]);
    let index_before = fs::read(p.repo.path().join(".git/index")).unwrap();
    let tool = std::env::var_os("REEL_TEST_TIG_PROPOSE")
        .expect("explicit pinned Tig proposal executable required");
    let run = |mode: &str| {
        let out = Command::new(&tool).arg(mode).arg(path).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    assert!(run("check")["proposal"].is_null());
    let created = run("create");
    assert_eq!(run("create")["proposal"], created["proposal"]);
    assert_eq!(git(p.repo.path(), &["rev-parse", "HEAD"]), base);
    assert_eq!(
        fs::read(p.repo.path().join(".git/index")).unwrap(),
        index_before
    );
    assert_eq!(
        fs::read(p.repo.path().join("records/episode-delivery.tig")).unwrap(),
        fixture.original
    );
    git(
        p.repo.path(),
        &[
            "cherry-pick",
            created["proposal"]["commit"].as_str().unwrap(),
        ],
    );
    assert_eq!(
        read(p.repo.path().join("records/episode-delivery.tig")),
        candidate
    );
    assert_eq!(
        git(
            p.repo.path(),
            &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]
        ),
        "records/episode-delivery.tig"
    );
}
