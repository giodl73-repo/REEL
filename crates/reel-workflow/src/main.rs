//! Explicit local contract tooling, never a managed production runner.
use reel_workflow::{Error, ErrorKind, MAX_CONTROL_BYTES, Packet, Plan, ReturnedResult};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

fn error(kind: ErrorKind, code: &str) -> Error {
    Error {
        kind,
        code: code.into(),
    }
}
fn control<T: serde::de::DeserializeOwned>(path: &str) -> reel_workflow::Result<T> {
    let file = File::open(path).map_err(|_| error(ErrorKind::Missing, "control-open"))?;
    let mut bytes = Vec::new();
    file.take(MAX_CONTROL_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error(ErrorKind::Integrity, "control-read"))?;
    reel_workflow::decode(&bytes)
}
fn publish_packet(path: &str, packet: &Packet) -> reel_workflow::Result<()> {
    publish_control(path, packet)
}
fn publish_control(path: &str, packet: &impl serde::Serialize) -> reel_workflow::Result<()> {
    let bytes = serde_json::to_vec_pretty(packet)
        .map_err(|_| error(ErrorKind::Invalid, "packet-encoding"))?;
    if bytes.len() > MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "packet-bytes"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(Path::new(path))
        .map_err(|_| error(ErrorKind::Conflict, "packet-create-new"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| error(ErrorKind::NeedsRecovery, "packet-write-preserve-partial"))
}
fn output_file(artifact: &reel_workflow::Artifact, path: &str) -> reel_workflow::Result<()> {
    let file = File::open(path).map_err(|_| error(ErrorKind::Missing, "output-open"))?;
    let mut reader = file.take(artifact.bytes.saturating_add(1));
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| error(ErrorKind::Integrity, "output-read"))?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        hash.update(&buffer[..count]);
    }
    let actual: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if bytes != artifact.bytes || actual != artifact.sha256 {
        return Err(error(ErrorKind::Integrity, "output-content"));
    }
    Ok(())
}
fn output_files(
    packet: &Packet,
    result: &ReturnedResult,
    bindings: &BTreeMap<String, String>,
) -> reel_workflow::Result<()> {
    reel_workflow::check_result(packet, result)?;
    if bindings.len() != result.artifacts.len()
        || result
            .artifacts
            .keys()
            .any(|key| !bindings.contains_key(key))
    {
        return Err(error(ErrorKind::Missing, "output-file-bindings"));
    }
    for (name, artifact) in &result.artifacts {
        output_file(artifact, &bindings[name])?
    }
    Ok(())
}
struct LocalInputs {
    paths: BTreeMap<String, String>,
    guards: Vec<File>,
}
fn io_open_error(value: std::io::Error) -> Error {
    let kind = match value.kind() {
        std::io::ErrorKind::NotFound => ErrorKind::Missing,
        std::io::ErrorKind::PermissionDenied => ErrorKind::Denied,
        std::io::ErrorKind::WouldBlock => ErrorKind::Busy,
        _ => ErrorKind::NeedsRecovery,
    };
    error(kind, "local-input-open")
}
impl LocalInputs {
    fn capture(&mut self, path: &str, max: u64) -> reel_workflow::Result<Vec<u8>> {
        let path = std::path::absolute(path).map_err(io_open_error)?;
        for part in path.ancestors() {
            let metadata = std::fs::symlink_metadata(part).map_err(io_open_error)?;
            #[cfg(windows)]
            let reparse = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let reparse = metadata.file_type().is_symlink();
            if reparse {
                return Err(error(ErrorKind::Denied, "local-input-reparse"));
            }
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let mut file = options.open(path).map_err(io_open_error)?;
        let metadata = file.metadata().map_err(io_open_error)?;
        if !metadata.is_file() || metadata.len() > max {
            return Err(error(ErrorKind::Budget, "local-input-bytes"));
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| error(ErrorKind::NeedsRecovery, "local-input-read"))?;
        if bytes.len() as u64 > max {
            return Err(error(ErrorKind::Budget, "local-input-growth"));
        }
        self.guards.push(file);
        Ok(bytes)
    }
}
impl reel_workflow::publication::InputReader for LocalInputs {
    fn read(
        &mut self,
        artifact: &reel_workflow::Artifact,
        max: u64,
    ) -> reel_workflow::Result<Vec<u8>> {
        let path = self
            .paths
            .get(&artifact.logical_id)
            .ok_or_else(|| error(ErrorKind::Missing, "local-input-binding"))?
            .clone();
        self.capture(&path, max)
    }
}
fn publication_inputs(
    packet: &Packet,
    paths: BTreeMap<String, String>,
    tool: &str,
) -> reel_workflow::Result<(reel_workflow::publication::BoundComic, LocalInputs)> {
    reel_workflow::check_packet(packet)?;
    if paths.len() != packet.task.inputs.len()
        || packet
            .task
            .inputs
            .iter()
            .any(|a| !paths.contains_key(&a.logical_id))
    {
        return Err(error(ErrorKind::Conflict, "local-input-binding-set"));
    }
    let mut reader = LocalInputs {
        paths,
        guards: Vec::new(),
    };
    let executable = reader.capture(tool, 128 * 1024 * 1024)?;
    reel_workflow::publication::check_tool_bytes(packet, &executable)?;
    let bound = reel_workflow::publication::bind_comic(packet, &mut reader)?;
    Ok((bound, reader))
}
fn run(args: &[String]) -> reel_workflow::Result<serde_json::Value> {
    match args {
        [
            command,
            packet,
            result,
            copy_input,
            copy_path,
            report_output,
            report_path,
        ] if command == "check-review-result" => {
            let mut reader = LocalInputs {
                paths: BTreeMap::new(),
                guards: Vec::new(),
            };
            let copy_bytes = reader.capture(copy_path, MAX_CONTROL_BYTES as u64)?;
            let report_bytes = reader.capture(report_path, MAX_CONTROL_BYTES as u64)?;
            let checked = reel_workflow::review::check_returned_report(
                &control(packet)?,
                &control(result)?,
                copy_input,
                &copy_bytes,
                report_output,
                &report_bytes,
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"task-bound-review-candidate","findings":checked.findings().len(),"viewing_verified":false,"retention_acknowledged":false,"approved":false}),
            )
        }
        [command, copy, report] if command == "check-review" => {
            let checked = reel_workflow::review::check_report(&control(copy)?, &control(report)?)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-review-contract-only","findings":checked.findings().len(),"viewing_verified":false,"issues_filed":false,"approved":false}),
            )
        }
        [command, copy, report, routes, output] if command == "export-review-issues" => {
            let checked = reel_workflow::review::check_report(&control(copy)?, &control(report)?)?;
            let proposals = reel_workflow::review::issue_proposals(&checked, &control(routes)?)?;
            publish_control(output, &proposals)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-local-issue-proposals","proposals":proposals.len(),"issues_filed":false}),
            )
        }
        [command, plan, copy, report, repair, output] if command == "plan-repair" => {
            let checked = reel_workflow::review::check_report(&control(copy)?, &control(report)?)?;
            let impact =
                reel_workflow::review::repair_impact(&control(plan)?, &checked, &control(repair)?)?;
            publish_control(output, &impact)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"proposed-repair-impact-only","rebuild_tasks":impact.rebuild.len(),"edits_applied":false,"findings_closed":false,"approved":false}),
            )
        }
        [command, plan, copy, report, repair, revision, output] if command == "revise-repair" => {
            let checked = reel_workflow::review::check_report(&control(copy)?, &control(report)?)?;
            let candidate = reel_workflow::repair::materialize(
                &control(plan)?,
                &checked,
                &control(repair)?,
                &control(revision)?,
            )?;
            publish_control(output, &candidate.plan)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"candidate-repair-plan","rebuild_tasks":candidate.impact.rebuild.len(),"production_admitted":false,"selected":false,"findings_closed":false,"approved":false}),
            )
        }
        [command, path] if command == "check-handoff" || command == "cancel-handoff" => {
            let mut journal = reel_workflow::handoff::Journal::open(Path::new(path))?;
            if command == "cancel-handoff" {
                journal.cancel()?;
            }
            Ok(
                serde_json::json!({"ok":true,"scope":"local-handoff-control-only","state":journal.state(),
                "native_state_verified":false,"protection_released":false,"task_completed":false,"selected":false}),
            )
        }
        [command, path] if command == "check-plan" => {
            let plan: Plan = control(path)?;
            let order = reel_workflow::check_plan(&plan)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-contract-check","task_count":order.len(),"production_admitted":false}),
            )
        }
        [command, path] if command == "check-composition" => {
            let declaration: reel_workflow::composition::Declaration = control(path)?;
            let report = reel_workflow::composition::check(&declaration)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-authored-composition-closure","report":report}),
            )
        }
        [command, path, recipe, report] if command == "export-composition" => {
            let declaration: reel_workflow::composition::Declaration = control(path)?;
            let checked = reel_workflow::composition::check(&declaration)?;
            publish_control(recipe, &declaration.recipe)?;
            publish_control(report, &checked)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-composition-recipe-export","production_admitted":false,
                "producer_order_verified":false,"producer_closure_verified":false,"native_bindings_verified":false,"task_completed":false}),
            )
        }
        [command, plan, task_id, new_packet] if command == "export-task" => {
            let packet = reel_workflow::make_packet(&control(plan)?, task_id, Default::default())?;
            publish_packet(new_packet, &packet)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-planning-packet","production_admitted":false,"dispatch_performed":false}),
            )
        }
        [command, plan, task_id, new_packet, dependencies] if command == "export-task" => {
            let packet =
                reel_workflow::make_packet(&control(plan)?, task_id, control(dependencies)?)?;
            publish_packet(new_packet, &packet)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-planning-packet","production_admitted":false,"dispatch_performed":false}),
            )
        }
        [command, packet, result, bindings] if command == "check-result" => {
            output_files(&control(packet)?, &control(result)?, &control(bindings)?)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"candidate-bytes-checked","retention_acknowledged":false,"task_completed":false,"selected":false}),
            )
        }
        [command, packet, bindings] if command == "check-film-intent" => {
            let packet: Packet = control(packet)?;
            let paths: BTreeMap<String, String> = control(bindings)?;
            if paths.len() > 3 {
                return Err(error(ErrorKind::Conflict, "film-control-binding-set"));
            }
            let mut reader = LocalInputs {
                paths,
                guards: Vec::new(),
            };
            let bound = reel_workflow::film_render::bind(&packet, &mut reader)?;
            let expected = std::collections::BTreeSet::from([
                &bound.job().logical_id,
                &bound.production().logical_id,
                &packet.task.tool.as_ref().unwrap().request.logical_id,
            ]);
            if reader
                .paths
                .keys()
                .collect::<std::collections::BTreeSet<_>>()
                != expected
            {
                return Err(error(ErrorKind::Conflict, "film-control-binding-set"));
            }
            Ok(
                serde_json::json!({"ok":true,"scope":"film-intent-metadata-only",
                "intent_schema":bound.request().schema,"limits":bound.request().limits,
                "streaming":bound.request().streaming,"declared_inputs":packet.task.inputs.len(),
                "media_bytes_verified":false,"tool_executed":false,"snapshot_binding_verified":false,
                "production_admitted":false,"native_v2_execution_qualified":false}),
            )
        }
        [command, packet, bindings, tool] if command == "check-publication-task" => {
            let packet: Packet = control(packet)?;
            let (bound, _guards) = publication_inputs(&packet, control(bindings)?, tool)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"local-publication-binding-only","declared_inputs":bound.input_objects(),"declared_bytes":bound.logical_bytes(),
                "tool_bytes_verified":true,"tool_executed":false,"snapshot_binding_verified":false,"production_admitted":false}),
            )
        }
        [command, packet, bindings, tool, result, receipt, payloads]
            if command == "check-publication-result" =>
        {
            let packet: Packet = control(packet)?;
            let result: ReturnedResult = control(result)?;
            let (bound, mut guards) = publication_inputs(&packet, control(bindings)?, tool)?;
            let receipt_bytes = guards.capture(receipt, MAX_CONTROL_BYTES as u64)?;
            reel_workflow::check_result(&packet, &result)?;
            reel_workflow::publication::check_comic_receipt(&bound, &result, &receipt_bytes)?;
            output_files(&packet, &result, &control(payloads)?)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"local-publication-result-only","source_correspondence_checked":true,"tool_executed":false,
                "snapshot_binding_verified":false,"retention_acknowledged":false,"task_completed":false,"selected":false}),
            )
        }
        _ => Err(error(ErrorKind::Invalid, "command-arguments")),
    }
}
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || matches!(args.as_slice(),[arg] if arg=="--help" || arg=="-h") {
        println!(
            "reel-workflow: private task/result contract prototype\n\
Usage: reel-workflow check-plan PLAN\n\
       reel-workflow check-composition DECLARATION\n\
       reel-workflow export-composition DECLARATION NEW_RECIPE NEW_REPORT\n\
       reel-workflow check-review COPY REPORT\n\
       reel-workflow check-review-result PACKET RESULT COPY_INPUT COPY_PATH REPORT_OUTPUT REPORT_PATH\n\
       reel-workflow export-review-issues COPY REPORT ROUTES NEW_PROPOSALS\n\
       reel-workflow plan-repair PLAN COPY REPORT REPAIR NEW_IMPACT\n\
       reel-workflow revise-repair PLAN COPY REPORT REPAIR REVISION NEW_CANDIDATE\n\
       reel-workflow export-task PLAN TASK NEW_PACKET [DEPENDENCY_DESCRIPTORS]\n\
       reel-workflow check-result PACKET RESULT PAYLOAD_BINDINGS\n\
       reel-workflow check-film-intent PACKET CONTROL_BINDINGS\n\
       reel-workflow check-handoff DIRECTORY\n\
       reel-workflow cancel-handoff DIRECTORY\n\
       reel-workflow check-publication-task PACKET INPUT_BINDINGS TOOL\n\
       reel-workflow check-publication-result PACKET INPUT_BINDINGS TOOL RESULT RECEIPT PAYLOAD_BINDINGS\n\
No storage admission, provider/tool execution, retention, task completion or selection.\n\
Packets contain private context; output files must be new. Preserve failed writes."
        );
        return std::process::ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(value) => {
            println!("{value}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            println!("{}", serde_json::json!({"ok":false,"error":error}));
            std::process::ExitCode::FAILURE
        }
    }
}
