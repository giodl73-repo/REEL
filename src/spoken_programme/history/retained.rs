use super::*;
use std::{
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    time::{Duration, Instant},
};
use tig_assets::graph::recipe::produced::Claim;
use tig_store::existing::{ByteSource, ReadCache, RequiredProtection, VerifiedRead};

struct ProtectedObjects {
    pins: BTreeMap<String, Object>,
    held: BTreeMap<tig_core::Digest, VerifiedRead>,
}
impl ProtectedObjects {
    fn open(cache: &mut ReadCache, pins: &BTreeMap<String, Object>) -> Result<Self> {
        let mut held = BTreeMap::new();
        for pin in pins.values() {
            if !held.contains_key(&pin.sha256) {
                let value = cache
                    .open_verified(pin)
                    .map_err(|_| anyhow::anyhow!("spoken retained object unavailable"))?;
                held.insert(pin.sha256.clone(), value);
            }
        }
        Ok(Self {
            pins: pins.clone(),
            held,
        })
    }
    fn bytes(&mut self, name: &str, max: u64) -> Result<Vec<u8>> {
        let pin = self
            .pins
            .get(name)
            .context("spoken retained control missing")?;
        require(pin.bytes <= max, "spoken retained control exceeds budget")?;
        let held = self
            .held
            .get_mut(&pin.sha256)
            .context("spoken retained guard missing")?;
        held.seek(SeekFrom::Start(0))
            .map_err(|_| anyhow::anyhow!("spoken retained control seek failed"))?;
        let mut bytes = Vec::new();
        held.take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow::anyhow!("spoken retained control read failed"))?;
        require(
            Object::from_bytes(&bytes) == *pin,
            "spoken retained control pin differs",
        )?;
        Ok(bytes)
    }
}
impl InputFiles for ProtectedObjects {
    fn verified_path(&self, file: &FileRef) -> Result<PathBuf> {
        let pin = self
            .pins
            .get(
                file.path
                    .to_str()
                    .context("spoken retained virtual name invalid")?,
            )
            .context("spoken retained binding missing")?;
        require(*pin == object(file)?, "spoken retained binding pin differs")?;
        self.held
            .get(&pin.sha256)
            .context("spoken retained guard missing")?
            .with_path(Path::to_path_buf)
            .map_err(|_| anyhow::anyhow!("spoken retained protected path unavailable"))
    }
}

/// Reopened byte/context evidence, with all input/output guards still held.
/// No constructor/deserializer, automatic adoption or permanent custody claim.
pub struct Reopened {
    frozen: Frozen,
    checked: checker::CheckedPackage,
    receipt: NativeReceipt,
    _candidate: ProtectedObjects,
    _access: ProtectedObjects,
    _receipt: ProtectedObjects,
}
impl Reopened {
    pub fn receipt(&self) -> &NativeReceipt {
        &self.receipt
    }
    pub fn summary(&self) -> &checker::CheckSummary {
        self.checked.summary()
    }
    pub fn native_scope_verified(&self) -> bool {
        true
    }
    pub fn check_review_binding(
        &self,
        edition: &str,
        record: &crate::review_decision::ReviewDecisionRecord,
    ) -> Result<()> {
        crate::review_decision::validate_record(record)
            .map_err(|_| anyhow::anyhow!("spoken review record invalid"))?;
        let expected =
            Object::from_bytes(&candidate::target(&self.frozen, &self.checked, edition)?);
        require(
            record.target_kind == "spoken-edition"
                && record.target_sha256 == expected.sha256.as_str(),
            "spoken review binding stale",
        )
    }
}

/// The exact private receipt pin, original source admission and unsigned claim
/// are independently supplied. Nothing is discovered, repaired or materialized.
pub fn reopen(
    receipt_pin: &Object,
    profile: ArchiveProfile,
    edition: &str,
    claim: &Claim,
    nodes: &mut impl NodeStore,
    trees: TreeLimits,
) -> Result<Reopened> {
    require(
        cfg!(windows) && profile.byte_source.required == RequiredProtection::PreventMutation,
        "spoken retained protection unsupported",
    )?;
    require(
        receipt_pin.bytes <= 65536,
        "spoken retained receipt exceeds budget",
    )?;
    let native_limits = profile.limits;
    let mut cache = ReadCache::new(profile.byte_source.clone())
        .map_err(|_| anyhow::anyhow!("spoken retained enrollment unavailable"))?;
    let mut receipt_input = ProtectedObjects::open(
        &mut cache,
        &BTreeMap::from([("native-receipt".into(), receipt_pin.clone())]),
    )?;
    let raw = receipt_input.bytes("native-receipt", 65536)?;
    let value = identity::parse_json(&raw)
        .map_err(|_| anyhow::anyhow!("spoken retained receipt JSON invalid"))?;
    let receipt: NativeReceipt = serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("spoken retained receipt fields invalid"))?;
    require(
        receipt.schema == candidate::RECEIPT_SCHEMA
            && receipt.native_inputs_guarded
            && receipt.independent_pcm_check_passed
            && !receipt.authority_verified
            && !receipt.release_authorized
            && receipt.archive == profile.archive
            && receipt.checkpoint == profile.checkpoint,
        "spoken retained original admission differs",
    )?;
    require(
        identity::canonical_json(&serde_json::to_value(&receipt)?)? == raw,
        "spoken retained receipt is not canonical",
    )?;
    let frozen = restore(profile, &receipt.request, &receipt.contexts, nodes, trees)?;
    require(
        Object::from_bytes(&frozen.prepared.plan_bytes) == receipt.plan,
        "spoken retained plan differs",
    )?;
    let expected_inputs = frozen
        .prepared
        .files
        .iter()
        .map(|(id, file)| object(file).map(|pin| (id.clone(), pin)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    require(
        expected_inputs == receipt.source_inputs,
        "spoken retained input set differs",
    )?;
    let mut candidate_names = BTreeSet::from(["receipt.json".to_owned()]);
    let mut access_names = BTreeSet::from(["access.json".to_owned()]);
    for index in 0..frozen.prepared.plan.editions.len() {
        candidate_names.insert(format!("edition-{index:03}.wav"));
        access_names.insert(format!("edition-{index:03}.txt"));
    }
    require(
        receipt.candidate.keys().cloned().collect::<BTreeSet<_>>() == candidate_names
            && receipt.access.keys().cloned().collect::<BTreeSet<_>>() == access_names,
        "spoken retained output set differs",
    )?;
    let mut all = BTreeMap::new();
    for pin in receipt
        .source_inputs
        .values()
        .chain(receipt.candidate.values())
        .chain(receipt.access.values())
        .chain([receipt_pin, claim.object()])
    {
        require(
            all.insert(pin.sha256.clone(), pin.bytes)
                .is_none_or(|old| old == pin.bytes),
            "spoken retained object sizes conflict",
        )?;
    }
    let bytes = all
        .values()
        .try_fold(0u64, |sum, n| sum.checked_add(*n))
        .context("spoken retained byte budget overflow")?;
    require(
        all.len() <= 1024 && bytes <= MAX_RETAINED_BYTES,
        "spoken retained object budget exceeded",
    )?;
    let context = frozen.context(edition)?;
    require(
        claim.context().root == context.selection().root
            && claim.context().identity == context.selection().context
            && claim.operation() == frozen.recipe(edition)?.operation(),
        "spoken retained claim context/operation differs",
    )?;
    let (fields, payloads) = recipe_policies(&frozen.request, &frozen.prepared);
    let admission = tig_git::source::native::produced::Admission {
        profiles: &frozen.request.profiles,
        consumption: &fields,
        operation: frozen.recipe(edition)?.operation(),
        native: native_limits,
        tree: trees,
        query: limits(),
        recipe: graph::recipe::Limits {
            nodes: 1024,
            edges: 1024,
            bytes: 16 * 1024 * 1024,
        },
    };
    let (_, replay) = tig_git::source::native::produced::replay_with_payloads(
        claim, &admission, &payloads, nodes,
    )
    .map_err(|_| anyhow::anyhow!("spoken retained original claim replay failed"))?;
    require(
        replay.context().selection() == context.selection(),
        "spoken retained replay context differs",
    )?;
    let mut candidate_files = ProtectedObjects::open(&mut cache, &receipt.candidate)?;
    let mut access_files = ProtectedObjects::open(&mut cache, &receipt.access)?;
    let producer_receipt = candidate_files.bytes("receipt.json", 16 * 1024 * 1024)?;
    let access_bytes = access_files.bytes("access.json", 16 * 1024 * 1024)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut checked = checker::check(
        checker::CheckRequest {
            document: &frozen.prepared.document,
            plan_bytes: &frozen.prepared.plan_bytes,
            receipt_bytes: &producer_receipt,
            processing: &frozen.prepared.processing,
            expected_runtime: &frozen.prepared.runtime,
        },
        &frozen.prepared.inputs,
        &candidate_files,
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )?;
    checker::verify_access(&mut checked, &access_bytes, &access_files, || {
        require(
            Instant::now() < deadline,
            "spoken retained check deadline elapsed",
        )
    })?;
    require(
        receipt.recipes.len() == checked.access().editions.len()
            && receipt.review_targets.len() == checked.access().editions.len(),
        "spoken retained edition bindings differ",
    )?;
    for current in &checked.access().editions {
        let pin = receipt
            .recipes
            .get(&current.edition_id)
            .context("spoken retained recipe missing")?;
        require(
            pin.compiled == current.compiled_recipe_sha256
                && pin.execution == current.execution_recipe_sha256
                && pin.native == *frozen.recipe(&current.edition_id)?.fingerprint(),
            "spoken retained recipe differs",
        )?;
        require(
            receipt.review_targets.get(&current.edition_id)
                == Some(&Object::from_bytes(&candidate::target(
                    &frozen,
                    &checked,
                    &current.edition_id,
                )?)),
            "spoken retained review target differs",
        )?;
    }
    let selected = checked
        .access()
        .editions
        .iter()
        .find(|e| e.edition_id == edition)
        .context("spoken retained claimed edition missing")?;
    require(
        claim.outputs().len() == 1 && claim.outputs()[0].object() == &object(&selected.wave)?,
        "spoken retained claimed output differs",
    )?;
    Ok(Reopened {
        frozen,
        checked,
        receipt,
        _candidate: candidate_files,
        _access: access_files,
        _receipt: receipt_input,
    })
}
