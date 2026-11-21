//! Actual SDK accessor projections for the recorded acquisition chapter.
use super::*;

pub(super) fn publication(
    receipt: &git_asset_store::accounted::AccountedReceipt,
    streamed: u64,
) -> Result<Value> {
    let git_asset_store::ledger::Response::FootprintCharged(settled) =
        receipt.settlement().response()
    else {
        return Err("unexpected acquisition settlement".into());
    };
    if *settled != receipt.charged_footprint() {
        return Err("acquisition charge differs from settlement".into());
    }
    Ok(
        json!({"request":receipt.publication().request(),"ack":receipt.publication().level(),
        "allocation":receipt.allocation(),"settlement":receipt.settlement().reference(),
        "checkpoint":receipt.settlement().checkpoint(),"was_replayed":receipt.settlement().was_replayed(),
        "charged_footprint":receipt.charged_footprint(),"basis":receipt.basis(),"streamed_bytes":streamed}),
    )
}

pub(super) fn copies(d: &Demo, before: &Value, after: &Value) -> Value {
    let mut rows = BTreeMap::new();
    for (name, object) in &d.objects {
        let roles = |snapshot: &Value| {
            ["backup", "history", "working"]
                .into_iter()
                .filter(|role| snapshot[*role].get(name).is_some())
                .collect::<Vec<_>>()
        };
        rows.insert(name,json!({"object":object,"before_copies":roles(before),"verified_copies":roles(after),"unknown_copies":[]}));
    }
    json!(rows)
}

pub(super) fn record(d: &mut Demo, observation: Value) -> Result<()> {
    let file = "reports/same-hash-reacquired.json";
    let object = save(&d.root.join(file), &observation)?;
    fs::copy(
        d.root.join(file),
        Layout::Flat.object_path(&d.root.join("lesson-evidence"), &object.sha256),
    )?;
    d.reports.push(json!({"lesson":"same-hash-reacquired","report":object,"file":file,
        "title":"Same bytes, new retained birth",
        "event":"Explicit synthetic ingest restores the original to backup under a new birth and retention grant. The completed old deletion retry removes no new bytes and preserves this copy. Recorded observation only."}));
    Ok(())
}
