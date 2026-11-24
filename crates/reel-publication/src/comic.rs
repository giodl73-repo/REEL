//! Bounded private comic producer. Shared REEL print adapters render pixels/PDF;
//! this module owns page assembly and source closure, never asset-store lifecycle.
use crate::{Artifact, ArtifactSet, valid_id};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use reel_print::vector::{RasterLayer, RasterPage, Renderer};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Component, Path},
};

pub use crate::comic_source::*;

pub(crate) struct Packet {
    pub(crate) receipt: Receipt,
    pub(crate) files: BTreeMap<String, Vec<u8>>,
}
struct Inputs<'a> {
    read: &'a mut dyn FnMut(&Pin) -> Result<Vec<u8>>,
    max: u64,
    used: u64,
    expanded_bytes: u64,
    expanded_limit: u64,
    expanded_pixels: u64,
    files: BTreeMap<String, Vec<u8>>,
}

pub fn object(raw: &[u8]) -> Object {
    Object {
        sha256: Sha256::digest(raw)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        bytes: raw.len() as u64,
    }
}
fn ordinary(path: &Path) -> Result<fs::Metadata> {
    let m = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let link = {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let link = m.file_type().is_symlink();
    ensure!(!link, "ordinary paths required");
    Ok(m)
}
pub(crate) fn read(path: &Path, max: u64) -> Result<Vec<u8>> {
    for p in path.parent().context("input parent required")?.ancestors() {
        ensure!(ordinary(p)?.is_dir(), "ordinary ancestors required");
    }
    let m = ordinary(path)?;
    ensure!(m.is_file() && m.len() <= max, "input byte bound exceeded");
    let mut raw = Vec::new();
    fs::File::open(path)?.take(max + 1).read_to_end(&mut raw)?;
    ensure!(raw.len() as u64 <= max, "input grew beyond bound");
    Ok(raw)
}
pub(crate) fn portable(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && path.split('/').all(valid_id)
}
impl Inputs<'_> {
    fn get(&mut self, pin: &Pin) -> Result<Vec<u8>> {
        ensure!(
            portable(&pin.path)
                && pin.path != "request.json"
                && (1..=16 * 1024 * 1024).contains(&pin.bytes),
            "source pin/path refused"
        );
        if let Some(raw) = self.files.get(&pin.path) {
            let actual = object(raw);
            ensure!(
                actual.sha256 == pin.sha256 && actual.bytes == pin.bytes,
                "repeated source pin conflicts"
            );
            return Ok(raw.clone());
        }
        ensure!(
            self.files.len() < 512 && !self.files.keys().any(|p| p.eq_ignore_ascii_case(&pin.path)),
            "source path collision/count refused"
        );
        ensure!(
            self.used
                .checked_add(pin.bytes)
                .is_some_and(|n| n <= self.max),
            "source closure byte bound exceeded"
        );
        let raw = (self.read)(pin)?;
        let actual = object(&raw);
        ensure!(
            actual.sha256 == pin.sha256 && actual.bytes == pin.bytes,
            "source pin differs"
        );
        self.used += pin.bytes;
        self.files.insert(pin.path.clone(), raw.clone());
        Ok(raw)
    }
}
fn inside(rect: [u32; 4], outer: [u32; 4]) -> bool {
    rect[2] > 0
        && rect[3] > 0
        && rect[0] >= outer[0]
        && rect[1] >= outer[1]
        && rect[0]
            .checked_add(rect[2])
            .zip(outer[0].checked_add(outer[2]))
            .is_some_and(|(a, b)| a <= b)
        && rect[1]
            .checked_add(rect[3])
            .zip(outer[1].checked_add(outer[3]))
            .is_some_and(|(a, b)| a <= b)
}
fn label(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(|c| c.is_control())
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn svg(width: u32, height: u32, body: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">{body}</svg>"
    )
}
fn rect([x, y, w, h]: [u32; 4], extra: &str) -> String {
    format!("<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" {extra}/>")
}
fn png(raw: &[u8]) -> Result<u64> {
    ensure!(
        raw.len() >= 24 && raw.starts_with(b"\x89PNG\r\n\x1a\n") && &raw[12..16] == b"IHDR",
        "PNG layer required"
    );
    let w = u32::from_be_bytes(raw[16..20].try_into()?);
    let h = u32::from_be_bytes(raw[20..24].try_into()?);
    ensure!(
        (1..=2048).contains(&w) && (1..=2048).contains(&h),
        "raster layer dimensions exceed bound"
    );
    ensure!(
        resvg::tiny_skia::Pixmap::decode_png(raw).is_ok(),
        "PNG layer decode failed"
    );
    Ok(w as u64 * h as u64)
}
fn page(
    source: &PageSource,
    inputs: &mut Inputs<'_>,
    renderer: &Renderer,
    family: &str,
) -> Result<(String, String, String, RasterPage)> {
    ensure!(
        source.schema == "reel.comic-page.v1"
            && valid_id(&source.id)
            && source.id.len() <= 64
            && (72..=2048).contains(&source.width)
            && (72..=2048).contains(&source.height),
        "comic page contract refused"
    );
    ensure!(
        (1..=32).contains(&source.panels.len())
            && source.layers.len() <= 128
            && source.utterances.len() <= 128,
        "page item bound exceeded"
    );
    let canvas = [0, 0, source.width, source.height];
    let mut panels = BTreeMap::new();
    for p in &source.panels {
        ensure!(
            valid_id(&p.id) && inside(p.rect, canvas) && panels.insert(&p.id, p.rect).is_none(),
            "panel identity/geometry refused"
        );
    }
    let mut art = rect(canvas, "fill=\"#fffdf6\"");
    let mut raster_layers = Vec::new();
    let mut frames = String::new();
    let mut ids = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for layer in &source.layers {
        let panel = panels.get(&layer.panel_id).context("layer panel missing")?;
        ensure!(
            valid_id(&layer.id) && ids.insert(&layer.id) && inside(layer.rect, *panel),
            "layer identity/placement refused"
        );
        inputs.expanded_bytes = inputs
            .expanded_bytes
            .checked_add(layer.source.bytes)
            .context("expanded layer byte overflow")?;
        ensure!(
            inputs.expanded_bytes <= inputs.expanded_limit,
            "expanded layer byte bound exceeded"
        );
        let raw = inputs.get(&layer.source)?;
        inputs.expanded_pixels += png(&raw)?;
        ensure!(
            inputs.expanded_pixels <= 64 * 1024 * 1024,
            "expanded layer pixel bound exceeded"
        );
        covered.insert(&layer.panel_id);
        let [x, y, w, h] = layer.rect;
        let role = match layer.role {
            LayerRole::Background => "background",
            LayerRole::Character => "character",
            LayerRole::Prop => "prop",
            LayerRole::Effect => "effect",
        };
        art += &format!(
            "<g id=\"art-{}\" data-role=\"{role}\"><image x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{}\"/></g>",
            layer.id,
            STANDARD.encode(&raw)
        );
        raster_layers.push(RasterLayer {
            png: raw,
            rect: layer.rect,
        });
    }
    ensure!(
        covered.len() == panels.len(),
        "each declared panel needs an art layer"
    );
    for p in &source.panels {
        frames += &format!(
            "<g id=\"panel-{}\">{}</g>",
            p.id,
            rect(
                p.rect,
                "fill=\"none\" stroke=\"#233449\" stroke-width=\"2\""
            )
        );
    }
    art += &frames;
    let mut lettering = String::new();
    let mut utterances = BTreeSet::new();
    for u in &source.utterances {
        let panel = panels.get(&u.panel_id).context("utterance panel missing")?;
        ensure!(
            valid_id(&u.id)
                && utterances.insert(&u.id)
                && label(&u.speaker, 128)
                && inside(u.rect, *panel)
                && (8..=96).contains(&u.font_size)
                && (1..=12).contains(&u.lines.len())
                && u.rect[2] > 20
                && u.rect[3] > 20,
            "utterance contract refused"
        );
        let size = u.font_size as f32;
        let line_height = size * 1.25;
        ensure!(
            line_height * u.lines.len() as f32 <= (u.rect[3] - 16) as f32,
            "lettering height exceeds balloon"
        );
        for line in &u.lines {
            ensure!(
                label(line, 2048) && renderer.text_width(line, size)? <= (u.rect[2] - 16) as f32,
                "lettering width/glyph exceeds balloon"
            );
        }
        lettering += &format!(
            "<g id=\"letter-{}\" aria-label=\"{}: {}\">",
            u.id,
            esc(&u.speaker),
            esc(&u.lines.join(" "))
        );
        if let Some(tail) = u.tail {
            ensure!(
                tail.iter().all(|[x, y]| *x >= panel[0]
                    && *y >= panel[1]
                    && *x <= panel[0] + panel[2]
                    && *y <= panel[1] + panel[3]),
                "balloon tail leaves panel"
            );
            lettering += &format!(
                "<polygon points=\"{},{} {},{} {},{}\" fill=\"white\" stroke=\"#233449\" stroke-width=\"2\"/>",
                tail[0][0], tail[0][1], tail[1][0], tail[1][1], tail[2][0], tail[2][1]
            );
        }
        lettering += &rect(
            u.rect,
            "rx=\"14\" fill=\"white\" stroke=\"#233449\" stroke-width=\"2\"",
        );
        let x = u.rect[0] as f32 + u.rect[2] as f32 / 2.0;
        let first =
            u.rect[1] as f32 + (u.rect[3] as f32 - line_height * u.lines.len() as f32) / 2.0 + size;
        for (index, line) in u.lines.iter().enumerate() {
            lettering += &format!(
                "<text x=\"{x}\" y=\"{}\" text-anchor=\"middle\" font-family=\"{}\" font-size=\"{size}\" fill=\"#1b293b\">{}</text>",
                first + index as f32 * line_height,
                esc(family),
                esc(line)
            );
        }
        lettering += "</g>";
    }
    Ok((
        svg(source.width, source.height, &art),
        svg(source.width, source.height, &lettering),
        svg(source.width, source.height, &format!("{art}{lettering}")),
        RasterPage {
            width: source.width,
            height: source.height,
            layers: raster_layers,
            frames: svg(source.width, source.height, &frames),
            lettering: svg(source.width, source.height, &lettering),
        },
    ))
}
fn prepare(request_path: &Path) -> Result<Packet> {
    let raw = read(request_path, 1024 * 1024)?;
    let root = request_path
        .parent()
        .context("request parent required")?
        .canonicalize()?;
    prepare_with(&raw, &mut |pin| read(&root.join(&pin.path), pin.bytes))
}
fn prepare_with(raw: &[u8], read_input: &mut dyn FnMut(&Pin) -> Result<Vec<u8>>) -> Result<Packet> {
    ensure!(raw.len() <= 1024 * 1024, "request byte bound exceeded");
    let request: Request = serde_json::from_slice(raw)?;
    ensure!(
        request.schema == "reel.comic-build.v1"
            && request.part_id.as_ref().is_none_or(|id| valid_id(id))
            && valid_id(&request.publication_id)
            && valid_id(&request.edition_id)
            && valid_id(&request.language)
            && label(&request.font_family, 128)
            && (1..=32).contains(&request.pages.len()),
        "comic build contract refused"
    );
    ensure!(
        (1..=64 * 1024 * 1024).contains(&request.limits.input_bytes)
            && (1..=128 * 1024 * 1024).contains(&request.limits.output_bytes),
        "comic byte allowance refused"
    );
    let mut inputs = Inputs {
        read: read_input,
        max: request.limits.input_bytes,
        used: raw.len() as u64,
        expanded_bytes: 0,
        expanded_limit: request
            .limits
            .input_bytes
            .min(request.limits.output_bytes / 4),
        expanded_pixels: 0,
        files: BTreeMap::new(),
    };
    ensure!(inputs.used <= inputs.max, "request exceeds input allowance");
    let font = inputs.get(&request.font)?;
    inputs.get(&request.font_license)?;
    let renderer = Renderer::new(&request.font_family, &[font])?;
    let mut raster_pages = Vec::new();
    let mut sources = Vec::new();
    let mut files = BTreeMap::new();
    let mut names = BTreeSet::new();
    for pin in &request.pages {
        ensure!(pin.bytes <= 256 * 1024, "page source bound exceeded");
        let bytes = inputs.get(pin)?;
        let source: PageSource = serde_json::from_slice(&bytes)?;
        ensure!(names.insert(source.id.clone()), "duplicate page identity");
        let (art, letters, composite, raster) =
            page(&source, &mut inputs, &renderer, &request.font_family)?;
        files.insert(format!("{}-clean.svg", source.id), art.as_bytes().to_vec());
        files.insert(format!("{}-lettering.svg", source.id), letters.into_bytes());
        files.insert(
            format!("{}-reading.svg", source.id),
            composite.as_bytes().to_vec(),
        );
        raster_pages.push(raster);
        sources.push(source);
    }
    let reading_book = renderer.render_raster_book(&raster_pages, request.limits.output_bytes)?;
    for (index, source) in sources.iter().enumerate() {
        files.insert(
            format!("{}-clean.png", source.id),
            reading_book.pages[index].clean_png.clone(),
        );
        files.insert(
            format!("{}-reading.png", source.id),
            reading_book.pages[index].reading_png.clone(),
        );
    }
    files.insert("reading.pdf".into(), reading_book.pdf);
    let transcript = serde_json::json!({"schema":"reel.comic-transcript.v1","language":request.language,
        "pages":sources.iter().map(|p|serde_json::json!({"page_id":p.id,"utterances":p.utterances})).collect::<Vec<_>>()});
    files.insert(
        "transcript.json".into(),
        serde_json::to_vec_pretty(&transcript)?,
    );
    inputs.files.insert("request.json".into(), raw.to_vec());
    let source_files = inputs
        .files
        .iter()
        .map(|(name, bytes)| (name.clone(), object(bytes)))
        .collect();
    for (name, bytes) in inputs.files {
        files.insert(format!("sources/{name}"), bytes);
    }
    if request.editable_packet {
        let carrier = crate::packet::encode(&files)?;
        files.insert("editable.packet".into(), carrier);
    }
    let outputs: ArtifactSet = files
        .iter()
        .filter(|(name, _)| !name.starts_with("sources/"))
        .map(|(name, bytes)| {
            let obj = object(bytes);
            let media = if name == "editable.packet" {
                crate::packet::MEDIA_TYPE
            } else if name.ends_with(".svg") {
                "image/svg+xml"
            } else if name.ends_with(".png") {
                "image/png"
            } else if name.ends_with(".pdf") {
                "application/pdf"
            } else {
                "application/json"
            };
            (
                name.clone(),
                Artifact {
                    logical_id: if let Some(part) = &request.part_id {
                        format!(
                            "{}/{}/parts/{part}/{name}",
                            request.publication_id, request.edition_id
                        )
                    } else {
                        format!("{}/{}/{name}", request.publication_id, request.edition_id)
                    },
                    sha256: obj.sha256,
                    bytes: obj.bytes,
                    media_type: media.into(),
                },
            )
        })
        .collect();
    crate::check_artifact_set(&outputs)?;
    let receipt = Receipt {
        schema: "reel.comic-build-receipt.v1".into(),
        part_id: request.part_id,
        publication_id: request.publication_id,
        edition_id: request.edition_id,
        language: request.language,
        request: object(raw),
        page_order: sources.iter().map(|s| s.id.clone()).collect(),
        outputs,
        source_files,
        human_approval: false,
        retained_owner_acknowledged: false,
        print_ready: false,
    };
    files.insert("receipt.json".into(), serde_json::to_vec_pretty(&receipt)?);
    let total = files.values().try_fold(0u64, |n, b| {
        n.checked_add(b.len() as u64)
            .context("comic output overflow")
    })?;
    ensure!(
        total <= request.limits.output_bytes,
        "comic packet byte bound exceeded"
    );
    Ok(Packet { receipt, files })
}
pub(crate) fn write_new(output: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let parent = output.parent().context("output parent required")?;
    for ancestor in parent.ancestors() {
        ensure!(
            ordinary(ancestor)?.is_dir(),
            "ordinary output ancestors required"
        );
    }
    fs::create_dir(output).context("new output directory required")?;
    for (relative, raw) in files {
        ensure!(portable(relative), "output path refused");
        let path = output.join(relative);
        fs::create_dir_all(path.parent().context("output parent absent")?)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(raw)?;
        file.sync_all()?;
    }
    Ok(())
}
pub fn render(request: &Path, output: &Path) -> Result<Receipt> {
    let request = std::path::absolute(request)?;
    let output = std::path::absolute(output)?;
    ensure!(!output.exists(), "existing comic output refused");
    let packet = prepare(&request)?;
    write_new(&output, &packet.files)?;
    Ok(packet.receipt)
}
pub fn check(request: &Path, output: &Path) -> Result<Receipt> {
    let packet = prepare(&std::path::absolute(request)?)?;
    check_packet(&packet, output)?;
    Ok(packet.receipt)
}
fn check_packet(packet: &Packet, output: &Path) -> Result<()> {
    let output = std::path::absolute(output)?;
    let mut actual = BTreeSet::new();
    inventory(&output, &output, 0, &mut actual)?;
    ensure!(
        actual == packet.files.keys().cloned().collect(),
        "comic packet file set differs"
    );
    for (name, expected) in &packet.files {
        ensure!(
            read(&output.join(name), expected.len() as u64)? == *expected,
            "comic packet differs"
        );
    }
    Ok(())
}
/// Deterministic replay against injected exact source/output readers. Performs
/// no filesystem writes and does not infer worker identity, rights or custody.
pub fn check_with(
    request: &[u8],
    mut input: impl FnMut(&Pin) -> Result<Vec<u8>>,
    mut output: impl FnMut(&str, u64) -> Result<Vec<u8>>,
) -> Result<Receipt> {
    let packet = prepare_with(request, &mut input)?;
    for (name, expected) in &packet.files {
        ensure!(
            output(name, expected.len() as u64)? == *expected,
            "comic packet differs"
        );
    }
    Ok(packet.receipt)
}
/// Build a bounded producer packet in memory from exact source reads. Storage,
/// workspace ownership and native task completion remain the caller's concern.
pub fn capture_with(
    request: &[u8],
    mut input: impl FnMut(&Pin) -> Result<Vec<u8>>,
) -> Result<(Receipt, BTreeMap<String, Vec<u8>>)> {
    let packet = prepare_with(request, &mut input)?;
    Ok((packet.receipt, packet.files))
}
pub(crate) fn inventory(
    root: &Path,
    at: &Path,
    depth: usize,
    files: &mut BTreeSet<String>,
) -> Result<()> {
    ensure!(
        depth <= 16 && ordinary(at)?.is_dir(),
        "packet directory bound exceeded"
    );
    for entry in fs::read_dir(at)? {
        let path = entry?.path();
        let m = ordinary(&path)?;
        if m.is_dir() {
            inventory(root, &path, depth + 1, files)?;
        } else {
            ensure!(
                m.is_file() && files.len() < 1024,
                "packet file/count refused"
            );
            files.insert(
                path.strip_prefix(root)?
                    .to_str()
                    .context("packet path encoding refused")?
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}
/// Requires an exact externally retained receipt hash, not trust in a mutable
/// receipt filename. All source bytes verify before a new destination is created.
pub fn restore_sources(packet: &Path, destination: &Path, receipt_sha256: &str) -> Result<()> {
    let packet = std::path::absolute(packet)?;
    let destination = std::path::absolute(destination)?;
    let raw = read(&packet.join("receipt.json"), 1024 * 1024)?;
    ensure!(
        object(&raw).sha256 == receipt_sha256,
        "restoration receipt pin differs"
    );
    let receipt: Receipt = serde_json::from_slice(&raw)?;
    ensure!(
        receipt.schema == "reel.comic-build-receipt.v1" && receipt.source_files.len() <= 513,
        "restoration receipt refused"
    );
    let mut total = 0u64;
    let mut files = BTreeMap::new();
    let mut names = BTreeSet::new();
    for (name, pin) in &receipt.source_files {
        ensure!(
            portable(name)
                && names.insert(name.to_ascii_lowercase())
                && (1..=16 * 1024 * 1024).contains(&pin.bytes),
            "restoration source refused"
        );
        total = total
            .checked_add(pin.bytes)
            .context("restore byte overflow")?;
        ensure!(total <= 64 * 1024 * 1024, "restore bytes exceeded");
        let bytes = read(&packet.join("sources").join(name), pin.bytes)?;
        ensure!(object(&bytes) == *pin, "restoration source pin differs");
        files.insert(name.clone(), bytes);
    }
    ensure!(
        files
            .get("request.json")
            .is_some_and(|b| object(b) == receipt.request),
        "restored request differs"
    );
    write_new(&destination, &files)
}
