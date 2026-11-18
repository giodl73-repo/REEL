//! Read-only source/edition and worker-receipt correspondence. These checks do
//! not admit a job, execute a tool, acknowledge retention or authenticate a worker.
use crate::*;
use reel_publication::{
    Content, Publication,
    comic_source::{Object, PageSource, Pin, Receipt, Request},
};

/// The native adapter owns exact committed record-to-object resolution and must
/// hold its guarded reads for the enclosing operation. Returning matching bytes
/// from this seam alone is not qualification of the reader or its custody.
pub trait InputReader {
    fn read(&mut self, artifact: &Artifact, max_bytes: u64) -> Result<Vec<u8>>;
}

#[derive(Debug)]
pub struct BoundComic {
    task_key: String,
    task_id: String,
    publication_id: String,
    edition_id: String,
    language: String,
    page_order: Vec<String>,
    logical_bytes: u64,
    input_objects: usize,
    expected_sources: BTreeMap<String, Object>,
    outputs: OutputContracts,
    executable: Artifact,
}
impl BoundComic {
    pub fn task_key(&self) -> &str {
        &self.task_key
    }
    pub fn input_objects(&self) -> usize {
        self.input_objects
    }
    pub fn logical_bytes(&self) -> u64 {
        self.logical_bytes
    }
}

pub(crate) fn portable_source(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && !path.contains('\\')
        && path.split('/').all(reel_publication::valid_id)
}
fn bad<T>(kind: ErrorKind, code: &str) -> Result<T> {
    Err(Error {
        kind,
        code: code.into(),
    })
}
fn verified(reader: &mut impl InputReader, artifact: &Artifact, max: u64) -> Result<Vec<u8>> {
    if artifact.bytes > max {
        return bad(ErrorKind::Budget, "publication-input-bytes");
    }
    let raw = reader.read(artifact, max)?;
    if raw.len() as u64 != artifact.bytes || digest(&raw) != artifact.sha256 {
        return bad(ErrorKind::Integrity, "publication-input-content");
    }
    Ok(raw)
}
fn pin_object(pin: &Pin) -> Object {
    Object {
        sha256: pin.sha256.clone(),
        bytes: pin.bytes,
    }
}
fn leaf<'a>(packet: &'a Packet, path: &Pin) -> Result<&'a Artifact> {
    if !portable_source(&path.path)
        || path.path == "request.json"
        || path.bytes == 0
        || path.bytes > 16 * 1024 * 1024
    {
        return bad(ErrorKind::Invalid, "producer-relative-pin");
    }
    let tool = packet.task.tool.as_ref().ok_or(Error {
        kind: ErrorKind::Missing,
        code: "deterministic-tool".into(),
    })?;
    let id = tool.source_bindings.get(&path.path).ok_or(Error {
        kind: ErrorKind::Missing,
        code: "source-logical-binding".into(),
    })?;
    let asset = packet
        .task
        .inputs
        .iter()
        .find(|a| &a.logical_id == id)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "source-declared-input".into(),
        })?;
    if asset.sha256 != path.sha256 || asset.bytes != path.bytes {
        return bad(ErrorKind::Conflict, "source-logical-pin-conflict");
    }
    Ok(asset)
}
fn add_source(sources: &mut BTreeMap<String, Object>, pin: &Pin) -> Result<()> {
    if let Some(prior) = sources.insert(pin.path.clone(), pin_object(pin)) {
        if prior != pin_object(pin) {
            return bad(ErrorKind::Conflict, "producer-path-pin-conflict");
        }
    }
    Ok(())
}
fn output_types(pages: &[String]) -> BTreeMap<String, &'static str> {
    let mut types = BTreeMap::new();
    for page in pages {
        for (suffix, media) in [
            ("clean.svg", "image/svg+xml"),
            ("clean.png", "image/png"),
            ("lettering.svg", "image/svg+xml"),
            ("reading.svg", "image/svg+xml"),
            ("reading.png", "image/png"),
        ] {
            types.insert(format!("{page}-{suffix}"), media);
        }
    }
    types.insert("reading.pdf".into(), "application/pdf");
    types.insert("transcript.json".into(), "application/json");
    types
}

/// Replays the exact declared closure and binds the selected page identities,
/// language, order, source objects and expected worker outputs. No files are written.
pub fn bind_comic(packet: &Packet, reader: &mut impl InputReader) -> Result<BoundComic> {
    check_packet(packet)?;
    if packet.task.operation != Operation::DeterministicTool {
        return bad(ErrorKind::Invalid, "publication-tool-task-required");
    }
    let tool = packet.task.tool.as_ref().ok_or(Error {
        kind: ErrorKind::Missing,
        code: "deterministic-tool".into(),
    })?;
    let publication: Publication = decode(&verified(
        reader,
        &tool.publication,
        MAX_CONTROL_BYTES as u64,
    )?)?;
    reel_publication::check_publication(&publication)?;
    let request: Request = decode(&verified(reader, &tool.request, MAX_CONTROL_BYTES as u64)?)?;
    let edition = publication
        .editions
        .iter()
        .find(|e| e.id == tool.edition_id)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "selected-edition".into(),
        })?;
    if request.schema != "reel.comic-build.v1"
        || request.publication_id != publication.id
        || request.edition_id != edition.id
        || request.language != edition.language
        || packet.task.language.as_deref() != Some(edition.language.as_str())
    {
        return bad(ErrorKind::Conflict, "publication-edition-language-binding");
    }
    let Content::Comic { pages, .. } = &edition.content else {
        return bad(ErrorKind::Unsupported, "comic-capability-required");
    };
    if pages.len() != request.pages.len()
        || pages.is_empty()
        || pages.len() > 32
        || request.limits.input_bytes == 0
        || request.limits.input_bytes > 64 * 1024 * 1024
        || request.limits.output_bytes == 0
        || request.limits.output_bytes > 128 * 1024 * 1024
    {
        return bad(ErrorKind::Budget, "comic-producer-scope");
    }
    if packet.task.outputs != edition.outputs {
        return bad(ErrorKind::Conflict, "edition-output-contract");
    }
    let page_order: Vec<_> = pages.iter().map(|p| p.id.clone()).collect();
    let types = output_types(&page_order);
    if types.len() != edition.outputs.len()
        || types.iter().any(|(name, media)| {
            edition.outputs.get(name).is_none_or(|c| {
                c.media_type != *media || !c.required || c.max_bytes > request.limits.output_bytes
            })
        })
    {
        return bad(ErrorKind::Unsupported, "comic-worker-output-contract");
    }
    let mut sources = BTreeMap::new();
    for pin in [&request.font, &request.font_license] {
        leaf(packet, pin)?;
        add_source(&mut sources, pin)?;
    }
    let mut source_ids = BTreeSet::from([
        tool.publication.logical_id.clone(),
        tool.request.logical_id.clone(),
    ]);
    let mut read_ids = BTreeSet::from([
        tool.publication.logical_id.clone(),
        tool.request.logical_id.clone(),
    ]);
    for (selected, pin) in pages.iter().zip(&request.pages) {
        let asset = leaf(packet, pin)?;
        // Matching content is insufficient: require the selected logical page input.
        if asset != &selected.source || asset.media_type != "application/json" {
            return bad(ErrorKind::Conflict, "selected-page-logical-binding");
        }
        let source: PageSource = decode(&verified(reader, asset, 256 * 1024)?)?;
        read_ids.insert(asset.logical_id.clone());
        if source.schema != "reel.comic-page.v1"
            || source.id != selected.id
            || source.panels.iter().map(|p| &p.id).collect::<Vec<_>>()
                != selected.panel_order.iter().collect::<Vec<_>>()
            || source.utterances.iter().map(|u| &u.id).collect::<Vec<_>>()
                != selected.utterance_order.iter().collect::<Vec<_>>()
        {
            return bad(ErrorKind::Conflict, "selected-page-order-binding");
        }
        add_source(&mut sources, pin)?;
        for layer in &source.layers {
            leaf(packet, &layer.source)?;
            add_source(&mut sources, &layer.source)?;
        }
    }
    if sources.len() != tool.source_bindings.len()
        || sources
            .keys()
            .any(|path| !tool.source_bindings.contains_key(path))
    {
        return bad(ErrorKind::Conflict, "unused-source-binding");
    }
    let mut folded = BTreeSet::new();
    for path in sources.keys() {
        if !folded.insert(path.to_ascii_lowercase()) {
            return bad(ErrorKind::Conflict, "source-path-case-collision");
        }
        source_ids.insert(tool.source_bindings[path].clone());
    }
    if source_ids.len() != packet.task.inputs.len()
        || packet
            .task
            .inputs
            .iter()
            .any(|a| !source_ids.contains(&a.logical_id))
    {
        return bad(ErrorKind::Conflict, "undeclared-or-unused-input");
    }
    let logical_bytes = packet.task.inputs.iter().try_fold(0u64, |n, a| {
        n.checked_add(a.bytes).ok_or(Error {
            kind: ErrorKind::Budget,
            code: "input-byte-overflow".into(),
        })
    })?;
    if logical_bytes > 64 * 1024 * 1024 {
        return bad(ErrorKind::Budget, "publication-closure-bytes");
    }
    let producer_bytes = sources.values().try_fold(tool.request.bytes, |n, object| {
        n.checked_add(object.bytes).ok_or(Error {
            kind: ErrorKind::Budget,
            code: "producer-input-overflow".into(),
        })
    })?;
    if producer_bytes > request.limits.input_bytes {
        return bad(ErrorKind::Budget, "producer-input-allowance");
    }
    for asset in &packet.task.inputs {
        if !read_ids.contains(&asset.logical_id) {
            verified(reader, asset, 16 * 1024 * 1024)?;
        }
    }
    sources.insert(
        "request.json".into(),
        Object {
            sha256: tool.request.sha256.clone(),
            bytes: tool.request.bytes,
        },
    );
    Ok(BoundComic {
        task_key: packet.task_key.clone(),
        task_id: packet.task.id.clone(),
        publication_id: publication.id,
        edition_id: edition.id.clone(),
        language: edition.language.clone(),
        page_order,
        logical_bytes,
        input_objects: source_ids.len(),
        expected_sources: sources,
        outputs: edition.outputs.clone(),
        executable: tool.executable.clone(),
    })
}

pub fn check_tool_bytes(packet: &Packet, bytes: &[u8]) -> Result<()> {
    check_packet(packet)?;
    let tool = packet.task.tool.as_ref().ok_or(Error {
        kind: ErrorKind::Missing,
        code: "deterministic-tool".into(),
    })?;
    if bytes.len() as u64 != tool.executable.bytes || digest(bytes) != tool.executable.sha256 {
        return bad(ErrorKind::Integrity, "tool-executable-content");
    }
    Ok(())
}

/// Source/result correspondence only. Supplied executable provenance is not a
/// signature or evidence of an actual managed run. Output byte checks remain separate.
pub fn check_comic_receipt(
    bound: &BoundComic,
    result: &ReturnedResult,
    receipt_bytes: &[u8],
) -> Result<()> {
    let receipt: Receipt = decode(receipt_bytes)?;
    if result.schema != RESULT_SCHEMA
        || result.task_id != bound.task_id
        || result.task_key != bound.task_key
        || !token(&result.attempt_id)
        || result.provenance.mode != Mode::Tool
        || result.provenance.executable.as_ref() != Some(&bound.executable)
        || result.provenance.provider.is_some()
        || result.provenance.model.is_some()
    {
        return bad(ErrorKind::Conflict, "comic-result-task-provenance");
    }
    if receipt.schema != "reel.comic-build-receipt.v1"
        || receipt.publication_id != bound.publication_id
        || receipt.edition_id != bound.edition_id
        || receipt.language != bound.language
        || receipt.page_order != bound.page_order
        || receipt.source_files != bound.expected_sources
        || bound.expected_sources.get("request.json") != Some(&receipt.request)
        || receipt.outputs != result.artifacts
    {
        return bad(ErrorKind::Conflict, "comic-receipt-source-correspondence");
    }
    if receipt.human_approval || receipt.retained_owner_acknowledged || receipt.print_ready {
        return bad(ErrorKind::Denied, "worker-receipt-claims-authority");
    }
    reel_publication::check_outputs(&bound.outputs, &result.artifacts)?;
    for (name, asset) in &result.artifacts {
        if asset.logical_id != format!("{}/{}/{}", bound.publication_id, bound.edition_id, name) {
            return bad(ErrorKind::Conflict, "comic-output-logical-binding");
        }
    }
    Ok(())
}
