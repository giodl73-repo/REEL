//! Private reported findings -> exact-baseline issue drafts. No issue delivery,
//! authenticated viewing, repair execution, selection, or retained ownership.
use crate::*;
use reel_publication::film_review::{self as wire, Receipt, ReviewMap, Target};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub const REPORT_SCHEMA: &str = "reel.workflow-film-findings.v1";
pub const MAX_FINDINGS: usize = 32;
pub const MAX_EVIDENCE: usize = 64;
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Picture,
    Timing,
    Audio,
    Captions,
    Continuity,
    SourceText,
    Rights,
}
impl Category {
    fn label(self) -> &'static str {
        match self {
            Self::Picture => "picture",
            Self::Timing => "timing",
            Self::Audio => "audio",
            Self::Captions => "captions",
            Self::Continuity => "continuity",
            Self::SourceText => "source text",
            Self::Rights => "rights",
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Method {
    HumanReport,
    ModelReport,
    DeterministicCheck,
}
impl Method {
    fn label(self) -> &'static str {
        match self {
            Self::HumanReport => "human report",
            Self::ModelReport => "model report",
            Self::DeterministicCheck => "deterministic check",
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub id: String,
    #[serde(default)]
    pub target_id: String,
    pub frame: u64,
    pub category: Category,
    pub method: Method,
    pub observation: String,
    #[serde(default)]
    pub evidence: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_reference: Option<BoardReference>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BoardReference {
    pub project: String,
    pub source_commit: String,
    pub story_id: String,
    pub display_id: String,
    pub semantic_id: String,
    pub language: String,
    pub version: BoardVersion,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BoardVersion {
    pub designation: String,
    pub asset: Artifact,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema: String,
    pub review: Artifact,
    pub map: Artifact,
    pub receipt: Artifact,
    pub findings: Vec<Finding>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssueDraft {
    pub schema: String,
    pub key: String,
    pub finding_identity: String,
    pub finding_id: String,
    pub project: String,
    pub source_commit: String,
    pub language: String,
    pub review: Artifact,
    pub map: Artifact,
    pub receipt: Artifact,
    pub target: Target,
    pub frame: u64,
    pub category: Category,
    pub method: Method,
    pub observation: String,
    pub evidence: Vec<Artifact>,
    pub title: String,
    pub body: String,
    pub viewing_verified: bool,
    pub authority_verified: bool,
    pub native_retention_verified: bool,
    pub issue_submitted: bool,
    pub selection_applied: bool,
}
/// Native callers preserve the exact record-to-leaf binding and live protection
/// across this whole synchronous call. The local file reader proves only bytes.
pub trait Reader {
    fn read(&mut self, artifact: &Artifact, maximum: u64) -> Result<Vec<u8>>;
    fn verify(&mut self, artifact: &Artifact, maximum: u64) -> Result<()>;
}
fn metadata<T: serde::de::DeserializeOwned>(reader: &mut impl Reader, a: &Artifact) -> Result<T> {
    if a.media_type != "application/json" {
        return fail(ErrorKind::Invalid, "film-findings-metadata");
    }
    if a.bytes > wire::MAX_MAP_BYTES as u64 {
        return fail(ErrorKind::Budget, "film-findings-metadata-bytes");
    }
    let raw = reader.read(a, wire::MAX_MAP_BYTES as u64)?;
    if raw.len() as u64 != a.bytes || digest(&raw) != a.sha256 {
        return fail(ErrorKind::Integrity, "film-findings-control-pin");
    }
    serde_json::from_slice(&raw).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "film-findings-control-json".into(),
    })
}
fn descriptors(report: &Report) -> Result<BTreeMap<String, Artifact>> {
    if report.schema != REPORT_SCHEMA {
        return fail(ErrorKind::Invalid, "film-findings-report-schema");
    }
    if report.findings.is_empty() || report.findings.len() > MAX_FINDINGS {
        return fail(ErrorKind::Budget, "film-findings-report-shape");
    }
    let mut artifacts = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut evidence = BTreeSet::new();
    for a in [&report.review, &report.map, &report.receipt] {
        artifact(a)?;
        if artifacts.insert(a.logical_id.clone(), a.clone()).is_some() {
            return fail(ErrorKind::Conflict, "film-findings-context-alias");
        }
    }
    if report.review.media_type != "video/mp4" {
        return fail(ErrorKind::Invalid, "film-findings-review-type");
    }
    if report.review.bytes > 16 * 1024 * 1024 * 1024 {
        return fail(ErrorKind::Budget, "film-findings-review-movie");
    }
    for finding in &report.findings {
        if !token(&finding.id)
            || !ids.insert(&finding.id)
            || !(token(&finding.target_id)
                || (finding.target_id.is_empty() && finding.board_reference.is_some()))
            || finding.evidence.len() > 16
            || finding.observation.trim().is_empty()
            || finding.observation.len() > 4096
            || finding
                .observation
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return fail(ErrorKind::Invalid, "film-findings-observation");
        }
        if let Some(reference) = &finding.board_reference {
            artifact(&reference.version.asset)?;
        }
        for a in &finding.evidence {
            artifact(a)?;
            if a.bytes > 32 * 1024 * 1024 {
                return fail(ErrorKind::Budget, "film-findings-evidence");
            }
            if [
                &report.review.logical_id,
                &report.map.logical_id,
                &report.receipt.logical_id,
            ]
            .contains(&&a.logical_id)
            {
                return fail(ErrorKind::Conflict, "film-findings-evidence-context-alias");
            }
            if artifacts
                .insert(a.logical_id.clone(), a.clone())
                .is_some_and(|old| old != *a)
            {
                return fail(ErrorKind::Conflict, "film-findings-artifact-alias");
            }
            evidence.insert(&a.logical_id);
        }
    }
    if evidence.len() > MAX_EVIDENCE
        || serde_json::to_vec(report)
            .map_err(|_| Error {
                kind: ErrorKind::Invalid,
                code: "film-findings-report-json".into(),
            })?
            .len()
            > MAX_CONTROL_BYTES
    {
        return fail(ErrorKind::Budget, "film-findings-report-budget");
    }
    Ok(artifacts)
}
fn receipt_binding(report: &Report, map: &ReviewMap, receipt: &Receipt) -> Result<()> {
    let movie = receipt.outputs.get("review.mp4").ok_or_else(|| Error {
        kind: ErrorKind::Missing,
        code: "film-findings-movie-output".into(),
    })?;
    let map_output = receipt
        .outputs
        .get("review-map.json")
        .ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-findings-map-output".into(),
        })?;
    if receipt.schema != wire::RECEIPT_SCHEMA
        || receipt.request_sha256 != map.request_sha256
        || receipt.frame_count != map.frame_count
        || receipt.audio_packet_count == 0
        || !wire::sha(&receipt.source_clocks_sha256)
        || receipt.source_clocks_sha256 != receipt.output_clocks_sha256
        || movie.path != Path::new("review.mp4")
        || movie.sha256 != report.review.sha256
        || movie.bytes != report.review.bytes
        || map_output.path != Path::new("review-map.json")
        || map_output.sha256 != report.map.sha256
        || map_output.bytes != report.map.bytes
        || receipt.visual_reviewed
        || receipt.approvals_verified
        || receipt.native_retention_verified
        || receipt.publication != "not-authorized"
    {
        return fail(ErrorKind::Conflict, "film-findings-review-baseline");
    }
    let expected: BTreeSet<_> = [
        "review.mp4".to_string(),
        "review-map.json".into(),
        "font.ttf".into(),
        "filters.txt".into(),
    ]
    .into_iter()
    .chain((0..map.targets.len()).map(|i| format!("label-{i}.txt")))
    .collect();
    if receipt.outputs.keys().cloned().collect::<BTreeSet<_>>() != expected
        || receipt.outputs.iter().any(|(name, file)| {
            file.path != Path::new(name) || file.bytes == 0 || !wire::sha(&file.sha256)
        })
    {
        return fail(ErrorKind::Conflict, "film-findings-receipt-closure");
    }
    Ok(())
}
/// Bounded byte/metadata checking; a reported observation is not verified viewing.
pub fn draft(report: &Report, reader: &mut impl Reader) -> Result<Vec<IssueDraft>> {
    let artifacts = descriptors(report)?;
    let map: ReviewMap = metadata(reader, &report.map)?;
    wire::check_map(&map).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "film-findings-map-shape".into(),
    })?;
    let receipt: Receipt = metadata(reader, &report.receipt)?;
    receipt_binding(report, &map, &receipt)?;
    let mut resolved = Vec::new();
    for finding in &report.findings {
        let covers_frame = |target: &Target| {
            target.start_frame <= finding.frame && finding.frame < target.end_frame
        };
        let target = if finding.target_id.is_empty() {
            map.targets.iter().find(|target| covers_frame(target))
        } else {
            map.targets
                .iter()
                .find(|target| target.attachment_id == finding.target_id)
        }
        .ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-findings-target".into(),
        })?;
        if finding.frame < target.start_frame || finding.frame >= target.end_frame {
            return fail(ErrorKind::Conflict, "film-findings-target-frame");
        }
        if let Some(reference) = &finding.board_reference
            && (reference.project != map.project
                || reference.source_commit != map.source_commit
                || reference.language != map.language
                || reference.story_id != target.story_id
                || reference.display_id != target.cel_id
                || reference.semantic_id != target.semantic_id
                || reference.version.designation != "selected"
                || reference.version.asset.sha256 != target.selected_picture.sha256
                || reference.version.asset.bytes != target.selected_picture.bytes)
        {
            return fail(ErrorKind::Conflict, "film-findings-board-reference");
        }
        resolved.push((finding, target));
    }
    // Bad target/baseline declarations fail before reading a multi-GB movie.
    reader.verify(&report.review, 16 * 1024 * 1024 * 1024)?;
    for a in artifacts.values().filter(|a| {
        a.logical_id != report.review.logical_id
            && a.logical_id != report.map.logical_id
            && a.logical_id != report.receipt.logical_id
    }) {
        reader.verify(a, 32 * 1024 * 1024)?;
    }
    let mut drafts = Vec::new();
    for (finding, target) in resolved {
        let finding_identity = canonical_digest(&(
            &report.review,
            &report.map,
            &report.receipt,
            target,
            &finding.id,
        ))?;
        let key = canonical_digest(&(
            &report.review,
            &report.map,
            &report.receipt,
            target,
            finding,
        ))?;
        let title = format!("Review {}: {}", finding.category.label(), target.cel_id);
        let mut body = format!(
            "Reported {} finding\nReported method: {}\n\n{}\n\nProject: `{}`\nLanguage: `{}`\nStory: `{}`\nCel: `{}`\nSemantic event: `{}`\nAttachment: `{}`\nFrame: {} (target interval [{}..{}))\nSource commit: `{}`\n\nExact review baseline:\n\n- Movie: `{}` / SHA-256 `{}`\n- Map: `{}` / SHA-256 `{}`\n- Review receipt: `{}` / SHA-256 `{}`\n- Selected picture source SHA-256: `{}` ({} bytes)\n- Original job SHA-256: `{}`\n- Original delivery receipt SHA-256: `{}`\n\nViewing, observation accuracy, authority and native custody remain unverified by this draft. Repair, selection and publication require their own decisions.\n",
            finding.category.label(),
            finding.method.label(),
            finding
                .observation
                .lines()
                .map(|s| format!("> {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
            map.project,
            map.language,
            target.story_id,
            target.cel_id,
            target.semantic_id,
            target.attachment_id,
            finding.frame,
            target.start_frame,
            target.end_frame,
            map.source_commit,
            report.review.logical_id,
            report.review.sha256,
            report.map.logical_id,
            report.map.sha256,
            report.receipt.logical_id,
            report.receipt.sha256,
            target.selected_picture.sha256,
            target.selected_picture.bytes,
            map.job.sha256,
            map.delivery_receipt.sha256
        );
        if !finding.evidence.is_empty() {
            body.push_str("\nReported evidence:\n\n");
            for a in &finding.evidence {
                body.push_str(&format!("- `{}` / SHA-256 `{}`\n", a.logical_id, a.sha256));
            }
        }
        body.push_str(&format!(
            "\nDraft version: `{key}`\n\n<!-- reel-film-finding:{finding_identity} -->\n"
        ));
        drafts.push(IssueDraft {
            schema: "reel.workflow-film-issue-draft.v1".into(),
            key,
            finding_identity,
            finding_id: finding.id.clone(),
            project: map.project.clone(),
            source_commit: map.source_commit.clone(),
            language: map.language.clone(),
            review: report.review.clone(),
            map: report.map.clone(),
            receipt: report.receipt.clone(),
            target: target.clone(),
            frame: finding.frame,
            category: finding.category,
            method: finding.method,
            observation: finding.observation.clone(),
            evidence: finding.evidence.clone(),
            title,
            body,
            viewing_verified: false,
            authority_verified: false,
            native_retention_verified: false,
            issue_submitted: false,
            selection_applied: false,
        });
    }
    Ok(drafts)
}
fn io_error(e: std::io::Error) -> Error {
    Error {
        kind: if (cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33)))
            || e.kind() == std::io::ErrorKind::WouldBlock
        {
            ErrorKind::Busy
        } else {
            match e.kind() {
                std::io::ErrorKind::NotFound => ErrorKind::Missing,
                std::io::ErrorKind::AlreadyExists => ErrorKind::Conflict,
                std::io::ErrorKind::PermissionDenied => ErrorKind::Denied,
                std::io::ErrorKind::Unsupported => ErrorKind::Unsupported,
                _ => ErrorKind::NeedsRecovery,
            }
        },
        code: "film-findings-file-operation".into(),
    }
}
pub struct Files {
    pub bindings: manual::FileBindings,
    held: Vec<File>,
}
impl Files {
    pub fn new(bindings: manual::FileBindings) -> Self {
        Self {
            bindings,
            held: vec![],
        }
    }
    fn opened(&self, a: &Artifact, maximum: u64) -> Result<File> {
        if a.bytes > maximum {
            return fail(ErrorKind::Budget, "film-findings-file-budget");
        }
        let path = self.bindings.get(&a.logical_id).ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-findings-file-binding".into(),
        })?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1).custom_flags(0x00200000);
        }
        let file = options.open(path).map_err(io_error)?;
        let info = file.metadata().map_err(io_error)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if info.file_attributes() & 0x400 != 0 {
                return fail(ErrorKind::Denied, "film-findings-reparse-file");
            }
        }
        if !info.is_file() || info.len() != a.bytes {
            return fail(ErrorKind::Integrity, "film-findings-file-length");
        }
        Ok(file)
    }
}
impl Reader for Files {
    fn read(&mut self, a: &Artifact, maximum: u64) -> Result<Vec<u8>> {
        let mut file = self.opened(a, maximum)?;
        let mut raw = Vec::new();
        Read::by_ref(&mut file)
            .take(maximum + 1)
            .read_to_end(&mut raw)
            .map_err(io_error)?;
        if raw.len() as u64 != a.bytes || digest(&raw) != a.sha256 {
            return fail(ErrorKind::Integrity, "film-findings-file-content");
        }
        self.held.push(file);
        Ok(raw)
    }
    fn verify(&mut self, a: &Artifact, maximum: u64) -> Result<()> {
        let mut file = self.opened(a, maximum)?;
        let mut hash = Sha256::new();
        let mut n = 0u64;
        let mut buffer = [0; 65536];
        loop {
            let count = file.read(&mut buffer).map_err(io_error)?;
            if count == 0 {
                break;
            }
            n = n.checked_add(count as u64).ok_or_else(|| Error {
                kind: ErrorKind::Budget,
                code: "film-findings-file-growth".into(),
            })?;
            if n > a.bytes {
                return fail(ErrorKind::Integrity, "film-findings-file-growth");
            }
            hash.update(&buffer[..count]);
        }
        let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if n != a.bytes || digest != a.sha256 {
            return fail(ErrorKind::Integrity, "film-findings-file-content");
        }
        self.held.push(file);
        Ok(())
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRequest {
    pub schema: String,
    pub report: Report,
    pub files: manual::FileBindings,
}
/// Private local files only. Native publication and issue API calls are separate.
pub fn export(path: &Path, pin: &str, directory: &Path) -> Result<bool> {
    let mut raw = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(MAX_CONTROL_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(io_error)?;
    if raw.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "film-findings-export-request-bytes");
    }
    if digest(&raw) != pin {
        return fail(ErrorKind::Integrity, "film-findings-export-request-pin");
    }
    let request: ExportRequest = decode(&raw)?;
    if request.schema != "reel.workflow-film-findings-export.v1" {
        return fail(ErrorKind::Invalid, "film-findings-export-schema");
    }
    if request.files.keys().collect::<BTreeSet<_>>()
        != descriptors(&request.report)?
            .keys()
            .collect::<BTreeSet<_>>()
    {
        return fail(ErrorKind::Conflict, "film-findings-export-closure");
    }
    let mut files = Files::new(request.files);
    let drafts = draft(&request.report, &mut files)?;
    let mut outputs = BTreeMap::new();
    for draft in &drafts {
        outputs.insert(
            format!("{}.json", draft.key),
            serde_json::to_vec_pretty(draft).map_err(|_| Error {
                kind: ErrorKind::Invalid,
                code: "film-findings-draft-json".into(),
            })?,
        );
        outputs.insert(format!("{}.md", draft.key), draft.body.as_bytes().to_vec());
        outputs.insert(
            format!("{}-github.json", draft.key),
            serde_json::to_vec_pretty(&serde_json::json!({"title":draft.title,"body":draft.body}))
                .unwrap(),
        );
    }
    outputs.insert("index.json".into(),serde_json::to_vec_pretty(&serde_json::json!({"schema":"reel.workflow-film-issue-bundle.v1","request_sha256":pin,"keys":drafts.iter().map(|d|&d.key).collect::<Vec<_>>(),"issue_submitted":false,"native_retention_verified":false})).unwrap());
    if directory.exists() {
        let names = fs::read_dir(directory)
            .map_err(io_error)?
            .map(|e| {
                e.map(|e| e.file_name().to_string_lossy().into_owned())
                    .map_err(io_error)
            })
            .collect::<Result<BTreeSet<_>>>()?;
        if names != outputs.keys().cloned().collect() {
            return fail(ErrorKind::Conflict, "film-findings-existing-export");
        }
        for (name, expected) in &outputs {
            let mut actual = Vec::new();
            File::open(directory.join(name))
                .map_err(io_error)?
                .take(expected.len() as u64 + 1)
                .read_to_end(&mut actual)
                .map_err(io_error)?;
            if &actual != expected {
                return fail(ErrorKind::Conflict, "film-findings-existing-export");
            }
        }
        return Ok(true);
    }
    fs::create_dir(directory).map_err(io_error)?;
    for (name, raw) in outputs {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))
            .map_err(io_error)?;
        file.write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(io_error)?;
    }
    Ok(false)
}
