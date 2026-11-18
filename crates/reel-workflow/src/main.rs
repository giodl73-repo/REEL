//! Explicit local contract tooling; never a managed production runner.
use reel_workflow::{Error, ErrorKind, MAX_CONTROL_BYTES, Packet, Plan, ReturnedResult};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::{
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
    let actual_hash: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if bytes != artifact.bytes || actual_hash != artifact.sha256 {
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
        output_file(artifact, &bindings[name])?;
    }
    Ok(())
}
fn run(args: &[String]) -> reel_workflow::Result<serde_json::Value> {
    match args {
        [command, path] if command == "check-plan" => {
            let plan: Plan = control(path)?;
            let order = reel_workflow::check_plan(&plan)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-contract-check",
                "task_count":order.len(),"production_admitted":false}),
            )
        }
        [command, plan, task_id, new_packet] if command == "export-task" => {
            let packet = reel_workflow::make_packet(&control(plan)?, task_id, Default::default())?;
            publish_packet(new_packet, &packet)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-planning-packet",
                "production_admitted":false,"dispatch_performed":false}),
            )
        }
        [command, plan, task_id, new_packet, dependencies] if command == "export-task" => {
            let packet =
                reel_workflow::make_packet(&control(plan)?, task_id, control(dependencies)?)?;
            publish_packet(new_packet, &packet)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-planning-packet",
                "production_admitted":false,"dispatch_performed":false}),
            )
        }
        [command, packet, result, bindings] if command == "check-result" => {
            output_files(&control(packet)?, &control(result)?, &control(bindings)?)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"candidate-bytes-checked",
                "retention_acknowledged":false,"task_completed":false,"selected":false}),
            )
        }
        _ => Err(error(ErrorKind::Invalid, "command-arguments")),
    }
}
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || matches!(args.as_slice(), [arg] if arg == "--help" || arg == "-h") {
        println!(
            "reel-workflow: private task/result contract prototype\n\
Usage: reel-workflow check-plan PLAN\n\
       reel-workflow export-task PLAN TASK NEW_PACKET [DEPENDENCY_DESCRIPTORS]\n\
       reel-workflow check-result PACKET RESULT PAYLOAD_BINDINGS\n\
No storage admission, provider call, retention, task completion or selection.\n\
Packets contain private prompts; output files must be new. Preserve failed writes."
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
