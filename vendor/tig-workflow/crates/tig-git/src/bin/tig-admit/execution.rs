use super::super::native_operation::fresh_operation;
use super::profile;
use tig_core::identity::canonical_json;
use tig_store::{
    accounted::{self, AccountedError},
    ledger::{CapacityLedger, LedgerError},
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
};

fn failure(error: AccountedError) -> String {
    let reason = match error {
        AccountedError::Ledger(LedgerError::Conflict) => "job binds different work",
        AccountedError::Ledger(LedgerError::Busy) => "capacity authority busy",
        AccountedError::Ledger(LedgerError::Capacity(_)) => "capacity admission refused",
        AccountedError::Ledger(LedgerError::Budget) => "capacity metadata budget exhausted",
        _ => "native admission unavailable",
    };
    format!("{reason}; metadata effects may exist; retry or reconcile exact input")
}
pub fn execute(input: profile::Input) -> Result<Vec<u8>, String> {
    let store = LocalStore::open(&input.destination.root)
        .map_err(|_| "cannot open existing destination")?;
    if store.identity() != &input.destination.place {
        return Err("destination identity changed".into());
    }
    let plan = accounted::plan(&store, &input.request)
        .map_err(|_| "request unsupported by native destination")?;
    if plan.required_peak() > input.maximum_peak {
        return Err("required peak exceeds explicit admission ceiling".into());
    }
    let mut ledger = CapacityLedger::open(&input.capacity.root, &input.capacity.minimum_checkpoint)
        .map_err(|_| "cannot open exact capacity authority")?;
    if ledger.identity() != &input.capacity.ledger {
        return Err("capacity authority identity changed".into());
    }
    if let Some(provider) = &input.provider {
        provider.preflight(&tig_store::provider::publication::native::capabilities(
            &store,
        ))?;
    }
    let receipt = match accounted::reserve_publication(&mut ledger, input.job.clone(), &plan) {
        Ok(receipt) => receipt,
        Err(AccountedError::Ledger(LedgerError::NeedsProbe)) => {
            let probe = LocalCapacityProbe::bind(&store, &mut ledger)
                .map_err(|_| "capacity enrollment unavailable")?;
            probe
                .observe(
                    &store,
                    &mut ledger,
                    fresh_operation("probe")?,
                    fresh_operation("sample")?,
                )
                .map_err(|_| "fresh capacity observation failed; metadata effects may exist")?;
            accounted::reserve_publication(&mut ledger, input.job, &plan).map_err(failure)?
        }
        Err(error) => return Err(failure(error)),
    };
    let mut output = serde_json::json!({"schema":"tig.acquisition-profile.experimental-1",
        "destination":input.destination,"admission":{"ledger_root":input.capacity.root,
            "ledger":ledger.identity(),"minimum_checkpoint":ledger.checkpoint(),"job":receipt.reference()},
        "plan":plan,"source":input.source});
    if let Some(provider) = input.provider {
        output["schema"] = serde_json::json!("tig.acquisition-profile.experimental-2");
        output["provider"] = serde_json::to_value(provider)
            .map_err(|_| "cannot encode provider context; reservation may exist")?;
    }
    canonical_json(&output)
        .map_err(|_| "cannot encode admission profile; reservation may exist".into())
}

impl crate::provider_profile::Envelope {
    pub(crate) fn preflight(
        &self,
        caps: &tig_store::provider::publication::Capabilities,
    ) -> Result<(), String> {
        self.profile
            .admit(&self.expected, &self.binding)
            .and_then(|guard| guard.check_publication(caps))
            .map_err(|error| format!("provider publication profile refused ({error})"))
    }
}
