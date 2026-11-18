use super::{profile, source};
use tig_core::{Digest, identity::canonical_json};
use tig_store::{acquire, ledger::CapacityLedger, local::LocalStore, provider::publication};

pub fn execute(profile: profile::Profile) -> Result<Vec<u8>, String> {
    let mut source = source::Deferred::new(profile.source);
    let store = LocalStore::open(&profile.destination.root)
        .map_err(|_| "cannot open existing native destination")?;
    if store.identity() != &profile.destination.place {
        return Err("destination identity changed".into());
    }
    let mut ledger = CapacityLedger::open(
        &profile.admission.ledger_root,
        &profile.admission.minimum_checkpoint,
    )
    .map_err(|_| "cannot open exact capacity ledger")?;
    if ledger.identity() != &profile.admission.ledger {
        return Err("capacity ledger identity changed".into());
    }
    let mut publisher = publication::native::Accounted::new(
        &store,
        &mut ledger,
        &profile.admission.job,
        &profile.plan,
    );
    let result = if let Some(provider) = &profile.provider {
        let guard = provider
            .profile
            .admit(&provider.expected, &provider.binding)
            .map_err(|_| "provider profile changed before publication")?;
        let mut guarded = tig_store::provider::profile::Publisher::new(&mut publisher, &guard);
        acquire::publish_placed(
            &mut source,
            &mut guarded,
            &profile.destination.place,
            profile.plan.request(),
        )
    } else {
        acquire::publish_placed(
            &mut source,
            &mut publisher,
            &profile.destination.place,
            profile.plan.request(),
        )
    };
    let published = result.map_err(|e| {
        format!("acquisition failed ({e}); effects may exist; reconcile the exact operation")
    })?;
    let native_version = Digest::parse(
        std::str::from_utf8(published.version.as_bytes())
            .map_err(|_| "native version unavailable after publication; effects may exist")?,
    )
    .map_err(|_| "native version invalid after publication; effects may exist")?;
    let receipt = &published.evidence;
    let mut output = serde_json::json!({"schema":"tig.acquisition.experimental-1",
        "place":published.place,"request":published.request,"native_publication_version":native_version,
        "achieved_acknowledgement":published.level,"allocation":receipt.allocation(),
        "settlement_checkpoint":receipt.settlement().checkpoint(),"ledger_checkpoint":ledger.checkpoint(),
        "charged_file_data_footprint":receipt.charged_footprint(),"accounting_basis":receipt.basis(),
        "source_opened":source.was_opened(),"custody_established_by_command":false,
        "current_custody":"unknown","removal_authorized":false});
    if let Some(provider) = &profile.provider {
        output["schema"] = serde_json::json!("tig.acquisition.experimental-2");
        output["provider_profile"] = serde_json::json!(provider.expected);
        output["binding_generation"] = serde_json::json!(provider.binding.generation);
    }
    canonical_json(&output).map_err(|_| "cannot encode completed result; effects may exist".into())
}
