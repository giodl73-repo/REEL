//! Read-only inspection of explicitly pinned, committed scene-authoring controls.
//! No media reads, model calls, rendering, source edits, selection or adoption.
use crate::*;
use reel_assembly::scene_authoring as scene;
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

pub const REQUEST_SCHEMA: &str = "reel.workflow-film-inspection.v1";
pub const SIDECAR_SCHEMA: &str = "reel.workflow-film-sidecar.v1";
const MAX_DOCUMENTS: usize = 256;
const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    /// A repository-relative committed ordinary JSON/text file, never an annex leaf.
    pub path: String,
    pub artifact: Artifact,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CueEvidence {
    pub text: Control,
    pub words: Control,
    pub selected_alignment: Option<Control>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CelIdentity {
    pub story_id: String,
    /// Project-owned ID, independent of drawing filename/version/hash.
    pub cel_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    /// Exact previously recorded subject; an older drawing remains an older subject.
    pub subject: Artifact,
    pub evidence: Control,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Direction {
    pub desired_seconds_per_cel: [u32; 2],
    pub visual_school: String,
    pub performance_style: String,
    pub animation_technique: String,
    pub renderer: Artifact,
    /// Exact HTML/CSS/SVG or other renderer settings; not executable support proof.
    pub settings: Control,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub project: String,
    pub source_commit: String,
    pub episode: Control,
    pub scene: Control,
    /// Season, episode and scene, respectively.
    pub scopes: [Control; 3],
    pub catalog: Control,
    pub policy: Control,
    /// Explicit identities per language/event; never inferred from a drawing hash.
    pub identities: BTreeMap<String, BTreeMap<String, CelIdentity>>,
    /// Complete requested language/cue evidence. No cross-language clock fallback.
    pub cues: BTreeMap<String, BTreeMap<String, CueEvidence>>,
    pub direction: Direction,
    pub approvals: Vec<Approval>,
}
#[derive(Debug, Serialize)]
pub struct InspectedCue {
    pub cue: scene::Cue,
    pub selected_take: scene::AssetRef,
    pub alignment: scene::NativeAlignment,
}
#[derive(Debug, Serialize)]
pub struct InspectedEvent {
    pub semantic_id: String,
    pub identity: CelIdentity,
    pub event: scene::Event,
    pub start_sample: u64,
    pub selected_picture: scene::AssetRef,
}
#[derive(Debug, Serialize)]
pub struct Lane {
    pub cues: Vec<InspectedCue>,
    pub events: Vec<InspectedEvent>,
    pub selected_inputs: BTreeMap<String, scene::AssetRef>,
}
#[derive(Debug, Serialize)]
pub struct Sidecar {
    pub schema: String,
    pub request: Request,
    pub episode: scene::Episode,
    pub authored_scene: scene::Scene,
    pub lanes: BTreeMap<String, Lane>,
    pub metadata_verified: bool,
    pub media_verified: bool,
    pub approvals_verified: bool,
    pub renderer_qualified: bool,
    pub production_adopted: bool,
}

/// Implementations must return only the exact named metadata from the declared
/// snapshot, within maximum. The CLI supplies GitReader; tests use synthetic Git.
pub trait Reader {
    fn read(&mut self, commit: &str, path: &str, maximum: u64) -> Result<Vec<u8>>;
}
pub struct GitReader<'a> {
    pub repository: &'a Path,
}
impl GitReader<'_> {
    fn command(&self) -> Command {
        let mut command = Command::new("git");
        command
            .args([
                "--no-replace-objects",
                "--literal-pathspecs",
                "-c",
                "protocol.allow=never",
                "-C",
            ])
            .arg(self.repository)
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        command
    }
}
impl Reader for GitReader<'_> {
    fn read(&mut self, commit: &str, path: &str, maximum: u64) -> Result<Vec<u8>> {
        if !hex(commit, &[40, 64]) || !portable_source(path) || maximum > MAX_CONTROL_BYTES as u64 {
            return fail(ErrorKind::Invalid, "film-inspect-git-address");
        }
        // Fail closed on promisor configurations, including older Git versions
        // which may not honor GIT_NO_LAZY_FETCH. Inspection must not hydrate.
        let partial = self
            .command()
            .args([
                "config",
                "--get-regexp",
                "^(extensions\\.partialclone|remote\\..*\\.promisor)$",
            ])
            .stdout(Stdio::null())
            .status()
            .map_err(|_| Error {
                kind: ErrorKind::Missing,
                code: "film-inspect-git-unavailable".into(),
            })?;
        if partial.code() != Some(1) {
            return fail(
                ErrorKind::Unsupported,
                "film-inspect-complete-local-repository-required",
            );
        }
        let kind = self
            .command()
            .args(["cat-file", "-t", commit])
            .stderr(Stdio::null())
            .output()
            .map_err(|_| Error {
                kind: ErrorKind::Missing,
                code: "film-inspect-git-unavailable".into(),
            })?;
        if !kind.status.success() || kind.stdout != b"commit\n" {
            return fail(ErrorKind::Invalid, "film-inspect-exact-commit-required");
        }
        let entry = self
            .command()
            .args(["ls-tree", "-z", commit, "--", path])
            .output()
            .map_err(|_| Error {
                kind: ErrorKind::Missing,
                code: "film-inspect-git-unavailable".into(),
            })?;
        if !entry.status.success()
            || !(entry.stdout.starts_with(b"100644 blob ")
                || entry.stdout.starts_with(b"100755 blob "))
            || !entry.stdout.ends_with(format!("\t{path}\0").as_bytes())
        {
            return fail(
                ErrorKind::Missing,
                "film-inspect-ordinary-committed-control-required",
            );
        }
        let mut child = self
            .command()
            .args(["cat-file", "blob", &format!("{commit}:{path}")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Error {
                kind: ErrorKind::Missing,
                code: "film-inspect-git-unavailable".into(),
            })?;
        let mut bytes = Vec::new();
        let read = child
            .stdout
            .take()
            .ok_or_else(|| Error {
                kind: ErrorKind::NeedsRecovery,
                code: "film-inspect-git-pipe".into(),
            })?
            .take(maximum + 1)
            .read_to_end(&mut bytes);
        if read.is_err() || bytes.len() as u64 > maximum {
            let _ = child.kill();
            let _ = child.wait();
            return fail(ErrorKind::Budget, "film-inspect-control-read-bound");
        }
        if !child.wait().is_ok_and(|s| s.success()) {
            return fail(ErrorKind::Missing, "film-inspect-committed-control-missing");
        }
        Ok(bytes)
    }
}
fn semantic<T, E>(result: std::result::Result<T, E>) -> Result<T> {
    result.map_err(|_| Error {
        kind: ErrorKind::Integrity,
        code: "film-inspect-source-contract".into(),
    })
}
fn controls(request: &Request) -> Vec<&Control> {
    let mut result = vec![
        &request.episode,
        &request.scene,
        &request.catalog,
        &request.policy,
        &request.direction.settings,
    ];
    result.extend(&request.scopes);
    for cues in request.cues.values() {
        for cue in cues.values() {
            result.extend([&cue.text, &cue.words]);
            result.extend(cue.selected_alignment.as_ref());
        }
    }
    for approval in &request.approvals {
        result.push(&approval.evidence);
    }
    result
}
/// Inspect one complete scene and its requested languages. All source controls
/// are hash-checked before semantic inspection; no selected media is opened.
pub fn inspect(request: &Request, reader: &mut impl Reader) -> Result<Sidecar> {
    canonical_digest(request)?;
    let direction = &request.direction;
    if request.schema != REQUEST_SCHEMA
        || !token(&request.project)
        || !hex(&request.source_commit, &[40, 64])
        || request.source_commit.bytes().all(|b| b == b'0')
        || request.cues.is_empty()
        || request.cues.len() > 32
        || request.approvals.len() > 128
        || direction
            .desired_seconds_per_cel
            .iter()
            .any(|n| !(1..=1_000_000).contains(n))
        || [
            &direction.visual_school,
            &direction.performance_style,
            &direction.animation_technique,
        ]
        .iter()
        .any(|s| s.trim().is_empty() || s.len() > 256 || s.chars().any(char::is_control))
    {
        return fail(ErrorKind::Invalid, "film-inspect-request");
    }
    artifact(&direction.renderer)?;
    for approval in &request.approvals {
        artifact(&approval.subject)?;
    }
    let mut declared = BTreeMap::new();
    let mut identities = BTreeMap::new();
    let mut total = 0u64;
    for control in controls(request) {
        artifact(&control.artifact)?;
        if !portable_source(&control.path)
            || control.artifact.bytes > MAX_CONTROL_BYTES as u64
            || !matches!(
                control.artifact.media_type.as_str(),
                "application/json" | "text/plain"
            )
        {
            return fail(ErrorKind::Invalid, "film-inspect-control");
        }
        if let Some(prior) = declared.insert(&control.path, &control.artifact) {
            if prior != &control.artifact {
                return fail(ErrorKind::Conflict, "film-inspect-path-alias");
            }
        } else {
            total += control.artifact.bytes;
        }
        if identities
            .insert(&control.artifact.logical_id, &control.artifact)
            .is_some_and(|prior| prior != &control.artifact)
        {
            return fail(ErrorKind::Conflict, "film-inspect-identity-alias");
        }
    }
    if declared.len() > MAX_DOCUMENTS || total > MAX_TOTAL_BYTES {
        return fail(ErrorKind::Budget, "film-inspect-control-closure");
    }
    let mut raw = BTreeMap::new();
    for (path, a) in declared {
        let bytes = reader.read(&request.source_commit, path, a.bytes)?;
        if bytes.len() as u64 != a.bytes || digest(&bytes) != a.sha256 {
            return fail(ErrorKind::Integrity, "film-inspect-control-content");
        }
        if a.media_type == "application/json" {
            decode::<serde_json::Value>(&bytes)?;
        }
        raw.insert(path, bytes);
    }
    fn json<T: serde::de::DeserializeOwned>(
        raw: &BTreeMap<&String, Vec<u8>>,
        c: &Control,
    ) -> Result<T> {
        if c.artifact.media_type != "application/json" {
            return fail(ErrorKind::Invalid, "film-inspect-json-type");
        }
        decode(&raw[&c.path])
    }
    let authored: scene::Scene = json(&raw, &request.scene)?;
    if authored.schema != scene::SCENE_SCHEMA_V2 {
        return fail(ErrorKind::Unsupported, "film-inspect-v2-scene-required");
    }
    let episode: scene::Episode = json(&raw, &request.episode)?;
    let catalog: scene::TemplateCatalog = json(&raw, &request.catalog)?;
    let policy: scene::ScenePolicy = json(&raw, &request.policy)?;
    let scopes: Vec<scene::ScopedBindings> = request
        .scopes
        .iter()
        .map(|c| json(&raw, c))
        .collect::<Result<_>>()?;
    let scope_refs: Vec<_> = scopes.iter().collect();
    let materialized = semantic(scene::materialize_scene(&authored))?;
    if request.cues.keys().ne(request.identities.keys())
        || request
            .cues
            .keys()
            .any(|lang| !token(lang) || !materialized.languages.contains_key(lang))
    {
        return fail(ErrorKind::Conflict, "film-inspect-language-set");
    }
    let mut lanes = BTreeMap::new();
    let mut cel_stories = BTreeMap::new();
    let mut semantic_cels = BTreeMap::new();
    for (language, cue_inputs) in &request.cues {
        let resolved = semantic(scene::resolve_scene_for_language(
            &catalog,
            &episode,
            &authored,
            &policy,
            scene::SceneBindingScopes {
                season: &scopes[0],
                episode: &scopes[1],
                scene: &scopes[2],
            },
            language,
        ))?;
        let source_lane = &materialized.languages[language];
        if cue_inputs.keys().collect::<BTreeSet<_>>()
            != source_lane.cues.iter().map(|c| &c.cue_id).collect()
            || request.identities[language].keys().collect::<BTreeSet<_>>()
                != source_lane.events.iter().map(|e| &e.event_id).collect()
        {
            return fail(ErrorKind::Conflict, "film-inspect-cue-or-event-set");
        }
        let mut cues = Vec::new();
        for cue in &source_lane.cues {
            if !cue.take_segments.is_empty() {
                return fail(ErrorKind::Unsupported, "film-inspect-segmented-takes");
            }
            let take_key = cue.take_binding.as_ref().ok_or_else(|| Error {
                kind: ErrorKind::Missing,
                code: "film-inspect-selected-take".into(),
            })?;
            let take = semantic(scene::resolve_selected_binding(take_key, &scope_refs))?;
            let evidence = &cue_inputs[&cue.cue_id];
            if evidence.text.artifact.media_type != "text/plain"
                || evidence.text.artifact.sha256 != cue.exact_text_sha256
            {
                return fail(ErrorKind::Conflict, "film-inspect-exact-text");
            }
            let text = std::str::from_utf8(&raw[&evidence.text.path]).map_err(|_| Error {
                kind: ErrorKind::Invalid,
                code: "film-inspect-text-utf8".into(),
            })?;
            let words: scene::WordTimingEvidence = json(&raw, &evidence.words)?;
            if words.language != *language
                || words.cue_id != cue.cue_id
                || words.selected_take_sha256 != take.sha256
            {
                return fail(ErrorKind::Conflict, "film-inspect-language-take-clock");
            }
            semantic(scene::verify_word_text(&words, text))?;
            let triggers = semantic(scene::authored_trigger_text(&authored, &words, text))?;
            let alignment = semantic(scene::resolve_text_triggers(&words, &triggers))?;
            match (&cue.phrase_alignment_binding, &evidence.selected_alignment) {
                (None, None) => (),
                (Some(key), Some(control)) => {
                    let selected = semantic(scene::resolve_selected_binding(key, &scope_refs))?;
                    if selected.logical_id != control.artifact.logical_id
                        || selected.sha256 != control.artifact.sha256
                        || selected.bytes != control.artifact.bytes
                    {
                        return fail(ErrorKind::Conflict, "film-inspect-selected-alignment-pin");
                    }
                    let existing: scene::NativeAlignment = json(&raw, control)?;
                    if canonical_digest(&existing)? != canonical_digest(&alignment)? {
                        return fail(ErrorKind::Conflict, "film-inspect-selected-alignment-clock");
                    }
                }
                _ => {
                    return fail(
                        ErrorKind::Missing,
                        "film-inspect-selected-alignment-required",
                    );
                }
            }
            cues.push(InspectedCue {
                cue: cue.clone(),
                selected_take: take.clone(),
                alignment,
            });
        }
        let mut events = Vec::new();
        for event in &source_lane.events {
            let identity = &request.identities[language][&event.event_id];
            let semantic_id = &authored
                .language_event_bindings
                .get(language)
                .and_then(|bindings| {
                    bindings
                        .iter()
                        .find(|binding| binding.event_id == event.event_id)
                })
                .ok_or_else(|| Error {
                    kind: ErrorKind::Missing,
                    code: "film-inspect-semantic-identity".into(),
                })?
                .semantic_id;
            if !token(&identity.story_id)
                || !token(&identity.cel_id)
                || cel_stories
                    .insert(&identity.cel_id, &identity.story_id)
                    .is_some_and(|prior| prior != &identity.story_id)
            {
                return fail(ErrorKind::Conflict, "film-inspect-stable-cel-identity");
            }
            if semantic_cels
                .insert(semantic_id, (&identity.story_id, &identity.cel_id))
                .is_some_and(|prior| prior != (&identity.story_id, &identity.cel_id))
            {
                return fail(
                    ErrorKind::Conflict,
                    "film-inspect-cross-language-cel-identity",
                );
            }
            let picture_key = event.picture_binding.as_ref().ok_or_else(|| Error {
                kind: ErrorKind::Missing,
                code: "film-inspect-selected-picture".into(),
            })?;
            let picture = semantic(scene::resolve_selected_binding(picture_key, &scope_refs))?;
            let alignment = &cues
                .iter()
                .find(|c| c.cue.cue_id == event.cue_id)
                .ok_or_else(|| Error {
                    kind: ErrorKind::Missing,
                    code: "film-inspect-event-cue".into(),
                })?
                .alignment;
            let start_sample = *alignment
                .semantic_markers
                .get(&event.semantic_trigger_id)
                .ok_or_else(|| Error {
                    kind: ErrorKind::Missing,
                    code: "film-inspect-event-marker".into(),
                })?;
            events.push(InspectedEvent {
                semantic_id: semantic_id.clone(),
                identity: identity.clone(),
                event: event.clone(),
                start_sample,
                selected_picture: picture.clone(),
            });
        }
        lanes.insert(
            language.clone(),
            Lane {
                cues,
                events,
                selected_inputs: resolved
                    .inputs_for_language(language)
                    .ok_or_else(|| Error {
                        kind: ErrorKind::Missing,
                        code: "film-inspect-lane-inputs".into(),
                    })?
                    .clone(),
            },
        );
    }
    let sidecar = Sidecar {
        schema: SIDECAR_SCHEMA.into(),
        request: request.clone(),
        episode,
        authored_scene: authored,
        lanes,
        metadata_verified: true,
        media_verified: false,
        approvals_verified: false,
        renderer_qualified: false,
        production_adopted: false,
    };
    // Sidecars are bounded controls too; a larger episode must use scene slices.
    canonical_digest(&sidecar)?;
    Ok(sidecar)
}
/// Creates only a new private sidecar, after every inspection check succeeds.
pub fn write_new(sidecar: &Sidecar, output: &Path) -> Result<()> {
    let bytes = serde_json::to_vec(sidecar).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "film-inspect-sidecar-encoding".into(),
    })?;
    if bytes.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "film-inspect-sidecar-bytes");
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| Error {
            kind: match error.kind() {
                std::io::ErrorKind::AlreadyExists => ErrorKind::Conflict,
                std::io::ErrorKind::PermissionDenied => ErrorKind::Denied,
                std::io::ErrorKind::NotFound => ErrorKind::Missing,
                _ => ErrorKind::NeedsRecovery,
            },
            code: "film-inspect-new-output-required".into(),
        })?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Error {
            kind: ErrorKind::NeedsRecovery,
            code: "film-inspect-partial-sidecar-preserved".into(),
        })
}
