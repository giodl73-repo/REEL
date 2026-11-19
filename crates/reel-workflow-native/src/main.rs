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
        [
            command,
            packet,
            source,
            profile,
            receipt,
            result,
            ack,
            output,
            tail @ ..,
        ] if command == "export-review-copy"
            && matches!(tail, [apply] | [_, apply] if apply == "--apply") =>
        {
            use std::io::Write;
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let review = reel_workflow_native::retention::review_copy(
                &packet,
                &source,
                &profile,
                &read_control(Path::new(receipt))?,
                &read_control(Path::new(result))?,
                &read_control(Path::new(ack))?,
            )?;
            let manifest_path = if tail.len() == 2 {
                Some(Path::new(&tail[0]))
            } else {
                None
            };
            if manifest_path.is_some() != review.manifest().is_some() {
                return Err(reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::Invalid,
                    code: "review-export-manifest-argument".into(),
                });
            }
            let raw = serde_json::to_vec(review.copy()).map_err(|_| reel_workflow::Error {
                kind: reel_workflow::ErrorKind::Invalid,
                code: "review-export-json".into(),
            })?;
            let mut output = new_receipt(Path::new(output), &source, &profile)?;
            let mut manifest = manifest_path
                .map(|p| new_receipt(p, &source, &profile))
                .transpose()?;
            let failed = |_| reel_workflow::Error {
                kind: reel_workflow::ErrorKind::NeedsRecovery,
                code: "review-export-write-preserve-partial".into(),
            };
            if let Some(manifest) = &mut manifest {
                manifest
                    .write_all(review.manifest().unwrap())
                    .and_then(|_| manifest.sync_all())
                    .map_err(failed)?;
            }
            output
                .write_all(&raw)
                .and_then(|_| output.sync_all())
                .map_err(failed)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-native-review-export","units":review.copy().units.len(),"native_producer_verified":true,
                "derived_controls_retained":false,"viewing_verified":false,"approved":false,"selected":false,"input_claims_released":false}),
            )
        }
        #[cfg(all(windows, feature = "film"))]
        [command, request, pin, apply] if command == "film-process-child" && apply == "--apply" => {
            reel_workflow_native::film_process::child(Path::new(request), pin)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"contained-film-process","task_completed":false,"selected":false}),
            )
        }
        [command, plan, bindings, id, config, directory, apply]
            if command == "dispatch-task" && apply == "--apply" =>
        {
            let summary = reel_workflow_native::dispatch::run(
                &read_control(Path::new(plan))?,
                &read_control(Path::new(bindings))?,
                id,
                &read_control(Path::new(config))?,
                Path::new(directory),
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-protected-task-dispatch","task_completed":summary.state==reel_workflow::handoff::State::Retained,"provider_invoked":summary.provider_called,"summary":summary,"selected":false,"input_claims_released":false}),
            )
        }
        [
            command,
            recipe,
            plan,
            bindings,
            config,
            directory,
            limit,
            apply,
        ] if command == "advance-recipe" && apply == "--apply" => {
            let limit = limit.parse().map_err(|_| reel_workflow::Error {
                kind: reel_workflow::ErrorKind::Invalid,
                code: "runner-limit".into(),
            })?;
            let summary = reel_workflow_native::runner::advance(
                &read_control(Path::new(recipe))?,
                &read_control(Path::new(plan))?,
                &read_control(Path::new(bindings))?,
                &read_control(Path::new(config))?,
                Path::new(directory),
                limit,
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-recipe-admission-runner","provider_invoked":summary.dispatched.iter().any(|s|s.provider_called),"summary":summary,"selected":false,"input_claims_released":false}),
            )
        }
        [command, plan, frozen, pin, config, directory, apply]
            if command == "advance-expanded" && apply == "--apply" =>
        {
            let (frozen, _held) =
                reel_workflow_native::expansion::read_pinned(Path::new(frozen), pin)?;
            let summary = reel_workflow_native::advancement::advance(
                &read_control(Path::new(plan))?,
                &frozen,
                &read_control(Path::new(config))?,
                Path::new(directory),
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-expanded-graph-adoption","summary":summary,
                "provider_invoked":false,"scratch_allocated":false,"selected":false,"input_claims_released":false}),
            )
        }
        [command, journal, profile, target, output, apply]
            if command == "prepare-close" && apply == "--apply" =>
        {
            let (ticket, preview) = reel_workflow_native::closing::prepare(
                Path::new(journal),
                read_control(Path::new(profile))?,
                read_control(Path::new(target))?,
            )?;
            let pin = ticket.export(Path::new(output))?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-close-preview","ticket_sha256":pin,"preview":preview,"slot_changed":false,
                "payloads_deleted":false,"scratch_retired":false,"selected":false}),
            )
        }
        [command, ticket, pin, output, apply]
            if (command == "close-job"
                || command == "recover-close"
                || command == "stage-close")
                && apply == "--apply" =>
        {
            let (ticket, _held) =
                reel_workflow_native::closing::read_pinned(Path::new(ticket), pin)?;
            let outcome = if command == "stage-close" {
                ticket.stage_to(Path::new(output))?
            } else {
                ticket.execute_to(Path::new(output), command == "recover-close")?
            };
            Ok(
                serde_json::json!({"ok":true,"scope":"native-original-job-close","slot_idle_observed":outcome.slot_idle,"original_clear_complete":outcome.original_clear_complete,
                "payloads_deleted":false,"scratch_retired":false,"retained_history_released":false,"selected":false}),
            )
        }
        [command, recipe, plan, bindings, limit, output, apply]
            if command == "schedule-recipe" && apply == "--apply" =>
        {
            let limit = limit.parse().map_err(|_| reel_workflow::Error {
                kind: reel_workflow::ErrorKind::Invalid,
                code: "recipe-proposal-limit".into(),
            })?;
            let count = reel_workflow_native::graph::export_recipe(
                &read_control(Path::new(plan))?,
                &read_control(Path::new(bindings))?,
                &read_control(Path::new(recipe))?,
                limit,
                Path::new(output),
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-native-recipe-proposals","proposed_tasks":count,"production_admitted":false,
                "atomic_custody_snapshot":false,"provider_invoked":false,"task_completed":false,"selected":false}),
            )
        }
        [
            command,
            plan,
            bindings,
            template,
            source,
            admission,
            attempt,
            output,
            apply,
        ] if command == "prepare-expanded" && apply == "--apply" => {
            let frozen = reel_workflow_native::expansion::prepare(
                &read_control(Path::new(plan))?,
                &read_control(Path::new(bindings))?,
                &read_control(Path::new(template))?,
                read_control(Path::new(source))?,
                read_control(Path::new(admission))?,
                attempt,
            )?;
            frozen.export(Path::new(output))?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-frozen-expansion","frozen_sha256":reel_workflow::canonical_digest(&frozen)?,"source_metadata_checked":true,"preview":frozen.preview,
                "production_admitted":false,"provider_invoked":false,"task_completed":false,"selected":false}),
            )
        }
        [command, plan, frozen, pin, output, apply]
            if (command == "accept-expanded" || command == "recover-expanded")
                && apply == "--apply" =>
        {
            let base = read_control(Path::new(plan))?;
            let (frozen, _frozen_pin) =
                reel_workflow_native::expansion::read_pinned(Path::new(frozen), pin)?;
            if command == "accept-expanded" {
                frozen.accept_to(&base, Path::new(output))?;
            } else {
                frozen.recover_to(&base, Path::new(output))?;
            }
            Ok(
                serde_json::json!({"ok":true,"scope":"native-expanded-input-admission","durable_input_claims_verified":true,
                "original_operation_recovered":command=="recover-expanded","provider_invoked":false,"task_completed":false,"selected":false,"input_claims_released":false}),
            )
        }
        [
            command,
            packet,
            source,
            profile,
            attempt,
            parents,
            output,
            apply,
        ] if command == "accept-upstream" && apply == "--apply" => {
            use std::io::Write;
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let parents: reel_workflow_native::upstream::Parents =
                read_control(Path::new(parents))?;
            let admission = reel_workflow_native::upstream::Admission::prepare(
                &packet,
                &source,
                &profile,
                attempt,
                parents.borrowed()?,
            )?;
            let mut file = new_receipt(Path::new(output), &source, &profile)?;
            let receipt = admission.accept()?;
            let _claims = reel_workflow_native::admission::Prepared::open(
                &packet, &source, &profile, attempt,
            )?
            .reopen(&receipt)?;
            file.write_all(&receipt.canonical_bytes()?)
                .and_then(|_| file.sync_all())
                .map_err(|_| reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::NeedsRecovery,
                    code: "composite-write-receipt-recover-original".into(),
                })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-composite-input-admission","durable_input_claims_verified":true,"parent_retention_verified":true,"task_completed":false,"selected":false,"input_claims_released":false}),
            )
        }
        [command, packet, source, profile, attempt, output, apply]
            if command == "recover-admission" && apply == "--apply" =>
        {
            use std::io::Write;
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let prepared = reel_workflow_native::admission::Prepared::open(
                &packet, &source, &profile, attempt,
            )?;
            let receipt = prepared.recover()?;
            let mut file = new_receipt(Path::new(output), &source, &profile)?;
            file.write_all(&receipt.canonical_bytes()?)
                .and_then(|_| file.sync_all())
                .map_err(|_| reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::NeedsRecovery,
                    code: "admission-recovered-receipt-write".into(),
                })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-original-admission-recovery","durable_input_claims_verified":true,"new_admission":false,"task_completed":false,"selected":false,"input_claims_released":false}),
            )
        }
        [command, plan, bindings, ticket, pin, apply]
            if command == "materialize-worker" && apply == "--apply" =>
        {
            let plan = read_control(Path::new(plan))?;
            let bindings = read_control(Path::new(bindings))?;
            reel_workflow_native::worker::materialize_file(
                &plan,
                &bindings,
                Path::new(ticket),
                pin,
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-worker-materialization","outputs_materialized":true,
                "task_completed":false,"provider_invoked":false,"selected":false,"scratch_retirement_authorized":false}),
            )
        }
        [
            command,
            plan,
            bindings,
            batch,
            pin,
            task,
            staging,
            output,
            apply,
        ] if command == "prepare-worker" && apply == "--apply" => {
            let plan = read_control(Path::new(plan))?;
            let bindings = read_control(Path::new(bindings))?;
            let staging = read_control(Path::new(staging))?;
            let batch = reel_workflow_native::graph::PinnedFile {
                path: batch.into(),
                sha256: pin.clone(),
            };
            reel_workflow_native::worker::prepare_file(
                &plan,
                &bindings,
                &batch,
                task,
                &staging,
                Path::new(output),
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-worker-preparation","worker_executed":false,"task_completed":false,"selected":false,"scratch_retirement_authorized":false}),
            )
        }
        [command, plan, bindings, ticket, pin, apply]
            if command == "run-worker" && apply == "--apply" =>
        {
            let plan = read_control(Path::new(plan))?;
            let bindings = read_control(Path::new(bindings))?;
            let state =
                reel_workflow_native::worker::run_file(&plan, &bindings, Path::new(ticket), pin)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-deterministic-worker","state":state,
                "task_completed":state==reel_workflow::handoff::State::Retained,"provider_invoked":false,
                "selected":false,"input_claims_released":false,"scratch_retirement_authorized":false}),
            )
        }
        [
            command,
            plan,
            bindings,
            batch,
            pin,
            task,
            result,
            returns,
            apply,
        ] if command == "import-ready" && apply == "--apply" => {
            let plan = read_control(Path::new(plan))?;
            let bindings = read_control(Path::new(bindings))?;
            let result = read_control(Path::new(result))?;
            let returns = read_control(Path::new(returns))?;
            let state = reel_workflow_native::graph::import_ready(
                &plan,
                &bindings,
                &reel_workflow_native::graph::PinnedFile {
                    path: batch.into(),
                    sha256: pin.clone(),
                },
                task,
                result,
                &returns,
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-ready-result","state":state,
                "task_completed":state==reel_workflow::handoff::State::Retained,"selected":false,
                "input_claims_released":false,"scratch_retirement_authorized":false}),
            )
        }
        [command, plan, bindings, template, output, apply]
            if command == "expand-plan" && apply == "--apply" =>
        {
            let count = reel_workflow_native::graph::export_expansion(
                &read_control(Path::new(plan))?,
                &read_control(Path::new(bindings))?,
                &read_control(Path::new(template))?,
                Path::new(output),
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-native-planning-expansion","declared_inputs":count,
                "atomic_custody_snapshot":false,"production_admitted":false,"provider_invoked":false,"task_completed":false,"selected":false}),
            )
        }
        [command, plan, bindings] if command == "inspect-run" => {
            let plan = read_control(Path::new(plan))?;
            let bindings = read_control(Path::new(bindings))?;
            let frontier = reel_workflow_native::graph::inspect(&plan, &bindings)?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-native-run-observations",
                "all_tasks_retained_observed":frontier.rows.iter().all(|r|r.status==reel_workflow::graph::Status::RetainedObserved),
                "tasks":frontier.rows,"verified_waiting":frontier.waiting.len(),"needs_admission":frontier.needs_admission.len(),
                "atomic_custody_snapshot":false,"provider_invoked":false,"task_completed":false,"selected":false}),
            )
        }
        [command, plan, bindings, tasks, output, apply]
            if command == "export-ready" && apply == "--apply" =>
        {
            let plan = read_control(Path::new(plan))?;
            let bindings = read_control(Path::new(bindings))?;
            let tasks = read_control::<Vec<String>>(Path::new(tasks))?;
            let count = reel_workflow_native::graph::export_ready(
                &plan,
                &bindings,
                &tasks,
                Path::new(output),
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-native-ready-batch","exported_tasks":count,
                "named_native_inputs_checked":true,"provider_invoked":false,"task_completed":false,
                "selected":false,"input_claims_released":false}),
            )
        }
        [
            command,
            packet,
            source,
            profile,
            receipt,
            result,
            ack,
            staging,
            ticket,
            apply,
        ] if command == "prepare-comic-export" && apply == "--apply" => {
            use std::io::Write;
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let receipt = read_control(Path::new(receipt))?;
            let result = read_control(Path::new(result))?;
            let ack = read_control(Path::new(ack))?;
            let staging = read_control(Path::new(staging))?;
            let mut file = new_receipt(Path::new(ticket), &source, &profile)?;
            let prepared = reel_workflow_native::comic_export::prepare(
                &packet, &source, &profile, &receipt, &result, &ack, &staging,
            )?;
            file.write_all(&prepared.canonical_bytes()?)
                .and_then(|_| file.sync_all())
                .map_err(|_| reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::NeedsRecovery,
                    code: "comic-export-ticket-write-preserve".into(),
                })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-comic-export-preparation",
                "native_retained_proof_rechecked":true,"payloads_copied":false,
                "task_completed":false,"selected":false,"input_claims_released":false,
                "scratch_retirement_authorized":false}),
            )
        }
        [command, packet, source, profile, ticket, pin, apply]
            if command == "export-comic" && apply == "--apply" =>
        {
            let packet = read_control(Path::new(packet))?;
            let source = read_control(Path::new(source))?;
            let profile = read_control(Path::new(profile))?;
            let summary = reel_workflow_native::comic_export::export_file(
                &packet,
                &source,
                &profile,
                Path::new(ticket),
                pin,
            )?;
            Ok(
                serde_json::json!({"ok":true,"scope":"native-comic-packet-export",
                "native_retained_proof_rechecked":true,"payloads_copied":true,"export":summary,
                "task_completed":false,"selected":false,"input_claims_released":false,
                "scratch_retirement_authorized":false}),
            )
        }
        [command, packet, output, receipt_pin] if command == "comic-restore" => {
            reel_publication::comic::restore_sources(
                Path::new(packet),
                Path::new(output),
                receipt_pin,
            )
            .map_err(|_| reel_workflow::Error {
                kind: reel_workflow::ErrorKind::Integrity,
                code: "comic-source-restore-refused-preserve".into(),
            })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-comic-source-restoration",
                "sources_restored":true,"native_retained_proof_rechecked":false,
                "task_completed":false,"selected":false,"print_ready":false}),
            )
        }
        [command, request, output] if command == "comic-render" || command == "comic-check" => {
            let checked = if command == "comic-render" {
                reel_publication::comic::render(Path::new(request), Path::new(output))
            } else {
                reel_publication::comic::check(Path::new(request), Path::new(output))
            };
            checked.map_err(|_| reel_workflow::Error {
                kind: reel_workflow::ErrorKind::Integrity,
                code: "comic-worker-refused-preserve".into(),
            })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-comic-worker","rendered":true,
                "outputs_written":command == "comic-render","retained_owner_acknowledged":false,
                "task_completed":false,"selected":false}),
            )
        }
        [command, request, output] if command == "comic-issue-assemble" => {
            reel_publication::comic_issue::assemble(Path::new(request), Path::new(output))
                .map_err(|_| reel_workflow::Error {
                    kind: reel_workflow::ErrorKind::Integrity,
                    code: "comic-issue-assembly-refused-preserve".into(),
                })?;
            Ok(
                serde_json::json!({"ok":true,"scope":"private-comic-issue-worker","outputs_written":true,
                "retained_owner_acknowledged":false,"task_completed":false,"selected":false,"print_ready":false}),
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
            reel_workflow_native::check_publication_inputs(&packet, &mut inputs)?;
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
            "reel-workflow-native inspect-run PLAN RUN_BINDINGS\n",
            "reel-workflow-native prepare-close JOURNAL PROFILE TARGET NEW_TICKET --apply\n",
            "reel-workflow-native close-job TICKET TICKET_SHA256 NEW_RECEIPT --apply\n",
            "reel-workflow-native stage-close TICKET TICKET_SHA256 NEW_STAGE_RECEIPT --apply\n",
            "reel-workflow-native recover-close TICKET TICKET_SHA256 NEW_RECEIPT --apply\n",
            "reel-workflow-native schedule-recipe RECIPE PLAN RUN_BINDINGS LIMIT NEW_PRIVATE_SCHEDULE --apply\n",
            "reel-workflow-native advance-expanded PLAN FROZEN FROZEN_SHA256 CONFIG PRIVATE_DIRECTORY --apply\n",
            "reel-workflow-native advance-recipe RECIPE BASE_PLAN BASE_BINDINGS CONFIG PRIVATE_DIRECTORY LIMIT --apply\n",
            "reel-workflow-native dispatch-task PLAN BINDINGS TASK CONFIG PRIVATE_DIRECTORY --apply\n",
            "reel-workflow-native expand-plan PLAN RUN_BINDINGS TEMPLATE NEW_PRIVATE_EXPANSION --apply\n",
            "reel-workflow-native prepare-expanded PLAN RUN_BINDINGS TEMPLATE SOURCE ADMISSION ATTEMPT NEW_FROZEN --apply\n",
            "reel-workflow-native accept-expanded PLAN FROZEN FROZEN_SHA256 NEW_RECEIPT --apply\n",
            "reel-workflow-native recover-expanded PLAN FROZEN FROZEN_SHA256 NEW_RECEIPT --apply\n",
            "reel-workflow-native prepare-worker PLAN RUN_BINDINGS BATCH BATCH_SHA256 TASK_ID STAGING_PROFILE NEW_TICKET --apply\n",
            "reel-workflow-native accept-upstream PACKET SOURCE ADMISSION ATTEMPT PARENTS NEW_RECEIPT --apply\n",
            "reel-workflow-native recover-admission PACKET SOURCE ADMISSION ATTEMPT NEW_RECEIPT --apply\n",
            "reel-workflow-native run-worker PLAN RUN_BINDINGS WORKER_TICKET TICKET_SHA256 --apply\n",
            "reel-workflow-native materialize-worker PLAN RUN_BINDINGS WORKER_TICKET TICKET_SHA256 --apply\n",
            "reel-workflow-native export-ready PLAN RUN_BINDINGS TASK_IDS_JSON NEW_PRIVATE_BATCH --apply\n",
            "reel-workflow-native import-ready PLAN RUN_BINDINGS BATCH BATCH_SHA256 TASK_ID RESULT RETURNS --apply\n",
            "reel-workflow-native comic-render REQUEST NEW_DIRECTORY\n",
            "reel-workflow-native comic-issue-assemble REQUEST NEW_DIRECTORY\n",
            "reel-workflow-native comic-check REQUEST DIRECTORY\n",
            "reel-workflow-native check-publication-native PACKET PROFILE\n",
            "reel-workflow-native preview-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT\n",
            "reel-workflow-native accept-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT NEW_RECEIPT --apply\n",
            "reel-workflow-native reopen-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT\n",
            "reel-workflow-native prepare-stage PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT STAGING_PROFILE NEW_TICKET\n",
            "reel-workflow-native prepare-comic-export PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT ACK STAGING_PROFILE NEW_TICKET --apply\n",
            "reel-workflow-native export-comic PACKET NATIVE_PROFILE ADMISSION_PROFILE TICKET TICKET_SHA256 --apply\n",
            "reel-workflow-native comic-restore PACKET NEW_DIRECTORY RECEIPT_SHA256\n",
            "reel-workflow-native export-stage PACKET NATIVE_PROFILE ADMISSION_PROFILE TICKET TICKET_SHA256\n",
            "reel-workflow-native stage-response PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT RETURNS NEW_CANDIDATE --apply\n",
            "reel-workflow-native register-response PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT RETURNS NEW_CANDIDATE --apply\n",
            "reel-workflow-native ingest-response PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT RETURNS NEW_CANDIDATE --apply\n",
            "reel-workflow-native stage-retention PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT RETAINED_TARGET NEW_ACK --apply\n",
            "reel-workflow-native retain-response PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT RETAINED_TARGET NEW_ACK --apply\n",
            "reel-workflow-native verify-retained PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT ACK\n",
            "reel-workflow-native export-review-copy PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT ACK NEW_COPY [NEW_COMIC_MANIFEST] --apply\n",
            "reel-workflow-native create-handoff PACKET HANDOFF_PROFILE NEW_JOURNAL --apply\n",
            "reel-workflow-native import-handoff JOURNAL HANDOFF_PROFILE RESULT RETURNS --apply\n",
            "reel-workflow-native resume-handoff JOURNAL HANDOFF_PROFILE RETURNS_OR_DASH --apply\n",
            "Response staging records work; registration admits output claims; ingestion publishes native bytes.\n",
            "Retain-response acknowledges retained ownership; verify-retained checks live retained protection and bytes.\n",
            "Native handoff completion requires original work, actual retained-owner proof and exact output bytes.\n",
            "No automatic provider execution, cleanup or selection."
        ));
        return ExitCode::SUCCESS;
    }
    // Native producer validation can be nested beneath recipe/dispatch ingress.
    // Windows' default main-thread stack is too small for that qualified path.
    // All effects stay on one bounded worker thread in this same pinned process.
    let result = std::thread::Builder::new()
        .name("reel-workflow-native".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || run(&args))
        .map_err(|_| reel_workflow::Error {
            kind: reel_workflow::ErrorKind::NeedsRecovery,
            code: "native-execution-thread-create".into(),
        })
        .and_then(|thread| {
            thread.join().map_err(|_| reel_workflow::Error {
                kind: reel_workflow::ErrorKind::NeedsRecovery,
                code: "native-execution-thread-panic-preserve".into(),
            })
        })
        .and_then(|result| result);
    match result {
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
