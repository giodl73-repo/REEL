#[cfg(windows)]
use super::*;

#[cfg(windows)]
#[test]
#[ignore = "requires native FFmpeg; verified runtime, in-memory controls and candidate publication"]
fn guarded_delivery_candidate_uses_existing_claim_and_publication_contracts() {
    use reel::media_runtime::{Bindings, BoundPrograms, ByteIdentity};
    use reel::tig_scene_production::{self, OutputBinding};
    use tig_assets::{
        graph::Reference,
        graph::recipe::produced::{BoundSubject, Selector},
        native::Instance,
    };
    let (mut f, request) = fixture();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let file = |path: PathBuf| {
        let path = path.canonicalize().unwrap();
        let pin = ByteIdentity::observe_file(&path).unwrap();
        reel::scene_delivery::FileRef {
            path,
            sha256: pin.sha256,
            bytes: pin.bytes,
        }
    };
    let locate = |name: &str| {
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(format!("{name}.exe")))
            .find(|p| p.is_file())
            .unwrap()
    };
    let host = ByteIdentity::observe_file(&std::env::current_exe().unwrap()).unwrap();
    let programs = BoundPrograms::bind(&Bindings {
        host,
        ffmpeg: file(locate("ffmpeg")),
        ffprobe: file(locate("ffprobe")),
        dependencies: Default::default(),
    })
    .unwrap();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let (_, context) =
        adapter::freeze_context(&mut source, &request, &mut nodes, Default::default()).unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    let schema = fs::read(f.root().join("definitions/authored.json")).unwrap();
    let definition = serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"types/output",
        "authored_schema":Object::from_bytes(&schema),"authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}})).unwrap();
    let admission = Reference {
        id: "types/output".into(),
        object: Object::from_bytes(&definition),
    };
    let bind = |id: &str, pin: &Object| {
        let record = serde_json::to_vec(&json!({"schema":"tig.instance.design-1","id":id,
            "definition":{"id":admission.id,"sha256":admission.object.sha256,"bytes":admission.object.bytes},
            "authored":{},"selected":pin,"streams":[],"status":{},"metadata":{}})).unwrap();
        let bound = Instance::parse(&record)
            .unwrap()
            .bind(&definition, &schema)
            .unwrap();
        BoundSubject::bind(&bound, &admission, Selector::Selected {})
            .unwrap()
            .unwrap()
    };
    drop(source);
    for name in [
        ".git",
        "records",
        "definitions",
        "job.json",
        "contract.json",
        "production.json",
    ] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let (mut source, context) = adapter::restore_context(
        profile,
        &request,
        context.selection(),
        &mut nodes,
        Default::default(),
    )
    .unwrap();
    let output = f.root().join("rendered-native-output");
    let candidate = tig_scene_production::render_delivery_with_programs(
        &mut source,
        &request,
        &context,
        &programs,
        &output,
    )
    .unwrap();
    assert_eq!(
        &candidate.recipe().operation().producer,
        &Object::from_bytes(programs.manifest_bytes())
    );
    assert!(!f.root().join("job.json").exists());
    assert!(!f.root().join("contract.json").exists());
    let selected = Object::from_bytes(&fs::read(output.join("master.mkv")).unwrap());
    let subject = bind("delivery/recording", &selected);
    let claim = candidate
        .claim(&[OutputBinding {
            output_name: "master.mkv",
            subject: &subject,
        }])
        .unwrap();
    claim.check_plan(candidate.recipe()).unwrap();
    let wrong = bind("delivery/recording", &Object::from_bytes(b"foreign bytes"));
    assert!(
        candidate
            .claim(&[OutputBinding {
                output_name: "master.mkv",
                subject: &wrong
            }])
            .is_err()
    );
    let before = fs::read(output.join("receipt.json")).unwrap();
    assert!(
        tig_scene_production::render_delivery_with_programs(
            &mut source,
            &request,
            &context,
            &programs,
            &output
        )
        .is_err()
    );
    assert_eq!(before, fs::read(output.join("receipt.json")).unwrap());
    #[cfg(feature = "tig-publication")]
    {
        let _ = candidate_publication::exercise(&candidate, &subject, f.root(), bind);
    }
}
