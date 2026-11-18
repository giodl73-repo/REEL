use reel_workflow_native::{
    NativeInputs, NativeProfile, SnapshotInputs, SnapshotProfile, read_control,
};
use std::{path::Path, process::ExitCode};
fn run(args: &[String]) -> reel_workflow::Result<serde_json::Value> {
    match args {
        [command, packet, profile] if command == "check-snapshot" => {
            let packet = read_control(Path::new(packet))?;
            let profile: SnapshotProfile = read_control(Path::new(profile))?;
            let inputs = SnapshotInputs::open(&packet, &profile)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"committed-native-input-binding","inputs":inputs.input_count(),
                "snapshot_binding_verified":true,"payloads_verified":false,"durable_job_protection_verified":false,"task_completed":false,"selected":false}),
            )
        }
        [command, packet, profile] if command == "check-publication-native" => {
            let packet = read_control(Path::new(packet))?;
            let profile: NativeProfile = read_control(Path::new(profile))?;
            let mut inputs = NativeInputs::open(&packet, &profile)?;
            reel_workflow::publication::bind_comic(&packet, &mut inputs)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"guarded-native-publication-inputs","snapshot_binding_verified":true,
                "payloads_verified":true,"read_guards_held_during_check":true,"durable_job_protection_verified":false,"task_completed":false,"selected":false}),
            )
        }
        _ => Err(reel_workflow::Error {
            kind: reel_workflow::ErrorKind::Invalid,
            code: "command-arguments".into(),
        }),
    }
}
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if matches!(args.as_slice(), [a] if a == "--help") {
        println!(
            "reel-workflow-native check-snapshot PACKET PROFILE\nreel-workflow-native check-publication-native PACKET PROFILE\nRead-only exact inputs; no durable job admission, production, retention or selection."
        );
        return ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!("{}", serde_json::json!({"ok":false,"error":error}));
            ExitCode::FAILURE
        }
    }
}
