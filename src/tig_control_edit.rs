//! File-pin-bound authored-control candidates. The caller's managed job owns
//! source protection, output retention and Git handoff; this worker grants none.
use crate::{scene_delivery::FileRef, tig_native_registration::DefinitionFiles};
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tig_assets::native::Instance;
use tig_core::{Digest, LogicalPath, Object, identity};

pub const SCHEMA: &str = "reel.tig-control-edit-candidate.experimental-1";
const LINEAGE: &str = "reel.control-edit-lineage.v1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    asset_id: String,
    original_record: FileRef,
    definition: DefinitionFiles,
    expected_selected: Object,
    replacement: FileRef,
    edit_request: FileRef,
    source_checkpoint: Digest,
    max_control_bytes: u64,
    #[serde(default)]
    payload_filename: Option<String>,
}
struct Prepared {
    record: Vec<u8>,
    payload: Vec<u8>,
    payload_filename: Option<String>,
    _held: Vec<fs::File>,
}
fn object(file: &FileRef) -> Result<Object> {
    Ok(Object {
        sha256: Digest::parse(&file.sha256)?,
        bytes: file.bytes,
    })
}
fn local(root: &Path, path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    LogicalPath::parse(
        path.to_str()
            .ok_or_else(|| anyhow::anyhow!("nonportable path"))?,
    )?;
    Ok(root.join(path))
}
fn read(root: &Path, pin: &FileRef, maximum: u64, held: &mut Vec<fs::File>) -> Result<Vec<u8>> {
    if pin.bytes > maximum {
        bail!("control input exceeds budget");
    }
    let path = local(root, &pin.path)?;
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() {
        bail!("ordinary control file required");
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            bail!("reparse control refused");
        }
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options.open(path)?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if Object::from_bytes(&bytes) != object(pin)? {
        bail!("control input pin differs");
    }
    held.push(file);
    Ok(bytes)
}
fn prepare(profile: &Path) -> Result<Prepared> {
    let root = profile
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut raw = Vec::new();
    fs::File::open(profile)?
        .take(65_537)
        .read_to_end(&mut raw)?;
    let p: Profile = serde_json::from_value(identity::parse_json(&raw)?)?;
    if p.schema != SCHEMA || p.max_control_bytes == 0 || p.max_control_bytes > 4 * 1024 * 1024 {
        bail!("unsupported control candidate profile");
    }
    let mut held = Vec::new();
    let original = read(root, &p.original_record, 65_536, &mut held)?;
    let definition = read(root, &p.definition.definition, 65_536, &mut held)?;
    let schema = read(root, &p.definition.authored_schema, 65_536, &mut held)?;
    let bound = Instance::parse(&original)?.bind(&definition, &schema)?;
    let mut record = serde_json::to_value(bound.instance().value())?;
    if record["id"] != p.asset_id {
        bail!("control target identity differs");
    }
    if bound
        .instance()
        .value()
        .selected
        .as_ref()
        .map(|pin| pin.object())
        != Some(p.expected_selected.clone())
    {
        bail!("predecessor selection differs");
    }
    let payload = read(root, &p.replacement, p.max_control_bytes, &mut held)?;
    let request = read(root, &p.edit_request, 65_536, &mut held)?;
    // Native JSON controls may contain decimal values. Their semantic schema,
    // source locks and correspondence are checked by the owning editor/adapter.
    if !serde_json::from_slice::<Value>(&payload)?.is_object()
        || !serde_json::from_slice::<Value>(&request)?.is_object()
        || object(&p.replacement)? == p.expected_selected
    {
        bail!("changed object controls required");
    }
    if !record["metadata"]["reel_control_edit"].is_null()
        && record["metadata"]["reel_control_edit"]["schema"] != LINEAGE
    {
        bail!("reserved lineage metadata conflicts");
    }
    let previous_status = record["status"].clone();
    let replacement = object(&p.replacement)?;
    record["selected"]["sha256"] = json!(replacement.sha256);
    record["selected"]["bytes"] = json!(replacement.bytes);
    record["status"] = json!({"selection_state":"draft-unreviewed","human_approval":false,
        "production_state":"unclassified","subject":replacement});
    record["metadata"]["reel_control_edit"] = json!({"schema":LINEAGE,
        "candidate_profile":Object::from_bytes(&raw),
        "predecessor_record":object(&p.original_record)?,"predecessor_selected":p.expected_selected,
        "predecessor_status":previous_status,"edit_request":object(&p.edit_request)?,
        "declared_source_checkpoint":p.source_checkpoint,"production_claim":null,
        "source_ownership_verified":false,"complete_semantic_coverage":false});
    let bytes = serde_json::to_vec_pretty(&record)?;
    Instance::parse(&bytes)?.bind(&definition, &schema)?;
    Ok(Prepared {
        record: bytes,
        payload,
        payload_filename: p.payload_filename,
        _held: held,
    })
}
fn publish(bytes: &[u8], output: &Path) -> Result<()> {
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("output parent missing"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("new output required"))?;
    Ok(())
}
pub fn run(profile: &Path, output: &Path) -> Result<Value> {
    let execute = || -> Result<Value> {
        let prepared = prepare(profile)?;
        let output = local(&std::env::current_dir()?, output)?;
        let parent = output
            .parent()
            .ok_or_else(|| anyhow::anyhow!("output parent missing"))?;
        if output.symlink_metadata().is_ok() {
            bail!("record output already exists");
        }
        if let Some(name) = &prepared.payload_filename {
            let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
            if name.len() > 128
                || !name.ends_with(".json")
                || name.starts_with('.')
                || name
                    .chars()
                    .any(|c| !c.is_ascii_alphanumeric() && !"-_.".contains(c))
                || [
                    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
                    "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7",
                    "LPT8", "LPT9",
                ]
                .contains(&stem.as_str())
                || output
                    .file_name()
                    .is_some_and(|file| file.to_string_lossy().eq_ignore_ascii_case(name))
            {
                bail!("portable distinct control filename required");
            }
            let payload_output = parent.join(name);
            if payload_output.symlink_metadata().is_ok() {
                bail!("control output already exists");
            }
            publish(&prepared.payload, &payload_output)?;
        }
        publish(&prepared.record, &output)?;
        Ok(
            json!({"schema":"reel.tig-control-edit-candidate-summary.experimental-1",
            "records":1,"record_bytes":prepared.record.len(),"payload_copied":prepared.payload_filename.is_some(),
            "source_ownership_verified":false,"outputs_retained":false,"adopted":false,
            "creative_approval":false,"production_claim":null}),
        )
    };
    execute().map_err(|_| {
        anyhow::anyhow!(
            "control edit candidate refused; verify exact inputs, definition, draft and new output"
        )
    })
}
