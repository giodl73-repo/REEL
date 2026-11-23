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
        [command, request, pin, directory] if command == "prepare-film-rebuild" => {
            let export_sha256 = reel_workflow_native::film_rebuild::export(
                Path::new(request),
                pin,
                Path::new(directory),
            )?;
            Ok(
                serde_json::json!({"ok":true,"export_sha256":export_sha256,"native_metadata_verified":true,"child_admitted":false,"consumers_rebuilt":false,"selection_verified":false}),
            )
        }
        #[cfg(all(windows, feature = "film"))]
        [command, request, pin, apply]
            if command == "dispatch-film-rebuild" && apply == "--apply" =>
        {
            reel_workflow_native::film_rebuild::dispatch::execute(Path::new(request), pin)
        }
        [command, directory, request_pin, export_pin]
            if command == "verify-film-rebuild-export" =>
        {
            let render_tasks = reel_workflow_native::film_rebuild::verify_export(
                Path::new(directory),
                request_pin,
                export_pin,
            )?;
            Ok(serde_json::json!({"ok":true,"render_tasks":render_tasks}))
        }
        #[cfg(all(windows, feature = "film"))]
        [command, request, pin, output, apply]
            if command == "prepare-film" && apply == "--apply" =>
        {
            let (request, _request) =
                reel_workflow_native::film_cli::read(Path::new(request), pin)?;
            let mut file = new_receipt(Path::new(output), &request.source, &request.admission)?;
            let ticket = reel_workflow_native::film_cli::prepare(&request, pin)?;
            let raw = ticket.bytes()?;
            use std::io::Write;
            file.write_all(&raw)
                .and_then(|_| file.sync_all())
                .map_err(|_| reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::NeedsRecovery,
                    code: "film-cli-ticket-write-preserve".into(),
                })?;
            Ok(
                serde_json::json!({"ok":true,"ticket_sha256":reel_workflow::digest(&raw),"task_completed":false,"capacity_released":false}),
            )
        }
        #[cfg(all(windows, feature = "film"))]
        [command, request, pin, ticket, ticket_pin, apply]
            if [
                "run-film",
                "resume-film",
                "stage-film-close",
                "close-film",
                "recover-film-close",
            ]
            .contains(&command.as_str())
                && apply == "--apply" =>
        {
            let (request, _request) =
                reel_workflow_native::film_cli::read(Path::new(request), pin)?;
            let (raw, _ticket) = reel_workflow_native::film_cli::read_ticket(Path::new(ticket))?;
            let action = match command.as_str() {
                "run-film" => reel_workflow_native::film_cli::Action::Run,
                "resume-film" => reel_workflow_native::film_cli::Action::Resume,
                "stage-film-close" => reel_workflow_native::film_cli::Action::StageClose,
                "close-film" => reel_workflow_native::film_cli::Action::Close,
                _ => reel_workflow_native::film_cli::Action::RecoverClose,
            };
            reel_workflow_native::film_cli::execute(&request, pin, &raw, ticket_pin, action)
        }
        #[cfg(all(windows, feature = "film"))]
        [command, request, pin, ticket, ticket_pin]
            if ["verify-film", "preview-film-close"].contains(&command.as_str()) =>
        {
            let (request, _request) =
                reel_workflow_native::film_cli::read(Path::new(request), pin)?;
            let (raw, _ticket) = reel_workflow_native::film_cli::read_ticket(Path::new(ticket))?;
            let action = if command == "verify-film" {
                reel_workflow_native::film_cli::Action::Verify
            } else {
                reel_workflow_native::film_cli::Action::PreviewClose
            };
            reel_workflow_native::film_cli::execute(&request, pin, &raw, ticket_pin, action)
        }
        #[cfg(all(windows, feature = "film"))]
        [command, path, pin, apply] if command == "film-process-child" && apply == "--apply" => {
            reel_workflow_native::film_process::child(Path::new(path), pin)?;
            Ok(serde_json::json!({"film_process_child_verified": true, "task_completed": false}))
        }
        [command] if command == "capabilities" => Ok(
            serde_json::json!({"native_snapshot_binding":true,"native_film_rebuild_binding":true,"still_control_verification":cfg!(all(windows,feature="film")),"native_byte_scope":"windows-ntfs","native_streaming_reader":cfg!(windows),"native_film_metadata_inspection":cfg!(windows),"retained_child_inputs":cfg!(windows),"bounded_manual_handoff":cfg!(windows),"retained_close_recovery":cfg!(windows),"contained_film_process_port":cfg!(all(windows, feature = "film")),"owned_film_dispatch":cfg!(all(windows, feature = "film")),"producer_execution":cfg!(all(windows, feature = "film")),"episode_scale_execution_qualified":false,"selection":false,"scratch_retirement":false}),
        ),
        [command, packet, source, request, output] if command == "inspect-film" => {
            let packet = read_control(Path::new(packet))?;
            let source: NativeProfile = read_control(Path::new(source))?;
            let request = read_control(Path::new(request))?;
            let mut inputs = NativeInputs::open(&packet, &source)?;
            let sidecar = reel_workflow_native::film_metadata::inspect(&request, &mut inputs)?;
            reel_workflow::film_inspect::write_new(&sidecar, Path::new(output))?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-native-scene-metadata-inspection","metadata_verified":true,"media_verified":false,"approvals_verified":false,"renderer_qualified":false,"production_adopted":false}),
            )
        }
        [command, request, pin] if command == "preview-close" => {
            let (request, _held) = reel_workflow_native::closing::read(Path::new(request), pin)?;
            reel_workflow_native::closing::run(
                &request,
                reel_workflow_native::closing::Action::Preview,
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"retained-close-preview-only","applied":false,"payload_deletion_authorized":false}),
            )
        }
        [command, request, pin, output, apply]
            if ["stage-close", "close-retained", "recover-close"].contains(&command.as_str())
                && apply == "--apply" =>
        {
            let (request, _held) = reel_workflow_native::closing::read(Path::new(request), pin)?;
            let mut file = new_receipt(Path::new(output), &request.source, &request.admission)?;
            let action = match command.as_str() {
                "stage-close" => reel_workflow_native::closing::Action::Stage,
                "recover-close" => reel_workflow_native::closing::Action::Recover,
                _ => reel_workflow_native::closing::Action::Apply,
            };
            let outcome = reel_workflow_native::closing::run(&request, action)?;
            use std::io::Write;
            serde_json::to_writer(&mut file, &outcome).map_err(|_| reel_workflow::Error {
                kind: reel_workflow::ErrorKind::NeedsRecovery,
                code: "close-outcome-write-preserve".into(),
            })?;
            file.flush()
                .and_then(|_| file.sync_all())
                .map_err(|_| reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::NeedsRecovery,
                    code: "close-outcome-write-preserve".into(),
                })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"original-retained-owner-close","slot_idle":outcome.slot_idle,"original_clear_complete":outcome.original_clear_complete,"scratch_retired":false,"payload_deletion_authorized":false}),
            )
        }
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
        [command, packet, source, profile, parents, attempt]
            if command == "preview-upstream-admission" =>
        {
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let parents: reel_workflow_native::upstream::Parents =
                read_control(Path::new(parents))?;
            let prepared = reel_workflow_native::upstream::Admission::prepare(
                &packet,
                &source,
                &profile,
                attempt,
                parents.borrowed()?,
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"retained-parent-input-preview","budget":prepared.preview()?,"parent_retention_verified":true,"child_claims_accepted":false,"selected":false}),
            )
        }
        [
            command,
            packet,
            source,
            profile,
            parents,
            attempt,
            output,
            apply,
        ] if command == "accept-upstream-admission" && apply == "--apply" => {
            use std::io::Write;
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let parents: reel_workflow_native::upstream::Parents =
                read_control(Path::new(parents))?;
            let prepared = reel_workflow_native::upstream::Admission::prepare(
                &packet,
                &source,
                &profile,
                attempt,
                parents.borrowed()?,
            )?;
            let mut file = new_receipt(Path::new(output), &source, &profile)?;
            let receipt = prepared.accept()?;
            let _guard = reel_workflow_native::admission::Prepared::open(
                &packet, &source, &profile, attempt,
            )?
            .reopen(&receipt)?;
            file.write_all(&receipt.canonical_bytes()?)
                .and_then(|_| file.sync_all())
                .map_err(|_| reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::NeedsRecovery,
                    code: "upstream-reconcile-original-receipt".into(),
                })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"child-owned-retained-input-claims","parent_retention_verified":true,"child_claims_accepted":true,"producer_executed":false,"selected":false}),
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
        [command, packet, profile] if command == "check-native-inputs" => {
            let packet = read_control(Path::new(packet))?;
            let profile: NativeProfile = read_control(Path::new(profile))?;
            let mut inputs = NativeInputs::open(&packet, &profile)?;
            use reel_workflow_native::InputReader;
            for input in &packet.task.inputs {
                inputs.read(input, input.bytes)?;
            }
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
            "reel-workflow-native capabilities\n",
            "reel-workflow-native prepare-film-rebuild REQUEST SHA256 NEW_DIRECTORY\n",
            "reel-workflow-native verify-film-rebuild-export DIRECTORY REQUEST_SHA256 EXPORT_SHA256\n",
            "Optional Windows film feature:\n",
            "reel-workflow-native dispatch-film-rebuild DISPATCH SHA256 --apply\n",
            "reel-workflow-native prepare-film REQUEST REQUEST_SHA NEW_TICKET --apply\n",
            "reel-workflow-native run-film REQUEST REQUEST_SHA TICKET TICKET_SHA --apply\n",
            "reel-workflow-native resume-film REQUEST REQUEST_SHA TICKET TICKET_SHA --apply\n",
            "reel-workflow-native verify-film REQUEST REQUEST_SHA TICKET TICKET_SHA\n",
            "reel-workflow-native preview-film-close REQUEST REQUEST_SHA TICKET TICKET_SHA\n",
            "reel-workflow-native stage-film-close REQUEST REQUEST_SHA TICKET TICKET_SHA --apply\n",
            "reel-workflow-native close-film REQUEST REQUEST_SHA TICKET TICKET_SHA --apply\n",
            "reel-workflow-native recover-film-close REQUEST REQUEST_SHA TICKET TICKET_SHA --apply\n",
            "reel-workflow-native inspect-film PACKET NATIVE_PROFILE REQUEST NEW_SIDECAR\n",
            "reel-workflow-native preview-close CLOSE_REQUEST REQUEST_SHA256\n",
            "reel-workflow-native stage-close CLOSE_REQUEST REQUEST_SHA256 NEW_OUTCOME --apply\n",
            "reel-workflow-native close-retained CLOSE_REQUEST REQUEST_SHA256 NEW_OUTCOME --apply\n",
            "reel-workflow-native recover-close CLOSE_REQUEST REQUEST_SHA256 NEW_OUTCOME --apply\n",
            "reel-workflow-native check-snapshot PACKET PROFILE\n",
            "reel-workflow-native check-native-inputs PACKET PROFILE\n",
            "reel-workflow-native preview-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT\n",
            "reel-workflow-native preview-upstream-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE PARENTS ATTEMPT\n",
            "reel-workflow-native accept-upstream-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE PARENTS ATTEMPT NEW_RECEIPT --apply\n",
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
            "Film execution and retained actor close require explicit commands; no API provider execution, cleanup or selection."
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
