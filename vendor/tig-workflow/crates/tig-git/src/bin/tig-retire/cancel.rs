//! Stop new native admission without needing metadata or payload providers online.
use super::codec;
use tig_store::{Key, retirement_intent, source_claims::SourceJournal};

pub fn cancel(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let handle = codec::handle(bytes)?;
    let mut journal = SourceJournal::open(
        &handle.authorities.journal.root,
        &handle.authorities.journal.minimum_checkpoint,
    )
    .map_err(|_| "intent journal unavailable or below required witness")?;
    if journal.identity() != &handle.authorities.journal.identity {
        return Err("foreign cancellation journal".into());
    }
    let source = Key::parse(handle.operation.as_str()).map_err(|_| "invalid intent source")?;
    let state = journal
        .source(&source)
        .map_err(|_| "intent state unavailable")?
        .ok_or("intent not admitted; no cancellation performed")?;
    if state.owner != handle.enrollment.owner {
        return Err("foreign cancellation owner".into());
    }
    let reported = state.cancelled;
    if !reported {
        if state.revision != 1 && state.revision != 2 {
            return Err("unexpected cancellation revision".into());
        }
        let command = Key::parse(&format!("retire-stop-{}", handle.operation.as_str()))
            .map_err(|_| "invalid cancellation command")?;
        retirement_intent::cancel(
            &mut journal,
            &handle.enrollment.owner,
            &handle.operation,
            state.revision,
            command,
        )
        .map_err(|_| "cancellation uncertain; reconcile same operation")?;
    }
    codec::encode(&serde_json::json!({
        "schema":"tig.native-retirement-cancel-result.experimental-1",
        "operation":handle.operation,"cancelled":true,
        "reported_completion_reused":reported,
        "already_admitted_work_preserved":true,
        "payload_deleted":false,
        "last_confirmed_minimum_checkpoints":{"journal":journal.checkpoint()}
    }))
}
