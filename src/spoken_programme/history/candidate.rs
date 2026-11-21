use super::*;
use crate::{
    review_decision::ReviewDecisionRecord,
    tig_scene_production::{DeliveryBudget, Publication, PublishedCandidate},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tig_assets::graph::recipe::produced::{BoundSubject, Claim, ContextPin};
use tig_store::PublicationBackend;

pub const RECEIPT_SCHEMA: &str = "reel.spoken-native-receipt.v1";
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipePins {
    pub compiled: String,
    pub execution: String,
    pub native: tig_core::Digest,
}

/// Private exact operation evidence, not a portable report or owner adoption.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeReceipt {
    pub schema: String,
    pub request: Request,
    pub archive: Object,
    pub checkpoint: tig_core::Digest,
    pub contexts: BTreeMap<String, Selection>,
    pub plan: Object,
    pub source_inputs: BTreeMap<String, Object>,
    pub candidate: BTreeMap<String, Object>,
    pub access: BTreeMap<String, Object>,
    pub recipes: BTreeMap<String, RecipePins>,
    pub review_targets: BTreeMap<String, Object>,
    pub native_inputs_guarded: bool,
    pub independent_pcm_check_passed: bool,
    pub authority_verified: bool,
    pub release_authorized: bool,
}

/// Sealed by protected native preparation plus M04 media/access verification.
/// Keep the Frozen owner alive through publication; its original inputs stay guarded.
pub struct NativeCandidate<'a> {
    frozen: &'a Frozen,
    checked: checker::CheckedPackage,
    receipt: NativeReceipt,
    receipt_bytes: Vec<u8>,
    candidate: PathBuf,
    access: PathBuf,
}
impl NativeCandidate<'_> {
    pub fn receipt(&self) -> &NativeReceipt {
        &self.receipt
    }
    pub fn receipt_bytes(&self) -> &[u8] {
        &self.receipt_bytes
    }
    pub fn wave(&self, edition: &str) -> Result<&FileRef> {
        self.checked
            .access()
            .editions
            .iter()
            .find(|e| e.edition_id == edition)
            .map(|e| &e.wave)
            .context("spoken native edition wave missing")
    }
    pub fn review_target(&self, edition: &str) -> Result<Vec<u8>> {
        target(self.frozen, &self.checked, edition)
    }
    /// Validates existing record structure/exact target binding only. It grants
    /// no reviewer authentication, listening pass, selection or approval.
    pub fn check_review_binding(&self, edition: &str, record: &ReviewDecisionRecord) -> Result<()> {
        crate::review_decision::validate_record(record)
            .map_err(|_| anyhow::anyhow!("spoken review record invalid"))?;
        require(
            record.target_kind == "spoken-edition"
                && record.target_sha256
                    == Object::from_bytes(&self.review_target(edition)?)
                        .sha256
                        .as_str(),
            "spoken review binding stale",
        )
    }
    pub fn publish_edition(
        &self,
        edition: &str,
        subject: &BoundSubject,
        publication: &Publication,
        backend: &mut impl PublicationBackend,
        budget: &DeliveryBudget,
    ) -> Result<PublishedCandidate> {
        require(
            budget.max_objects > 0
                && budget.max_objects <= 1024
                && budget.max_bytes > 0
                && budget.max_bytes <= MAX_RETAINED_BYTES,
            "spoken retained publication budget invalid",
        )?;
        // Reopen and rederive again immediately before any provider reservation.
        let fresh = check(self.frozen, &self.candidate, &self.access)?;
        require(
            fresh.receipt_bytes == self.receipt_bytes,
            "spoken native candidate changed after checking",
        )?;
        let selected = self
            .checked
            .access()
            .editions
            .iter()
            .find(|e| e.edition_id == edition)
            .context("spoken published edition missing")?;
        require(
            subject.subject().object() == &object(&selected.wave)?,
            "spoken output subject differs from checked wave",
        )?;
        let context = self.frozen.context(edition)?;
        let claim = Claim::create(
            self.frozen.recipe(edition)?,
            ContextPin {
                root: context.selection().root.clone(),
                identity: context.selection().context.clone(),
            },
            vec![subject.subject().clone()],
        )
        .map_err(|_| anyhow::anyhow!("spoken native claim construction failed"))?;
        let mut files = BTreeMap::new();
        for reference in self.frozen.prepared.files.values() {
            let pin = object(reference)?;
            let path = self
                .frozen
                .prepared
                .inputs
                .verified_path(reference)
                .map_err(|_| anyhow::anyhow!("spoken retained input unavailable"))?;
            files.insert(pin.sha256.clone(), (pin, path));
        }
        for (root, pins) in [
            (&self.candidate, &self.receipt.candidate),
            (&self.access, &self.receipt.access),
        ] {
            for (name, pin) in pins {
                let reference = FileRef {
                    path: name.into(),
                    sha256: pin.sha256.as_str().into(),
                    bytes: pin.bytes,
                };
                let path = root
                    .verified_path(&reference)
                    .map_err(|_| anyhow::anyhow!("spoken retained output unavailable"))?;
                if let Some((previous, _)) = files.insert(pin.sha256.clone(), (pin.clone(), path)) {
                    require(previous == *pin, "spoken retained payload size conflict")?;
                }
            }
        }
        let outputs = files
            .into_values()
            .map(|(pin, path)| {
                let name = pin.sha256.as_str().to_owned();
                publication
                    .request("output", &name, pin)
                    .map(|request| (request, path))
            })
            .collect::<Result<Vec<_>>>()?;
        crate::tig_scene_production::publication::publish_checked(
            claim,
            &self.receipt_bytes,
            &Object::from_bytes(&self.receipt_bytes),
            outputs,
            None,
            publication,
            backend,
            Some(budget),
        )
        .map_err(|_| anyhow::anyhow!("spoken retained publication failed"))
    }
}

pub(super) fn target(
    frozen: &Frozen,
    checked: &checker::CheckedPackage,
    edition: &str,
) -> Result<Vec<u8>> {
    let access = checked
        .access()
        .editions
        .iter()
        .find(|e| e.edition_id == edition)
        .context("spoken review edition missing")?;
    identity::canonical_json(&serde_json::json!({"schema":"reel.spoken-review-target.v1",
        "edition_id":edition,"compiled_recipe":access.compiled_recipe_sha256,"execution_recipe":access.execution_recipe_sha256,
        "native_recipe":frozen.recipe(edition)?.fingerprint(),"producer":object(&frozen.request.producer)?,
        "wave":object(&access.wave)?,"text":Object::from_bytes(&serde_json::to_vec(access)?)}))
        .map_err(|_|anyhow::anyhow!("spoken review target serialization failed"))
}

pub(super) fn check<'a>(
    frozen: &'a Frozen,
    candidate: &Path,
    access: &Path,
) -> Result<NativeCandidate<'a>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut tick = || {
        require(
            Instant::now() < deadline,
            "spoken native check deadline elapsed",
        )
    };
    let producer_receipt = producer::bounded(&candidate.join("receipt.json"), 16 * 1024 * 1024)?;
    let access_bytes = producer::bounded(&access.join("access.json"), 16 * 1024 * 1024)?;
    let mut checked = checker::check(
        checker::CheckRequest {
            document: &frozen.prepared.document,
            plan_bytes: &frozen.prepared.plan_bytes,
            receipt_bytes: &producer_receipt,
            processing: &frozen.prepared.processing,
            expected_runtime: &frozen.prepared.runtime,
        },
        &frozen.prepared.inputs,
        candidate,
        producer::ProductionControl {
            timeout: deadline.saturating_duration_since(Instant::now()),
            cancellation: || Ok(()),
        },
    )?;
    checker::verify_access(&mut checked, &access_bytes, access, &mut tick)?;
    let mut candidate_files =
        BTreeMap::from([("receipt.json".into(), Object::from_bytes(&producer_receipt))]);
    let mut access_files =
        BTreeMap::from([("access.json".into(), Object::from_bytes(&access_bytes))]);
    let mut recipes = BTreeMap::new();
    let mut review_targets = BTreeMap::new();
    let mut contexts = BTreeMap::new();
    let first = frozen
        .contexts
        .values()
        .next()
        .context("spoken native context set empty")?;
    for edition in &checked.access().editions {
        let context = frozen.context(&edition.edition_id)?;
        require(
            context.archive() == first.archive()
                && context.requirements().checkpoint() == first.requirements().checkpoint(),
            "spoken native contexts differ in source archive",
        )?;
        contexts.insert(edition.edition_id.clone(), context.selection().clone());
        candidate_files.insert(
            edition
                .wave
                .path
                .to_str()
                .context("spoken output name invalid")?
                .into(),
            object(&edition.wave)?,
        );
        access_files.insert(
            edition
                .transcript
                .path
                .to_str()
                .context("spoken text name invalid")?
                .into(),
            object(&edition.transcript)?,
        );
        recipes.insert(
            edition.edition_id.clone(),
            RecipePins {
                compiled: edition.compiled_recipe_sha256.clone(),
                execution: edition.execution_recipe_sha256.clone(),
                native: frozen.recipe(&edition.edition_id)?.fingerprint().clone(),
            },
        );
        review_targets.insert(
            edition.edition_id.clone(),
            Object::from_bytes(&target(frozen, &checked, &edition.edition_id)?),
        );
    }
    let source_inputs = frozen
        .prepared
        .files
        .iter()
        .map(|(id, file)| object(file).map(|pin| (id.clone(), pin)))
        .collect::<Result<_>>()?;
    let receipt = NativeReceipt {
        schema: RECEIPT_SCHEMA.into(),
        request: frozen.request.clone(),
        archive: first.archive().clone(),
        checkpoint: first.requirements().checkpoint().clone(),
        contexts,
        plan: Object::from_bytes(&frozen.prepared.plan_bytes),
        source_inputs,
        candidate: candidate_files,
        access: access_files,
        recipes,
        review_targets,
        native_inputs_guarded: true,
        independent_pcm_check_passed: true,
        authority_verified: false,
        release_authorized: false,
    };
    let receipt_bytes = identity::canonical_json(&serde_json::to_value(&receipt)?)?;
    require(
        receipt_bytes.len() <= 65536,
        "spoken native receipt exceeds budget",
    )?;
    tick()?;
    Ok(NativeCandidate {
        frozen,
        checked,
        receipt,
        receipt_bytes,
        candidate: candidate.into(),
        access: access.into(),
    })
}
