//! Exact-text underlay authoring through explicit native Tig bindings.
use crate::tig_music_editor::{Captured, Editor, Lock, Proposal, read, write};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

const LOCK: &str = "reel.tig-music-underlay-lock.experimental-1";
const UNDERLAY: &str = "reel.music-underlay-authoring.v1";
const BINDINGS: &str = "reel.music-model-input-bindings.v1";
const TEXT: &str = "reel.music-lyric-source.v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UnderlayLock {
    schema: String,
    model: Lock,
    lyric: Lock,
    layer_id: String,
}
struct Pair {
    model: Captured,
    lyric: Captured,
    layer_id: String,
}
impl Pair {
    fn lock(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(
            &json!({"schema":LOCK,"model":self.model.lock,
            "lyric":self.lyric.lock,"layer_id":self.layer_id}),
        )?)
    }
}
fn unchanged(old: &Lock, current: &Lock) -> bool {
    old.record == current.record
        && old.definition == current.definition
        && old.subject == current.subject
        && old.schema == current.schema
}
impl Editor {
    fn underlay_pair(&mut self, id: &str) -> Result<Pair> {
        let model = self.capture(id)?;
        let (record, definition, _) = self.source.record_documents(id)?;
        let definition: Value = serde_json::from_slice(&definition)?;
        if definition["semantic_rules"]["underlay_profile"] != UNDERLAY
            || definition["semantic_rules"]["binding_profile"] != BINDINGS
        {
            bail!("model definition does not admit native underlay bindings");
        }
        let native: reel_music::model::MusicModel = serde_yaml::from_slice(&model.raw)?;
        let layer_id = native
            .lead_sheet
            .as_ref()
            .and_then(|s| s.lyric_layer_id.as_ref())
            .ok_or_else(|| anyhow::anyhow!("model has no selected native lyric layer"))?;
        let layers: Vec<_> = native
            .lyric_layers
            .iter()
            .filter(|l| &l.id == layer_id)
            .collect();
        if layers.len() != 1 {
            bail!("selected native lyric layer is absent or ambiguous");
        }
        let layer = layers[0];
        let record: Value = serde_json::from_slice(&record)?;
        let reference = &record["authored"]["binding_ids"][format!("lyric:{layer_id}")];
        if reference.as_object().is_none_or(|r| r.len() != 1) {
            bail!("selected lyric needs one explicit Tig ID binding");
        }
        let lyric_id = reference["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("selected lyric has no Tig ID binding"))?;
        let lyric = self.capture_bounded(lyric_id, 128 * 1024)?;
        let (lyric_record, lyric_definition, _) = self.source.record_documents(lyric_id)?;
        let lyric_record: Value = serde_json::from_slice(&lyric_record)?;
        let lyric_definition: Value = serde_json::from_slice(&lyric_definition)?;
        if lyric_definition["semantic_rules"]["profile"] != TEXT
            || lyric_record["authored"]["language"] != layer.language
            || lyric_record["authored"]["lyric_kind"] != serde_json::to_value(layer.kind)?
            || !lyric
                .lock
                .subject
                .sha256
                .as_str()
                .eq_ignore_ascii_case(&layer.sha256)
        {
            bail!("selected lyric kind, language or observed hash is stale or incompatible");
        }
        // Both byte-provider guards remain alive through export or proposal.
        Ok(Pair {
            model,
            lyric,
            layer_id: layer_id.clone(),
        })
    }
    pub fn export_underlay(&mut self, id: &str, output: &Path) -> Result<usize> {
        self.destination(output)?;
        let pair = self.underlay_pair(id)?;
        let lock = pair.lock()?;
        let count =
            reel_music::underlay_editor::export_bytes(&pair.model.raw, &pair.lyric.raw, output)?;
        write(&output.join("source-lock.json"), &lock)?;
        Ok(count)
    }
    pub fn propose_underlay(
        &mut self,
        lock_path: &Path,
        edit_path: &Path,
        output: &Path,
    ) -> Result<usize> {
        self.destination(output)?;
        let lock = read(lock_path, 65_536)?;
        let old: UnderlayLock = serde_json::from_slice(&lock)?;
        if old.schema != LOCK {
            bail!("unsupported underlay source lock");
        }
        let pair = self.underlay_pair(&old.model.record.id)?;
        if !unchanged(&old.model, &pair.model.lock)
            || !unchanged(&old.lyric, &pair.lyric.lock)
            || old.layer_id != pair.layer_id
        {
            bail!("stale model, lyric, definition or binding lock");
        }
        let edit = read(edit_path, 2 * 1024 * 1024)?;
        let (candidate, count) =
            reel_music::underlay_editor::propose_bytes(&pair.model.raw, &pair.lyric.raw, &edit)?;
        self.write_proposal(
            &pair.model,
            Proposal {
                candidate,
                edit,
                lock,
                count,
                underlay: true,
                dependency: Some(
                    json!({"record":pair.lyric.lock.record,"definition":pair.lyric.lock.definition,
                "subject":pair.lyric.lock.subject,"layer_id":pair.layer_id}),
                ),
            },
            output,
        )
    }
}
