//! Bounded metadata projection of an independently admitted frozen delivery.
//! Reuses the native lowerer; never opens primary media or adopts a job.
use super::*;
use tig_store::existing::RequiredProtection;

const SCHEMA: &str = "reel.tig-delivery-inspection-profile.experimental-1";
const MAX_CONTROLS: u64 = 4 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    source: ArchiveProfile,
    context: Selection,
    request: crate::tig_delivery_template::Request,
    producer: Object,
    recipe: Digest,
    max_control_bytes: u64,
}

fn inspect(path: &Path, stage: &mut &'static str) -> Result<Value> {
    let profile: Profile = read(path, 128 * 1024, None)?;
    if profile.schema != SCHEMA
        || !(1..=MAX_CONTROLS).contains(&profile.max_control_bytes)
        || profile.source.byte_source.required != RequiredProtection::PreventMutation
        || !profile.metadata_root.is_absolute()
        || !fs::symlink_metadata(&profile.metadata_root)?.is_dir()
    {
        bail!("invalid delivery inspection admission");
    }
    profile.request.validate()?;
    *stage = "frozen source replay";
    let mut reader = MetadataReader::open(&profile.metadata_root, &profile.metadata_minimum)?;
    let (mut source, context) = crate::tig_delivery_template::restore_context(
        profile.source,
        &profile.request,
        &profile.context,
        &mut reader,
        TreeLimits::default(),
    )?;
    *stage = "original recipe admission";
    let recipe = crate::tig_delivery_template::recipe(
        &mut source,
        &profile.request,
        &context,
        &profile.producer,
    )?;
    if recipe.fingerprint() != &profile.recipe {
        bail!("delivery inspection recipe differs");
    }
    *stage = "native control projection";
    let prepared = crate::tig_delivery_template::lower(&mut source, &profile.request)?;
    let raw = BTreeMap::from([
        ("job.json", serde_json::to_vec(&prepared.job)?),
        ("contract.json", prepared.contract_bytes),
        ("production.json", prepared.production_bytes),
    ]);
    let total = raw.values().try_fold(0u64, |sum, bytes| {
        sum.checked_add(bytes.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("control sum overflow"))
    })?;
    if total > profile.max_control_bytes {
        bail!("delivery controls exceed admission");
    }
    let controls = raw
        .into_iter()
        .map(|(name, bytes)| {
            let object = Object::from_bytes(&bytes);
            Ok((
                name,
                json!({"reference":{"path":name,"sha256":object.sha256,"bytes":object.bytes},
                    "utf8":String::from_utf8(bytes)?}),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(
        json!({"schema":"reel.tig-delivery-inspection.experimental-1",
        "source_checkpoint":source.checkpoint(),"context":profile.context,
        "producer_admission":profile.producer,"recipe":profile.recipe,
        "metadata_minimum":reader.checkpoint(),"controls":controls,
        "primary_bindings":prepared.consumed_bindings,"control_bytes":total,
        "preparation_only":true,"materialized_media_bytes":0,
        "execution_attested":false,"production_claim":null,
        "authorizes_production":false,"approval_transferred":false,
        "authorizes_eviction":false}),
    )
}

/// Read-only metadata inspection. Stdout is private operational payload, not
/// build acceptance, primary-byte availability, source selection or approval.
pub fn inspect_delivery(path: &Path) -> Result<Value> {
    let mut stage = "profile admission";
    inspect(path, &mut stage).map_err(|_| anyhow::anyhow!("delivery inspection refused at {stage}"))
}
