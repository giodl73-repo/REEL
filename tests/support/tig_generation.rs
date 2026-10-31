use super::*;
use reel::tig_generation::{self, Request};
use tig_assets::graph::{
    Reference,
    recipe::produced::{self, BoundSubject, Claim, ContextPin, Correspondence, Selector},
};
use tig_core::tree::{MemoryNodes, TreeLimits};

fn cache(f: &Fixture, bytes: &[u8]) -> Object {
    let pin = Object::from_bytes(bytes);
    fs::write(
        f.root().join("cache/objects").join(pin.sha256.as_str()),
        bytes,
    )
    .unwrap();
    pin
}
fn record(f: &Fixture, id: &str) -> Value {
    serde_json::from_slice(
        &fs::read(
            f.root()
                .join(format!("records/{}.tig", id.replace('/', "-"))),
        )
        .unwrap(),
    )
    .unwrap()
}
fn save(f: &Fixture, id: &str, value: &Value) {
    write(
        f.root(),
        &format!("records/{}.tig", id.replace('/', "-")),
        value,
    );
}
fn commit(f: &mut Fixture) {
    git(f.root(), &["add", "records", "definitions"]);
    git(f.root(), &["commit", "-m", "generation fixture edit"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
}
fn setup() -> (Fixture, Request, Request) {
    let mut f = Fixture::new();
    let schema = Object::from_bytes(&fs::read(f.root().join("definitions/authored.json")).unwrap());
    let mut defs = BTreeMap::new();
    for (kind, property) in [
        ("generation-spec", "document"),
        ("generation-profile", "document"),
        ("generated", "generation_spec"),
    ] {
        let pin = write(
            f.root(),
            &format!("definitions/{kind}.json"),
            &json!({"schema":"tig.definition.design-1","id":format!("types/{kind}"),"authored_schema":schema,"authoring":{"properties":[property],"required":[property],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
        );
        f.profile["admitted"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":format!("types/{kind}"),"object":pin,"payload":{"kind":"selected"}}));
        defs.insert(kind, pin);
    }
    let media = Object::from_bytes(&fs::read(f.root().join("definitions/media.json")).unwrap());
    for (id, kind, authored, selected) in [
        (
            "text/es",
            "media",
            json!({}),
            Some(cache(&f, b"Buenos dias")),
        ),
        (
            "text/en",
            "generated",
            json!({"generation_spec":{"id":"generation/translation"}}),
            Some(cache(&f, b"Good day")),
        ),
        (
            "cue/en",
            "generated",
            json!({"generation_spec":{"id":"generation/speech"}}),
            Some(cache(&f, b"synthetic recorded cue bytes")),
        ),
        (
            "profile/text",
            "generation-profile",
            json!({"document":{"media_type":"text/plain","max_bytes":4096}}),
            None,
        ),
        (
            "profile/audio",
            "generation-profile",
            json!({"document":{"media_type":"audio/wav","sample_rate":48000,"max_seconds":3}}),
            None,
        ),
        (
            "generation/translation",
            "generation-spec",
            json!({"document":{"schema":"reel.generation-spec.v1","operation":"text.translate","inputs":{"source":{"id":"text/es"}},"output_profile":{"id":"profile/text"},"parameters":{"source_language":"es","target_language":"en"}}}),
            None,
        ),
        (
            "generation/speech",
            "generation-spec",
            json!({"document":{"schema":"reel.generation-spec.v1","operation":"audio.record","inputs":{"text":{"id":"text/en"}},"output_profile":{"id":"profile/audio"},"parameters":{"language":"en","voice":"synthetic-fixture"}}}),
            None,
        ),
    ] {
        let pin = if kind == "media" { &media } else { &defs[kind] };
        save(
            &f,
            id,
            &json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":format!("types/{kind}"),"sha256":pin.sha256,"bytes":pin.bytes},"authored":authored,"selected":selected,"streams":[],"status":{"selection_state":"selected-private-production"},"metadata":{}}),
        );
    }
    commit(&mut f);
    let req = |output, spec, operation| {
        serde_json::from_value(json!({"schema":"reel.tig-generation-request.experimental-1","output_id":output,"output_definition":"types/generated","spec_id":spec,"spec_definition":"types/generation-spec","profile_definition":"types/generation-profile","primary_definitions":["types/media","types/generated"],"operation":operation,"allowed_input_states":["selected-private-production"]})).unwrap()
    };
    (
        f,
        req("text/en", "generation/translation", "text.translate"),
        req("cue/en", "generation/speech", "audio.record"),
    )
}
fn plan(f: &Fixture, r: &Request) -> tig_assets::graph::recipe::Plan {
    let mut source = f.source();
    let result = tig_generation::recipe(
        &mut source,
        r,
        &Object::from_bytes(b"independently admitted synthetic generator"),
    )
    .unwrap();
    assert_eq!(source.byte_requests(), 0);
    result
}

#[test]
fn generation_basis_changes_for_text_and_parameters_not_selected_results_or_annotations() {
    let (mut f, mut translation, speech) = setup();
    let before = plan(&f, &translation);
    let cue_before = plan(&f, &speech);
    let mut output = record(&f, "text/en");
    output["selected"] = json!(cache(&f, b"New selected translation bytes"));
    save(&f, "text/en", &output);
    commit(&mut f);
    let selected = plan(&f, &translation);
    assert_eq!(before.fingerprint(), selected.fingerprint());
    assert_ne!(before.checkpoint(), selected.checkpoint());
    assert_ne!(cue_before.fingerprint(), plan(&f, &speech).fingerprint());
    let mut spec = record(&f, "generation/translation");
    spec["metadata"]["review_note"] = json!("Annotation only");
    save(&f, "generation/translation", &spec);
    commit(&mut f);
    assert_eq!(selected.fingerprint(), plan(&f, &translation).fingerprint());
    translation
        .allowed_input_states
        .insert("another-admitted-state".into());
    assert_eq!(selected.fingerprint(), plan(&f, &translation).fingerprint());
    let mut source = record(&f, "text/es");
    source["selected"] = json!(cache(&f, b"Buenas noches"));
    save(&f, "text/es", &source);
    commit(&mut f);
    let changed = plan(&f, &translation);
    assert_ne!(selected.fingerprint(), changed.fingerprint());
    // The cue still consumes the old English bytes. Upstream stale-generation
    // eligibility must be evaluated separately; a direct match is not permission.
    let cue = plan(&f, &speech);
    let source = f.source();
    assert!(
        tig_generation::upstream(&source, &translation)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        tig_generation::upstream(&source, &speech).unwrap(),
        std::collections::BTreeSet::from(["text/en".into()])
    );
    use tig_assets::graph::recipe::assessment::State;
    use tig_generation::advice::{Node, order};
    let checkpoint = source.checkpoint().clone();
    let advice = order(
        &checkpoint,
        &[
            Node {
                asset_id: "text/en".into(),
                checkpoint: checkpoint.clone(),
                basis: Some(State::StaleRecipe),
                upstream: std::collections::BTreeSet::new(),
            },
            Node {
                asset_id: "cue/en".into(),
                checkpoint: checkpoint.clone(),
                basis: Some(State::ClaimedRecipeMatch),
                upstream: tig_generation::upstream(&source, &speech).unwrap(),
            },
        ],
        2,
    )
    .unwrap();
    assert_eq!(advice[1].asset_id, "cue/en");
    assert_eq!(advice[1].advice, State::Blocked);
    assert_eq!(source.byte_requests(), 0);
    spec["authored"]["document"]["parameters"]["target_language"] = json!("en-GB");
    save(&f, "generation/translation", &spec);
    commit(&mut f);
    assert_ne!(changed.fingerprint(), plan(&f, &translation).fingerprint());
    assert_eq!(cue.fingerprint(), plan(&f, &speech).fingerprint());
    let before_profile = plan(&f, &speech);
    let mut profile = record(&f, "profile/audio");
    profile["authored"]["document"]["sample_rate"] = json!(44100);
    save(&f, "profile/audio", &profile);
    commit(&mut f);
    assert_ne!(
        before_profile.fingerprint(),
        plan(&f, &speech).fingerprint()
    );
    let mut source = f.source();
    assert_ne!(
        plan(&f, &speech).fingerprint(),
        tig_generation::recipe(
            &mut source,
            &speech,
            &Object::from_bytes(b"different generator version")
        )
        .unwrap()
        .fingerprint()
    );
}

#[test]
fn generation_claim_replays_archived_source_and_reports_stale_without_new_execution() {
    let (mut f, request, _) = setup();
    let producer = Object::from_bytes(b"independently admitted synthetic generator");
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let context =
        tig_generation::freeze_context(&mut source, &request, &mut nodes, TreeLimits::default())
            .unwrap();
    let basis = tig_generation::recipe(&mut source, &request, &producer).unwrap();
    let definition = Reference {
        id: "types/generated".into(),
        object: serde_json::from_value(
            f.profile["admitted"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["id"] == "types/generated")
                .unwrap()["object"]
                .clone(),
        )
        .unwrap(),
    };
    let subject = BoundSubject::resolve(
        &source_record(&f, "text/en"),
        &definition,
        Selector::Selected {},
    )
    .unwrap()
    .unwrap();
    // Unsigned metadata fixture, never an assertion that a real generator ran.
    let claim = Claim::create(
        &basis,
        ContextPin {
            root: context.selection().root.clone(),
            identity: context.selection().context.clone(),
        },
        vec![subject.subject().clone()],
    )
    .unwrap();
    let archive:reel::tig_inputs::ArchiveProfile=serde_json::from_value(json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":context.archive(),"checkpoint":source.checkpoint(),"admitted":f.profile["admitted"],"byte_source":f.profile["byte_source"]})).unwrap();
    let mut spanish = record(&f, "text/es");
    spanish["selected"] = json!(cache(&f, b"Changed Spanish source"));
    save(&f, "text/es", &spanish);
    commit(&mut f);
    let desired = plan(&f, &request);
    for dir in [".git", "records", "definitions"] {
        fs::rename(f.root().join(dir), f.root().join(format!("offline-{dir}"))).unwrap();
    }
    for case in [
        "asset",
        "definition",
        "definition-pin",
        "selector",
        "multiple",
    ] {
        let mut wire: Value = serde_json::from_slice(claim.bytes()).unwrap();
        match case {
            "asset" => wire["outputs"][0]["asset_id"] = json!("other/output"),
            "definition" => wire["outputs"][0]["definition"]["id"] = json!("types/media"),
            "definition-pin" => {
                wire["outputs"][0]["definition"]["object"] =
                    json!(Object::from_bytes(b"unadmitted definition"))
            }
            "selector" => {
                wire["outputs"][0]["selector"] = json!(Selector::Stream {
                    slot: "past".into()
                })
            }
            _ => {
                let mut other = wire["outputs"][0].clone();
                other["asset_id"] = json!("other/output");
                wire["outputs"].as_array_mut().unwrap().push(other);
            }
        }
        let raw = serde_json::to_vec(&wire).unwrap();
        let contradictory = Claim::parse(&Object::from_bytes(&raw), &raw).unwrap();
        assert!(
            tig_generation::restore_claim(
                archive.clone(),
                &request,
                &producer,
                &contradictory,
                &mut nodes,
                TreeLimits::default()
            )
            .is_err(),
            "{case}"
        );
    }
    let (reopened, replayed) = tig_generation::restore_claim(
        archive,
        &request,
        &producer,
        &claim,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(reopened.byte_requests(), 0);
    assert_eq!(
        produced::compare(&basis, Some(&subject), Some(replayed.checked())).unwrap(),
        Correspondence::ClaimedRecipeMatch
    );
    assert_eq!(
        produced::compare(&desired, Some(&subject), Some(replayed.checked())).unwrap(),
        Correspondence::StaleRecipe
    );
    assert_eq!(
        produced::compare(&desired, Some(&subject), None).unwrap(),
        Correspondence::Unproven
    );
}
fn source_record(f: &Fixture, id: &str) -> tig_assets::resolve::Resolved {
    use tig_assets::resolve::AssetResolver;
    let profile: reel::tig_inputs::Profile = serde_json::from_value(f.profile.clone()).unwrap();
    let repo = tig_git::source::Repository::new(
        profile.git_executable,
        profile.repository,
        Default::default(),
    )
    .unwrap();
    let view = repo
        .commit(tig_git::source::GitId::parse(&profile.commit).unwrap())
        .unwrap();
    let snapshot = repo
        .native(
            &view,
            tig_git::source::Scope::new(vec![tig_core::LogicalPath::parse("records").unwrap()])
                .unwrap(),
            tig_git::source::Scope::new(vec![tig_core::LogicalPath::parse("definitions").unwrap()])
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    snapshot.resolve_id(id).unwrap()
}

#[test]
fn self_reference_unknown_controls_and_unadmitted_inputs_refuse_without_payload_reads() {
    for case in ["self", "unknown", "state", "type", "planned", "output-role"] {
        let (mut f, mut r, _) = setup();
        let mut spec = record(&f, "generation/translation");
        match case {
            "self" => spec["authored"]["document"]["inputs"]["source"]["id"] = json!("text/en"),
            "unknown" => spec["authored"]["document"]["future"] = json!(true),
            "output-role" => {
                r.primary_definitions.remove("types/generated");
            }
            _ => {
                let mut input = record(&f, "text/es");
                if case == "state" {
                    input["status"]["selection_state"] = json!("not-admitted");
                } else if case == "planned" {
                    input["selected"] = Value::Null;
                } else {
                    input["definition"] = record(&f, "profile/text")["definition"].clone();
                    input["authored"] = json!({"document":{}});
                }
                save(&f, "text/es", &input);
            }
        }
        save(&f, "generation/translation", &spec);
        if case != "output-role" {
            commit(&mut f);
        }
        let mut source = f.source();
        assert!(
            tig_generation::recipe(&mut source, &r, &Object::from_bytes(b"producer")).is_err(),
            "{case}"
        );
        assert_eq!(source.byte_requests(), 0);
    }
}

#[test]
#[cfg(feature = "tig-publication")]
fn generation_inspection_cli_uses_archived_metadata_and_keeps_missing_basis_unproven() {
    use tig_store::metadata::{Limits, MetadataStore};
    let (f, request, _) = setup();
    let mut source = f.source();
    let metadata = f.root().join("generation-metadata");
    let mut store = MetadataStore::create(
        &metadata,
        Limits {
            nodes: 16384,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let context =
        tig_generation::freeze_context(&mut source, &request, &mut store, TreeLimits::default())
            .unwrap();
    let profile = json!({"schema":"reel.tig-generation-inspection.experimental-1","metadata_root":metadata,"metadata_minimum":store.finish().unwrap(),"source":{"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":context.archive(),"checkpoint":source.checkpoint(),"admitted":f.profile["admitted"],"byte_source":f.profile["byte_source"]},"request":request,"producer":Object::from_bytes(b"synthetic generator"),"original":null});
    drop(store);
    drop(source);
    let path = f.root().join("generation-inspection.json");
    fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    fs::rename(
        f.root().join("cache/objects"),
        f.root().join("cache/offline-objects"),
    )
    .unwrap();
    let report = f.root().join("generation-report.json");
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("inspect-generation")
            .arg(&path)
            .arg(&report)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    let result = invoke();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["state"], "unproven");
    assert_eq!(summary["primary_payloads_verified"], false);
    assert_eq!(summary["authorizes_execution"], false);
    assert!(!String::from_utf8_lossy(&result.stdout).contains("text/en"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("sha256"));
    let original = fs::read(&report).unwrap();
    let denied = invoke();
    assert!(!denied.status.success());
    assert_eq!(fs::read(&report).unwrap(), original);
    assert_eq!(
        String::from_utf8_lossy(&denied.stderr).trim(),
        "generation inspection refused; verify source, definitions, request and original evidence"
    );
}

#[test]
#[cfg(feature = "tig-publication")]
fn generation_git_inspection_uses_exact_commit_without_archive_or_primary_bytes() {
    let (f, request, _) = setup();
    fs::rename(
        f.root().join("cache/objects"),
        f.root().join("cache/offline-objects"),
    )
    .unwrap();
    let path = f.root().join("git-inspection.json");
    let mut profile = json!({"schema":"reel.tig-generation-inspection.experimental-1","source":f.profile,"request":request,"producer":Object::from_bytes(b"synthetic generator")});
    let invoke = |profile: &Value, name: &str| {
        fs::write(&path, serde_json::to_vec(profile).unwrap()).unwrap();
        let report = f.root().join(name);
        let result = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("inspect-generation")
            .arg(&path)
            .arg(&report)
            .env("PATH", "")
            .output()
            .unwrap();
        (result, report)
    };
    let (first, report) = invoke(&profile, "git-first.json");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let before: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(before["state"], "unproven");
    assert_eq!(before["primary_payloads_verified"], false);
    let mut spec = record(&f, "generation/translation");
    spec["authored"]["document"]["parameters"]["target_language"] = json!("en-GB");
    save(&f, "generation/translation", &spec);
    let (dirty, report) = invoke(&profile, "git-dirty.json");
    assert!(dirty.status.success());
    let dirty: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(before, dirty);
    // Commit explicitly without borrowing the whole fixture mutably while the
    // invocation closure retains its immutable root reference.
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "committed generation change"]);
    profile["source"]["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let (changed, report) = invoke(&profile, "git-changed.json");
    assert!(changed.status.success());
    let changed: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_ne!(before["desired_recipe"], changed["desired_recipe"]);
    for case in ["partial-authority", "mixed-source", "unadmitted-original"] {
        let mut invalid = profile.clone();
        match case {
            "partial-authority" => invalid["metadata_root"] = json!(f.root()),
            "mixed-source" => invalid["source"]["archive"] = json!(Object::from_bytes(b"archive")),
            _ => {
                invalid["original"] = json!({"source":{},"request":request,"producer":Object::from_bytes(b"producer"),"claim":Object::from_bytes(b"claim")})
            }
        }
        let (denied, report) = invoke(&invalid, &format!("{case}.json"));
        assert!(!denied.status.success(), "{case}");
        assert!(!report.exists());
        assert_eq!(
            String::from_utf8_lossy(&denied.stderr).trim(),
            "generation inspection refused; verify source, definitions, request and original evidence"
        );
    }
}

#[test]
#[cfg(feature = "tig-publication")]
fn generation_git_inspection_replays_claim_from_original_provider() {
    use tig_store::metadata::{Limits, MetadataStore};
    let (mut f, request, _) = setup();
    let producer = Object::from_bytes(b"synthetic generator");
    let metadata = f.root().join("generation-metadata");
    let mut store = MetadataStore::create(
        &metadata,
        Limits {
            nodes: 16384,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let mut source = f.source();
    let context =
        tig_generation::freeze_context(&mut source, &request, &mut store, TreeLimits::default())
            .unwrap();
    let basis = tig_generation::recipe(&mut source, &request, &producer).unwrap();
    let target = source_record(&f, "text/en");
    let subject = BoundSubject::resolve(
        &target,
        &Reference {
            id: "types/generated".into(),
            object: Object::from_bytes(
                &fs::read(f.root().join("definitions/generated.json")).unwrap(),
            ),
        },
        Selector::Selected {},
    )
    .unwrap()
    .unwrap();
    let claim = Claim::create(
        &basis,
        ContextPin {
            root: context.selection().root.clone(),
            identity: context.selection().context.clone(),
        },
        vec![subject.subject().clone()],
    )
    .unwrap();
    let pin = cache(&f, claim.bytes());
    let original = json!({"source":{"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":context.archive(),"checkpoint":source.checkpoint(),"admitted":f.profile["admitted"],"byte_source":f.profile["byte_source"]},"request":request,"producer":producer,"claim":pin});
    let minimum = store.finish().unwrap();
    drop(store);
    drop(source);
    // Current providers contain neither old claims nor any primary payloads.
    let current = f.root().join("current-cache");
    fs::create_dir(&current).unwrap();
    f.profile["byte_source"]["places"][0]["root"] = json!(current);
    let path = f.root().join("inspection.json");
    for (name, expected) in [
        ("matching", "claimed_recipe_match"),
        ("changed", "stale_recipe"),
    ] {
        if name == "changed" {
            let mut spec = record(&f, "generation/translation");
            spec["authored"]["document"]["parameters"]["target_language"] = json!("en-GB");
            save(&f, "generation/translation", &spec);
            commit(&mut f);
        }
        let profile = json!({"schema":"reel.tig-generation-inspection.experimental-1","source":f.profile,"metadata_root":metadata,"metadata_minimum":minimum,"request":request,"producer":producer,"original":original});
        fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("inspect-generation")
            .arg(&path)
            .arg(f.root().join(format!("{name}.json")))
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(summary["state"], expected);
        assert_eq!(summary["authorizes_execution"], false);
    }
    let invalid = json!({"schema":"reel.tig-generation-inspection.experimental-1","source":f.profile,"request":request,"producer":producer,"original":original});
    fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    let report = f.root().join("no-authority.json");
    let refused = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .arg("inspect-generation")
        .arg(&path)
        .arg(&report)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(!report.exists());
    assert_eq!(
        String::from_utf8_lossy(&refused.stderr).trim(),
        "generation inspection refused; verify source, definitions, request and original evidence"
    );
}

#[test]
#[cfg(feature = "tig-publication")]
fn generation_set_cli_blocks_stale_or_missing_translation_before_matching_cue() {
    use tig_store::metadata::{Limits, MetadataStore};
    let (f, translation, speech) = setup();
    let producer = Object::from_bytes(b"synthetic generator");
    let metadata = f.root().join("set-metadata");
    let mut store = MetadataStore::create(
        &metadata,
        Limits {
            nodes: 16384,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let definition = Reference {
        id: "types/generated".into(),
        object: Object::from_bytes(&fs::read(f.root().join("definitions/generated.json")).unwrap()),
    };
    let mut targets = Vec::new();
    let mut claims = Vec::new();
    for request in [&speech, &translation] {
        let mut source = f.source();
        let context =
            tig_generation::freeze_context(&mut source, request, &mut store, TreeLimits::default())
                .unwrap();
        let basis = tig_generation::recipe(&mut source, request, &producer).unwrap();
        let subject = BoundSubject::resolve(
            &source_record(&f, &request.output_id),
            &definition,
            Selector::Selected {},
        )
        .unwrap()
        .unwrap();
        let claim = Claim::create(
            &basis,
            ContextPin {
                root: context.selection().root.clone(),
                identity: context.selection().context.clone(),
            },
            vec![subject.subject().clone()],
        )
        .unwrap();
        let pin = Object::from_bytes(claim.bytes());
        claims.push((pin.clone(), claim.bytes().to_vec()));
        let archive = json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":context.archive(),"checkpoint":source.checkpoint(),"admitted":f.profile["admitted"],"byte_source":f.profile["byte_source"]});
        targets.push(json!({"request":request,"producer":producer,"original":{"source":archive,"request":request,"producer":producer,"claim":pin}}));
    }
    let minimum = store.finish().unwrap();
    drop(store);
    fs::rename(
        f.root().join("cache/objects"),
        f.root().join("cache/offline-primary-objects"),
    )
    .unwrap();
    fs::create_dir(f.root().join("cache/objects")).unwrap();
    for (pin, raw) in claims {
        fs::write(
            f.root().join("cache/objects").join(pin.sha256.as_str()),
            raw,
        )
        .unwrap();
    }
    let path = f.root().join("set-profile.json");
    let invoke = |profile: &Value, name: &str| {
        fs::write(&path, serde_json::to_vec(profile).unwrap()).unwrap();
        let report = f.root().join(format!("{name}.json"));
        let result = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("inspect-generation-set")
            .arg(&path)
            .arg(&report)
            .env("PATH", "")
            .output()
            .unwrap();
        (result, report)
    };
    let mut profile = json!({"schema":"reel.tig-generation-set-inspection.experimental-1","source":f.profile,"metadata_root":metadata,"metadata_minimum":minimum,"targets":targets});
    let (result, report) = invoke(&profile, "matching-set");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let matching: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(matching["entries"][0]["output_id"], "text/en");
    assert_eq!(matching["entries"][1]["output_id"], "cue/en");
    assert_eq!(matching["entries"][1]["advice"], "claimed_recipe_match");
    assert_eq!(matching["primary_payloads_verified"], false);
    assert_eq!(matching["authorizes_execution"], false);
    let preserved = fs::read(&report).unwrap();
    assert!(!invoke(&profile, "matching-set").0.status.success());
    assert_eq!(preserved, fs::read(&report).unwrap());
    // Change source text without touching the English selection or cue.
    let mut spanish = record(&f, "text/es");
    spanish["selected"] = json!(Object::from_bytes(b"changed Spanish bytes unavailable"));
    save(&f, "text/es", &spanish);
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "upstream source change"]);
    profile["source"]["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let (result, report) = invoke(&profile, "stale-set");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stale: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(stale["entries"][0]["basis"], "stale_recipe");
    assert_eq!(stale["entries"][0]["catch_up_candidate"], true);
    assert_eq!(stale["entries"][1]["basis"], "claimed_recipe_match");
    assert_eq!(stale["entries"][1]["advice"], "blocked");
    assert_eq!(stale["entries"][1]["catch_up_candidate"], false);
    for case in ["missing", "unproven"] {
        let mut p = profile.clone();
        if case == "missing" {
            p["targets"].as_array_mut().unwrap().pop();
        } else {
            p["targets"][1]["original"] = Value::Null;
        }
        let (result, report) = invoke(&p, case);
        assert!(result.status.success());
        assert!(!String::from_utf8_lossy(&result.stdout).contains("text/en"));
        assert!(!String::from_utf8_lossy(&result.stdout).contains("sha256"));
        let report: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(
            report["entries"].as_array().unwrap().last().unwrap()["advice"],
            "blocked"
        );
        if case == "missing" {
            assert_eq!(report["missing_upstream"], json!(["text/en"]));
            assert_eq!(report["declared_upstream_scope_complete"], false);
        }
    }
    for case in ["empty", "duplicate", "unknown", "too-many", "bad-original"] {
        let mut p = profile.clone();
        match case {
            "empty" => p["targets"] = json!([]),
            "duplicate" => {
                let target = p["targets"][0].clone();
                p["targets"].as_array_mut().unwrap().push(target);
            }
            "unknown" => p["future"] = json!(true),
            "too-many" => p["targets"] = json!(vec![p["targets"][0].clone(); 129]),
            _ => {
                p["targets"][0]["original"]["claim"] =
                    json!(Object::from_bytes(b"unavailable claim"))
            }
        }
        let (result, report) = invoke(&p, case);
        assert!(!result.status.success(), "{case}");
        assert!(!report.exists());
        assert_eq!(
            String::from_utf8_lossy(&result.stderr).trim(),
            "generation set inspection refused; verify source, scope, definitions and original evidence"
        );
    }
    // New planned upstream records have no selected bytes yet. Keep the
    // upstream's actionable unbuilt basis, while downstream has no recipe.
    let english = record(&f, "text/en");
    let mut planned = english.clone();
    planned["selected"] = Value::Null;
    planned["status"]["selection_state"] = json!("planned-draft");
    save(&f, "text/en", &planned);
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "planned upstream primary"]);
    profile["source"]["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    // No original claim is admitted for the intentionally unselected output.
    // The blocked cue keeps its independently replayed original evidence.
    let translation_original = profile["targets"][1]["original"].clone();
    profile["targets"][1]["original"] = Value::Null;
    let (result, report) = invoke(&profile, "planned-upstream");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(report["entries"][0]["basis"], "unbuilt");
    assert_eq!(report["entries"][0]["catch_up_candidate"], true);
    assert!(report["entries"][0]["desired_recipe"].is_string());
    assert_eq!(report["entries"][1]["basis"], Value::Null);
    assert_eq!(report["entries"][1]["desired_recipe"], Value::Null);
    assert_eq!(
        report["entries"][1]["unavailable_inputs"],
        json!(["text/en"])
    );
    assert_eq!(report["entries"][1]["advice"], "blocked");
    assert_eq!(report["entries"][1]["catch_up_candidate"], false);
    assert_eq!(report["authorizes_execution"], false);
    assert_eq!(report["primary_payloads_verified"], false);
    let mut corrupt = profile.clone();
    corrupt["targets"][0]["original"]["claim"] =
        json!(Object::from_bytes(b"missing blocked claim"));
    let (result, report) = invoke(&corrupt, "blocked-bad-original");
    assert!(!result.status.success());
    assert!(!report.exists());
    let mut foreign = profile.clone();
    foreign["targets"][0]["original"] = translation_original;
    let (result, report) = invoke(&foreign, "blocked-foreign-original");
    assert!(!result.status.success());
    assert!(!report.exists());
    let cue = record(&f, "cue/en");
    let mut changed_cue = cue.clone();
    changed_cue["selected"] = json!(Object::from_bytes(b"different current cue"));
    save(&f, "cue/en", &changed_cue);
    git(f.root(), &["add", "records"]);
    git(
        f.root(),
        &["commit", "-m", "different blocked output subject"],
    );
    let mut mismatch = profile.clone();
    mismatch["source"]["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let (result, report) = invoke(&mismatch, "blocked-subject-mismatch");
    assert!(!result.status.success());
    assert!(!report.exists());
    save(&f, "cue/en", &cue);
    // An ordinary not-yet-recorded primary also blocks planning, rather than
    // supplying invented bytes or requiring a fake generator declaration.
    let spanish = record(&f, "text/es");
    let mut planned_spanish = spanish.clone();
    planned_spanish["selected"] = Value::Null;
    save(&f, "text/es", &planned_spanish);
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "unselected source primary"]);
    profile["source"]["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let (result, report) = invoke(&profile, "planned-source");
    assert!(result.status.success());
    let report: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(
        report["entries"][0]["unavailable_inputs"],
        json!(["text/es"])
    );
    assert_eq!(report["entries"][0]["basis"], Value::Null);
    assert_eq!(report["entries"][0]["advice"], "blocked");
    assert_eq!(report["entries"][1]["advice"], "blocked");
    // Selected but ineligible inputs are still refusals, not planned records.
    save(&f, "text/es", &spanish);
    let mut held = english.clone();
    held["status"]["selection_state"] = json!("not-admitted");
    save(&f, "text/en", &held);
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "ineligible selected input"]);
    profile["source"]["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let (result, report) = invoke(&profile, "held-selected");
    assert!(!result.status.success());
    assert!(!report.exists());
    save(&f, "text/en", &english);
    let mut spec = record(&f, "generation/translation");
    spec["authored"]["document"]["inputs"]["source"]["id"] = json!("cue/en");
    save(&f, "generation/translation", &spec);
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "cyclic generators"]);
    profile["source"]["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let (result, report) = invoke(&profile, "cycle");
    assert!(!result.status.success());
    assert!(!report.exists());
}
