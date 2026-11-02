//! Private, bounded note authoring over an explicitly captured Tig namespace.
//! No dependency discovery, musical validation, publication or Git mutation.
use crate::tig_inputs::{Profile, Source};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tig_assets::{graph::Reference, native::Instance};
use tig_core::{Digest, Object};
use tig_store::existing::VerifiedRead;

const LOCK: &str = "reel.tig-music-editor-lock.experimental-1";
const LINEAGE: &str = "reel.tig-music-note-edit.experimental-1";
const MODEL_BYTES: usize = 2 * 1024 * 1024;
/// Project definitions explicitly admit independently editable native models.
pub const AUTHORING_PROFILE: &str = "reel.music-note-authoring.v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Lock {
    schema: String,
    checkpoint: Digest,
    record: Reference,
    definition: Reference,
    subject: Object,
}
struct Captured {
    lock: Lock,
    raw: Vec<u8>,
    _held: VerifiedRead,
}
pub struct Editor {
    source: Source,
    cache_roots: Vec<PathBuf>,
}
impl Editor {
    pub fn open(profile: Profile) -> Result<Self> {
        let cache_roots = profile
            .byte_source
            .places
            .iter()
            .map(|p| p.root.clone())
            .collect();
        Ok(Self {
            source: Source::open(profile)?,
            cache_roots,
        })
    }
    fn capture(&mut self, id: &str) -> Result<Captured> {
        let record = self.source.record(id)?;
        let file = self.source.authoring_file(id)?;
        if file.bytes > MODEL_BYTES as u64 {
            bail!("model exceeds authoring budget");
        }
        let (raw, held) = self.source.authoring_snapshot(&file, MODEL_BYTES)?;
        let subject = Object::from_bytes(&raw);
        if subject.sha256.as_str() != file.sha256 || subject.bytes != file.bytes {
            bail!("captured model pin changed");
        }
        Ok(Captured {
            lock: Lock {
                schema: LOCK.into(),
                checkpoint: self.source.checkpoint().clone(),
                record: record.reference().clone(),
                definition: record.definition().clone(),
                subject,
            },
            raw,
            _held: held,
        })
    }
    fn destination(&self, output: &Path) -> Result<()> {
        if !output.is_absolute()
            || fs::symlink_metadata(output).is_ok()
            || output.components().any(|c| {
                matches!(c, std::path::Component::ParentDir)
                    || c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git")
            })
        {
            bail!("authoring requires a new absolute destination outside Git internals");
        }
        let parent = output
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing destination parent"))?;
        for ancestor in parent.ancestors() {
            let metadata = fs::symlink_metadata(ancestor)?;
            #[cfg(windows)]
            let reparse = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let reparse = metadata.file_type().is_symlink();
            if reparse || !metadata.is_dir() {
                bail!("authoring destination ancestor is not an ordinary directory");
            }
        }
        let parent = parent.canonicalize()?;
        for cache in &self.cache_roots {
            if parent.starts_with(cache.canonicalize().unwrap_or_else(|_| cache.clone())) {
                bail!(
                    "authoring workspace must be outside byte-provider roots; publish separately"
                );
            }
        }
        Ok(())
    }
    pub fn export(&mut self, id: &str, output: &Path) -> Result<usize> {
        self.destination(output)?;
        let captured = self.capture(id)?;
        let lock = serde_json::to_vec(&captured.lock)?;
        let count = reel_music::note_editor::export_bytes(&captured.raw, output)?;
        write(&output.join("source-lock.json"), &lock)?;
        Ok(count)
    }
    /// A locked unreviewed candidate, never an adopted Git change or CAS object.
    pub fn propose(&mut self, lock_path: &Path, edit_path: &Path, output: &Path) -> Result<usize> {
        self.destination(output)?;
        let lock_raw = read(lock_path, 65_536)?;
        let old: Lock = serde_json::from_slice(&lock_raw)?;
        if old.schema != LOCK {
            bail!("unsupported music source lock");
        }
        let captured = self.capture(&old.record.id)?;
        // Unrelated source changes are allowed; the edited record, definition
        // and exact model must remain identical. No other dependency is consumed.
        if old.record != captured.lock.record
            || old.definition != captured.lock.definition
            || old.subject != captured.lock.subject
        {
            bail!("stale music asset, definition or subject lock");
        }
        let (original, definition, schema) = self.source.record_documents(&old.record.id)?;
        if serde_json::from_slice::<serde_json::Value>(&definition)?["semantic_rules"]["profile"]
            != AUTHORING_PROFILE
        {
            bail!("model definition does not admit native note authoring");
        }
        let edit = read(edit_path, MODEL_BYTES)?;
        let (candidate, count) = reel_music::note_editor::propose_bytes(&captured.raw, &edit)?;
        let replacement = Object::from_bytes(&candidate);
        let bound = Instance::parse(&original)?.bind(&definition, &schema)?;
        let mut record = serde_json::to_value(bound.instance().value())?;
        if !record["metadata"]["reel_music_edit"].is_null()
            && record["metadata"]["reel_music_edit"]["schema"] != LINEAGE
        {
            bail!("reserved music lineage metadata conflicts");
        }
        let previous_status = record["status"].clone();
        record["selected"]["sha256"] = json!(replacement.sha256);
        record["selected"]["bytes"] = json!(replacement.bytes);
        record["selected"]["media_type"] = json!("application/json");
        record["status"] = json!({"selection_state":"draft-unreviewed","human_approval":false,
            "production_state":"unclassified","subject":replacement});
        record["metadata"]["reel_music_edit"] = json!({"schema":LINEAGE,
            "predecessor_record":old.record,"predecessor_selected":old.subject,
            "predecessor_status":previous_status,"source_lock":Object::from_bytes(&lock_raw),
            "edit_request":Object::from_bytes(&edit),"checkpoint":captured.lock.checkpoint,
            "production_claim":null,"native_validation":"not-run","dependency_names_preserved":true});
        let record = serde_json::to_vec_pretty(&record)?;
        if record.len() > 65_536 {
            bail!("candidate record exceeds authoring budget");
        }
        Instance::parse(&record)?.bind(&definition, &schema)?;
        let receipt = serde_json::to_vec(
            &json!({"schema":"reel.tig-music-note-proposal.experimental-1",
            "expected_record":old.record,"replacement_record":Object::from_bytes(&record),
            "replacement_model":replacement,"changed_notes":count,"native_validation":"not-run",
            "dependency_resolution":"requires the original logical binding context; no standalone path validation",
            "published":false,"git_changed":false,"human_approval":false,"authorizes_eviction":false}),
        )?;
        fs::create_dir(output)?;
        for (name, bytes) in [
            ("model.json", candidate.as_slice()),
            ("record.tig", record.as_slice()),
            ("source-lock.json", lock_raw.as_slice()),
            ("edit.json", edit.as_slice()),
            ("predecessor.tig", original.as_slice()),
            ("definition.json", definition.as_slice()),
            ("authored-schema.json", schema.as_slice()),
            ("receipt.json", receipt.as_slice()),
        ] {
            write(&output.join(name), bytes)?;
        }
        Ok(count)
    }
}
fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = metadata.file_type().is_symlink();
    if reparse || !metadata.is_file() || metadata.len() > limit as u64 {
        bail!("bounded ordinary editor input required");
    }
    let mut raw = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > limit {
        bail!("editor input exceeds budget");
    }
    Ok(raw)
}
fn write(path: &Path, raw: &[u8]) -> Result<()> {
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(raw)?;
    Ok(())
}
