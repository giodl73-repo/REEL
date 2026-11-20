//! Ordered BBJ SVG layers, without converting their geometry or rewriting text.
//! Local source composition is separate from native admission and retention.
use crate::comic::{object, read, write_new};
use crate::comic_source::{Object, Pin};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
}
impl Asset {
    fn pin(&self) -> Pin {
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
fn portable(path: &str) -> bool {
    crate::comic::portable(path) && path.split('/').count() <= 16
}
fn validate(source: &Source) -> Result<()> {
    ensure!(
        source.schema == "bbj.layered-page-source.v1",
        "layered source schema"
    );
    ensure!(
        (1..=4096).contains(&source.canvas[0]) && (1..=6144).contains(&source.canvas[1]),
        "layered canvas bounds"
    );
    ensure!(
        !source.layers.is_empty() && source.layers.len() <= 256 && source.title.len() <= 4096,
        "layered count/title bounds"
    );
    let mut ids = BTreeSet::new();
    for layer in &source.layers {
        ensure!(
            crate::valid_id(&layer.id) && crate::valid_id(&layer.role) && ids.insert(&layer.id),
            "layer identity"
        );
        ensure!(
            portable(&layer.asset.file) && layer.asset.file.ends_with(".svg"),
            "SVG layer required; raster layers need an explicit adapter"
        );
        ensure!(
            layer.x.is_finite()
                && layer.y.is_finite()
                && layer.opacity.is_finite()
                && (0.0..=1.0).contains(&layer.opacity),
            "layer placement"
        );
    }
    Ok(())
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn passive_svg(raw: &[u8]) -> Result<&str> {
    let text = std::str::from_utf8(raw)?;
    let doc = roxmltree::Document::parse(text)?;
    ensure!(
        doc.root_element().has_tag_name("svg") && doc.descendants().count() <= 100_000,
        "SVG structure"
    );
    for node in doc.descendants().filter(|n| n.is_element()) {
        ensure!(
            [
                "svg",
                "g",
                "defs",
                "clipPath",
                "mask",
                "rect",
                "circle",
                "ellipse",
                "path",
                "line",
                "polyline",
                "polygon",
                "text",
                "tspan",
                "title",
                "desc",
                "image",
                "use",
                "pattern",
                "linearGradient",
                "radialGradient",
                "stop",
                "filter",
                "feGaussianBlur",
                "feColorMatrix",
                "feOffset",
                "feMerge",
                "feMergeNode",
                "symbol",
                "marker"
            ]
            .contains(&node.tag_name().name())
                && node
                    .tag_name()
                    .namespace()
                    .is_none_or(|ns| ns == "http://www.w3.org/2000/svg"),
            "unsupported SVG element or namespace"
        );
        for attr in node.attributes() {
            ensure!(
                !attr.name().to_ascii_lowercase().starts_with("on")
                    && attr.name() != "base"
                    && attr.name() != "style",
                "active SVG attribute"
            );
            if attr.name() == "href" {
                ensure!(
                    attr.value().starts_with('#')
                        || attr.value().starts_with("data:image/png;base64,"),
                    "external SVG reference"
                );
            }
            ensure!(
                !attr.value().contains("://") && !attr.value().contains("@import"),
                "external SVG resource"
            );
            if attr.value().contains("url(") {
                ensure!(
                    attr.value().starts_with("url(#") && attr.value().ends_with(')'),
                    "external SVG URL"
                );
            }
        }
    }
    Ok(text)
}

/// Capture every layer, including invisible layers, for future editable restore.
/// The exact original source is retained; only visible layers enter page.svg.
pub fn compose_with(
    request: &Request,
    mut input: impl FnMut(&Pin) -> Result<Vec<u8>>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    ensure!(
        request.schema == "reel.layered-comic-request.v1"
            && crate::valid_id(&request.page_id)
            && crate::valid_id(&request.language),
        "layered request identity"
    );
    ensure!(
        (1..=256 * 1024 * 1024).contains(&request.input_bytes)
            && (1..=512 * 1024 * 1024).contains(&request.output_bytes)
            && request.provenance.len() <= 32,
        "layered request limits"
    );
    let mut sources = BTreeMap::<String, Vec<u8>>::new();
    let mut used = 0u64;
    let mut get = |pin: &Pin| -> Result<Vec<u8>> {
        ensure!(
            portable(&pin.path)
                && pin.path != "request.json"
                && pin.bytes > 0
                && pin.bytes <= request.input_bytes,
            "layer source pin"
        );
        if let Some(raw) = sources.get(&pin.path) {
            ensure!(
                object(raw)
                    == Object {
                        sha256: pin.sha256.clone(),
                        bytes: pin.bytes
                    },
                "conflicting source pin"
            );
            return Ok(raw.clone());
        }
        ensure!(
            sources.len() < 512 && !sources.keys().any(|p| p.eq_ignore_ascii_case(&pin.path)),
            "source path collision/count"
        );
        used = used.checked_add(pin.bytes).context("source overflow")?;
        ensure!(used <= request.input_bytes, "layered input budget");
        let raw = input(pin)?;
        ensure!(
            object(&raw)
                == Object {
                    sha256: pin.sha256.clone(),
                    bytes: pin.bytes
                },
            "source bytes differ"
        );
        sources.insert(pin.path.clone(), raw.clone());
        Ok(raw)
    };
    let raw = get(&request.source)?;
    ensure!(
        raw.len() <= crate::MAX_CONTROL_BYTES,
        "source control bound"
    );
    let source: Source = serde_json::from_slice(&raw)?;
    validate(&source)?;
    let mut body = String::new();
    for layer in &source.layers {
        let bytes = get(&layer.asset.pin())?;
        let svg = passive_svg(&bytes)?;
        if !layer.visible {
            continue;
        }
        let start = svg.find("<svg").context("SVG root")?;
        let content = start + svg[start..].find('>').context("SVG root end")? + 1;
        let end = svg.rfind("</svg>").context("SVG closing root")?;
        ensure!(content <= end, "SVG root bounds");
        body += &format!(
            "<g id=\"{}\" data-role=\"{}\" opacity=\"{}\" transform=\"translate({} {})\">{}</g>",
            esc(&layer.id),
            esc(&layer.role),
            layer.opacity,
            layer.x,
            layer.y,
            &svg[content..end]
        );
        ensure!(
            body.len() as u64 <= request.output_bytes,
            "layered SVG budget"
        );
    }
    for pin in &request.provenance {
        get(pin)?;
    }
    let [w, h] = source.canvas;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><title>{}</title>{body}</svg>",
        esc(&source.title)
    );
    let pins: BTreeMap<_, _> = sources
        .iter()
        .map(|(p, b)| (p.clone(), object(b)))
        .collect();
    let receipt = serde_json::to_vec_pretty(&serde_json::json!({
        "schema":"reel.layered-comic-receipt.v1", "page_id":request.page_id, "language":request.language,
        "request":object(&serde_json::to_vec(request)?), "source_files":pins, "page_svg":object(svg.as_bytes()),
        "layer_order":source.layers.iter().map(|l| &l.id).collect::<Vec<_>>(),
        "retained_owner_acknowledged":false,"human_approval":false,"rasterized":false,
        "source_scope":"selected-layer-stack-and-listed-provenance",
        "full_original_generation_closure_captured":false,"font_license_verified":false
    }))?;
    let mut files: BTreeMap<_, _> = sources
        .into_iter()
        .map(|(p, b)| (format!("sources/{p}"), b))
        .collect();
    files.insert("request.json".into(), serde_json::to_vec_pretty(request)?);
    files.insert("page.svg".into(), svg.into_bytes());
    files.insert("receipt.json".into(), receipt);
    ensure!(
        files.values().map(|b| b.len() as u64).sum::<u64>() <= request.output_bytes,
        "layered output budget"
    );
    Ok(files)
}
pub fn compose(request_path: &Path, bindings_path: &Path, output: &Path) -> Result<()> {
    let request: Request =
        serde_json::from_slice(&read(request_path, crate::MAX_CONTROL_BYTES as u64)?)?;
    let bindings: BTreeMap<String, PathBuf> =
        serde_json::from_slice(&read(bindings_path, crate::MAX_CONTROL_BYTES as u64)?)?;
    let mut opened = BTreeSet::new();
    let files = compose_with(&request, |pin| {
        let path = bindings.get(&pin.path).context("missing source binding")?;
        ensure!(path.is_absolute(), "explicit absolute source binding");
        opened.insert(pin.path.clone());
        read(path, pin.bytes)
    })?;
    ensure!(
        opened == bindings.keys().cloned().collect(),
        "unused source bindings"
    );
    write_new(&std::path::absolute(output)?, &files)
}

/// Prepare one ordered request per selected sheet from an exact BBJ book control.
/// Reads named JSON controls only; actual layer bytes are opened by compose.
pub fn prepare_bbj(
    selection: &Path,
    root: &Path,
    publication: &str,
    language: &str,
    output: &Path,
) -> Result<()> {
    ensure!(
        crate::valid_id(publication) && crate::valid_id(language),
        "publication/language identity"
    );
    let root = root.canonicalize()?;
    let selected = read(selection, crate::MAX_CONTROL_BYTES as u64)?;
    let book: serde_json::Value = serde_json::from_slice(&selected)?;
    ensure!(
        book["schema"] == "refraction.book-preview.v1",
        "BBJ complete-book selection schema"
    );
    let sheets = book["sheets"].as_array().context("selected book sheets")?;
    ensure!(!sheets.is_empty() && sheets.len() <= 128, "BBJ sheet count");
    let story_pages = book["story_pages"]
        .as_u64()
        .context("declared story page count")?;
    let matter_pages = book["book_matter_sheets"]
        .as_u64()
        .context("declared book matter count")?;
    ensure!(
        story_pages.checked_add(matter_pages) == Some(sheets.len() as u64),
        "complete selected book count"
    );
    let story_numbers: Vec<_> = sheets
        .iter()
        .filter(|s| s["kind"] == "story")
        .map(|s| s["story_page"].as_u64())
        .collect();
    ensure!(
        story_numbers == (1..=story_pages).map(Some).collect::<Vec<_>>(),
        "complete story reading order"
    );
    let mut files = BTreeMap::new();
    let mut pages = Vec::new();
    let mut source_paths = BTreeSet::new();
    for (index, sheet) in sheets.iter().enumerate() {
        ensure!(
            sheet["sheet"].as_u64() == Some(index as u64 + 1),
            "BBJ sheet order"
        );
        let asset: Asset = serde_json::from_value(sheet["source"].clone())?;
        ensure!(
            portable(&asset.file) && source_paths.insert(asset.file.clone()),
            "BBJ source path identity"
        );
        let raw = read(&root.join(&asset.file), crate::MAX_CONTROL_BYTES as u64)?;
        ensure!(
            object(&raw)
                == Object {
                    sha256: asset.sha256.clone(),
                    bytes: asset.bytes
                },
            "selected source differs"
        );
        let source: Source = serde_json::from_slice(&raw)?;
        validate(&source)?;
        let page_id = if sheet["kind"] == "story" {
            let number = sheet["story_page"]
                .as_u64()
                .context("stable story page number")?;
            format!("{publication}-story-{number:03}")
        } else {
            format!(
                "{publication}-{}",
                sheet["kind"].as_str().context("book matter kind")?
            )
        };
        ensure!(
            !pages
                .iter()
                .any(|p: &serde_json::Value| p["page_id"] == page_id),
            "duplicate stable page identity"
        );
        let authoring: Asset = serde_json::from_value(sheet["authoring"].clone())?;
        ensure!(portable(&authoring.file), "authoring path");
        let author_raw = read(&root.join(&authoring.file), crate::MAX_CONTROL_BYTES as u64)?;
        ensure!(
            object(&author_raw)
                == Object {
                    sha256: authoring.sha256.clone(),
                    bytes: authoring.bytes
                },
            "selected authoring differs"
        );
        let author: serde_json::Value = serde_json::from_slice(&author_raw)?;
        let mut provenance = vec![authoring.pin()];
        for field in ["script", "font", "font_license"] {
            if let Some(value) = author.get(field).filter(|v| !v.is_null()) {
                let asset: Asset = serde_json::from_value(value.clone())?;
                provenance.push(asset.pin());
            }
        }
        let mut bindings = BTreeMap::new();
        let mut declared = BTreeMap::new();
        for pin in std::iter::once(asset.pin())
            .chain(source.layers.iter().map(|l| l.asset.pin()))
            .chain(provenance.iter().cloned())
        {
            ensure!(portable(&pin.path), "BBJ bound source path");
            if let Some(old) = declared.insert(
                pin.path.clone(),
                Object {
                    sha256: pin.sha256.clone(),
                    bytes: pin.bytes,
                },
            ) {
                ensure!(
                    old == Object {
                        sha256: pin.sha256.clone(),
                        bytes: pin.bytes
                    },
                    "conflicting selected pins"
                );
            }
            bindings.insert(pin.path.clone(), root.join(&pin.path));
        }
        let input_bytes = declared
            .values()
            .try_fold(0u64, |sum, pin| sum.checked_add(pin.bytes))
            .context("BBJ declared input overflow")?;
        ensure!(
            input_bytes > 0 && input_bytes <= 256 * 1024 * 1024,
            "BBJ page source budget"
        );
        let request = Request {
            schema: "reel.layered-comic-request.v1".into(),
            page_id: page_id.clone(),
            language: language.into(),
            source: asset.pin(),
            provenance,
            input_bytes,
            output_bytes: 512 * 1024 * 1024,
        };
        let directory = format!("sheet-{:03}", index + 1);
        files.insert(
            format!("{directory}/request.json"),
            serde_json::to_vec_pretty(&request)?,
        );
        files.insert(
            format!("{directory}/bindings.json"),
            serde_json::to_vec_pretty(&bindings)?,
        );
        pages.push(serde_json::json!({"page_id":page_id,"sheet":index+1,"kind":sheet["kind"],"request":format!("{directory}/request.json"),"bindings":format!("{directory}/bindings.json"),"selected_svg":sheet["svg"],"selected_png":sheet["png"],"review_ids":author["page"]["reading_order"]}));
    }
    files.insert("book.json".into(), serde_json::to_vec_pretty(&serde_json::json!({"schema":"reel.bbj-book-import.v1","publication_id":publication,"language":language,"selection":object(&selected),"pages":pages,"native_admitted":false,"payloads_verified":false}))?);
    files.insert("selected-book.json".into(), selected);
    write_new(&std::path::absolute(output)?, &files)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Request, BTreeMap<String, Vec<u8>>) {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="200"><text x="2" y="4">Exact &amp; words</text></svg>"#.to_vec();
        let pin = object(&svg);
        let source = Source {
            schema: "bbj.layered-page-source.v1".into(),
            canvas: [100, 200],
            title: "A & B".into(),
            layers: vec![Layer {
                id: "lettering".into(),
                role: "lettering".into(),
                asset: Asset {
                    file: "layers/words.svg".into(),
                    sha256: pin.sha256,
                    bytes: pin.bytes,
                },
                x: 3.,
                y: 4.,
                opacity: 0.5,
                visible: true,
            }],
        };
        let raw = serde_json::to_vec(&source).unwrap();
        let source_pin = object(&raw);
        let request = Request {
            schema: "reel.layered-comic-request.v1".into(),
            page_id: "book-story-001".into(),
            language: "es".into(),
            source: Pin {
                path: "page/source.json".into(),
                sha256: source_pin.sha256,
                bytes: source_pin.bytes,
            },
            provenance: vec![],
            input_bytes: 1024 * 1024,
            output_bytes: 2 * 1024 * 1024,
        };
        (
            request,
            BTreeMap::from([
                ("page/source.json".into(), raw),
                ("layers/words.svg".into(), svg),
            ]),
        )
    }
    #[test]
    fn preserves_bbj_order_placement_words_and_all_editable_source_bytes() {
        let (request, inputs) = fixture();
        let files = compose_with(&request, |pin| Ok(inputs[&pin.path].clone())).unwrap();
        assert_eq!(
            std::str::from_utf8(&files["page.svg"]).unwrap(),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"200\" viewBox=\"0 0 100 200\"><title>A &amp; B</title><g id=\"lettering\" data-role=\"lettering\" opacity=\"0.5\" transform=\"translate(3 4)\"><text x=\"2\" y=\"4\">Exact &amp; words</text></g></svg>"
        );
        assert_eq!(
            files["sources/layers/words.svg"],
            inputs["layers/words.svg"]
        );
        assert_eq!(
            files["sources/page/source.json"],
            inputs["page/source.json"]
        );
        let mut changed = inputs.clone();
        changed.get_mut("layers/words.svg").unwrap().push(b' ');
        assert!(compose_with(&request, |pin| Ok(changed[&pin.path].clone())).is_err());
        let mut small = request;
        small.input_bytes = 1;
        assert!(compose_with(&small, |pin| Ok(inputs[&pin.path].clone())).is_err());
    }
    #[test]
    fn refuses_silent_unsupported_geometry_and_active_or_external_svg() {
        assert!(serde_json::from_value::<Layer>(serde_json::json!({"id":"a","role":"art","asset":{"file":"a.svg","sha256":"a","bytes":1},"rotation":30})).is_err());
        for svg in [
            "<svg><script>bad()</script></svg>",
            "<svg><image href='file:///private.png'/></svg>",
            "<svg><g onclick='bad()'/></svg>",
            "<svg><rect fill='url(https://example.com/a)'/></svg>",
        ] {
            assert!(passive_svg(svg.as_bytes()).is_err());
        }
    }
}
