//! Command/namespace qualification only; no native custody is supplied here.
use reel_workflow::{expansion, *};
use std::{collections::BTreeMap, fs, path::Path, process::Command};
fn save(path: &Path, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
#[test]
fn expansion_cli_exports_only_new_private_planning_controls_and_refuses_unretained_dependencies() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let task = Task {
        id: "root".into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::TextProposal,
        tool: None,
        language: Some("es".into()),
        prompt: "Synthetic planning only.".into(),
        inputs: vec![],
        depends_on: vec![],
        outputs: BTreeMap::from([(
            "note".into(),
            OutputContract {
                media_type: "text/plain".into(),
                max_bytes: 128,
                required: true,
            },
        )]),
    };
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic-expansion".into(),
        source_commit: "a".repeat(40),
        tasks: vec![task.clone()],
    };
    let mut template = expansion::Template {
        schema: "reel.workflow-expansion-template.v1".into(),
        base_plan_sha256: canonical_digest(&plan).unwrap(),
        task: expansion::TaskTemplate {
            id: "late-child".into(),
            operation: task.operation,
            language: task.language,
            prompt: task.prompt,
            depends_on: vec![],
            inputs: vec![],
            outputs: task.outputs,
            tool: None,
        },
    };
    let plan_path = root.join("plan.json");
    let bindings_path = root.join("bindings.json");
    let template_path = root.join("template.json");
    let output_path = root.join("new-expansion.json");
    save(&plan_path, &plan);
    save(
        &bindings_path,
        &serde_json::json!({"schema":"reel.workflow-run-bindings.v1","tasks":{}}),
    );
    save(&template_path, &template);
    let run = |output: &Path, apply: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_reel-workflow-native"));
        command.args([
            "expand-plan".as_ref(),
            plan_path.as_os_str(),
            bindings_path.as_os_str(),
            template_path.as_os_str(),
            output.as_os_str(),
        ]);
        if apply {
            command.arg("--apply");
        }
        command.output().unwrap()
    };
    let output = run(&output_path, true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["production_admitted"], false);
    assert_eq!(response["task_completed"], false);
    let raw = fs::read(&output_path).unwrap();
    let exported: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let expanded: Plan = serde_json::from_value(exported["plan"].clone()).unwrap();
    assert_eq!(expanded.tasks.len(), 2);
    assert_eq!(expanded.tasks[0], plan.tasks[0]);
    assert!(expanded.tasks[1].native_inputs.is_none());
    assert!(!run(&output_path, true).status.success());
    assert_eq!(fs::read(&output_path).unwrap(), raw);
    let pending = root.join("pending.json");
    assert!(!run(&pending, false).status.success());
    assert!(!pending.exists());
    let recipe_path = root.join("recipe.json");
    let recipe = reel_workflow::recipe::Recipe {
        schema: "reel.workflow-recipe.v1".into(),
        base: plan.clone(),
        steps: vec![template.task.clone()],
    };
    save(&recipe_path, &recipe);
    let schedule_path = root.join("schedule.json");
    let scheduled = Command::new(env!("CARGO_BIN_EXE_reel-workflow-native"))
        .args([
            "schedule-recipe".as_ref(),
            recipe_path.as_os_str(),
            plan_path.as_os_str(),
            bindings_path.as_os_str(),
            "1".as_ref(),
            schedule_path.as_os_str(),
            "--apply".as_ref(),
        ])
        .output()
        .unwrap();
    assert!(
        scheduled.status.success(),
        "{}",
        String::from_utf8_lossy(&scheduled.stdout)
    );
    let scheduled: serde_json::Value =
        serde_json::from_slice(&fs::read(schedule_path).unwrap()).unwrap();
    assert_eq!(scheduled["rows"][0]["state"], "ready");
    assert_eq!(scheduled["proposals"].as_array().unwrap().len(), 1);
    template.task.depends_on.push("root".into());
    save(&template_path, &template);
    let refused = run(&pending, true);
    assert!(!refused.status.success());
    let response: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(response["error"]["kind"], "busy");
    assert!(!pending.exists());
}
