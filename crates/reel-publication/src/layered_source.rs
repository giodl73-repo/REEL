//! Portable ordered-layer source and request types; no rendering or file I/O.
use crate::comic_source::Pin;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
}
impl Asset {
    pub fn pin(&self) -> Pin {
        Pin {
            path: self.file.clone(),
            sha256: self.sha256.clone(),
            bytes: self.bytes,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub id: String,
    pub role: String,
    pub asset: Asset,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default = "one")]
    pub opacity: f64,
    #[serde(default = "yes")]
    pub visible: bool,
}
fn one() -> f64 {
    1.0
}
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub schema: String,
    pub canvas: [u32; 2],
    pub title: String,
    pub layers: Vec<Layer>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub page_id: String,
    pub language: String,
    pub source: Pin,
    /// Authored provenance is preserved as authored, including older script pins.
    pub provenance: Vec<Pin>,
    pub input_bytes: u64,
    pub output_bytes: u64,
}
