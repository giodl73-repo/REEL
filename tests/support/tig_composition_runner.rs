//! Generated cache-only composition, isolated from the eviction fixture's ledger.
use super::*;
use reel::tig_job_runner::{self, Profile};
use tig_core::{Digest, LogicalPath, tree::NodeStore};
use tig_git::source::native::{context::ProductionContext, job};
use tig_store::source_claims::Command as ClaimCommand;

pub(super) fn exercise(
    p: &mut Project,
    request: &reel::tig_episode::Request,
    context: &ProductionContext,
    recipe: &Plan,
    metadata: (&Path, &tig_store::metadata::Checkpoint),
) {
    let plan = reel::tig_episode::plan(&mut p.source(), request).unwrap();
    let selected = plan.components[0].selected.clone();
    let evidence_bytes = br#"{"fixture":"input evidence, not production approval"}"#;
    let evidence = p.publish("hot", evidence_bytes);
    let composition = BTreeMap::from([(LogicalPath::parse("scene").unwrap(), selected.clone())]);
    let proofs = BTreeMap::from([(LogicalPath::parse("assertion").unwrap(), evidence.clone())]);
    let runtime = p
        .programs
        .components()
        .map(|(name, pin)| {
            (
                LogicalPath::parse(Digest::blob(name.as_bytes()).as_str()).unwrap(),
                Object {
                    sha256: Digest::parse(&pin.sha256).unwrap(),
                    bytes: pin.bytes,
                },
            )
        })
        .collect();
    let admission = job::Admission {
        context,
        plan: recipe,
        runtime: &runtime,
        output: None,
    };
    let dependencies = job::Dependencies {
        composition: &composition,
        evidence: &proofs,
    };
    let mut writer = MetadataStore::open(metadata.0, metadata.1).unwrap();
    assert_eq!(
        writer.put(p.programs.manifest_bytes()).unwrap(),
        recipe.operation().producer
    );
    let candidate = job::freeze_with_dependencies(
        &admission,
        &dependencies,
        None,
        &mut writer,
        TreeLimits::default(),
    )
    .unwrap();
    let witness = writer.finish().unwrap();
    drop(writer);
    let mut reader = MetadataReader::open(metadata.0, &witness).unwrap();
    let resources = job::restore_with_dependencies(
        candidate.selection(),
        &admission,
        &dependencies,
        None,
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert!(resources.of(job::Role::Primary).is_empty());
    assert!(resources.of(job::Role::Composition).contains(&selected));
    assert!(resources.of(job::Role::Evidence).contains(&evidence));

    let active = p.data.path().join("composition-runner-active");
    fs::create_dir_all(active.join("objects/sha256")).unwrap();
    let project = p.data.path().join("composition-runner-project");
    fs::create_dir(&project).unwrap();
    let coordination = Coordination {
        root: p.data.path().join("coordination"),
    };
    let mut config = reel_assets::Config {
        path_policy: reel_assets::PathPolicy::ConfiguredLocalRoots,
        cache_roots: vec![active.clone(), p.hot()],
        coordination_root: Some(coordination.root.clone()),
        ..Default::default()
    };
    config.capacity.minimum_free_bytes = 0;
    config.capacity.minimum_free_basis_points = 0;
    let source = key("composition-runner");
    let owner = key("composition-owner");
    let prepared = p
        .owners
        .apply(
            key("composition-prepare"),
            ClaimCommand::Prepare {
                source: source.clone(),
                owner: owner.clone(),
                expected_revision: 0,
                selection: resources.selection().clone(),
            },
        )
        .unwrap();
    let adopted = p
        .owners
        .apply(
            key("composition-adopt"),
            ClaimCommand::Adopt {
                source: source.clone(),
                owner: owner.clone(),
                expected_revision: prepared.source_revision,
                prepared: prepared.reference,
            },
        )
        .unwrap();
    let mut profile = Profile {
        config: &config,
        coordination: &coordination,
        identity: Identity { source, owner },
        expected_revision: adopted.source_revision,
        operation: key("composition-retain"),
        metadata_root: metadata.0,
        metadata_minimum: &witness,
        tree_limits: TreeLimits::default(),
    };
    let manifest = p.data.path().join("composition-runner.json");
    let args = [
        "-v",
        "error",
        "-nostdin",
        "-n",
        "-i",
        "scene.mkv",
        "-map",
        "0:v",
        "-map",
        "0:a?",
        "-c",
        "copy",
        "-attach",
        "evidence.json",
        "-metadata:s:t",
        "mimetype=application/json",
        "out.mkv",
    ];
    let original = json!({"budget_bytes":8*1024*1024,"scratch_bytes":65536,
        "inputs":{"scene.mkv":selected,"evidence.json":evidence},"tools":{},
        "installed_tools":{"encoder":p.runtime.ffmpeg},
        "steps":[{"tool":"encoder","args":args,"timeout_seconds":30}],
        "outputs":{"video":{"path":"out.mkv","max_bytes":1024*1024}}});
    let unchanged = p.owners.checkpoint();
    // No manifest exists yet: both refusals must happen before manifest/provider IO.
    assert!(
        tig_job_runner::run(&project, &manifest, &resources, &mut p.owners, &profile)
            .unwrap_err()
            .to_string()
            .contains("planned input context")
    );
    let raw = fs::read(p.repo.path().join("records/output-opening.tig")).unwrap();
    assert_eq!(Object::from_bytes(&raw), plan.components[0].record.object);
    let definition = fs::read(p.repo.path().join("definitions/output.json")).unwrap();
    let schema = fs::read(p.repo.path().join("definitions/authored.json")).unwrap();
    let bound = Instance::parse(&raw)
        .unwrap()
        .bind(&definition, &schema)
        .unwrap();
    let subject = BoundSubject::bind(
        &bound,
        &Reference {
            id: "types/output".into(),
            object: Object::from_bytes(&definition),
        },
        Selector::Selected {},
    )
    .unwrap()
    .unwrap();
    let claim = Claim::create(
        recipe,
        tig_assets::graph::recipe::produced::ContextPin {
            root: context.selection().root.clone(),
            identity: context.selection().context.clone(),
        },
        vec![subject.subject().clone()],
    )
    .unwrap();
    let attachments = BTreeMap::new();
    let post_admission = job::Admission {
        output: Some(job::OutputEvidence {
            claim: &claim,
            attachments: &attachments,
        }),
        ..admission
    };
    let post = job::freeze_with_dependencies(
        &post_admission,
        &dependencies,
        None,
        &mut tig_core::tree::MemoryNodes::default(),
        TreeLimits::default(),
    )
    .unwrap();
    assert!(
        tig_job_runner::run_with_dependencies(&project, &manifest, &post, &mut p.owners, &profile)
            .unwrap_err()
            .to_string()
            .contains("planned input context")
    );
    assert_eq!(unchanged, p.owners.checkpoint());
    assert!(!active.join("staging/caminos").exists());

    for case in ["owner", "size", "runtime-as-input", "composition-as-tool"] {
        let mut altered = original.clone();
        match case {
            "owner" => profile.identity.owner = key("foreign-owner"),
            "size" => altered["inputs"]["scene.mkv"]["bytes"] = json!(selected.bytes + 1),
            "runtime-as-input" => {
                let tool = &p.runtime.ffmpeg;
                altered["budget_bytes"] = json!(tool.bytes + 8 * 1024 * 1024);
                altered["inputs"]["foreign.bin"] = json!({"sha256":tool.sha256,"bytes":tool.bytes});
            }
            _ => {
                altered["installed_tools"]["encoder"] = json!({
                    "path":object_file(&p.hot(),&selected),"sha256":selected.sha256,"bytes":selected.bytes,
                })
            }
        }
        fs::write(&manifest, serde_json::to_vec(&altered).unwrap()).unwrap();
        let refusal = tig_job_runner::run_with_dependencies(
            &project,
            &manifest,
            &resources,
            &mut p.owners,
            &profile,
        )
        .unwrap_err()
        .to_string();
        if case != "owner" {
            assert!(refusal.contains("admitted Tig role"), "{case}: {refusal}");
        }
        profile.identity.owner = key("composition-owner");
        assert_eq!(unchanged, p.owners.checkpoint(), "{case}");
        assert!(!active.join("staging/caminos").exists(), "{case}");
    }
    fs::write(&manifest, serde_json::to_vec(&original).unwrap()).unwrap();
    let result = tig_job_runner::run_with_dependencies(
        &project,
        &manifest,
        &resources,
        &mut p.owners,
        &profile,
    )
    .unwrap();
    assert_eq!(result["workspace_retained"], false);
    let output = Path::new(result["outputs"]["video"]["path"].as_str().unwrap());
    let input = object_file(&p.hot(), &selected);
    assert_eq!(decoded(&input, false), decoded(output, false));
    assert_eq!(decoded(&input, true), decoded(output, true));
    let extracted = p.data.path().join("extracted-evidence.json");
    let extraction = std::process::Command::new(&p.runtime.ffmpeg.path)
        .args(["-v", "error", "-nostdin", "-n", "-dump_attachment:t:0"])
        .arg(&extracted)
        .arg("-i")
        .arg(output)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        extraction.status.success(),
        "{}",
        String::from_utf8_lossy(&extraction.stderr)
    );
    assert_eq!(fs::read(extracted).unwrap(), evidence_bytes);
    let state = p.owners.source(&profile.identity.source).unwrap().unwrap();
    assert_eq!(state.revision, 4);
    assert_eq!(state.predecessors.len(), 1);
    let mut latest = MetadataReader::open(metadata.0, &witness).unwrap();
    let selection = state.current.unwrap().selection;
    let (header, _) = tig_core::tree::Tree::from_root(selection.root)
        .get(
            &LogicalPath::parse("retention/identity").unwrap(),
            &mut latest,
            TreeLimits::default(),
        )
        .unwrap();
    let header: Value =
        serde_json::from_slice(&latest.get(&header.unwrap()).unwrap().unwrap()).unwrap();
    assert_eq!(header["succeeded"], true);
    assert!(header["production_claim"].is_null());
    assert!(
        fs::read_dir(active.join("staging/caminos"))
            .unwrap()
            .next()
            .is_none()
    );
}
