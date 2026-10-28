//! Explicit file ingress through the existing publisher; no new cache or job.
use super::*;
use crate::tig_publication::{ExistingPublisher, Profile as ProviderProfile};
use anyhow::Context;
use tig_store::{Request as PublicationRequest, existing};

const SCHEMA: &str = "reel.tig-file-publication.experimental-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Publication {
    schema: String,
    artifact_root: PathBuf,
    namespace: Key,
    storage: Config,
    destination: PathBuf,
    reader_limits: existing::Limits,
    protection: existing::RequiredProtection,
    request: PublicationRequest,
    source: PathBuf,
}
impl Publication {
    fn open(&self) -> Result<ExistingPublisher> {
        Ok(ExistingPublisher::open(ProviderProfile {
            artifact_root: self.artifact_root.clone(),
            namespace: self.namespace.clone(),
            storage: self.storage.clone(),
            destination: self.destination.clone(),
            reader_limits: self.reader_limits,
            protection: self.protection,
        })?)
    }
}
fn source(path: &Path, expected: &Object) -> Result<fs::File> {
    if !fs::symlink_metadata(path)?.is_file() {
        bail!("source is not a regular file");
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1); // Read sharing only: exclude writes and deletion.
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != expected.bytes {
        bail!("source shape differs from exact publication object");
    }
    Ok(file)
}
fn publish(profile: &Path, output: &Path) -> Result<Value> {
    let raw = read_bytes(profile, 65_536, None)?;
    let admitted: Publication = serde_json::from_value(identity::parse_json(&raw)?)?;
    if admitted.schema != SCHEMA || !admitted.source.is_absolute() || !output.is_absolute() {
        bail!("invalid file publication admission or output");
    }
    match fs::symlink_metadata(output) {
        Ok(_) => bail!("publication output already exists"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    let parent = output
        .parent()
        .context("publication output parent missing")?
        .canonicalize()?;
    if !parent.is_dir() {
        bail!("publication output parent is not a directory");
    }
    for root in admitted
        .storage
        .cache_roots
        .iter()
        .chain(admitted.storage.coordination_root.iter())
    {
        if !root.is_absolute()
            || parent.starts_with(root)
            || root
                .canonicalize()
                .is_ok_and(|known| parent.starts_with(known))
        {
            bail!("receipt cannot enter managed cache/control paths");
        }
    }
    let mut publisher = admitted.open()?;
    // Recovery may succeed without opening a deleted/unavailable caller source.
    let recovered = tig_store::recover(&mut publisher, &admitted.request)?;
    let reused = recovered.is_some();
    let mut retained_source = None;
    if recovered.is_none() {
        let mut file = source(&admitted.source, &admitted.request.object)?;
        tig_store::publish(&mut publisher, &admitted.request, &mut file)?;
        retained_source = Some(file);
    }
    drop(publisher);
    let mut reopened = admitted.open()?;
    let receipt = tig_store::recover(&mut reopened, &admitted.request)?
        .context("completed publication unavailable after reopen")?;
    let value = json!({"schema":"reel.tig-file-publication-receipt.experimental-1",
        "profile":Object::from_bytes(&raw),"request":receipt.request(),"level":receipt.level(),
        "reopened_verified":true,"source_opened":retained_source.is_some(),
        "reused_verified_object":reused,"source_may_be_retired":false,
        "complete_job_enrollment":false,"independent_custody_verified":false,
        "creative_approval":false,"authorizes_eviction":false});
    let bytes = serde_json::to_vec(&value)?;
    if bytes.len() > 65_536 {
        bail!("publication receipt exceeds budget");
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("publication receipt already exists or cannot publish"))?;
    // Keep source and fresh cache guards through receipt publication.
    drop(retained_source);
    drop(reopened);
    Ok(
        json!({"schema":"reel.tig-file-publication-summary.experimental-1",
        "bytes":admitted.request.object.bytes,"reopened_verified":true,
        "reused_verified_object":reused,"source_may_be_retired":false,
        "complete_job_enrollment":false,"authorizes_eviction":false}),
    )
}
pub fn publish_file(profile: &Path, output: &Path) -> Result<Value> {
    publish(profile, output).map_err(|_| anyhow::anyhow!(
        "file publication refused; verify exact profile, source, roots, capacity, operation and output"))
}
