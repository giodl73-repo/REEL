//! Complete selected-image reading editions. Captured source definitions and
//! authoring remain separate from editable layer restoration and press approval.
use crate::{publication::InputReader, *};
use reel_publication::{
    ComicPage, Content, Publication,
    comic_source::Pin,
    layered_source::{Asset, Source},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub id: String,
    pub sheet: u32,
    pub source: Pin,
    pub authoring: Pin,
    pub png: Pin,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub selection: Pin,
    pub pages: Vec<Page>,
    pub input_bytes: u64,
    pub output_bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub task_key: String,
    pub publication: Artifact,
    pub edition_id: String,
    pub language: String,
    pub inputs: BTreeMap<String, Artifact>,
    pub outputs: ArtifactSet,
    pub page_order: Vec<String>,
    pub reading_pixels_reused: bool,
    pub editable_layers_restored: bool,
    pub human_approval: bool,
    pub retained_owner_acknowledged: bool,
}
pub struct Bound {
    packet: Packet,
    request: Request,
    edition: String,
    language: String,
    units: BTreeMap<String, review::Unit>,
    reading: BTreeMap<String, Artifact>,
    #[cfg(feature = "comic")]
    canvas: BTreeMap<String, [u32; 2]>,
}
fn bytes(reader: &mut impl InputReader, a: &Artifact, max: u64) -> Result<Vec<u8>> {
    if a.bytes > max {
        return fail(ErrorKind::Budget, "reading-issue-input-bytes");
    }
    let raw = reader.read(a, max)?;
    if raw.len() as u64 != a.bytes || digest(&raw) != a.sha256 {
        return fail(ErrorKind::Integrity, "reading-issue-input-pin");
    }
    Ok(raw)
}
fn leaf<'a>(packet: &'a Packet, pin: &Pin) -> Result<&'a Artifact> {
    let tool = packet.task.tool.as_ref().ok_or(Error {
        kind: ErrorKind::Missing,
        code: "reading-issue-tool".into(),
    })?;
    let id = tool.source_bindings.get(&pin.path).ok_or(Error {
        kind: ErrorKind::Missing,
        code: "reading-issue-source-binding".into(),
    })?;
    let a = packet
        .task
        .inputs
        .iter()
        .find(|a| &a.logical_id == id)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "reading-issue-source-input".into(),
        })?;
    if a.sha256 != pin.sha256 || a.bytes != pin.bytes || !publication::portable_source(&pin.path) {
        return fail(ErrorKind::Conflict, "reading-issue-source-pin");
    }
    Ok(a)
}
fn units(page: &ComicPage, author: &serde_json::Value) -> Result<BTreeMap<String, review::Unit>> {
    let panels: Vec<String> = if author["schema"] == "refraction.native-scene-authoring.v1" {
        decode(
            &serde_json::to_vec(&author["page"]["reading_order"]).map_err(|_| Error {
                kind: ErrorKind::Invalid,
                code: "reading-issue-authoring".into(),
            })?,
        )?
    } else if author["schema"] == "refraction.native-matter-authoring.v1" {
        vec![]
    } else {
        return fail(ErrorKind::Unsupported, "reading-issue-authoring-schema");
    };
    let dialogue = author
        .get("dialogue")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let ids: Vec<String> = dialogue
        .iter()
        .map(|v| {
            v["id"].as_str().map(String::from).ok_or(Error {
                kind: ErrorKind::Invalid,
                code: "reading-issue-dialogue-id".into(),
            })
        })
        .collect::<Result<_>>()?;
    if panels != page.panel_order || ids != page.utterance_order {
        return fail(ErrorKind::Conflict, "reading-issue-review-order");
    }
    let mut out = BTreeMap::new();
    let mut add = |panel: Option<String>, balloon: Option<String>| -> Result<()> {
        let u = review::Unit::Comic {
            page_id: page.id.clone(),
            panel_id: panel,
            balloon_id: balloon,
        };
        if out.insert(review::unit_key(&u)?, u).is_some() {
            return fail(ErrorKind::Conflict, "reading-issue-review-unit");
        }
        Ok(())
    };
    add(None, None)?;
    for p in &panels {
        add(Some(p.clone()), None)?;
    }
    for d in dialogue {
        let p = d["panel"]
            .as_str()
            .filter(|p| panels.iter().any(|id| id == p))
            .ok_or(Error {
                kind: ErrorKind::Conflict,
                code: "reading-issue-dialogue-panel".into(),
            })?;
        add(Some(p.into()), Some(d["id"].as_str().unwrap().into()))?;
    }
    Ok(out)
}
pub fn bind(packet: &Packet, reader: &mut impl InputReader) -> Result<Bound> {
    check_packet(packet)?;
    let tool = packet.task.tool.as_ref().ok_or(Error {
        kind: ErrorKind::Missing,
        code: "reading-issue-tool".into(),
    })?;
    if tool.capability != ToolCapability::ComicReadingAssembly
        || tool.part.is_some()
        || !tool.auxiliary_executables.is_empty()
    {
        return fail(ErrorKind::Unsupported, "reading-issue-capability");
    }
    let publication: Publication =
        decode(&bytes(reader, &tool.publication, MAX_CONTROL_BYTES as u64)?)?;
    reel_publication::check_publication(&publication)?;
    let request: Request = decode(&bytes(reader, &tool.request, MAX_CONTROL_BYTES as u64)?)?;
    if request.schema != "reel.workflow-reading-issue.v1"
        || !(1..=256 * 1024 * 1024).contains(&request.input_bytes)
        || !(1..=256 * 1024 * 1024).contains(&request.output_bytes)
    {
        return fail(ErrorKind::Budget, "reading-issue-request-bound");
    }
    let edition = publication
        .editions
        .iter()
        .find(|e| e.id == tool.edition_id)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "reading-issue-edition".into(),
        })?;
    if packet.task.language.as_ref() != Some(&edition.language) {
        return fail(ErrorKind::Conflict, "reading-issue-language");
    }
    let Content::Comic { pages, .. } = &edition.content else {
        return fail(ErrorKind::Unsupported, "reading-issue-comic-required");
    };
    let selection: serde_json::Value = decode(&bytes(
        reader,
        leaf(packet, &request.selection)?,
        MAX_CONTROL_BYTES as u64,
    )?)?;
    let sheets = selection["sheets"].as_array().ok_or(Error {
        kind: ErrorKind::Invalid,
        code: "reading-issue-selection".into(),
    })?;
    if selection["schema"] != "refraction.book-preview.v1"
        || request.pages.len() != pages.len()
        || sheets.len() != pages.len()
        || pages.len() > 128
    {
        return fail(ErrorKind::Conflict, "reading-issue-complete-book-coverage");
    }
    let mut all_units = BTreeMap::new();
    let mut reading = BTreeMap::new();
    let mut canvas = BTreeMap::new();
    let mut paths = BTreeSet::from([request.selection.path.clone()]);
    for (index, ((spec, page), selected)) in request.pages.iter().zip(pages).zip(sheets).enumerate()
    {
        if spec.id != page.id
            || spec.sheet != index as u32 + 1
            || selected["sheet"].as_u64() != Some(spec.sheet as u64)
            || leaf(packet, &spec.source)? != &page.source
        {
            return fail(ErrorKind::Conflict, "reading-issue-page-order-source");
        }
        for (field, pin) in [
            ("source", &spec.source),
            ("authoring", &spec.authoring),
            ("png", &spec.png),
        ] {
            if !paths.insert(pin.path.clone()) {
                return fail(ErrorKind::Conflict, "reading-issue-source-alias");
            }
            let declared: Asset =
                serde_json::from_value(selected[field].clone()).map_err(|_| Error {
                    kind: ErrorKind::Invalid,
                    code: "reading-issue-selected-record".into(),
                })?;
            let a = leaf(packet, pin)?;
            if declared.file != pin.path
                || declared.sha256 != pin.sha256
                || declared.bytes != pin.bytes
                || selected
                    .get(format!("{field}_asset_id"))
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id != a.logical_id)
            {
                return fail(
                    ErrorKind::Conflict,
                    "reading-issue-selected-record-association",
                );
            }
        }
        let source: Source = decode(&bytes(
            reader,
            leaf(packet, &spec.source)?,
            MAX_CONTROL_BYTES as u64,
        )?)?;
        if source.schema != "bbj.layered-page-source.v1"
            || !(1..=4096).contains(&source.canvas[0])
            || !(1..=6144).contains(&source.canvas[1])
            || u64::from(source.canvas[0]) * u64::from(source.canvas[1]) > 16 * 1024 * 1024
        {
            return fail(ErrorKind::Budget, "reading-issue-source-canvas");
        }
        let author: serde_json::Value = decode(&bytes(
            reader,
            leaf(packet, &spec.authoring)?,
            MAX_CONTROL_BYTES as u64,
        )?)?;
        for (key, u) in units(page, &author)? {
            if all_units.insert(key, u).is_some() {
                return fail(ErrorKind::Conflict, "reading-issue-unit-alias");
            }
        }
        let png = leaf(packet, &spec.png)?;
        if png.media_type != "image/png" || !(24..=16 * 1024 * 1024).contains(&png.bytes) {
            return fail(ErrorKind::Budget, "reading-issue-png-bound");
        }
        reading.insert(page.id.clone(), png.clone());
        canvas.insert(page.id.clone(), source.canvas);
    }
    if paths.iter().ne(tool.source_bindings.keys()) {
        return fail(ErrorKind::Conflict, "reading-issue-source-set");
    }
    let mut required: BTreeSet<_> = tool.source_bindings.values().cloned().collect();
    required.extend([
        tool.publication.logical_id.clone(),
        tool.request.logical_id.clone(),
    ]);
    if packet.task.inputs.iter().any(|a| a == &tool.executable) {
        required.insert(tool.executable.logical_id.clone());
    }
    if required
        != packet
            .task
            .inputs
            .iter()
            .map(|a| a.logical_id.clone())
            .collect()
    {
        return fail(ErrorKind::Conflict, "reading-issue-input-set");
    }
    let total = packet
        .task
        .inputs
        .iter()
        .try_fold(0u64, |n, a| n.checked_add(a.bytes))
        .ok_or(Error {
            kind: ErrorKind::Budget,
            code: "reading-issue-input-overflow".into(),
        })?;
    if total > request.input_bytes {
        return fail(ErrorKind::Budget, "reading-issue-total-inputs");
    }
    let expected = [
        ("reading.pdf", "application/pdf"),
        ("reading-manifest.json", "application/json"),
        ("source-index.json", "application/json"),
    ];
    if packet.task.outputs.len() != expected.len()
        || expected.iter().any(|(n, m)| {
            packet.task.outputs.get(*n).is_none_or(|c| {
                !c.required || c.media_type != *m || c.max_bytes > request.output_bytes
            })
        })
    {
        return fail(ErrorKind::Conflict, "reading-issue-output-contract");
    }
    Ok(Bound {
        packet: packet.clone(),
        request,
        edition: edition.id.clone(),
        language: edition.language.clone(),
        units: all_units,
        reading,
        #[cfg(feature = "comic")]
        canvas,
    })
}
impl Bound {
    pub fn output_bytes(&self) -> u64 {
        self.request.output_bytes
    }
    fn manifest(&self) -> Result<Vec<u8>> {
        let raw=serde_json::to_vec(&serde_json::json!({"schema":"reel.workflow-comic-review.v1","task_key":self.packet.task_key,"edition_id":self.edition,"language":self.language,"page_order":self.request.pages.iter().map(|p|&p.id).collect::<Vec<_>>(),"reading":self.reading})).map_err(|_|Error{kind:ErrorKind::Invalid,code:"reading-issue-manifest".into()})?;
        if raw.len() > MAX_CONTROL_BYTES {
            return fail(ErrorKind::Budget, "reading-issue-manifest-bound");
        }
        Ok(raw)
    }
    pub fn check_receipt(&self, result: &ReturnedResult, raw: &[u8]) -> Result<()> {
        check_result(&self.packet, result)?;
        let r: Receipt = decode(raw)?;
        let tool = self.packet.task.tool.as_ref().unwrap();
        if r.schema != "reel.workflow-reading-issue-receipt.v1"
            || r.task_key != self.packet.task_key
            || r.publication != tool.publication
            || r.edition_id != self.edition
            || r.language != self.language
            || r.inputs
                != self
                    .packet
                    .task
                    .inputs
                    .iter()
                    .map(|a| (a.logical_id.clone(), a.clone()))
                    .collect()
            || r.outputs != result.artifacts
            || r.page_order
                != self
                    .request
                    .pages
                    .iter()
                    .map(|p| p.id.clone())
                    .collect::<Vec<_>>()
            || !r.reading_pixels_reused
            || r.editable_layers_restored
            || r.human_approval
            || r.retained_owner_acknowledged
        {
            return fail(ErrorKind::Conflict, "reading-issue-receipt");
        }
        Ok(())
    }
    pub fn review_copy(&self, result: &ReturnedResult, raw: &[u8]) -> Result<review::Export> {
        self.check_receipt(result, raw)?;
        let manifest = self.manifest()?;
        let a = &result.artifacts["reading-manifest.json"];
        if a.sha256 != digest(&manifest) || a.bytes != manifest.len() as u64 {
            return fail(ErrorKind::Integrity, "reading-issue-manifest-pin");
        }
        let copy = review::Copy {
            schema: "reel.workflow-review-copy.v1".into(),
            project: self.packet.project.clone(),
            source_commit: self.packet.source_commit.clone(),
            language: self.language.clone(),
            publication: self.packet.task.tool.as_ref().unwrap().publication.clone(),
            review: a.clone(),
            format: review::Format::Comic {
                page_order: self.request.pages.iter().map(|p| p.id.clone()).collect(),
            },
            units: self.units.clone(),
        };
        review::check_copy(&copy)?;
        review::comic_reading_inputs(&copy, &manifest)?;
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
    let pdf = reel_publication::reading::pdf(
        bound.request.pages.len(),
        bound.request.input_bytes,
        bound.request.output_bytes,
        |index| {
            let p = &bound.request.pages[index];
            let a = &bound.reading[&p.id];
            let raw = bytes(reader, a, 16 * 1024 * 1024)?;
            reel_publication::layered::check_reading_png(&raw)?;
            let canvas = bound.canvas[&p.id];
            if u32::from_be_bytes(raw[16..20].try_into()?) != canvas[0]
                || u32::from_be_bytes(raw[20..24].try_into()?) != canvas[1]
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "selected reading canvas differs",
                )
                .into());
            }
            Ok(raw)
        },
    )
    .map_err(|_| Error {
        kind: ErrorKind::Integrity,
        code: "reading-issue-pdf-replay".into(),
    })?;
    let index=serde_json::to_vec(&serde_json::json!({"schema":"reel.reading-source-index.v1","publication":packet.task.tool.as_ref().unwrap().publication,"edition_id":bound.edition,"selection":bound.request.selection,"pages":bound.request.pages,"inputs":packet.task.inputs,"editable_layers_restored":false,"print_ready":false,"human_approval":false})).map_err(|_|Error{kind:ErrorKind::Invalid,code:"reading-issue-index".into()})?;
    if index.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "reading-issue-source-index-bound");
    }
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("reading.pdf".into(), pdf),
        ("reading-manifest.json".into(), bound.manifest()?),
        ("source-index.json".into(), index),
    ]);
    let outputs: ArtifactSet = files
        .iter()
        .map(|(n, b)| {
            (
                n.clone(),
                Artifact {
                    logical_id: format!("workflow/{}/{n}", packet.task_key),
                    sha256: digest(b),
                    bytes: b.len() as u64,
                    media_type: packet.task.outputs[n].media_type.clone(),
                },
            )
        })
        .collect();
    reel_publication::check_outputs(&packet.task.outputs, &outputs)?;
    let receipt = Receipt {
        schema: "reel.workflow-reading-issue-receipt.v1".into(),
        task_key: packet.task_key.clone(),
        publication: packet.task.tool.as_ref().unwrap().publication.clone(),
        edition_id: bound.edition.clone(),
        language: bound.language.clone(),
        inputs: packet
            .task
            .inputs
            .iter()
            .map(|a| (a.logical_id.clone(), a.clone()))
            .collect(),
        outputs: outputs.clone(),
        page_order: bound.request.pages.iter().map(|p| p.id.clone()).collect(),
        reading_pixels_reused: true,
        editable_layers_restored: false,
        human_approval: false,
        retained_owner_acknowledged: false,
    };
    let raw = serde_json::to_vec(&receipt).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "reading-issue-receipt-json".into(),
    })?;
    files.insert("receipt.json".into(), raw);
    if files
        .values()
        .try_fold(0u64, |n, b| n.checked_add(b.len() as u64))
        .is_none_or(|n| n > bound.request.output_bytes)
    {
        return fail(ErrorKind::Budget, "reading-issue-total-output");
    }
    Ok(Generated {
        bound,
        outputs,
        files,
    })
}
