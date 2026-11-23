use super::{MAX_EXPORT_BYTES, Path, verified_manifest};
use crate::{admission, error, film_cli, staging};
use reel_workflow::{ErrorKind, Result, digest};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    request: PathBuf,
    request_sha256: String,
    ticket: PathBuf,
    ticket_sha256: String,
    action: Action,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Run,
    Resume,
    Verify,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dispatch {
    schema: String,
    export_directory: PathBuf,
    rebuild_request_sha256: String,
    export_sha256: String,
    editions: BTreeMap<String, Entry>,
}

pub fn execute(path: &Path, pin: &str) -> Result<serde_json::Value> {
    let (raw, _held) = staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if digest(&raw) != pin {
        return Err(error(ErrorKind::Integrity, "film-rebuild-dispatch-pin"));
    }
    let dispatch: Dispatch = reel_workflow::decode(&raw)?;
    if dispatch.schema != "reel.workflow-film-rebuild-dispatch.v1" {
        return Err(error(ErrorKind::Invalid, "film-rebuild-dispatch-schema"));
    }
    let export = verified_manifest(
        &dispatch.export_directory,
        &dispatch.rebuild_request_sha256,
        &dispatch.export_sha256,
    )?;
    if export.render_tasks.len() != dispatch.editions.len()
        || !export
            .render_tasks
            .iter()
            .all(|id| dispatch.editions.contains_key(id))
    {
        return Err(error(ErrorKind::Conflict, "film-rebuild-dispatch-editions"));
    }

    let mut operations = BTreeSet::new();
    let mut controls = Vec::new();
    let mut total = raw.len() as u64;
    for id in &export.render_tasks {
        let entry = &dispatch.editions[id];
        let (request, held_request) = film_cli::read(&entry.request, &entry.request_sha256)?;
        for (suffix, bytes) in [
            ("packet", admission::encode(&request.packet)?),
            ("source", admission::encode(&request.source)?),
        ] {
            let expected = &export.files[&format!("{id}-{suffix}.json")];
            if bytes.len() != expected.bytes || digest(&bytes) != expected.sha256 {
                return Err(error(ErrorKind::Conflict, "film-rebuild-dispatch-binding"));
            }
        }
        if !operations.insert(request.receipt.operation_identity()?) {
            return Err(error(
                ErrorKind::Conflict,
                "film-rebuild-dispatch-operation",
            ));
        }
        let (ticket, held_ticket) = film_cli::read_ticket(&entry.ticket)?;
        total += held_request.metadata().map_err(crate::io_error)?.len() + ticket.len() as u64;
        if total > MAX_EXPORT_BYTES as u64 {
            return Err(error(
                ErrorKind::Budget,
                "film-rebuild-dispatch-control-bytes",
            ));
        }
        let owned = film_cli::open(
            &request,
            &entry.request_sha256,
            &ticket,
            &entry.ticket_sha256,
        )?;
        controls.push((id, entry, request, owned, held_request, held_ticket));
    }

    let mut editions = BTreeMap::new();
    for (id, entry, request, owned, _held_request, _held_ticket) in controls {
        let action = match entry.action {
            Action::Run => film_cli::Action::Run,
            Action::Resume => film_cli::Action::Resume,
            Action::Verify => film_cli::Action::Verify,
        };
        let outcome = film_cli::execute_owned(&request, owned, action)?;
        editions.insert(id, outcome);
    }
    Ok(serde_json::json!({"ok":true,"editions":editions}))
}
