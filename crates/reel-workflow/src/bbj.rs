//! BBJ page intake and late-bound composition planning. No native effects.
use crate::{expansion, upstream, *};
use reel_publication::{Content, Publication, comic_source::Pin, layered_source};
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub project: String,
    pub source_commit: String,
    /// Distinct authored task prefix; never generated as an uncertainty retry.
    pub task_prefix: String,
    pub attempt_id: String,
    /// Exact qualified native runtime to be used by the eventual worker.
    pub executable: Pin,
    /// Exact native declaration for the empty-input intake source profile.
    pub native_inputs: Artifact,
}

fn invalid<T>(code: &str) -> Result<T> {
    fail(ErrorKind::Invalid, code)
}
fn named_control<T: serde::de::DeserializeOwned>(raw: &[u8], name: &str) -> Result<T> {
    decode(raw).map_err(|e| Error {
        kind: e.kind,
        code: format!("bbj-planning-{name}-control"),
    })
}
fn json(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw = serde_json::to_vec_pretty(value).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "bbj-planning-json".into(),
    })?;
    if raw.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "bbj-planning-control-bytes");
    }
    Ok(raw)
}
fn parent(task: &str, name: &str) -> expansion::Input {
    expansion::Input::Parent {
        reference: upstream::Reference {
            task_id: task.into(),
            artifact: upstream::Selector::Output { name: name.into() },
        },
    }
}
struct Capture {
    incoming: BTreeMap<String, Vec<u8>>,
    artifacts: ArtifactSet,
    sources: BTreeMap<String, String>,
    namespace: String,
    bytes: u64,
}
impl Capture {
    fn add(
        &mut self,
        name: &str,
        raw: Vec<u8>,
        media: &str,
        source: Option<&Pin>,
    ) -> Result<Artifact> {
        if self.incoming.contains_key(name) || self.incoming.len() >= 120 {
            return fail(ErrorKind::Conflict, "bbj-planning-output-name");
        }
        let a = Artifact {
            logical_id: format!("{}{name}", self.namespace),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: media.into(),
        };
        artifact(&a)?;
        if let Some(pin) = source {
            if pin.sha256 != a.sha256
                || pin.bytes != a.bytes
                || !publication::portable_source(&pin.path)
            {
                return fail(ErrorKind::Integrity, "bbj-planning-source-pin");
            }
            if self.sources.insert(pin.path.clone(), name.into()).is_some() {
                return fail(ErrorKind::Conflict, "bbj-planning-source-alias");
            }
        }
        self.bytes = self.bytes.checked_add(a.bytes).ok_or(Error {
            kind: ErrorKind::Budget,
            code: "bbj-planning-input-overflow".into(),
        })?;
        if self.bytes > 128 * 1024 * 1024 {
            return fail(ErrorKind::Budget, "bbj-planning-intake-bytes");
        }
        self.artifacts.insert(name.into(), a.clone());
        self.incoming.insert(name.into(), raw);
        Ok(a)
    }
}
struct Memory<'a>(&'a Capture);
impl publication::InputReader for Memory<'_> {
    fn read(&mut self, a: &Artifact, max: u64) -> Result<Vec<u8>> {
        let name = self
            .0
            .artifacts
            .iter()
            .find(|(_, v)| *v == a)
            .map(|(k, _)| k)
            .ok_or(Error {
                kind: ErrorKind::Missing,
                code: "bbj-planning-input".into(),
            })?;
        let raw = &self.0.incoming[name];
        if raw.len() as u64 > max {
            return fail(ErrorKind::Budget, "bbj-planning-read-bytes");
        }
        Ok(raw.clone())
    }
}

/// Capture a complete bounded page capsule before any native admission. The
/// caller must keep read guards live until all returned files are written.
/// Result files are planning/return declarations, never retained completions.
pub fn prepare(
    config: &Config,
    prepared: &Path,
    sheet: u32,
    output: &Path,
    mut read: impl FnMut(&Path, u64) -> Result<Vec<u8>>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    if config.schema != "reel.workflow-bbj-page-plan.v1"
        || !token(&config.task_prefix)
        || config.task_prefix.len() > 110
        || !token(&config.project)
        || !hex(&config.source_commit, &[40, 64])
        || config.source_commit.bytes().all(|b| b == b'0')
        || !token(&config.attempt_id)
        || !(1..=128).contains(&sheet)
        || !output.is_absolute()
        || !Path::new(&config.executable.path).is_absolute()
        || !(1..=128 * 1024 * 1024).contains(&config.executable.bytes)
        || !hex(&config.executable.sha256, &[64])
    {
        return invalid("bbj-planning-configuration");
    }
    artifact(&config.native_inputs)?;
    if config.native_inputs.media_type != "application/json"
        || config.native_inputs.bytes > MAX_CONTROL_BYTES as u64
    {
        return invalid("bbj-planning-native-declaration");
    }
    let control = |p: &Path, read: &mut dyn FnMut(&Path, u64) -> Result<Vec<u8>>| {
        read(p, MAX_CONTROL_BYTES as u64)
    };
    let publication_raw = control(&prepared.join("publication.json"), &mut read)?;
    let publication: Publication = named_control(&publication_raw, "publication")?;
    reel_publication::check_publication(&publication)?;
    let book: serde_json::Value =
        named_control(&control(&prepared.join("book.json"), &mut read)?, "book")?;
    if book["schema"] != "reel.bbj-book-import.v1" || book["publication_id"] != publication.id {
        return invalid("bbj-planning-prepared-book");
    }
    let folder = format!("sheet-{sheet:03}");
    let row = book["pages"]
        .as_array()
        .and_then(|p| p.get(sheet as usize - 1))
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "bbj-planning-sheet".into(),
        })?;
    if row["sheet"].as_u64() != Some(sheet as u64)
        || row["request"] != format!("{folder}/request.json")
        || row["bindings"] != format!("{folder}/bindings.json")
    {
        return invalid("bbj-planning-sheet-control-path");
    }
    let mut page: layered_source::Request = decode(&control(
        &prepared.join(&folder).join("request.json"),
        &mut read,
    )?)?;
    let bindings: BTreeMap<String, std::path::PathBuf> = decode(&control(
        &prepared.join(&folder).join("bindings.json"),
        &mut read,
    )?)?;
    let readings: BTreeMap<String, std::path::PathBuf> = decode(&control(
        &prepared.join("reading-bindings.json"),
        &mut read,
    )?)?;
    let edition = publication
        .editions
        .iter()
        .find(|e| e.language == page.language)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "bbj-planning-edition".into(),
        })?;
    let Content::Comic { pages, .. } = &edition.content else {
        return invalid("bbj-planning-comic");
    };
    let declared = pages
        .get(sheet as usize - 1)
        .filter(|p| p.id == page.page_id)
        .ok_or(Error {
            kind: ErrorKind::Conflict,
            code: "bbj-planning-page-order".into(),
        })?;
    let namespace = declared
        .source
        .logical_id
        .strip_suffix("source.json")
        .ok_or(Error {
            kind: ErrorKind::Invalid,
            code: "bbj-planning-source-namespace".into(),
        })?
        .to_owned();
    let mut capture = Capture {
        incoming: BTreeMap::new(),
        artifacts: BTreeMap::new(),
        sources: BTreeMap::new(),
        namespace,
        bytes: 0,
    };
    let mut capture_pin =
        |capture: &mut Capture, name: &str, pin: &Pin, media: &str| -> Result<Artifact> {
            if !(1..=128 * 1024 * 1024).contains(&pin.bytes) {
                return fail(ErrorKind::Budget, "bbj-planning-source-bytes");
            }
            let path = bindings
                .get(&pin.path)
                .filter(|p| p.is_absolute())
                .ok_or(Error {
                    kind: ErrorKind::Missing,
                    code: "bbj-planning-source-binding".into(),
                })?;
            capture.add(name, read(path, pin.bytes)?, media, Some(pin))
        };
    let source = capture_pin(
        &mut capture,
        "source.json",
        &page.source,
        "application/json",
    )?;
    if source != declared.source {
        return fail(ErrorKind::Conflict, "bbj-planning-source-identity");
    }
    let layers: layered_source::Source = decode(&capture.incoming["source.json"])?;
    for (i, layer) in layers.layers.iter().enumerate() {
        capture_pin(
            &mut capture,
            &format!("layer-{:03}.svg", i + 1),
            &layer.asset.pin(),
            "image/svg+xml",
        )?;
    }
    for (i, pin) in page.provenance.iter().enumerate() {
        let media = match Path::new(&pin.path).extension().and_then(|s| s.to_str()) {
            Some("json") => "application/json",
            Some("ttf") => "font/ttf",
            _ => "application/octet-stream",
        };
        capture_pin(
            &mut capture,
            &format!("provenance-{:03}.bin", i + 1),
            pin,
            media,
        )?;
    }
    if capture.sources.keys().ne(bindings.keys()) {
        return fail(ErrorKind::Conflict, "bbj-planning-binding-closure");
    }
    let selection_raw = control(&prepared.join("selected-book.json"), &mut read)?;
    if book["selection"]["sha256"].as_str() != Some(digest(&selection_raw).as_str())
        || book["selection"]["bytes"].as_u64() != Some(selection_raw.len() as u64)
    {
        return fail(ErrorKind::Integrity, "bbj-planning-selected-book-pin");
    }
    let selection: serde_json::Value = decode(&selection_raw)?;
    let selected = selection["sheets"]
        .as_array()
        .and_then(|s| s.get(sheet as usize - 1))
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "bbj-planning-selected-sheet".into(),
        })?;
    let author: layered_source::Asset = serde_json::from_value(selected["authoring"].clone())
        .map_err(|_| Error {
            kind: ErrorKind::Invalid,
            code: "bbj-planning-authoring".into(),
        })?;
    let selected_svg: layered_source::Asset = serde_json::from_value(selected["svg"].clone())
        .map_err(|_| Error {
            kind: ErrorKind::Invalid,
            code: "bbj-planning-svg".into(),
        })?;
    let png: layered_source::Asset =
        serde_json::from_value(selected["png"].clone()).map_err(|_| Error {
            kind: ErrorKind::Invalid,
            code: "bbj-planning-png".into(),
        })?;
    if !(24..=16 * 1024 * 1024).contains(&png.bytes) {
        return fail(ErrorKind::Budget, "bbj-planning-png-bytes");
    }
    let selection_pin = Pin {
        path: "control/bbj-selected-book.json".into(),
        sha256: digest(&selection_raw),
        bytes: selection_raw.len() as u64,
    };
    capture.add(
        "selected-book.json",
        selection_raw,
        "application/json",
        Some(&selection_pin),
    )?;
    let png_path = readings
        .get(&png.file)
        .filter(|p| p.is_absolute())
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "bbj-planning-reading-binding".into(),
        })?;
    let pixels = read(png_path, png.bytes)?;
    reel_publication::layered::check_reading_png(&pixels).map_err(|_| Error {
        kind: ErrorKind::Integrity,
        code: "bbj-planning-reading-png".into(),
    })?;
    capture.add("reading.png", pixels, "image/png", Some(&png.pin()))?;
    let executable_raw = read(Path::new(&config.executable.path), config.executable.bytes)?;
    if digest(&executable_raw) != config.executable.sha256
        || executable_raw.len() as u64 != config.executable.bytes
    {
        return fail(ErrorKind::Integrity, "bbj-planning-runtime-pin");
    }
    let executable = capture.add(
        "engine.exe",
        executable_raw,
        "application/octet-stream",
        None,
    )?;
    let pub_asset = capture.add(
        "publication.json",
        publication_raw,
        "application/json",
        None,
    )?;
    page.output_bytes = 128 * 1024 * 1024;
    let request = layered::Request {
        schema: "reel.workflow-layered-comic.v1".into(),
        page,
        authoring: author.pin(),
        selection: selection_pin,
        sheet,
        selected_svg: reel_publication::comic_source::Object {
            sha256: selected_svg.sha256,
            bytes: selected_svg.bytes,
        },
        reading_png: png.pin(),
    };
    let req_asset = capture.add(
        "managed-request.json",
        json(&request)?,
        "application/json",
        None,
    )?;
    let input_id = format!("{}-intake", config.task_prefix);
    let compose_id = format!("{}-compose", config.task_prefix);
    let empty = upstream::Bindings {
        schema: "reel.workflow-upstream-empty.v1".into(),
        inputs: BTreeMap::new(),
        streaming: None,
    };
    let empty_raw = empty.canonical_bytes()?;
    let intake = Task { id: input_id.clone(), native_inputs: Some(config.native_inputs.clone()),
        upstream_inputs: Some(Artifact { logical_id: format!("workflow/{}/empty-inputs", config.task_prefix), sha256: digest(&empty_raw), bytes: empty_raw.len() as u64, media_type: "application/json".into() }),
        operation: Operation::ExternalSourceIntake, tool: None, language: Some(request.page.language.clone()),
        prompt: "Register these exact existing BBJ page sources as unselected candidates. No provider, creative approval, selection or release.".into(),
        inputs: vec![], depends_on: vec![], outputs: capture.artifacts.iter().map(|(n,a)| (n.clone(), OutputContract { media_type:a.media_type.clone(), max_bytes:a.bytes, required:true })).collect() };
    let base = Plan {
        schema: PLAN_SCHEMA.into(),
        project: config.project.clone(),
        source_commit: config.source_commit.clone(),
        task_snapshots: BTreeMap::new(),
        tasks: vec![intake],
    };
    let packet = make_packet(&base, &input_id, BTreeMap::new())?;
    let contracts: OutputContracts = [
        ("page.svg", "image/svg+xml", 128 * 1024 * 1024),
        ("page.png", "image/png", 16 * 1024 * 1024),
        (
            "editable.packet",
            "application/vnd.reel.editable-packet.v1",
            128 * 1024 * 1024,
        ),
    ]
    .into_iter()
    .map(|(n, m, b)| {
        (
            n.into(),
            OutputContract {
                media_type: m.into(),
                max_bytes: b,
                required: true,
            },
        )
    })
    .collect();
    let tool = ToolContract {
        schema: "reel.workflow-tool.v1".into(),
        capability: ToolCapability::LayeredComicCompose,
        executable: executable.clone(),
        auxiliary_executables: BTreeMap::new(),
        publication: pub_asset,
        edition_id: edition.id.clone(),
        request: req_asset,
        source_bindings: capture
            .sources
            .iter()
            .map(|(p, n)| (p.clone(), capture.artifacts[n].logical_id.clone()))
            .collect(),
        part: None,
    };
    let preview = Task {
        id: compose_id.clone(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::DeterministicTool,
        tool: Some(tool),
        language: Some(request.page.language.clone()),
        prompt: String::new(),
        inputs: capture.artifacts.values().cloned().collect(),
        depends_on: vec![],
        outputs: contracts.clone(),
    };
    let preview_plan = Plan {
        tasks: vec![preview],
        ..base.clone()
    };
    let preview_packet = make_packet(&preview_plan, &compose_id, BTreeMap::new())?;
    layered::bind(&preview_packet, &mut Memory(&capture))?;
    let step = expansion::TaskTemplate {
        id: compose_id,
        operation: Operation::DeterministicTool,
        language: Some(request.page.language.clone()),
        prompt: String::new(),
        depends_on: vec![input_id.clone()],
        inputs: capture
            .artifacts
            .keys()
            .map(|n| parent(&input_id, n))
            .collect(),
        outputs: contracts,
        tool: Some(expansion::Tool {
            capability: ToolCapability::LayeredComicCompose,
            executable,
            auxiliary_executables: BTreeMap::new(),
            publication: parent(&input_id, "publication.json"),
            edition_id: edition.id.clone(),
            request: parent(&input_id, "managed-request.json"),
            sources: capture
                .sources
                .iter()
                .map(|(p, n)| (p.clone(), parent(&input_id, n)))
                .collect(),
            part: None,
        }),
        streaming: Some(upstream::Streaming {
            schema: "reel.workflow-upstream-streaming.v1".into(),
            input_bytes: 256 * 1024 * 1024,
            input_object_bytes: 128 * 1024 * 1024,
        }),
    };
    let recipe = recipe::Recipe {
        schema: "reel.workflow-recipe.v2".into(),
        base: base.clone(),
        steps: vec![step],
    };
    recipe::check(&recipe)?;
    let proposed = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: input_id,
        task_key: packet.task_key.clone(),
        attempt_id: config.attempt_id.clone(),
        artifacts: capture.artifacts.clone(),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
        worker_receipt: None,
    };
    check_result(&packet, &proposed)?;
    let mut files = BTreeMap::new();
    files.insert("intent.json".into(), json(&serde_json::json!({"schema":"reel.workflow-bbj-page-capsule.v1","config":canonical_digest(config)?,"sheet":sheet,"page_id":request.page.page_id,"source_bytes":capture.bytes,"source_files":capture.incoming.len(),"native_admitted":false,"retained_owner_acknowledged":false,"selected":false}))?);
    files.insert("intake-plan.json".into(), json(&base)?);
    files.insert("intake-packet.json".into(), json(&packet)?);
    files.insert("upstream-empty.json".into(), empty_raw);
    files.insert(
        "parents-empty.json".into(),
        json(&serde_json::json!({"schema":"reel.workflow-native-parents.empty.v1","parents":{}}))?,
    );
    files.insert("recipe.json".into(), json(&recipe)?);
    files.insert("proposed-return.json".into(), json(&proposed)?);
    files.insert("local-preview-packet.json".into(), json(&preview_packet)?);
    let paths: BTreeMap<_, _> = capture
        .artifacts
        .iter()
        .map(|(n, a)| (a.logical_id.clone(), output.join("incoming").join(n)))
        .collect();
    files.insert("local-inputs.json".into(), json(&paths)?);
    let names: BTreeMap<_, _> = capture
        .artifacts
        .keys()
        .map(|n| (n.clone(), n.clone()))
        .collect();
    files.insert("returns.json".into(), json(&serde_json::json!({"schema":"reel.workflow-returns.v1","root":output.join("incoming"),"files":names}))?);
    for (name, bytes) in capture.incoming {
        files.insert(format!("incoming/{name}"), bytes);
    }
    Ok(files)
}
