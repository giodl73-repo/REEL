//! Explicit private S3 profile; no ambient access or initialization.
use super::{Result, document, raw, report};
use git_asset_core::Digest;
use git_asset_store::{
    Key, Request,
    ledger::{CapacityLedger, Checkpoint, JournalRef, LedgerError},
    local::LocalStore,
    orchestration::Place,
    s3::{self, Budget, Client, Plan, PlanRequest, Target},
};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
fn ledger_open_error(error: LedgerError) -> &'static str {
    match error {
        LedgerError::Busy => "busy",
        _ => "ledger_unavailable",
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    source_root: PathBuf,
    source: Place,
    witness_root: PathBuf,
    witness: Place,
    ledger_root: PathBuf,
    minimum: Checkpoint,
    job: JournalRef,
    target: Target,
    owner: Key,
    context: Digest,
    budget: Budget,
    put_url: String,
    get_url: String,
    #[serde(default)]
    allow_upload: bool,
    #[serde(default)]
    loopback_fixture: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    operation: Key,
    publication: Request,
}
impl Profile {
    fn validate(&self) -> Result<()> {
        if self.schema != "tig.s3-profile.design-1"
            || !self.source_root.is_absolute()
            || !self.witness_root.is_absolute()
            || !self.ledger_root.is_absolute()
        {
            return Err("invalid_profile");
        }
        self.budget.validate().map_err(|e| e.code())?;
        Ok(())
    }
    fn client(&self, publication: &Request) -> Result<Client> {
        self.validate()?;
        let client = Client::presigned(
            self.target.instance.clone(),
            self.target.generation,
            publication.object.clone(),
            &self.put_url,
            &self.get_url,
            self.loopback_fixture,
        )
        .map_err(|e| e.code())?;
        if client.target() != &self.target {
            return Err("invalid_access");
        }
        Ok(client)
    }
    fn admit(&self, p: &Plan) -> Result<()> {
        if p.source != self.source
            || p.witness != self.witness
            || p.target != self.target
            || p.owner != self.owner
            || p.context != self.context
            || p.budget != self.budget
        {
            return Err("stale_admission");
        }
        Ok(())
    }
}
pub(super) fn run(args: &[String]) -> Result<Vec<u8>> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err("usage");
    };
    if action == "describe" && args.len() == 1 {
        return report(
            json!({"schema":"tig.s3-interface.design-1","profile_schema":"tig.s3-profile.design-1",
        "operations":["plan PROFILE REQUEST","peak PROFILE PLAN","status PROFILE PLAN","apply PROFILE PLAN","resume PROFILE PLAN"],
        "max_document_bytes":65536,"max_plan_bytes":16384,"max_object_bytes":git_asset_store::local::MAX_OBJECT_BYTES,
        "default_apply":false,"retained_custody":false,"removal_authority":false,"initializes_stores":false}),
        );
    }
    if args.len() != 3 || !matches!(action, "plan" | "peak" | "status" | "apply" | "resume") {
        return Err("usage");
    }
    let profile: Profile = document(Path::new(&args[1]))?;
    profile.validate()?;
    let p = if action == "plan" {
        let input: Input = document(Path::new(&args[2]))?;
        if input.schema != "tig.s3-upload-request.design-1" {
            return Err("invalid_document");
        }
        // Resolve explicit existing capacity, without creating a provider/job.
        let capacity = CapacityLedger::open(&profile.ledger_root, &profile.minimum)
            .map_err(ledger_open_error)?;
        Plan::new(PlanRequest {
            operation: input.operation,
            owner: profile.owner.clone(),
            source: profile.source.clone(),
            publication: input.publication,
            witness: profile.witness.clone(),
            capacity: capacity.identity().clone(),
            target: profile.target.clone(),
            context: profile.context.clone(),
            budget: profile.budget.clone(),
        })
        .map_err(|e| e.code())?
    } else {
        Plan::from_json(&raw(Path::new(&args[2]))?).map_err(|e| e.code())?
    };
    profile.admit(&p)?;
    let client = profile.client(&p.publication)?;
    if action == "plan" {
        return p.to_json().map_err(|e| e.code());
    }
    if matches!(action, "apply" | "resume") && !profile.allow_upload {
        return Err("policy_denied");
    }
    let witness = LocalStore::open(&profile.witness_root).map_err(|_| "witness_unavailable")?;
    let mut ledger =
        CapacityLedger::open(&profile.ledger_root, &profile.minimum).map_err(ledger_open_error)?;
    if ledger.identity() != &p.capacity {
        return Err("wrong_capacity");
    }
    let value = if action == "peak" {
        json!({"schema":"tig.s3-peak.design-1","plan":p.id().map_err(|e|e.code())?,"local_witness_peak_bytes":p.required_peak(&witness).map_err(|e|e.code())?,"remote_quota_reserved":false})
    } else if action == "status" {
        serde_json::to_value(
            s3::status(&witness, &mut ledger, &profile.job, &client, &p).map_err(|e| e.code())?,
        )
        .map_err(|_| "response_budget")?
    } else {
        let source = LocalStore::open(&profile.source_root).map_err(|_| "source_unavailable")?;
        let result = s3::Execution {
            source: &source,
            witness: &witness,
            ledger: &mut ledger,
            job: &profile.job,
            client: &client,
            context: &profile.context,
        }
        .apply(&p)
        .map_err(|e| e.code())?;
        serde_json::to_value(result).map_err(|_| "response_budget")?
    };
    report(value)
}
