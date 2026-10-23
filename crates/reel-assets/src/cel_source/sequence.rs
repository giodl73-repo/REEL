//! Cue-local authoring clocks over native scene/alignment contracts. No media I/O.
use super::{
    inspect,
    project::{MAX_CUES, alignment_role},
};
use crate::Result;
use reel_assembly::{self as assembly, scene_authoring as native};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const JS_SAFE: u64 = 9_007_199_254_740_991;
pub struct CueClock {
    pub cue_id: String,
    pub alignment_role: String,
    pub alignment_binding: String,
    pub alignment: native::NativeAlignment,
    samples_per_frame: Option<u64>,
}
pub struct Cell {
    pub semantic_id: String,
    pub cue_id: String,
    pub event_id: String,
    pub picture_binding: String,
    pub picture_slot_id: String,
    pub trigger_id: String,
}
pub struct EditedCell {
    pub semantic_id: String,
    pub picture_binding: String,
    pub cue_id: String,
    pub start_sample: u64,
    pub end_sample: u64,
    pub sample_rate: u32,
}
pub struct EditPlan {
    pub cells: Vec<EditedCell>,
    pub markers: BTreeMap<String, BTreeMap<String, u64>>,
}
pub struct Sequence {
    pub language: String,
    pub cues: Vec<CueClock>,
    pub cells: BTreeMap<String, Cell>,
    multi: bool,
    rows: Value,
    profile_sample_rate: Option<u64>,
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing native {key}").into())
}
fn integer(value: &Value, key: &str) -> Result<u64> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("Missing integer {key}").into())
}
fn selected(state: &str) -> bool {
    matches!(
        state,
        "selected-private-production" | "principal-approved" | "release-cleared"
    )
}
impl Sequence {
    pub fn new(docs: &BTreeMap<String, Value>, multi: bool) -> Result<Self> {
        if [
            "scene.json",
            "scene-bindings.json",
            "selected-graph.json",
            "selected-pointer.json",
            "profile.json",
        ]
        .iter()
        .any(|key| !docs.contains_key(*key))
        {
            return Err("Missing required native editor control".into());
        }
        let report = inspect(
            &serde_json::to_vec(&docs["scene.json"])?,
            &serde_json::to_vec(&docs["scene-bindings.json"])?,
        )?;
        if report.imported_evidence || report.languages.len() != 1 || report.cels.len() > 1000 {
            return Err("Editor needs one bounded ready native language".into());
        }
        let scene: native::Scene = serde_json::from_value(docs["scene.json"].clone())?;
        let materialized = native::materialize_scene(&scene)?;
        let language = report.languages[0].clone();
        let lane = &materialized.languages[&language];
        if lane.cues.is_empty() || lane.cues.len() > MAX_CUES || (!multi && lane.cues.len() != 1) {
            return Err("Unsupported native cue count".into());
        }
        let bindings: native::ScopedBindings =
            serde_json::from_value(docs["scene-bindings.json"].clone())?;
        let graph: assembly::Graph = serde_json::from_value(docs["selected-graph.json"].clone())?;
        let pointer: assembly::SelectedPointer =
            serde_json::from_value(docs["selected-pointer.json"].clone())?;
        assembly::validate_selected_graph(&pointer, &graph)?;
        if graph.nodes.len() != 1 || graph.nodes[0].id != scene.scene_id {
            return Err("Editor requires one native scene node".into());
        }
        let mut clocks = Vec::new();
        let mut alignments = BTreeMap::new();
        let mut alignment_bindings = BTreeSet::new();
        let fps = integer(&docs["profile.json"], "frame_rate_numerator")?;
        let den = integer(&docs["profile.json"], "frame_rate_denominator")?;
        if fps == 0 || den == 0 {
            return Err("Invalid native output clock".into());
        }
        for cue in &lane.cues {
            let role = if multi {
                alignment_role(&cue.cue_id)
            } else {
                "alignment.json".into()
            };
            let alignment: native::NativeAlignment = serde_json::from_value(
                docs.get(&role)
                    .ok_or("Missing declared cue alignment")?
                    .clone(),
            )?;
            let take = bindings
                .assets
                .get(
                    cue.take_binding
                        .as_deref()
                        .ok_or("Missing native take binding")?,
                )
                .ok_or("Unbound native take")?;
            let key = cue
                .phrase_alignment_binding
                .as_ref()
                .ok_or("Missing native alignment binding")?;
            let bound = bindings.assets.get(key).ok_or("Unbound native alignment")?;
            if !alignment_bindings.insert(key.clone())
                || !selected(&bound.selection_state)
                || !selected(&take.selection_state)
            {
                return Err("Ambiguous or unselected native cue binding".into());
            }
            native::validate_native_alignment(&alignment, &language, &cue.cue_id, &take.sha256)?;
            if alignment.cue_end_sample > JS_SAFE {
                return Err("Cue clock exceeds exact browser integer budget".into());
            }
            let samples_per_frame = if multi {
                None
            } else {
                let numerator = (alignment.sample_rate as u64)
                    .checked_mul(den)
                    .ok_or("Output clock overflow")?;
                if numerator % fps != 0 || numerator / fps == 0 {
                    return Err("Unsupported nonintegral sample/frame clock".into());
                }
                Some(numerator / fps)
            };
            alignments.insert(cue.cue_id.clone(), alignment.clone());
            clocks.push(CueClock {
                cue_id: cue.cue_id.clone(),
                alignment_role: role,
                alignment_binding: key.clone(),
                alignment,
                samples_per_frame,
            });
        }
        if multi {
            let declared: BTreeSet<_> = docs
                .keys()
                .filter(|key| key.starts_with("alignment:"))
                .cloned()
                .collect();
            let expected = clocks
                .iter()
                .map(|cue| cue.alignment_role.clone())
                .collect();
            if declared != expected {
                return Err("Native cue path set differs from scene cue set".into());
            }
        }
        let spans = native::compile_native_event_spans(&language, lane, &alignments)?;
        let mut cells = BTreeMap::new();
        let mut by_event = BTreeMap::new();
        for cel in report.cels {
            let id = cel
                .shared_semantic_id
                .ok_or("Missing shared native event identity")?;
            let slot = graph
                .slots
                .iter()
                .find(|slot| slot.slot_id == cel.picture_slot_id)
                .ok_or("Missing selected picture slot")?;
            let revision = slot
                .revisions
                .iter()
                .find(|revision| Some(&revision.revision_id) == slot.selected_revision_id.as_ref())
                .ok_or("Missing selected picture revision")?;
            let event = graph
                .events
                .iter()
                .find(|event| event.event_id == cel.event_id)
                .ok_or("Missing native picture event")?;
            let cue = lane
                .cues
                .iter()
                .find(|cue| cue.cue_id == cel.cue_id)
                .ok_or("Unknown native cue")?;
            let take = &bindings.assets[cue.take_binding.as_ref().ok_or("Missing native take")?];
            if slot.lane != assembly::Lane::Picture
                || slot.disposition != assembly::Disposition::Selected
                || revision.asset.sha256 != cel.asset.sha256
                || event.picture.sha256 != cel.asset.sha256
                || event.narration.sha256 != take.sha256
                || !selected(&cel.asset.selection_state)
            {
                return Err("Native effective picture/take differs from selected graph".into());
            }
            by_event.insert(cel.event_id.clone(), id.clone());
            cells.insert(
                id.clone(),
                Cell {
                    semantic_id: id,
                    cue_id: cel.cue_id,
                    event_id: cel.event_id,
                    picture_binding: cel.picture_binding,
                    picture_slot_id: cel.picture_slot_id,
                    trigger_id: cel.semantic_trigger_id,
                },
            );
        }
        let graph_ids: BTreeSet<_> = graph
            .events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect();
        let expected: BTreeSet<_> = cells.values().map(|cell| cell.event_id.as_str()).collect();
        let picture_slots: BTreeSet<_> = graph
            .slots
            .iter()
            .filter(|slot| slot.lane == assembly::Lane::Picture)
            .map(|slot| slot.slot_id.as_str())
            .collect();
        if graph_ids != expected
            || picture_slots
                != cells
                    .values()
                    .map(|cell| cell.picture_slot_id.as_str())
                    .collect()
        {
            return Err("Unsupported unrelated graph scope".into());
        }
        let mut rows = Vec::new();
        for span in spans {
            let cell = &cells[&by_event[&span.event_id]];
            let clock = clocks
                .iter()
                .find(|clock| clock.cue_id == span.cue_id)
                .ok_or("Missing native clock")?;
            let quantity = span.end_sample - span.start_sample;
            let mut row =
                json!({"semantic_id":cell.semantic_id,"picture_binding":cell.picture_binding});
            if multi {
                row["cue_id"] = json!(cell.cue_id);
                row["samples"] = json!(quantity);
                row["sample_rate"] = json!(clock.alignment.sample_rate);
            } else {
                let spf = clock.samples_per_frame.ok_or("Missing frame clock")?;
                if span.start_sample % spf != 0 || span.end_sample % spf != 0 {
                    return Err("Marker/end not frame aligned".into());
                }
                row["frames"] = json!(quantity / spf);
            }
            rows.push(row);
        }
        Ok(Self {
            language,
            cues: clocks,
            cells,
            multi,
            rows: json!(rows),
            profile_sample_rate: docs["profile.json"]["sample_rate"].as_u64(),
        })
    }
    pub fn rows(&self) -> Value {
        self.rows.clone()
    }
    /// Native clocks plus a rate-only delivery compatibility observation.
    /// This is not a project compilation or media/readiness certificate.
    pub fn session_clock(&self) -> Value {
        let mut clock = if self.multi {
            let mut seconds = 0.0;
            let mut cues = Vec::new();
            for cue in &self.cues {
                let duration =
                    cue.alignment.cue_end_sample as f64 / cue.alignment.sample_rate as f64;
                cues.push(json!({"cue_id":cue.cue_id,"samples":cue.alignment.cue_end_sample,"sample_rate":cue.alignment.sample_rate,"start_seconds":seconds}));
                seconds += duration;
            }
            json!({"total_seconds":seconds,"cues":cues})
        } else {
            let cue = &self.cues[0];
            json!({"sample_rate":cue.alignment.sample_rate,"end_sample":cue.alignment.cue_end_sample})
        };
        let rates: BTreeSet<_> = self
            .cues
            .iter()
            .map(|cue| cue.alignment.sample_rate)
            .collect();
        let state = if rates.len() > 1 {
            "mixed-native-rates"
        } else if self.profile_sample_rate.is_none() {
            "not-declared"
        } else if self.profile_sample_rate == rates.first().map(|rate| u64::from(*rate)) {
            "matches-profile"
        } else {
            "profile-rate-mismatch"
        };
        clock["delivery_clock"] = json!({"state":state,"native_sample_rates":rates,"profile_sample_rate":self.profile_sample_rate});
        clock
    }

    pub fn plan(&self, edited: &[Value]) -> Result<EditPlan> {
        if edited.is_empty() || edited.len() > self.cells.len() {
            return Err("Unsupported event insertion or empty scene".into());
        }
        let pictures: BTreeSet<_> = self
            .cells
            .values()
            .map(|cell| cell.picture_binding.as_str())
            .collect();
        let mut markers: BTreeMap<_, _> = self
            .cues
            .iter()
            .map(|cue| (cue.cue_id.clone(), cue.alignment.semantic_markers.clone()))
            .collect();
        for cell in self.cells.values() {
            markers
                .get_mut(&cell.cue_id)
                .ok_or("Unknown cue")?
                .remove(&cell.trigger_id);
        }
        let mut positions = vec![0u64; self.cues.len()];
        let mut seen = BTreeSet::new();
        let mut previous = 0;
        let mut output = Vec::new();
        for row in edited {
            let id = text(row, "semantic_id")?;
            let cell = self.cells.get(id).ok_or("Unknown native event")?;
            let binding = text(row, "picture_binding")?;
            if !seen.insert(id) || !pictures.contains(binding) {
                return Err("Invalid stable event or selected picture".into());
            }
            let index = self
                .cues
                .iter()
                .position(|cue| cue.cue_id == cell.cue_id)
                .ok_or("Unknown event cue")?;
            let cue = &self.cues[index];
            if index < previous {
                return Err("Native cue order cannot change".into());
            }
            previous = index;
            let quantity = if self.multi {
                if text(row, "cue_id")? != cell.cue_id
                    || integer(row, "sample_rate")? != cue.alignment.sample_rate as u64
                    || row.get("frames").is_some()
                {
                    return Err("Edited event crossed native cue or clock scope".into());
                }
                integer(row, "samples")?
            } else {
                if row.get("samples").is_some() {
                    return Err("Frame edits cannot change sample-clock schema".into());
                }
                integer(row, "frames")?
                    .checked_mul(cue.samples_per_frame.ok_or("Missing frame clock")?)
                    .ok_or("Hold overflow")?
            };
            if quantity == 0 {
                return Err("Native hold must be positive".into());
            }
            let start = positions[index];
            let end = start.checked_add(quantity).ok_or("Hold overflow")?;
            if end > cue.alignment.cue_end_sample {
                return Err("Edited holds exceed native cue duration".into());
            }
            positions[index] = end;
            markers
                .get_mut(&cell.cue_id)
                .ok_or("Unknown cue")?
                .insert(cell.trigger_id.clone(), start);
            output.push(EditedCell {
                semantic_id: id.into(),
                picture_binding: binding.into(),
                cue_id: cell.cue_id.clone(),
                start_sample: start,
                end_sample: end,
                sample_rate: cue.alignment.sample_rate,
            });
        }
        if self
            .cues
            .iter()
            .enumerate()
            .any(|(index, cue)| positions[index] != cue.alignment.cue_end_sample)
        {
            return Err("Each cue must retain its complete unchanged recording".into());
        }
        Ok(EditPlan {
            cells: output,
            markers,
        })
    }
}
