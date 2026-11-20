//! Read-only existing-parent CLI planning; no new native task admission.
use super::*;
use reel_workflow::{expansion, recipe};
use reel_workflow_native::{graph, handoff};
use tig_store::source_claims::protection::Index;
fn load<T: serde::de::DeserializeOwned>(path: &Path, held: &mut Vec<fs::File>) -> T {
    let (bytes, guard) = reel_workflow_native::read_candidate_control(path).unwrap();
    held.push(guard);
    decode(&bytes).unwrap()
}
fn save(path: &Path, value: &impl serde::Serialize) {
    use std::io::Write;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    output
        .write_all(&serde_json::to_vec(value).unwrap())
        .unwrap();
    output.sync_all().unwrap();
}
#[test]
#[ignore = "Explicit frozen CLI and existing retained synthetic comic; exports v2 expansion/recipe controls with unchanged actual Index checkpoint, no child admission or production"]
fn native_cli_expands_and_schedules_streaming_inputs_from_original_retained_results() {
    let root = PathBuf::from(
        std::env::var_os("REEL_UPSTREAM_PARENT_ROOT").expect("Set retained parent root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_STREAMING_EXPANSION_EVIDENCE")
            .expect("Set owned evidence directory"),
    );
    let cli =
        PathBuf::from(std::env::var_os("REEL_STREAMING_EXPANSION_CLI").expect("Set frozen CLI"));
    assert!(root.is_absolute() && evidence.is_absolute() && evidence.is_dir() && cli.is_absolute());
    let mut held = Vec::new();
    let packet: Packet = load(&root.join("packet.json"), &mut held);
    let profile: handoff::Profile = load(&root.join("handoff-profile.json"), &mut held);
    let result: ReturnedResult = load(&root.join("result.json"), &mut held);
    assert!(packet.task.depends_on.is_empty());
    let before = Index::open(
        &profile.admission.index_root,
        &profile.admission.index_minimum,
    )
    .unwrap()
    .checkpoint();
    let base = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: packet.project.clone(),
        source_commit: packet.source_commit.clone(),
        tasks: vec![packet.task.clone()],
    };
    let (name, asset) = result
        .artifacts
        .iter()
        .max_by_key(|(_, a)| a.bytes)
        .unwrap();
    let template = expansion::Template {
        schema: "reel.workflow-expansion-template.v2".into(),
        base_plan_sha256: canonical_digest(&base).unwrap(),
        task: expansion::TaskTemplate {
            id: "streaming-planned-child".into(),
            operation: Operation::TextProposal,
            language: Some("es".into()),
            prompt: "Use the exact synthetic retained result; no production or adoption.".into(),
            depends_on: vec![packet.task.id.clone()],
            inputs: vec![expansion::Input::Parent {
                reference: reel_workflow::upstream::Reference {
                    task_id: packet.task.id.clone(),
                    artifact: reel_workflow::upstream::Selector::Output { name: name.clone() },
                },
            }],
            outputs: BTreeMap::from([(
                "note".into(),
                OutputContract {
                    media_type: "text/plain".into(),
                    max_bytes: 100,
                    required: true,
                },
            )]),
            tool: None,
            streaming: Some(reel_workflow::upstream::Streaming {
                schema: "reel.workflow-upstream-streaming.v1".into(),
                input_bytes: 4 * 1024 * 1024 * 1024,
                input_object_bytes: 4 * 1024 * 1024 * 1024,
            }),
        },
    };
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([(
            packet.task.id.clone(),
            graph::Binding {
                journal: root.join("journal"),
                profile: graph::PinnedFile {
                    path: root.join("handoff-profile.json"),
                    sha256: digest(&fs::read(root.join("handoff-profile.json")).unwrap()),
                },
                staging_ticket: None,
            },
        )]),
    };
    let recipe = recipe::Recipe {
        schema: "reel.workflow-recipe.v2".into(),
        base: base.clone(),
        steps: vec![template.task.clone()],
    };
    let base_path = evidence.join("base.json");
    let bindings_path = evidence.join("bindings.json");
    let template_path = evidence.join("template.json");
    let recipe_path = evidence.join("recipe.json");
    save(&base_path, &base);
    save(&bindings_path, &bindings);
    save(&template_path, &template);
    save(&recipe_path, &recipe);
    let expanded_path = evidence.join("expanded.json");
    let schedule_path = evidence.join("schedule.json");
    let expanded = Command::new(&cli)
        .arg("expand-plan")
        .arg(&base_path)
        .arg(&bindings_path)
        .arg(&template_path)
        .arg(&expanded_path)
        .arg("--apply")
        .output()
        .unwrap();
    save(
        &evidence.join("expand-outcome.json"),
        &json!({"exit_code":expanded.status.code(),"stdout_sha256":digest(&expanded.stdout),"stderr_sha256":digest(&expanded.stderr)}),
    );
    assert!(
        expanded.status.success(),
        "{}",
        String::from_utf8_lossy(&expanded.stdout)
    );
    let flags: serde_json::Value = decode(&expanded.stdout).unwrap();
    assert_eq!(flags["production_admitted"], false);
    let expanded: expansion::Expansion = decode(&fs::read(expanded_path).unwrap()).unwrap();
    assert_eq!(
        expanded.packet.task.inputs.as_slice(),
        std::slice::from_ref(asset)
    );
    assert_eq!(
        expanded.upstream.as_ref().unwrap().schema,
        "reel.workflow-upstream-inputs.v3"
    );
    assert_eq!(
        expanded.packet.dependencies[&packet.task.id].result_sha256,
        canonical_digest(&result).unwrap()
    );
    let scheduled = Command::new(&cli)
        .arg("schedule-recipe")
        .arg(&recipe_path)
        .arg(&base_path)
        .arg(&bindings_path)
        .arg("1")
        .arg(&schedule_path)
        .arg("--apply")
        .output()
        .unwrap();
    save(
        &evidence.join("schedule-outcome.json"),
        &json!({"exit_code":scheduled.status.code(),"stdout_sha256":digest(&scheduled.stdout),"stderr_sha256":digest(&scheduled.stderr)}),
    );
    assert!(
        scheduled.status.success(),
        "{}",
        String::from_utf8_lossy(&scheduled.stdout)
    );
    let flags: serde_json::Value = decode(&scheduled.stdout).unwrap();
    assert_eq!(flags["production_admitted"], false);
    let schedule: serde_json::Value = decode(&fs::read(schedule_path).unwrap()).unwrap();
    assert_eq!(schedule["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(
        schedule["proposals"][0]["packet"],
        serde_json::to_value(&expanded.packet).unwrap()
    );
    assert_eq!(
        schedule["proposals"][0]["template"]["schema"],
        "reel.workflow-expansion-template.v2"
    );
    let after = Index::open(&profile.admission.index_root, &before)
        .unwrap()
        .checkpoint();
    assert_eq!(before, after);
    save(
        &evidence.join("result.json"),
        &json!({"schema":"reel.workflow-streaming-expansion-qualification.v1",
        "parent_task_key":packet.task_key,"child_task_key":expanded.packet.task_key,"input":asset,
        "index_before":before,"index_after":after,"production_admitted":false,"task_completed":false,"consumer_adoption":false}),
    );
}
