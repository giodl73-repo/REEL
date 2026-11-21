//! Authored reading indexes over independently checked audio and print packets.
//! All detailed output is private; providers retain their own byte guards.
use super::{Document, checker, compiler, layers, producer};
use crate::{
    media_runtime::ByteIdentity,
    scene_delivery::{FileRef, InputFiles},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const SCHEMA: &str = "reel.narrated-reading.v1";
const CONTROL: u64 = 4 * 1024 * 1024;
const PRINT: u64 = 32 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub id: String,
    pub declaration: FileRef,
    pub producer: FileRef,
    pub processing: Option<FileRef>,
    pub dialogue_receipt: FileRef,
    pub access: FileRef,
    pub print_request: FileRef,
    pub print_receipt: FileRef,
    pub language_columns: BTreeMap<String, Column>,
    pub editions: Vec<Edition>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Column {
    Text,
    Left,
    Right,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edition {
    pub edition_id: String,
    pub language: String,
    pub compiled_recipe_sha256: String,
    pub execution_recipe_sha256: String,
    pub audio: Audio,
    pub chapters: Vec<Chapter>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Audio {
    Dialogue,
    Layered { mix_identity_sha256: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chapter {
    pub module_id: String,
    pub print_entry_id: String,
    pub spans: Vec<Span>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub performance_id: String,
    pub start_byte: usize,
    pub end_byte: usize,
}
pub struct Layered<'a> {
    pub prepared: &'a layers::Prepared<'a>,
    pub directory: &'a Path,
}
#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub assets: &'a dyn InputFiles,
    pub dialogue: &'a dyn InputFiles,
    pub access: &'a dyn InputFiles,
    pub print: &'a dyn InputFiles,
    pub layered: &'a BTreeMap<String, Layered<'a>>,
}
/// Sealed independent result. Each use revalidates every consumed original pin.
pub struct Prepared<'a> {
    request: Request,
    inputs: Inputs<'a>,
    files: BTreeMap<String, Vec<u8>>,
    bindings: BTreeMap<(u8, PathBuf), FileRef>,
    layered_receipts: BTreeMap<String, FileRef>,
}
type Files = BTreeMap<String, Vec<u8>>;
struct Derived {
    files: Files,
    layered_receipts: BTreeMap<String, FileRef>,
}
struct Capture<'a> {
    provider: &'a dyn InputFiles,
    namespace: u8,
    bindings: &'a RefCell<BTreeMap<(u8, PathBuf), FileRef>>,
}
impl Capture<'_> {
    fn record(&self, pin: &FileRef) -> Result<()> {
        let mut bindings = self.bindings.borrow_mut();
        let key = (self.namespace, pin.path.clone());
        if let Some(prior) = bindings.get(&key) {
            ensure!(
                prior.sha256 == pin.sha256 && prior.bytes == pin.bytes,
                "reading conflicting input pins"
            );
        } else {
            ensure!(
                bindings.len() < 4096,
                "reading binding count exceeds profile"
            );
            let total = bindings
                .values()
                .try_fold(pin.bytes, |n, p| n.checked_add(p.bytes))
                .context("reading binding size overflow")?;
            ensure!(
                pin.bytes <= 64 * 1024 * 1024 && total <= 256 * 1024 * 1024,
                "reading binding bytes exceed profile"
            );
            bindings.insert(key, pin.clone());
        }
        Ok(())
    }
}
impl InputFiles for Capture<'_> {
    fn verified_path(&self, pin: &FileRef) -> Result<PathBuf> {
        self.record(pin)?;
        self.provider.verified_path(pin)
    }
    fn control_bytes(&self, pin: &FileRef) -> Result<Vec<u8>> {
        self.record(pin)?;
        self.provider.control_bytes(pin)
    }
    fn validate_reference_name(&self, pin: &FileRef) -> Result<()> {
        self.provider.validate_reference_name(pin)
    }
    fn media_program(&self, program: crate::scene_delivery::MediaProgram) -> Result<PathBuf> {
        self.provider.media_program(program)
    }
}
#[derive(Serialize)]
pub struct Summary {
    pub editions: usize,
    pub chapters: usize,
    pub source_order_checked: bool,
    pub authority_verified: bool,
    pub release_authorized: bool,
}
fn digest(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn reference(path: impl Into<PathBuf>, raw: &[u8]) -> FileRef {
    FileRef {
        path: path.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
    }
}
fn read(
    provider: &dyn InputFiles,
    pin: &FileRef,
    max: u64,
    tick: &mut impl FnMut() -> Result<()>,
) -> Result<Vec<u8>> {
    tick()?;
    ensure!(
        pin.bytes > 0
            && pin.bytes <= max
            && pin.sha256.len() == 64
            && pin
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "reading pin exceeds profile"
    );
    provider.validate_reference_name(pin)?;
    let raw = if pin.bytes <= CONTROL {
        provider.control_bytes(pin)?
    } else {
        let mut raw = Vec::new();
        fs::File::open(provider.verified_path(pin)?)?
            .take(pin.bytes + 1)
            .read_to_end(&mut raw)?;
        raw
    };
    ensure!(
        raw.len() as u64 == pin.bytes && digest(&raw) == pin.sha256,
        "reading input bytes differ"
    );
    tick()?;
    Ok(raw)
}
fn from_pin(pin: &reel_print::Pin) -> FileRef {
    FileRef {
        path: pin.path.clone(),
        sha256: pin.sha256.clone(),
        bytes: pin.bytes,
    }
}
fn entry(entry: &reel_print::Entry, column: Column) -> Result<(&str, &reel_print::Pin)> {
    match (entry, column) {
        (reel_print::Entry::Text { id, source, .. }, Column::Text) => Ok((id, source)),
        (reel_print::Entry::ParallelText { id, left, .. }, Column::Left) => Ok((id, left)),
        (reel_print::Entry::ParallelText { id, right, .. }, Column::Right) => Ok((id, right)),
        _ => anyhow::bail!("reading column or visual entry unsupported"),
    }
}
fn derive(
    request: &Request,
    inputs: Inputs<'_>,
    tick: &mut impl FnMut() -> Result<()>,
    remaining: &impl Fn() -> Duration,
) -> Result<Derived> {
    tick()?;
    ensure!(
        request.schema == SCHEMA
            && !request.id.is_empty()
            && request.id.len() <= 256
            && (1..=4).contains(&request.editions.len())
            && request.language_columns.len() <= 4,
        "invalid reading profile"
    );
    let declaration = read(
        inputs.assets,
        &request.declaration,
        super::MAX_DECLARATION_BYTES as u64,
        tick,
    )?;
    let document: Document = serde_json::from_slice(&declaration)?;
    ensure!(
        request.producer.bytes > 0 && request.producer.bytes <= 64 * 1024 * 1024,
        "reading producer budget exceeded"
    );
    inputs.assets.validate_reference_name(&request.producer)?;
    let identity = ByteIdentity::observe_file_bounded(
        &inputs.assets.verified_path(&request.producer)?,
        64 * 1024 * 1024,
    )?;
    ensure!(
        identity.sha256 == request.producer.sha256 && identity.bytes == request.producer.bytes,
        "reading original producer differs"
    );
    let runtime = compiler::CompilerIdentity {
        sha256: identity.sha256,
        bytes: identity.bytes,
    };
    let processing = match &request.processing {
        Some(pin) => producer::parse_processing(&read(
            inputs.assets,
            pin,
            super::MAX_DECLARATION_BYTES as u64,
            tick,
        )?)?,
        None => producer::ProcessingPolicy::identity(),
    };
    let receipt_bytes = read(inputs.dialogue, &request.dialogue_receipt, CONTROL, tick)?;
    let receipt: producer::ProductionReceipt = serde_json::from_slice(&receipt_bytes)?;
    let plan = compiler::compile_with_inputs(
        &document,
        inputs.assets,
        &runtime,
        receipt.compilation_profile,
    )?;
    let plan_bytes = serde_json::to_vec_pretty(&plan)?;
    let access_bytes = read(inputs.access, &request.access, CONTROL, tick)?;
    // Untrusted metadata is used only for early structural rejection. It is
    // independently rebuilt and checked before any prepared result is returned.
    let access_package: checker::AccessPackage = serde_json::from_slice(&access_bytes)?;
    let print_bytes = read(inputs.assets, &request.print_request, 65_536, tick)?;
    let print_request: reel_print::Request = serde_json::from_slice(&print_bytes)?;
    ensure!(
        (1..=16).contains(&print_request.entries.len())
            && print_request.limits.pages <= 32
            && print_request.limits.input_bytes <= PRINT
            && print_request.limits.output_bytes <= PRINT,
        "reading print profile exceeded"
    );
    ensure!(
        print_request.entries.iter().all(|e| matches!(
            e,
            reel_print::Entry::Text { .. } | reel_print::Entry::ParallelText { .. }
        )),
        "reading visual reference unsupported"
    );
    let original_print_receipt = read(inputs.print, &request.print_receipt, CONTROL, tick)?;
    let print_receipt: reel_print::Receipt = serde_json::from_slice(&original_print_receipt)?;
    // The namespace includes the receipt itself; it is rebuilt from original controls.
    let mut namespace = print_receipt.outputs.clone();
    ensure!(
        !namespace.contains_key("receipt.json"),
        "recursive print receipt"
    );
    namespace.insert(
        "receipt.json".into(),
        reel_print::Object {
            sha256: request.print_receipt.sha256.clone(),
            bytes: request.print_receipt.bytes,
        },
    );
    let rebuilt = reel_print::check_with_inputs(
        &print_bytes,
        |pin| read(inputs.assets, &from_pin(pin), PRINT, tick),
        &namespace,
        |name, object| {
            let pin = if name == "receipt.json" {
                request.print_receipt.clone()
            } else {
                FileRef {
                    path: name.into(),
                    sha256: object.sha256.clone(),
                    bytes: object.bytes,
                }
            };
            // These callbacks execute sequentially, but share the same outer deadline.
            read(inputs.print, &pin, PRINT, &mut || Ok(()))
        },
    )?;
    ensure!(rebuilt == print_receipt, "reading print receipt differs");
    tick()?;
    let mut ids = BTreeSet::new();
    let mut languages = BTreeSet::new();
    let mut editions = Vec::new();
    let mut files = BTreeMap::new();
    let mut layered_receipts = BTreeMap::new();
    for (ordinal, edition) in request.editions.iter().enumerate() {
        tick()?;
        ensure!(ids.insert(&edition.edition_id), "duplicate reading edition");
        languages.insert(&edition.language);
        let column = *request
            .language_columns
            .get(&edition.language)
            .context("reading language column missing")?;
        let compiled = plan
            .editions
            .iter()
            .find(|e| e.recipe.edition_id == edition.edition_id)
            .context("reading edition absent")?;
        let access = access_package
            .editions
            .iter()
            .find(|e| e.edition_id == edition.edition_id)
            .context("reading access edition absent")?;
        ensure!(
            edition.language == compiled.recipe.language
                && edition.language == access.language
                && edition.compiled_recipe_sha256 == compiled.recipe_sha256
                && edition.execution_recipe_sha256 == access.execution_recipe_sha256,
            "reading language or parent recipe differs"
        );
        ensure!(
            edition.chapters.len() == print_request.entries.len()
                && edition.chapters.len() == compiled.chapters.len()
                && compiled.recipe.cues.len() <= 64,
            "reading chapter/cue coverage differs"
        );
        let mut text = String::new();
        let mut chapters = Vec::new();
        for ((chapter, clock), printed) in edition
            .chapters
            .iter()
            .zip(&compiled.chapters)
            .zip(&print_request.entries)
        {
            let (entry_id, source) = entry(printed, column)?;
            ensure!(
                chapter.module_id == clock.module_id && chapter.print_entry_id == entry_id,
                "reading chapter order differs"
            );
            let source_pin = from_pin(source);
            let source_bytes = read(inputs.assets, &source_pin, 256 * 1024, tick)?;
            let source_text =
                std::str::from_utf8(&source_bytes).context("reading source must be UTF-8")?;
            let cues: Vec<_> = compiled
                .recipe
                .cues
                .iter()
                .filter(|c| c.module_id == chapter.module_id)
                .collect();
            ensure!(
                !cues.is_empty() && cues.len() == chapter.spans.len(),
                "reading cue span coverage differs"
            );
            let mut cursor = 0;
            let mut spans = Vec::new();
            for (span, cue) in chapter.spans.iter().zip(cues) {
                ensure!(
                    span.performance_id == cue.performance.id
                        && span.start_byte >= cursor
                        && span.end_byte > span.start_byte,
                    "reading span order differs"
                );
                let gap = source_text
                    .get(cursor..span.start_byte)
                    .context("reading UTF-8 span boundary invalid")?;
                let selected = source_text
                    .get(span.start_byte..span.end_byte)
                    .context("reading UTF-8 span boundary invalid")?;
                ensure!(
                    gap.trim().is_empty() && selected == cue.script.text,
                    "reading authored wording differs"
                );
                cursor = span.end_byte;
                spans.push(serde_json::json!({"performance_id":span.performance_id,"start_byte":span.start_byte,"end_byte":span.end_byte,"script_sha256":cue.script.sha256,"text_basis":"authored-source"}));
            }
            ensure!(
                source_text
                    .get(cursor..)
                    .context("reading source remainder invalid")?
                    .trim()
                    .is_empty(),
                "reading omits source text"
            );
            let pages: Vec<_> = rebuilt
                .pages
                .iter()
                .enumerate()
                .filter(|(_, p)| p.entry == entry_id)
                .map(|(i, _)| i + 1)
                .collect();
            ensure!(!pages.is_empty(), "reading print navigation absent");
            chapters.push(serde_json::json!({"module_id":chapter.module_id,"print_entry_id":entry_id,"source":source_pin,"source_provider":"assets","spans":spans,"pages":pages,"start_sample":clock.start_sample,"end_sample":clock.end_sample}));
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(source_text);
        }
        let (audio, provider) = match &edition.audio {
            Audio::Dialogue => (access.wave.clone(), "dialogue"),
            Audio::Layered {
                mix_identity_sha256,
            } => {
                let layer = inputs
                    .layered
                    .get(&edition.edition_id)
                    .context("reading checked mix missing")?;
                let root = layer.directory.to_path_buf();
                let receipt_path = root.join("receipt.json");
                let mut bytes = Vec::new();
                fs::File::open(&receipt_path)?
                    .take(CONTROL + 1)
                    .read_to_end(&mut bytes)?;
                ensure!(
                    bytes.len() as u64 <= CONTROL,
                    "reading mix receipt exceeds budget"
                );
                layers::check(layer.prepared, layer.directory, remaining())?;
                ensure!(
                    read(&root, &reference("receipt.json", &bytes), CONTROL, tick)? == bytes,
                    "reading mix receipt changed during check"
                );
                layered_receipts.insert(
                    edition.edition_id.clone(),
                    reference("receipt.json", &bytes),
                );
                let mix: layers::Receipt = serde_json::from_slice(&bytes)?;
                ensure!(
                    mix.mix_identity_sha256 == *mix_identity_sha256
                        && mix.request.edition_id == edition.edition_id
                        && mix.request.compiled_recipe_sha256 == edition.compiled_recipe_sha256
                        && mix.request.execution_recipe_sha256 == edition.execution_recipe_sha256
                        && mix.dialogue.sha256 == access.wave.sha256
                        && mix.dialogue.bytes == access.wave.bytes
                        && mix.samples == access.duration_samples,
                    "reading mix parent differs"
                );
                (
                    mix.outputs
                        .get("mix.wav")
                        .context("reading mix output absent")?
                        .clone(),
                    "layered",
                )
            }
        };
        let name = format!("reading-{ordinal:03}.txt");
        let authored = reference(&name, text.as_bytes());
        files.insert(name, text.into_bytes());
        editions.push(serde_json::json!({"edition_id":edition.edition_id,"language":edition.language,
            "compiled_recipe_sha256":edition.compiled_recipe_sha256,"execution_recipe_sha256":edition.execution_recipe_sha256,
            "audio":audio,"audio_provider":provider,"duration_samples":access.duration_samples,
            "transcript":access.transcript,"transcript_provider":"access","authored_text":authored,"chapters":chapters}));
    }
    ensure!(
        languages.len() == request.language_columns.len(),
        "unused reading language column"
    );
    let index = serde_json::json!({"schema":SCHEMA,"request":request,"sample_rate":48000,"editions":editions,
        "print_outputs":rebuilt.outputs,"print_provider":"print","layered_receipts":layered_receipts,"text_basis":"authored-source",
        "pdf_raster_check":rebuilt.pdf_raster_check,"authority_verified":false,"release_authorized":false});
    files.insert("reading.json".into(), serde_json::to_vec_pretty(&index)?);
    ensure!(
        files.values().map(Vec::len).sum::<usize>() <= CONTROL as usize,
        "reading output budget exceeded"
    );
    let mut checked = checker::check(
        checker::CheckRequest {
            document: &document,
            plan_bytes: &plan_bytes,
            receipt_bytes: &receipt_bytes,
            processing: &processing,
            expected_runtime: &runtime,
        },
        inputs.assets,
        inputs.dialogue,
        producer::ProductionControl {
            timeout: remaining(),
            cancellation: &mut *tick,
        },
    )?;
    checker::verify_access(&mut checked, &access_bytes, inputs.access, &mut *tick)?;
    tick()?;
    Ok(Derived {
        files,
        layered_receipts,
    })
}
fn deadline(timeout: Duration) -> Result<Instant> {
    ensure!(
        timeout <= Duration::from_secs(30),
        "reading deadline exceeds profile"
    );
    Ok(Instant::now() + timeout)
}
pub fn prepare<'a>(
    request: Request,
    inputs: Inputs<'a>,
    timeout: Duration,
) -> Result<Prepared<'a>> {
    ensure!(
        serde_json::to_vec(&request)?.len() <= super::MAX_DECLARATION_BYTES,
        "reading request exceeds budget"
    );
    let deadline = deadline(timeout)?;
    let bindings = RefCell::new(BTreeMap::new());
    let captured = |provider, namespace| Capture {
        provider,
        namespace,
        bindings: &bindings,
    };
    let assets = captured(inputs.assets, 0);
    let dialogue = captured(inputs.dialogue, 1);
    let access = captured(inputs.access, 2);
    let print = captured(inputs.print, 3);
    let derived = derive(
        &request,
        Inputs {
            assets: &assets,
            dialogue: &dialogue,
            access: &access,
            print: &print,
            layered: inputs.layered,
        },
        &mut || {
            ensure!(Instant::now() < deadline, "reading deadline elapsed");
            Ok(())
        },
        &|| deadline.saturating_duration_since(Instant::now()),
    )?;
    Ok(Prepared {
        request,
        inputs,
        files: derived.files,
        layered_receipts: derived.layered_receipts,
        bindings: bindings.into_inner(),
    })
}
fn recheck(
    prepared: &Prepared<'_>,
    deadline: Instant,
    tick: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    for ((namespace, _), pin) in &prepared.bindings {
        tick()?;
        let provider = match namespace {
            0 => prepared.inputs.assets,
            1 => prepared.inputs.dialogue,
            2 => prepared.inputs.access,
            3 => prepared.inputs.print,
            _ => unreachable!(),
        };
        if pin.bytes <= CONTROL {
            read(provider, pin, CONTROL, tick)?;
        } else {
            let actual =
                ByteIdentity::observe_file_bounded(&provider.verified_path(pin)?, pin.bytes)?;
            ensure!(
                actual.sha256 == pin.sha256 && actual.bytes == pin.bytes,
                "reading original binding bytes differ"
            );
        }
    }
    for (edition, pin) in &prepared.layered_receipts {
        let layer = &prepared.inputs.layered[edition];
        let root = layer.directory.to_path_buf();
        read(&root, pin, CONTROL, tick)?;
        layers::check(
            layer.prepared,
            layer.directory,
            deadline.saturating_duration_since(Instant::now()),
        )?;
        read(&root, pin, CONTROL, tick)?;
    }
    tick()?;
    Ok(())
}
pub fn produce(
    prepared: &Prepared<'_>,
    output: &Path,
    mut control: producer::ProductionControl<impl FnMut() -> Result<()>>,
) -> Result<Summary> {
    let deadline = deadline(control.timeout)?;
    let mut tick = || {
        (control.cancellation)()?;
        ensure!(Instant::now() < deadline, "reading deadline elapsed");
        Ok(())
    };
    recheck(prepared, deadline, &mut tick)?;
    let mut owner = producer::OutputOwner::reserve(output)?;
    for (name, bytes) in prepared
        .files
        .iter()
        .filter(|(n, _)| n.as_str() != "reading.json")
    {
        producer::persist_file(&mut owner, name, bytes, &mut tick)?;
    }
    producer::persist_file(
        &mut owner,
        "reading.json",
        &prepared.files["reading.json"],
        &mut tick,
    )?;
    owner.finish();
    Ok(summary(prepared))
}
pub fn check(prepared: &Prepared<'_>, output: &Path, timeout: Duration) -> Result<Summary> {
    let deadline = deadline(timeout)?;
    let mut tick = || {
        ensure!(Instant::now() < deadline, "reading deadline elapsed");
        Ok(())
    };
    recheck(prepared, deadline, &mut tick)?;
    let names: BTreeSet<_> = fs::read_dir(output)?
        .take(prepared.files.len() + 1)
        .map(|e| e.map(|e| e.file_name()))
        .collect::<std::io::Result<_>>()?;
    ensure!(
        names
            == prepared
                .files
                .keys()
                .map(std::ffi::OsString::from)
                .collect(),
        "reading packet namespace differs"
    );
    let root = output.to_path_buf();
    for (name, expected) in &prepared.files {
        ensure!(
            read(&root, &reference(name, expected), CONTROL, &mut tick)? == *expected,
            "reading packet bytes differ"
        );
    }
    Ok(summary(prepared))
}
fn summary(prepared: &Prepared<'_>) -> Summary {
    Summary {
        editions: prepared.request.editions.len(),
        chapters: prepared
            .request
            .editions
            .iter()
            .map(|e| e.chapters.len())
            .sum(),
        source_order_checked: true,
        authority_verified: false,
        release_authorized: false,
    }
}
