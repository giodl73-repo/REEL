use reel_workflow::{manual, *};
use std::{collections::BTreeMap, env, path::Path};

fn run() -> Result<serde_json::Value> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, repository, request, output] if command == "inspect-film" => {
            let request: film_inspect::Request = manual::read_control(Path::new(request))?;
            let sidecar = film_inspect::inspect(&request, &mut film_inspect::GitReader {repository: Path::new(repository)})?;
            film_inspect::write_new(&sidecar, Path::new(output))?;
            Ok(serde_json::json!({"ok":true,"scope":"private-scene-metadata-inspection","metadata_verified":true,"media_verified":false,"approvals_verified":false,"renderer_qualified":false,"production_adopted":false}))
        }
        [command, directory] if command == "check-handoff" || command == "cancel-handoff" => {
            let mut journal = handoff::Journal::open(Path::new(directory))?;
            if command == "cancel-handoff" { journal.cancel()?; }
            Ok(serde_json::json!({"ok":true,"scope":"local-journal-only","state":journal.state(),"native_state_verified":false,"native_claims_released":false}))
        }
        [command] if command == "capabilities" => Ok(serde_json::json!({"manual_exchange":true,"film_metadata_inspection":true,"provider_calls":false,"native_storage":false,"producer_execution":false,"media_semantics_verified":false})),
        [command, plan] if command == "check-plan" => {
            let plan = manual::read_control(Path::new(plan))?;
            let count = check_plan(&plan)?.len();
            Ok(serde_json::json!({"ok":true,"scope":"workflow-contract-only","tasks":count,"production_admitted":false}))
        }
        [command, plan, task, attempt, dependencies, directory] if command == "export-work" => {
            let plan = manual::read_control(Path::new(plan))?;
            let dependencies: BTreeMap<String,DependencyResult> = manual::read_control(Path::new(dependencies))?;
            let packet = make_packet(&plan,task,dependencies)?;
            manual::export_work(&packet,attempt,Path::new(directory))?;
            Ok(serde_json::json!({"ok":true,"scope":"private-manual-work-export","provider_invoked":false,"production_admitted":false}))
        }
        [command, packet, result] if command == "check-result" => {
            check_result(&manual::read_control(Path::new(packet))?,&manual::read_control(Path::new(result))?)?;
            Ok(serde_json::json!({"ok":true,"scope":"result-contract-only","payloads_verified":false,"task_completed":false}))
        }
        [command, plan, task, output] if command == "export-task" => {
            manual::export_packet(&manual::read_control(Path::new(plan))?,task,BTreeMap::new(),Path::new(output))?;
            Ok(serde_json::json!({"ok":true,"scope":"private-planning-packet","production_admitted":false,"dispatch_performed":false}))
        }
        [command, packet, result, files] if command == "check-result" => {
            manual::verify_outputs(&manual::read_control(Path::new(packet))?,&manual::read_control(Path::new(result))?,&manual::read_control(Path::new(files))?)?;
            Ok(serde_json::json!({"ok":true,"scope":"returned-byte-check-only","payloads_verified":true,"retention_acknowledged":false,"task_completed":false}))
        }
        [command, directory, attempt, files, candidate] if command == "capture-work" => {
            let replay = manual::capture_work(Path::new(directory),attempt,&manual::read_control(Path::new(files))?,Path::new(candidate))?;
            Ok(serde_json::json!({"ok":true,"scope":"private-byte-checked-candidate","replay":replay,"native_retention_verified":false,"task_completed":false,"human_approval":false}))
        }
        [command, directory, attempt, result, files, candidate] if command == "accept-work" => {
            let replay = manual::accept_work(Path::new(directory),attempt,&manual::read_control(Path::new(result))?,&manual::read_control(Path::new(files))?,Path::new(candidate))?;
            Ok(serde_json::json!({"ok":true,"scope":"private-byte-checked-candidate","replay":replay,"native_retention_verified":false,"task_completed":false,"human_approval":false}))
        }
        _ => Err(Error {kind:ErrorKind::Invalid,code:"usage: check-plan PLAN | export-task PLAN TASK NEW_PACKET | export-work PLAN TASK ATTEMPT DEPENDENCIES_JSON NEW_DIRECTORY | check-result PACKET RESULT [FILE_BINDINGS_JSON] | capture-work WORK_DIRECTORY ATTEMPT FILE_BINDINGS_JSON NEW_CANDIDATE_DIRECTORY | accept-work WORK_DIRECTORY ATTEMPT RESULT FILE_BINDINGS_JSON NEW_CANDIDATE_DIRECTORY | capabilities".into()}),
    }
}
fn main() {
    if env::args()
        .skip(1)
        .any(|arg| arg == "--help" || arg == "-h")
    {
        println!(
            "reel-workflow capabilities\n\
reel-workflow check-plan PLAN\n\
reel-workflow inspect-film REPOSITORY REQUEST NEW_SIDECAR\n\
reel-workflow export-task PLAN TASK NEW_PACKET\n\
reel-workflow export-work PLAN TASK ATTEMPT DEPENDENCIES_JSON NEW_DIRECTORY\n\
reel-workflow check-result PACKET RESULT [FILE_BINDINGS_JSON]\n\
reel-workflow capture-work WORK_DIRECTORY ATTEMPT FILE_BINDINGS_JSON NEW_CANDIDATE_DIRECTORY\n\
reel-workflow accept-work WORK_DIRECTORY ATTEMPT RESULT FILE_BINDINGS_JSON NEW_CANDIDATE_DIRECTORY\n\
reel-workflow check-handoff DIRECTORY\n\
reel-workflow cancel-handoff DIRECTORY\n\
Manual exchange only. No provider calls, native admission, production completion or approval.\n\
Packets and candidate files contain private context. Preserve failed writes."
        );
        return;
    }
    let result = run();
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            println!("{}", serde_json::json!({"ok":false,"error":error}));
            std::process::exit(1);
        }
    }
}
