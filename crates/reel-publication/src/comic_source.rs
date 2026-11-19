//! Portable comic source and receipt contracts, independent of rendering.
use crate::ArtifactSet;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Object {
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub input_bytes: u64,
    pub output_bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub editable_packet: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    pub publication_id: String,
    pub edition_id: String,
    pub language: String,
    pub font_family: String,
    pub font: Pin,
    pub font_license: Pin,
    pub pages: Vec<Pin>,
    pub limits: Limits,
}
fn is_false(value: &bool) -> bool {
    !value
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    pub id: String,
    pub rect: [u32; 4],
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayerRole {
    Background,
    Character,
    Prop,
    Effect,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub id: String,
    pub panel_id: String,
    pub role: LayerRole,
    pub source: Pin,
    pub rect: [u32; 4],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Utterance {
    pub id: String,
    pub panel_id: String,
    pub speaker: String,
    pub lines: Vec<String>,
    pub rect: [u32; 4],
    pub font_size: u32,
    pub tail: Option<[[u32; 2]; 3]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageSource {
    pub schema: String,
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub panels: Vec<Panel>,
    pub layers: Vec<Layer>,
    pub utterances: Vec<Utterance>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    pub publication_id: String,
    pub edition_id: String,
    pub language: String,
    pub request: Object,
    pub page_order: Vec<String>,
    pub outputs: ArtifactSet,
    pub source_files: BTreeMap<String, Object>,
    pub human_approval: bool,
    pub retained_owner_acknowledged: bool,
    pub print_ready: bool,
}
