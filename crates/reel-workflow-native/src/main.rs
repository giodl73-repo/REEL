use reel_workflow_native::{
    NativeInputs, NativeProfile, SnapshotInputs, SnapshotProfile, read_control,
};
use std::{path::Path, process::ExitCode};
fn new_receipt(
    path: &Path,
    source: &NativeProfile,
    profile: &reel_workflow_native::admission::Profile,
) -> reel_workflow::Result<std::fs::File> {
    let invalid = || reel_workflow::Error {
        kind: reel_workflow::ErrorKind::Denied,
        code: "admission-private-receipt-scope".into(),
    };
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
        || path
            .file_name()
            .and_then(|n| n.to_str())
            .is_none_or(|n| n.contains(':'))
    {
        return Err(invalid());
    }
    let parent = path.parent().ok_or_else(invalid)?;
    for ancestor in parent.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor).map_err(|_| invalid())?;
        #[cfg(windows)]
        let linked = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let linked = metadata.file_type().is_symlink();
        if linked || !metadata.is_dir() {
            return Err(invalid());
        }
    }
    let parent = parent.canonicalize().map_err(|_| invalid())?;
    for root in [
        &profile.index_root,
        &profile.metadata_root,
        &profile.scope.coordination_root,
        &source.source.pool.repository,
    ] {
        let root = root.canonicalize().unwrap_or_else(|_| root.clone());
        if parent.starts_with(root) {
            return Err(invalid());
        }
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| reel_workflow::Error {
            kind: if e.kind() == std::io::ErrorKind::AlreadyExists {
                reel_workflow::ErrorKind::Conflict
            } else {
                reel_workflow::ErrorKind::NeedsRecovery
            },
            code: "admission-create-receipt-preserve".into(),
        })
}
fn run(args: &[String]) -> reel_workflow::Result<serde_json::Value> {
    match args {
        [command, packet, source, profile, attempt] if command == "preview-admission" => {
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let prepared = reel_workflow_native::admission::Prepared::open(
                &packet, &source, &profile, attempt,
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-input-claim-preview","budget":prepared.preview()?,"claims_accepted":false,"payloads_verified":false,"selected":false}),
            )
        }
        [command, packet, source, profile, attempt, output, apply]
            if command == "accept-admission" && apply == "--apply" =>
        {
            use std::io::Write;
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let prepared = reel_workflow_native::admission::Prepared::open(
                &packet, &source, &profile, attempt,
            )?;
            let path = Path::new(output);
            if !path.is_absolute()
                || path
                    .components()
                    .any(|p| matches!(p, std::path::Component::ParentDir))
            {
                return Err(reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::Invalid,
                    code: "admission-new-absolute-receipt".into(),
                });
            }
            // Reserve a new receipt before effects. Preserve partials on any
            // failure; reconcile the original operation into a NEW receipt path.
            let mut file = new_receipt(path, &source, &profile)?;
            let receipt = prepared.accept()?;
            let _guard = reel_workflow_native::admission::Prepared::open(
                &packet, &source, &profile, attempt,
            )?
            .reopen(&receipt)?;
            file.write_all(&receipt.canonical_bytes()?)
                .and_then(|_| file.sync_all())
                .map_err(|_| reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::NeedsRecovery,
                    code: "admission-write-receipt-reconcile-original".into(),
                })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-input-claims-accepted","durable_input_claims_verified":true,"payloads_verified":false,"retained_owner_acknowledged":false,"task_completed":false,"selected":false}),
            )
        }
        [command, packet, source, profile, receipt] if command == "reopen-admission" => {
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let receipt: reel_workflow_native::admission::Receipt =
                read_control(Path::new(receipt))?;
            let _guard = reel_workflow_native::admission::Prepared::open(
                &packet,
                &source,
                &profile,
                &receipt.attempt_id,
            )?
            .reopen(&receipt)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"original-native-input-claims-reopened","durable_input_claims_verified":true,"payloads_verified":false,"retained_owner_acknowledged":false,"task_completed":false,"selected":false}),
            )
        }
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
            "reel-workflow-native check-snapshot PACKET PROFILE\nreel-workflow-native check-publication-native PACKET PROFILE\nreel-workflow-native preview-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT\nreel-workflow-native accept-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT NEW_RECEIPT --apply\nreel-workflow-native reopen-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT\nOnly explicit acceptance changes input claims. No production, retained-result ownership, completion or selection."
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
