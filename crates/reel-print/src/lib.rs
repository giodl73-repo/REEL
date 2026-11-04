//! Local, provider-neutral print production. All inputs are captured by hash
//! before parsing, all outputs are budgeted before a new directory is reserved.
//! No musical interpretation, selection, publication or cache mutation.
mod pages;
mod pdf;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const SCHEMA: &str = "reel.print-collection.v1";
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub pages: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Entry {
    Text {
        id: String,
        title: String,
        source: Pin,
    },
    ParallelText {
        id: String,
        title: String,
        left: Pin,
        right: Pin,
        left_label: String,
        right_label: String,
    },
    NativeScore {
        id: String,
        title: String,
        source: Pin,
        systems_per_page: usize,
    },
    RetainedPdf {
        id: String,
        source: Pin,
    },
}
impl Entry {
    fn id(&self) -> &str {
        match self {
            Self::Text { id, .. }
            | Self::ParallelText { id, .. }
            | Self::NativeScore { id, .. }
            | Self::RetainedPdf { id, .. } => id,
        }
    }
    fn pins(&self) -> Vec<&Pin> {
        match self {
            Self::ParallelText { left, right, .. } => vec![left, right],
            Self::Text { source, .. }
            | Self::NativeScore { source, .. }
            | Self::RetainedPdf { source, .. } => vec![source],
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub title: String,
    pub author: String,
    pub notice: String,
    pub private_only: bool,
    pub fonts: Vec<Pin>,
    pub body_font: String,
    pub entries: Vec<Entry>,
    pub limits: Limits,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Object {
    pub sha256: String,
    pub bytes: u64,
}
fn object(bytes: &[u8]) -> Object {
    Object {
        sha256: Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        bytes: bytes.len() as u64,
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub entry: String,
    pub kind: String,
    pub width: f32,
    pub height: f32,
    pub source_band: Option<[f32; 2]>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub request: Object,
    pub inputs: Vec<Pin>,
    pub outputs: BTreeMap<String, Object>,
    pub pages: Vec<Page>,
    pub human_approval: bool,
    pub published: bool,
    pub authorizes_eviction: bool,
    pub pdf_raster_check: String,
}
struct Packet {
    receipt: Receipt,
    files: BTreeMap<String, Vec<u8>>,
}

fn ordinary(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let link = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let link = metadata.file_type().is_symlink();
    ensure!(!link, "ordinary file or directory required");
    Ok(metadata)
}
fn read(path: &Path, max: u64) -> Result<Vec<u8>> {
    let m = ordinary(path)?;
    ensure!(
        m.is_file() && m.len() <= max,
        "input exceeds ordinary-file bound"
    );
    let mut raw = Vec::new();
    fs::File::open(path)?.take(max + 1).read_to_end(&mut raw)?;
    ensure!(raw.len() as u64 <= max, "input grew beyond bound");
    Ok(raw)
}
fn capture(pin: &Pin, base: &Path) -> Result<Vec<u8>> {
    ensure!(
        pin.sha256.len() == 64
            && pin
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid source hash"
    );
    ensure!(
        pin.bytes > 0 && pin.bytes <= 32 * 1024 * 1024,
        "invalid source byte bound"
    );
    let path = if pin.path.is_absolute() {
        pin.path.clone()
    } else {
        ensure!(
            !pin.path.components().any(|c| matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )),
            "relative source escapes request directory"
        );
        base.join(&pin.path)
    };
    for ancestor in path.parent().context("source parent required")?.ancestors() {
        ensure!(
            ordinary(ancestor)?.is_dir(),
            "ordinary source ancestors required"
        );
    }
    let raw = read(&path, pin.bytes)?;
    let actual = object(&raw);
    ensure!(
        actual.sha256 == pin.sha256 && actual.bytes == pin.bytes,
        "source pin mismatch"
    );
    Ok(raw)
}
fn prepare(path: &Path) -> Result<Packet> {
    let raw = read(path, 65_536)?;
    let request: Request = serde_json::from_slice(&raw).context("invalid print request")?;
    ensure!(
        request.schema == SCHEMA && request.private_only,
        "explicit private print request required"
    );
    ensure!(
        !request.title.trim().is_empty()
            && request.title.len() <= 1024
            && request.author.len() <= 1024
            && request.notice.len() <= 4096,
        "invalid collection heading"
    );
    ensure!(
        (1..=100).contains(&request.entries.len()) && (1..=8).contains(&request.fonts.len()),
        "bounded entries and explicit fonts required"
    );
    ensure!(
        (1..=256).contains(&request.limits.pages)
            && request.limits.input_bytes <= 256 * 1024 * 1024
            && request.limits.output_bytes <= 256 * 1024 * 1024,
        "invalid print limits"
    );
    let mut ids = std::collections::BTreeSet::new();
    for entry in &request.entries {
        ensure!(
            !entry.id().is_empty() && entry.id().len() <= 256 && ids.insert(entry.id()),
            "duplicate or invalid entry id"
        );
    }
    let base = path.parent().unwrap_or(Path::new("."));
    let pins: Vec<_> = request
        .fonts
        .iter()
        .chain(request.entries.iter().flat_map(Entry::pins))
        .collect();
    let total = pins.iter().try_fold(0u64, |n, p| {
        n.checked_add(p.bytes).context("input byte overflow")
    })?;
    ensure!(
        total <= request.limits.input_bytes,
        "input byte budget exceeded"
    );
    // Read each declared source once. Rendering never reopens a mutable path.
    let captured: Vec<_> = pins
        .iter()
        .map(|p| capture(p, base))
        .collect::<Result<_>>()?;
    let mut fonts = pages::Fonts::new(&request.body_font, &captured[..request.fonts.len()])?;
    let mut produced = pages::text(
        &fonts,
        "collection",
        &request.title,
        &[format!("{}\n\n{}", request.author, request.notice)],
        false,
        None,
    )?;
    let mut at = request.fonts.len();
    for entry in &request.entries {
        let more = match entry {
            Entry::Text { id, title, .. } => pages::text(
                &fonts,
                id,
                title,
                &[String::from_utf8(captured[at].clone())?],
                false,
                None,
            )?,
            Entry::ParallelText {
                id,
                title,
                left_label,
                right_label,
                ..
            } => pages::text(
                &fonts,
                id,
                title,
                &[
                    String::from_utf8(captured[at].clone())?,
                    String::from_utf8(captured[at + 1].clone())?,
                ],
                true,
                Some([left_label, right_label]),
            )?,
            Entry::NativeScore {
                id,
                title,
                systems_per_page,
                ..
            } => pages::notation(&fonts, id, title, &captured[at], *systems_per_page)?,
            Entry::RetainedPdf { id, .. } => pdf::retained(id, &captured[at])?,
        };
        at += entry.pins().len();
        produced.extend(more);
        ensure!(
            produced.len() <= request.limits.pages,
            "page budget exceeded"
        );
    }
    let mut documents = Vec::new();
    let mut descriptions = Vec::new();
    let mut files = BTreeMap::new();
    let mut retained_bytes = 0u64;
    for (index, page) in produced.into_iter().enumerate() {
        let doc = match page.content {
            pages::Content::Svg(svg) => {
                let (pdf, png) = fonts.render(&svg)?;
                retained_bytes += (svg.len() + png.len() + pdf.len()) as u64;
                ensure!(
                    retained_bytes <= request.limits.output_bytes,
                    "output byte budget exceeded"
                );
                files.insert(format!("page-{:03}.svg", index + 1), svg.into_bytes());
                files.insert(format!("page-{:03}.png", index + 1), png);
                std::sync::Arc::new(lopdf::Document::load_mem(&pdf)?)
            }
            pages::Content::Pdf(doc) => doc,
        };
        if !documents
            .last()
            .is_some_and(|last| std::sync::Arc::ptr_eq(last, &doc))
        {
            documents.push(doc);
        }
        descriptions.push(page.description);
    }
    let pdf = pdf::merge(documents)?;
    files.insert("collection.pdf".into(), pdf);
    let bytes = files.values().try_fold(0u64, |n, b| {
        n.checked_add(b.len() as u64)
            .context("output byte overflow")
    })?;
    ensure!(
        bytes <= request.limits.output_bytes,
        "output byte budget exceeded"
    );
    let receipt = Receipt {
        schema: "reel.print-collection-receipt.v1".into(),
        request: object(&raw),
        inputs: pins.into_iter().cloned().collect(),
        outputs: files
            .iter()
            .map(|(name, raw)| (name.clone(), object(raw)))
            .collect(),
        pages: descriptions,
        human_approval: false,
        published: false,
        authorizes_eviction: false,
        pdf_raster_check:
            "external-verification-required; PNGs render the source page SVG, not the written PDF"
                .into(),
    };
    let receipt_raw = serde_json::to_vec_pretty(&receipt)?;
    ensure!(
        bytes + receipt_raw.len() as u64 <= request.limits.output_bytes,
        "receipt exceeds output byte budget"
    );
    files.insert("receipt.json".into(), receipt_raw);
    Ok(Packet { receipt, files })
}
pub fn render(request: &Path, output: &Path) -> Result<Receipt> {
    ensure!(
        output.is_absolute()
            && !output
                .components()
                .any(|c| matches!(c, Component::ParentDir)
                    || c.as_os_str().eq_ignore_ascii_case(".git")),
        "new absolute print directory required"
    );
    ensure!(
        fs::symlink_metadata(output).is_err(),
        "print output already exists"
    );
    for ancestor in output
        .parent()
        .context("output parent required")?
        .ancestors()
    {
        ensure!(
            ordinary(ancestor)?.is_dir(),
            "ordinary output ancestors required"
        );
    }
    let packet = prepare(request)?;
    // Atomic name reservation refuses concurrent writers even for an empty dir.
    // IO failure leaves only owned incomplete staging; no receipt is published.
    fs::create_dir(output)?;
    for (name, raw) in packet
        .files
        .iter()
        .filter(|(name, _)| name.as_str() != "receipt.json")
    {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))?
            .write_all(raw)?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("receipt.json"))?
        .write_all(&packet.files["receipt.json"])?;
    Ok(packet.receipt)
}
pub fn check(request: &Path, output: &Path) -> Result<Receipt> {
    let packet = prepare(request)?;
    ensure!(
        ordinary(output)?.is_dir(),
        "ordinary output directory required"
    );
    let actual: std::collections::BTreeSet<_> = fs::read_dir(output)?
        .take(packet.files.len() + 1)
        .map(|e| e.map(|e| e.file_name()))
        .collect::<std::io::Result<_>>()?;
    let expected: std::collections::BTreeSet<_> =
        packet.files.keys().map(std::ffi::OsString::from).collect();
    ensure!(actual == expected, "unexpected or missing print output");
    for (name, raw) in &packet.files {
        ensure!(
            read(&output.join(name), raw.len() as u64)? == *raw,
            "print output differs from rebuilt packet"
        );
    }
    Ok(packet.receipt)
}
