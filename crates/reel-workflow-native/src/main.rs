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
        [command, packet, profile, journal, apply]
            if command == "create-handoff" && apply == "--apply" =>
        {
            let packet = read_control(Path::new(packet))?;
            let profile = read_control(Path::new(profile))?;
            let state =
                reel_workflow_native::handoff::create(Path::new(journal), &packet, &profile)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-handoff","state":state,"task_completed":false,"selected":false,"input_claims_released":false}),
            )
        }
        [command, journal, profile, result, returns, apply]
            if command == "import-handoff" && apply == "--apply" =>
        {
            let profile = read_control(Path::new(profile))?;
            let result = read_control(Path::new(result))?;
            let returns = read_control(Path::new(returns))?;
            let state = reel_workflow_native::handoff::import(
                Path::new(journal),
                &profile,
                result,
                &returns,
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-handoff","state":state,
                "task_completed":state == reel_workflow::handoff::State::Retained,"selected":false,"input_claims_released":false,"scratch_retirement_authorized":false}),
            )
        }
        [command, journal, profile, returns, apply]
            if command == "resume-handoff" && apply == "--apply" =>
        {
            let profile = read_control(Path::new(profile))?;
            let returns = if returns == "-" {
                None
            } else {
                Some(read_control(Path::new(returns))?)
            };
            let state = reel_workflow_native::handoff::resume(
                Path::new(journal),
                &profile,
                returns.as_ref(),
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-handoff","state":state,
                "task_completed":state == reel_workflow::handoff::State::Retained,"selected":false,"input_claims_released":false,"scratch_retirement_authorized":false}),
            )
        }
        [
            command,
            packet,
            source,
            profile,
            receipt,
            result,
            target,
            output,
            apply,
        ] if matches!(command.as_str(), "stage-retention" | "retain-response")
            && apply == "--apply" =>
        {
            use std::io::Write;
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let receipt = read_control(Path::new(receipt))?;
            let result = read_control(Path::new(result))?;
            let target = read_control(Path::new(target))?;
            let mut file = new_receipt(Path::new(output), &source, &profile)?;
            let ack = if command == "retain-response" {
                reel_workflow_native::retention::retain(
                    &packet, &source, &profile, &receipt, &result, &target,
                )?
            } else {
                reel_workflow_native::retention::stage(
                    &packet, &source, &profile, &receipt, &result, &target,
                )?
            };
            file.write_all(&ack.canonical_bytes()?)
                .and_then(|_| file.sync_all())
                .map_err(|_| reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::NeedsRecovery,
                    code: "retention-write-ack-reconcile-original".into(),
                })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"workflow-retained-registration",
                "retained_owner_acknowledged":command == "retain-response",
                "native_bytes_reopened":command == "retain-response",
                "task_completed":false,"selected":false,"input_claims_released":false,
                "scratch_retirement_authorized":false}),
            )
        }
        [command, packet, source, profile, receipt, result, ack]
            if command == "verify-retained" =>
        {
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let receipt = read_control(Path::new(receipt))?;
            let result = read_control(Path::new(result))?;
            let ack = read_control(Path::new(ack))?;
            let _guard = reel_workflow_native::retention::verify(
                &packet, &source, &profile, &receipt, &result, &ack,
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"workflow-retained-verification",
                "retained_owner_acknowledged":true,"native_bytes_reopened":true,
                "task_completed":false,"selected":false,"input_claims_released":false,
                "scratch_retirement_authorized":false}),
            )
        }
        [
            command,
            packet,
            source,
            profile,
            receipt,
            result,
            returns,
            output,
            apply,
        ] if matches!(
            command.as_str(),
            "stage-response" | "register-response" | "ingest-response"
        ) && apply == "--apply" =>
        {
            use std::io::Write;
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let receipt = read_control(Path::new(receipt))?;
            let result = read_control(Path::new(result))?;
            let returns = read_control(Path::new(returns))?;
            let mut file = new_receipt(Path::new(output), &source, &profile)?;
            let candidate = if command == "ingest-response" {
                reel_workflow_native::response::ingest(
                    &packet, &source, &profile, &receipt, &result, &returns,
                )?
            } else if command == "register-response" {
                reel_workflow_native::response::register(
                    &packet, &source, &profile, &receipt, &result, &returns,
                )?
            } else {
                reel_workflow_native::response::stage(
                    &packet, &source, &profile, &receipt, &result, &returns,
                )?
            };
            file.write_all(
                &serde_json::to_vec(&candidate).map_err(|_| reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::Invalid,
                    code: "response-receipt-json".into(),
                })?,
            )
            .and_then(|_| file.sync_all())
            .map_err(|_| reel_workflow::Error {
                kind: reel_workflow::ErrorKind::NeedsRecovery,
                code: "response-receipt-write-reconcile-original".into(),
            })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"unselected-response-candidate",
                "logical_registration_verified":true,"positive_job_protection_verified":command != "stage-response",
                "native_bytes_reopened":command == "ingest-response",
                "retained_owner_acknowledged":false,"task_completed":false,
                "scratch_retirement_authorized":false,"input_claims_released":false,"selected":false}),
            )
        }
        [command, packet, source, profile, receipt, stage, output]
            if command == "prepare-stage" =>
        {
            use std::io::Write;
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let receipt = read_control(Path::new(receipt))?;
            let stage = read_control(Path::new(stage))?;
            let mut file = new_receipt(Path::new(output), &source, &profile)?;
            let ticket = reel_workflow_native::staging::prepare(
                &packet, &source, &profile, &receipt, &stage,
            )?;
            file.write_all(&ticket.canonical_bytes()?)
                .and_then(|_| file.sync_all())
                .map_err(|_| reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::NeedsRecovery,
                    code: "staging-ticket-write-preserve".into(),
                })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-staging-ticket","payloads_exported":false,"capacity_preserved":true,"input_claims_released":false,"task_completed":false,"selected":false}),
            )
        }
        [command, packet, source, profile, ticket, pin] if command == "export-stage" => {
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let summary = reel_workflow_native::staging::export_file(
                &packet,
                &source,
                &profile,
                Path::new(ticket),
                pin,
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-guarded-input-export","workspace":summary,"retained_owner_acknowledged":false,"input_claims_released":false,"task_completed":false,"selected":false}),
            )
        }
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
        println!(concat!(
            "reel-workflow-native check-snapshot PACKET PROFILE\n",
            "reel-workflow-native check-publication-native PACKET PROFILE\n",
            "reel-workflow-native preview-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT\n",
            "reel-workflow-native accept-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT NEW_RECEIPT --apply\n",
            "reel-workflow-native reopen-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT\n",
            "reel-workflow-native prepare-stage PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT STAGING_PROFILE NEW_TICKET\n",
            "reel-workflow-native export-stage PACKET NATIVE_PROFILE ADMISSION_PROFILE TICKET TICKET_SHA256\n",
            "reel-workflow-native stage-response PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT RETURNS NEW_CANDIDATE --apply\n",
            "reel-workflow-native register-response PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT RETURNS NEW_CANDIDATE --apply\n",
            "reel-workflow-native ingest-response PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT RETURNS NEW_CANDIDATE --apply\n",
            "reel-workflow-native stage-retention PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT RETAINED_TARGET NEW_ACK --apply\n",
            "reel-workflow-native retain-response PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT RETAINED_TARGET NEW_ACK --apply\n",
            "reel-workflow-native verify-retained PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT ACK\n",
            "reel-workflow-native create-handoff PACKET HANDOFF_PROFILE NEW_JOURNAL --apply\n",
            "reel-workflow-native import-handoff JOURNAL HANDOFF_PROFILE RESULT RETURNS --apply\n",
            "reel-workflow-native resume-handoff JOURNAL HANDOFF_PROFILE RETURNS_OR_DASH --apply\n",
            "Response staging records work; registration admits output claims; ingestion publishes native bytes.\n",
            "Retain-response acknowledges retained ownership; verify-retained checks live retained protection and bytes.\n",
            "Native handoff completion requires original work, actual retained-owner proof and exact output bytes.\n",
            "No provider execution, cleanup, actor close or selection."
        ));
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
