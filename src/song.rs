use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use sha2::{Digest, Sha256};
use tempfile::tempdir_in;

pub const SCHEMA: &str = "reel.song-generation.v0.1";
const REQUEST_SCHEMA: &str = "reel.song-engine-request.v0.1";
const RECEIPT_SCHEMA: &str = "reel.song-engine-plan-receipt.v0.1";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SongManifest {
    pub schema: String,
    pub song_id: String,
    pub title: String,
    pub source: Source,
    pub composition: Composition,
    pub engine: Engine,
    pub outputs: Outputs,
    pub permissions: Permissions,
    pub review: Review,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub language: String,
    pub lyrics: Lyrics,
    #[serde(default)]
    pub source_ranges: Vec<SourceRange>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Lyrics {
    pub path: PathBuf,
    pub sha256: String,
    pub exact_text: bool,
    pub allow_repetition: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRange {
    pub id: String,
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub duration_seconds: f64,
    pub meter: String,
    pub tempo_bpm: f64,
    pub key: String,
    pub prompt: String,
    #[serde(default)]
    pub negative_prompt: String,
    pub named_artist_imitation: bool,
    #[serde(default)]
    pub listening_references: Vec<String>,
    #[serde(default)]
    pub references: Vec<GenerationReference>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationReference {
    pub id: String,
    pub kind: String,
    pub path: PathBuf,
    pub sha256: String,
    pub egress: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Engine {
    pub kind: String,
    pub version: String,
    pub model_id: String,
    pub model_revision: String,
    pub model_license: String,
    pub local_only: bool,
    pub executable: String,
    pub working_directory: PathBuf,
    pub network_policy: String,
    pub seed: u64,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Outputs {
    pub requested: Vec<RequestedOutput>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedOutput {
    pub id: String,
    pub kind: String,
    pub format: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    pub lyrics_scope: String,
    pub voice_identity: String,
    pub voice_consent: String,
    #[serde(default)]
    pub voice_consent_evidence: Vec<String>,
    pub third_party_upload: bool,
    pub public_release: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub status: String,
    pub required_roles: Vec<String>,
    #[serde(default)]
    pub principal_findings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub song_id: String,
    pub lyrics_sha256: String,
    pub lyric_bytes: usize,
    pub lyric_lines: usize,
    pub duration_seconds: f64,
    pub engine: String,
    pub local_only: bool,
    pub human_listening_required: bool,
    pub public_release_declared: bool,
    pub verified: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct EngineRequest {
    schema: String,
    song_id: String,
    title: String,
    language: String,
    lyrics: String,
    exact_text: bool,
    allow_repetition: bool,
    composition: EngineComposition,
    engine: Engine,
    outputs: Outputs,
    human_listening_required: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct EngineComposition {
    duration_seconds: f64,
    meter: String,
    tempo_bpm: f64,
    key: String,
    prompt: String,
    negative_prompt: String,
    references: Vec<GenerationReference>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Receipt {
    schema: String,
    manifest_sha256: String,
    lyrics_sha256: String,
    request_sha256: String,
    reference_sha256: BTreeMap<String, String>,
    song_id: String,
    lyric_bytes: usize,
    lyric_lines: usize,
    engine: String,
    engine_version: String,
    model_id: String,
    model_revision: String,
    seed: u64,
    requested_outputs: Vec<String>,
    local_only: bool,
    third_party_upload: bool,
    public_release: bool,
    voice_consent: String,
    human_listening_required: bool,
    verified: bool,
}

#[derive(Debug, Serialize)]
pub struct PacketReport {
    pub schema: String,
    pub request: String,
    pub receipt: String,
    pub receipt_sha256: String,
    pub verified: bool,
}

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub schema: String,
    pub request_sha256: String,
    pub receipt_sha256: String,
    pub verified: bool,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub schema: String,
    pub engine: String,
    pub executable: String,
    pub executable_found: bool,
    pub working_directory_exists: bool,
    pub model_revision_pinned: bool,
    pub offline_after_install: bool,
    pub ready: bool,
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let (manifest, lyrics, lyrics_hash, _) = load_and_validate(path)?;
    Ok(ValidationReport {
        schema: SCHEMA.into(),
        song_id: manifest.song_id,
        lyrics_sha256: lyrics_hash,
        lyric_bytes: lyrics.len(),
        lyric_lines: lyrics.lines().count(),
        duration_seconds: manifest.composition.duration_seconds,
        engine: manifest.engine.kind,
        local_only: manifest.engine.local_only,
        human_listening_required: true,
        public_release_declared: manifest.permissions.public_release,
        verified: true,
    })
}

pub fn write_plan(manifest_path: &Path, output_dir: &Path) -> Result<PacketReport> {
    if output_dir.exists() {
        bail!(
            "song engine packet output already exists: {}",
            output_dir.display()
        );
    }
    let (manifest, lyrics, lyrics_hash, references) = load_and_validate(manifest_path)?;
    let manifest_hash = sha256_path(manifest_path)?;
    let request = EngineRequest {
        schema: REQUEST_SCHEMA.into(),
        song_id: manifest.song_id.clone(),
        title: manifest.title.clone(),
        language: manifest.source.language.clone(),
        lyrics,
        exact_text: manifest.source.lyrics.exact_text,
        allow_repetition: manifest.source.lyrics.allow_repetition,
        composition: EngineComposition {
            duration_seconds: manifest.composition.duration_seconds,
            meter: manifest.composition.meter.clone(),
            tempo_bpm: manifest.composition.tempo_bpm,
            key: manifest.composition.key.clone(),
            prompt: manifest.composition.prompt.clone(),
            negative_prompt: manifest.composition.negative_prompt.clone(),
            references: manifest.composition.references.clone(),
        },
        engine: manifest.engine.clone(),
        outputs: manifest.outputs.clone(),
        human_listening_required: true,
    };
    let request_bytes = serde_json::to_vec_pretty(&request)?;
    let request_hash = sha256_bytes(&request_bytes);
    let receipt = Receipt {
        schema: RECEIPT_SCHEMA.into(),
        manifest_sha256: manifest_hash,
        lyrics_sha256: lyrics_hash,
        request_sha256: request_hash,
        reference_sha256: references,
        song_id: manifest.song_id,
        lyric_bytes: request.lyrics.len(),
        lyric_lines: request.lyrics.lines().count(),
        engine: manifest.engine.kind,
        engine_version: manifest.engine.version,
        model_id: manifest.engine.model_id,
        model_revision: manifest.engine.model_revision,
        seed: manifest.engine.seed,
        requested_outputs: manifest
            .outputs
            .requested
            .iter()
            .map(|output| output.id.clone())
            .collect(),
        local_only: manifest.engine.local_only,
        third_party_upload: manifest.permissions.third_party_upload,
        public_release: manifest.permissions.public_release,
        voice_consent: manifest.permissions.voice_consent,
        human_listening_required: true,
        verified: true,
    };
    let receipt_bytes = serde_json::to_vec_pretty(&receipt)?;
    let readme = "# REEL local song-engine packet\n\n`request.json` is private and may contain exact lyrics and local paths. `receipt.json` is path-free and lyric-free. Generation is not approval: human listening and a separate release decision remain required.\n";
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = tempdir_in(parent)?;
    fs::write(temp.path().join("request.json"), &request_bytes)?;
    fs::write(temp.path().join("receipt.json"), &receipt_bytes)?;
    fs::write(temp.path().join("README.md"), readme)?;
    fs::rename(temp.keep(), output_dir)?;
    check(output_dir, manifest_path)?;
    Ok(PacketReport {
        schema: "reel.song-engine-plan-packet.v0.1".into(),
        request: output_dir.join("request.json").display().to_string(),
        receipt: output_dir.join("receipt.json").display().to_string(),
        receipt_sha256: sha256_bytes(&receipt_bytes),
        verified: true,
    })
}

pub fn check(packet_dir: &Path, manifest_path: &Path) -> Result<CheckReport> {
    let request_bytes = fs::read(packet_dir.join("request.json"))?;
    let receipt_bytes = fs::read(packet_dir.join("receipt.json"))?;
    let request: EngineRequest = serde_json::from_slice(&request_bytes)?;
    let receipt: Receipt = serde_json::from_slice(&receipt_bytes)?;
    let (manifest, lyrics, lyrics_hash, references) = load_and_validate(manifest_path)?;
    if receipt.schema != RECEIPT_SCHEMA || !receipt.verified || !receipt.human_listening_required {
        bail!("invalid song engine receipt");
    }
    if request.schema != REQUEST_SCHEMA || !request.human_listening_required {
        bail!("invalid song engine request");
    }
    let request_hash = sha256_bytes(&request_bytes);
    if receipt.manifest_sha256 != sha256_path(manifest_path)?
        || receipt.lyrics_sha256 != lyrics_hash
        || receipt.request_sha256 != request_hash
        || receipt.reference_sha256 != references
        || request.song_id != manifest.song_id
        || request.lyrics != lyrics
        || request.engine.kind != manifest.engine.kind
        || request.engine.version != manifest.engine.version
        || request.engine.model_revision != manifest.engine.model_revision
        || request.engine.seed != manifest.engine.seed
        || receipt.local_only != manifest.engine.local_only
        || receipt.third_party_upload != manifest.permissions.third_party_upload
        || receipt.public_release != manifest.permissions.public_release
        || receipt.voice_consent != manifest.permissions.voice_consent
    {
        bail!("song engine plan and receipt do not match current inputs");
    }
    Ok(CheckReport {
        schema: "reel.song-engine-plan-check.v0.1".into(),
        request_sha256: request_hash,
        receipt_sha256: sha256_bytes(&receipt_bytes),
        verified: true,
    })
}

pub fn doctor(path: &Path) -> Result<DoctorReport> {
    let (manifest, _, _, _) = load_and_validate(path)?;
    let executable_found = find_executable(&manifest.engine.executable);
    let working_directory_exists = resolve(path, &manifest.engine.working_directory).is_dir();
    let model_revision_pinned = !manifest.engine.model_revision.trim().is_empty()
        && manifest.engine.model_revision != "latest";
    let offline_after_install = manifest.engine.network_policy == "offline-after-install";
    Ok(DoctorReport {
        schema: "reel.song-engine-doctor.v0.1".into(),
        engine: manifest.engine.kind,
        executable: manifest.engine.executable,
        executable_found,
        working_directory_exists,
        model_revision_pinned,
        offline_after_install,
        ready: executable_found
            && working_directory_exists
            && model_revision_pinned
            && offline_after_install,
    })
}

fn load_and_validate(
    path: &Path,
) -> Result<(SongManifest, String, String, BTreeMap<String, String>)> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let manifest: SongManifest = serde_yaml::from_slice(&bytes)
        .with_context(|| format!("song manifest is not valid YAML: {}", path.display()))?;
    if manifest.schema != SCHEMA {
        bail!("song schema must be {SCHEMA}");
    }
    nonempty("song_id", &manifest.song_id)?;
    nonempty("title", &manifest.title)?;
    nonempty("source.language", &manifest.source.language)?;
    if !manifest.source.lyrics.exact_text {
        bail!("source.lyrics.exact_text must be true");
    }
    let lyrics_path = resolve(path, &manifest.source.lyrics.path);
    let lyrics = fs::read_to_string(&lyrics_path)
        .with_context(|| format!("failed to read lyrics: {}", lyrics_path.display()))?;
    if lyrics.trim().is_empty() {
        bail!("lyrics must not be empty");
    }
    let lyrics_hash = sha256_path(&lyrics_path)?;
    if lyrics_hash != manifest.source.lyrics.sha256.to_lowercase() {
        bail!("lyrics sha256 does not match source.lyrics.sha256");
    }
    if !(10.0..=600.0).contains(&manifest.composition.duration_seconds) {
        bail!("composition.duration_seconds must be between 10 and 600");
    }
    if !(30.0..=240.0).contains(&manifest.composition.tempo_bpm) {
        bail!("composition.tempo_bpm must be between 30 and 240");
    }
    for (field, value) in [
        ("composition.meter", &manifest.composition.meter),
        ("composition.key", &manifest.composition.key),
        ("composition.prompt", &manifest.composition.prompt),
        ("engine.kind", &manifest.engine.kind),
        ("engine.version", &manifest.engine.version),
        ("engine.model_id", &manifest.engine.model_id),
        ("engine.model_license", &manifest.engine.model_license),
        ("engine.executable", &manifest.engine.executable),
        ("engine.network_policy", &manifest.engine.network_policy),
        (
            "permissions.lyrics_scope",
            &manifest.permissions.lyrics_scope,
        ),
        (
            "permissions.voice_identity",
            &manifest.permissions.voice_identity,
        ),
        (
            "permissions.voice_consent",
            &manifest.permissions.voice_consent,
        ),
        ("review.status", &manifest.review.status),
    ] {
        nonempty(field, value)?;
    }
    if manifest.composition.named_artist_imitation {
        bail!("composition.named_artist_imitation must be false");
    }
    if manifest.engine.kind != "ace-step-local" {
        bail!("only the ace-step-local engine adapter is supported in v0.2.25");
    }
    if !manifest.engine.local_only {
        bail!("engine.local_only must be true for ace-step-local");
    }
    if manifest.permissions.third_party_upload {
        bail!("permissions.third_party_upload must be false for local-only generation");
    }
    if manifest.permissions.public_release {
        bail!("permissions.public_release must remain false; release requires a separate decision");
    }
    if manifest.permissions.voice_identity == "original-unassigned" {
        if manifest.permissions.voice_consent != "not-applicable"
            || !manifest.permissions.voice_consent_evidence.is_empty()
        {
            bail!(
                "an original-unassigned voice requires not-applicable consent and no consent evidence"
            );
        }
    } else if manifest.permissions.voice_consent != "recorded"
        || manifest.permissions.voice_consent_evidence.is_empty()
    {
        bail!("an assigned voice identity requires recorded consent evidence");
    }
    if manifest.engine.network_policy != "offline-after-install" {
        bail!("engine.network_policy must be offline-after-install");
    }
    if manifest.engine.model_revision.trim().is_empty()
        || manifest.engine.model_revision == "latest"
    {
        bail!("engine.model_revision must be pinned and must not be latest");
    }
    validate_ranges(&manifest.source.source_ranges)?;
    unique_nonempty(
        "review.required_roles",
        manifest.review.required_roles.iter().map(String::as_str),
    )?;
    if manifest.review.required_roles.is_empty() {
        bail!("review.required_roles must not be empty");
    }
    if manifest.outputs.requested.is_empty() {
        bail!("outputs.requested must not be empty");
    }
    unique_nonempty(
        "outputs.requested ids",
        manifest
            .outputs
            .requested
            .iter()
            .map(|item| item.id.as_str()),
    )?;
    let allowed_kinds = ["full-mix", "vocal", "instrumental", "stems"];
    let allowed_formats = ["wav", "flac"];
    for output in &manifest.outputs.requested {
        if !allowed_kinds.contains(&output.kind.as_str()) {
            bail!("unsupported requested output kind: {}", output.kind);
        }
        if !allowed_formats.contains(&output.format.as_str()) {
            bail!("unsupported requested output format: {}", output.format);
        }
    }
    let mut references = BTreeMap::new();
    for reference in &manifest.composition.references {
        nonempty("composition.references[].id", &reference.id)?;
        nonempty("composition.references[].kind", &reference.kind)?;
        if reference.egress != "local-only" {
            bail!(
                "generation reference {} must have local-only egress",
                reference.id
            );
        }
        let reference_path = resolve(path, &reference.path);
        let hash = sha256_path(&reference_path)?;
        if hash != reference.sha256.to_lowercase() {
            bail!(
                "generation reference {} sha256 does not match",
                reference.id
            );
        }
        if references.insert(reference.id.clone(), hash).is_some() {
            bail!("duplicate generation reference id: {}", reference.id);
        }
    }
    Ok((manifest, lyrics, lyrics_hash, references))
}

fn validate_ranges(ranges: &[SourceRange]) -> Result<()> {
    let mut prior_end = 0;
    let mut ids = BTreeSet::new();
    for range in ranges {
        nonempty("source.source_ranges[].id", &range.id)?;
        if !ids.insert(&range.id) {
            bail!("duplicate source range id: {}", range.id);
        }
        if range.start == 0 || range.end < range.start {
            bail!(
                "source range {} must have positive ordered bounds",
                range.id
            );
        }
        if range.start <= prior_end {
            bail!("source ranges must be ordered and non-overlapping");
        }
        prior_end = range.end;
    }
    Ok(())
}

fn nonempty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

fn unique_nonempty<'a>(field: &str, values: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        nonempty(field, value)?;
        if !seen.insert(value) {
            bail!("{field} must be unique; duplicate: {value}");
        }
    }
    Ok(())
}

fn resolve(manifest_path: &Path, value: &Path) -> PathBuf {
    if value.is_absolute() {
        value.to_path_buf()
    } else {
        manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(value)
    }
}

fn find_executable(executable: &str) -> bool {
    let candidate = Path::new(executable);
    if candidate.components().count() > 1 {
        return candidate.is_file();
    }
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|directory| {
            let plain = directory.join(executable);
            if plain.is_file() {
                return true;
            }
            if cfg!(windows) {
                ["exe", "cmd", "bat"].iter().any(|extension| {
                    directory
                        .join(format!("{executable}.{extension}"))
                        .is_file()
                })
            } else {
                false
            }
        })
    })
}

fn sha256_path(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
