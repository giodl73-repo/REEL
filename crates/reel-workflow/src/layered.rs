//! Exact task binding for selected layered comic sources and reading pixels.
//! The native runner owns admission, retained custody and guarded source reads.
use crate::{publication::InputReader, *};
use reel_publication::{
    Content, Publication,
    comic_source::{Object, Pin},
    layered_source::{Request as PageRequest, Source},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub page: PageRequest,
    pub authoring: Pin,
    pub selection: Pin,
    pub sheet: u32,
    /// Reusing a reading PNG requires exact reproduction of this selected SVG.
    pub selected_svg: Object,
    pub reading_png: Pin,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub task_key: String,
    pub publication: Artifact,
    pub edition_id: String,
    pub language: String,
    pub page_id: String,
    pub request: Artifact,
    pub inputs: BTreeMap<String, Artifact>,
    pub outputs: ArtifactSet,
    pub local_receipt: Object,
    pub reading_pixels_reused: bool,
    pub human_approval: bool,
    pub retained_owner_acknowledged: bool,
}
pub struct Bound {
    packet: Packet,
    pub request: Request,
    inputs: BTreeMap<String, Artifact>,
    units: BTreeMap<String, review::Unit>,
    #[cfg(feature = "comic")]
    canvas: [u32; 2],
}
fn verified(reader: &mut impl InputReader, a: &Artifact, max: u64) -> Result<Vec<u8>> {
    if a.bytes > max {
        return fail(ErrorKind::Budget, "layered-input-bound");
    }
    let raw = reader.read(a, max)?;
    if raw.len() as u64 != a.bytes || digest(&raw) != a.sha256 {
        return fail(ErrorKind::Integrity, "layered-input-bytes");
    }
    Ok(raw)
}
fn object(pin: &Pin) -> Object {
    Object {
        sha256: pin.sha256.clone(),
        bytes: pin.bytes,
    }
}
fn leaf<'a>(packet: &'a Packet, pin: &Pin) -> Result<&'a Artifact> {
    if !publication::portable_source(&pin.path)
        || pin.path.len() > 249
        || pin.path.split('/').count() > 15
        || pin.path == "request.json"
        || pin.bytes == 0
        || pin.bytes > 256 * 1024 * 1024
    {
        return fail(ErrorKind::Invalid, "layered-source-path-bound");
    }
    let tool = packet.task.tool.as_ref().ok_or(Error {
        kind: ErrorKind::Missing,
        code: "layered-tool".into(),
    })?;
    let id = tool.source_bindings.get(&pin.path).ok_or(Error {
        kind: ErrorKind::Missing,
        code: "layered-source-binding".into(),
    })?;
    let asset = packet
        .task
        .inputs
        .iter()
        .find(|a| &a.logical_id == id)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "layered-source-input".into(),
        })?;
    if asset.sha256 != pin.sha256 || asset.bytes != pin.bytes {
        return fail(ErrorKind::Conflict, "layered-source-pin");
    }
    Ok(asset)
}
pub fn bind(packet: &Packet, reader: &mut impl InputReader) -> Result<Bound> {
    check_packet(packet)?;
    let tool = packet.task.tool.as_ref().ok_or(Error {
        kind: ErrorKind::Missing,
        code: "layered-tool".into(),
    })?;
    if tool.capability != ToolCapability::LayeredComicCompose
        || tool.part.is_some()
        || !tool.auxiliary_executables.is_empty()
    {
        return fail(ErrorKind::Unsupported, "layered-capability");
    }
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
            code: "layered-edition".into(),
        })?;
    if request.schema != "reel.workflow-layered-comic.v1"
        || request.page.schema != "reel.layered-comic-request.v1"
        || request.page.language != edition.language
        || packet.task.language.as_ref() != Some(&edition.language)
    {
        return fail(ErrorKind::Conflict, "layered-edition-language");
    }
    if !(1..=120 * 1024 * 1024).contains(&request.page.input_bytes)
        || !(1..=128 * 1024 * 1024).contains(&request.page.output_bytes)
        || !hex(&request.selected_svg.sha256, &[64])
        || !(1..=128 * 1024 * 1024).contains(&request.selected_svg.bytes)
        || !(24..=16 * 1024 * 1024).contains(&request.reading_png.bytes)
    {
        return fail(ErrorKind::Budget, "layered-managed-budget");
    }
    let Content::Comic { pages, .. } = &edition.content else {
        return fail(ErrorKind::Unsupported, "layered-comic-edition");
    };
    let page = pages
        .iter()
        .find(|p| p.id == request.page.page_id)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "layered-selected-page".into(),
        })?;
    let source_asset = leaf(packet, &request.page.source)?;
    if *source_asset != page.source || source_asset.media_type != "application/json" {
        return fail(ErrorKind::Conflict, "layered-selected-source-identity");
    }
    let source: Source = decode(&verified(reader, source_asset, MAX_CONTROL_BYTES as u64)?)?;
    let selection: serde_json::Value = decode(&verified(
        reader,
        leaf(packet, &request.selection)?,
        MAX_CONTROL_BYTES as u64,
    )?)?;
    if selection["schema"] != "refraction.book-preview.v1"
        || request.sheet == 0
        || request.sheet > 128
    {
        return fail(ErrorKind::Invalid, "layered-selected-book");
    }
    let selected = selection["sheets"]
        .as_array()
        .and_then(|s| s.get(request.sheet as usize - 1))
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "layered-selected-sheet".into(),
        })?;
    if selected["sheet"].as_u64() != Some(request.sheet as u64) {
        return fail(ErrorKind::Conflict, "layered-selected-sheet-order");
    }
    for (field, pin) in [
        ("source", &request.page.source),
        ("authoring", &request.authoring),
        ("png", &request.reading_png),
    ] {
        let declared: reel_publication::layered_source::Asset =
            decode(&serde_json::to_vec(&selected[field]).map_err(|_| Error {
                kind: ErrorKind::Invalid,
                code: "layered-selection-json".into(),
            })?)?;
        if declared.file != pin.path || object(&declared.pin()) != object(pin) {
            return fail(ErrorKind::Conflict, "layered-selected-record-association");
        }
        if selected
            .get(format!("{field}_asset_id"))
            .and_then(|v| v.as_str())
            .is_some_and(|id| {
                id != leaf(packet, pin)
                    .map(|a| a.logical_id.as_str())
                    .unwrap_or("")
            })
        {
            return fail(ErrorKind::Conflict, "layered-selected-native-id");
        }
    }
    let selected_svg: reel_publication::layered_source::Asset =
        decode(&serde_json::to_vec(&selected["svg"]).map_err(|_| Error {
            kind: ErrorKind::Invalid,
            code: "layered-selection-json".into(),
        })?)?;
    if object(&selected_svg.pin()) != request.selected_svg {
        return fail(ErrorKind::Conflict, "layered-selected-svg-association");
    }
    if source.schema != "bbj.layered-page-source.v1"
        || source.layers.is_empty()
        || source.layers.len() > 128
        || request.page.provenance.len() > 32
        || !(1..=4096).contains(&source.canvas[0])
        || !(1..=6144).contains(&source.canvas[1])
    {
        return fail(ErrorKind::Invalid, "layered-source-contract");
    }
    let mut layer_ids = BTreeSet::new();
    if source.layers.iter().any(|l| {
        !token(&l.id)
            || !token(&l.role)
            || !layer_ids.insert(&l.id)
            || !l.x.is_finite()
            || !l.y.is_finite()
            || !l.opacity.is_finite()
            || !(0.0..=1.0).contains(&l.opacity)
            || !l.asset.file.ends_with(".svg")
    }) {
        return fail(ErrorKind::Invalid, "layered-source-placement");
    }
    let mut expected = BTreeMap::new();
    let mut add = |pin: Pin| -> Result<()> {
        leaf(packet, &pin)?;
        if expected
            .insert(pin.path.clone(), object(&pin))
            .is_some_and(|old| old != object(&pin))
        {
            return fail(ErrorKind::Conflict, "layered-source-alias");
        }
        Ok(())
    };
    add(request.page.source.clone())?;
    for layer in &source.layers {
        add(layer.asset.pin())?;
    }
    for pin in &request.page.provenance {
        add(pin.clone())?;
    }
    if !request
        .page
        .provenance
        .iter()
        .any(|p| p.path == request.authoring.path && object(p) == object(&request.authoring))
    {
        return fail(ErrorKind::Missing, "layered-authoring-provenance");
    }
    add(request.authoring.clone())?;
    add(request.selection.clone())?;
    add(request.reading_png.clone())?;
    let mut inputs = BTreeMap::new();
    let mut folded = BTreeSet::new();
    let mut ids = BTreeSet::from([
        tool.publication.logical_id.clone(),
        tool.request.logical_id.clone(),
    ]);
    // A producer may explicitly retain its executable as an upstream input.
    // The worker independently requires these bytes to match its pinned runtime.
    if packet.task.inputs.contains(&tool.executable) {
        ids.insert(tool.executable.logical_id.clone());
    }
    let mut source_total = 0u64;
    for (path, pin) in &expected {
        if !folded.insert(path.to_ascii_lowercase()) {
            return fail(ErrorKind::Conflict, "layered-source-case-alias");
        }
        let a = leaf(
            packet,
            &Pin {
                path: path.clone(),
                sha256: pin.sha256.clone(),
                bytes: pin.bytes,
            },
        )?;
        ids.insert(a.logical_id.clone());
        inputs.insert(path.clone(), a.clone());
        if path != &request.reading_png.path && path != &request.selection.path {
            source_total = source_total.checked_add(pin.bytes).ok_or(Error {
                kind: ErrorKind::Budget,
                code: "layered-input-overflow".into(),
            })?;
        }
    }
    if source_total > request.page.input_bytes
        || inputs.len() != tool.source_bindings.len()
        || ids.len() != packet.task.inputs.len()
        || packet
            .task
            .inputs
            .iter()
            .any(|a| !ids.contains(&a.logical_id))
    {
        return fail(ErrorKind::Conflict, "layered-complete-input-set");
    }
    // Reserve the actual authored request plus bounded carrier/receipt indices.
    // Source paths and artifact identities have already passed their bounds.
    let request_json_bytes = serde_json::to_vec_pretty(&request.page)
        .map_err(|_| Error {
            kind: ErrorKind::Invalid,
            code: "layered-request-json".into(),
        })?
        .len() as u64;
    let metadata_upper = request_json_bytes * 2 + (expected.len() as u64 + 8) * 4096;
    let output_upper = source_total
        + request.selection.bytes
        + request.selected_svg.bytes
        + request.reading_png.bytes
        + metadata_upper;
    if output_upper > request.page.output_bytes
        || packet
            .task
            .outputs
            .get("page.svg")
            .is_some_and(|c| c.max_bytes < request.selected_svg.bytes)
        || packet
            .task
            .outputs
            .get("page.png")
            .is_some_and(|c| c.max_bytes < request.reading_png.bytes)
    {
        return fail(ErrorKind::Budget, "layered-managed-output-reservation");
    }
    let total = packet
        .task
        .inputs
        .iter()
        .try_fold(0u64, |n, a| n.checked_add(a.bytes))
        .ok_or(Error {
            kind: ErrorKind::Budget,
            code: "layered-input-overflow".into(),
        })?;
    if total > 256 * 1024 * 1024 {
        return fail(ErrorKind::Budget, "layered-task-input-budget");
    }
    if leaf(packet, &request.reading_png)?.media_type != "image/png" {
        return fail(ErrorKind::Invalid, "layered-reading-media");
    }
    let author: serde_json::Value = decode(&verified(
        reader,
        leaf(packet, &request.authoring)?,
        MAX_CONTROL_BYTES as u64,
    )?)?;
    let panels: Vec<String> = if author["schema"] == "refraction.native-scene-authoring.v1" {
        decode(
            &serde_json::to_vec(&author["page"]["reading_order"]).map_err(|_| Error {
                kind: ErrorKind::Invalid,
                code: "layered-authoring-json".into(),
            })?,
        )?
    } else if author["schema"] == "refraction.native-matter-authoring.v1" {
        vec![]
    } else {
        return fail(ErrorKind::Unsupported, "layered-authoring-schema");
    };
    let dialogues = author
        .get("dialogue")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let utterances: Vec<_> = dialogues
        .iter()
        .map(|v| {
            v["id"].as_str().map(str::to_owned).ok_or(Error {
                kind: ErrorKind::Invalid,
                code: "layered-dialogue-id".into(),
            })
        })
        .collect::<Result<_>>()?;
    if panels != page.panel_order || utterances != page.utterance_order {
        return fail(ErrorKind::Conflict, "layered-review-source-order");
    }
    let mut units = BTreeMap::new();
    let mut unit = |panel_id: Option<String>, balloon_id: Option<String>| -> Result<()> {
        let u = review::Unit::Comic {
            page_id: page.id.clone(),
            panel_id,
            balloon_id,
        };
        let key = review::unit_key(&u)?;
        if units.insert(key, u).is_some() {
            return fail(ErrorKind::Conflict, "layered-review-unit");
        }
        Ok(())
    };
    unit(None, None)?;
    for id in &panels {
        unit(Some(id.clone()), None)?;
    }
    for dialogue in &dialogues {
        let panel = dialogue["panel"].as_str().ok_or(Error {
            kind: ErrorKind::Missing,
            code: "layered-dialogue-panel".into(),
        })?;
        if !panels.iter().any(|id| id == panel) {
            return fail(ErrorKind::Conflict, "layered-dialogue-panel");
        }
        unit(
            Some(panel.into()),
            Some(dialogue["id"].as_str().unwrap().into()),
        )?;
    }
    let types = BTreeMap::from([
        ("page.svg", "image/svg+xml"),
        ("page.png", "image/png"),
        ("editable.packet", "application/vnd.reel.editable-packet.v1"),
    ]);
    if packet.task.outputs.len() != types.len()
        || types.iter().any(|(name, media)| {
            packet.task.outputs.get(*name).is_none_or(|c| {
                !c.required || c.media_type != *media || c.max_bytes > request.page.output_bytes
            })
        })
    {
        return fail(ErrorKind::Conflict, "layered-output-contract");
    }
    Ok(Bound {
        packet: packet.clone(),
        request,
        inputs,
        units,
        #[cfg(feature = "comic")]
        canvas: source.canvas,
    })
}
impl Bound {
    pub fn output_bytes(&self) -> u64 {
        self.request.page.output_bytes
    }
    pub fn check_receipt(&self, result: &ReturnedResult, raw: &[u8]) -> Result<()> {
        check_result(&self.packet, result)?;
        let receipt: Receipt = decode(raw)?;
        if result.artifacts["page.svg"].sha256 != self.request.selected_svg.sha256
            || result.artifacts["page.svg"].bytes != self.request.selected_svg.bytes
            || result.artifacts["page.png"].sha256 != self.request.reading_png.sha256
            || result.artifacts["page.png"].bytes != self.request.reading_png.bytes
        {
            return fail(ErrorKind::Conflict, "layered-selected-output-bytes");
        }
        let tool = self.packet.task.tool.as_ref().unwrap();
        let pin = result.worker_receipt.as_ref().ok_or(Error {
            kind: ErrorKind::Missing,
            code: "layered-worker-receipt".into(),
        })?;
        if pin.sha256 != digest(raw)
            || pin.bytes != raw.len() as u64
            || receipt.schema != "reel.workflow-layered-comic-receipt.v1"
            || receipt.task_key != self.packet.task_key
            || receipt.publication != tool.publication
            || receipt.edition_id != tool.edition_id
            || receipt.language != self.request.page.language
            || receipt.page_id != self.request.page.page_id
            || receipt.request != tool.request
            || receipt.inputs != self.inputs
            || receipt.outputs != result.artifacts
            || !receipt.reading_pixels_reused
            || receipt.human_approval
            || receipt.retained_owner_acknowledged
        {
            return fail(ErrorKind::Conflict, "layered-receipt-correspondence");
        }
        for (name, a) in &result.artifacts {
            if a.logical_id != format!("workflow/{}/{name}", self.packet.task_key) {
                return fail(ErrorKind::Conflict, "layered-output-identity");
            }
        }
        Ok(())
    }
    pub fn review_copy(&self, result: &ReturnedResult, receipt: &[u8]) -> Result<review::Export> {
        self.check_receipt(result, receipt)?;
        let tool = self.packet.task.tool.as_ref().unwrap();
        let page = self.request.page.page_id.clone();
        let manifest=serde_json::to_vec(&serde_json::json!({"schema":"reel.workflow-comic-review.v1","task_key":self.packet.task_key,"edition_id":tool.edition_id,"language":self.request.page.language,"page_order":[page],"reading":{page.clone():result.artifacts["page.png"]}})).map_err(|_|Error{kind:ErrorKind::Invalid,code:"layered-review-json".into()})?;
        let copy = review::Copy {
            schema: "reel.workflow-review-copy.v1".into(),
            project: self.packet.project.clone(),
            source_commit: self.packet.source_commit.clone(),
            language: self.request.page.language.clone(),
            publication: tool.publication.clone(),
            review: Artifact {
                logical_id: format!("workflow/review/{}/reading", self.packet.task_key),
                sha256: digest(&manifest),
                bytes: manifest.len() as u64,
                media_type: "application/json".into(),
            },
            format: review::Format::Comic {
                page_order: vec![self.request.page.page_id.clone()],
            },
            units: self.units.clone(),
        };
        review::check_copy(&copy)?;
        Ok(review::Export {
            copy,
            manifest: Some(manifest),
        })
    }
}

#[cfg(feature = "comic")]
pub struct Generated {
    pub bound: Bound,
    pub outputs: ArtifactSet,
    pub files: BTreeMap<String, Vec<u8>>,
}
#[cfg(feature = "comic")]
pub fn generate(packet: &Packet, reader: &mut impl InputReader) -> Result<Generated> {
    let bound = bind(packet, reader)?;
    let mut files = reel_publication::layered::compose_with(&bound.request.page, |pin| {
        Ok(verified(reader, leaf(packet, pin)?, pin.bytes)?)
    })
    .map_err(|_| Error {
        kind: ErrorKind::Integrity,
        code: "layered-production-replay".into(),
    })?;
    if reel_publication::comic::object(&files["page.svg"]) != bound.request.selected_svg {
        return fail(ErrorKind::Conflict, "layered-selected-svg-replay");
    }
    let svg = files.remove("page.svg").unwrap();
    let local_receipt = reel_publication::comic::object(&files["receipt.json"]);
    let selection = verified(
        reader,
        leaf(packet, &bound.request.selection)?,
        MAX_CONTROL_BYTES as u64,
    )?;
    let selection_path = format!("sources/{}", bound.request.selection.path);
    if files
        .get(&selection_path)
        .is_some_and(|raw| raw != &selection)
    {
        return fail(ErrorKind::Conflict, "layered-selection-archive-alias");
    }
    files.entry(selection_path).or_insert(selection);
    let bundle = reel_publication::packet::encode(&files).map_err(|_| Error {
        kind: ErrorKind::Budget,
        code: "layered-editable-packet-bound".into(),
    })?;
    let png = verified(
        reader,
        leaf(packet, &bound.request.reading_png)?,
        bound.request.reading_png.bytes,
    )?;
    reel_publication::layered::check_reading_png(&png).map_err(|_| Error {
        kind: ErrorKind::Integrity,
        code: "layered-reading-png".into(),
    })?;
    let actual_canvas = [
        u32::from_be_bytes(png[16..20].try_into().unwrap()),
        u32::from_be_bytes(png[20..24].try_into().unwrap()),
    ];
    if actual_canvas != bound.canvas {
        return fail(ErrorKind::Conflict, "layered-reading-canvas");
    }
    let files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("page.svg".into(), svg),
        ("page.png".into(), png),
        ("editable.packet".into(), bundle),
    ]);
    let outputs: ArtifactSet = files
        .iter()
        .map(|(name, raw)| {
            (
                name.clone(),
                Artifact {
                    logical_id: format!("workflow/{}/{name}", packet.task_key),
                    sha256: digest(raw),
                    bytes: raw.len() as u64,
                    media_type: packet.task.outputs[name].media_type.clone(),
                },
            )
        })
        .collect();
    reel_publication::check_outputs(&packet.task.outputs, &outputs)?;
    let tool = packet.task.tool.as_ref().unwrap();
    let receipt = Receipt {
        schema: "reel.workflow-layered-comic-receipt.v1".into(),
        task_key: packet.task_key.clone(),
        publication: tool.publication.clone(),
        edition_id: tool.edition_id.clone(),
        language: bound.request.page.language.clone(),
        page_id: bound.request.page.page_id.clone(),
        request: tool.request.clone(),
        inputs: bound.inputs.clone(),
        outputs: outputs.clone(),
        local_receipt,
        reading_pixels_reused: true,
        human_approval: false,
        retained_owner_acknowledged: false,
    };
    let raw = serde_json::to_vec(&receipt).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "layered-receipt-json".into(),
    })?;
    if raw.len() > MAX_CONTROL_BYTES
        || files.values().map(|b| b.len() as u64).sum::<u64>() + raw.len() as u64
            > bound.output_bytes()
    {
        return fail(ErrorKind::Budget, "layered-output-byte-bound");
    }
    let mut files = files;
    files.insert("receipt.json".into(), raw);
    Ok(Generated {
        bound,
        outputs,
        files,
    })
}
