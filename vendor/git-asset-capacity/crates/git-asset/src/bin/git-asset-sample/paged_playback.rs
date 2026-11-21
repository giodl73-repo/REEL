//! Export only the exact bounded metadata closure read by SDK inspection.
use super::*;
use git_asset_core::tree::{NodeStore, TreeError, TreeResult};
use git_asset_store::final_retirement::paged::{self as selection, control};
use std::cell::Cell;
struct Recording {
    reader: git_asset_store::metadata::MetadataReader,
    root: PathBuf,
    used: Cell<u64>,
}
impl NodeStore for Recording {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        let used = self
            .used
            .get()
            .checked_add(object.bytes)
            .ok_or(TreeError::Budget)?;
        if object.bytes > 65536 || used > 1024 * 1024 {
            return Err(TreeError::Budget);
        }
        self.used.set(used);
        let bytes = self
            .reader
            .get(object)?
            .ok_or_else(|| TreeError::MissingNode(object.clone()))?;
        if Object::from_bytes(&bytes) != *object {
            return Err(TreeError::Integrity);
        }
        let path = Layout::Flat.object_path(&self.root, &object.sha256);
        if path.exists() {
            if fs::read(path).map_err(|_| TreeError::InvalidNode)? != bytes {
                return Err(TreeError::Integrity);
            }
        } else {
            fs::write(path, &bytes).map_err(|_| TreeError::InvalidNode)?;
        }
        Ok(Some(bytes))
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::InvalidNode)
    }
}
pub(super) fn record(
    d: &mut Demo,
    enrollment: &control::Enrollment,
    decision: &control::Decision,
    report: &control::Report,
    before: &Value,
    after: &Value,
    execution_time: u64,
) -> Result<()> {
    d.metadata.finish()?;
    let mut nodes = Recording {
        reader: d.metadata.finished_reader()?,
        root: d.root.join("lesson-evidence"),
        used: Cell::new(0),
    };
    control::inspect(
        enrollment,
        decision,
        &mut nodes,
        &selection::Limits::default(),
    )?;
    let mut observations = BTreeMap::new();
    for (name, object) in &d.objects {
        let roles = |snapshot: &Value| {
            ["backup", "history", "working"]
                .into_iter()
                .filter(|r| snapshot[*r].get(name).is_some())
                .collect::<Vec<_>>()
        };
        observations.insert(name,json!({"object":object,"before_copies":roles(before),"verified_copies":roles(after),"unknown_copies":[]}));
    }
    let file = "reports/aged-category-final-loss.json";
    let commit = git(&d.git, &d.root.join("project"), &["rev-parse", "HEAD"])?;
    let object = save(
        &d.root.join(file),
        &json!({"schema":"tig.teaching-paged-final-observation.design-1","lesson":"aged-category-final-loss",
        "source_commit":commit,"source_checkpoint":d.namespaces[0].checkpoint(),"enrollment":enrollment,
        "decision":serde_json::from_slice::<Value>(&decision.to_json()?)?,"receipt":report,
        "providers":d.config.providers.iter().map(|p|&p.place.instance).collect::<Vec<_>>(),"observations":observations,
        "synthetic_claim_consent":true,"synthetic_final_consent":true,"simulated_clock":true,"execution_time":{"earliest":execution_time,"latest":execution_time},"independent_hardware_backup_proven":false}),
    )?;
    fs::copy(
        d.root.join(file),
        Layout::Flat.object_path(&d.root.join("lesson-evidence"), &object.sha256),
    )?;
    d.reports.push(json!({"lesson":"aged-category-final-loss","report":object,"file":file,"title":"Category and retained-age final loss",
        "event":"Exact admitted category block and retained occurrence age; separate synthetic claim-release and final-loss consent. Early time refused; actual aged controller completed. Recorded observation only."}));
    Ok(())
}
